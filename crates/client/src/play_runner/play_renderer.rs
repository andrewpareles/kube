// should contain PlayRunner

use super::game_chat::GameChat;
use super::input_snapshot::{PlayInputState, getCameraPosition, getCameraTarget, getClampedCamera};
use super::mouse_cursor::{MouseCursorPicture, keep_window_mouse_cursor_shown, make_mouse_cursor_picture, set_window_mouse_cursor};
use super::play::GuiTriggeredHandler;
use super::render_snapshot::FullWorldRenderSnapshot;
use super::script_input_tracker::ScriptInputTracker;
use super::time_estimator::TimeEstimatorReader;
use crate::app_runner::{WindowInput, is_window_in_front};
use crate::play_web_view::{HostToWebViewMessage, PlayWebView, PlayersListEntry, UiRects, WebViewToHostMessage, get_script_gui_roots};
use glam::Vec3;
use indexmap::IndexMap;
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix, build_ray_from_camera_viewport_point},
    computePropertyValue::{applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime},
    computeSkeletonPose::{SkeletonPose, computeSkeletonPoseOfIdAtTime},
    renderer::{RenderFrame, SimpleViewportRegion},
    renderer_types::RenderObject,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::simple_renderer::SimpleRenderer;
use game_types::{
    consts::{CAMERA_MOUSE_WHEEL_ZOOM_STEP, CAMERA_PINCH_ZOOM_SENSITIVITY, CAMERA_ROTATE_SENSITIVITY},
    data_types::DataIds,
    object_types::{CameraLocation, InterpolationStyle, ObjectId, Orientation3D},
};
use shared::{
    game::{
        data_fetcher::DataFetcherArc,
        gui_types::{GuiRenderedRoot, ScriptGuiKey},
        interpolate_physics::{extrapolate_physics_info_3D, interpolate_between_location_3Ds_lerp, interpolate_between_physics_info_3Ds_hermite},
        jsRuntime::{CameraLimits, MouseCursor, MouseCursorImage},
        world::JsCallbackInputs,
    },
    protocol::messages::{ClientGameChatMessage, GameChatEntry},
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use winit::{
    dpi::PhysicalPosition,
    event::{DeviceEvent, MouseButton, MouseScrollDelta, WindowEvent},
    window::{CursorGrabMode, Window},
};

const CAMERA_BLOCKED_ZOOM_EPSILON: f32 = 0.0001;
const CAMERA_BLOCKED_ZOOM_IN_DELAY_SECS: f32 = 0.03; // TODO: AI-PICKED-VALUE: This ignores one to two frame camera obstructions while reacting quickly to real blocked views.
const CAMERA_BLOCKED_ZOOM_OUT_SPEED_PER_SEC: f32 = 300.0; // TODO: AI-PICKED-VALUE: This is how fast the camera is allowed to move back out after being blocked, so recovery feels smooth without making camera input feel delayed. It is a plain speed rather than a fraction of the zoom range, so changing MAX_CAMERA_ZOOM does not change how recovery feels.

// This computes the rendered zoom state so blocked zoom-in waits for stable obstruction while blocked zoom-out recovery can be smoothed.
pub fn computeNextRenderedZoomFromBlocking(
    camera_zoom_requested: f32,
    rendered_camera_zoom: f32,
    is_zoom_recovering_from_blocked: bool,
    blocked_camera_zoom: f32,
    blocked_zoom_elapsed_secs: f32,
    elapsed_redraw_secs: f32,
) -> (f32, bool, f32) {
    let mut next_rendered_camera_zoom = rendered_camera_zoom;
    let mut is_zoom_recovering_from_blocked_next = is_zoom_recovering_from_blocked;
    let elapsed_redraw_secs = elapsed_redraw_secs.max(0.0);
    let is_zoom_blocked = blocked_camera_zoom + CAMERA_BLOCKED_ZOOM_EPSILON < camera_zoom_requested;
    let blocked_zoom_elapsed_secs_next = if is_zoom_blocked {
        blocked_zoom_elapsed_secs.max(0.0) + elapsed_redraw_secs
    } else {
        0.0
    };
    let should_apply_blocked_zoom = is_zoom_blocked && blocked_zoom_elapsed_secs_next >= CAMERA_BLOCKED_ZOOM_IN_DELAY_SECS;
    let camera_zoom_target = if should_apply_blocked_zoom {
        blocked_camera_zoom
    } else {
        camera_zoom_requested
    };

    if should_apply_blocked_zoom {
        is_zoom_recovering_from_blocked_next = true;
    }

    let is_zooming_out = camera_zoom_target > next_rendered_camera_zoom;
    if is_zooming_out && is_zoom_recovering_from_blocked_next {
        let max_zoom_step = CAMERA_BLOCKED_ZOOM_OUT_SPEED_PER_SEC * elapsed_redraw_secs;
        let next_zoom = next_rendered_camera_zoom + max_zoom_step;
        next_rendered_camera_zoom = next_zoom.min(camera_zoom_target);
    } else {
        next_rendered_camera_zoom = camera_zoom_target;
    }

    if !should_apply_blocked_zoom && next_rendered_camera_zoom + CAMERA_BLOCKED_ZOOM_EPSILON >= camera_zoom_target {
        is_zoom_recovering_from_blocked_next = false;
    }
    (next_rendered_camera_zoom, is_zoom_recovering_from_blocked_next, blocked_zoom_elapsed_secs_next)
}

pub struct PlayRunnerInputs {
    pub play_input_state_tx: watch::Sender<PlayInputState>,
    pub render_snapshot_rx: watch::Receiver<Arc<FullWorldRenderSnapshot>>,
    pub blocked_camera_zoom_rx: watch::Receiver<f32>,
    pub gui_handler_trigger_tx: mpsc::UnboundedSender<GuiTriggeredHandler>,
    pub volume_tx: watch::Sender<f32>,
    pub key_and_mouse_event_tx: mpsc::UnboundedSender<JsCallbackInputs>,
    pub last_input_at_tx: watch::Sender<Instant>,
    pub game_chat_message_tx: mpsc::Sender<ClientGameChatMessage>,
    pub game_chat_history_rx: watch::Receiver<VecDeque<GameChatEntry>>,
    pub time_estimator_reader: TimeEstimatorReader,
    pub kill_play_cancel_token: CancellationToken,
    pub data_fetcher: DataFetcherArc,
    pub starter_data_ids: DataIds,
}

// -------------------------------------------- Client --------------------------------------------

// What is turning the camera. While something is, the pointer stays locked in place.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CameraTurner {
    RightMouseButton,
    TouchpadClick,
    TouchpadDrag,
}

pub struct PlayRunner {
    // received from play simulation thread
    blocked_camera_zoom_rx: watch::Receiver<f32>,
    render_snapshot_rx: watch::Receiver<Arc<FullWorldRenderSnapshot>>,

    // sent to play simulation thread
    play_input_state_tx: watch::Sender<PlayInputState>,
    gui_handler_trigger_tx: mpsc::UnboundedSender<GuiTriggeredHandler>,
    volume_tx: watch::Sender<f32>,
    script_input_tracker: ScriptInputTracker, // turns window input into script events on the JS callback queue
    last_input_at_tx: watch::Sender<Instant>, // read by the idle kick

    // player state
    camera_orientation: Orientation3D,
    camera_turner: Option<CameraTurner>, // only one thing turns the camera at a time
    // where on the screen the OS holds the pointer, or None while it moves freely
    pointer_lock_screen_position: Option<PhysicalPosition<f64>>,
    // on macOS the first mouse motion after the game moves the pointer also counts that move
    should_skip_next_mouse_motion: bool,
    // the cursor the window last finished with: shown, or skipped because its picture couldn't be made
    finished_mouse_cursor: Option<MouseCursor>,
    mouse_cursor_picture_rx: Option<(MouseCursor, oneshot::Receiver<anyhow::Result<MouseCursorPicture>>)>, // a picture being made on its own thread
    is_using_fallback_camera: bool,
    current_mouse_position: Option<PhysicalPosition<f64>>, // where the cursor is on the window, in the same space as the play viewport rect
    mouse_ray: Option<(Vec3, Vec3)>,                       // the ray under the cursor, rebuilt each frame from the camera the frame was drawn with

    // zoom state
    desired_camera_zoom: f32,
    rendered_camera_zoom: f32,
    camera_bind_group: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    latest_camera_target_position: Option<Vec3>,
    prev_redraw_live_client_wall_ms: f64,
    prev_drawn_dely_client_wall_ms: f64,
    is_zoom_recovering_from_blocked: bool,
    blocked_zoom_elapsed_secs: f32,

    prev_snapshot: Arc<FullWorldRenderSnapshot>,
    next_snapshot: Arc<FullWorldRenderSnapshot>,

    time_estimator_reader: TimeEstimatorReader,
    kill_play_cancel_token: CancellationToken,
    data_fetcher: DataFetcherArc,
    starter_data_ids: DataIds, // meshes and textures from the world and the server's templates that aren't on the GPU yet

    // the play webview draws the GUI and chat, and is only sent what has changed
    game_chat: GameChat,
    sent_gui_root_of_script_gui_key: IndexMap<ScriptGuiKey, GuiRenderedRoot>,
    players_list_on_page: Vec<PlayersListEntry>,
    // what the page last said about itself, so input from the OS can be sorted without waiting on the page
    ui_rects: UiRects,
    is_chat_focused: bool,
    is_menu_open: bool,
    // a script locked the mouse, the window is in front and the menu is closed, so the pointer is held in the middle and moving the mouse turns the camera
    is_mouse_locked: bool,
}

// Pulled out of redraw so the set of handled variants can be tested: a variant missing from the match
// renders on raw snapshot positions while everything else glides, and anything attached to it separates.
pub fn interpolate_render_object_of_id(
    prev_render_object_of_id: &HashMap<ObjectId, RenderObject>,
    next_render_object_of_id: &HashMap<ObjectId, RenderObject>,
    prev_snapshot_dely_client_sim_ms: f64,
    next_snapshot_dely_client_sim_ms: f64,
    drawn_dely_client_wall_ms: f64,
) -> HashMap<ObjectId, RenderObject> {
    let mut interpolated_render_object_of_id: HashMap<ObjectId, RenderObject> = next_render_object_of_id.clone();
    let snapshot_span_ms = (next_snapshot_dely_client_sim_ms - prev_snapshot_dely_client_sim_ms) as f32;
    if snapshot_span_ms > f32::EPSILON {
        let ms_since_next_snapshot = (drawn_dely_client_wall_ms - next_snapshot_dely_client_sim_ms) as f32;
        let alpha = InterpolationStyle::Linear.computeAlpha(0.0, snapshot_span_ms, ms_since_next_snapshot);
        let snapshot_span_secs = snapshot_span_ms / 1000.0;
        // We draw at the present and blend the two newest snapshots, so positions sit one tick behind, like Unity's
        // Interpolate. But ticks run on their own thread, so a frame can show up while the newest tick is still running
        // (about 8 ms in a heavy game like 308). Unity never hits this because its frame waits for physics to finish, and
        // Other platforms mostly sidestep it by simulating at up to 240 Hz. Waiting for the tick would drop frames at 120 Hz, and
        // drawing a fixed 15 ms in the past (what we did on Oct 9) costs latency you can feel. So for those few ms we carry
        // the newest snapshot forward along its velocity and spin, like Unity's Extrapolate, for at most one step. The
        // only cost is a stop or bounce in that window showing up a few cm late for a frame.
        let secs_past_next_step = (ms_since_next_snapshot - snapshot_span_ms).clamp(0.0, snapshot_span_ms) / 1000.0;

        for (object_id, next_render_object) in interpolated_render_object_of_id.iter_mut() {
            let Some(prev_render_object) = prev_render_object_of_id.get(object_id) else {
                continue;
            };

            if std::mem::discriminant(prev_render_object) != std::mem::discriminant(next_render_object) {
                continue;
            }
            let (Some(prev_transform), Some(next_transform)) = (prev_render_object.getTransform(), next_render_object.getTransformMut()) else {
                continue;
            };
            // skip interpolation for object_ids_moved_by_force
            // jumped since the last snapshot, so the renderer shows them where they are instead of smoothing them there
            // Compare persistent counts so this also catches teleports in skipped snapshots.
            if prev_transform.teleport_count != next_transform.teleport_count {
                continue;
            }
            // Interpolate each body once. Welded parts only interpolate changes to their local offset.
            // Changing groups snaps to the new attachment instead of blending coordinates from different groups.
            match (prev_transform.rigid_group_to_follow.as_ref(), next_transform.rigid_group_to_follow.as_mut()) {
                (Some((prev_group_id, prev_location)), Some((next_group_id, next_location))) if prev_group_id == next_group_id => {
                    *next_location = interpolate_between_location_3Ds_lerp(prev_location, next_location, alpha);
                }
                (None, None) => {
                    let blended_physics_info_3d =
                        interpolate_between_physics_info_3Ds_hermite(&prev_transform.physicsInfo3D, &next_transform.physicsInfo3D, snapshot_span_secs, alpha);
                    next_transform.physicsInfo3D = extrapolate_physics_info_3D(&blended_physics_info_3d, secs_past_next_step);
                }
                _ => {}
            }
        }
    }

    interpolated_render_object_of_id
}

// The snapshots' objects posed for this moment.
pub fn pose_render_objects(
    prev_snapshot: &FullWorldRenderSnapshot,
    next_snapshot: &FullWorldRenderSnapshot,
    drawn_live_client_wall_ms: f64,
    drawn_dely_client_wall_ms: f64,
    drawn_est_dely_server_wall_ms: f64,
) -> (HashMap<ObjectId, RenderObject>, HashMap<ObjectId, SkeletonPose>) {
    let prev_snapshot_dely_client_sim_ms = prev_snapshot.snapshot_live_client_sim_ms - prev_snapshot.snapshot_client_world_delay_ms as f64;
    let next_snapshot_dely_client_sim_ms = next_snapshot.snapshot_live_client_sim_ms - next_snapshot.snapshot_client_world_delay_ms as f64;

    let mut interpolated_render_object_of_id = interpolate_render_object_of_id(
        &prev_snapshot.render_snapshot.render_object_of_id,
        &next_snapshot.render_snapshot.render_object_of_id,
        prev_snapshot_dely_client_sim_ms,
        next_snapshot_dely_client_sim_ms,
        drawn_dely_client_wall_ms,
    );

    // compute updated SkAnimations (aka Skeletons) and property animations
    let propertyOverrideOfPathOfObjectId = computePropertyOverridesOfObjectIdAtTime(
        &next_snapshot.render_snapshot.property_overrides_of_id,
        &next_snapshot.render_snapshot.rotation_base_degrees_of_id,
        drawn_est_dely_server_wall_ms,
        Some(drawn_live_client_wall_ms),
    );

    // update property overrides now; Skeleton overrides are simply passed in and used later
    applyPropertyOverridesToRenderObjectOfId(
        &mut interpolated_render_object_of_id,
        &propertyOverrideOfPathOfObjectId,
        &next_snapshot.render_snapshot.pivot_offset_of_id,
    );

    // after the overrides, so animated joint rest poses show in the pose
    let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(
        &interpolated_render_object_of_id,
        drawn_est_dely_server_wall_ms,
        Some(drawn_live_client_wall_ms),
    );

    (interpolated_render_object_of_id, skeleton_pose_of_id)
}

// Behind the followed position, moved by the camera offset, at this zoom, or the world's fallback camera without one, as (position, target, shadow_center).
pub fn place_play_camera(
    followed_position: Option<Vec3>,
    camera_orientation: &Orientation3D,
    camera_zoom: f32,
    camera_offset: Vec3,
    fallback_camera_location: CameraLocation,
) -> (Vec3, Vec3, Vec3) {
    if let Some(followed_position) = followed_position {
        let camera_target_position = getCameraTarget(followed_position, camera_orientation, camera_offset);
        (
            getCameraPosition(camera_target_position, camera_orientation, camera_zoom),
            camera_target_position,
            followed_position,
        )
    } else {
        let fallback_position = fallback_camera_location.position;
        let camera_target_position = fallback_position + fallback_camera_location.orientation.to_unit_direction();
        (fallback_position, camera_target_position, fallback_position)
    }
}

impl PlayRunner {
    pub fn new(play_mode_runner_inputs: PlayRunnerInputs, play_web_view: &mut PlayWebView) -> anyhow::Result<Self> {
        let PlayRunnerInputs {
            play_input_state_tx,
            render_snapshot_rx,
            blocked_camera_zoom_rx,
            gui_handler_trigger_tx,
            volume_tx,
            key_and_mouse_event_tx,
            last_input_at_tx,
            game_chat_message_tx,
            game_chat_history_rx,
            time_estimator_reader,
            kill_play_cancel_token,
            data_fetcher,
            starter_data_ids,
        } = play_mode_runner_inputs;

        // The starter data downloads in the background, and redrawWorld puts its meshes and textures on the GPU as they land.
        // This is here and not where play starts so headless playtest players, who never draw, don't download it.
        tokio::spawn({
            let data_fetcher = data_fetcher.clone();
            let kill_play_cancel_token = kill_play_cancel_token.clone();
            let starter_data_ids = starter_data_ids.clone();
            async move {
                tokio::select! {
                    _ = kill_play_cancel_token.cancelled() => {}
                    _ = data_fetcher.preloadDataIds(starter_data_ids) => {}
                }
            }
        });

        let play_input_state = PlayInputState::new();
        let next_snapshot = render_snapshot_rx.borrow().clone();
        let prev_snapshot = next_snapshot.clone();
        play_web_view.send(&HostToWebViewMessage::ShowPlayScreen);
        let prev_redraw_live_client_wall_ms = time_estimator_reader.get_live_client_wall_ms();
        let prev_drawn_dely_client_wall_ms = prev_redraw_live_client_wall_ms - next_snapshot.snapshot_client_world_delay_ms as f64;
        Ok(Self {
            camera_orientation: play_input_state.camera_orientation,
            camera_turner: None,
            pointer_lock_screen_position: None,
            should_skip_next_mouse_motion: false,
            finished_mouse_cursor: None,
            mouse_cursor_picture_rx: None,
            is_using_fallback_camera: false,
            current_mouse_position: None,
            mouse_ray: None,
            desired_camera_zoom: play_input_state.desired_camera_zoom,
            rendered_camera_zoom: play_input_state.desired_camera_zoom,
            latest_camera_target_position: None,
            prev_redraw_live_client_wall_ms,
            prev_drawn_dely_client_wall_ms,
            is_zoom_recovering_from_blocked: false,
            blocked_zoom_elapsed_secs: 0.0,
            camera_bind_group: None,
            prev_snapshot,
            next_snapshot,
            play_input_state_tx,
            gui_handler_trigger_tx,
            volume_tx,
            script_input_tracker: ScriptInputTracker::new(key_and_mouse_event_tx),
            last_input_at_tx,
            render_snapshot_rx,
            blocked_camera_zoom_rx,
            time_estimator_reader,
            kill_play_cancel_token,
            data_fetcher,
            starter_data_ids,
            game_chat: GameChat::new(game_chat_message_tx, game_chat_history_rx),
            sent_gui_root_of_script_gui_key: IndexMap::new(),
            players_list_on_page: Vec::new(),
            ui_rects: UiRects::default(),
            is_chat_focused: false,
            is_menu_open: false,
            is_mouse_locked: false,
        })
    }

    pub fn push_snapshot(&mut self, snapshot: Arc<FullWorldRenderSnapshot>) {
        self.prev_snapshot = std::mem::replace(&mut self.next_snapshot, snapshot);
    }

    fn sendPlayInputState(&self) {
        let play_input_state = PlayInputState {
            camera_orientation: self.camera_orientation,
            desired_camera_zoom: self.desired_camera_zoom,
            mouse_ray: self.mouse_ray,
        };
        let _ = self.play_input_state_tx.send(play_input_state);
    }

    fn redrawWorld(&mut self, renderer: &mut SimpleRenderer, viewport_region: SimpleViewportRegion, render_frame: &mut RenderFrame) {
        if let Ok(true) = self.render_snapshot_rx.has_changed() {
            let render_snapshot = self.render_snapshot_rx.borrow_and_update().clone();
            self.push_snapshot(render_snapshot);
            // A script may have locked the camera or changed the zoom range since the last snapshot.
            (self.camera_orientation, self.desired_camera_zoom) =
                getClampedCamera(self.next_snapshot.camera_limits, self.camera_orientation, self.desired_camera_zoom);
            self.sendPlayInputState();
        }

        // Keep the camera smooth on the render thread even before we add transform interpolation between prev/next snapshots.
        let redraw_live_client_wall_ms = self.time_estimator_reader.get_live_client_wall_ms();
        let elapsed_redraw_secs = ((redraw_live_client_wall_ms - self.prev_redraw_live_client_wall_ms).max(0.0) / 1000.0) as f32;
        self.prev_redraw_live_client_wall_ms = redraw_live_client_wall_ms;
        let blocked_camera_zoom = *self.blocked_camera_zoom_rx.borrow_and_update();
        (self.rendered_camera_zoom, self.is_zoom_recovering_from_blocked, self.blocked_zoom_elapsed_secs) = computeNextRenderedZoomFromBlocking(
            self.desired_camera_zoom,
            self.rendered_camera_zoom,
            self.is_zoom_recovering_from_blocked,
            blocked_camera_zoom,
            self.blocked_zoom_elapsed_secs,
            elapsed_redraw_secs,
        );
        // Start from the authoritative next snapshot, then only overwrite physics for matching drawables.
        // That makes adds, removes, variant changes, and materials snap cleanly to next while redraw can still resample the SkKeyframePose at display time.
        // the play loop owns the world delay; read the value in effect when the snapshot was produced

        let (prev_snapshot, next_snapshot) = (self.prev_snapshot.as_ref(), self.next_snapshot.as_ref());

        // force monotonic
        let drawn_dely_client_wall_ms = redraw_live_client_wall_ms - next_snapshot.snapshot_client_world_delay_ms as f64;
        let drawn_dely_client_wall_ms = drawn_dely_client_wall_ms.max(self.prev_drawn_dely_client_wall_ms);
        self.prev_drawn_dely_client_wall_ms = drawn_dely_client_wall_ms;

        let drawn_est_dely_server_wall_ms = self.time_estimator_reader.get_est_server_ms_at(drawn_dely_client_wall_ms);
        let (interpolated_render_object_of_id, skeleton_pose_of_id) = pose_render_objects(
            prev_snapshot,
            next_snapshot,
            redraw_live_client_wall_ms,
            drawn_dely_client_wall_ms,
            drawn_est_dely_server_wall_ms,
        );
        let followed_position = next_snapshot
            .find_camera_target_position(&interpolated_render_object_of_id, &skeleton_pose_of_id)
            .or(self.latest_camera_target_position);
        let (camera_position, camera_target_position, shadow_center) = place_play_camera(
            followed_position,
            &self.camera_orientation,
            self.rendered_camera_zoom,
            next_snapshot.camera_limits.offset,
            next_snapshot.fallback_camera_location,
        );
        self.latest_camera_target_position = followed_position;
        self.is_using_fallback_camera = followed_position.is_none();
        let camera_fov_degrees = next_snapshot.camera_limits.fov_degrees.unwrap_or(DEFAULT_CAMERA_FOV_DEGREES);

        // this goes here instead of in mouse movements, because camera can move and change this
        let mouse_ray = if let Some(mouse_position) = self.current_mouse_position {
            build_ray_from_camera_viewport_point(
                camera_position,
                camera_target_position,
                camera_fov_degrees,
                viewport_region,
                mouse_position.x,
                mouse_position.y,
            )
        } else {
            None
        };
        if mouse_ray != self.mouse_ray {
            self.mouse_ray = mouse_ray;
            self.sendPlayInputState();
        }

        // Update the renderer camera from the snapshot data instead of reading World directly.
        let aspectRatio = viewport_region.width as f32 / viewport_region.height.max(1) as f32;
        if self.camera_bind_group.is_none() {
            self.camera_bind_group = Some(renderer.gpu.create_camera_matrix_buffer_pair(
                camera_position,
                camera_target_position,
                aspectRatio,
                camera_fov_degrees,
            ));
        }
        let Some((camera_buffer, camera_bind_group)) = self.camera_bind_group.as_ref() else {
            return;
        };
        renderer
            .gpu
            .update_camera_matrix_buffer(camera_buffer, camera_position, camera_target_position, aspectRatio, camera_fov_degrees);
        renderer.render_objects(
            render_frame,
            &interpolated_render_object_of_id,
            &self.data_fetcher,
            &mut self.starter_data_ids,
            Some(viewport_region),
            Some(&skeleton_pose_of_id),
            camera_bind_group,
            shadow_center,
            camera_position,
            build_camera_view_proj_matrix(camera_position, camera_target_position, aspectRatio, camera_fov_degrees),
        );
    }

    fn sendGuiToWebView(&mut self, play_web_view: &mut PlayWebView) {
        let gui_root_of_script_gui_key = &self.next_snapshot.gui_root_of_script_gui_key;
        // IndexMap equality ignores order, and a GUI hidden and shown again in one tick only moves.
        if !gui_root_of_script_gui_key.iter().eq(&self.sent_gui_root_of_script_gui_key) {
            play_web_view.send(&HostToWebViewMessage::SetScriptGuiRoots(get_script_gui_roots(gui_root_of_script_gui_key)));
            self.sent_gui_root_of_script_gui_key = gui_root_of_script_gui_key.clone();
        }
        if self.next_snapshot.players_list != self.players_list_on_page {
            play_web_view.send(&HostToWebViewMessage::SetPlayersList(&self.next_snapshot.players_list));
            self.players_list_on_page = self.next_snapshot.players_list.clone();
        }
        self.game_chat.send_to_web_view(play_web_view);
    }

    pub fn onRequestRedraw(
        &mut self,
        renderer: &mut SimpleRenderer,
        viewport_region: SimpleViewportRegion,
        render_frame: &mut RenderFrame,
        play_web_view: &mut PlayWebView,
        window: &Window,
    ) {
        self.redrawWorld(renderer, viewport_region, render_frame);
        self.sendGuiToWebView(play_web_view);
        self.update_pointer_lock(window);
        self.update_mouse_cursor(window);
    }

    // The window shows the game's mouse cursor once its picture is made. Decoding and resizing a big texture takes a while,
    // so each picture is made on its own thread, and the window keeps its last cursor until then.
    fn update_mouse_cursor(&mut self, window: &Window) {
        let mouse_cursor = self.next_snapshot.mouse_cursor;
        if self.finished_mouse_cursor == Some(mouse_cursor) {
            return;
        }
        if let Some((making_mouse_cursor, mouse_cursor_picture_rx)) = self.mouse_cursor_picture_rx.as_mut()
            && *making_mouse_cursor == mouse_cursor
        {
            let mouse_cursor_picture = match mouse_cursor_picture_rx.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => return,
                Err(oneshot::error::TryRecvError::Closed) => Err(anyhow::anyhow!("The thread making it stopped")),
                Ok(mouse_cursor_picture) => mouse_cursor_picture,
            };
            match mouse_cursor_picture {
                Ok(mouse_cursor_picture) => set_window_mouse_cursor(window, Some(mouse_cursor_picture)),
                Err(error) => log::warn!("Failed to make the mouse cursor {:?}: {}", mouse_cursor, error),
            }
            self.finished_mouse_cursor = Some(mouse_cursor);
            self.mouse_cursor_picture_rx = None;
            return;
        }
        let texture_data_arc = match mouse_cursor {
            MouseCursor::Builtin(_) => None,
            MouseCursor::Image(MouseCursorImage { textureDataId, .. }) => {
                let Some(texture_data_arc) = self.data_fetcher.getOrFetchTextureData(textureDataId) else {
                    return;
                };
                Some(texture_data_arc)
            }
        };
        let (mouse_cursor_picture_tx, mouse_cursor_picture_rx) = oneshot::channel();
        std::thread::spawn(move || {
            let _ = mouse_cursor_picture_tx.send(make_mouse_cursor_picture(mouse_cursor, texture_data_arc.as_deref()));
        });
        self.mouse_cursor_picture_rx = Some((mouse_cursor, mouse_cursor_picture_rx));
    }

    // The pointer is held in the middle of the window while a script locks the mouse, and in place while something turns
    // the camera. On macOS a held pointer stays still in every app, so it is only held while the window is in front. Its
    // spot is kept in screen pixels, so moving or resizing the window holds it again in the new middle. A script's lock
    // lets go while the menu is open, so the player can click its buttons.
    fn update_pointer_lock(&mut self, window: &Window) {
        let Ok(window_position) = window.inner_position() else {
            return;
        };
        let get_screen_position =
            |position: PhysicalPosition<f64>| PhysicalPosition::new(window_position.x as f64 + position.x, window_position.y as f64 + position.y);
        let window_size = window.inner_size();
        let is_window_in_front = is_window_in_front(window);
        let is_mouse_locked = is_window_in_front && self.next_snapshot.camera_limits.is_mouse_locked && !self.is_menu_open;
        let pointer_lock_screen_position = if is_mouse_locked {
            Some(get_screen_position(PhysicalPosition::new(
                (window_size.width / 2) as f64,
                (window_size.height / 2) as f64,
            )))
        } else if is_window_in_front && self.camera_turner.is_some() {
            self.pointer_lock_screen_position.or(self.current_mouse_position.map(get_screen_position))
        } else {
            None
        };
        self.is_mouse_locked = is_mouse_locked;
        self.set_pointer_lock_screen_position(pointer_lock_screen_position, window);
    }

    // The OS is only told when the pointer's spot changes.
    fn set_pointer_lock_screen_position(&mut self, pointer_lock_screen_position: Option<PhysicalPosition<f64>>, window: &Window) {
        if pointer_lock_screen_position == self.pointer_lock_screen_position {
            return;
        }
        self.pointer_lock_screen_position = pointer_lock_screen_position;
        // Windows holds the pointer by keeping it inside one pixel, and the game can't move it out of there either, so it is let go first.
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        let (Some(pointer_lock_screen_position), Ok(window_position)) = (pointer_lock_screen_position, window.inner_position()) else {
            return;
        };
        let pointer_lock_position = PhysicalPosition::new(
            pointer_lock_screen_position.x - window_position.x as f64,
            pointer_lock_screen_position.y - window_position.y as f64,
        );
        let _ = window.set_cursor_position(pointer_lock_position);
        let _ = window.set_cursor_grab(CursorGrabMode::Locked);
        // Moving the pointer sends no event, so the mouse ray would aim from the old spot until the mouse moves.
        self.current_mouse_position = Some(pointer_lock_position);
        self.should_skip_next_mouse_motion = true;
    }

    // Locks the pointer in place while something turns the camera. The pointer moves with a mouse, and the
    // lock keeps it from wandering off the window, while raw mouse motion still arrives.
    fn start_turning_camera(&mut self, camera_turner: CameraTurner, window: &Window) {
        if self.camera_turner.is_some() {
            return;
        }
        self.camera_turner = Some(camera_turner);
        self.update_pointer_lock(window);
    }

    // Only the thing that started turning the camera can stop it.
    fn stop_turning_camera(&mut self, camera_turner: CameraTurner, window: &Window) {
        if self.camera_turner != Some(camera_turner) {
            return;
        }
        self.camera_turner = None;
        self.update_pointer_lock(window);
    }

    // Releases everything the window can no longer report: swallowed keyups (like after Cmd+Tab) would otherwise stay held forever.
    fn handleWindowFocusLost(&mut self, window: &Window) {
        self.script_input_tracker.releaseAll();
        self.camera_turner = None;
        self.update_pointer_lock(window);
        self.sendPlayInputState();
    }

    fn rotate_camera_by_drag(&mut self, delta_x: f32, delta_y: f32) {
        if self.is_using_fallback_camera || self.next_snapshot.camera_limits.locked_orientation.is_some() {
            return;
        }
        self.camera_orientation
            .rotate_as_camera(-delta_x * CAMERA_ROTATE_SENSITIVITY, delta_y * CAMERA_ROTATE_SENSITIVITY);
        self.sendPlayInputState();
    }

    fn zoom_camera_by_distance(&mut self, zoom_distance: f32) {
        let next = self.desired_camera_zoom - zoom_distance;
        let CameraLimits { min_zoom, max_zoom, .. } = self.next_snapshot.camera_limits;
        self.desired_camera_zoom = next.clamp(min_zoom, max_zoom);
        self.sendPlayInputState();
    }

    // The play webview covers the window, so the window itself only reports focus and, on mac, pinching.
    pub fn onInputWindowEvent(&mut self, event: &WindowEvent, window: &Window) {
        match event {
            WindowEvent::Focused(false) => {
                // On Windows the webview takes keyboard focus right after the window gets it, so this comes then instead
                // of when the window goes to the back. The Windows input monitor reports that as WindowInput::FocusLost.
                if cfg!(target_os = "windows") {
                    return;
                }
                self.handleWindowFocusLost(window);
            }
            WindowEvent::PinchGesture { delta, .. } => {
                // mac: zoom in/out
                // (this is only on mac)
                self.last_input_at_tx.send_replace(Instant::now());
                self.zoom_camera_by_distance(*delta as f32 * CAMERA_PINCH_ZOOM_SENSITIVITY);
            }
            _ => {}
        }
    }

    // Keys and the mouse come straight from the OS, before the play webview sees them. Presses on the page's UI
    // and typing in its chat stay out of the game, using what the page last reported. Releases always reach
    // scripts, so a press that reached scripts always gets its matching release.
    pub fn onWindowInput(&mut self, window_input: WindowInput, window: &Window) {
        // Windows reports where the cursor is every frame, so only a cursor that moved counts.
        let is_player_input = match window_input {
            WindowInput::CursorMoved { position } => position != self.current_mouse_position,
            WindowInput::FocusLost => false,
            _ => true,
        };
        if is_player_input {
            self.last_input_at_tx.send_replace(Instant::now());
        }
        match window_input {
            WindowInput::Key { key_code, is_pressed } => {
                if is_pressed && self.is_chat_focused {
                    return;
                }
                self.script_input_tracker.pushKeyEvent(key_code, is_pressed);
            }
            WindowInput::MouseButton {
                button,
                is_pressed: true,
                position,
            } => {
                let Some(position) = position else {
                    return;
                };
                if !self.ui_rects.should_game_get_press(position) {
                    return;
                }
                self.script_input_tracker.pushMouseEvent(button, true);
                if button == MouseButton::Right {
                    self.start_turning_camera(CameraTurner::RightMouseButton, window);
                }
            }
            WindowInput::MouseButton { button, is_pressed: false, .. } => {
                self.script_input_tracker.pushMouseEvent(button, false);
                if button == MouseButton::Right {
                    self.stop_turning_camera(CameraTurner::RightMouseButton, window);
                }
            }
            WindowInput::CursorMoved { position } => {
                self.current_mouse_position = position;
                keep_window_mouse_cursor_shown();
            }
            WindowInput::Wheel { delta, position } => {
                let Some(position) = position else {
                    return;
                };
                if !self.ui_rects.should_game_get_scroll(position) {
                    return;
                }
                match delta {
                    // A mouse wheel zooms, by part of a step for a wheel that spins freely.
                    MouseScrollDelta::LineDelta(_, delta_y) => self.zoom_camera_by_distance(delta_y * CAMERA_MOUSE_WHEEL_ZOOM_STEP),
                    // A Mac trackpad's two-finger drag turns the camera.
                    MouseScrollDelta::PixelDelta(scroll_pixels) => self.rotate_camera_by_drag(scroll_pixels.x as f32, scroll_pixels.y as f32),
                }
            }
            // A two-finger click on a Windows touchpad is a right click, like on a Mac. Held down, Windows also turns
            // it into a left click on the page, so it only belongs to the game away from the page's buttons.
            WindowInput::TouchpadClick { is_pressed: true, position } => {
                let Some(position) = position else {
                    return;
                };
                if !self.ui_rects.should_game_get_press(position) {
                    return;
                }
                self.script_input_tracker.pushMouseEvent(MouseButton::Right, true);
                self.start_turning_camera(CameraTurner::TouchpadClick, window);
            }
            WindowInput::TouchpadClick { is_pressed: false, .. } => {
                self.script_input_tracker.pushMouseEvent(MouseButton::Right, false);
                self.stop_turning_camera(CameraTurner::TouchpadClick, window);
            }
            // Two light fingers moving together turn the camera too, unless they start over the page's UI, which scrolls instead.
            WindowInput::TouchpadDrag { is_dragging: true, position } => {
                let Some(position) = position else {
                    return;
                };
                if !self.ui_rects.should_game_get_scroll(position) {
                    return;
                }
                self.start_turning_camera(CameraTurner::TouchpadDrag, window);
            }
            WindowInput::TouchpadDrag { is_dragging: false, .. } => {
                self.stop_turning_camera(CameraTurner::TouchpadDrag, window);
            }
            WindowInput::TouchpadMove { delta_x, delta_y } => match self.camera_turner {
                Some(CameraTurner::TouchpadClick | CameraTurner::TouchpadDrag) => self.rotate_camera_by_drag(delta_x, delta_y),
                Some(CameraTurner::RightMouseButton) | None => {}
            },
            WindowInput::TouchpadPinch { delta, position } => {
                let Some(position) = position else {
                    return;
                };
                if !self.ui_rects.should_game_get_scroll(position) {
                    return;
                }
                self.zoom_camera_by_distance(delta * CAMERA_PINCH_ZOOM_SENSITIVITY);
            }
            WindowInput::FocusLost => {
                self.handleWindowFocusLost(window);
            }
        }
    }

    pub fn onWebViewMessage(&mut self, web_view_to_host_message: WebViewToHostMessage) {
        match web_view_to_host_message {
            // PlayWebView handles this itself
            WebViewToHostMessage::Ready => {}
            WebViewToHostMessage::SetUiRects(ui_rects) => {
                self.ui_rects = ui_rects;
            }
            WebViewToHostMessage::SetIsChatFocused(is_chat_focused) => {
                self.is_chat_focused = is_chat_focused;
            }
            WebViewToHostMessage::SetIsMenuOpen(is_menu_open) => {
                self.is_menu_open = is_menu_open;
            }
            WebViewToHostMessage::GuiHandler(gui_triggered_handler) => {
                let _ = self.gui_handler_trigger_tx.send(gui_triggered_handler);
            }
            WebViewToHostMessage::SendGameChatMessage { message } => self.game_chat.send_message(message),
            WebViewToHostMessage::SetVolume(volume) => {
                let _ = self.volume_tx.send(volume);
            }
            // AppRunner handles these itself
            WebViewToHostMessage::SetGraphicsQuality(_) | WebViewToHostMessage::LeaveGame => {}
        }
    }

    pub fn onInputDeviceEvent(&mut self, event: &DeviceEvent) {
        match event {
            DeviceEvent::MouseMotion { delta } => {
                if std::mem::take(&mut self.should_skip_next_mouse_motion) {
                    return;
                }
                // orient camera, only if right mouse is down or a script locked the mouse
                if self.camera_turner != Some(CameraTurner::RightMouseButton) && !self.is_mouse_locked {
                    return;
                }
                self.last_input_at_tx.send_replace(Instant::now());
                let (delta_x, delta_y) = *delta;
                self.rotate_camera_by_drag(delta_x as f32, delta_y as f32);
            }
            _ => {}
        }
    }

    // A game can end while the camera turns, and the pointer must not stay locked after it.
    pub fn cleanup(&mut self, window: &Window) {
        self.set_pointer_lock_screen_position(None, window);
        set_window_mouse_cursor(window, None);
        self.kill_play_cancel_token.cancel();
    }
}
