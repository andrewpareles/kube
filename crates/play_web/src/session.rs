use crate::simulation::{HeldMovementKeys, turnedCameraOrientation, walkingVelocity};
use game_types::{
    consts::{CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK, CLIENT_SIM_AND_SEND_SECS, DEFAULT_CAMERA_ZOOM},
    object_types::{ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, Orientation3D, PlayerInfo},
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
    physics::SetPhysicsInfo3D,
    world::{World, WorldControlMode},
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

// -------------------------- One player's session --------------------------

// BrowserPlaySession is the whole of a browser game: one world, one local player, one canvas. The page calls
// render_frame once per animation frame and tells it which keys are held.
#[wasm_bindgen]
pub struct BrowserPlaySession {
    world: World,
    player_object_id: ObjectId,
    // The object the camera follows once the world's Character belongs to the local player.
    head_object_id: Option<ObjectId>,
    camera_orientation: Orientation3D,
    held_keys: HeldMovementKeys,
    // The last simulation boundary that physics reached, so a slow frame catches up and never fast-forwards.
    prev_live_client_sim_ms: f64,
    num_physics_steps_taken: u64,
    last_frame_ms: f32,
    surface: BrowserSurface,
    camera_matrix_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
}

#[wasm_bindgen]
impl BrowserPlaySession {
    pub async fn new(canvas: HtmlCanvasElement, world_json: &str) -> Result<Self, JsValue> {
        let mut world = World::fromStoredWorldString(world_json, WorldControlMode::Editor, None).map_err(to_js_error)?;
        let player_object_id = world.addObject(
            "Player".to_string(),
            ObjectBeingAdded::Player(PlayerInfo {
                username: "Player".to_string(),
                userid: None,
                preferredColor: None,
                equippedItemId: None,
                headObjectId: None,
                characterObjectId: None,
            }),
            world.getRootObjectId(),
        );
        world.worldControlMode = WorldControlMode::Player(player_object_id);
        let head_object_id = giveWorldsCharacterToLocalPlayer(&mut world, player_object_id);
        let camera_orientation = world.fallback_camera_location.orientation;

        let surface = BrowserSurface::new(canvas).await?;
        let (camera_matrix_buffer, camera_bind_group) = surface
            .renderer
            .create_camera_matrix_buffer_pair(Vec3::Z, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);

        Ok(Self {
            world,
            player_object_id,
            head_object_id,
            camera_orientation,
            held_keys: HeldMovementKeys {
                is_forward: false,
                is_backward: false,
                is_left: false,
                is_right: false,
            },
            prev_live_client_sim_ms: 0.0,
            num_physics_steps_taken: 0,
            last_frame_ms: 0.0,
            surface,
            camera_matrix_buffer,
            camera_bind_group,
        })
    }

    // Draws one frame and runs the simulation steps that came due since the last one.
    pub fn render_frame(&mut self, now_ms: f64) -> Result<(), JsValue> {
        // The first frame starts the timeline instead of catching up from zero.
        if self.prev_live_client_sim_ms == 0.0 {
            self.prev_live_client_sim_ms = now_ms;
        }

        // The same accumulator the desktop client uses: a slow frame runs the steps it missed, up to a cap, so a long
        // stall drops the missed time instead of fast-forwarding the world.
        let step_ms = CLIENT_SIM_AND_SEND_SECS * 1000.0;
        let num_steps = World::get_num_physics_steps_to_take(self.prev_live_client_sim_ms, now_ms, step_ms, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK);
        let first_step_sim_ms = now_ms - num_steps.saturating_sub(1) as f64 * step_ms as f64;
        for step_index in 0..num_steps {
            let sim_ms = first_step_sim_ms + step_index as f64 * step_ms as f64;
            self.walkCharacterStep();
            self.world
                .step_physics(1, CLIENT_SIM_AND_SEND_SECS, sim_ms, Some(sim_ms))
                .map_err(to_js_error)?;
            self.num_physics_steps_taken += 1;
        }
        if num_steps > 0 {
            self.prev_live_client_sim_ms = now_ms;
        }

        self.draw(self.prev_live_client_sim_ms)?;
        let frame_end_ms = web_sys::window()
            .and_then(|window| window.performance())
            .map_or(now_ms, |performance| performance.now());
        self.last_frame_ms = (frame_end_ms - now_ms) as f32;
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
}

impl BrowserPlaySession {
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
            .head_object_id
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

// Phase 1 stands in for the game's spawn script. That script gives every player a copy of the world's Character; here
// the one Character in the world becomes this player's body, so a world with a Character can be walked before any
// script exists. A world without one still simulates, it just has nobody to walk.
fn giveWorldsCharacterToLocalPlayer(world: &mut World, player_object_id: ObjectId) -> Option<ObjectId> {
    let character_object_id = *world.getObjectIdsByName("Character").first()?;
    let body_object_id = world
        .getDirectChildrenIds(character_object_id)
        .ok()?
        .iter()
        .copied()
        .find(|child_id| world.getObject(*child_id).is_ok_and(|child| child.name == "Body"))?;
    let head_object_id = world
        .getDirectChildrenIds(body_object_id)
        .ok()
        .and_then(|child_ids| {
            child_ids
                .iter()
                .copied()
                .find(|child_id| world.getObject(*child_id).is_ok_and(|child| child.name == "Head"))
        })
        .unwrap_or(body_object_id);
    let ObjectInfo::Player(player) = &mut world.getObjectMut(player_object_id).ok()?.info else {
        return None;
    };
    player.characterObjectId = Some(ObjectIdPointer(body_object_id));
    player.headObjectId = Some(ObjectIdPointer(head_object_id));
    Some(head_object_id)
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
