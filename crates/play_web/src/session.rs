use crate::net::ServerConnection;
use crate::simulation::{HeldMovementKeys, turnedCameraOrientation, walkingVelocity};
use game_types::{
    consts::{CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK, CLIENT_SIM_AND_SEND_SECS, DEFAULT_CAMERA_ZOOM},
    object_types::{ObjectId, ObjectKind, Orientation3D},
};
use glam::Vec3;
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    computePropertyValue::{applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime},
    computeSkeletonPose::computeSkeletonPoseOfIdAtTime,
    render_scene::{build_render_scene, computeRenderObjectTransform},
    renderable::Renderable,
    renderer::{RenderFrame, Renderer},
};
use shared::game::{
    client_world_delay::ClientWorldDelay,
    physics::SetPhysicsInfo3D,
    snapshot_applier::SnapshotApplier,
    snapshot_buffer::{SnapshotChanges, SnapshotInterpolationBuffer},
    time_estimator::TimeEstimator,
    world::{World, WorldControlMode},
};
use shared::protocol::messages::{
    ClientAckMessage, ClientInputMessage, ClientMessageUDP, ClientPingMessage, ForeignCharacterDelta, ForeignPlayerDelta, InputNum, MessageNum, ObjectDelta,
    ServerMessageUDP, ServerSnapshotMessage,
};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

// Browsers either give the canvas a context or an adapter, and either way there is nothing to draw with.
const NO_WEBGPU_ERROR: &str = "This browser did not give the page WebGPU. Turn on graphics acceleration in your browser settings, then reload the page.";

// TODO: AI-PICKED-VALUE: The shipped character controller walks at 7 units a second and reaches that speed with an
// acceleration of 160, so walking here feels the same as it will once that script runs in the browser.
const WALKING_SPEED: f32 = 7.0;
const WALKING_ACCELERATION: f32 = 160.0;
// TODO: AI-PICKED-VALUE: One ping a second is what the desktop client sends, and it is enough to follow the ping changing.
const PING_INTERVAL_MS: f64 = 1000.0;

// -------------------------- One player's session --------------------------

// BrowserPlaySession is the whole of a browser game: one world the server runs, one local player, one canvas. The page
// calls render_frame once per animation frame and tells it which keys are held.
#[wasm_bindgen]
pub struct BrowserPlaySession {
    world: World,
    connection: ServerConnection,
    player_object_id: ObjectId,
    camera_orientation: Orientation3D,
    held_keys: HeldMovementKeys,
    // The server's clock on this page's clock, kept from the pongs the server answers.
    time_estimator: TimeEstimator,
    // The snapshots the server sent that are not due yet, and the changes they carry.
    snapshot_interpolation_buffer: SnapshotInterpolationBuffer,
    snapshot_applier: SnapshotApplier,
    client_world_delay: ClientWorldDelay,
    // The newest snapshot whose changes to this player's own object were applied, which the next input reports.
    latest_applied_message_num: MessageNum,
    prev_input_num: InputNum,
    // The last simulation boundary that physics reached, so a slow frame catches up and never fast-forwards.
    prev_live_client_sim_ms: f64,
    prev_live_client_wall_ms: f64,
    prev_ping_sent_ms: f64,
    rtt_ms: f32,
    num_physics_steps_taken: u64,
    last_frame_ms: f32,
    ended_reason: Option<String>,
    surface: BrowserSurface,
    camera_matrix_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
}

#[wasm_bindgen]
impl BrowserPlaySession {
    // Joins a game and starts drawing it. The page passes the address the game's play session gave it, the token that
    // lets it in, and the server certificate's hash when the certificate has no authority behind it.
    pub async fn connect(canvas: HtmlCanvasElement, server_url: &str, play_auth: &str, certificate_hash: Option<Vec<u8>>) -> Result<Self, JsValue> {
        let (connection, initial_world) = ServerConnection::join(server_url, play_auth, certificate_hash)
            .await
            .map_err(|error| JsValue::from_str(&error))?;
        let world = World::fromStoredWorldString(
            &initial_world.stored_world_string,
            WorldControlMode::Player(initial_world.player_object_id),
            None,
        )
        .map_err(to_js_error)?;
        let surface = BrowserSurface::new(canvas).await?;
        let (camera_matrix_buffer, camera_bind_group) = surface
            .renderer
            .create_camera_matrix_buffer_pair(Vec3::Z, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);

        let live_client_wall_ms = get_live_client_wall_ms();
        Ok(Self {
            world,
            connection,
            player_object_id: initial_world.player_object_id,
            camera_orientation: Orientation3D::Z(),
            held_keys: HeldMovementKeys {
                is_forward: false,
                is_backward: false,
                is_left: false,
                is_right: false,
            },
            time_estimator: TimeEstimator::new(initial_world.live_server_wall_ms, live_client_wall_ms),
            snapshot_interpolation_buffer: SnapshotInterpolationBuffer::new(),
            snapshot_applier: SnapshotApplier::default(),
            client_world_delay: ClientWorldDelay::new(),
            latest_applied_message_num: 0,
            prev_input_num: 0,
            prev_live_client_sim_ms: 0.0,
            prev_live_client_wall_ms: live_client_wall_ms,
            prev_ping_sent_ms: live_client_wall_ms,
            rtt_ms: 0.0,
            num_physics_steps_taken: 0,
            last_frame_ms: 0.0,
            ended_reason: None,
            surface,
            camera_matrix_buffer,
            camera_bind_group,
        })
    }

    // Draws one frame, applies what the server sent, and runs the simulation steps that came due since the last one.
    pub fn render_frame(&mut self, now_ms: f64) -> Result<(), JsValue> {
        // The first frame starts the timeline instead of catching up from zero.
        if self.prev_live_client_sim_ms == 0.0 {
            self.prev_live_client_sim_ms = now_ms;
        }

        self.apply_server_messages(now_ms);
        if let Some(ended_reason) = self.ended_reason.take() {
            return Err(JsValue::from_str(&ended_reason));
        }

        let ms_since_prev_update = (now_ms - self.prev_live_client_wall_ms) as f32;
        self.client_world_delay.update(self.rtt_ms, ms_since_prev_update);
        self.prev_live_client_wall_ms = now_ms;
        if now_ms - self.prev_ping_sent_ms >= PING_INTERVAL_MS {
            self.prev_ping_sent_ms = now_ms;
            self.connection.send(ClientMessageUDP::Ping(ClientPingMessage { t0_client_send_ms: now_ms }));
        }

        // The same accumulator the desktop client uses: a slow frame runs the steps it missed, up to a cap, so a long
        // stall drops the missed time instead of fast-forwarding the world.
        let step_ms = CLIENT_SIM_AND_SEND_SECS * 1000.0;
        let num_steps = World::get_num_physics_steps_to_take(self.prev_live_client_sim_ms, now_ms, step_ms, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK);
        let first_step_sim_ms = now_ms - num_steps.saturating_sub(1) as f64 * step_ms as f64;
        for step_index in 0..num_steps {
            let live_client_sim_ms = first_step_sim_ms + step_index as f64 * step_ms as f64;
            self.stepOnce(live_client_sim_ms);
            self.num_physics_steps_taken += 1;
        }
        if num_steps > 0 {
            self.prev_live_client_sim_ms = now_ms;
            self.sendInput(self.prev_live_client_sim_ms);
        }

        self.draw(self.prev_live_client_sim_ms)?;
        self.last_frame_ms = (get_live_client_wall_ms() - now_ms) as f32;
        Ok(())
    }

    // Keys arrive by their browser code. Anything but the walking keys is ignored.
    pub fn set_key(&mut self, code: &str, is_down: bool) {
        match code {
            "KeyW" | "ArrowUp" => self.held_keys.is_forward = is_down,
            "KeyS" | "ArrowDown" => self.held_keys.is_backward = is_down,
            "KeyA" | "ArrowLeft" => self.held_keys.is_left = is_down,
            "KeyD" | "ArrowRight" => self.held_keys.is_right = is_down,
            _ => {}
        }
    }

    // Mouse look, in pixels the pointer moved while the page holds it locked.
    pub fn turn_camera(&mut self, delta_x_pixels: f32, delta_y_pixels: f32) {
        self.camera_orientation = turnedCameraOrientation(self.camera_orientation, delta_x_pixels, delta_y_pixels);
    }

    // The page's debug readout: how long the last frame took, and how many physics steps the world has run.
    pub fn last_frame_ms(&self) -> f32 {
        self.last_frame_ms
    }

    pub fn num_physics_steps_taken(&self) -> u32 {
        self.num_physics_steps_taken.min(u32::MAX as u64) as u32
    }

    pub fn ping_ms(&self) -> f32 {
        self.rtt_ms
    }

    // Where the local player's body is, which the page shows so it is clear the keys reached the world.
    pub fn character_position(&self) -> Vec<f32> {
        let Some(body_object_id) = self.world.getPlayerCharacterObject(self.player_object_id) else {
            return Vec::new();
        };
        let Some(physics_info_3d) = self.world.getObjectPhysicsInfo3D(body_object_id) else {
            return Vec::new();
        };
        physics_info_3d.location.position.to_array().to_vec()
    }

    // How many other players this page can draw, which proves the server's players reached it.
    pub fn num_other_players(&self) -> u32 {
        self.world._kindOfObjectId.get_keys(&ObjectKind::Player).map_or(0, |player_object_ids| {
            player_object_ids
                .iter()
                .filter(|&&player_object_id| player_object_id != self.player_object_id)
                .count() as u32
        })
    }
}

impl BrowserPlaySession {
    // Everything the server sent since the last frame: snapshots to place on the delayed timeline, pongs to keep the
    // clock estimate, and the reason the game ended when it does.
    fn apply_server_messages(&mut self, now_ms: f64) {
        for message in self.connection.take_incoming() {
            match message {
                ServerMessageUDP::Snapshot(mut snapshot) => {
                    // The ack goes out the moment a snapshot arrives, before the play loop takes it in, so the server's
                    // resend timer never waits on a slow frame.
                    self.connection.send(ClientMessageUDP::Ack(ClientAckMessage {
                        message_num: snapshot.message_num,
                    }));
                    self.apply_snapshot(snapshot);
                }
                ServerMessageUDP::Pong(pong) => {
                    self.rtt_ms = (now_ms - pong.t0_client_send_ms) as f32;
                    self.time_estimator.process_pong(now_ms, pong.t0_client_send_ms, pong.t1_server_recv_ms);
                }
                ServerMessageUDP::PlayerPings(_) => {}
            }
        }
        if let Some(ended_reason) = self.connection.take_ended_reason() {
            self.ended_reason = Some(ended_reason);
        }
    }

    // This player's own objects are applied immediately, ahead of the buffer, so the next input's pose includes them.
    fn apply_snapshot(&mut self, mut snapshot: ServerSnapshotMessage) {
        self.snapshot_applier
            .apply_modifies(&mut self.world, snapshot.deltaOfObjectId2.drain().collect());
        self.latest_applied_message_num = self.latest_applied_message_num.max(snapshot.message_num);

        // Ignore resent adds for objects this page already has, so it never buffers a second copy of the world.
        snapshot.deltaOfObjectId.retain(|object_id, delta| {
            let was_already_sent = match delta {
                ObjectDelta::Add(_) => self.world.hasObject(*object_id),
                _ => false,
            };
            !was_already_sent
        });
        if snapshot.deltaOfObjectId.is_empty() {
            return;
        }
        self.snapshot_interpolation_buffer.process_new_snapshot(snapshot);
    }

    // One simulation step: the world at the delayed time the server's changes are shown at, then this player's walking.
    fn stepOnce(&mut self, live_client_sim_ms: f64) {
        let dely_client_sim_ms = live_client_sim_ms - self.client_world_delay.current_delay_ms() as f64;
        let est_dely_server_sim_ms = self.time_estimator.get_est_server_ms(dely_client_sim_ms);

        let SnapshotChanges { modifies, removes } = self.snapshot_interpolation_buffer.pop_due_snapshots(est_dely_server_sim_ms);
        self.snapshot_applier.apply_modifies(&mut self.world, modifies);
        self.snapshot_applier.apply_removes(&mut self.world, removes);
        self.snapshot_interpolation_buffer.forget_old_timeline_entries(est_dely_server_sim_ms);
        let interpolated_poses = self.snapshot_interpolation_buffer.get_interpolated_poses(est_dely_server_sim_ms);
        self.snapshot_applier.apply_interpolated_poses(&mut self.world, interpolated_poses);
        let due_foreign_players = self.snapshot_interpolation_buffer.get_due_foreign_players(est_dely_server_sim_ms);
        self.snapshot_applier.apply_foreign_players(&mut self.world, due_foreign_players);

        // Applying an add can make its children's adds valid, so a whole character lands in one step instead of one level
        // per step. A page runs no scripts, so the scripts an add brings are dropped here.
        loop {
            let (due_adds, due_parent_changes) = self.snapshot_interpolation_buffer.pop_due_valid_parent_deltas(&self.world);
            if due_adds.is_empty() && due_parent_changes.is_empty() {
                break;
            }
            self.snapshot_applier.apply_adds(&mut self.world, due_adds);
            self.snapshot_applier.apply_parent_changes(&mut self.world, due_parent_changes);
        }

        self.walkCharacterStep();
        if let Err(error) = self
            .world
            .step_physics(1, CLIENT_SIM_AND_SEND_SECS, est_dely_server_sim_ms, Some(live_client_sim_ms))
        {
            log::warn!("Could not step the browser's physics: {error}");
        }
    }

    // The character accelerates toward the walking velocity and keeps whatever the physics gives it vertically, so the
    // same keys walk it the same way the shipped character controller will once scripts run in the browser.
    fn walkCharacterStep(&mut self) {
        let Some(body_object_id) = self.world.getPlayerCharacterObject(self.player_object_id) else {
            return;
        };
        let Some(physics_info_3d) = self.world.getObjectPhysicsInfo3D(body_object_id) else {
            return;
        };
        let desired_velocity = walkingVelocity(self.camera_orientation, self.held_keys, WALKING_SPEED);
        let horizontal_velocity = Vec3::new(physics_info_3d.movement.velocity.x, 0.0, physics_info_3d.movement.velocity.z);
        let max_velocity_change = WALKING_ACCELERATION * CLIENT_SIM_AND_SEND_SECS;
        let velocity_change = (desired_velocity - horizontal_velocity).clamp_length(0.0, max_velocity_change);
        self.world.setPhysicsInfo3D(
            body_object_id,
            SetPhysicsInfo3D {
                velocity: Some(physics_info_3d.movement.velocity + velocity_change),
                ..Default::default()
            },
            false,
        );
    }

    // What this page was doing at this tick, which the server takes as this player's pose and passes to everyone else.
    fn sendInput(&mut self, live_client_sim_ms: f64) {
        self.world.setPlayerCameraOrientation(self.player_object_id, self.camera_orientation);
        let est_live_server_sim_ms = self.time_estimator.get_est_server_ms(live_client_sim_ms);
        let foreign_player_delta = ForeignPlayerDelta {
            cameraOrientation: self.camera_orientation,
            playerMetadata: self.world.getPlayerMetadata(self.player_object_id).map(str::to_string),
            est_live_server_sim_ms,
        };
        let foreign_character_delta = self.world.getPlayerCharacterObject(self.player_object_id).and_then(|character_object_id| {
            let physics_info_3d = self.world.getObjectPhysicsInfo3D(character_object_id)?;
            Some(ForeignCharacterDelta {
                physicsInfo3D: physics_info_3d,
                est_live_server_sim_ms,
                teleportCount: self.world.getTeleportCount(character_object_id),
            })
        });

        self.prev_input_num += 1;
        self.connection.send(ClientMessageUDP::Input(ClientInputMessage {
            input_num: self.prev_input_num,
            latest_applied_message_num: self.latest_applied_message_num,
            foreign_player_delta,
            foreign_character_delta,
        }));
    }

    // Poses the world at the simulation boundary that physics reached and sends it to the graphics card.
    fn draw(&mut self, sim_ms: f64) -> Result<(), JsValue> {
        let snapshot = self.world.getRenderSnapshot();
        let mut render_object_of_id = snapshot.render_object_of_id;
        let property_overrides_of_object_id =
            computePropertyOverridesOfObjectIdAtTime(&snapshot.property_overrides_of_id, &snapshot.rotation_base_degrees_of_id, sim_ms, Some(sim_ms));
        applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &property_overrides_of_object_id, &snapshot.pivot_offset_of_id);
        let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(&render_object_of_id, sim_ms, Some(sim_ms));
        // Downloaded meshes arrive with the asset loading that comes after this phase, so nothing is drawn from one yet.
        let mesh_data_of_id: HashMap<_, _> = HashMap::new();
        let scene = build_render_scene(&render_object_of_id, &mesh_data_of_id, Some(&skeleton_pose_of_id));

        let followed_position = self
            .world
            .getPlayerHeadObjectId(self.player_object_id)
            .and_then(|head_object_id| computeRenderObjectTransform(&render_object_of_id, &skeleton_pose_of_id, head_object_id))
            .map(|transform| transform.w_axis.truncate());
        let fallback_camera_location = self.world.fallback_camera_location;
        let (camera_position, camera_target) = match followed_position {
            Some(followed_position) => (
                followed_position - self.camera_orientation.to_unit_direction() * DEFAULT_CAMERA_ZOOM,
                followed_position,
            ),
            None => (
                fallback_camera_location.position,
                fallback_camera_location.position + fallback_camera_location.orientation.to_unit_direction(),
            ),
        };

        let (width, height) = (self.surface.canvas.width().max(1), self.surface.canvas.height().max(1));
        self.surface.resize(width, height)?;
        let aspect = width as f32 / height as f32;
        let view_proj = build_camera_view_proj_matrix(camera_position, camera_target, aspect, DEFAULT_CAMERA_FOV_DEGREES);
        self.surface
            .renderer
            .update_camera_matrix_buffer(&self.camera_matrix_buffer, camera_position, camera_target, aspect, DEFAULT_CAMERA_FOV_DEGREES);
        let (output, mut render_frame) = self.surface.begin_frame()?;
        self.surface.renderer.render_scene(
            &mut render_frame,
            &scene,
            None,
            &self.camera_bind_group,
            camera_target,
            camera_position,
            view_proj,
        );
        self.surface.renderer.finish_frame(render_frame);
        output.present();
        Ok(())
    }
}

// The page's clock, in milliseconds since it loaded. Every timestamp in the protocol is a millisecond count.
fn get_live_client_wall_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

fn to_js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

// -------------------------- The canvas and its graphics device --------------------------

// BrowserSurface owns the canvas surface and the shared core renderer, the same way the editor's browser surface does.
struct BrowserSurface {
    canvas: HtmlCanvasElement,
    wgpu_surface: wgpu::Surface<'static>,
    renderer: Renderer,
    surface_config: wgpu::SurfaceConfiguration,
}

impl BrowserSurface {
    async fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let instance = wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        })
        .await;
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let wgpu_surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|_| JsValue::from_str(NO_WEBGPU_ERROR))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&wgpu_surface),
                ..Default::default()
            })
            .await
            .map_err(|_| JsValue::from_str(NO_WEBGPU_ERROR))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("kube browser play"),
                required_features: adapter.features() & (wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(to_js_error)?;
        device.on_uncaptured_error(std::sync::Arc::new(|error| web_sys::console::error_1(&JsValue::from_str(&error.to_string()))));
        let mut surface_config = wgpu_surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| JsValue::from_str("No compatible WebGPU canvas format"))?;
        // Render linear shader output through an sRGB view, just as the native renderer does.
        let format = surface_config.format.add_srgb_suffix();
        surface_config.view_formats = vec![format];
        wgpu_surface.configure(&device, &surface_config);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let renderer = Renderer::new(device, queue, format, width, height).map_err(to_js_error)?;
        if let Some(error) = renderer.device().pop_error_scope().await {
            return Err(JsValue::from_str(&error.to_string()));
        }
        Ok(Self {
            canvas,
            wgpu_surface,
            renderer,
            surface_config,
        })
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        if width == 0 || height == 0 {
            return Err(JsValue::from_str("Canvas dimensions must be positive"));
        }
        let limit = self.renderer.device().limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(JsValue::from_str("Canvas exceeds the GPU texture size limit"));
        }
        if width != self.surface_config.width || height != self.surface_config.height {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.wgpu_surface.configure(self.renderer.device(), &self.surface_config);
            self.renderer.resize(width, height);
        }
        Ok(())
    }

    fn begin_frame(&mut self) -> Result<(wgpu::SurfaceTexture, RenderFrame), JsValue> {
        let output = self.wgpu_surface.get_current_texture().map_err(to_js_error)?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.surface_config.format.add_srgb_suffix()),
            ..Default::default()
        });
        let render_frame = self.renderer.begin_frame(view);
        Ok((output, render_frame))
    }
}

impl Drop for BrowserSurface {
    fn drop(&mut self) {
        self.renderer.device().destroy();
    }
}
