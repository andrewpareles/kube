// a new thread for running the event loop

use super::input_snapshot::{PlayInputState, getCameraPosition, getCameraTarget, getClampedCamera};
use super::mesh_camera_shapes::MeshCameraShapes;
use super::render_snapshot::FullWorldRenderSnapshot;
use super::snapshot_buffer::{SnapshotChanges, SnapshotInterpolationBuffer};
use super::sound_playback_manager::SoundPlaybackManager;
use super::thread_priority::raise_current_thread_priority;
use super::time_estimator::TimeEstimatorReader;
use deno_core::JsRuntime;
use game_types::{
    consts::{CAMERA_SIZE, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK, CLIENT_SIM_AND_SEND_SECS, MIN_CAMERA_ZOOM},
    data_types::{DataId, MeshData},
    object_types::{ObjectBeingAdded, ObjectId, ObjectInfoTrackedFields, Orientation3D},
};
use glam::Vec3;
use quinn::Connection;
use render_core::renderable::RenderObjectCache;
use serde::Deserialize;
use shared::game::client_world_delay::ClientWorldDelay;
use shared::game::data_fetcher::DataFetcherArc;
use shared::game::jsRuntime::{
    JsConsoleLogCallback, JsConsoleLogEntry, JsRuntimeCameraOrientation, JsRuntimeCameraZoom, JsRuntimeClientWorldDelay, JsRuntimeDesiredCameraZoom,
    JsRuntimeInitialization, JsRuntimeMouseRay, JsRuntimePlayerPings, ScriptManager, callJsFn, callRegisteredGuiHandler, initializeJsAndRunInitialScripts,
    initializeJsRuntime, runJsCallbacks, runPendingJS,
};
use shared::game::physics::SetPhysicsToggles3D;
use shared::game::world::{JsCallbackInputs, World};
use shared::protocol::messages::{
    ClientFunctionCallMessage, ClientInputMessage, ClientMessageUDP, ForeignCharacterDelta, ForeignPlayerDelta, InputNum, MessageNum, NetworkedPhysicsInfo3D,
    ObjectAddDelta, ObjectDelta, ObjectModifyDelta, ServerPlayerPingsMessage, ServerScriptRawCodeMessage, ServerSnapshotMessage,
};
use shared::protocol::quic_send_receive_helpers::sendDatagramMessage;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

pub fn computeEffectiveCameraZoom(
    head_position: Vec3,
    camera_target_object_id: Option<ObjectId>,
    camera_orientation: &Orientation3D,
    camera_zoom: f32,
    world: &World,
    mesh_camera_shapes: &mut MeshCameraShapes,
    get_mesh_data: &dyn Fn(DataId) -> Option<Arc<MeshData>>,
    est_dely_server_sim_ms: f64,
    live_client_sim_ms: f64,
) -> f32 {
    let desired_camera_position = getCameraPosition(head_position, camera_orientation, camera_zoom);
    let ray_direction = desired_camera_position - head_position; // object->camera vector
    let camera_target_rigid_group_id = camera_target_object_id.and_then(|object_id| world.physics.getSubitemRigidGroupId(object_id));
    let is_player_object = |object_id: ObjectId| {
        Some(object_id) == camera_target_object_id || Some(object_id) == camera_target_rigid_group_id || world.getPlayerIdWhoControlsObject(object_id).is_some()
    };

    // cast ray to find non-transparent occluding object
    let hit = world.physics.castBall(head_position, ray_direction, CAMERA_SIZE, &|object_id| {
        // The local player's head may be separate from their controlled body.
        if is_player_object(object_id) {
            return true;
        }
        let Ok(isTransparent) = world.getIsObjectTransparent(object_id) else {
            return true; // exclude
        };
        if isTransparent {
            return true; // exclude
        }
        return false;
    });

    // Meshes only block the camera where they are closer than the nearest collider.
    let collider_hit_distance = hit.map(|(_, hit_distance)| hit_distance);
    let mesh_translation = ray_direction.normalize_or_zero() * collider_hit_distance.unwrap_or(camera_zoom);
    let mesh_hit_distance = mesh_camera_shapes.cast_camera_ball(
        world,
        get_mesh_data,
        head_position,
        mesh_translation,
        &is_player_object,
        est_dely_server_sim_ms,
        live_client_sim_ms,
    );
    let Some(occluding_hit_distance) = mesh_hit_distance.or(collider_hit_distance) else {
        return camera_zoom;
    };

    occluding_hit_distance.clamp(MIN_CAMERA_ZOOM, camera_zoom)
}

pub fn getBlockedCameraZoom(
    world: &World,
    player_object_id: ObjectId,
    play_input_state: &PlayInputState,
    camera_offset: Vec3,
    mesh_camera_shapes: &mut MeshCameraShapes,
    get_mesh_data: &dyn Fn(DataId) -> Option<Arc<MeshData>>,
    est_dely_server_sim_ms: f64,
    live_client_sim_ms: f64,
) -> f32 {
    let camera_target_object_id = world.getPlayerHeadObjectId(player_object_id);
    let camera_target_position = camera_target_object_id
        .and_then(|camera_target_object_id| world.getObjectPhysicsInfo3D(camera_target_object_id))
        .map(|camera_target_physics_info_3d| {
            getCameraTarget(
                camera_target_physics_info_3d.location.position,
                &play_input_state.camera_orientation,
                camera_offset,
            )
        });
    let Some(camera_target_position) = camera_target_position else {
        return play_input_state.desired_camera_zoom;
    };
    computeEffectiveCameraZoom(
        camera_target_position,
        camera_target_object_id,
        &play_input_state.camera_orientation,
        play_input_state.desired_camera_zoom,
        world,
        mesh_camera_shapes,
        get_mesh_data,
        est_dely_server_sim_ms,
        live_client_sim_ms,
    )
}

// A click, mouse button or hover handler a client script gave its GUI, which the play webview says was triggered.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiTriggeredHandler {
    pub script_object_id: ObjectId,
    pub gui_key: String,
    pub handler_id: String,
    pub button: Option<String>, // "left" or "right" for onMouseDown and onMouseUp
}

pub struct PlayLoopInputs {
    pub world: World,
    pub player_object_id: ObjectId,
    pub snapshot_rx: mpsc::UnboundedReceiver<ServerSnapshotMessage>,
    pub player_pings_rx: mpsc::UnboundedReceiver<ServerPlayerPingsMessage>,
    pub script_raw_code_rx: mpsc::UnboundedReceiver<ServerScriptRawCodeMessage>,
    pub client_js_fn_request_rx: mpsc::UnboundedReceiver<(ClientFunctionCallMessage, oneshot::Sender<anyhow::Result<String>>)>,
    pub gui_handler_trigger_rx: mpsc::UnboundedReceiver<GuiTriggeredHandler>,
    pub volume_rx: watch::Receiver<f32>,
    pub js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    pub key_and_mouse_event_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    pub time_estimator_reader: TimeEstimatorReader,
    pub connection: Connection,
    pub play_input_state_rx: watch::Receiver<PlayInputState>,
    pub render_snapshot_tx: watch::Sender<Arc<FullWorldRenderSnapshot>>,
    pub blocked_camera_zoom_tx: watch::Sender<f32>,
    pub sound_playback_manager: Option<SoundPlaybackManager>,
    pub console_log_callback: Option<JsConsoleLogCallback>,
    pub data_fetcher: DataFetcherArc,
}

// only awaits 1 or the other, if have both we are done
enum AwaitingScriptState {
    ReceivedTCRawCode(String),
    ReceivedUDPSnapshot,
}

pub struct PlayLoopRunner {
    world: World,
    jsRuntime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,

    // use these to read and manage snapshots that come in
    snapshot_rx: mpsc::UnboundedReceiver<ServerSnapshotMessage>, // when a udp snapshot is received by nwing, it is sent here
    player_pings_rx: mpsc::UnboundedReceiver<ServerPlayerPingsMessage>, // every player's ping, which the server sends once a second
    ping_ms_of_player_object_id: HashMap<ObjectId, u16>,         // every player's ping from the server's latest update
    script_raw_code_rx: mpsc::UnboundedReceiver<ServerScriptRawCodeMessage>, // when tcp script content is received by nwing, it is sent here
    client_js_fn_request_rx: mpsc::UnboundedReceiver<(ClientFunctionCallMessage, oneshot::Sender<anyhow::Result<String>>)>,
    gui_handler_trigger_rx: mpsc::UnboundedReceiver<GuiTriggeredHandler>,
    volume_rx: watch::Receiver<f32>,
    js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>, // world systems queue typed JS callback inputs here
    key_and_mouse_event_rx: mpsc::UnboundedReceiver<JsCallbackInputs>, // the play window's key and mouse events, handed to scripts before physics
    latest_applied_message_num: MessageNum,                           // the newest snapshot taken in. Inputs send it so the server knows which poses include it

    snapshot_interpolation_buffer: SnapshotInterpolationBuffer,
    recently_removed_object_ids: HashSet<ObjectId>, // so we don't spuriously add an object that was removed due to weird network conditions
    awaiting_scripts: HashMap<ObjectId, AwaitingScriptState>, // wait for script snapshot (UDP) AND script raw code (TCP)

    time_estimator_reader: TimeEstimatorReader,
    prev_input_num: InputNum,
    connection: Connection, // should only be used for sending messages, the other items here are already here for reading them

    play_input_state_rx: watch::Receiver<PlayInputState>, // receives the camera and mouse state from the render thread
    render_object_cache: RenderObjectCache,               // so each tick's render snapshot only rebuilds what changed
    render_snapshot_tx: watch::Sender<Arc<FullWorldRenderSnapshot>>, // sends render world snapshots to render thread
    blocked_camera_zoom_tx: watch::Sender<f32>,
    sound_playback_manager: Option<SoundPlaybackManager>,
    console_log_callback: Option<JsConsoleLogCallback>,
    data_fetcher: DataFetcherArc,
    mesh_camera_shapes: MeshCameraShapes, // the triangles of each mesh the camera has come near, so meshes block it like bricks do

    // player state
    player_object_id: ObjectId,
    play_input_state: PlayInputState,

    // physics + world-step state
    prev_update_live_client_wall_ms: f64, // client wall-clock time of the previous world update.
    prev_live_client_sim_ms: f64,         // client simulation time the live world's latest physics tick was stepped to.
    did_prev_tick_overrun_budget: bool,   // when true, the next physics call runs 1 tick instead of the full catch-up burst

    client_world_delay: ClientWorldDelay,
}

impl PlayLoopRunner {
    async fn new(play_loop_inputs: PlayLoopInputs) -> anyhow::Result<Self> {
        let PlayLoopInputs {
            mut world,
            player_object_id,
            snapshot_rx,
            player_pings_rx,
            script_raw_code_rx,
            client_js_fn_request_rx,
            gui_handler_trigger_rx,
            volume_rx,
            js_callback_inputs_rx,
            key_and_mouse_event_rx,
            time_estimator_reader,
            connection,
            play_input_state_rx,
            render_snapshot_tx,
            blocked_camera_zoom_tx,
            sound_playback_manager,
            console_log_callback,
            data_fetcher,
        } = play_loop_inputs;

        // initialize js runtime + run initially-loaded scripts
        let (mut jsRuntime, script_manager_rc) =
            match initializeJsRuntime(JsRuntimeInitialization::ServerFnConnection(connection.clone()), console_log_callback.clone()).await {
                Ok(js_runtime_parts) => js_runtime_parts,
                Err(error) => return Err(error),
            };

        let live_client_wall_ms = time_estimator_reader.get_live_client_wall_ms();
        let est_live_server_wall_ms = time_estimator_reader.get_est_server_ms_at(live_client_wall_ms);
        if let Err(error) = initializeJsAndRunInitialScripts(
            &mut jsRuntime,
            &mut world,
            est_live_server_wall_ms,
            Some(live_client_wall_ms),
            &script_manager_rc,
        )
        .await
        {
            return Err(error);
        }
        Ok(Self {
            connection,
            snapshot_rx,
            player_pings_rx,
            ping_ms_of_player_object_id: HashMap::new(),
            script_raw_code_rx,
            client_js_fn_request_rx,
            gui_handler_trigger_rx,
            volume_rx,
            js_callback_inputs_rx,
            key_and_mouse_event_rx,
            latest_applied_message_num: 0,
            time_estimator_reader,

            snapshot_interpolation_buffer: SnapshotInterpolationBuffer::new(),
            recently_removed_object_ids: HashSet::new(),
            awaiting_scripts: HashMap::new(),

            world,
            jsRuntime,
            script_manager_rc,

            player_object_id,
            play_input_state: PlayInputState::new(),
            prev_input_num: 0,
            play_input_state_rx,
            render_object_cache: RenderObjectCache::default(),
            render_snapshot_tx,
            blocked_camera_zoom_tx,
            sound_playback_manager,
            console_log_callback,
            data_fetcher,
            mesh_camera_shapes: MeshCameraShapes::new(),

            prev_update_live_client_wall_ms: live_client_wall_ms,
            prev_live_client_sim_ms: live_client_wall_ms,
            did_prev_tick_overrun_budget: false,
            client_world_delay: ClientWorldDelay::new(),
        })
    }

    // Errors around a script also go where its console.log goes, so a headless playtest can see them.
    fn report_error(&self, message: String) {
        log::error!("{message}");
        if let Some(console_log_callback) = &self.console_log_callback {
            console_log_callback(JsConsoleLogEntry {
                log_level: "error".to_string(),
                message,
            });
        }
    }

    // if we already got udp snapshot, mark it ready else mark it receivedTcp
    fn on_get_tcp_script_code(&mut self, object_id: ObjectId, raw_code: String) {
        let awaiting_script_state = self.awaiting_scripts.remove(&object_id);
        match awaiting_script_state {
            Some(AwaitingScriptState::ReceivedUDPSnapshot) => {
                self.world.markScriptAsReadyToRun(object_id, raw_code);
            }
            _ => {
                self.awaiting_scripts.insert(object_id, AwaitingScriptState::ReceivedTCRawCode(raw_code));
            }
        }
    }

    // vice versa of the above
    fn on_get_udp_script_added(&mut self, object_id: ObjectId) {
        let awaiting_script_state = self.awaiting_scripts.remove(&object_id);
        match awaiting_script_state {
            Some(AwaitingScriptState::ReceivedTCRawCode(raw_code)) => {
                self.world.markScriptAsReadyToRun(object_id, raw_code);
            }
            _ => {
                self.awaiting_scripts.insert(object_id, AwaitingScriptState::ReceivedUDPSnapshot);
            }
        }
    }
    // getTime() inside a step callback reads the time of the step being simulated, not the real wall clock
    fn runCharacterStepCallbacks(&mut self, character_step_input: JsCallbackInputs, live_client_sim_ms: f64) {
        let est_live_server_sim_ms = self.time_estimator_reader.get_est_server_ms_at(live_client_sim_ms);
        let res = runJsCallbacks(
            &mut self.jsRuntime,
            &mut self.world,
            est_live_server_sim_ms,
            Some(live_client_sim_ms),
            &self.script_manager_rc,
            vec![character_step_input],
        );
        if let Err(error) = res {
            log::warn!("Error running character step callbacks on client: {}", error);
        }
    }

    fn stepClientPhysicsOneTick(&mut self, live_client_sim_ms: f64, num_physics_steps: u64) {
        // Everything that feeds the simulation is sampled at this tick's boundary, never at the real wall clock.
        let dely_client_sim_ms = live_client_sim_ms - self.client_world_delay.current_delay_ms() as f64;
        let est_dely_server_sim_ms = self.time_estimator_reader.get_est_server_ms_at(dely_client_sim_ms);

        // 1. update the world: the changes of the snapshots that are due, then the poses, cameras and metadata shown at this moment

        // NOTE: parent changes are NOT done by pop_due_snapshots, they're done by pop_due_valid_parent_deltas
        // - `adds`
        // - `modifies`.modifiedParent
        let SnapshotChanges { modifies, removes } = self.snapshot_interpolation_buffer.pop_due_snapshots(est_dely_server_sim_ms);
        self.update_world_using_due_modifies(modifies);
        self.update_world_using_due_removes(removes);
        self.snapshot_interpolation_buffer.forget_old_timeline_entries(est_dely_server_sim_ms);
        let interpolated_poses = self.snapshot_interpolation_buffer.get_interpolated_poses(est_dely_server_sim_ms);
        self.update_world_using_interpolated_poses(interpolated_poses);
        let due_foreign_players = self.snapshot_interpolation_buffer.get_due_foreign_players(est_dely_server_sim_ms);
        self.update_world_using_due_foreign_players(due_foreign_players);
        // in future when we add local objects, we need to step the local Skeleton poses for them here

        // 2. update world using due parent changes.
        // Loop to a fixpoint: applying a parent's add can make its children's adds valid, and a deep
        // object tree (e.g. a spawned character) should land in one tick, not one nesting level per tick.
        loop {
            let (due_adds, due_parent_changes) = self.snapshot_interpolation_buffer.pop_due_valid_parent_deltas(&self.world);
            if due_adds.is_empty() && due_parent_changes.is_empty() {
                break;
            }
            self.update_world_using_due_adds(due_adds);
            self.update_world_using_parent_due_changes(due_parent_changes);
        }

        // clientCharacterService's before-step callbacks run right before the solve.
        // Step callbacks only run when a step actually happens and the local player has a character.
        if num_physics_steps > 0
            && let Some(character_object_id) = self.world.getPlayerCharacterObject(self.player_object_id)
        {
            self.runCharacterStepCallbacks(
                JsCallbackInputs::BeforeCharacterStep {
                    character_object_id,
                    dt_secs: CLIENT_SIM_AND_SEND_SECS,
                },
                live_client_sim_ms,
            );
        }

        // 3. step physics
        let res = self
            .world
            .step_physics(num_physics_steps, CLIENT_SIM_AND_SEND_SECS, est_dely_server_sim_ms, Some(live_client_sim_ms));
        if let Err(error) = res {
            log::warn!("Error stepping client physics with rigid-group Skeleton followers: {}", error);
        }

        // clientCharacterService's after-step callbacks see where the solve put the character, and can correct it before the frame is drawn
        if num_physics_steps > 0
            && let Some(character_object_id) = self.world.getPlayerCharacterObject(self.player_object_id)
        {
            self.runCharacterStepCallbacks(
                JsCallbackInputs::AfterCharacterStep {
                    character_object_id,
                    dt_secs: CLIENT_SIM_AND_SEND_SECS,
                },
                live_client_sim_ms,
            );
        }
    }

    fn stepClientPhysics(&mut self, to_live_client_sim_ms: f64) {
        // Catch-up must not amplify overload: if the previous call already blew the per-tick budget,
        // running the full catch-up burst multiplies the cost of the next call too, which pins a slow
        // machine at max-catch-up forever. Run a single tick instead and let the capped-backlog
        // dropping below absorb the rest (the game runs slightly slow-motion instead of spiraling).
        let max_num_steps = if self.did_prev_tick_overrun_budget {
            1
        } else {
            CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK
        };

        // get num_steps
        let num_steps = World::get_num_physics_steps_to_take(
            self.prev_live_client_sim_ms,
            to_live_client_sim_ms,
            CLIENT_SIM_AND_SEND_SECS * 1000.0,
            max_num_steps,
        );
        if num_steps != 0 {
            self.prev_live_client_sim_ms = to_live_client_sim_ms;
        }

        let physics_call_start = std::time::Instant::now();
        let client_step_ms = CLIENT_SIM_AND_SEND_SECS * 1000.0;
        let first_live_client_sim_ms = to_live_client_sim_ms - num_steps.saturating_sub(1) as f64 * client_step_ms as f64;
        if num_steps == 0 {
            // nothing will step, so stay on the last boundary we actually stepped to
            self.stepClientPhysicsOneTick(self.prev_live_client_sim_ms, 0);
            return;
        }

        for step_index in 0..num_steps {
            let live_client_sim_ms = first_live_client_sim_ms + step_index as f64 * client_step_ms as f64;
            self.stepClientPhysicsOneTick(live_client_sim_ms, 1);
        }

        // budget: the wall time this call was simulating. Overrunning it means we are falling behind.
        let simulated_ms = num_steps as f32 * client_step_ms;
        self.did_prev_tick_overrun_budget = physics_call_start.elapsed().as_secs_f32() * 1000.0 > simulated_ms;
    }

    // -------------------------- Applying object deltas --------------------------
    // These methods apply received deltas to World. The browser counterpart is StoredWorldRenderable::apply_object_deltas.

    fn update_world_using_parent_due_changes(&mut self, valid_parent_changes: Vec<(ObjectId, ObjectId)>) {
        let world = &mut self.world;
        for (object_id, desired_parent_id) in valid_parent_changes {
            if !world.hasObject(object_id) {
                continue;
            }
            let res = world.setParent(object_id, desired_parent_id);
            if let Err(error) = res {
                log::error!(
                    "update_world_using_parent_changes: Unexpected error setting parent {object_id} -> {desired_parent_id}: {}",
                    error
                );
                continue;
            }
        }
    }

    fn update_world_using_due_adds(&mut self, valid_adds: Vec<(ObjectId, ObjectAddDelta)>) {
        let world = &mut self.world;
        let recently_removed_buffer = &self.recently_removed_object_ids;

        let mut scripts_added = Vec::new();
        world.refreshAnimationKeyframesOnceAfter(|world| {
            for (objectId, object_add_delta) in valid_adds {
                if recently_removed_buffer.contains(&objectId) {
                    continue;
                }
                if world.hasObject(objectId) {
                    log::debug!("Skipping snapshot add for existing object id: {}", objectId); // an add sent again before its ACK arrived
                    continue;
                }
                // keep track of scripts we added for later
                match &object_add_delta.objectBeingAdded {
                    ObjectBeingAdded::Script(_) => scripts_added.push(objectId),
                    _ => {}
                }
                world.addObjectWithIdFromDelta(objectId, object_add_delta);
            }
        });

        for object_id in scripts_added {
            self.on_get_udp_script_added(object_id);
        }
    }

    fn update_world_using_due_modifies(&mut self, modifies: Vec<(ObjectId, ObjectModifyDelta)>) {
        for (object_id, updateInfo) in modifies {
            if !self.world.hasObject(object_id) {
                continue;
            }

            let ObjectModifyDelta {
                modifiedObjectInfo: changeInfo,
                modifiedPrimitiveSettings: changePrimitiveSettings,
                modifiedPhysicsToggles3D: changePhysicsToggles,
                modifiedPhysicsInfo3D: changePhysicsInfo3D,
                modifiedName: changeName,
                ..
            } = updateInfo;

            // update physics toggles
            if let Some(physicsToggles3D) = changePhysicsToggles {
                self.world.setPhysicsToggles3D(
                    object_id,
                    SetPhysicsToggles3D {
                        isWalkthrough: Some(physicsToggles3D.isWalkthrough),
                        isPushable: Some(physicsToggles3D.isPushable),
                        density: Some(physicsToggles3D.density),
                    },
                    false,
                );
            }

            // update primitive info
            if let Some(primitive_settings) = changePrimitiveSettings {
                if let Err(error) = self.world.setPrimitiveSettings(object_id, primitive_settings, false) {
                    log::warn!("Error setting primitive info on client: {}", error);
                }
            }

            // update physics info 3d
            if let Some(networked_physics_info_3d) = changePhysicsInfo3D {
                let res = self.world.setNetworkedPhysicsInfo3D(object_id, networked_physics_info_3d);
                if let Err(error) = res {
                    log::warn!("Error applying due snapshot physics on client: {}", error);
                }
            }

            // update name
            if let Some(name) = changeName {
                let _ = self.world.setObjectName(object_id, name);
            }

            // update object info - get mutable object so we can change it for name and object_info
            // this fn only gets called on client, so we don't need to mark anything modified/added/removed after changing it here s
            let Ok(obj_mut) = self.world.getObjectMut(object_id) else {
                log::error!("update_world_using_modifies: object not found in world");
                continue;
            };
            if let Some(info) = changeInfo {
                let old_tracked_fields = ObjectInfoTrackedFields::from_object_info(&obj_mut.info);
                obj_mut.info = info;
                let new_tracked_fields = ObjectInfoTrackedFields::from_object_info(&obj_mut.info);
                self.world.afterObjectInfoChange(object_id, old_tracked_fields, new_tracked_fields);
            }
        }
    }

    fn update_world_using_interpolated_poses(&mut self, interpolated_poses: Vec<(ObjectId, NetworkedPhysicsInfo3D)>) {
        for (object_id, interpolated_pose) in interpolated_poses {
            if !self.world.hasObject(object_id) {
                continue;
            }
            if let Err(error) = self.world.setNetworkedPhysicsInfo3D(object_id, interpolated_pose) {
                log::warn!("Error applying an interpolated pose on client: {}", error);
            }
        }
    }

    fn update_world_using_due_foreign_players(&mut self, foreign_players: Vec<(ObjectId, ForeignPlayerDelta)>) {
        for (player_object_id, foreign_player_delta) in foreign_players {
            if !self.world.hasObject(player_object_id) {
                continue;
            }
            self.world.setPlayerCameraOrientation(player_object_id, foreign_player_delta.cameraOrientation);
            self.world.setPlayerMetadata(player_object_id, foreign_player_delta.playerMetadata);
        }
    }

    fn update_world_using_due_removes(&mut self, removes: Vec<ObjectId>) {
        let world = &mut self.world;
        for objectId in removes {
            self.recently_removed_object_ids.insert(objectId);
            self.awaiting_scripts.remove(&objectId);

            if !world.hasObject(objectId) {
                // once the server removes an object this client already deleted, there's nothing left to keep deleted
                if !world.serverObjectIdsDeletedOnClient.remove(&objectId) {
                    log::debug!("Skipping snapshot remove for missing object id: {}", objectId); // a remove sent again before its ACK arrived
                }
                continue;
            }
            let res = world.removeObject(objectId);
            // removeObject remembers it as removed on this client, but the server removed it for real
            world.serverObjectIdsDeletedOnClient.remove(&objectId);
            if let Err(error) = res {
                log::warn!("update_world_using_removes: Error removing object on client: {}", error);
                continue;
            }
        }
    }

    // -------------------------- Sending input and stepping the client --------------------------

    fn sendInput(&mut self) -> anyhow::Result<()> {
        self.world
            .setPlayerCameraOrientation(self.player_object_id, self.play_input_state.camera_orientation);
        // when this tick was simulated. The camera, metadata and pose all carry it, so watchers apply them together
        let est_live_server_sim_ms = self.time_estimator_reader.get_est_server_ms_at(self.prev_live_client_sim_ms);
        let foreign_player_delta = ForeignPlayerDelta {
            cameraOrientation: self.play_input_state.camera_orientation,
            playerMetadata: self.world.getPlayerMetadata(self.player_object_id).map(str::to_string),
            est_live_server_sim_ms,
        };
        let foreign_character_delta = {
            if let Some(controlled_object_id) = self.world.getPlayerCharacterObject(self.player_object_id) {
                if let Some(physicsInfo3D) = self.world.getObjectPhysicsInfo3D(controlled_object_id) {
                    Some(ForeignCharacterDelta {
                        physicsInfo3D,
                        est_live_server_sim_ms,
                        teleportCount: self.world.getTeleportCount(controlled_object_id),
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };

        let input_num = self.prev_input_num + 1;
        self.prev_input_num = input_num;
        sendDatagramMessage(
            &self.connection,
            &ClientMessageUDP::Input(ClientInputMessage {
                input_num,
                latest_applied_message_num: self.latest_applied_message_num,
                foreign_player_delta,
                foreign_character_delta,
            }),
        )?;
        Ok(())
    }

    // This runs the sim-owned steps that used to happen inside redraw before rendering was split out.
    async fn stepOnce(&mut self, tick_instant: std::time::Instant) {
        self.play_input_state = self.play_input_state_rx.borrow_and_update().clone();
        // The renderer applies the camera limits too, but a headless client has no renderer, and scripts should see them right away.
        (self.play_input_state.camera_orientation, self.play_input_state.desired_camera_zoom) = getClampedCamera(
            self.script_manager_rc.borrow().camera_limits(),
            self.play_input_state.camera_orientation,
            self.play_input_state.desired_camera_zoom,
        );

        // update js state based on current player inputs
        self.jsRuntime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeCameraOrientation>()
            .set(self.play_input_state.camera_orientation);
        self.jsRuntime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeMouseRay>()
            .set(self.play_input_state.mouse_ray);
        self.jsRuntime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeDesiredCameraZoom>()
            .set(self.play_input_state.desired_camera_zoom);

        // 1. apply any ordered TCP script raw code updates.
        while let Ok(script_raw_code_message) = self.script_raw_code_rx.try_recv() {
            let ServerScriptRawCodeMessage { object_id, raw_code } = script_raw_code_message;
            self.on_get_tcp_script_code(object_id, raw_code);
        }
        if !self.script_raw_code_rx.is_empty() {
            log::warn!("Script raw code receiver is not empty after trying to step client world. If this prints frequently we need to fix it");
        }

        // 2. add due snapshots to the snapshot buffer
        // this happens a lot so ok if we miss it once or twice, should be rare
        while let Ok(mut snapshot) = self.snapshot_rx.try_recv() {
            // apply the instantaneous modifies immediately, they skip the buffer completely
            self.update_world_using_due_modifies(snapshot.deltaOfObjectId2.drain().collect());

            // this player's own objects now include the snapshot's changes, so the next input's pose does too
            self.latest_applied_message_num = self.latest_applied_message_num.max(snapshot.message_num);

            // Ignore resend adds for objects we already have so the client does not keep buffering a second copy of the initial world.
            snapshot.deltaOfObjectId.retain(|object_id, delta| match delta {
                ObjectDelta::Add(_) => !self.world.hasObject(*object_id),
                _ => true,
            });

            if snapshot.deltaOfObjectId.is_empty() {
                continue;
            }

            // update the buffers with the new snapshot
            self.snapshot_interpolation_buffer.process_new_snapshot(snapshot);
        }
        if !self.snapshot_rx.is_empty() {
            log::warn!("Snapshot receiver is not empty after trying to step client world. If this prints frequently we need to fix it");
        }

        // the server sends every player's ping once a second
        while let Ok(player_pings) = self.player_pings_rx.try_recv() {
            self.ping_ms_of_player_object_id = player_pings.pingMsOfPlayerObjectId;
            self.jsRuntime
                .op_state()
                .borrow_mut()
                .borrow_mut::<JsRuntimePlayerPings>()
                .set(self.ping_ms_of_player_object_id.clone());
        }

        // 3. step physics if needed
        let live_client_wall_ms = self.time_estimator_reader.get_live_client_wall_ms();
        let live_client_sim_ms = self.time_estimator_reader.get_live_client_sim_ms_at(tick_instant);
        let est_live_server_sim_ms = self.time_estimator_reader.get_est_server_ms_at(live_client_sim_ms);

        let ms_since_prev_update = (live_client_wall_ms - self.prev_update_live_client_wall_ms) as f32;
        self.client_world_delay
            .update(self.connection.rtt().as_secs_f32() * 1000.0, ms_since_prev_update);
        self.prev_update_live_client_wall_ms = live_client_wall_ms;

        let client_world_delay_ms = self.client_world_delay.current_delay_ms();
        self.jsRuntime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeClientWorldDelay>()
            .set(client_world_delay_ms);

        // key and mouse events reach scripts before physics, so step callbacks see this pass's keys
        let key_and_mouse_events: Vec<JsCallbackInputs> = std::iter::from_fn(|| self.key_and_mouse_event_rx.try_recv().ok()).collect();
        if !key_and_mouse_events.is_empty()
            && let Err(error) = runJsCallbacks(
                &mut self.jsRuntime,
                &mut self.world,
                est_live_server_sim_ms,
                Some(live_client_sim_ms),
                &self.script_manager_rc,
                key_and_mouse_events,
            )
        {
            log::error!("Error running client key and mouse callbacks: {}", error);
        }

        // the simulation lives in the past
        self.stepClientPhysics(live_client_sim_ms);

        // 4. run at most one client JS call the server wants, then send it back to the networking thread
        if let Ok((client_function_call_message, client_js_fn_oneshot_tx)) = self.client_js_fn_request_rx.try_recv() {
            let ClientFunctionCallMessage { fn_id, js_args } = client_function_call_message;
            let client_js_fn = self.script_manager_rc.borrow().get_client_js_fn(fn_id.as_str()).cloned();
            let client_js_fn_result = match client_js_fn {
                Some(client_js_fn) => {
                    callJsFn(
                        &mut self.jsRuntime,
                        &mut self.world,
                        est_live_server_sim_ms,
                        Some(live_client_sim_ms),
                        &client_js_fn,
                        js_args.as_str(),
                    )
                    .await
                }
                None => Err(anyhow::anyhow!("Client function not found: {}", fn_id)),
            };
            if let Err(error) = &client_js_fn_result {
                self.report_error(format!("Error calling client function {fn_id}: {error}"));
            }
            let _ = client_js_fn_oneshot_tx.send(client_js_fn_result);
        }

        // 5. run client JS after world updates so newly added scripts can start immediately.
        if let Err(error) = runPendingJS(
            &mut self.jsRuntime,
            &mut self.world,
            est_live_server_sim_ms,
            Some(live_client_sim_ms),
            &self.script_manager_rc,
            &mut self.js_callback_inputs_rx,
        )
        .await
        {
            self.report_error(format!("Error running client JS: {error}"));
        }

        // 6. Run click handlers after pending JS so state updates land in the next render snapshot.
        while let Ok(gui_triggered_handler) = self.gui_handler_trigger_rx.try_recv() {
            let GuiTriggeredHandler {
                script_object_id,
                gui_key,
                handler_id,
                button,
            } = gui_triggered_handler;
            if let Err(error) = callRegisteredGuiHandler(
                &mut self.jsRuntime,
                &mut self.world,
                est_live_server_sim_ms,
                Some(live_client_sim_ms),
                &self.script_manager_rc,
                script_object_id,
                gui_key.as_str(),
                handler_id.as_str(),
                button.as_deref(),
            )
            .await
            {
                self.report_error(format!("Error calling GUI handler {handler_id} for gui {gui_key}: {error}"));
            }
        }

        // 7. apply transitions
        // the server's fades play at the delayed time on this client, like everything else the server moves
        let est_dely_server_sim_ms = self
            .time_estimator_reader
            .get_est_server_ms_at(live_client_sim_ms - client_world_delay_ms as f64);
        self.world
            .flushAndSetFinishedWeightTransitions(est_dely_server_sim_ms, Some(live_client_sim_ms));

        // 8. Keep audio on the same delayed timeline that the renderer uses for animation.
        // Use the real wall clock rather than tick boundary (which simulation uses), because nothing divides a sound's time by dt, so it may as well be as current as possible.
        let to_dely_client_wall_ms = live_client_wall_ms - client_world_delay_ms as f64;
        let est_dely_server_wall_ms = self.time_estimator_reader.get_est_server_ms_at(to_dely_client_wall_ms);
        if let Some(sound_playback_manager) = self.sound_playback_manager.as_mut() {
            if self.volume_rx.has_changed().unwrap_or(false) {
                sound_playback_manager.set_volume(*self.volume_rx.borrow_and_update());
            }
            sound_playback_manager.updateSoundPlaybackFromWorld(&self.world, est_dely_server_wall_ms, live_client_wall_ms);
        }

        // 9. send renderer snapshots
        // send renderworld snapshot to renderer
        let gui_root_of_script_gui_key = self.script_manager_rc.borrow().gui_root_of_script_gui_key().clone();
        let changed_object_ids = self.world.takeObjectIdsChangedForRender();
        let render_snapshot = FullWorldRenderSnapshot::new(
            self.render_object_cache.build_render_snapshot(&self.world, changed_object_ids),
            &self.world,
            self.player_object_id,
            live_client_sim_ms,
            client_world_delay_ms,
            gui_root_of_script_gui_key,
            self.script_manager_rc.borrow().camera_limits(),
            self.script_manager_rc.borrow().mouse_cursor(),
            &self.ping_ms_of_player_object_id,
        );
        let _ = self.render_snapshot_tx.send(Arc::new(render_snapshot));

        // send blocked camera zoom to renderer
        let blocked_camera_zoom = getBlockedCameraZoom(
            &self.world,
            self.player_object_id,
            &self.play_input_state,
            self.script_manager_rc.borrow().camera_limits().offset,
            &mut self.mesh_camera_shapes,
            &|data_id| self.data_fetcher.getDownloadedMeshData(data_id),
            est_dely_server_sim_ms,
            live_client_sim_ms,
        );
        let _ = self.blocked_camera_zoom_tx.send(blocked_camera_zoom);
        // scripts read it with getZoom next tick
        self.jsRuntime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeCameraZoom>()
            .set(blocked_camera_zoom);
    }
}

// Start the dedicated play thread and keep the client world + JS runtime on that single thread.
pub fn start_running_play_in_bg(play_loop_inputs: PlayLoopInputs, kill_play_cancel_token: CancellationToken) {
    std::thread::spawn(move || {
        raise_current_thread_priority();
        let result = run_play_on_current_thread_forever(play_loop_inputs, kill_play_cancel_token.clone());
        match result {
            Ok(()) => log::info!("Play loop exited successfully"),
            Err(error) => log::error!("Play loop exited with error: {}", error),
        }
        // always signal shutdown when the play loop thread exits so the renderer/network tasks can unwind too.
        kill_play_cancel_token.cancel();
    });
}

// give this fn its own single-threaded tokio runtime, you must call it from inside std::thread::spawn()
// play must not be passed around among worker threads because of jsruntime, must be run on 1 thread, this tells it to do that
#[tokio::main(flavor = "current_thread")]
async fn run_play_on_current_thread_forever(play_loop_inputs: PlayLoopInputs, kill_play_cancel_token: CancellationToken) -> anyhow::Result<()> {
    log::info!("Play loop running!!!!!!!!!!!!!!!!");

    let mut play_loop_runner = PlayLoopRunner::new(play_loop_inputs).await?;
    let connection = play_loop_runner.connection.clone();

    // start game loop
    let mut physics_ticker = tokio::time::interval(Duration::from_secs_f32(CLIENT_SIM_AND_SEND_SECS));
    physics_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        enum Tick {
            Break,
            StepPhysics(tokio::time::Instant),
            Disconnect(quinn::ConnectionError),
        }
        let tick = tokio::select! {
            _ = kill_play_cancel_token.cancelled() => Tick::Break,
            tick_instant = physics_ticker.tick() => Tick::StepPhysics(tick_instant),
            disconnect_error = connection.closed() => Tick::Disconnect(disconnect_error),
        };

        match tick {
            Tick::Break => {
                break;
            }
            Tick::StepPhysics(tick_instant) => {
                play_loop_runner.stepOnce(tick_instant.into_std()).await;
                let res = play_loop_runner.sendInput();
                if let Err(error) = res {
                    log::warn!("Error sending input: {}", error);
                }
            }
            Tick::Disconnect(disconnect_error) => match disconnect_error {
                quinn::ConnectionError::LocallyClosed => {
                    break;
                }
                quinn::ConnectionError::ApplicationClosed(application_close) => {
                    log::info!("Connection closed by application: {}", String::from_utf8_lossy(&application_close.reason));
                    break;
                }
                other_error => {
                    anyhow::bail!(other_error.to_string());
                }
            },
        }
    }

    Ok(())
}
