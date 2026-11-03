use crate::{
    avatar::{AvatarData, AvatarFraming, face_framed_bounds, paint_avatar, parse_hex_color},
    stored_world_renderable_impl::StoredWorldRenderable,
};
use glam::{Mat4, Vec3};
use render_core::{
    camera::{CAMERA_NEAR_PLANE, DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    computePropertyValue::{applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime},
    computeSkeletonPose::computeSkeletonPoseOfIdAtTime,
    create_gpu_primitive_shape::label_image_geometry,
    create_gpu_texture::{choose_texture_upload_format, mip_level_byte_count},
    dehydrated_gltf::DehydratedGltf,
    dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world,
    material_to_stored_world::material_to_stored_world,
    picking::pick_scene_object,
    preview_lighting::add_neutral_preview_lighting,
    render_scene::{
        LabelContent, RenderGeometry, RenderScene, build_render_scene, collect_material_texture_data_ids, collect_mesh_data_ids,
        world_lighting_info_of_render_object_of_id,
    },
    renderable::{RenderSnapshot, Renderable},
    renderer::{RenderFrame, Renderer},
    renderer_types::RenderObject,
    skybox_to_stored_world::skybox_to_stored_world,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use game_types::{
    consts::{ANIMATOR_WEIGHT_MAX, DEHYDRATED_GLTF_CURRENT_VERSION, EDITOR_WORLD_PROTOCOL_VERSION, MESH_DATA_CURRENT_VERSION, STORED_WORLD_CURRENT_VERSION},
    data_types::{DataId, MeshData},
    editor_sync::{EditorServerDelta, EditorState, EditorStatus},
    migrateDehydratedGltf,
    migrateStoredWorld::{self, StoredWorldKind},
    object_types::{InterpolationStyle, ObjectId, ObjectInfo, ObjectLocation, VisualSettings},
    stored_world_type::{StoredMaterial, StoredWorld},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

// The editor stream owns the authoritative world; React overlays local property edits and script drafts.
#[wasm_bindgen]
pub struct BrowserEditorWorld {
    world: StoredWorldRenderable,
}
#[wasm_bindgen]
impl BrowserEditorWorld {
    pub fn from_json(json: &str) -> Result<Self, JsValue> {
        let state: EditorState = serde_json::from_str(json).map_err(|error| JsValue::from_str(&error.to_string()))?;
        if state.protocol_version != EDITOR_WORLD_PROTOCOL_VERSION {
            return Err(JsValue::from_str("Unsupported editor sync protocol version"));
        }
        let world = StoredWorldRenderable::new(state.world).map_err(|error| JsValue::from_str(&error.to_string()))?;
        Ok(Self { world })
    }

    // WebSocket batches use the same Rust delta types as the server; JavaScript only passes the bytes through.
    // The answer is serialized straight from its types: going through serde_json::Value would widen every
    // f32 to f64 and hand 1.1 back as 1.100000023841858, which no longer matches what was typed.
    pub fn apply_deltas(&mut self, bytes: &[u8]) -> Result<String, JsValue> {
        let delta: EditorServerDelta = bincode::deserialize(bytes).map_err(|error| JsValue::from_str(&error.to_string()))?;
        let did_world_change = !delta.object_deltas.is_empty() || !delta.script_raw_code_of_id.is_empty();
        self.world
            .apply_object_deltas(delta.object_deltas, delta.script_raw_code_of_id)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        // Status-only messages keep React's world reference and the rendered scene intact.
        let world = if did_world_change { Some(self.world.stored_world()) } else { None };
        serde_json::to_string(&BrowserEditorDelta {
            seq: delta.seq,
            world,
            editor_status: delta.editor_status,
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

#[derive(Serialize)]
struct BrowserEditorDelta<'a> {
    seq: u64,
    world: Option<&'a StoredWorld>,
    editor_status: EditorStatus,
}

// Both ways a browser can refuse to draw, no canvas context and no adapter, show this.
const NO_GRAPHICS_CARD_ERROR: &str =
    "Your browser did not give this page a graphics card. Turn on graphics acceleration in your browser settings, then reload the page.";

// BrowserSurface owns the canvas surface and the shared core renderer. Scene logic stays in render_core.
struct BrowserSurface {
    wgpu_surface: wgpu::Surface<'static>,
    core_renderer: Renderer,
    surface_config: wgpu::SurfaceConfiguration,
}
impl BrowserSurface {
    async fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        // Both builds run this. Only the WebGL2 build has wgpu's GL backend, which this uses when WebGPU gives no adapter.
        let instance = wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..Default::default()
        })
        .await;
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let wgpu_surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|_| JsValue::from_str(NO_GRAPHICS_CARD_ERROR))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&wgpu_surface),
                ..Default::default()
            })
            .await
            .map_err(|_| JsValue::from_str(NO_GRAPHICS_CARD_ERROR))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("kube WebGPU preview"),
                required_features: adapter.features() & (wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC),
                // WebGL2 cannot meet wgpu's default limits, since it has no compute shaders or storage buffers.
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        device.on_uncaptured_error(std::sync::Arc::new(|error| web_sys::console::error_1(&JsValue::from_str(&error.to_string()))));
        let mut surface_config = wgpu_surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| JsValue::from_str("No compatible WebGPU canvas format"))?;
        // Render linear shader output through an sRGB view, just as the native renderer does.
        let format = surface_config.format.add_srgb_suffix();
        surface_config.view_formats = vec![format];
        // A WebGPU canvas takes premultiplied alpha. wgpu offers a WebGL2 canvas only as opaque, but it leaves the canvas's
        // own alpha on, so a see-through preview still shows the page behind it.
        surface_config.alpha_mode = match adapter.get_info().backend {
            wgpu::Backend::BrowserWebGpu => wgpu::CompositeAlphaMode::PreMultiplied,
            _ => wgpu::CompositeAlphaMode::Opaque,
        };
        wgpu_surface.configure(&device, &surface_config);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let core_renderer = Renderer::new(device, queue, format, width, height).map_err(|error| JsValue::from_str(&error.to_string()))?;
        if let Some(error) = core_renderer.device().pop_error_scope().await {
            return Err(JsValue::from_str(&error.to_string()));
        }
        Ok(Self {
            wgpu_surface,
            core_renderer,
            surface_config,
        })
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        if width == 0 || height == 0 {
            return Err(JsValue::from_str("Canvas dimensions must be positive"));
        }
        let limit = self.core_renderer.device().limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(JsValue::from_str("Canvas exceeds the GPU texture size limit"));
        }
        if width != self.surface_config.width || height != self.surface_config.height {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.wgpu_surface.configure(self.core_renderer.device(), &self.surface_config);
            self.core_renderer.resize(width, height);
        }
        Ok(())
    }
    fn begin_frame(&mut self) -> Result<(wgpu::SurfaceTexture, RenderFrame), JsValue> {
        let output = self.wgpu_surface.get_current_texture().map_err(|error| JsValue::from_str(&error.to_string()))?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.surface_config.format.add_srgb_suffix()),
            ..Default::default()
        });
        let frame = self.core_renderer.begin_frame(view);
        Ok((output, frame))
    }
}
// Release the device when React unmounts its canvas.
impl Drop for BrowserSurface {
    fn drop(&mut self) {
        self.core_renderer.device().destroy();
    }
}

// BrowserWgpuRenderContext shares the browser surface and GPU assets. Each BrowserWgpuRenderer owns its scene and camera.
#[wasm_bindgen]
pub struct BrowserWgpuRenderContext {
    browser_surface: BrowserSurface,
    mesh_data_arc_of_id: HashMap<DataId, Arc<MeshData>>,
}

#[wasm_bindgen]
impl BrowserWgpuRenderContext {
    pub async fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        Ok(Self {
            browser_surface: BrowserSurface::new(canvas).await?,
            mesh_data_arc_of_id: HashMap::new(),
        })
    }

    pub fn load_mesh_bytes_into_gpu(&mut self, data_id: i64, mesh_bytes: &[u8]) -> Result<(), JsValue> {
        // Stored mesh bytes contain a u64 version followed by MeshData. Check the header before decoding the payload.
        let mut remaining_bytes = mesh_bytes;
        let version: u64 = bincode::deserialize_from(&mut remaining_bytes).map_err(|error| JsValue::from_str(&error.to_string()))?;
        if version != MESH_DATA_CURRENT_VERSION {
            return Err(JsValue::from_str(&format!(
                "Unsupported data version: expected {}, got {}",
                MESH_DATA_CURRENT_VERSION, version
            )));
        }
        let mesh_data = bincode::deserialize_from(&mut remaining_bytes).map_err(|error| JsValue::from_str(&error.to_string()))?;
        let mesh_data_arc = Arc::new(mesh_data);
        self.browser_surface.core_renderer.load_mesh_into_gpu(data_id, &mesh_data_arc);
        self.mesh_data_arc_of_id.insert(data_id, mesh_data_arc);
        Ok(())
    }
    // The browser's Basis transcoder writes each texture straight into this GPU format, named as WebGPU names it.
    pub fn texture_upload_format(&self, width: u32, height: u32, is_srgb: bool, has_alpha: bool) -> String {
        match choose_texture_upload_format(self.browser_surface.core_renderer.device().features(), width, height, is_srgb, has_alpha) {
            wgpu::TextureFormat::Bc1RgbaUnorm => "bc1-rgba-unorm",
            wgpu::TextureFormat::Bc7RgbaUnorm => "bc7-rgba-unorm",
            wgpu::TextureFormat::Astc { .. } => "astc-4x4-unorm",
            _ => "rgba8unorm",
        }
        .to_string()
    }
    // The browser texture decoder supplies five little-endian u32s followed by every mip level in texture_upload_format.
    // Preserve the stored color space: normal/roughness/metallic textures must not be decoded as sRGB.
    pub fn load_texture_bytes_into_gpu(&mut self, data_id: i64, texture_bytes: &[u8]) -> Result<(), JsValue> {
        if texture_bytes.len() < 20 {
            return Err(JsValue::from_str("Truncated render texture"));
        }
        let mut header = texture_bytes[..20].chunks_exact(4).map(|part| u32::from_le_bytes(part.try_into().unwrap()));
        let width = header.next().unwrap();
        let height = header.next().unwrap();
        let mip_level_count = header.next().unwrap();
        let srgb_flag = header.next().unwrap();
        let alpha_flag = header.next().unwrap();
        if width == 0 || height == 0 || mip_level_count == 0 || mip_level_count > 32 || srgb_flag > 1 || alpha_flag > 1 {
            return Err(JsValue::from_str("Invalid render texture metadata"));
        }
        let upload_format = choose_texture_upload_format(
            self.browser_surface.core_renderer.device().features(),
            width,
            height,
            srgb_flag == 1,
            alpha_flag == 1,
        );
        let mut offset = 20usize;
        let mut mip_data = Vec::new();
        for level in 0..mip_level_count {
            let size = mip_level_byte_count(upload_format, width, height, level).ok_or_else(|| JsValue::from_str("Render texture is too large"))?;
            let end = offset.checked_add(size).ok_or_else(|| JsValue::from_str("Render texture is too large"))?;
            let mip_level_data = texture_bytes
                .get(offset..end)
                .ok_or_else(|| JsValue::from_str("Truncated render texture mip"))?;
            mip_data.push(mip_level_data.to_vec());
            offset = end;
        }
        if offset != texture_bytes.len() {
            return Err(JsValue::from_str("Unexpected render texture bytes"));
        }
        self.browser_surface
            .core_renderer
            .load_texture_into_gpu(data_id, &mip_data, width, height, upload_format, srgb_flag == 1)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

#[wasm_bindgen]
pub struct BrowserWgpuRenderer {
    stored_world_renderable: StoredWorldRenderable,
    base_render_snapshot: RenderSnapshot,
    frame_render_object_of_id: HashMap<ObjectId, RenderObject>,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    initial_scene_bounds: Option<(Vec3, Vec3)>,
    avatar_framing: Option<AvatarFraming>,
    is_in_rest_pose: bool,
    displayed_scene: Option<(RenderScene, Mat4)>,
    camera_position: Vec3,
}

// Expose animator controls, identified by animator object IDs, so duplicate clip names and multiple skeletons stay independent.
#[wasm_bindgen(getter_with_clone)]
pub struct BrowserSkAnimatorInfo {
    pub animator_id: usize,
    pub skeleton_id: usize,
    pub clock_id: usize,
    pub animation_name: String,
    pub target_weight: f32,
}

impl BrowserWgpuRenderer {
    fn rebuild_render_snapshot(&mut self) {
        self.base_render_snapshot = self.stored_world_renderable.getRenderSnapshot();
    }

    fn from_stored_world(render_context: &BrowserWgpuRenderContext, mut stored: StoredWorld) -> Result<Self, JsValue> {
        StoredWorldRenderable::restart_preview_timeline(&mut stored).map_err(|error| JsValue::from_str(&error.to_string()))?;
        let camera_position = stored.fallback_camera_location.position;
        let stored_world_renderable = StoredWorldRenderable::new(stored).map_err(|error| JsValue::from_str(&error.to_string()))?;
        let base_render_snapshot = stored_world_renderable.getRenderSnapshot();
        let (camera_buffer, camera_bind_group) =
            render_context
                .browser_surface
                .core_renderer
                .create_camera_matrix_buffer_pair(Vec3::Z, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
        Ok(Self {
            stored_world_renderable,
            frame_render_object_of_id: base_render_snapshot.render_object_of_id.clone(),
            base_render_snapshot,
            camera_buffer,
            camera_bind_group,
            initial_scene_bounds: None,
            avatar_framing: None,
            is_in_rest_pose: false,
            displayed_scene: None,
            camera_position,
        })
    }
}

// THIS IS THE MAIN ENTRYPOINT
#[wasm_bindgen]
impl BrowserWgpuRenderer {
    pub fn from_asset_data_json(render_context: &BrowserWgpuRenderContext, asset_data_json: &str, asset_name: &str) -> Result<Self, JsValue> {
        // Deserialize the existing asset variants directly; each becomes the same render source.
        #[derive(serde::Deserialize)]
        enum AssetRenderData {
            Object(serde_json::Value),
            Mesh(serde_json::Value),
            Material(serde_json::Value),
            Skybox(DataId),
            Avatar(AvatarData),
        }

        let mut avatar_framing = None;
        let mut is_in_rest_pose = false;
        let mut stored = match serde_json::from_str(asset_data_json).map_err(|error| JsValue::from_str(&error.to_string()))? {
            // An asset object saved at an older world schema version is brought up to date the same way a saved world is.
            AssetRenderData::Object(mut stored_world_json) => {
                migrateStoredWorld::updateToVersion(&mut stored_world_json, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::AssetObject)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                serde_json::from_value::<StoredWorld>(stored_world_json).map_err(|error| JsValue::from_str(&error.to_string()))?
            }
            AssetRenderData::Mesh(mut mesh_json) => {
                migrateDehydratedGltf::updateToVersion(&mut mesh_json, DEHYDRATED_GLTF_CURRENT_VERSION)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                let gltf: DehydratedGltf = serde_json::from_value(mesh_json).map_err(|error| JsValue::from_str(&error.to_string()))?;
                dehydrated_gltf_to_stored_world(asset_name, gltf).map_err(|error| JsValue::from_str(&error.to_string()))?
            }
            AssetRenderData::Material(mut stored_material_json) => {
                migrateStoredWorld::updateStoredMaterialToVersion(&mut stored_material_json, STORED_WORLD_CURRENT_VERSION)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                let StoredMaterial { material, .. } = serde_json::from_value(stored_material_json).map_err(|error| JsValue::from_str(&error.to_string()))?;
                material_to_stored_world(asset_name, material)
            }
            AssetRenderData::Skybox(texture_data_id) => skybox_to_stored_world(asset_name, texture_data_id),
            AssetRenderData::Avatar(avatar) => {
                is_in_rest_pose = avatar.is_in_rest_pose;
                let stored = avatar.into_stored_world().map_err(|error| JsValue::from_str(&error.to_string()))?;
                avatar_framing = AvatarFraming::of(&stored);
                stored
            }
        };
        add_neutral_preview_lighting(&mut stored);
        let mut renderer = Self::from_stored_world(render_context, stored)?;
        renderer.avatar_framing = avatar_framing;
        renderer.is_in_rest_pose = is_in_rest_pose;
        Ok(renderer)
    }

    pub fn from_world_json(render_context: &BrowserWgpuRenderContext, world_json: &str) -> Result<Self, JsValue> {
        let stored = serde_json::from_str(world_json).map_err(|error| JsValue::from_str(&error.to_string()))?;
        Self::from_stored_world(render_context, stored)
    }

    // Replace only scene data after streamed changes or local edits; keep GPU resources and the camera alive.
    pub fn set_world(&mut self, json: &str) -> Result<(), JsValue> {
        let mut stored = serde_json::from_str(json).map_err(|error| JsValue::from_str(&error.to_string()))?;
        StoredWorldRenderable::restart_preview_timeline(&mut stored).map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.stored_world_renderable = StoredWorldRenderable::new(stored).map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    // An avatar takes each color as it is picked, keeping its GPU resources and camera.
    pub fn set_avatar_color(&mut self, color: &str) -> Result<(), JsValue> {
        let to_js_error = |error: anyhow::Error| JsValue::from_str(&error.to_string());
        let mut stored = self.stored_world_renderable.stored_world().clone();
        paint_avatar(&mut stored, parse_hex_color(color).map_err(to_js_error)?).map_err(to_js_error)?;
        self.stored_world_renderable = StoredWorldRenderable::new(stored).map_err(to_js_error)?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    // The editor starts at the preview's framed position, then moves its camera independently.
    pub fn camera_position(&self) -> Vec<f32> {
        self.camera_position.to_array().to_vec()
    }

    // React requests framing only for Explorer activation; groups include their rendered descendants.
    pub fn camera_position_for_object(
        &self,
        render_context: &BrowserWgpuRenderContext,
        object_id: usize,
        aspect: f32,
        yaw: f32,
        pitch: f32,
    ) -> Option<Vec<f32>> {
        self.displayed_scene.as_ref()?;
        let ids = self.stored_world_renderable.object_and_descendant_ids(object_id);
        // Invisible objects, like colliders, are framed as if they were drawn, so they can still be found.
        let mut render_object_of_id = self.frame_render_object_of_id.clone();
        for id in &ids {
            if let Some(visual_settings) = render_object_of_id.get_mut(id).and_then(visual_settings_mut) {
                visual_settings.opacity = 1.0;
            }
        }
        let poses = computeSkeletonPoseOfIdAtTime(&render_object_of_id, 0.0, Some(0.0));
        let scene = build_render_scene(&render_object_of_id, &render_context.mesh_data_arc_of_id, Some(&poses));
        let bounds = scene_bounds(&scene, &render_context.mesh_data_arc_of_id, Some(&ids))?;
        Some(camera_position_for_bounds(bounds, aspect, yaw, pitch, 1.0).to_array().to_vec())
    }

    // Screenshots render the full editor world from the camera requested by the tool.
    pub fn render_from_camera(
        &mut self,
        render_context: &mut BrowserWgpuRenderContext,
        width: u32,
        height: u32,
        position: &[f32],
        target: &[f32],
        selected_object_ids: &[u32],
    ) -> Result<(), JsValue> {
        if position.len() != 3 || target.len() != 3 {
            return Err(JsValue::from_str("Camera requires two three-dimensional points."));
        }
        let position = Vec3::from_slice(position);
        let target = Vec3::from_slice(target);
        if !position.is_finite() || !target.is_finite() || position.distance_squared(target) <= f32::EPSILON {
            return Err(JsValue::from_str("Invalid camera."));
        }
        render_context.browser_surface.resize(width, height)?;
        self.frame_render_object_of_id.clone_from(&self.base_render_snapshot.render_object_of_id);
        // Preserve the same authored animation frame when switching from the initial orbit to the free camera.
        let overrides = computePropertyOverridesOfObjectIdAtTime(
            &self.base_render_snapshot.property_overrides_of_id,
            &self.base_render_snapshot.rotation_base_degrees_of_id,
            0.0,
            Some(0.0),
        );
        applyPropertyOverridesToRenderObjectOfId(&mut self.frame_render_object_of_id, &overrides, &self.base_render_snapshot.pivot_offset_of_id);
        let poses = computeSkeletonPoseOfIdAtTime(&self.frame_render_object_of_id, 0.0, Some(0.0));
        let mut scene = build_render_scene(&self.frame_render_object_of_id, &render_context.mesh_data_arc_of_id, Some(&poses));
        // Each selected object contributes its subtree to the same outline pass.
        for &object_id in selected_object_ids {
            scene
                .selected_object_ids
                .extend(self.stored_world_renderable.object_and_descendant_ids(object_id as usize));
        }
        let aspect = width as f32 / height as f32;
        let view_proj = build_camera_view_proj_matrix(position, target, aspect, DEFAULT_CAMERA_FOV_DEGREES);
        render_context
            .browser_surface
            .core_renderer
            .update_camera_matrix_buffer(&self.camera_buffer, position, target, aspect, DEFAULT_CAMERA_FOV_DEGREES);
        let (output, mut frame) = render_context.browser_surface.begin_frame()?;
        render_context
            .browser_surface
            .core_renderer
            .render_scene(&mut frame, &scene, None, &self.camera_bind_group, target, position, view_proj);
        render_context.browser_surface.core_renderer.finish_frame(frame);
        output.present();
        self.displayed_scene = Some((scene, view_proj));
        self.camera_position = position;
        Ok(())
    }

    // This is a read-only geometry query. React decides what to do with the returned object ID.
    pub fn pick_object(&self, render_context: &BrowserWgpuRenderContext, ndc_x: f32, ndc_y: f32) -> Option<usize> {
        let (scene, view_proj) = self.displayed_scene.as_ref()?;
        pick_scene_object(scene, &render_context.mesh_data_arc_of_id, *view_proj, ndc_x, ndc_y)
    }

    pub fn mesh_data_ids(&self) -> Vec<i64> {
        collect_mesh_data_ids(&self.base_render_snapshot.render_object_of_id).into_iter().collect()
    }
    pub fn texture_data_ids(&self) -> Vec<i64> {
        let mut data_ids = collect_material_texture_data_ids(&self.base_render_snapshot.render_object_of_id);
        if let Some(data_id) = world_lighting_info_of_render_object_of_id(&self.base_render_snapshot.render_object_of_id).skyboxTextureDataId {
            data_ids.insert(data_id);
        }
        data_ids.into_iter().collect()
    }

    pub fn sk_animators(&self) -> Vec<BrowserSkAnimatorInfo> {
        let object_of_id = self.stored_world_renderable.objects();
        let mut sk_animators = Vec::new();
        for (object_id, object) in object_of_id {
            let ObjectInfo::SkAnimator(animator) = &object.info else { continue };
            let (Some(skeleton_id), Some(animation_id), Some(clock_id)) = (animator.skeletonId, animator.skAnimationId, animator.clockId) else {
                continue;
            };
            let Some(animation_object) = object_of_id.get(&ObjectId::from(animation_id)) else {
                continue;
            };
            sk_animators.push(BrowserSkAnimatorInfo {
                animator_id: *object_id,
                skeleton_id: skeleton_id.into(),
                clock_id: clock_id.into(),
                animation_name: animation_object.name.clone(),
                target_weight: animator.weightTransition.map_or(animator.weight, |transition| transition.finalValue),
            });
        }
        sk_animators.sort_by_key(|animator_info| animator_info.animator_id);
        sk_animators
    }

    // The browser supplies its preview timeline; the control calculations are shared with native scripts.
    pub fn pause_clock(&mut self, clock_id: usize, elapsed_ms: f64) -> Result<(), JsValue> {
        self.stored_world_renderable
            .pause_clock(clock_id, elapsed_ms, Some(elapsed_ms))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    pub fn play_clock(&mut self, clock_id: usize, animation_time_secs: f32, elapsed_ms: f64) -> Result<(), JsValue> {
        self.stored_world_renderable
            .play_clock(clock_id, animation_time_secs, ObjectLocation::Client, elapsed_ms, Some(elapsed_ms))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    pub fn set_clock_speed(&mut self, clock_id: usize, speed: f32, elapsed_ms: f64) -> Result<(), JsValue> {
        self.stored_world_renderable
            .set_clock_speed(clock_id, speed, elapsed_ms, Some(elapsed_ms))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    pub fn smoothly_change_weight(
        &mut self,
        animator_id: usize,
        target_weight: f32,
        duration_ms: f32,
        interpolation_style: String,
        elapsed_ms: f64,
    ) -> Result<(), JsValue> {
        let easing = InterpolationStyle::fromString(&interpolation_style).ok_or_else(|| JsValue::from_str("Unknown interpolation style"))?;
        self.stored_world_renderable
            .smoothly_change_weights(
                [(animator_id, target_weight)],
                duration_ms,
                easing,
                ObjectLocation::Client,
                elapsed_ms,
                Some(elapsed_ms),
            )
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    pub fn select_sk_animator(&mut self, animator_id: usize, duration_ms: f32, elapsed_ms: f64) -> Result<(), JsValue> {
        let sk_animators = self.sk_animators();
        let selected_animator = sk_animators
            .iter()
            .find(|animator_info| animator_info.animator_id == animator_id)
            .ok_or_else(|| JsValue::from_str("Animation was not found"))?;
        let weights = sk_animators
            .iter()
            .filter(|animator_info| animator_info.skeleton_id == selected_animator.skeleton_id)
            .map(|animator_info| {
                let weight = if animator_info.animator_id == animator_id { ANIMATOR_WEIGHT_MAX } else { 0.0 };
                (animator_info.animator_id, weight)
            });
        self.stored_world_renderable
            .smoothly_change_weights(
                weights,
                duration_ms,
                InterpolationStyle::DEFAULT,
                ObjectLocation::Client,
                elapsed_ms,
                Some(elapsed_ms),
            )
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.rebuild_render_snapshot();
        Ok(())
    }

    pub fn render(
        &mut self,
        render_context: &mut BrowserWgpuRenderContext,
        width: u32,
        height: u32,
        elapsed_ms: f64,
        yaw: f32,
        pitch: f32,
        zoom: f32,
        should_draw_sky: bool,
        should_orbit_up_axis: bool,
    ) -> Result<(), JsValue> {
        if !elapsed_ms.is_finite() || !yaw.is_finite() || !pitch.is_finite() || !zoom.is_finite() || zoom <= 0.0 {
            return Err(JsValue::from_str("Invalid preview controls"));
        }
        render_context.browser_surface.resize(width, height)?;
        self.frame_render_object_of_id.clone_from(&self.base_render_snapshot.render_object_of_id);
        let overrides = computePropertyOverridesOfObjectIdAtTime(
            &self.base_render_snapshot.property_overrides_of_id,
            &self.base_render_snapshot.rotation_base_degrees_of_id,
            elapsed_ms,
            Some(elapsed_ms),
        );
        applyPropertyOverridesToRenderObjectOfId(&mut self.frame_render_object_of_id, &overrides, &self.base_render_snapshot.pivot_offset_of_id);
        // With no pose, every part a skeleton moves stays where it was built, in the rest pose.
        let poses = match self.is_in_rest_pose {
            true => HashMap::new(),
            false => computeSkeletonPoseOfIdAtTime(&self.frame_render_object_of_id, elapsed_ms, Some(elapsed_ms)),
        };
        let mut scene = build_render_scene(&self.frame_render_object_of_id, &render_context.mesh_data_arc_of_id, Some(&poses));
        scene.should_draw_sky = should_draw_sky;
        if self.initial_scene_bounds.is_none() {
            let mut bounds_object_of_id = self.base_render_snapshot.render_object_of_id.clone();
            let start_overrides = computePropertyOverridesOfObjectIdAtTime(
                &self.base_render_snapshot.property_overrides_of_id,
                &self.base_render_snapshot.rotation_base_degrees_of_id,
                0.0,
                Some(0.0),
            );
            applyPropertyOverridesToRenderObjectOfId(&mut bounds_object_of_id, &start_overrides, &self.base_render_snapshot.pivot_offset_of_id);
            let start_poses = match self.is_in_rest_pose {
                true => HashMap::new(),
                false => computeSkeletonPoseOfIdAtTime(&bounds_object_of_id, 0.0, Some(0.0)),
            };
            // Frame authored geometry even when its opacity starts at zero and an animation reveals it later.
            for (id, property_overrides) in &self.base_render_snapshot.property_overrides_of_id {
                // Permanent invisible colliders do not affect framing. Only opacity animations can reveal hidden geometry.
                if !property_overrides
                    .computed_property_animations
                    .iter()
                    .any(|(path, _)| path.len() == 1 && path[0] == "opacity")
                {
                    continue;
                }
                let Some(visual_settings) = bounds_object_of_id.get_mut(id).and_then(visual_settings_mut) else {
                    continue;
                };
                visual_settings.opacity = 1.0;
            }
            let bounds_scene = build_render_scene(&bounds_object_of_id, &render_context.mesh_data_arc_of_id, Some(&start_poses));
            let bounds_of = |object_ids: Option<&HashSet<ObjectId>>| scene_bounds(&bounds_scene, &render_context.mesh_data_arc_of_id, object_ids);
            self.initial_scene_bounds = match &self.avatar_framing {
                Some(framing) => bounds_of(Some(&framing.head_ids)).map(|head| face_framed_bounds(head, bounds_of(Some(&framing.torso_ids)))),
                None => bounds_of(None),
            };
        }
        let Some((min, max)) = self.initial_scene_bounds else {
            // New drafts contain only a root group; render the sky until tools add geometry.
            // A skybox has no geometry either, so the camera stays put and the preview controls look around the sky:
            // dragging pulls the sky along, and yaw 0 faces the middle of the panorama.
            let position = self.camera_position;
            let pitch = pitch.clamp(-1.45, 1.45);
            let target = position + Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
            return self.render_from_camera(render_context, width, height, &position.to_array(), &target.to_array(), &[]);
        };
        // Orbiting the up axis turns the camera around the vertical line through the object's origin. The bounds are
        // widened to be even on both sides of that line, so framing centers on it, and a lopsided object like a teapot
        // turns in place instead of swinging around the middle of its bounds.
        let (min, max) = match should_orbit_up_axis {
            true => {
                let half = min.abs().max(max.abs());
                (Vec3::new(-half.x, min.y, -half.z), Vec3::new(half.x, max.y, half.z))
            }
            false => (min, max),
        };
        let target = (min + max) * 0.5;
        let aspect = width as f32 / height as f32;
        let zoom = match self.avatar_framing {
            Some(_) => zoom * FACE_FRAMED_ZOOM,
            None => zoom,
        };
        let camera_position = camera_position_for_bounds((min, max), aspect, yaw, pitch, zoom);
        self.camera_position = camera_position;
        let view_proj = build_camera_view_proj_matrix(camera_position, target, aspect, DEFAULT_CAMERA_FOV_DEGREES);
        render_context.browser_surface.core_renderer.update_camera_matrix_buffer(
            &self.camera_buffer,
            camera_position,
            target,
            aspect,
            DEFAULT_CAMERA_FOV_DEGREES,
        );
        let (output, mut frame) = render_context.browser_surface.begin_frame()?;
        render_context
            .browser_surface
            .core_renderer
            .render_scene(&mut frame, &scene, None, &self.camera_bind_group, target, camera_position, view_proj);
        render_context.browser_surface.core_renderer.finish_frame(frame);
        output.present();
        self.displayed_scene = Some((scene, view_proj));
        Ok(())
    }
}

// Bounds for framing an already resolved scene, including skinned meshes and labels.
fn visual_settings_mut(object: &mut RenderObject) -> Option<&mut VisualSettings> {
    match object {
        RenderObject::Ball(object) => Some(&mut object.visualSettings),
        RenderObject::Brick(object) => Some(&mut object.visualSettings),
        RenderObject::Cylinder(object) => Some(&mut object.visualSettings),
        RenderObject::Mesh(object) => Some(&mut object.visualSettings),
        RenderObject::RenderLabel(object) => Some(&mut object.visualSettings),
        _ => None,
    }
}

fn scene_bounds(scene: &RenderScene, mesh_data_of_id: &HashMap<DataId, Arc<MeshData>>, object_ids: Option<&HashSet<ObjectId>>) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut include = |point: Vec3| {
        if point.is_finite() {
            min = min.min(point);
            max = max.max(point);
        }
    };
    for instance in &scene.instances {
        if object_ids.is_some_and(|ids| !ids.contains(&instance.object_id)) {
            continue;
        }
        let model = Mat4::from_cols_array_2d(&instance.content.model);
        let half = match instance.geometry {
            RenderGeometry::Ball => Vec3::ONE,
            RenderGeometry::Brick => Vec3::splat(0.5),
            RenderGeometry::Cylinder => Vec3::new(1.0, 1.0, 0.5),
            RenderGeometry::Mesh { data_id, skeleton_id } => {
                let Some(mesh) = mesh_data_of_id.get(&data_id) else {
                    continue;
                };
                match mesh.as_ref() {
                    MeshData::StaticMeshData { vertices, .. } => {
                        for vertex in vertices {
                            include(model.transform_point3(vertex.position));
                        }
                    }
                    MeshData::SkinnedMeshData { vertices, .. } => {
                        let joint_matrices =
                            skeleton_id.and_then(|skeleton_id| scene.joint_matrices.iter().find(|(mesh_id, id, _)| *mesh_id == data_id && *id == skeleton_id));
                        for vertex in vertices {
                            let position = if let Some((_, _, joints)) = joint_matrices {
                                let mut position = Vec3::ZERO;
                                for (index, weight) in vertex.skinning.jointIndices.iter().zip(vertex.skinning.jointWeights) {
                                    if weight == 0.0 {
                                        continue;
                                    }
                                    if let Some(matrix) = joints.matrices.get(*index as usize) {
                                        position += Mat4::from_cols_array_2d(matrix).transform_point3(vertex.position) * weight;
                                    }
                                }
                                position
                            } else {
                                vertex.position
                            };
                            include(model.transform_point3(position));
                        }
                    }
                }
                continue;
            }
        };
        for x in [-half.x, half.x] {
            for y in [-half.y, half.y] {
                for z in [-half.z, half.z] {
                    include(model.transform_point3(Vec3::new(x, y, z)));
                }
            }
        }
    }
    for label in &scene.labels {
        if object_ids.is_some_and(|ids| !ids.contains(&label.object_id)) {
            continue;
        }
        match &label.content {
            LabelContent::Text { geometry, instances, .. } => {
                let model = Mat4::from_cols_array_2d(&instances[1].model);
                for vertex in &geometry.vertices {
                    include(model.transform_point3(Vec3::from_array(vertex.position)));
                }
            }
            LabelContent::Image { instance, .. } => {
                let model = Mat4::from_cols_array_2d(&instance.model);
                for vertex in label_image_geometry().vertices.iter() {
                    include(model.transform_point3(Vec3::from_array(vertex.position)));
                }
            }
        }
    }
    if min.is_finite() && max.is_finite() { Some((min, max)) } else { None }
}

// TODO: AI-PICKED-VALUE: camera_position_for_bounds leaves room for a sphere around the bounds, but an avatar framed on its face is
// framed by its height alone. This brings the camera in until that height fills the circle with 5% to spare: 1.05 × cos(35°) / 1.1.
const FACE_FRAMED_ZOOM: f32 = 0.78;

// Fit the bounding sphere inside both viewport axes, preserving the user's viewing angle.
fn camera_position_for_bounds((min, max): (Vec3, Vec3), aspect: f32, yaw: f32, pitch: f32, zoom: f32) -> Vec3 {
    let target = (min + max) * 0.5;
    let radius = (max - min).length() * 0.5;
    let half_fov_radians = DEFAULT_CAMERA_FOV_DEGREES.to_radians() * 0.5;
    let half_angle = half_fov_radians.tan().min(half_fov_radians.tan() * aspect).atan();
    // TODO: AI-PICKED-VALUE: Leave ten percent around the initial bounds; orbit and zoom keep that framing stable as objects animate.
    let distance = (radius / half_angle.sin()).max(radius + CAMERA_NEAR_PLANE) * 1.1 * zoom;
    let pitch = pitch.clamp(-1.45, 1.45);
    target + Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * distance
}

#[cfg(test)]
#[path = "../tests/unit/scene_bounds_tests.rs"]
mod scene_bounds_tests;

#[cfg(test)]
#[path = "../tests/unit/editor_world_deltas_tests.rs"]
mod editor_world_deltas_tests;
