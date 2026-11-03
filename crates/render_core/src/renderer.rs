//! The complete GPU renderer used by the desktop app and the browser. Callers supply a drawing target, decoded assets, and resolved scene inputs.
use crate::{
    camera::{CAMERA_NEAR_PLANE, build_camera_view_proj_matrix},
    create_gpu_primitive_shape::{brick_geometry, create_ball_buffers, create_primitive_buffers, cylinder_geometry, label_image_geometry},
    create_gpu_texture::create_gpu_texture_from_mip_data,
    culling::{BoundingSphere, SHADOW_CASTER_BUCKET_COUNT, ShadowCascadeSquares, ShadowCasterBucketStarts, ViewEdges, largest_stretch, shadow_caster_ranges},
    detail_levels::{DetailLevel, bounding_radius_around_origin, build_simpler_detail_levels, detail_level_for_instance},
    render_scene::*,
};
use anyhow::Result;
use bytemuck::Zeroable;
use game_types::{
    data_types::{DataId, MeshData},
    object_types::ObjectId,
};
use glam::{Mat4, Vec3};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Range,
    sync::Arc,
};
use wgpu::util::DeviceExt;
struct RenderTargetConfig {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const DEPTH_CLEAR_VALUE: f32 = 0.0;
const DEPTH_COMPARE_FUNCTION: wgpu::CompareFunction = wgpu::CompareFunction::GreaterEqual;
const SHADOW_DEPTH_CLEAR_VALUE: f32 = 1.0;
const SHADOW_DEPTH_COMPARE_FUNCTION: wgpu::CompareFunction = wgpu::CompareFunction::LessEqual;
const OIT_ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const SELECTION_MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const OIT_WEIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
const AMBIENT_OCCLUSION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const AMBIENT_OCCLUSION_VIEW_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
// The blurred glow keeps faint tails that 8 bits would band.
const BLOOM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
// TODO: AI-PICKED-VALUE: Six halvings from half resolution spread a glow over about a twentieth of the picture.
const BLOOM_LEVEL_COUNT: u32 = 6;
// The sun's glow reaches onto the picture while the sun's middle is within this far past the picture's edges, in the units
// of normalized device coordinates, where the picture spans -1 to 1.
const SUN_GLOW_REACH_NDC: f32 = 1.2;
// Where the glow shaders read the scene's depth. Keep this synced with scene_depth_texture in scene.wgsl and sky_sun.wgsl.
const GLOW_SCENE_DEPTH_BINDING: u32 = 8;
// WebGL2 cannot blend each target its own way, so both OIT targets share this blend. Color adds up, and alpha is
// multiplied by one minus each layer's alpha, which leaves how much of the scene still shows through.
const OIT_BLENDING: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};
const ADDITIVE_BLENDING: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};
// Far samples read coarser levels of the view depth, as in XeGTAO.
// Keep this synced with ambient_occlusion.wgsl VIEW_DEPTH_MIP_COUNT.
const AMBIENT_OCCLUSION_VIEW_DEPTH_MIP_COUNT: u32 = 5;
// Keep this synced with ambient_occlusion.wgsl SKY_VIEW_DEPTH.
const AMBIENT_OCCLUSION_SKY_VIEW_DEPTH: f64 = 1.0e8;
const OPAQUE_MULTISAMPLE_COUNT: u32 = 4; // 4x MSAA
// The sky draws into the same multisampled target as opaque geometry, before the resolve, so that a
// silhouette pixel resolves to a blend of geometry and the sky actually behind it. Drawing the sky
// after the resolve instead would leave those partly covered pixels blended with the clear color and
// would overwrite geometry coverage wherever the pixel center happened to miss, which reads as jagged
// edges against the sky.
const SKY_MULTISAMPLE_STATE: wgpu::MultisampleState = wgpu::MultisampleState {
    count: OPAQUE_MULTISAMPLE_COUNT,
    mask: !0,
    alpha_to_coverage_enabled: false,
};
const MATERIAL_SAMPLER_ANISOTROPY_CLAMP: u16 = 16; // 16x anisotropic filtering
// TODO: AI-PICKED-VALUE: This restores the original near cascade and adds one intermediate step before the old medium cascade.
const SHADOW_CASCADE_HALF_SIZES: [f32; SHADOW_CASCADE_COUNT] = [16.0, 32.0, 64.0, 256.0, 2048.0];
// TODO: AI-PICKED-VALUE: Every cascade is one layer of the same texture array, so they share this size and cascade
// quality changes only because world coverage changes.
const SHADOW_MAP_SIZE: u32 = 2048;
// TODO: AI-PICKED-VALUE: This gives the sun shadow depth range room before and after the scene to avoid clipping tall or animated objects.
const SHADOW_DEPTH_PADDING: f32 = 64.0;
// Every cascade puts its shadow camera this far up-sun and spans twice that in depth, so a caster only the outer
// cascade's box would reach still writes into the near cascades. Sizing the depth range per cascade instead clips
// far casters out of the near cascade the player stands in, and because that cascade box follows the shadow center,
// their shadow peels away as the player walks.
const SHADOW_CASTER_EYE_DISTANCE: f32 = SHADOW_CASCADE_HALF_SIZES[SHADOW_CASCADE_COUNT - 1] + SHADOW_DEPTH_PADDING;
// TODO: AI-PICKED-VALUE: This follows the wgpu shadow example bias for a standard LessEqual shadow depth map.
const SHADOW_DEPTH_BIAS_CONSTANT: i32 = 2;
const SHADOW_DEPTH_BIAS_SLOPE_SCALE: f32 = 2.0;
// TODO: AI-PICKED-VALUE: This offsets shadow receivers by two close-cascade texels toward the sun so low-angle surfaces
// do not shadow themselves. Every cascade uses the same world bias so the same shadow edge does not shift where cascades
// hand off.
const SHADOW_RECEIVER_DEPTH_BIAS_WORLD: f32 = 0.015625;
// TODO: AI-PICKED-VALUE: Six close-cascade texels along the surface normal is enough to stabilize low-angle shadows on
// thin overlapping bricks without making normal sun angles visibly detach.
const SHADOW_RECEIVER_NORMAL_BIAS_WORLD: f32 = 0.046875;
// TODO: AI-PICKED-VALUE: Blurrier than this, shadows stop reading as shapes.
const SUN_SHADOW_SOFTNESS_MAX: f32 = 2.0;
// TODO: AI-PICKED-VALUE: This only switches shadow camera up axes when the sun is close enough to vertical to make look_at_rh unstable.
const SHADOW_CAMERA_UP_AXIS_EPSILON: f32 = 0.0001;

fn identity_joint_matrix_buffer_content() -> JointMatrixBufferContent {
    let identity = Mat4::IDENTITY.to_cols_array_2d();
    JointMatrixBufferContent {
        matrices: [identity; MAX_JOINTS],
    }
}

// ------------------------ CPU-side helpers for structs that hold GPU (wgpu) buffers, textures, etc -----------------------

// never nest these

pub struct GpuGeometryBuffer {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
    pub index_format: wgpu::IndexFormat,
    pub bounding_radius: f32, // how far the farthest vertex is from the geometry's origin, before an instance scales it
    pub simpler_detail_levels: Vec<DetailLevel>, // simpler copies whose triangles follow the full geometry's in index_buffer, simplest last
}

fn shadow_camera_up_for_sun_angle(sun_direction: Vec3) -> Vec3 {
    if sun_direction.cross(Vec3::Y).length_squared() > SHADOW_CAMERA_UP_AXIS_EPSILON {
        return Vec3::Y;
    }
    Vec3::Z
}

fn build_shadow_view_projection(sun_direction: Vec3, snapped_shadow_center: Vec3, shadow_half_size: f32) -> Mat4 {
    let shadow_up = shadow_camera_up_for_sun_angle(sun_direction);
    let eye = snapped_shadow_center + sun_direction * SHADOW_CASTER_EYE_DISTANCE;
    let view = Mat4::look_at_rh(eye, snapped_shadow_center, shadow_up);
    let proj = Mat4::orthographic_rh(
        -shadow_half_size,
        shadow_half_size,
        -shadow_half_size,
        shadow_half_size,
        CAMERA_NEAR_PLANE,
        SHADOW_CASTER_EYE_DISTANCE * 2.0,
    );
    proj * view
}

pub fn build_shadow_cascade_view_projections(sun_direction: Vec3, shadow_center: Vec3) -> [Mat4; SHADOW_CASCADE_COUNT] {
    let shadow_up = shadow_camera_up_for_sun_angle(sun_direction);
    let close_cascade_index = 0;
    let outer_cascade_index = SHADOW_CASCADE_COUNT - 1;
    let close_shadow_world_texel_size = SHADOW_CASCADE_HALF_SIZES[close_cascade_index] * 2.0 / SHADOW_MAP_SIZE as f32;
    let outer_shadow_world_texel_size = SHADOW_CASCADE_HALF_SIZES[outer_cascade_index] * 2.0 / SHADOW_MAP_SIZE as f32;
    let view_for_snap = Mat4::look_at_rh(shadow_center + sun_direction * SHADOW_CASTER_EYE_DISTANCE, shadow_center, shadow_up);

    // Snap once at the outer cascade texel size so every cascade moves together in light space.
    let origin_light_position = view_for_snap.transform_point3(Vec3::ZERO);
    let snapped_origin_light_position = Vec3::new(
        (origin_light_position.x / outer_shadow_world_texel_size).round() * outer_shadow_world_texel_size,
        (origin_light_position.y / outer_shadow_world_texel_size).round() * outer_shadow_world_texel_size,
        origin_light_position.z,
    );
    let center_light_position = view_for_snap.transform_point3(shadow_center);
    let center_snap_offset_light = Vec3::new(
        origin_light_position.x - snapped_origin_light_position.x,
        origin_light_position.y - snapped_origin_light_position.y,
        0.0,
    );
    let snapped_shadow_center_light = center_light_position + center_snap_offset_light;

    std::array::from_fn(|cascade_index| {
        let cascade_shadow_world_texel_size = SHADOW_CASCADE_HALF_SIZES[cascade_index] * 2.0 / SHADOW_MAP_SIZE as f32;
        // Shift each larger cascade by half the texel-size difference so its texel centers land on close-cascade texel centers, not edges.
        let cascade_center_offset_light = Vec3::new(
            (close_shadow_world_texel_size - cascade_shadow_world_texel_size) * 0.5,
            (close_shadow_world_texel_size - cascade_shadow_world_texel_size) * 0.5,
            0.0,
        );
        let snapped_shadow_center = view_for_snap
            .inverse()
            .transform_point3(snapped_shadow_center_light + cascade_center_offset_light);
        build_shadow_view_projection(sun_direction, snapped_shadow_center, SHADOW_CASCADE_HALF_SIZES[cascade_index])
    })
}

struct GpuJointMatrixBuffer {
    joint_matrix_buffer: wgpu::Buffer,
    joint_matrix_bind_group: wgpu::BindGroup, // uniform buffers need a bind group
}

struct GpuMaterialTextures {
    basecolor_texture_gpu: GpuTextureData,
    metallic_texture_gpu: GpuTextureData,
    roughness_texture_gpu: GpuTextureData,
    normal_texture_gpu: GpuTextureData,
}

struct GpuLayerTextures {
    opaque_multisample_color_texture_view: wgpu::TextureView,
    opaque_multisample_depth_texture_view: wgpu::TextureView,
    scene_color_texture_view: wgpu::TextureView,
    oit_accum_texture_view: wgpu::TextureView,
    oit_weight_texture_view: wgpu::TextureView,
    depth_texture_view: wgpu::TextureView,
    oit_composite_bind_group: wgpu::BindGroup,
    ambient_occlusion_view_depth_bind_group: wgpu::BindGroup,
    ambient_occlusion_blur_horizontal_bind_group: wgpu::BindGroup,
    ambient_occlusion_blur_vertical_bind_group: wgpu::BindGroup,
}

// Every render target uses the same dimensions, including captures larger than the window.
struct GpuRenderTargets {
    width: u32,
    height: u32,
    base_layer_textures: GpuLayerTextures,  // typical layer
    front_layer_textures: GpuLayerTextures, // always on top layer
    selection_mask_texture_view: wgpu::TextureView,
    selection_outline_bind_group: wgpu::BindGroup,
    // The layers draw one after the other, so they take turns with one set of ambient occlusion textures.
    ambient_occlusion_view_depth_mip_texture_views: Vec<wgpu::TextureView>,
    ambient_occlusion_view_depth_downsample_bind_groups: Vec<wgpu::BindGroup>,
    ambient_occlusion_bind_group: wgpu::BindGroup,
    ambient_occlusion_noisy_texture_view: wgpu::TextureView,
    ambient_occlusion_blurred_horizontally_texture_view: wgpu::TextureView,
    ambient_occlusion_blurred_texture_view: wgpu::TextureView,
    // Bloom: the glow pass draws only glowing objects and the sun into level 0, half the picture's size, then the smaller
    // levels blur it.
    bloom_level_texture_views: Vec<wgpu::TextureView>,
    // The base layer's depth, which the glow pass tests by hand, since level 0 is too small to use it as its depth buffer.
    glow_scene_depth_bind_group: wgpu::BindGroup,
    // bloom_downsample_bind_groups[level] reads level, which shrinks into level + 1.
    bloom_downsample_bind_groups: Vec<wgpu::BindGroup>,
    // bloom_upsample_bind_groups[level] reads level + 1, which grows back into level.
    bloom_upsample_bind_groups: Vec<wgpu::BindGroup>,
    // Reads level 0 for the base layer's composite, which adds the finished glow to the picture.
    bloom_composite_bind_group: wgpu::BindGroup,
}

struct AmbientOcclusionBindGroupLayouts {
    view_depth: wgpu::BindGroupLayout,
    view_depth_downsample: wgpu::BindGroupLayout,
    ambient_occlusion: wgpu::BindGroupLayout,
    blur: wgpu::BindGroupLayout,
}

// helpers for indexing the above

pub struct GpuTextureData {
    pub _texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    // Material data needs raw channel values even when this image is also used as sRGB color.
    pub linear_texture_view: wgpu::TextureView,
}

// key used to identify a ball instance in the ball instance buffer (material it's made of)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct BallInstanceBufferKey {
    material_texture_data_ids: MaterialTextureDataIds,
    detail_level: usize, // which copy of the ball to draw, see detail_levels.rs
}

// key used to identify a brick instance buffer (material it's made of)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct BrickInstanceBufferKey {
    material_texture_data_ids: MaterialTextureDataIds,
}

// key used to identify a cylinder instance buffer (material it's made of)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CylinderInstanceBufferKey {
    material_texture_data_ids: MaterialTextureDataIds,
}

// key used to identify a mesh instance in the static mesh instance buffer (meshid + material it's made of)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct MeshInstanceBufferKey {
    mesh_data_id: DataId,                              // mesh data id
    material_texture_data_ids: MaterialTextureDataIds, // material texture data ids
    detail_level: usize,                               // which copy of the mesh to draw, see detail_levels.rs
}

// key used to identify a skinned mesh instance in the skinned mesh instance buffer (mesh id + material + Skeleton id)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SkinnedMeshInstanceBufferKey {
    mesh_data_id: DataId,                              // mesh data id
    material_texture_data_ids: MaterialTextureDataIds, // material texture data ids
    skeleton_id: Option<ObjectId>,                     // Skeleton object id
}

// key used to identify a skinned mesh joint matrix buffer (mesh id + Skeleton id)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SkinnedMeshJointMatrixBufferKey {
    mesh_data_id: DataId,  // mesh data id
    skeleton_id: ObjectId, // Skeleton object id
}

// Depth-only passes draw an Opaque material from its vertex positions alone, since its texture cannot cut holes in it. Mask
// and Blend materials keep the full vertex shader, so the fragment shader can read their texture's alpha.
struct DepthOnlyRenderPipelines {
    alpha_tested: [wgpu::RenderPipeline; 2],
    position_only: [wgpu::RenderPipeline; 2],
}

impl DepthOnlyRenderPipelines {
    fn render_pipeline_for_material(&self, material_texture_data_ids: MaterialTextureDataIds) -> &wgpu::RenderPipeline {
        let render_pipelines = match material_texture_data_ids.alpha_mode {
            MATERIAL_ALPHA_MODE_OPAQUE => &self.position_only,
            _ => &self.alpha_tested,
        };
        &render_pipelines[usize::from(material_texture_data_ids.is_double_sided)]
    }
}

// ------------------------------ Actual code ------------------------------

fn material_alpha_mode_buffer_content_of_material_texture_key(material_texture_data_ids: MaterialTextureDataIds) -> MaterialAlphaModeBufferContent {
    MaterialAlphaModeBufferContent {
        alpha_mode: material_texture_data_ids.alpha_mode,
        alpha_cutoff: f32::from_bits(material_texture_data_ids.alpha_cutoff_bits),
        _padding0: 0,
        _padding1: 0,
    }
}

fn should_render_in_transparent_pass(alpha: f32, material: MaterialTextureDataIds) -> bool {
    // Texture alpha can vary across a fully opaque instance, so Blend always uses OIT.
    alpha != 1.0 || material.alpha_mode == MATERIAL_ALPHA_MODE_BLEND
}

// Opaque instances draw nearest first, so the depth test throws away the hidden surfaces behind them before their
// fragment shader runs. The opaque fragment shader blurs the sun's shadow with dozens of taps, so every hidden fragment
// it skips saves a lot of work.
fn sort_instances_front_to_back(instance_indices: &mut [u32], scene_instances: &[RenderInstance], camera_position: Vec3) {
    instance_indices.sort_by_cached_key(|&instance_index| {
        let [x, y, z, _] = scene_instances[instance_index as usize].content.model[3];
        camera_position.distance_squared(Vec3::new(x, y, z)).to_bits()
    });
}

// One key's instances in each of the frame's instance lists, as indices into the scene's instances. Each list leaves out
// the instances its passes cannot show.
#[derive(Default)]
struct InstanceListsOfKey {
    selected: Vec<u32>,
    glowing: Vec<u32>,
    opaque: Vec<u32>,
    transparent: Vec<u32>,
    opaque_always_on_top: Vec<u32>,
    transparent_always_on_top: Vec<u32>,
    shadow_casters: [Vec<u32>; SHADOW_CASTER_BUCKET_COUNT], // in buckets by the cascades they cast into, see culling.rs
}

// Which of the frame's instance lists one instance goes in.
struct InstancePlacement {
    is_in_view: bool,
    is_selected: bool,
    glows: bool,
    is_always_on_top: bool,
    is_transparent: bool,
    has_opaque_blend_pixels: bool,
    shadow_caster_bucket: Option<usize>,
}

impl InstanceListsOfKey {
    fn push(&mut self, instance_index: u32, placement: &InstancePlacement) {
        if placement.is_in_view {
            if placement.is_selected {
                self.selected.push(instance_index);
            }
            if placement.glows {
                self.glowing.push(instance_index);
            }
            let instances = match (placement.is_always_on_top, placement.is_transparent) {
                (false, false) => &mut self.opaque,
                (false, true) => &mut self.transparent,
                (true, false) => &mut self.opaque_always_on_top,
                (true, true) => &mut self.transparent_always_on_top,
            };
            instances.push(instance_index);
            if placement.has_opaque_blend_pixels {
                let opaque_instances = match placement.is_always_on_top {
                    true => &mut self.opaque_always_on_top,
                    false => &mut self.opaque,
                };
                opaque_instances.push(instance_index);
            }
        }
        if let Some(shadow_caster_bucket) = placement.shadow_caster_bucket {
            self.shadow_casters[shadow_caster_bucket].push(instance_index);
        }
    }

    fn len(&self) -> usize {
        [
            &self.selected,
            &self.glowing,
            &self.opaque,
            &self.transparent,
            &self.opaque_always_on_top,
            &self.transparent_always_on_top,
        ]
        .into_iter()
        .chain(&self.shadow_casters)
        .map(Vec::len)
        .sum()
    }
}

// Writes the frame's instances straight into its instance buffer while the buffer is mapped, one after another.
struct FrameInstanceWriter<'a> {
    instance_bytes: &'a mut [u8],
    next_instance: u32,
}

impl FrameInstanceWriter<'_> {
    fn write<'b>(&mut self, instances: impl IntoIterator<Item = &'b InstanceContent>) -> Range<u32> {
        let start = self.next_instance;
        for instance in instances {
            let offset = self.next_instance as usize * size_of::<InstanceContent>();
            self.instance_bytes[offset..offset + size_of::<InstanceContent>()].copy_from_slice(bytemuck::bytes_of(instance));
            self.next_instance += 1;
        }
        start..self.next_instance
    }
}

// Where one key type's instance lists sit in the frame's instance buffer.
struct InstanceRangesOfKey<K> {
    selected: HashMap<K, Range<u32>>,
    glowing: HashMap<K, Range<u32>>,
    opaque: HashMap<K, Range<u32>>,
    transparent: HashMap<K, Range<u32>>,
    opaque_always_on_top: HashMap<K, Range<u32>>,
    transparent_always_on_top: HashMap<K, Range<u32>>,
    shadow_caster_bucket_starts: HashMap<K, ShadowCasterBucketStarts>,
}

impl<K: Copy + Eq + Hash> InstanceRangesOfKey<K> {
    fn write(instance_lists_of_key: &HashMap<K, InstanceListsOfKey>, scene_instances: &[RenderInstance], writer: &mut FrameInstanceWriter) -> Self {
        let mut instance_ranges = Self {
            selected: HashMap::new(),
            glowing: HashMap::new(),
            opaque: HashMap::new(),
            transparent: HashMap::new(),
            opaque_always_on_top: HashMap::new(),
            transparent_always_on_top: HashMap::new(),
            shadow_caster_bucket_starts: HashMap::new(),
        };
        for (key, instance_lists) in instance_lists_of_key {
            let lists = [
                (&instance_lists.selected, &mut instance_ranges.selected),
                (&instance_lists.glowing, &mut instance_ranges.glowing),
                (&instance_lists.opaque, &mut instance_ranges.opaque),
                (&instance_lists.transparent, &mut instance_ranges.transparent),
                (&instance_lists.opaque_always_on_top, &mut instance_ranges.opaque_always_on_top),
                (&instance_lists.transparent_always_on_top, &mut instance_ranges.transparent_always_on_top),
            ];
            for (instance_indices, range_of_key) in lists {
                if !instance_indices.is_empty() {
                    let instances = instance_indices.iter().map(|&instance_index| &scene_instances[instance_index as usize].content);
                    range_of_key.insert(*key, writer.write(instances));
                }
            }
            if instance_lists.shadow_casters.iter().any(|bucket| !bucket.is_empty()) {
                let mut bucket_starts = [writer.next_instance; SHADOW_CASTER_BUCKET_COUNT + 1];
                for (bucket_index, bucket) in instance_lists.shadow_casters.iter().enumerate() {
                    let instances = bucket.iter().map(|&instance_index| &scene_instances[instance_index as usize].content);
                    bucket_starts[bucket_index + 1] = writer.write(instances).end;
                }
                instance_ranges.shadow_caster_bucket_starts.insert(*key, bucket_starts);
            }
        }
        instance_ranges
    }
}

// One label's draw for this frame.
#[derive(Clone)]
struct LabelDraw {
    content: LabelDrawContent,
    first_instance: u32, // where its instances start in the frame's instance buffer
    distance_to_camera_squared: f32,
}

#[derive(Clone)]
enum LabelDrawContent {
    // It has exactly two instances: the background panel first, then the text, so each can carry its own color while sharing
    // one geometry buffer.
    Text {
        geometry_cache_key: u64,
        background_index_count: u32,
        draws_background: bool,
        is_background_opaque: bool,
        is_text_opaque: bool,
    },
    // It has the one instance that sizes the shared label image quad.
    Image {
        texture_data_id: DataId,
        is_opaque: bool,
    },
}

// The uploaded glyph atlas, kept until the client hands over one with a newer version.
struct LabelFontAtlasGpu {
    version: u64,
    bind_group: wgpu::BindGroup,
    _texture: GpuTextureData,
}

fn create_layer_textures(
    device: &wgpu::Device,
    config: &RenderTargetConfig,
    oit_composite_bind_group_layout: &wgpu::BindGroupLayout,
    selection_outline_bind_group_layout: &wgpu::BindGroupLayout,
    ambient_occlusion_bind_group_layouts: &AmbientOcclusionBindGroupLayouts,
    lighting_buffer: &wgpu::Buffer,
    bloom_bind_group_layout: &wgpu::BindGroupLayout,
    bloom_sampler: &wgpu::Sampler,
    glow_scene_depth_bind_group_layout: &wgpu::BindGroupLayout,
) -> GpuRenderTargets {
    fn create_depth_texture(device: &wgpu::Device, config: &RenderTargetConfig, sample_count: u32, label: &str) -> wgpu::TextureView {
        let usage = if sample_count == 1 {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage,
            view_formats: &[],
        });
        depth_texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn create_color_texture_view(
        device: &wgpu::Device,
        config: &RenderTargetConfig,
        format: wgpu::TextureFormat,
        sample_count: u32,
        label: &str,
    ) -> wgpu::TextureView {
        let usage = if sample_count == 1 {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn create_oit_composite_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene_color_texture_view: &wgpu::TextureView,
        oit_accum_texture_view: &wgpu::TextureView,
        oit_weight_texture_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("OIT Composite Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(scene_color_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(oit_accum_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(oit_weight_texture_view),
                },
            ],
        })
    }

    fn create_layer_textures(
        device: &wgpu::Device,
        config: &RenderTargetConfig,
        oit_composite_bind_group_layout: &wgpu::BindGroupLayout,
        ambient_occlusion_bind_group_layouts: &AmbientOcclusionBindGroupLayouts,
        ambient_occlusion_noisy_texture_view: &wgpu::TextureView,
        ambient_occlusion_blurred_horizontally_texture_view: &wgpu::TextureView,
        lighting_buffer: &wgpu::Buffer,
        layer_name: &str,
    ) -> GpuLayerTextures {
        let opaque_multisample_color_label = format!("{} Opaque Multisample Color Texture", layer_name);
        let opaque_multisample_depth_label = format!("{} Opaque Multisample Depth Texture", layer_name);
        let scene_color_label = format!("{} Scene Color Texture", layer_name);
        let oit_accum_label = format!("{} OIT Accum Texture", layer_name);
        let oit_weight_label = format!("{} OIT Weight Texture", layer_name);
        let depth_label = format!("{} Depth Texture", layer_name);

        let opaque_multisample_color_texture_view =
            create_color_texture_view(device, config, config.format, OPAQUE_MULTISAMPLE_COUNT, &opaque_multisample_color_label);
        let opaque_multisample_depth_texture_view = create_depth_texture(device, config, OPAQUE_MULTISAMPLE_COUNT, &opaque_multisample_depth_label);
        let scene_color_texture_view = create_color_texture_view(device, config, config.format, 1, &scene_color_label);
        let oit_accum_texture_view = create_color_texture_view(device, config, OIT_ACCUM_FORMAT, 1, &oit_accum_label);
        let oit_weight_texture_view = create_color_texture_view(device, config, OIT_WEIGHT_FORMAT, 1, &oit_weight_label);
        let depth_texture_view = create_depth_texture(device, config, 1, &depth_label);
        let oit_composite_bind_group = create_oit_composite_bind_group(
            device,
            oit_composite_bind_group_layout,
            &scene_color_texture_view,
            &oit_accum_texture_view,
            &oit_weight_texture_view,
        );
        let ambient_occlusion_view_depth_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Ambient Occlusion View Depth Bind Group"),
            layout: &ambient_occlusion_bind_group_layouts.view_depth,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&depth_texture_view),
            }],
        });
        let create_ambient_occlusion_blur_bind_group = |label: &str, texture_view_to_blur: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &ambient_occlusion_bind_group_layouts.blur,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&depth_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(texture_view_to_blur),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: lighting_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let ambient_occlusion_blur_horizontal_bind_group =
            create_ambient_occlusion_blur_bind_group("Ambient Occlusion Blur Horizontal Bind Group", ambient_occlusion_noisy_texture_view);
        let ambient_occlusion_blur_vertical_bind_group = create_ambient_occlusion_blur_bind_group(
            "Ambient Occlusion Blur Vertical Bind Group",
            ambient_occlusion_blurred_horizontally_texture_view,
        );

        GpuLayerTextures {
            opaque_multisample_color_texture_view,
            opaque_multisample_depth_texture_view,
            scene_color_texture_view,
            oit_accum_texture_view,
            oit_weight_texture_view,
            depth_texture_view,
            oit_composite_bind_group,
            ambient_occlusion_view_depth_bind_group,
            ambient_occlusion_blur_horizontal_bind_group,
            ambient_occlusion_blur_vertical_bind_group,
        }
    }

    let ambient_occlusion_view_depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Ambient Occlusion View Depth Texture"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: AMBIENT_OCCLUSION_VIEW_DEPTH_MIP_COUNT.min(config.width.max(config.height).max(1).ilog2() + 1),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: AMBIENT_OCCLUSION_VIEW_DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let ambient_occlusion_view_depth_mip_texture_views: Vec<wgpu::TextureView> = (0..ambient_occlusion_view_depth_texture.mip_level_count())
        .map(|mip_level| {
            ambient_occlusion_view_depth_texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let ambient_occlusion_view_depth_downsample_bind_groups = ambient_occlusion_view_depth_mip_texture_views
        [..ambient_occlusion_view_depth_mip_texture_views.len() - 1]
        .iter()
        .map(|finer_mip_texture_view| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Ambient Occlusion View Depth Downsample Bind Group"),
                layout: &ambient_occlusion_bind_group_layouts.view_depth_downsample,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: lighting_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(finer_mip_texture_view),
                    },
                ],
            })
        })
        .collect();
    let ambient_occlusion_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Ambient Occlusion Bind Group"),
        layout: &ambient_occlusion_bind_group_layouts.ambient_occlusion,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 2,
                resource: lighting_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&ambient_occlusion_view_depth_texture.create_view(&wgpu::TextureViewDescriptor::default())),
            },
        ],
    });
    let ambient_occlusion_noisy_texture_view = create_color_texture_view(device, config, AMBIENT_OCCLUSION_FORMAT, 1, "Ambient Occlusion Noisy Texture");
    let ambient_occlusion_blurred_horizontally_texture_view =
        create_color_texture_view(device, config, AMBIENT_OCCLUSION_FORMAT, 1, "Ambient Occlusion Blurred Horizontally Texture");
    let ambient_occlusion_blurred_texture_view = create_color_texture_view(device, config, AMBIENT_OCCLUSION_FORMAT, 1, "Ambient Occlusion Blurred Texture");
    let base_layer_textures = create_layer_textures(
        device,
        config,
        oit_composite_bind_group_layout,
        ambient_occlusion_bind_group_layouts,
        &ambient_occlusion_noisy_texture_view,
        &ambient_occlusion_blurred_horizontally_texture_view,
        lighting_buffer,
        "Base Layer",
    );
    let front_layer_textures = create_layer_textures(
        device,
        config,
        oit_composite_bind_group_layout,
        ambient_occlusion_bind_group_layouts,
        &ambient_occlusion_noisy_texture_view,
        &ambient_occlusion_blurred_horizontally_texture_view,
        lighting_buffer,
        "Front Layer",
    );
    let selection_mask_texture_view = create_color_texture_view(device, config, SELECTION_MASK_FORMAT, 1, "Selection Mask Texture");
    let selection_outline_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Selection Outline Bind Group"),
        layout: selection_outline_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&selection_mask_texture_view),
        }],
    });
    // Rounded up, so the glow pass can draw the whole picture at exactly half its size.
    let bloom_width = config.width.div_ceil(2).max(1);
    let bloom_height = config.height.div_ceil(2).max(1);
    let bloom_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Bloom Texture"),
        size: wgpu::Extent3d {
            width: bloom_width,
            height: bloom_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: BLOOM_LEVEL_COUNT.min(bloom_width.max(bloom_height).ilog2() + 1),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: BLOOM_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let bloom_level_texture_views: Vec<wgpu::TextureView> = (0..bloom_texture.mip_level_count())
        .map(|level| {
            bloom_texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let create_bloom_bind_group = |source_texture_view: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Bind Group"),
            layout: bloom_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(bloom_sampler),
                },
            ],
        })
    };
    let bloom_downsample_bind_groups = bloom_level_texture_views[..bloom_level_texture_views.len() - 1]
        .iter()
        .map(create_bloom_bind_group)
        .collect();
    let bloom_upsample_bind_groups = bloom_level_texture_views[1..].iter().map(create_bloom_bind_group).collect();
    let bloom_composite_bind_group = create_bloom_bind_group(&bloom_level_texture_views[0]);
    let glow_scene_depth_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Glow Scene Depth Bind Group"),
        layout: glow_scene_depth_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: GLOW_SCENE_DEPTH_BINDING,
            resource: wgpu::BindingResource::TextureView(&base_layer_textures.depth_texture_view),
        }],
    });
    GpuRenderTargets {
        selection_mask_texture_view,
        selection_outline_bind_group,
        width: config.width,
        height: config.height,
        base_layer_textures,
        front_layer_textures,
        ambient_occlusion_view_depth_mip_texture_views,
        ambient_occlusion_view_depth_downsample_bind_groups,
        ambient_occlusion_bind_group,
        ambient_occlusion_noisy_texture_view,
        ambient_occlusion_blurred_horizontally_texture_view,
        ambient_occlusion_blurred_texture_view,
        bloom_level_texture_views,
        glow_scene_depth_bind_group,
        bloom_downsample_bind_groups,
        bloom_upsample_bind_groups,
        bloom_composite_bind_group,
    }
}

fn create_joint_matrix_buffer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, joint_matrix: JointMatrixBufferContent) -> GpuJointMatrixBuffer {
    let joint_matrix_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Joint Matrices Buffer"),
        contents: bytemuck::cast_slice(&[joint_matrix]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let joint_matrix_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Joint Matrices Bind Group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: joint_matrix_buffer.as_entire_binding(),
        }],
    });

    GpuJointMatrixBuffer {
        joint_matrix_buffer,
        joint_matrix_bind_group,
    }
}

// The lighting bind group holds the ambient occlusion texture, which is sized to the render targets, so it is rebuilt
// whenever they are.
fn create_lighting_bind_group(
    device: &wgpu::Device,
    lighting_bind_group_layout: &wgpu::BindGroupLayout,
    lighting_buffer: &wgpu::Buffer,
    shadow_buffer: &wgpu::Buffer,
    shadow_depth_array_texture_view: &wgpu::TextureView,
    shadow_compare_sampler: &wgpu::Sampler,
    ambient_occlusion_blurred_texture_view: &wgpu::TextureView,
    skybox_texture_view: &wgpu::TextureView,
    skybox_sampler: &wgpu::Sampler,
    skybox_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Lighting Bind Group"),
        layout: lighting_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: lighting_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shadow_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(shadow_depth_array_texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(shadow_compare_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(ambient_occlusion_blurred_texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(skybox_texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(skybox_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: skybox_buffer.as_entire_binding(),
            },
        ],
    })
}

fn create_fallback_gpu_material_textures(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<GpuMaterialTextures> {
    let create_one_pixel_texture = |rgba: Vec<u8>, is_srgb: bool, label: &str| {
        create_gpu_texture_from_mip_data(device, queue, &[rgba], 1, 1, wgpu::TextureFormat::Rgba8Unorm, is_srgb, label)
    };
    let basecolor_texture_gpu = create_one_pixel_texture(vec![255, 255, 255, 255], true, "Fallback Material Basecolor Texture")?;
    let metallic_texture_gpu = create_one_pixel_texture(vec![0, 0, 0, 255], false, "Fallback Material Metallic Texture")?;
    let roughness_texture_gpu = create_one_pixel_texture(vec![128, 128, 128, 255], false, "Fallback Material Roughness Texture")?;
    let normal_texture_gpu = create_one_pixel_texture(vec![128, 128, 255, 255], false, "Fallback Material Normal Texture")?;

    Ok(GpuMaterialTextures {
        basecolor_texture_gpu,
        metallic_texture_gpu,
        roughness_texture_gpu,
        normal_texture_gpu,
    })
}

// ------------------------------ Renderer ------------------------------

// we pass this around when calling render fns
pub struct RenderFrame {
    // The 3D game draws on this
    view: wgpu::TextureView,
    pub encoder: wgpu::CommandEncoder,
    did_render_world: bool,
}

// other helpers
#[derive(Clone, Copy, Debug)]
pub struct SimpleViewportRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl SimpleViewportRegion {
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64 && x < self.x as f64 + self.width as f64 && y >= self.y as f64 && y < self.y as f64 + self.height as f64
    }
}

pub struct Renderer {
    device: wgpu::Device,       // wgpu device
    queue: wgpu::Queue,         // wgpu queue
    config: RenderTargetConfig, // wgpu surface configuration

    render_pipeline_opaque: [wgpu::RenderPipeline; 2],              // scene.wgsl: opaque static entry points
    render_pipeline_opaque_mask: [wgpu::RenderPipeline; 2],         // scene.wgsl: opaque static entry points with alpha-to-coverage
    render_pipeline_opaque_depth_only: DepthOnlyRenderPipelines,    // scene.wgsl: alpha-tested opaque static depth replay entry point
    render_pipeline_transparent: [wgpu::RenderPipeline; 2],         // scene.wgsl: transparent static OIT entry points
    shadow_render_pipeline: DepthOnlyRenderPipelines,               // scene.wgsl: sun shadow static entry points
    skinned_render_pipeline_opaque: [wgpu::RenderPipeline; 2],      // scene.wgsl: opaque skinned entry points
    skinned_render_pipeline_opaque_mask: [wgpu::RenderPipeline; 2], // scene.wgsl: opaque skinned entry points with alpha-to-coverage
    skinned_render_pipeline_opaque_depth_only: DepthOnlyRenderPipelines, // scene.wgsl: alpha-tested opaque skinned depth replay entry point
    skinned_render_pipeline_transparent: [wgpu::RenderPipeline; 2], // scene.wgsl: transparent skinned OIT entry points
    skinned_shadow_render_pipeline: DepthOnlyRenderPipelines,       // scene.wgsl: sun shadow skinned entry points
    skybox_render_pipeline: wgpu::RenderPipeline,                   // skybox.wgsl: equirectangular skybox behind world geometry
    sky_gradient_render_pipeline: wgpu::RenderPipeline,             // sky_gradient.wgsl: three stop sky gradient behind world geometry
    sky_sun_render_pipeline: wgpu::RenderPipeline,                  // sky_sun.wgsl: visible sun disk behind world geometry
    oit_composite_pipeline: wgpu::RenderPipeline,                   // oit_composite.wgsl
    oit_composite_see_through_pipeline: wgpu::RenderPipeline,       // oit_composite.wgsl: a layer's composite with no sky behind it
    label_render_pipeline: wgpu::RenderPipeline,                    // scene.wgsl: unlit, uncalled text entry point
    label_depth_render_pipeline: wgpu::RenderPipeline,              // scene.wgsl: writes a drawn label's depth, so labels behind it lose
    label_image_render_pipeline: wgpu::RenderPipeline,              // scene.wgsl: a LabelImage, unlit like label text
    label_image_depth_render_pipeline: wgpu::RenderPipeline,        // scene.wgsl: writes a drawn LabelImage's depth where it is opaque
    selection_mask_render_pipeline: [wgpu::RenderPipeline; 2],
    skinned_selection_mask_render_pipeline: [wgpu::RenderPipeline; 2],
    label_selection_mask_render_pipeline: wgpu::RenderPipeline,
    label_image_selection_mask_render_pipeline: wgpu::RenderPipeline,
    selection_outline_pipeline: wgpu::RenderPipeline,
    glow_render_pipeline: [wgpu::RenderPipeline; 2],         // scene.wgsl: glowing instances into bloom level 0
    skinned_glow_render_pipeline: [wgpu::RenderPipeline; 2], // scene.wgsl: glowing skinned instances into bloom level 0
    label_glow_render_pipeline: wgpu::RenderPipeline,        // scene.wgsl: glowing label text into bloom level 0
    bloom_downsample_render_pipeline: wgpu::RenderPipeline,  // bloom.wgsl: one bloom level shrunk to the next
    bloom_upsample_render_pipeline: wgpu::RenderPipeline,    // bloom.wgsl: one bloom level grown back onto the one above
    oit_composite_with_bloom_pipeline: wgpu::RenderPipeline, // oit_composite.wgsl: the base layer's composite, adding the finished glow
    sun_glow_render_pipeline: wgpu::RenderPipeline,          // sky_sun.wgsl: the sun's glow into bloom level 0
    ambient_occlusion_view_depth_render_pipeline: wgpu::RenderPipeline,
    ambient_occlusion_view_depth_downsample_render_pipeline: wgpu::RenderPipeline,
    ambient_occlusion_render_pipeline: wgpu::RenderPipeline,
    ambient_occlusion_blur_horizontal_render_pipeline: wgpu::RenderPipeline,
    ambient_occlusion_blur_vertical_render_pipeline: wgpu::RenderPipeline,
    label_geometry_buffer_of_cache_key: HashMap<u64, GpuGeometryBuffer>, // one tessellated text mesh per distinct label
    label_font_atlas_gpu: Option<LabelFontAtlasGpu>,

    // buffers:
    ball_buffer: GpuGeometryBuffer,
    brick_buffer: GpuGeometryBuffer,
    cylinder_buffer: GpuGeometryBuffer,
    label_image_buffer: GpuGeometryBuffer,
    mesh_geometry_buffers: HashMap<DataId, GpuGeometryBuffer>, // mesh url -> buffer
    is_mesh_geometry_buffer_skinned: HashMap<DataId, bool>,    // mesh url -> is skinned

    lighting_buffer: wgpu::Buffer,
    lighting_bind_group: wgpu::BindGroup,
    shadow_camera_buffers: Vec<wgpu::Buffer>,
    shadow_camera_bind_groups: Vec<wgpu::BindGroup>,
    shadow_buffer: wgpu::Buffer,
    shadow_depth_texture_views: Vec<wgpu::TextureView>,
    shadow_depth_array_texture_view: wgpu::TextureView,
    shadow_compare_sampler: wgpu::Sampler,
    skybox_buffer: wgpu::Buffer,

    skinned_mesh_joint_matrix_buffers: HashMap<SkinnedMeshJointMatrixBufferKey, GpuJointMatrixBuffer>, // mesh/Skeleton id -> buffer
    texture_gpu_of_data_id: HashMap<DataId, GpuTextureData>,                                           // texture data id -> uploaded texture
    material_bind_group_of_key: HashMap<MaterialTextureDataIds, wgpu::BindGroup>,                      // material texture key -> bind group
    material_alpha_mode_buffer_of_key: HashMap<MaterialTextureDataIds, wgpu::Buffer>,                  // material texture key -> alpha mode buffer
    skybox_bind_group_of_data_id: HashMap<DataId, wgpu::BindGroup>,                                    // skybox texture data id -> bind group
    label_image_bind_group_of_data_id: HashMap<DataId, wgpu::BindGroup>,                               // label image texture data id -> bind group
    fallback_skybox_texture: GpuTextureData, // transparent, so reflections show the gradient when a world has no skybox
    // The skybox texture the lighting bind group reflects, or None when it has to be rebuilt.
    lighting_bind_group_skybox_data_id: Option<Option<DataId>>,
    fallback_material_textures: GpuMaterialTextures, // default textures used when material textures are missing
    fallback_joint_matrix_buffer: GpuJointMatrixBuffer, // identity matrix if the Skeleton is missing

    // textures:
    render_targets: GpuRenderTargets,

    // bind group layouts are schemas (only needed for certain types of buffers, not vertex/index buffers):
    material_bind_group_layout: wgpu::BindGroupLayout,
    material_sampler: wgpu::Sampler,
    skybox_bind_group_layout: wgpu::BindGroupLayout,
    skybox_sampler: wgpu::Sampler,
    label_image_sampler: wgpu::Sampler,
    label_image_alpha_mode_buffer: wgpu::Buffer, // fs_label_image never reads it, but the material bind group layout requires it
    joint_matrix_bind_group_layout: wgpu::BindGroupLayout,
    oit_composite_bind_group_layout: wgpu::BindGroupLayout,
    selection_outline_bind_group_layout: wgpu::BindGroupLayout,
    bloom_bind_group_layout: wgpu::BindGroupLayout,
    bloom_sampler: wgpu::Sampler,
    glow_scene_depth_bind_group_layout: wgpu::BindGroupLayout,
    camera_bind_group_layout: wgpu::BindGroupLayout,
    lighting_bind_group_layout: wgpu::BindGroupLayout,
    ambient_occlusion_bind_group_layouts: AmbientOcclusionBindGroupLayouts,
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat, width: u32, height: u32) -> Result<Self> {
        fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<VertexContent>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            }
        }

        fn skinned_vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SkinnedVertexContent>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &SKINNED_VERTEX_ATTRIBUTES,
            }
        }

        fn instance_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<InstanceContent>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            }
        }

        let config = RenderTargetConfig {
            width: width.max(1),
            height: height.max(1),
            format,
        };
        // ------------ bind_group_layouts ------------
        // eg @group(0) @binding(0) var<uniform> camera: Camera;
        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let lighting_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Lighting Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        sample_type: wgpu::TextureSampleType::Depth,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let joint_matrix_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Joint Matrices Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let material_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Material Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let skybox_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Skybox Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        // Texture uploads already build mip chains, so anisotropic filtering can use those mip levels to keep angled surfaces sharper.
        let material_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Material Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: MATERIAL_SAMPLER_ANISOTROPY_CLAMP,
            ..Default::default()
        });
        // A label image shows its picture once, so its edges clamp instead of blending in the opposite edge.
        let label_image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Label Image Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: MATERIAL_SAMPLER_ANISOTROPY_CLAMP,
            ..Default::default()
        });
        let label_image_alpha_mode_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Label Image Alpha Mode Buffer"),
            contents: bytemuck::bytes_of(&MaterialAlphaModeBufferContent {
                alpha_mode: MATERIAL_ALPHA_MODE_OPAQUE,
                alpha_cutoff: 0.0,
                _padding0: 0,
                _padding1: 0,
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let skybox_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Skybox Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: MATERIAL_SAMPLER_ANISOTROPY_CLAMP,
            ..Default::default()
        });
        let oit_composite_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("OIT Composite Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                },
            ],
        });

        let selection_outline_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Selection Outline Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                },
                count: None,
            }],
        });
        let ambient_occlusion_float_texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
        };
        let ambient_occlusion_depth_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
        };
        let ambient_occlusion_lighting_entry = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let ambient_occlusion_bind_group_layouts = AmbientOcclusionBindGroupLayouts {
            view_depth: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Ambient Occlusion View Depth Bind Group Layout"),
                entries: &[ambient_occlusion_depth_entry],
            }),
            view_depth_downsample: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Ambient Occlusion View Depth Downsample Bind Group Layout"),
                entries: &[ambient_occlusion_lighting_entry, ambient_occlusion_float_texture_entry(3)],
            }),
            ambient_occlusion: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Ambient Occlusion Bind Group Layout"),
                entries: &[ambient_occlusion_lighting_entry, ambient_occlusion_float_texture_entry(4)],
            }),
            blur: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Ambient Occlusion Blur Bind Group Layout"),
                entries: &[
                    ambient_occlusion_depth_entry,
                    ambient_occlusion_float_texture_entry(1),
                    ambient_occlusion_lighting_entry,
                ],
            }),
        };

        // ------------  Render pipelines ------------
        // Store [single-sided, double-sided] for every material pass, retaining hardware culling by default.
        let create_material_render_pipelines = |mut descriptor: wgpu::RenderPipelineDescriptor<'_>| {
            let single_sided = device.create_render_pipeline(&descriptor);
            descriptor.primitive.cull_mode = None;
            let double_sided = device.create_render_pipeline(&descriptor);
            [single_sided, double_sided]
        };
        // Each depth-only pipeline gets a copy for Opaque materials, which runs only the position steps of the vertex shader and
        // has no fragment stage.
        let create_depth_only_render_pipelines = |position_only_vertex_entry_point: &'static str, descriptor: wgpu::RenderPipelineDescriptor<'_>| {
            let mut position_only_descriptor = descriptor.clone();
            position_only_descriptor.vertex.entry_point = Some(position_only_vertex_entry_point);
            position_only_descriptor.fragment = None;
            DepthOnlyRenderPipelines {
                alpha_tested: create_material_render_pipelines(descriptor),
                position_only: create_material_render_pipelines(position_only_descriptor),
            }
        };
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Scene Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &material_bind_group_layout,
                &joint_matrix_bind_group_layout,
                &lighting_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });
        // Cutout coverage rides in the fragment shader's sample_mask output rather than in
        // alpha_to_coverage_enabled, so the color target's alpha channel stays free to mean "opaque".
        // See the alpha-masked entry points in scene.wgsl for why sharing it broke the OIT composite.
        let create_opaque_render_pipeline = |label: &'static str, fragment_entry_point: &'static str| {
            create_material_render_pipelines(wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some("vs_main_static"),
                    buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: DEPTH_COMPARE_FUNCTION,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: OPAQUE_MULTISAMPLE_COUNT,
                    ..Default::default()
                },
                multiview: None,
                cache: None,
            })
        };
        // WebGL2 has no sample mask, so the WebGL build cuts out whole pixels instead.
        let mask_fragment_entry_point = if cfg!(feature = "webgl") {
            "fs_main_mask_whole_pixels"
        } else {
            "fs_main_mask"
        };
        let render_pipeline_opaque = create_opaque_render_pipeline("Opaque Render Pipeline", "fs_main");
        let render_pipeline_opaque_mask = create_opaque_render_pipeline("Opaque Mask Render Pipeline", mask_fragment_entry_point);
        let render_pipeline_opaque_depth_only_descriptor = wgpu::RenderPipelineDescriptor {
            label: Some("Opaque Depth Replay Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main_static"),
                buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_opaque_depth_only"),
                targets: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        };
        let render_pipeline_opaque_depth_only = create_depth_only_render_pipelines("vs_position_only_static", render_pipeline_opaque_depth_only_descriptor);
        let render_pipeline_transparent = create_material_render_pipelines(wgpu::RenderPipelineDescriptor {
            label: Some("Transparent Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main_static"),
                buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_oit_transparent"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: OIT_ACCUM_FORMAT,
                        blend: Some(OIT_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: OIT_WEIGHT_FORMAT,
                        blend: Some(OIT_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Labels reuse the static vertex path (billboarding lives in build_model_matrix) but need their own
        // fragment entry point to stay unlit, and no back face culling: a billboard that ends up mirrored
        // should still show its text rather than vanish.
        let create_label_render_pipeline = |label: &'static str, fragment_entry_point: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some("vs_main_static"),
                    buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    // Labels test depth so world geometry hides them, but this draw never writes it. Writing it made
                    // two label surfaces at nearly the same depth fight over the same pixels, which is exactly what
                    // a panel and the glyphs sitting on it are. Ordering inside one label comes from its draw order.
                    // Between labels, label_depth_render_pipeline writes each label's depth once it is drawn, so the
                    // nearer label wins wherever it is solid, and the back to front sort only decides soft edges and
                    // labels on the same plane.
                    depth_write_enabled: false,
                    depth_compare: DEPTH_COMPARE_FUNCTION,
                    stencil: wgpu::StencilState::default(),
                    // Text placed flat on a surface is coplanar with it, and coplanar surfaces disagree about
                    // depth from pixel to pixel. This nudges labels toward the camera so they win that argument
                    // and can sit flush on a wall or a block face instead of having to hover off it.
                    // Depth runs reversed here, so nearer is larger and the bias is positive.
                    bias: wgpu::DepthBiasState {
                        constant: 16,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let label_render_pipeline = create_label_render_pipeline("Label Render Pipeline", "fs_label");
        let label_image_render_pipeline = create_label_render_pipeline("Label Image Render Pipeline", "fs_label_image");
        let create_label_depth_render_pipeline = |label: &'static str, fragment_entry_point: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some("vs_main_static"),
                    buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: DEPTH_COMPARE_FUNCTION,
                    stencil: wgpu::StencilState::default(),
                    // Keep this synced with label_render_pipeline's bias: the same slope, so a label just behind fails at any
                    // viewing angle, and half the constant, so a label drawn later on the same plane still passes.
                    bias: wgpu::DepthBiasState {
                        constant: 8,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let label_depth_render_pipeline = create_label_depth_render_pipeline("Label Depth Render Pipeline", "fs_label_depth");
        let label_image_depth_render_pipeline = create_label_depth_render_pipeline("Label Image Depth Render Pipeline", "fs_label_image_depth");
        let shadow_render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shadow Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &material_bind_group_layout, &joint_matrix_bind_group_layout],
            push_constant_ranges: &[],
        });
        let shadow_render_pipeline_descriptor = wgpu::RenderPipelineDescriptor {
            label: Some("Shadow Render Pipeline"),
            layout: Some(&shadow_render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main_static"),
                buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_depth_only"),
                targets: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: SHADOW_DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: SHADOW_DEPTH_BIAS_CONSTANT,
                    slope_scale: SHADOW_DEPTH_BIAS_SLOPE_SCALE,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        };
        let shadow_render_pipeline = create_depth_only_render_pipelines("vs_position_only_static", shadow_render_pipeline_descriptor);
        let skinned_render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Skinned Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &material_bind_group_layout,
                &joint_matrix_bind_group_layout,
                &lighting_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });
        let create_skinned_opaque_render_pipeline = |label: &'static str, fragment_entry_point: &'static str| {
            create_material_render_pipelines(wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&skinned_render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some("vs_main_skinned"),
                    buffers: &[skinned_vertex_buffer_layout(), instance_buffer_layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: DEPTH_COMPARE_FUNCTION,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: OPAQUE_MULTISAMPLE_COUNT,
                    ..Default::default()
                },
                multiview: None,
                cache: None,
            })
        };
        let skinned_render_pipeline_opaque = create_skinned_opaque_render_pipeline("Opaque Skinned Render Pipeline", "fs_main");
        let skinned_render_pipeline_opaque_mask = create_skinned_opaque_render_pipeline("Opaque Mask Skinned Render Pipeline", mask_fragment_entry_point);
        let skinned_render_pipeline_opaque_depth_only_descriptor = wgpu::RenderPipelineDescriptor {
            label: Some("Opaque Skinned Depth Replay Pipeline"),
            layout: Some(&skinned_render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main_skinned"),
                buffers: &[skinned_vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_opaque_depth_only"),
                targets: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        };
        let skinned_render_pipeline_opaque_depth_only =
            create_depth_only_render_pipelines("vs_position_only_skinned", skinned_render_pipeline_opaque_depth_only_descriptor);
        let skinned_render_pipeline_transparent = create_material_render_pipelines(wgpu::RenderPipelineDescriptor {
            label: Some("Transparent Skinned Render Pipeline"),
            layout: Some(&skinned_render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main_skinned"),
                buffers: &[skinned_vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_oit_transparent"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: OIT_ACCUM_FORMAT,
                        blend: Some(OIT_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: OIT_WEIGHT_FORMAT,
                        blend: Some(OIT_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let skinned_shadow_render_pipeline_descriptor = wgpu::RenderPipelineDescriptor {
            label: Some("Skinned Shadow Render Pipeline"),
            layout: Some(&shadow_render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main_skinned"),
                buffers: &[skinned_vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_depth_only"),
                targets: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: SHADOW_DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: SHADOW_DEPTH_BIAS_CONSTANT,
                    slope_scale: SHADOW_DEPTH_BIAS_SLOPE_SCALE,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        };
        let skinned_shadow_render_pipeline = create_depth_only_render_pipelines("vs_position_only_skinned", skinned_shadow_render_pipeline_descriptor);
        let skybox_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Skybox Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("skybox.wgsl").into()),
        });
        let skybox_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Skybox Render Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Skybox Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &skybox_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &skybox_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &skybox_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: SKY_MULTISAMPLE_STATE,
            multiview: None,
            cache: None,
        });
        let sky_gradient_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sky Gradient Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky_gradient.wgsl").into()),
        });
        let sky_gradient_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sky Gradient Render Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Sky Gradient Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &lighting_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &sky_gradient_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_gradient_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: SKY_MULTISAMPLE_STATE,
            multiview: None,
            cache: None,
        });
        let sky_sun_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sky Sun Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky_sun.wgsl").into()),
        });
        let sky_sun_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sky Sun Render Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Sky Sun Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &lighting_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &sky_sun_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_sun_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: DEPTH_COMPARE_FUNCTION,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: SKY_MULTISAMPLE_STATE,
            multiview: None,
            cache: None,
        });
        // The glow pass draws at half the picture's size, too small to use the scene's depth texture as its depth buffer, so the
        // glow shaders read that texture and test it themselves. scene.wgsl reads it in the group where its other entry points
        // read the lighting, after the lighting's bindings.
        let glow_scene_depth_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glow Scene Depth Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: GLOW_SCENE_DEPTH_BINDING,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                },
                count: None,
            }],
        });
        // The sun feeds bloom from the glow pass: the same full screen triangle as the sun's disk, into bloom level 0, only
        // where the sky shows.
        let sun_glow_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sun Glow Render Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Sun Glow Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &lighting_bind_group_layout, &glow_scene_depth_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &sky_sun_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_sun_shader,
                entry_point: Some("fs_glow"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: BLOOM_FORMAT,
                    blend: Some(ADDITIVE_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let oit_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OIT Composite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("oit_composite.wgsl").into()),
        });
        let oit_composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("OIT Composite Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("OIT Composite Pipeline Layout"),
                bind_group_layouts: &[&oit_composite_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &oit_composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &oit_composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let oit_composite_see_through_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("OIT Composite See-Through Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("OIT Composite See-Through Pipeline Layout"),
                bind_group_layouts: &[&oit_composite_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &oit_composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &oit_composite_shader,
                entry_point: Some("fs_main_see_through"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        // The editor draws selected silhouettes into a mask, then composites their blue edges.
        // Reusing the scene vertex paths keeps skinning, billboards, scale, and material sidedness identical.
        let create_selection_mask_pipeline = |vertex_entry_point: &str, fragment_entry_point: &str, vertex_layout: wgpu::VertexBufferLayout<'_>| {
            create_material_render_pipelines(wgpu::RenderPipelineDescriptor {
                label: Some("Selection Mask Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some(vertex_entry_point),
                    buffers: &[vertex_layout, instance_buffer_layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SELECTION_MASK_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let selection_mask_render_pipeline = create_selection_mask_pipeline("vs_main_static", "fs_selection_mask", vertex_buffer_layout());
        let skinned_selection_mask_render_pipeline = create_selection_mask_pipeline("vs_main_skinned", "fs_selection_mask", skinned_vertex_buffer_layout());
        let [_, label_selection_mask_render_pipeline] = create_selection_mask_pipeline("vs_main_static", "fs_label_selection_mask", vertex_buffer_layout());
        let [_, label_image_selection_mask_render_pipeline] =
            create_selection_mask_pipeline("vs_main_static", "fs_label_image_selection_mask", vertex_buffer_layout());
        let selection_outline_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Selection Outline Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("selection_outline.wgsl").into()),
        });
        let selection_outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Selection Outline Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Selection Outline Pipeline Layout"),
                bind_group_layouts: &[&selection_outline_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &selection_outline_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &selection_outline_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Glowing instances draw their glow into bloom level 0, where glows that overlap add up. The glow shaders test the
        // scene's depth themselves, so a wall still hides the glow of whatever is behind it.
        let glow_render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Glow Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &material_bind_group_layout,
                &joint_matrix_bind_group_layout,
                &glow_scene_depth_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });
        let create_glow_pipeline = |vertex_entry_point: &str, fragment_entry_point: &str, vertex_layout: wgpu::VertexBufferLayout<'_>| {
            create_material_render_pipelines(wgpu::RenderPipelineDescriptor {
                label: Some("Glow Render Pipeline"),
                layout: Some(&glow_render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &scene_shader,
                    entry_point: Some(vertex_entry_point),
                    buffers: &[vertex_layout, instance_buffer_layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &scene_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: BLOOM_FORMAT,
                        blend: Some(ADDITIVE_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let glow_render_pipeline = create_glow_pipeline("vs_main_static", "fs_glow", vertex_buffer_layout());
        let skinned_glow_render_pipeline = create_glow_pipeline("vs_main_skinned", "fs_glow", skinned_vertex_buffer_layout());
        let [_, label_glow_render_pipeline] = create_glow_pipeline("vs_main_static", "fs_label_glow", vertex_buffer_layout());
        let bloom_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bloom Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bloom_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bloom Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        let bloom_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom Pipeline Layout"),
            bind_group_layouts: &[&bloom_bind_group_layout],
            push_constant_ranges: &[],
        });
        let create_bloom_pipeline = |label: &str, fragment_entry_point: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&bloom_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &bloom_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &bloom_shader,
                    entry_point: Some(fragment_entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let bloom_downsample_render_pipeline = create_bloom_pipeline("Bloom Downsample Render Pipeline", "fs_downsample", BLOOM_FORMAT, None);
        let bloom_upsample_render_pipeline = create_bloom_pipeline("Bloom Upsample Render Pipeline", "fs_upsample", BLOOM_FORMAT, Some(ADDITIVE_BLENDING));
        // The base layer's composite, which also adds the finished glow from bloom level 0.
        let oit_composite_with_bloom_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("OIT Composite With Bloom Pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("OIT Composite With Bloom Pipeline Layout"),
                bind_group_layouts: &[&oit_composite_bind_group_layout, &bloom_bind_group_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &oit_composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &oit_composite_shader,
                entry_point: Some("fs_main_with_bloom"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let ambient_occlusion_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ambient Occlusion Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ambient_occlusion.wgsl").into()),
        });
        let create_ambient_occlusion_render_pipeline =
            |label: &'static str, bind_group_layout: &wgpu::BindGroupLayout, fragment_entry_point: &'static str, format: wgpu::TextureFormat| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some(label),
                        bind_group_layouts: &[&camera_bind_group_layout, bind_group_layout],
                        push_constant_ranges: &[],
                    })),
                    vertex: wgpu::VertexState {
                        module: &ambient_occlusion_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &ambient_occlusion_shader,
                        entry_point: Some(fragment_entry_point),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                })
            };
        let ambient_occlusion_view_depth_render_pipeline = create_ambient_occlusion_render_pipeline(
            "Ambient Occlusion View Depth Pipeline",
            &ambient_occlusion_bind_group_layouts.view_depth,
            "fs_view_depth",
            AMBIENT_OCCLUSION_VIEW_DEPTH_FORMAT,
        );
        let ambient_occlusion_view_depth_downsample_render_pipeline = create_ambient_occlusion_render_pipeline(
            "Ambient Occlusion View Depth Downsample Pipeline",
            &ambient_occlusion_bind_group_layouts.view_depth_downsample,
            "fs_downsample_view_depth",
            AMBIENT_OCCLUSION_VIEW_DEPTH_FORMAT,
        );
        let ambient_occlusion_render_pipeline = create_ambient_occlusion_render_pipeline(
            "Ambient Occlusion Pipeline",
            &ambient_occlusion_bind_group_layouts.ambient_occlusion,
            "fs_ambient_occlusion",
            AMBIENT_OCCLUSION_FORMAT,
        );
        let ambient_occlusion_blur_horizontal_render_pipeline = create_ambient_occlusion_render_pipeline(
            "Ambient Occlusion Blur Horizontal Pipeline",
            &ambient_occlusion_bind_group_layouts.blur,
            "fs_blur_horizontal",
            AMBIENT_OCCLUSION_FORMAT,
        );
        let ambient_occlusion_blur_vertical_render_pipeline = create_ambient_occlusion_render_pipeline(
            "Ambient Occlusion Blur Vertical Pipeline",
            &ambient_occlusion_bind_group_layouts.blur,
            "fs_blur_vertical",
            AMBIENT_OCCLUSION_FORMAT,
        );
        // ------------  Buffers ------------
        let brick_buffer = create_primitive_buffers(&device, &brick_geometry());
        let ball_buffer = create_ball_buffers(&device);
        let cylinder_buffer = create_primitive_buffers(&device, &cylinder_geometry());
        let label_image_buffer = create_primitive_buffers(&device, &label_image_geometry());
        let mesh_geometry_buffers = HashMap::new();
        let is_mesh_geometry_buffer_skinned = HashMap::new();
        let skinned_mesh_joint_matrix_buffers = HashMap::new();
        let fallback_joint_matrix_buffer = create_joint_matrix_buffer(&device, &joint_matrix_bind_group_layout, identity_joint_matrix_buffer_content());
        let texture_gpu_of_data_id = HashMap::new();
        let fallback_material_textures = create_fallback_gpu_material_textures(&device, &queue)?;
        let material_bind_group_of_key = HashMap::new();
        let material_alpha_mode_buffer_of_key = HashMap::new();

        let lighting_buffer_content = LightingBufferContent::zeroed();
        let lighting_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Lighting Buffer"),
            contents: bytemuck::cast_slice(&[lighting_buffer_content]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shadow_camera_buffer_content = CameraMatrixBufferContent {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inverse_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera_position: [0.0, 0.0, 0.0, 1.0],
            view_clip_w_row: clip_w_row_of_view_proj(Mat4::IDENTITY),
        };
        let shadow_depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Sun Shadow Depth Texture"),
            size: wgpu::Extent3d {
                width: SHADOW_MAP_SIZE,
                height: SHADOW_MAP_SIZE,
                depth_or_array_layers: SHADOW_CASCADE_COUNT as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_depth_array_texture_view = shadow_depth_texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let mut shadow_camera_buffers = Vec::with_capacity(SHADOW_CASCADE_COUNT);
        let mut shadow_camera_bind_groups = Vec::with_capacity(SHADOW_CASCADE_COUNT);
        let mut shadow_depth_texture_views = Vec::with_capacity(SHADOW_CASCADE_COUNT);
        for cascade_index in 0..SHADOW_CASCADE_COUNT {
            let shadow_camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("Shadow Camera Buffer Cascade {cascade_index}")),
                contents: bytemuck::cast_slice(&[shadow_camera_buffer_content]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let shadow_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("Shadow Camera Bind Group Cascade {cascade_index}")),
                layout: &camera_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: shadow_camera_buffer.as_entire_binding(),
                }],
            });
            shadow_camera_buffers.push(shadow_camera_buffer);
            shadow_camera_bind_groups.push(shadow_camera_bind_group);
            shadow_depth_texture_views.push(shadow_depth_texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some(&format!("Sun Shadow Depth Texture View Cascade {cascade_index}")),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: cascade_index as u32,
                array_layer_count: Some(1),
                ..Default::default()
            }));
        }
        let shadow_compare_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Sun Shadow Compare Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(SHADOW_DEPTH_COMPARE_FUNCTION),
            ..Default::default()
        });
        let default_shadow_view_projections = build_shadow_cascade_view_projections(Vec3::Y, Vec3::ZERO);
        let shadow_buffer_content = Self::build_shadow_buffer_content(&default_shadow_view_projections, 0.0);
        let shadow_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Shadow Buffer"),
            contents: bytemuck::cast_slice(&[shadow_buffer_content]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let skybox_buffer_content = Self::build_skybox_buffer_content(1.0, 1);
        let skybox_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Skybox Buffer"),
            contents: bytemuck::cast_slice(&[skybox_buffer_content]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let render_targets = create_layer_textures(
            &device,
            &config,
            &oit_composite_bind_group_layout,
            &selection_outline_bind_group_layout,
            &ambient_occlusion_bind_group_layouts,
            &lighting_buffer,
            &bloom_bind_group_layout,
            &bloom_sampler,
            &glow_scene_depth_bind_group_layout,
        );
        let fallback_skybox_texture = create_gpu_texture_from_mip_data(
            &device,
            &queue,
            &[vec![0, 0, 0, 0]],
            1,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            true,
            "Fallback Skybox Texture",
        )?;
        let lighting_bind_group = create_lighting_bind_group(
            &device,
            &lighting_bind_group_layout,
            &lighting_buffer,
            &shadow_buffer,
            &shadow_depth_array_texture_view,
            &shadow_compare_sampler,
            &render_targets.ambient_occlusion_blurred_texture_view,
            &fallback_skybox_texture.texture_view,
            &skybox_sampler,
            &skybox_buffer,
        );

        Ok(Self {
            device,
            queue,
            config,

            render_pipeline_opaque,
            render_pipeline_opaque_mask,
            render_pipeline_opaque_depth_only,
            render_pipeline_transparent,
            shadow_render_pipeline,
            skinned_render_pipeline_opaque,
            skinned_render_pipeline_opaque_mask,
            skinned_render_pipeline_opaque_depth_only,
            skinned_render_pipeline_transparent,
            skinned_shadow_render_pipeline,
            skybox_render_pipeline,
            sky_gradient_render_pipeline,
            sky_sun_render_pipeline,
            oit_composite_pipeline,
            oit_composite_see_through_pipeline,
            label_render_pipeline,
            label_depth_render_pipeline,
            label_image_render_pipeline,
            label_image_depth_render_pipeline,
            selection_mask_render_pipeline,
            skinned_selection_mask_render_pipeline,
            label_selection_mask_render_pipeline,
            label_image_selection_mask_render_pipeline,
            selection_outline_pipeline,
            glow_render_pipeline,
            skinned_glow_render_pipeline,
            label_glow_render_pipeline,
            bloom_downsample_render_pipeline,
            bloom_upsample_render_pipeline,
            oit_composite_with_bloom_pipeline,
            sun_glow_render_pipeline,
            ambient_occlusion_view_depth_render_pipeline,
            ambient_occlusion_view_depth_downsample_render_pipeline,
            ambient_occlusion_render_pipeline,
            ambient_occlusion_blur_horizontal_render_pipeline,
            ambient_occlusion_blur_vertical_render_pipeline,
            label_geometry_buffer_of_cache_key: HashMap::new(),
            label_font_atlas_gpu: None,

            brick_buffer,
            ball_buffer,
            cylinder_buffer,
            label_image_buffer,
            mesh_geometry_buffers,
            is_mesh_geometry_buffer_skinned,
            lighting_buffer,
            lighting_bind_group,
            shadow_camera_buffers,
            shadow_camera_bind_groups,
            shadow_buffer,
            shadow_depth_texture_views,
            shadow_depth_array_texture_view,
            shadow_compare_sampler,
            skybox_buffer,
            skinned_mesh_joint_matrix_buffers,
            texture_gpu_of_data_id,
            material_bind_group_of_key,
            material_alpha_mode_buffer_of_key,
            skybox_bind_group_of_data_id: HashMap::new(),
            label_image_bind_group_of_data_id: HashMap::new(),
            fallback_skybox_texture,
            lighting_bind_group_skybox_data_id: Some(None),

            fallback_joint_matrix_buffer,
            fallback_material_textures,

            render_targets,

            material_bind_group_layout,
            material_sampler,
            skybox_bind_group_layout,
            skybox_sampler,
            label_image_sampler,
            label_image_alpha_mode_buffer,
            joint_matrix_bind_group_layout,
            oit_composite_bind_group_layout,
            selection_outline_bind_group_layout,
            bloom_bind_group_layout,
            bloom_sampler,
            glow_scene_depth_bind_group_layout,
            camera_bind_group_layout,
            lighting_bind_group_layout,
            ambient_occlusion_bind_group_layouts,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.config.width && height == self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.render_targets = create_layer_textures(
            &self.device,
            &self.config,
            &self.oit_composite_bind_group_layout,
            &self.selection_outline_bind_group_layout,
            &self.ambient_occlusion_bind_group_layouts,
            &self.lighting_buffer,
            &self.bloom_bind_group_layout,
            &self.bloom_sampler,
            &self.glow_scene_depth_bind_group_layout,
        );
        self.rebuild_lighting_bind_group(self.lighting_bind_group_skybox_data_id.flatten());
    }

    // The lighting bind group holds the ambient occlusion texture, sized to the render targets, and the skybox texture
    // surfaces reflect, so it is rebuilt when either changes.
    fn rebuild_lighting_bind_group(&mut self, skybox_texture_data_id: Option<DataId>) {
        let skybox_texture_view = skybox_texture_data_id
            .and_then(|skybox_texture_data_id| self.texture_gpu_of_data_id.get(&skybox_texture_data_id))
            .map_or(&self.fallback_skybox_texture.texture_view, |texture_gpu| &texture_gpu.texture_view);
        self.lighting_bind_group = create_lighting_bind_group(
            &self.device,
            &self.lighting_bind_group_layout,
            &self.lighting_buffer,
            &self.shadow_buffer,
            &self.shadow_depth_array_texture_view,
            &self.shadow_compare_sampler,
            &self.render_targets.ambient_occlusion_blurred_texture_view,
            skybox_texture_view,
            &self.skybox_sampler,
            &self.skybox_buffer,
        );
        self.lighting_bind_group_skybox_data_id = Some(skybox_texture_data_id);
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    fn build_camera_matrix_buffer_content(camera_position: Vec3, target: Vec3, aspect: f32, fov_degrees: f32) -> CameraMatrixBufferContent {
        let view_proj = build_camera_view_proj_matrix(camera_position, target, aspect, fov_degrees);
        CameraMatrixBufferContent {
            view_proj: view_proj.to_cols_array_2d(),
            inverse_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_position: [camera_position.x, camera_position.y, camera_position.z, 1.0],
            view_clip_w_row: clip_w_row_of_view_proj(view_proj),
        }
    }

    fn build_shadow_buffer_content(shadow_view_projections: &[Mat4; SHADOW_CASCADE_COUNT], sun_shadow_softness: f32) -> ShadowBufferContent {
        ShadowBufferContent {
            light_view_proj: shadow_view_projections.map(|view_proj| view_proj.to_cols_array_2d()),
            cascade_settings: SHADOW_CASCADE_HALF_SIZES.map(|half_size| [half_size, half_size * 2.0 / SHADOW_MAP_SIZE as f32, 0.0, 0.0]),
            settings: [
                SHADOW_RECEIVER_DEPTH_BIAS_WORLD,
                SHADOW_RECEIVER_NORMAL_BIAS_WORLD,
                sun_shadow_softness.clamp(0.0, SUN_SHADOW_SOFTNESS_MAX),
                SHADOW_CASTER_EYE_DISTANCE * 2.0 - CAMERA_NEAR_PLANE,
            ],
        }
    }

    fn build_skybox_buffer_content(skybox_texture_opacity: f32, reflected_skybox_mip_level_count: u32) -> SkyboxBufferContent {
        SkyboxBufferContent {
            settings: [skybox_texture_opacity, reflected_skybox_mip_level_count as f32, 0.0, 0.0],
        }
    }

    pub fn create_camera_matrix_buffer_pair(&self, camera_position: Vec3, target: Vec3, aspect: f32, fov_degrees: f32) -> (wgpu::Buffer, wgpu::BindGroup) {
        let camera_matrix_buffer_content = Self::build_camera_matrix_buffer_content(camera_position, target, aspect, fov_degrees);
        let camera_matrix_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Per-View Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_matrix_buffer_content]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_matrix_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Per-View Camera Bind Group"),
            layout: &self.camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_matrix_buffer.as_entire_binding(),
            }],
        });
        (camera_matrix_buffer, camera_matrix_bind_group)
    }

    pub fn update_camera_matrix_buffer(&self, camera_matrix_buffer: &wgpu::Buffer, camera_position: Vec3, target: Vec3, aspect: f32, fov_degrees: f32) {
        let new_content = Self::build_camera_matrix_buffer_content(camera_position, target, aspect, fov_degrees);
        self.queue.write_buffer(camera_matrix_buffer, 0, bytemuck::cast_slice(&[new_content]));
    }

    fn update_skinned_mesh_joint_matrix_buffer(
        &mut self,
        skinned_mesh_joint_matrix_buffer_key: SkinnedMeshJointMatrixBufferKey,
        joint_matrix: JointMatrixBufferContent,
    ) -> Result<()> {
        let buffer = &self.skinned_mesh_joint_matrix_buffers.get(&skinned_mesh_joint_matrix_buffer_key);
        let Some(buffer) = buffer else {
            return Err(anyhow::anyhow!(
                "Skinned mesh joint matrix buffer not found for mesh data id {} and Skeleton id {}",
                skinned_mesh_joint_matrix_buffer_key.mesh_data_id,
                skinned_mesh_joint_matrix_buffer_key.skeleton_id
            ));
        };
        self.queue.write_buffer(&buffer.joint_matrix_buffer, 0, bytemuck::cast_slice(&[joint_matrix]));
        Ok(())
    }

    pub fn begin_frame(&self, view: wgpu::TextureView) -> RenderFrame {
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") });
        RenderFrame {
            view,
            encoder,
            did_render_world: false,
        }
    }

    pub fn finish_frame(&mut self, render_frame: RenderFrame) {
        self.queue.submit(std::iter::once(render_frame.encoder.finish()));
    }

    // For frames with no world in them. The desktop client's webview draws everything else on top.
    pub fn clear_frame(&self, render_frame: &mut RenderFrame, clear_color: wgpu::Color) {
        render_frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Clear Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &render_frame.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    // ------------------------ Render objects ------------------------

    pub fn has_mesh(&self, data_id: DataId) -> bool {
        self.mesh_geometry_buffers.contains_key(&data_id)
    }
    pub fn load_mesh_into_gpu(&mut self, data_id: DataId, mesh: &MeshData) {
        fn create_mesh_buffers(device: &wgpu::Device, mesh_data: &MeshData) -> GpuGeometryBuffer {
            let vertex_buffer = match mesh_data {
                MeshData::SkinnedMeshData { vertices, .. } => {
                    let vertices: Vec<SkinnedVertexContent> = vertices
                        .iter()
                        .map(|v| SkinnedVertexContent {
                            position: v.position.to_array(),
                            normal: v.normal.to_array(),
                            uv: v.uv.to_array(),
                            tangent: v.tangent.to_array(),
                            joint_indices: v.skinning.jointIndices,
                            joint_weights: v.skinning.jointWeights,
                        })
                        .collect();
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Skinned Mesh VB"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
                }
                MeshData::StaticMeshData { vertices, .. } => {
                    let vertices: Vec<VertexContent> = vertices
                        .iter()
                        .map(|v| VertexContent {
                            position: v.position.to_array(),
                            normal: v.normal.to_array(),
                            uv: v.uv.to_array(),
                            tangent: v.tangent.to_array(),
                        })
                        .collect();
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Mesh VB"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
                }
            };

            let indices = match mesh_data {
                MeshData::SkinnedMeshData { indices, .. } => indices,
                MeshData::StaticMeshData { indices, .. } => indices,
            };
            let positions: Vec<[f32; 3]> = match mesh_data {
                MeshData::SkinnedMeshData { vertices, .. } => vertices.iter().map(|vertex| vertex.position.to_array()).collect(),
                MeshData::StaticMeshData { vertices, .. } => vertices.iter().map(|vertex| vertex.position.to_array()).collect(),
            };
            // Skinned meshes keep only their full triangles, since their joints can pull apart vertices that a copy made in the
            // rest pose merged.
            let (indices_with_simpler_detail_levels, simpler_detail_levels) = match mesh_data {
                MeshData::SkinnedMeshData { .. } => (indices.clone(), Vec::new()),
                MeshData::StaticMeshData { vertices, .. } => {
                    let uvs: Vec<[f32; 2]> = vertices.iter().map(|vertex| vertex.uv.to_array()).collect();
                    build_simpler_detail_levels(&positions, &uvs, indices)
                }
            };
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Mesh IB"),
                contents: bytemuck::cast_slice(indices_with_simpler_detail_levels.as_slice()),
                usage: wgpu::BufferUsages::INDEX,
            });

            GpuGeometryBuffer {
                vertex_buffer,
                index_buffer,
                num_indices: indices.len() as u32,
                index_format: wgpu::IndexFormat::Uint32,
                bounding_radius: bounding_radius_around_origin(positions),
                simpler_detail_levels,
            }
        }
        self.mesh_geometry_buffers.insert(data_id, create_mesh_buffers(&self.device, mesh));
        self.is_mesh_geometry_buffer_skinned.insert(
            data_id,
            match mesh {
                MeshData::SkinnedMeshData { .. } => true,
                MeshData::StaticMeshData { .. } => false,
            },
        );
    }
    pub fn has_texture(&self, data_id: DataId) -> bool {
        self.texture_gpu_of_data_id.contains_key(&data_id)
    }
    pub fn retain_textures(&mut self, data_ids: &HashSet<DataId>) {
        self.texture_gpu_of_data_id.retain(|id, _| data_ids.contains(id));
        // A bind group keeps its textures alive and drawn, so the ones using a dropped texture go too.
        self.material_bind_group_of_key.retain(|key, _| {
            [
                key.basecolor_texture_data_id,
                key.metallic_texture_data_id,
                key.roughness_texture_data_id,
                key.normal_texture_data_id,
            ]
            .iter()
            .flatten()
            .all(|id| data_ids.contains(id))
        });
        self.skybox_bind_group_of_data_id.retain(|id, _| data_ids.contains(id));
        self.label_image_bind_group_of_data_id.retain(|id, _| data_ids.contains(id));
    }
    pub fn load_texture_into_gpu(
        &mut self,
        data_id: DataId,
        mip_data: &[Vec<u8>],
        width: u32,
        height: u32,
        upload_format: wgpu::TextureFormat,
        is_srgb: bool,
    ) -> Result<()> {
        let texture = create_gpu_texture_from_mip_data(
            &self.device,
            &self.queue,
            mip_data,
            width,
            height,
            upload_format,
            is_srgb,
            &format!("Texture {data_id}"),
        )?;
        self.texture_gpu_of_data_id.insert(data_id, texture);
        // A texture newly loaded into the GPU replaces any material bindings that still held its previous/default view.
        self.material_bind_group_of_key.retain(|key, _| {
            ![
                key.basecolor_texture_data_id,
                key.metallic_texture_data_id,
                key.roughness_texture_data_id,
                key.normal_texture_data_id,
            ]
            .contains(&Some(data_id))
        });
        self.skybox_bind_group_of_data_id.remove(&data_id);
        self.label_image_bind_group_of_data_id.remove(&data_id);
        if self.lighting_bind_group_skybox_data_id == Some(Some(data_id)) {
            self.lighting_bind_group_skybox_data_id = None;
        }
        Ok(())
    }

    fn update_skybox_bind_group(&mut self, skybox_texture_data_id: Option<DataId>) {
        self.skybox_bind_group_of_data_id
            .retain(|texture_data_id, _| Some(*texture_data_id) == skybox_texture_data_id);
        let Some(skybox_texture_data_id) = skybox_texture_data_id else {
            return;
        };
        if self.skybox_bind_group_of_data_id.contains_key(&skybox_texture_data_id) {
            return;
        }
        let Some(texture_gpu) = self.texture_gpu_of_data_id.get(&skybox_texture_data_id) else {
            return;
        };

        let skybox_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Skybox Bind Group"),
            layout: &self.skybox_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_gpu.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.skybox_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.skybox_buffer.as_entire_binding(),
                },
            ],
        });
        self.skybox_bind_group_of_data_id.insert(skybox_texture_data_id, skybox_bind_group);
    }

    fn update_label_image_bind_groups_for_frame(&mut self, label_image_texture_data_ids_used_by_frame: &HashSet<DataId>) {
        self.label_image_bind_group_of_data_id
            .retain(|texture_data_id, _| label_image_texture_data_ids_used_by_frame.contains(texture_data_id));
        for texture_data_id in label_image_texture_data_ids_used_by_frame {
            if self.label_image_bind_group_of_data_id.contains_key(texture_data_id) {
                continue;
            }
            let Some(texture_gpu) = self.texture_gpu_of_data_id.get(texture_data_id) else {
                continue;
            };
            let label_image_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Label Image Bind Group"),
                layout: &self.material_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_gpu.texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.fallback_material_textures.metallic_texture_gpu.texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.fallback_material_textures.roughness_texture_gpu.texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&self.fallback_material_textures.normal_texture_gpu.texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.label_image_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.label_image_alpha_mode_buffer.as_entire_binding(),
                    },
                ],
            });
            self.label_image_bind_group_of_data_id.insert(*texture_data_id, label_image_bind_group);
        }
    }

    fn update_material_bind_groups_for_frame(&mut self, material_texture_keys_used_by_frame: &HashSet<MaterialTextureDataIds>) {
        self.material_bind_group_of_key
            .retain(|material_texture_data_ids, _| material_texture_keys_used_by_frame.contains(material_texture_data_ids));
        self.material_alpha_mode_buffer_of_key
            .retain(|material_texture_data_ids, _| material_texture_keys_used_by_frame.contains(material_texture_data_ids));

        for material_texture_data_ids in material_texture_keys_used_by_frame {
            // Reuse the cached bind group unless it was built with fallback textures that have since had a chance to load.
            if self.material_bind_group_of_key.contains_key(material_texture_data_ids) {
                continue;
            }

            let basecolor_texture_view = material_texture_data_ids
                .basecolor_texture_data_id
                .and_then(|texture_data_id| self.texture_gpu_of_data_id.get(&texture_data_id))
                .map(|texture_gpu| &texture_gpu.texture_view)
                .unwrap_or(&self.fallback_material_textures.basecolor_texture_gpu.texture_view);
            let metallic_texture_view = material_texture_data_ids
                .metallic_texture_data_id
                .and_then(|texture_data_id| self.texture_gpu_of_data_id.get(&texture_data_id))
                .map(|texture_gpu| &texture_gpu.linear_texture_view)
                .unwrap_or(&self.fallback_material_textures.metallic_texture_gpu.linear_texture_view);
            let roughness_texture_view = material_texture_data_ids
                .roughness_texture_data_id
                .and_then(|texture_data_id| self.texture_gpu_of_data_id.get(&texture_data_id))
                .map(|texture_gpu| &texture_gpu.linear_texture_view)
                .unwrap_or(&self.fallback_material_textures.roughness_texture_gpu.linear_texture_view);
            let normal_texture_view = material_texture_data_ids
                .normal_texture_data_id
                .and_then(|texture_data_id| self.texture_gpu_of_data_id.get(&texture_data_id))
                .map(|texture_gpu| &texture_gpu.linear_texture_view)
                .unwrap_or(&self.fallback_material_textures.normal_texture_gpu.linear_texture_view);
            let material_alpha_mode_buffer_content = material_alpha_mode_buffer_content_of_material_texture_key(*material_texture_data_ids);
            let material_alpha_mode_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Material Alpha Mode Buffer"),
                contents: bytemuck::bytes_of(&material_alpha_mode_buffer_content),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

            let material_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Material Bind Group"),
                layout: &self.material_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(basecolor_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(metallic_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(roughness_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(normal_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.material_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: material_alpha_mode_buffer.as_entire_binding(),
                    },
                ],
            });
            self.material_bind_group_of_key.insert(*material_texture_data_ids, material_bind_group);
            self.material_alpha_mode_buffer_of_key
                .insert(*material_texture_data_ids, material_alpha_mode_buffer);
        }
    }

    // Labels arrive already laid out and already rasterized, so all this has to do is notice when the
    // atlas or a piece of text changed and mirror that onto the GPU. Geometry cache keys come from the
    // client and already fold in the atlas version, so stale UVs can never outlive their atlas.
    fn update_label_gpu_state(&mut self, scene: &RenderScene) -> HashSet<u64> {
        let mut label_geometry_cache_keys_used_by_frame: HashSet<u64> = HashSet::new();
        let mut newest_atlas: Option<&Arc<FontAtlas>> = None;
        for label in &scene.labels {
            let LabelContent::Text { geometry, atlas, .. } = &label.content else {
                continue;
            };
            if geometry.indices.is_empty() {
                continue;
            }
            label_geometry_cache_keys_used_by_frame.insert(geometry.cache_key);
            if newest_atlas.is_none_or(|newest_atlas| newest_atlas.version < atlas.version) {
                newest_atlas = Some(atlas);
            }
        }

        let Some(atlas) = newest_atlas else {
            self.label_geometry_buffer_of_cache_key.clear();
            self.label_font_atlas_gpu = None;
            return HashSet::new();
        };

        let needs_atlas_upload = self.label_font_atlas_gpu.as_ref().is_none_or(|atlas_gpu| atlas_gpu.version != atlas.version);
        if needs_atlas_upload {
            // Distances are plain numbers, not colors, so this stays unorm.
            match create_gpu_texture_from_mip_data(
                &self.device,
                &self.queue,
                &atlas.mip_levels,
                atlas.width,
                atlas.height,
                wgpu::TextureFormat::R8Unorm,
                false,
                "Label Font Atlas Texture",
            ) {
                Ok(atlas_texture_gpu) => {
                    log::info!(
                        "Label font atlas v{}: {}x{}, {:.1} MB",
                        atlas.version,
                        atlas.width,
                        atlas.height,
                        atlas.mip_levels.iter().map(Vec::len).sum::<usize>() as f32 / (1024.0 * 1024.0),
                    );
                    // fs_oit_label never reads this, but the shared material bind group layout requires it.
                    let alpha_mode_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Label Font Atlas Alpha Mode Buffer"),
                        contents: bytemuck::bytes_of(&MaterialAlphaModeBufferContent {
                            alpha_mode: MATERIAL_ALPHA_MODE_OPAQUE,
                            alpha_cutoff: 0.0,
                            _padding0: 0,
                            _padding1: 0,
                        }),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    });
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Label Font Atlas Bind Group"),
                        layout: &self.material_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&atlas_texture_gpu.texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&self.fallback_material_textures.metallic_texture_gpu.texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::TextureView(&self.fallback_material_textures.roughness_texture_gpu.texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::TextureView(&self.fallback_material_textures.normal_texture_gpu.texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: wgpu::BindingResource::Sampler(&self.material_sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 5,
                                resource: alpha_mode_buffer.as_entire_binding(),
                            },
                        ],
                    });
                    self.label_font_atlas_gpu = Some(LabelFontAtlasGpu {
                        version: atlas.version,
                        bind_group,
                        _texture: atlas_texture_gpu,
                    });
                }
                Err(error) => {
                    log::warn!("Could not upload the label font atlas: {error}");
                    self.label_font_atlas_gpu = None;
                }
            }
        }
        if self.label_font_atlas_gpu.is_none() {
            self.label_geometry_buffer_of_cache_key.clear();
            return HashSet::new();
        }

        self.label_geometry_buffer_of_cache_key
            .retain(|cache_key, _| label_geometry_cache_keys_used_by_frame.contains(cache_key));
        for label in &scene.labels {
            let LabelContent::Text { geometry, .. } = &label.content else {
                continue;
            };
            if geometry.indices.is_empty() || self.label_geometry_buffer_of_cache_key.contains_key(&geometry.cache_key) {
                continue;
            }
            let vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Label VB"),
                contents: bytemuck::cast_slice(&geometry.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Label IB"),
                contents: bytemuck::cast_slice(&geometry.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            self.label_geometry_buffer_of_cache_key.insert(
                geometry.cache_key,
                GpuGeometryBuffer {
                    vertex_buffer,
                    index_buffer,
                    num_indices: geometry.indices.len() as u32,
                    index_format: wgpu::IndexFormat::Uint32,
                    bounding_radius: bounding_radius_around_origin(geometry.vertices.iter().map(|vertex| vertex.position)),
                    simpler_detail_levels: Vec::new(),
                },
            );
        }

        label_geometry_cache_keys_used_by_frame
    }

    pub fn render_scene(
        &mut self,
        render_frame: &mut RenderFrame,
        scene: &RenderScene,
        viewport: Option<SimpleViewportRegion>, // if this is None, the viewport is the entire screen
        camera_bind_group: &wgpu::BindGroup,
        shadow_center: Vec3,
        camera_position: Vec3,
        // The same matrix camera_bind_group was built from. The shadow pass draws with the sun's
        // matrix but still has to size constant-size instances against this one.
        camera_view_proj: Mat4,
    ) {
        // This helper block owns the full pass flow for this frame:
        // each layer runs the same 3 stages: opaque -> transparent OIT -> OIT composite.
        // We run this once for the base layer and once for the always-on-top front layer.
        // Between the base layer's transparent OIT and its composite, glowing things are drawn and blurred into bloom,
        // which that composite adds.
        // The helpers below keep pass setup and draw logic consistent across both layers.
        type ClampedViewport = Option<(u32, u32, u32, u32)>;

        // One pass's instances: for each key, its range of the frame's instance buffer.
        struct PassInstanceBuffers<'a> {
            instance_buffer: &'a wgpu::Buffer,
            brick_instance_range_of_key: &'a HashMap<BrickInstanceBufferKey, Range<u32>>,
            ball_instance_range_of_key: &'a HashMap<BallInstanceBufferKey, Range<u32>>,
            cylinder_instance_range_of_key: &'a HashMap<CylinderInstanceBufferKey, Range<u32>>,
            mesh_instance_range_of_key: &'a HashMap<MeshInstanceBufferKey, Range<u32>>,
            skinned_mesh_instance_range_of_key: &'a HashMap<SkinnedMeshInstanceBufferKey, Range<u32>>,
            label_draws: &'a Vec<LabelDraw>,
        }

        fn pass_instance_buffers_are_empty(pass_instance_buffers: &PassInstanceBuffers) -> bool {
            pass_instance_buffers.brick_instance_range_of_key.is_empty()
                && pass_instance_buffers.ball_instance_range_of_key.is_empty()
                && pass_instance_buffers.cylinder_instance_range_of_key.is_empty()
                && pass_instance_buffers.mesh_instance_range_of_key.is_empty()
                && pass_instance_buffers.skinned_mesh_instance_range_of_key.is_empty()
                && pass_instance_buffers.label_draws.is_empty()
        }

        // The shadow casters: for each key, where its buckets start in the frame's instance buffer. See culling.rs.
        struct ShadowCasterBuffers<'a> {
            instance_buffer: &'a wgpu::Buffer,
            brick_bucket_starts_of_key: &'a HashMap<BrickInstanceBufferKey, ShadowCasterBucketStarts>,
            ball_bucket_starts_of_key: &'a HashMap<BallInstanceBufferKey, ShadowCasterBucketStarts>,
            cylinder_bucket_starts_of_key: &'a HashMap<CylinderInstanceBufferKey, ShadowCasterBucketStarts>,
            mesh_bucket_starts_of_key: &'a HashMap<MeshInstanceBufferKey, ShadowCasterBucketStarts>,
            skinned_mesh_bucket_starts_of_key: &'a HashMap<SkinnedMeshInstanceBufferKey, ShadowCasterBucketStarts>,
        }

        struct LayerSharedDrawInputs<'a> {
            camera_bind_group: &'a wgpu::BindGroup,
            lighting_bind_group: &'a wgpu::BindGroup,
            skybox_bind_group: Option<&'a wgpu::BindGroup>,
            brick_buffer: &'a GpuGeometryBuffer,
            ball_buffer: &'a GpuGeometryBuffer,
            cylinder_buffer: &'a GpuGeometryBuffer,
            mesh_geometry_buffers: &'a HashMap<DataId, GpuGeometryBuffer>,
            skinned_mesh_joint_matrix_buffers: &'a HashMap<SkinnedMeshJointMatrixBufferKey, GpuJointMatrixBuffer>,
            fallback_joint_matrix_buffer: &'a GpuJointMatrixBuffer,
            material_bind_group_of_key: &'a HashMap<MaterialTextureDataIds, wgpu::BindGroup>,
            default_material_texture_key: MaterialTextureDataIds,
            label_geometry_buffer_of_cache_key: &'a HashMap<u64, GpuGeometryBuffer>,
            label_font_atlas_bind_group: Option<&'a wgpu::BindGroup>,
            label_image_draw_inputs: LabelImageDrawInputs<'a>,
        }

        // What a pass needs to draw LabelImages. Passes that never draw them, like glow, leave it out.
        struct LabelImageDrawInputs<'a> {
            render_pipeline: &'a wgpu::RenderPipeline,
            depth_render_pipeline: Option<&'a wgpu::RenderPipeline>,
            quad_buffer: &'a GpuGeometryBuffer,
            bind_group_of_data_id: &'a HashMap<DataId, wgpu::BindGroup>,
        }

        struct LayerPassPipelines<'a> {
            opaque_static_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            opaque_mask_static_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            opaque_depth_only_static_render_pipeline: &'a DepthOnlyRenderPipelines,
            opaque_skinned_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            opaque_mask_skinned_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            opaque_depth_only_skinned_render_pipeline: &'a DepthOnlyRenderPipelines,
            shadow_static_render_pipeline: &'a DepthOnlyRenderPipelines,
            shadow_skinned_render_pipeline: &'a DepthOnlyRenderPipelines,
            transparent_static_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            transparent_skinned_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            skybox_render_pipeline: &'a wgpu::RenderPipeline,
            sky_gradient_render_pipeline: &'a wgpu::RenderPipeline,
            sky_sun_render_pipeline: &'a wgpu::RenderPipeline,
            oit_composite_render_pipeline: &'a wgpu::RenderPipeline,
            label_render_pipeline: &'a wgpu::RenderPipeline,
            label_depth_render_pipeline: &'a wgpu::RenderPipeline,
            ambient_occlusion_view_depth_render_pipeline: &'a wgpu::RenderPipeline,
            ambient_occlusion_view_depth_downsample_render_pipeline: &'a wgpu::RenderPipeline,
            ambient_occlusion_render_pipeline: &'a wgpu::RenderPipeline,
            ambient_occlusion_blur_horizontal_render_pipeline: &'a wgpu::RenderPipeline,
            ambient_occlusion_blur_vertical_render_pipeline: &'a wgpu::RenderPipeline,
        }

        struct LayerPassTargets<'a> {
            opaque_multisample_color_view: &'a wgpu::TextureView,
            opaque_multisample_depth_view: &'a wgpu::TextureView,
            scene_color_view: &'a wgpu::TextureView,
            depth_view: &'a wgpu::TextureView,
            oit_accum_view: &'a wgpu::TextureView,
            oit_weight_view: &'a wgpu::TextureView,
            composite_output_color_view: &'a wgpu::TextureView,
            oit_composite_bind_group: &'a wgpu::BindGroup,
            ambient_occlusion_view_depth_mip_views: &'a [wgpu::TextureView],
            ambient_occlusion_view_depth_downsample_bind_groups: &'a [wgpu::BindGroup],
            ambient_occlusion_noisy_view: &'a wgpu::TextureView,
            ambient_occlusion_blurred_horizontally_view: &'a wgpu::TextureView,
            ambient_occlusion_blurred_view: &'a wgpu::TextureView,
            ambient_occlusion_view_depth_bind_group: &'a wgpu::BindGroup,
            ambient_occlusion_bind_group: &'a wgpu::BindGroup,
            ambient_occlusion_blur_horizontal_bind_group: &'a wgpu::BindGroup,
            ambient_occlusion_blur_vertical_bind_group: &'a wgpu::BindGroup,
        }

        struct LayerPassPlan<'a> {
            opaque_color_load: wgpu::LoadOp<wgpu::Color>,
            opaque_depth_load: wgpu::LoadOp<f32>,
            oit_composite_color_load: wgpu::LoadOp<wgpu::Color>,
            should_draw_sky: bool,
            opaque_pass_label: &'static str,
            opaque_depth_only_pass_label: &'static str,
            transparent_pass_label: &'static str,
            oit_composite_pass_label: &'static str,
            label_pass_label: &'static str,
            opaque_pass_instance_buffers: &'a PassInstanceBuffers<'a>,
            transparent_pass_instance_buffers: &'a PassInstanceBuffers<'a>,
        }

        // Each label brings its own tessellated text, so labels are drawn one at a time rather than instanced.
        fn draw_label_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            label: &'static str,
            color_view: &'a wgpu::TextureView,
            depth_view: &'a wgpu::TextureView,
            clamped_viewport: ClampedViewport,
            camera_bind_group: &'a wgpu::BindGroup,
            lighting_bind_group: &'a wgpu::BindGroup,
            fallback_joint_matrix_buffer: &'a GpuJointMatrixBuffer,
            label_render_pipeline: &'a wgpu::RenderPipeline,
            label_depth_render_pipeline: &'a wgpu::RenderPipeline,
            label_geometry_buffer_of_cache_key: &'a HashMap<u64, GpuGeometryBuffer>,
            label_font_atlas_bind_group: Option<&'a wgpu::BindGroup>,
            label_image_draw_inputs: &'a LabelImageDrawInputs<'a>,
            pass_instance_buffers: &PassInstanceBuffers<'_>,
        ) {
            if pass_instance_buffers.label_draws.is_empty() {
                return;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            set_pass_viewport(&mut pass, clamped_viewport);
            draw_pass_labels(
                &mut pass,
                camera_bind_group,
                lighting_bind_group,
                fallback_joint_matrix_buffer,
                label_render_pipeline,
                Some(label_depth_render_pipeline),
                label_geometry_buffer_of_cache_key,
                label_font_atlas_bind_group,
                Some(label_image_draw_inputs),
                pass_instance_buffers,
            );
        }

        fn draw_pass_labels(
            pass: &mut wgpu::RenderPass<'_>,
            camera_bind_group: &wgpu::BindGroup,
            lighting_bind_group: &wgpu::BindGroup,
            fallback_joint_matrix_buffer: &GpuJointMatrixBuffer,
            label_render_pipeline: &wgpu::RenderPipeline,
            label_depth_render_pipeline: Option<&wgpu::RenderPipeline>,
            label_geometry_buffer_of_cache_key: &HashMap<u64, GpuGeometryBuffer>,
            label_font_atlas_bind_group: Option<&wgpu::BindGroup>,
            label_image_draw_inputs: Option<&LabelImageDrawInputs>,
            pass_instance_buffers: &PassInstanceBuffers<'_>,
        ) {
            if pass_instance_buffers.label_draws.is_empty() {
                return;
            }

            pass.set_bind_group(0, camera_bind_group, &[]);
            pass.set_bind_group(2, &fallback_joint_matrix_buffer.joint_matrix_bind_group, &[]);
            pass.set_bind_group(3, lighting_bind_group, &[]);
            pass.set_vertex_buffer(1, pass_instance_buffers.instance_buffer.slice(..));
            for label_draw in pass_instance_buffers.label_draws {
                let first_instance = label_draw.first_instance;
                match &label_draw.content {
                    LabelDrawContent::Text {
                        geometry_cache_key,
                        background_index_count,
                        draws_background,
                        is_background_opaque,
                        is_text_opaque,
                    } => {
                        let (Some(label_font_atlas_bind_group), Some(label_geometry_buffer)) =
                            (label_font_atlas_bind_group, label_geometry_buffer_of_cache_key.get(geometry_cache_key))
                        else {
                            continue;
                        };
                        let background_instance = first_instance..first_instance + 1;
                        let text_instance = first_instance + 1..first_instance + 2;
                        pass.set_pipeline(label_render_pipeline);
                        pass.set_bind_group(1, label_font_atlas_bind_group, &[]);
                        pass.set_vertex_buffer(0, label_geometry_buffer.vertex_buffer.slice(..));
                        pass.set_index_buffer(label_geometry_buffer.index_buffer.slice(..), label_geometry_buffer.index_format);
                        if *draws_background {
                            pass.draw_indexed(0..*background_index_count, 0, background_instance.clone());
                        }
                        if label_geometry_buffer.num_indices > *background_index_count {
                            pass.draw_indexed(*background_index_count..label_geometry_buffer.num_indices, 0, text_instance.clone());
                        }
                        if let Some(label_depth_render_pipeline) = label_depth_render_pipeline {
                            pass.set_pipeline(label_depth_render_pipeline);
                            if *draws_background && *is_background_opaque {
                                pass.draw_indexed(0..*background_index_count, 0, background_instance.clone());
                            }
                            if label_geometry_buffer.num_indices > *background_index_count && *is_text_opaque {
                                pass.draw_indexed(*background_index_count..label_geometry_buffer.num_indices, 0, text_instance.clone());
                            }
                        }
                    }
                    LabelDrawContent::Image { texture_data_id, is_opaque } => {
                        let Some(label_image_draw_inputs) = label_image_draw_inputs else {
                            continue;
                        };
                        let Some(label_image_bind_group) = label_image_draw_inputs.bind_group_of_data_id.get(texture_data_id) else {
                            continue;
                        };
                        let quad_buffer = label_image_draw_inputs.quad_buffer;
                        let image_instance = first_instance..first_instance + 1;
                        pass.set_pipeline(label_image_draw_inputs.render_pipeline);
                        pass.set_bind_group(1, label_image_bind_group, &[]);
                        pass.set_vertex_buffer(0, quad_buffer.vertex_buffer.slice(..));
                        pass.set_index_buffer(quad_buffer.index_buffer.slice(..), quad_buffer.index_format);
                        pass.draw_indexed(0..quad_buffer.num_indices, 0, image_instance.clone());
                        if let Some(depth_render_pipeline) = label_image_draw_inputs.depth_render_pipeline
                            && *is_opaque
                        {
                            pass.set_pipeline(depth_render_pipeline);
                            pass.draw_indexed(0..quad_buffer.num_indices, 0, image_instance.clone());
                        }
                    }
                }
            }
        }

        // This is the shared geometry draw routine used by both single-color and OIT instance passes.
        // When a mask-only opaque pipeline is provided, it switches only mask materials into that pipeline
        // so alpha-to-coverage stays scoped to cutout edges instead of affecting all opaque draws.
        // The depth replay uses the same switch to draw Opaque materials from their positions alone.
        fn draw_pass_instances(
            pass: &mut wgpu::RenderPass<'_>,
            camera_bind_group: &wgpu::BindGroup,
            lighting_bind_group: &wgpu::BindGroup,
            static_render_pipeline: &[wgpu::RenderPipeline; 2],
            static_render_pipeline_for_alpha_mode: Option<(u32, &[wgpu::RenderPipeline; 2])>,
            skinned_render_pipeline: &[wgpu::RenderPipeline; 2],
            skinned_render_pipeline_for_alpha_mode: Option<(u32, &[wgpu::RenderPipeline; 2])>,
            brick_buffer: &GpuGeometryBuffer,
            ball_buffer: &GpuGeometryBuffer,
            cylinder_buffer: &GpuGeometryBuffer,
            mesh_geometry_buffers: &HashMap<DataId, GpuGeometryBuffer>,
            skinned_mesh_joint_matrix_buffers: &HashMap<SkinnedMeshJointMatrixBufferKey, GpuJointMatrixBuffer>,
            fallback_joint_matrix_buffer: &GpuJointMatrixBuffer,
            material_bind_group_of_key: &HashMap<MaterialTextureDataIds, wgpu::BindGroup>,
            default_material_texture_key: MaterialTextureDataIds,
            pass_instance_buffers: &PassInstanceBuffers<'_>,
        ) {
            fn sync_material_render_pipeline<'a>(
                pass: &mut wgpu::RenderPass<'a>,
                camera_bind_group: &wgpu::BindGroup,
                lighting_bind_group: &wgpu::BindGroup,
                fallback_joint_matrix_buffer: &GpuJointMatrixBuffer,
                material_texture_data_ids: MaterialTextureDataIds,
                render_pipeline: &[wgpu::RenderPipeline; 2],
                render_pipeline_for_alpha_mode: Option<(u32, &[wgpu::RenderPipeline; 2])>,
                material_pipeline_state: &mut (bool, bool),
            ) {
                let should_use_alpha_mode_render_pipeline =
                    render_pipeline_for_alpha_mode.is_some_and(|(alpha_mode, _)| alpha_mode == material_texture_data_ids.alpha_mode);
                let next_state = (should_use_alpha_mode_render_pipeline, material_texture_data_ids.is_double_sided);
                if next_state == *material_pipeline_state {
                    return;
                }
                let next_render_pipeline = match (should_use_alpha_mode_render_pipeline, render_pipeline_for_alpha_mode) {
                    (true, Some((_, alpha_mode_render_pipeline))) => alpha_mode_render_pipeline,
                    _ => render_pipeline,
                };
                pass.set_pipeline(&next_render_pipeline[usize::from(material_texture_data_ids.is_double_sided)]);
                pass.set_bind_group(0, camera_bind_group, &[]);
                pass.set_bind_group(2, &fallback_joint_matrix_buffer.joint_matrix_bind_group, &[]);
                pass.set_bind_group(3, lighting_bind_group, &[]);
                *material_pipeline_state = next_state;
            }

            pass.set_pipeline(&static_render_pipeline[0]);
            pass.set_bind_group(0, camera_bind_group, &[]);
            pass.set_bind_group(2, &fallback_joint_matrix_buffer.joint_matrix_bind_group, &[]);
            pass.set_bind_group(3, lighting_bind_group, &[]);
            pass.set_vertex_buffer(1, pass_instance_buffers.instance_buffer.slice(..));
            let default_material_bind_group = material_bind_group_of_key.get(&default_material_texture_key);
            let mut static_material_pipeline_state = (false, false);

            for (brick_instance_buffer_key, brick_instance_range) in pass_instance_buffers.brick_instance_range_of_key.iter() {
                sync_material_render_pipeline(
                    pass,
                    camera_bind_group,
                    lighting_bind_group,
                    fallback_joint_matrix_buffer,
                    brick_instance_buffer_key.material_texture_data_ids,
                    static_render_pipeline,
                    static_render_pipeline_for_alpha_mode,
                    &mut static_material_pipeline_state,
                );
                let material_bind_group = material_bind_group_of_key
                    .get(&brick_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, brick_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(brick_buffer.index_buffer.slice(..), brick_buffer.index_format);
                pass.draw_indexed(0..brick_buffer.num_indices, 0, brick_instance_range.clone());
            }
            for (ball_instance_buffer_key, ball_instance_range) in pass_instance_buffers.ball_instance_range_of_key.iter() {
                sync_material_render_pipeline(
                    pass,
                    camera_bind_group,
                    lighting_bind_group,
                    fallback_joint_matrix_buffer,
                    ball_instance_buffer_key.material_texture_data_ids,
                    static_render_pipeline,
                    static_render_pipeline_for_alpha_mode,
                    &mut static_material_pipeline_state,
                );
                let material_bind_group = material_bind_group_of_key
                    .get(&ball_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, ball_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(ball_buffer.index_buffer.slice(..), ball_buffer.index_format);
                pass.draw_indexed(
                    ball_buffer.index_range_for_detail_level(ball_instance_buffer_key.detail_level),
                    0,
                    ball_instance_range.clone(),
                );
            }
            for (cylinder_instance_buffer_key, cylinder_instance_range) in pass_instance_buffers.cylinder_instance_range_of_key.iter() {
                sync_material_render_pipeline(
                    pass,
                    camera_bind_group,
                    lighting_bind_group,
                    fallback_joint_matrix_buffer,
                    cylinder_instance_buffer_key.material_texture_data_ids,
                    static_render_pipeline,
                    static_render_pipeline_for_alpha_mode,
                    &mut static_material_pipeline_state,
                );
                let material_bind_group = material_bind_group_of_key
                    .get(&cylinder_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, cylinder_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(cylinder_buffer.index_buffer.slice(..), cylinder_buffer.index_format);
                pass.draw_indexed(0..cylinder_buffer.num_indices, 0, cylinder_instance_range.clone());
            }
            for (mesh_instance_buffer_key, mesh_instance_range) in pass_instance_buffers.mesh_instance_range_of_key.iter() {
                sync_material_render_pipeline(
                    pass,
                    camera_bind_group,
                    lighting_bind_group,
                    fallback_joint_matrix_buffer,
                    mesh_instance_buffer_key.material_texture_data_ids,
                    static_render_pipeline,
                    static_render_pipeline_for_alpha_mode,
                    &mut static_material_pipeline_state,
                );
                let Some(mesh_geometry_buffer) = mesh_geometry_buffers.get(&mesh_instance_buffer_key.mesh_data_id) else {
                    continue;
                };
                let material_bind_group = material_bind_group_of_key
                    .get(&mesh_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, mesh_geometry_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh_geometry_buffer.index_buffer.slice(..), mesh_geometry_buffer.index_format);
                pass.draw_indexed(
                    mesh_geometry_buffer.index_range_for_detail_level(mesh_instance_buffer_key.detail_level),
                    0,
                    mesh_instance_range.clone(),
                );
            }

            if pass_instance_buffers.skinned_mesh_instance_range_of_key.is_empty() {
                return;
            }

            pass.set_pipeline(&skinned_render_pipeline[0]);
            pass.set_bind_group(0, camera_bind_group, &[]);
            pass.set_bind_group(2, &fallback_joint_matrix_buffer.joint_matrix_bind_group, &[]);
            pass.set_bind_group(3, lighting_bind_group, &[]);
            let mut skinned_material_pipeline_state = (false, false);
            for (skinned_mesh_instance_key, mesh_instance_range) in pass_instance_buffers.skinned_mesh_instance_range_of_key.iter() {
                sync_material_render_pipeline(
                    pass,
                    camera_bind_group,
                    lighting_bind_group,
                    fallback_joint_matrix_buffer,
                    skinned_mesh_instance_key.material_texture_data_ids,
                    skinned_render_pipeline,
                    skinned_render_pipeline_for_alpha_mode,
                    &mut skinned_material_pipeline_state,
                );
                let Some(mesh_geometry_buffer) = mesh_geometry_buffers.get(&skinned_mesh_instance_key.mesh_data_id) else {
                    continue;
                };
                let joint_matrix_buffer = if let Some(skeleton_id) = skinned_mesh_instance_key.skeleton_id {
                    let skinned_mesh_joint_matrix_buffer_key = SkinnedMeshJointMatrixBufferKey {
                        mesh_data_id: skinned_mesh_instance_key.mesh_data_id,
                        skeleton_id,
                    };
                    skinned_mesh_joint_matrix_buffers
                        .get(&skinned_mesh_joint_matrix_buffer_key)
                        .unwrap_or(fallback_joint_matrix_buffer)
                } else {
                    fallback_joint_matrix_buffer
                };
                let material_bind_group = material_bind_group_of_key
                    .get(&skinned_mesh_instance_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_bind_group(2, &joint_matrix_buffer.joint_matrix_bind_group, &[]);
                pass.set_vertex_buffer(0, mesh_geometry_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh_geometry_buffer.index_buffer.slice(..), mesh_geometry_buffer.index_format);
                pass.draw_indexed(0..mesh_geometry_buffer.num_indices, 0, mesh_instance_range.clone());
            }
        }

        // This renders one sun shadow cascade's casters from the sun's point of view into its layer of the shadow depth texture.
        // Transparent instances are casters too, so they cast the same solid shadow an opaque instance of the same shape would.
        fn draw_shadow_instances_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            cascade_index: usize,
            shadow_depth_texture_view: &'a wgpu::TextureView,
            shadow_camera_bind_group: &'a wgpu::BindGroup,
            static_shadow_render_pipeline: &'a DepthOnlyRenderPipelines,
            skinned_shadow_render_pipeline: &'a DepthOnlyRenderPipelines,
            brick_buffer: &'a GpuGeometryBuffer,
            ball_buffer: &'a GpuGeometryBuffer,
            cylinder_buffer: &'a GpuGeometryBuffer,
            mesh_geometry_buffers: &'a HashMap<DataId, GpuGeometryBuffer>,
            skinned_mesh_joint_matrix_buffers: &'a HashMap<SkinnedMeshJointMatrixBufferKey, GpuJointMatrixBuffer>,
            fallback_joint_matrix_buffer: &'a GpuJointMatrixBuffer,
            material_bind_group_of_key: &'a HashMap<MaterialTextureDataIds, wgpu::BindGroup>,
            default_material_texture_key: MaterialTextureDataIds,
            shadow_caster_buffers: &ShadowCasterBuffers<'a>,
        ) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Sun Shadow Pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: shadow_depth_texture_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SHADOW_DEPTH_CLEAR_VALUE),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&static_shadow_render_pipeline.alpha_tested[0]);
            pass.set_bind_group(0, shadow_camera_bind_group, &[]);
            pass.set_bind_group(2, &fallback_joint_matrix_buffer.joint_matrix_bind_group, &[]);
            pass.set_vertex_buffer(1, shadow_caster_buffers.instance_buffer.slice(..));
            let default_material_bind_group = material_bind_group_of_key.get(&default_material_texture_key);
            let casts_into_cascade = |bucket_starts: &ShadowCasterBucketStarts| shadow_caster_ranges(bucket_starts, cascade_index).next().is_some();

            for (brick_instance_buffer_key, bucket_starts) in shadow_caster_buffers.brick_bucket_starts_of_key.iter() {
                if !casts_into_cascade(bucket_starts) {
                    continue;
                }
                pass.set_pipeline(static_shadow_render_pipeline.render_pipeline_for_material(brick_instance_buffer_key.material_texture_data_ids));
                let material_bind_group = material_bind_group_of_key
                    .get(&brick_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, brick_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(brick_buffer.index_buffer.slice(..), brick_buffer.index_format);
                for instance_range in shadow_caster_ranges(bucket_starts, cascade_index) {
                    pass.draw_indexed(0..brick_buffer.num_indices, 0, instance_range);
                }
            }
            for (ball_instance_buffer_key, bucket_starts) in shadow_caster_buffers.ball_bucket_starts_of_key.iter() {
                if !casts_into_cascade(bucket_starts) {
                    continue;
                }
                pass.set_pipeline(static_shadow_render_pipeline.render_pipeline_for_material(ball_instance_buffer_key.material_texture_data_ids));
                let material_bind_group = material_bind_group_of_key
                    .get(&ball_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, ball_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(ball_buffer.index_buffer.slice(..), ball_buffer.index_format);
                for instance_range in shadow_caster_ranges(bucket_starts, cascade_index) {
                    pass.draw_indexed(
                        ball_buffer.index_range_for_detail_level(ball_instance_buffer_key.detail_level),
                        0,
                        instance_range,
                    );
                }
            }
            for (cylinder_instance_buffer_key, bucket_starts) in shadow_caster_buffers.cylinder_bucket_starts_of_key.iter() {
                if !casts_into_cascade(bucket_starts) {
                    continue;
                }
                pass.set_pipeline(static_shadow_render_pipeline.render_pipeline_for_material(cylinder_instance_buffer_key.material_texture_data_ids));
                let material_bind_group = material_bind_group_of_key
                    .get(&cylinder_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, cylinder_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(cylinder_buffer.index_buffer.slice(..), cylinder_buffer.index_format);
                for instance_range in shadow_caster_ranges(bucket_starts, cascade_index) {
                    pass.draw_indexed(0..cylinder_buffer.num_indices, 0, instance_range);
                }
            }
            for (mesh_instance_buffer_key, bucket_starts) in shadow_caster_buffers.mesh_bucket_starts_of_key.iter() {
                if !casts_into_cascade(bucket_starts) {
                    continue;
                }
                let Some(mesh_geometry_buffer) = mesh_geometry_buffers.get(&mesh_instance_buffer_key.mesh_data_id) else {
                    continue;
                };
                pass.set_pipeline(static_shadow_render_pipeline.render_pipeline_for_material(mesh_instance_buffer_key.material_texture_data_ids));
                let material_bind_group = material_bind_group_of_key
                    .get(&mesh_instance_buffer_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_vertex_buffer(0, mesh_geometry_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh_geometry_buffer.index_buffer.slice(..), mesh_geometry_buffer.index_format);
                for instance_range in shadow_caster_ranges(bucket_starts, cascade_index) {
                    pass.draw_indexed(
                        mesh_geometry_buffer.index_range_for_detail_level(mesh_instance_buffer_key.detail_level),
                        0,
                        instance_range,
                    );
                }
            }

            if shadow_caster_buffers.skinned_mesh_bucket_starts_of_key.is_empty() {
                return;
            }

            pass.set_pipeline(&skinned_shadow_render_pipeline.alpha_tested[0]);
            pass.set_bind_group(0, shadow_camera_bind_group, &[]);
            pass.set_bind_group(2, &fallback_joint_matrix_buffer.joint_matrix_bind_group, &[]);
            for (skinned_mesh_instance_key, bucket_starts) in shadow_caster_buffers.skinned_mesh_bucket_starts_of_key.iter() {
                if !casts_into_cascade(bucket_starts) {
                    continue;
                }
                let Some(mesh_geometry_buffer) = mesh_geometry_buffers.get(&skinned_mesh_instance_key.mesh_data_id) else {
                    continue;
                };
                let joint_matrix_buffer = if let Some(skeleton_id) = skinned_mesh_instance_key.skeleton_id {
                    let skinned_mesh_joint_matrix_buffer_key = SkinnedMeshJointMatrixBufferKey {
                        mesh_data_id: skinned_mesh_instance_key.mesh_data_id,
                        skeleton_id,
                    };
                    skinned_mesh_joint_matrix_buffers
                        .get(&skinned_mesh_joint_matrix_buffer_key)
                        .unwrap_or(fallback_joint_matrix_buffer)
                } else {
                    fallback_joint_matrix_buffer
                };
                pass.set_pipeline(skinned_shadow_render_pipeline.render_pipeline_for_material(skinned_mesh_instance_key.material_texture_data_ids));
                let material_bind_group = material_bind_group_of_key
                    .get(&skinned_mesh_instance_key.material_texture_data_ids)
                    .or(default_material_bind_group);
                let Some(material_bind_group) = material_bind_group else {
                    continue;
                };
                pass.set_bind_group(1, material_bind_group, &[]);
                pass.set_bind_group(2, &joint_matrix_buffer.joint_matrix_bind_group, &[]);
                pass.set_vertex_buffer(0, mesh_geometry_buffer.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh_geometry_buffer.index_buffer.slice(..), mesh_geometry_buffer.index_format);
                for instance_range in shadow_caster_ranges(bucket_starts, cascade_index) {
                    pass.draw_indexed(0..mesh_geometry_buffer.num_indices, 0, instance_range);
                }
            }
        }

        // This keeps viewport and scissor setup consistent for every pass that supports sub-view rendering.
        fn set_pass_viewport(pass: &mut wgpu::RenderPass<'_>, clamped_viewport: ClampedViewport) {
            if let Some((x, y, width, height)) = clamped_viewport {
                pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, width, height);
            }
        }

        // This creates one-color-target pass descriptors for opaque and composite passes.
        fn begin_single_color_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            label: &'static str,
            color_view: &'a wgpu::TextureView,
            resolve_target: Option<&'a wgpu::TextureView>,
            color_load: wgpu::LoadOp<wgpu::Color>,
            depth_view: Option<&'a wgpu::TextureView>,
            depth_load: Option<wgpu::LoadOp<f32>>,
        ) -> wgpu::RenderPass<'a> {
            let depth_stencil_attachment = match (depth_view, depth_load) {
                (Some(depth_view), Some(depth_load)) => Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                _ => None,
            };

            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: color_load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_stencil_attachment,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
        }

        // This replays opaque geometry into the single-sample depth texture so the later passes can keep using normal sampled depth textures.
        fn draw_depth_only_instances_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            label: &'static str,
            depth_view: &'a wgpu::TextureView,
            depth_load: wgpu::LoadOp<f32>,
            clamped_viewport: ClampedViewport,
            camera_bind_group: &'a wgpu::BindGroup,
            lighting_bind_group: &'a wgpu::BindGroup,
            static_render_pipeline: &'a DepthOnlyRenderPipelines,
            skinned_render_pipeline: &'a DepthOnlyRenderPipelines,
            brick_buffer: &'a GpuGeometryBuffer,
            ball_buffer: &'a GpuGeometryBuffer,
            cylinder_buffer: &'a GpuGeometryBuffer,
            mesh_geometry_buffers: &'a HashMap<DataId, GpuGeometryBuffer>,
            skinned_mesh_joint_matrix_buffers: &'a HashMap<SkinnedMeshJointMatrixBufferKey, GpuJointMatrixBuffer>,
            fallback_joint_matrix_buffer: &'a GpuJointMatrixBuffer,
            material_bind_group_of_key: &'a HashMap<MaterialTextureDataIds, wgpu::BindGroup>,
            default_material_texture_key: MaterialTextureDataIds,
            pass_instance_buffers: &PassInstanceBuffers<'a>,
        ) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            set_pass_viewport(&mut pass, clamped_viewport);
            draw_pass_instances(
                &mut pass,
                camera_bind_group,
                lighting_bind_group,
                &static_render_pipeline.alpha_tested,
                Some((MATERIAL_ALPHA_MODE_OPAQUE, &static_render_pipeline.position_only)),
                &skinned_render_pipeline.alpha_tested,
                Some((MATERIAL_ALPHA_MODE_OPAQUE, &skinned_render_pipeline.position_only)),
                brick_buffer,
                ball_buffer,
                cylinder_buffer,
                mesh_geometry_buffers,
                skinned_mesh_joint_matrix_buffers,
                fallback_joint_matrix_buffer,
                material_bind_group_of_key,
                default_material_texture_key,
                pass_instance_buffers,
            );
        }

        // This is the transparent geometry stage that writes weighted color and revealage OIT buffers.
        fn draw_oit_instances_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            label: &'static str,
            oit_accum_view: &'a wgpu::TextureView,
            oit_weight_view: &'a wgpu::TextureView,
            depth_view: &'a wgpu::TextureView,
            clamped_viewport: ClampedViewport,
            camera_bind_group: &'a wgpu::BindGroup,
            lighting_bind_group: &'a wgpu::BindGroup,
            static_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            skinned_render_pipeline: &'a [wgpu::RenderPipeline; 2],
            brick_buffer: &'a GpuGeometryBuffer,
            ball_buffer: &'a GpuGeometryBuffer,
            cylinder_buffer: &'a GpuGeometryBuffer,
            mesh_geometry_buffers: &'a HashMap<DataId, GpuGeometryBuffer>,
            skinned_mesh_joint_matrix_buffers: &'a HashMap<SkinnedMeshJointMatrixBufferKey, GpuJointMatrixBuffer>,
            fallback_joint_matrix_buffer: &'a GpuJointMatrixBuffer,
            material_bind_group_of_key: &'a HashMap<MaterialTextureDataIds, wgpu::BindGroup>,
            default_material_texture_key: MaterialTextureDataIds,
            pass_instance_buffers: &PassInstanceBuffers<'a>,
        ) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: oit_accum_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: oit_weight_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 0.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            set_pass_viewport(&mut pass, clamped_viewport);
            draw_pass_instances(
                &mut pass,
                camera_bind_group,
                lighting_bind_group,
                static_render_pipeline,
                None,
                skinned_render_pipeline,
                None,
                brick_buffer,
                ball_buffer,
                cylinder_buffer,
                mesh_geometry_buffers,
                skinned_mesh_joint_matrix_buffers,
                fallback_joint_matrix_buffer,
                material_bind_group_of_key,
                default_material_texture_key,
                pass_instance_buffers,
            );
        }

        // Every ambient occlusion pass is a full screen triangle reading this layer's depth through the camera.
        fn draw_ambient_occlusion_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            label: &'static str,
            color_view: &'a wgpu::TextureView,
            clear_color: wgpu::Color,
            clamped_viewport: ClampedViewport,
            camera_bind_group: &'a wgpu::BindGroup,
            render_pipeline: &'a wgpu::RenderPipeline,
            bind_group: &'a wgpu::BindGroup,
        ) {
            let mut pass = begin_single_color_pass(encoder, label, color_view, None, wgpu::LoadOp::Clear(clear_color), None, None);
            set_pass_viewport(&mut pass, clamped_viewport);
            pass.set_pipeline(render_pipeline);
            pass.set_bind_group(0, camera_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // This is the fullscreen stage that composites one layer's OIT result. The base layer's also adds the finished bloom,
        // which bloom_bind_group reads.
        fn draw_fullscreen_composite_pass<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            label: &'static str,
            color_view: &'a wgpu::TextureView,
            color_load: wgpu::LoadOp<wgpu::Color>,
            clamped_viewport: ClampedViewport,
            composite_pipeline: &'a wgpu::RenderPipeline,
            composite_bind_group: &'a wgpu::BindGroup,
            bloom_bind_group: Option<&'a wgpu::BindGroup>,
        ) {
            let mut pass = begin_single_color_pass(encoder, label, color_view, None, color_load, None, None);
            set_pass_viewport(&mut pass, clamped_viewport);
            pass.set_pipeline(composite_pipeline);
            pass.set_bind_group(0, composite_bind_group, &[]);
            if let Some(bloom_bind_group) = bloom_bind_group {
                pass.set_bind_group(1, bloom_bind_group, &[]);
            }
            pass.draw(0..3, 0..1);
        }

        // This runs the full render pipeline for one layer:
        // alpha-tested depth replay into single-sample depth
        // -> that depth as distance from the camera, with coarser levels, then ambient occlusion from it, then its blur
        //    across and then down
        // -> opaque color with MSAA, and alpha-to-coverage only for mask materials, then the sky, in one pass
        // -> transparent OIT.
        // The caller composites the layer, so the base layer's glow can be drawn and blurred first, for its composite to add.
        fn draw_layer_with_oit<'a>(
            encoder: &'a mut wgpu::CommandEncoder,
            clamped_viewport: ClampedViewport,
            layer_shared_draw_inputs: &LayerSharedDrawInputs<'a>,
            layer_pass_pipelines: &LayerPassPipelines<'a>,
            layer_pass_targets: &LayerPassTargets<'a>,
            layer_pass_plan: &LayerPassPlan<'a>,
        ) {
            draw_depth_only_instances_pass(
                encoder,
                layer_pass_plan.opaque_depth_only_pass_label,
                layer_pass_targets.depth_view,
                layer_pass_plan.opaque_depth_load,
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_shared_draw_inputs.lighting_bind_group,
                layer_pass_pipelines.opaque_depth_only_static_render_pipeline,
                layer_pass_pipelines.opaque_depth_only_skinned_render_pipeline,
                layer_shared_draw_inputs.brick_buffer,
                layer_shared_draw_inputs.ball_buffer,
                layer_shared_draw_inputs.cylinder_buffer,
                layer_shared_draw_inputs.mesh_geometry_buffers,
                layer_shared_draw_inputs.skinned_mesh_joint_matrix_buffers,
                layer_shared_draw_inputs.fallback_joint_matrix_buffer,
                layer_shared_draw_inputs.material_bind_group_of_key,
                layer_shared_draw_inputs.default_material_texture_key,
                layer_pass_plan.opaque_pass_instance_buffers,
            );
            draw_ambient_occlusion_pass(
                encoder,
                "Ambient Occlusion View Depth Pass",
                &layer_pass_targets.ambient_occlusion_view_depth_mip_views[0],
                wgpu::Color {
                    r: AMBIENT_OCCLUSION_SKY_VIEW_DEPTH,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                },
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_pass_pipelines.ambient_occlusion_view_depth_render_pipeline,
                layer_pass_targets.ambient_occlusion_view_depth_bind_group,
            );
            for (coarser_mip_view, downsample_bind_group) in layer_pass_targets.ambient_occlusion_view_depth_mip_views[1..]
                .iter()
                .zip(layer_pass_targets.ambient_occlusion_view_depth_downsample_bind_groups)
            {
                draw_ambient_occlusion_pass(
                    encoder,
                    "Ambient Occlusion View Depth Downsample Pass",
                    coarser_mip_view,
                    wgpu::Color::WHITE,
                    None,
                    layer_shared_draw_inputs.camera_bind_group,
                    layer_pass_pipelines.ambient_occlusion_view_depth_downsample_render_pipeline,
                    downsample_bind_group,
                );
            }
            draw_ambient_occlusion_pass(
                encoder,
                "Ambient Occlusion Pass",
                layer_pass_targets.ambient_occlusion_noisy_view,
                wgpu::Color::WHITE,
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_pass_pipelines.ambient_occlusion_render_pipeline,
                layer_pass_targets.ambient_occlusion_bind_group,
            );
            draw_ambient_occlusion_pass(
                encoder,
                "Ambient Occlusion Blur Horizontal Pass",
                layer_pass_targets.ambient_occlusion_blurred_horizontally_view,
                wgpu::Color::WHITE,
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_pass_pipelines.ambient_occlusion_blur_horizontal_render_pipeline,
                layer_pass_targets.ambient_occlusion_blur_horizontal_bind_group,
            );
            draw_ambient_occlusion_pass(
                encoder,
                "Ambient Occlusion Blur Vertical Pass",
                layer_pass_targets.ambient_occlusion_blurred_view,
                wgpu::Color::WHITE,
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_pass_pipelines.ambient_occlusion_blur_vertical_render_pipeline,
                layer_pass_targets.ambient_occlusion_blur_vertical_bind_group,
            );

            // The world and then the sky draw in one multisampled pass, which resolves as it ends. Nothing reads the
            // multisampled color or depth after that, so neither is ever written out.
            let mut opaque_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(layer_pass_plan.opaque_pass_label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: layer_pass_targets.opaque_multisample_color_view,
                    resolve_target: Some(layer_pass_targets.scene_color_view),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: layer_pass_plan.opaque_color_load,
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: layer_pass_targets.opaque_multisample_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: layer_pass_plan.opaque_depth_load,
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            set_pass_viewport(&mut opaque_pass, clamped_viewport);
            draw_pass_instances(
                &mut opaque_pass,
                layer_shared_draw_inputs.camera_bind_group,
                layer_shared_draw_inputs.lighting_bind_group,
                layer_pass_pipelines.opaque_static_render_pipeline,
                Some((MATERIAL_ALPHA_MODE_MASK, layer_pass_pipelines.opaque_mask_static_render_pipeline)),
                layer_pass_pipelines.opaque_skinned_render_pipeline,
                Some((MATERIAL_ALPHA_MODE_MASK, layer_pass_pipelines.opaque_mask_skinned_render_pipeline)),
                layer_shared_draw_inputs.brick_buffer,
                layer_shared_draw_inputs.ball_buffer,
                layer_shared_draw_inputs.cylinder_buffer,
                layer_shared_draw_inputs.mesh_geometry_buffers,
                layer_shared_draw_inputs.skinned_mesh_joint_matrix_buffers,
                layer_shared_draw_inputs.fallback_joint_matrix_buffer,
                layer_shared_draw_inputs.material_bind_group_of_key,
                layer_shared_draw_inputs.default_material_texture_key,
                layer_pass_plan.opaque_pass_instance_buffers,
            );
            // The sky draws after the world, so the depth the world wrote keeps it to the samples geometry left uncovered.
            // It is built up in order: the gradient covers every sky pixel, then a skybox texture draws over it if the world
            // has one, then the sun goes on top of both. Each is a full screen triangle reading the camera and one more group.
            if layer_pass_plan.should_draw_sky {
                let sky_draws = [
                    Some((layer_pass_pipelines.sky_gradient_render_pipeline, layer_shared_draw_inputs.lighting_bind_group)),
                    layer_shared_draw_inputs
                        .skybox_bind_group
                        .map(|skybox_bind_group| (layer_pass_pipelines.skybox_render_pipeline, skybox_bind_group)),
                    Some((layer_pass_pipelines.sky_sun_render_pipeline, layer_shared_draw_inputs.lighting_bind_group)),
                ];
                for (sky_render_pipeline, sky_bind_group) in sky_draws.into_iter().flatten() {
                    opaque_pass.set_pipeline(sky_render_pipeline);
                    opaque_pass.set_bind_group(0, layer_shared_draw_inputs.camera_bind_group, &[]);
                    opaque_pass.set_bind_group(1, sky_bind_group, &[]);
                    opaque_pass.draw(0..3, 0..1);
                }
            }
            drop(opaque_pass);

            // Before the transparent pass, not after: transparent surfaces write no depth, so a label drawn
            // after the composite would have nothing to fail its depth test against and would ignore glass.
            draw_label_pass(
                encoder,
                layer_pass_plan.label_pass_label,
                layer_pass_targets.scene_color_view,
                layer_pass_targets.depth_view,
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_shared_draw_inputs.lighting_bind_group,
                layer_shared_draw_inputs.fallback_joint_matrix_buffer,
                layer_pass_pipelines.label_render_pipeline,
                layer_pass_pipelines.label_depth_render_pipeline,
                layer_shared_draw_inputs.label_geometry_buffer_of_cache_key,
                layer_shared_draw_inputs.label_font_atlas_bind_group,
                &layer_shared_draw_inputs.label_image_draw_inputs,
                layer_pass_plan.transparent_pass_instance_buffers,
            );

            draw_oit_instances_pass(
                encoder,
                layer_pass_plan.transparent_pass_label,
                layer_pass_targets.oit_accum_view,
                layer_pass_targets.oit_weight_view,
                layer_pass_targets.depth_view,
                clamped_viewport,
                layer_shared_draw_inputs.camera_bind_group,
                layer_shared_draw_inputs.lighting_bind_group,
                layer_pass_pipelines.transparent_static_render_pipeline,
                layer_pass_pipelines.transparent_skinned_render_pipeline,
                layer_shared_draw_inputs.brick_buffer,
                layer_shared_draw_inputs.ball_buffer,
                layer_shared_draw_inputs.cylinder_buffer,
                layer_shared_draw_inputs.mesh_geometry_buffers,
                layer_shared_draw_inputs.skinned_mesh_joint_matrix_buffers,
                layer_shared_draw_inputs.fallback_joint_matrix_buffer,
                layer_shared_draw_inputs.material_bind_group_of_key,
                layer_shared_draw_inputs.default_material_texture_key,
                layer_pass_plan.transparent_pass_instance_buffers,
            );
        }

        // ------------------------ No more helpers, actual render_scene code ------------------------

        self.update_skybox_bind_group(scene.skybox_texture_data_id);
        let reflected_skybox_texture_data_id = scene
            .skybox_texture_data_id
            .filter(|skybox_texture_data_id| self.texture_gpu_of_data_id.contains_key(skybox_texture_data_id));
        if self.lighting_bind_group_skybox_data_id != Some(reflected_skybox_texture_data_id) {
            self.rebuild_lighting_bind_group(reflected_skybox_texture_data_id);
        }
        self.queue.write_buffer(&self.lighting_buffer, 0, bytemuck::bytes_of(&scene.lighting));
        let reflected_skybox_mip_level_count = reflected_skybox_texture_data_id.map_or(1, |skybox_texture_data_id| {
            self.texture_gpu_of_data_id[&skybox_texture_data_id].texture_view.texture().mip_level_count()
        });
        let skybox_buffer_content = Self::build_skybox_buffer_content(scene.skybox_texture_opacity, reflected_skybox_mip_level_count);
        self.queue.write_buffer(&self.skybox_buffer, 0, bytemuck::cast_slice(&[skybox_buffer_content]));
        // Now, create per-frame instance contents. Opaque and transparent opacity instances are separated for OIT.
        // Each primitive has 4 buckets: normal/always-on-top x opaque/transparent.
        let mut brick_instance_lists_of_key: HashMap<BrickInstanceBufferKey, InstanceListsOfKey> = HashMap::new();
        let mut ball_instance_lists_of_key: HashMap<BallInstanceBufferKey, InstanceListsOfKey> = HashMap::new();
        let mut cylinder_instance_lists_of_key: HashMap<CylinderInstanceBufferKey, InstanceListsOfKey> = HashMap::new();
        let mut mesh_instance_lists_of_key: HashMap<MeshInstanceBufferKey, InstanceListsOfKey> = HashMap::new();
        let mut skinned_mesh_instance_lists_of_key: HashMap<SkinnedMeshInstanceBufferKey, InstanceListsOfKey> = HashMap::new();

        let default_material_texture_key = MaterialTextureDataIds {
            basecolor_texture_data_id: None,
            metallic_texture_data_id: None,
            roughness_texture_data_id: None,
            normal_texture_data_id: None,
            alpha_mode: MATERIAL_ALPHA_MODE_OPAQUE,
            alpha_cutoff_bits: 0.0f32.to_bits(), // unused
            is_double_sided: false,
        };
        let mut material_texture_keys_used_by_frame: HashSet<MaterialTextureDataIds> = HashSet::new();
        material_texture_keys_used_by_frame.insert(default_material_texture_key);

        let shadow_view_projections = build_shadow_cascade_view_projections(Vec3::from_slice(&scene.lighting.sun_direction), shadow_center);
        let view_edges = ViewEdges::of_view_proj(camera_view_proj);
        let shadow_cascade_squares = ShadowCascadeSquares::new(&shadow_view_projections, SHADOW_CASCADE_HALF_SIZES, SHADOW_MAP_SIZE);
        // How many pixels tall one unit looks from one unit away, which every ball and static mesh uses to pick its detail level.
        let pixels_per_unit_at_unit_distance =
            0.5 * viewport.map_or(self.render_targets.height, |viewport| viewport.height) as f32 * camera_view_proj.row(1).truncate().length();
        for (instance_index, draw) in scene.instances.iter().enumerate() {
            let instance = &draw.content;
            if instance.color[3] == 0.0 {
                continue;
            }
            let material_texture_data_ids = draw.material;
            // Culled instances still count as used, so their material's bind group is still there when they come back.
            material_texture_keys_used_by_frame.insert(material_texture_data_ids);
            let is_always_on_top = draw.is_always_on_top;
            // None stands for a skinned mesh, which is never culled, since its skeleton can pose its vertices anywhere.
            let placement_for_geometry = |geometry_bounding_radius: Option<f32>| {
                let model = instance_model_for_view(instance, camera_view_proj);
                let bounding_sphere = BoundingSphere {
                    center: model.w_axis.truncate(),
                    radius: geometry_bounding_radius.map_or(f32::INFINITY, |radius| radius * largest_stretch(model)),
                };
                let is_in_view = view_edges.contains(bounding_sphere);
                let shadow_caster_bucket = match is_always_on_top {
                    true => None,
                    false => shadow_cascade_squares.shadow_caster_bucket(bounding_sphere),
                };
                (is_in_view || shadow_caster_bucket.is_some()).then(|| InstancePlacement {
                    is_in_view,
                    is_selected: is_in_view && scene.selected_object_ids.contains(&draw.object_id),
                    glows: instance.glow > 0.0 && !is_always_on_top,
                    is_always_on_top,
                    is_transparent: should_render_in_transparent_pass(instance.color[3], material_texture_data_ids),
                    has_opaque_blend_pixels: instance.color[3] == 1.0 && material_texture_data_ids.alpha_mode == MATERIAL_ALPHA_MODE_BLEND,
                    shadow_caster_bucket,
                })
            };
            let instance_index = instance_index as u32;
            match draw.geometry {
                RenderGeometry::Ball => {
                    let Some(placement) = placement_for_geometry(Some(self.ball_buffer.bounding_radius)) else {
                        continue;
                    };
                    let key = BallInstanceBufferKey {
                        material_texture_data_ids,
                        detail_level: detail_level_for_instance(
                            &self.ball_buffer.simpler_detail_levels,
                            self.ball_buffer.bounding_radius,
                            &instance.model,
                            camera_position,
                            pixels_per_unit_at_unit_distance,
                        ),
                    };
                    ball_instance_lists_of_key.entry(key).or_default().push(instance_index, &placement);
                }
                RenderGeometry::Brick => {
                    let Some(placement) = placement_for_geometry(Some(self.brick_buffer.bounding_radius)) else {
                        continue;
                    };
                    let key = BrickInstanceBufferKey { material_texture_data_ids };
                    brick_instance_lists_of_key.entry(key).or_default().push(instance_index, &placement);
                }
                RenderGeometry::Cylinder => {
                    let Some(placement) = placement_for_geometry(Some(self.cylinder_buffer.bounding_radius)) else {
                        continue;
                    };
                    let key = CylinderInstanceBufferKey { material_texture_data_ids };
                    cylinder_instance_lists_of_key.entry(key).or_default().push(instance_index, &placement);
                }
                RenderGeometry::Mesh { data_id, skeleton_id } => {
                    let Some(&is_skinned) = self.is_mesh_geometry_buffer_skinned.get(&data_id) else {
                        continue;
                    };
                    if is_skinned {
                        let Some(placement) = placement_for_geometry(None) else {
                            continue;
                        };
                        let key = SkinnedMeshInstanceBufferKey {
                            mesh_data_id: data_id,
                            material_texture_data_ids,
                            skeleton_id,
                        };
                        skinned_mesh_instance_lists_of_key.entry(key).or_default().push(instance_index, &placement);
                    } else {
                        let mesh_geometry_buffer = &self.mesh_geometry_buffers[&data_id];
                        let Some(placement) = placement_for_geometry(Some(mesh_geometry_buffer.bounding_radius)) else {
                            continue;
                        };
                        let key = MeshInstanceBufferKey {
                            mesh_data_id: data_id,
                            material_texture_data_ids,
                            detail_level: detail_level_for_instance(
                                &mesh_geometry_buffer.simpler_detail_levels,
                                mesh_geometry_buffer.bounding_radius,
                                &instance.model,
                                camera_position,
                                pixels_per_unit_at_unit_distance,
                            ),
                        };
                        mesh_instance_lists_of_key.entry(key).or_default().push(instance_index, &placement);
                    }
                }
            }
        }
        let available_label_geometry_cache_keys = self.update_label_gpu_state(scene);
        let mut label_instance_contents: Vec<InstanceContent> = Vec::new();
        let mut selected_label_draws: Vec<LabelDraw> = Vec::new();
        let mut glowing_label_draws: Vec<LabelDraw> = Vec::new();
        let mut label_draws: Vec<LabelDraw> = Vec::new();
        let mut label_draws_always_on_top: Vec<LabelDraw> = Vec::new();
        let no_label_draws: Vec<LabelDraw> = Vec::new();
        let mut label_image_texture_data_ids_used_by_frame: HashSet<DataId> = HashSet::new();
        for label in &scene.labels {
            let (content, instances, geometry_bounding_radius, glows) = match &label.content {
                LabelContent::Text {
                    geometry,
                    instances,
                    draws_background,
                    ..
                } => {
                    if !available_label_geometry_cache_keys.contains(&geometry.cache_key) {
                        continue;
                    }
                    let Some(label_geometry_buffer) = self.label_geometry_buffer_of_cache_key.get(&geometry.cache_key) else {
                        continue;
                    };
                    let content = LabelDrawContent::Text {
                        geometry_cache_key: geometry.cache_key,
                        background_index_count: geometry.background_index_count,
                        draws_background: *draws_background,
                        is_background_opaque: instances[0].color[3] == 1.0,
                        is_text_opaque: instances[1].color[3] == 1.0,
                    };
                    (content, instances.as_slice(), label_geometry_buffer.bounding_radius, instances[1].glow > 0.0)
                }
                LabelContent::Image { texture_data_id, instance } => {
                    // Until its picture has downloaded, an image is left out rather than drawn as a blank rectangle.
                    if !self.texture_gpu_of_data_id.contains_key(texture_data_id) {
                        continue;
                    }
                    label_image_texture_data_ids_used_by_frame.insert(*texture_data_id);
                    let content = LabelDrawContent::Image {
                        texture_data_id: *texture_data_id,
                        is_opaque: instance.color[3] == 1.0,
                    };
                    (content, std::slice::from_ref(instance), self.label_image_buffer.bounding_radius, false)
                }
            };
            let model = instance_model_for_view(&instances[0], camera_view_proj);
            let bounding_sphere = BoundingSphere {
                center: model.w_axis.truncate(),
                radius: geometry_bounding_radius * largest_stretch(model),
            };
            if !view_edges.contains(bounding_sphere) {
                continue;
            }
            let label_draw = LabelDraw {
                content,
                first_instance: label_instance_contents.len() as u32,
                distance_to_camera_squared: camera_position.distance_squared(bounding_sphere.center),
            };
            label_instance_contents.extend_from_slice(instances);
            if scene.selected_object_ids.contains(&label.object_id) {
                selected_label_draws.push(label_draw.clone());
            }
            if glows && !label.is_always_on_top {
                glowing_label_draws.push(label_draw.clone());
            }
            if label.is_always_on_top {
                label_draws_always_on_top.push(label_draw);
            } else {
                label_draws.push(label_draw);
            }
        }
        for cascade_index in 0..SHADOW_CASCADE_COUNT {
            // The projection is the sun's, but the billboard and constant-size fields stay the player's:
            // a shadow has to be cast by the same oriented, same sized instance the player sees.
            let shadow_camera_buffer_content = CameraMatrixBufferContent {
                view_proj: shadow_view_projections[cascade_index].to_cols_array_2d(),
                inverse_view_proj: shadow_view_projections[cascade_index].inverse().to_cols_array_2d(),
                camera_position: [camera_position.x, camera_position.y, camera_position.z, 1.0],
                view_clip_w_row: clip_w_row_of_view_proj(camera_view_proj),
            };
            self.queue.write_buffer(
                &self.shadow_camera_buffers[cascade_index],
                0,
                bytemuck::cast_slice(&[shadow_camera_buffer_content]),
            );
        }
        let shadow_buffer_content = Self::build_shadow_buffer_content(&shadow_view_projections, scene.sun_shadow_softness);
        self.queue.write_buffer(&self.shadow_buffer, 0, bytemuck::cast_slice(&[shadow_buffer_content]));

        let every_key_instance_lists = brick_instance_lists_of_key
            .values_mut()
            .chain(ball_instance_lists_of_key.values_mut())
            .chain(cylinder_instance_lists_of_key.values_mut())
            .chain(mesh_instance_lists_of_key.values_mut())
            .chain(skinned_mesh_instance_lists_of_key.values_mut());
        let mut instance_count = label_instance_contents.len();
        for instance_lists in every_key_instance_lists {
            sort_instances_front_to_back(&mut instance_lists.opaque, &scene.instances, camera_position);
            sort_instances_front_to_back(&mut instance_lists.opaque_always_on_top, &scene.instances, camera_position);
            instance_count += instance_lists.len();
        }

        self.update_material_bind_groups_for_frame(&material_texture_keys_used_by_frame);
        self.update_label_image_bind_groups_for_frame(&label_image_texture_data_ids_used_by_frame);

        // Every instance list of the frame shares this one instance buffer, each in its own range of it.
        let instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Instance Buffer"),
            // Slicing an empty buffer panics, so a frame with nothing to draw still makes room for one instance.
            size: (instance_count.max(1) * size_of::<InstanceContent>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        let mut instance_buffer_view = instance_buffer.slice(..).get_mapped_range_mut();
        let mut frame_instance_writer = FrameInstanceWriter {
            instance_bytes: &mut instance_buffer_view,
            next_instance: 0,
        };
        frame_instance_writer.write(&label_instance_contents);
        let brick_instance_ranges = InstanceRangesOfKey::write(&brick_instance_lists_of_key, &scene.instances, &mut frame_instance_writer);
        let ball_instance_ranges = InstanceRangesOfKey::write(&ball_instance_lists_of_key, &scene.instances, &mut frame_instance_writer);
        let cylinder_instance_ranges = InstanceRangesOfKey::write(&cylinder_instance_lists_of_key, &scene.instances, &mut frame_instance_writer);
        let mesh_instance_ranges = InstanceRangesOfKey::write(&mesh_instance_lists_of_key, &scene.instances, &mut frame_instance_writer);
        let skinned_mesh_instance_ranges = InstanceRangesOfKey::write(&skinned_mesh_instance_lists_of_key, &scene.instances, &mut frame_instance_writer);
        drop(instance_buffer_view);
        instance_buffer.unmap();
        // Labels write depth only after they are drawn and only where they are solid, so draw order still decides
        // soft edges and labels on the same plane.
        for label_draws in [&mut label_draws, &mut label_draws_always_on_top] {
            label_draws.sort_by(|left, right| {
                right
                    .distance_to_camera_squared
                    .partial_cmp(&left.distance_to_camera_squared)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let selected_pass_instance_buffers = PassInstanceBuffers {
            instance_buffer: &instance_buffer,
            brick_instance_range_of_key: &brick_instance_ranges.selected,
            ball_instance_range_of_key: &ball_instance_ranges.selected,
            cylinder_instance_range_of_key: &cylinder_instance_ranges.selected,
            mesh_instance_range_of_key: &mesh_instance_ranges.selected,
            skinned_mesh_instance_range_of_key: &skinned_mesh_instance_ranges.selected,
            label_draws: &selected_label_draws,
        };
        let glowing_pass_instance_buffers = PassInstanceBuffers {
            instance_buffer: &instance_buffer,
            brick_instance_range_of_key: &brick_instance_ranges.glowing,
            ball_instance_range_of_key: &ball_instance_ranges.glowing,
            cylinder_instance_range_of_key: &cylinder_instance_ranges.glowing,
            mesh_instance_range_of_key: &mesh_instance_ranges.glowing,
            skinned_mesh_instance_range_of_key: &skinned_mesh_instance_ranges.glowing,
            label_draws: &glowing_label_draws,
        };
        let opaque_pass_instance_buffers = PassInstanceBuffers {
            instance_buffer: &instance_buffer,
            brick_instance_range_of_key: &brick_instance_ranges.opaque,
            ball_instance_range_of_key: &ball_instance_ranges.opaque,
            cylinder_instance_range_of_key: &cylinder_instance_ranges.opaque,
            mesh_instance_range_of_key: &mesh_instance_ranges.opaque,
            skinned_mesh_instance_range_of_key: &skinned_mesh_instance_ranges.opaque,
            label_draws: &no_label_draws,
        };
        let transparent_pass_instance_buffers = PassInstanceBuffers {
            instance_buffer: &instance_buffer,
            brick_instance_range_of_key: &brick_instance_ranges.transparent,
            ball_instance_range_of_key: &ball_instance_ranges.transparent,
            cylinder_instance_range_of_key: &cylinder_instance_ranges.transparent,
            mesh_instance_range_of_key: &mesh_instance_ranges.transparent,
            skinned_mesh_instance_range_of_key: &skinned_mesh_instance_ranges.transparent,
            label_draws: &label_draws,
        };
        let opaque_pass_instance_buffers_always_on_top = PassInstanceBuffers {
            instance_buffer: &instance_buffer,
            brick_instance_range_of_key: &brick_instance_ranges.opaque_always_on_top,
            ball_instance_range_of_key: &ball_instance_ranges.opaque_always_on_top,
            cylinder_instance_range_of_key: &cylinder_instance_ranges.opaque_always_on_top,
            mesh_instance_range_of_key: &mesh_instance_ranges.opaque_always_on_top,
            skinned_mesh_instance_range_of_key: &skinned_mesh_instance_ranges.opaque_always_on_top,
            label_draws: &no_label_draws,
        };
        let transparent_pass_instance_buffers_always_on_top = PassInstanceBuffers {
            instance_buffer: &instance_buffer,
            brick_instance_range_of_key: &brick_instance_ranges.transparent_always_on_top,
            ball_instance_range_of_key: &ball_instance_ranges.transparent_always_on_top,
            cylinder_instance_range_of_key: &cylinder_instance_ranges.transparent_always_on_top,
            mesh_instance_range_of_key: &mesh_instance_ranges.transparent_always_on_top,
            skinned_mesh_instance_range_of_key: &skinned_mesh_instance_ranges.transparent_always_on_top,
            label_draws: &label_draws_always_on_top,
        };
        let shadow_caster_buffers = ShadowCasterBuffers {
            instance_buffer: &instance_buffer,
            brick_bucket_starts_of_key: &brick_instance_ranges.shadow_caster_bucket_starts,
            ball_bucket_starts_of_key: &ball_instance_ranges.shadow_caster_bucket_starts,
            cylinder_bucket_starts_of_key: &cylinder_instance_ranges.shadow_caster_bucket_starts,
            mesh_bucket_starts_of_key: &mesh_instance_ranges.shadow_caster_bucket_starts,
            skinned_mesh_bucket_starts_of_key: &skinned_mesh_instance_ranges.shadow_caster_bucket_starts,
        };

        for (mesh_data_id, skeleton_id, joint_matrix) in &scene.joint_matrices {
            let skinned_mesh_joint_matrix_buffer_key = SkinnedMeshJointMatrixBufferKey {
                mesh_data_id: *mesh_data_id,
                skeleton_id: *skeleton_id,
            };
            let joint_matrix = *joint_matrix;
            if self.skinned_mesh_joint_matrix_buffers.contains_key(&skinned_mesh_joint_matrix_buffer_key) {
                let res = self.update_skinned_mesh_joint_matrix_buffer(skinned_mesh_joint_matrix_buffer_key, joint_matrix);
                if let Err(err) = res {
                    log::error!("Error updating skinned mesh joint matrix buffer: {}", err);
                    continue;
                }
            } else {
                let resources = create_joint_matrix_buffer(&self.device, &self.joint_matrix_bind_group_layout, joint_matrix);
                self.skinned_mesh_joint_matrix_buffers.insert(skinned_mesh_joint_matrix_buffer_key, resources);
            }
        }

        // --------------------------------- Start drawing ---------------------------------
        let render_targets = &self.render_targets;
        let clamped_viewport = viewport.map(|viewport| {
            let max_width = render_targets.width.max(1);
            let max_height = render_targets.height.max(1);
            let x = viewport.x.min(max_width - 1);
            let y = viewport.y.min(max_height - 1);
            let width = viewport.width.max(1).min(max_width - x);
            let height = viewport.height.max(1).min(max_height - y);
            (x, y, width, height)
        });

        let view = &render_frame.view;

        let base_layer_textures = &render_targets.base_layer_textures;
        let front_layer_textures = &render_targets.front_layer_textures;
        let skybox_bind_group = scene
            .skybox_texture_data_id
            .and_then(|skybox_texture_data_id| self.skybox_bind_group_of_data_id.get(&skybox_texture_data_id));
        let layer_shared_draw_inputs = LayerSharedDrawInputs {
            camera_bind_group,
            lighting_bind_group: &self.lighting_bind_group,
            skybox_bind_group,
            brick_buffer: &self.brick_buffer,
            ball_buffer: &self.ball_buffer,
            cylinder_buffer: &self.cylinder_buffer,
            mesh_geometry_buffers: &self.mesh_geometry_buffers,
            skinned_mesh_joint_matrix_buffers: &self.skinned_mesh_joint_matrix_buffers,
            fallback_joint_matrix_buffer: &self.fallback_joint_matrix_buffer,
            material_bind_group_of_key: &self.material_bind_group_of_key,
            default_material_texture_key,
            label_geometry_buffer_of_cache_key: &self.label_geometry_buffer_of_cache_key,
            label_font_atlas_bind_group: self.label_font_atlas_gpu.as_ref().map(|atlas_gpu| &atlas_gpu.bind_group),
            label_image_draw_inputs: LabelImageDrawInputs {
                render_pipeline: &self.label_image_render_pipeline,
                depth_render_pipeline: Some(&self.label_image_depth_render_pipeline),
                quad_buffer: &self.label_image_buffer,
                bind_group_of_data_id: &self.label_image_bind_group_of_data_id,
            },
        };
        let layer_pass_pipelines = LayerPassPipelines {
            opaque_static_render_pipeline: &self.render_pipeline_opaque,
            opaque_mask_static_render_pipeline: &self.render_pipeline_opaque_mask,
            opaque_depth_only_static_render_pipeline: &self.render_pipeline_opaque_depth_only,
            opaque_skinned_render_pipeline: &self.skinned_render_pipeline_opaque,
            opaque_mask_skinned_render_pipeline: &self.skinned_render_pipeline_opaque_mask,
            opaque_depth_only_skinned_render_pipeline: &self.skinned_render_pipeline_opaque_depth_only,
            shadow_static_render_pipeline: &self.shadow_render_pipeline,
            shadow_skinned_render_pipeline: &self.skinned_shadow_render_pipeline,
            transparent_static_render_pipeline: &self.render_pipeline_transparent,
            transparent_skinned_render_pipeline: &self.skinned_render_pipeline_transparent,
            skybox_render_pipeline: &self.skybox_render_pipeline,
            sky_gradient_render_pipeline: &self.sky_gradient_render_pipeline,
            sky_sun_render_pipeline: &self.sky_sun_render_pipeline,
            oit_composite_render_pipeline: match scene.should_draw_sky {
                true => &self.oit_composite_pipeline,
                false => &self.oit_composite_see_through_pipeline,
            },
            label_render_pipeline: &self.label_render_pipeline,
            label_depth_render_pipeline: &self.label_depth_render_pipeline,
            ambient_occlusion_view_depth_render_pipeline: &self.ambient_occlusion_view_depth_render_pipeline,
            ambient_occlusion_view_depth_downsample_render_pipeline: &self.ambient_occlusion_view_depth_downsample_render_pipeline,
            ambient_occlusion_render_pipeline: &self.ambient_occlusion_render_pipeline,
            ambient_occlusion_blur_horizontal_render_pipeline: &self.ambient_occlusion_blur_horizontal_render_pipeline,
            ambient_occlusion_blur_vertical_render_pipeline: &self.ambient_occlusion_blur_vertical_render_pipeline,
        };
        let layer_pass_targets_base = LayerPassTargets {
            opaque_multisample_color_view: &base_layer_textures.opaque_multisample_color_texture_view,
            opaque_multisample_depth_view: &base_layer_textures.opaque_multisample_depth_texture_view,
            scene_color_view: &base_layer_textures.scene_color_texture_view,
            depth_view: &base_layer_textures.depth_texture_view,
            oit_accum_view: &base_layer_textures.oit_accum_texture_view,
            oit_weight_view: &base_layer_textures.oit_weight_texture_view,
            composite_output_color_view: view,
            oit_composite_bind_group: &base_layer_textures.oit_composite_bind_group,
            ambient_occlusion_view_depth_mip_views: &render_targets.ambient_occlusion_view_depth_mip_texture_views,
            ambient_occlusion_view_depth_downsample_bind_groups: &render_targets.ambient_occlusion_view_depth_downsample_bind_groups,
            ambient_occlusion_noisy_view: &render_targets.ambient_occlusion_noisy_texture_view,
            ambient_occlusion_blurred_horizontally_view: &render_targets.ambient_occlusion_blurred_horizontally_texture_view,
            ambient_occlusion_blurred_view: &render_targets.ambient_occlusion_blurred_texture_view,
            ambient_occlusion_view_depth_bind_group: &base_layer_textures.ambient_occlusion_view_depth_bind_group,
            ambient_occlusion_bind_group: &render_targets.ambient_occlusion_bind_group,
            ambient_occlusion_blur_horizontal_bind_group: &base_layer_textures.ambient_occlusion_blur_horizontal_bind_group,
            ambient_occlusion_blur_vertical_bind_group: &base_layer_textures.ambient_occlusion_blur_vertical_bind_group,
        };
        let layer_pass_targets_front = LayerPassTargets {
            opaque_multisample_color_view: &front_layer_textures.opaque_multisample_color_texture_view,
            opaque_multisample_depth_view: &front_layer_textures.opaque_multisample_depth_texture_view,
            scene_color_view: &front_layer_textures.scene_color_texture_view,
            depth_view: &front_layer_textures.depth_texture_view,
            oit_accum_view: &front_layer_textures.oit_accum_texture_view,
            oit_weight_view: &front_layer_textures.oit_weight_texture_view,
            composite_output_color_view: view,
            oit_composite_bind_group: &front_layer_textures.oit_composite_bind_group,
            ambient_occlusion_view_depth_mip_views: &render_targets.ambient_occlusion_view_depth_mip_texture_views,
            ambient_occlusion_view_depth_downsample_bind_groups: &render_targets.ambient_occlusion_view_depth_downsample_bind_groups,
            ambient_occlusion_noisy_view: &render_targets.ambient_occlusion_noisy_texture_view,
            ambient_occlusion_blurred_horizontally_view: &render_targets.ambient_occlusion_blurred_horizontally_texture_view,
            ambient_occlusion_blurred_view: &render_targets.ambient_occlusion_blurred_texture_view,
            ambient_occlusion_view_depth_bind_group: &front_layer_textures.ambient_occlusion_view_depth_bind_group,
            ambient_occlusion_bind_group: &render_targets.ambient_occlusion_bind_group,
            ambient_occlusion_blur_horizontal_bind_group: &front_layer_textures.ambient_occlusion_blur_horizontal_bind_group,
            ambient_occlusion_blur_vertical_bind_group: &front_layer_textures.ambient_occlusion_blur_vertical_bind_group,
        };
        // The sky gradient pass repaints every sky sample, so this clear survives only in samples that neither the sky
        // nor geometry ever writes. Clearing to the horizon color keeps those from contributing a color the sky never
        // actually uses. With no sky, the background is left see-through, so whatever is behind the canvas shows.
        let sky_horizon_color = scene.lighting.sky_horizon_color_and_padding;
        let bg_color = match scene.should_draw_sky {
            true => wgpu::Color {
                r: sky_horizon_color[0] as f64,
                g: sky_horizon_color[1] as f64,
                b: sky_horizon_color[2] as f64,
                a: 1.0,
            },
            false => wgpu::Color::TRANSPARENT,
        };
        let did_render_world_before_this_call = render_frame.did_render_world;
        let layer_pass_plan_base = LayerPassPlan {
            opaque_color_load: wgpu::LoadOp::Clear(bg_color),
            opaque_depth_load: wgpu::LoadOp::Clear(DEPTH_CLEAR_VALUE),
            // In overlay mode (edit tab over UI), preserve existing swapchain pixels and only draw the viewport region.
            oit_composite_color_load: if did_render_world_before_this_call {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(bg_color)
            },
            should_draw_sky: scene.should_draw_sky,
            opaque_pass_label: "Opaque Pass",
            opaque_depth_only_pass_label: "Opaque Depth Replay Pass",
            transparent_pass_label: "Transparent OIT Pass",
            oit_composite_pass_label: "OIT Composite Pass",
            label_pass_label: "Label Pass",
            opaque_pass_instance_buffers: &opaque_pass_instance_buffers,
            transparent_pass_instance_buffers: &transparent_pass_instance_buffers,
        };
        let layer_pass_plan_front = LayerPassPlan {
            opaque_color_load: wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }),
            opaque_depth_load: wgpu::LoadOp::Clear(DEPTH_CLEAR_VALUE),
            // oit_composite_color_load=Load preserves base-layer pixels so this composite pass alpha-blends front-layer results over them.
            oit_composite_color_load: wgpu::LoadOp::Load,
            should_draw_sky: false,
            opaque_pass_label: "Always-On-Top Opaque Pass",
            opaque_depth_only_pass_label: "Always-On-Top Opaque Depth Replay Pass",
            transparent_pass_label: "Always-On-Top Transparent OIT Pass",
            oit_composite_pass_label: "Always-On-Top Composite Pass",
            label_pass_label: "Always-On-Top Label Pass",
            opaque_pass_instance_buffers: &opaque_pass_instance_buffers_always_on_top,
            transparent_pass_instance_buffers: &transparent_pass_instance_buffers_always_on_top,
        };

        // This updates each sun shadow cascade before shaded passes sample them.
        for cascade_index in 0..SHADOW_CASCADE_COUNT {
            draw_shadow_instances_pass(
                &mut render_frame.encoder,
                cascade_index,
                &self.shadow_depth_texture_views[cascade_index],
                &self.shadow_camera_bind_groups[cascade_index],
                layer_pass_pipelines.shadow_static_render_pipeline,
                layer_pass_pipelines.shadow_skinned_render_pipeline,
                layer_shared_draw_inputs.brick_buffer,
                layer_shared_draw_inputs.ball_buffer,
                layer_shared_draw_inputs.cylinder_buffer,
                layer_shared_draw_inputs.mesh_geometry_buffers,
                layer_shared_draw_inputs.skinned_mesh_joint_matrix_buffers,
                layer_shared_draw_inputs.fallback_joint_matrix_buffer,
                layer_shared_draw_inputs.material_bind_group_of_key,
                layer_shared_draw_inputs.default_material_texture_key,
                &shadow_caster_buffers,
            );
        }

        // This draws the base (non-always-on-top) layer.
        draw_layer_with_oit(
            &mut render_frame.encoder,
            clamped_viewport,
            &layer_shared_draw_inputs,
            &layer_pass_pipelines,
            &layer_pass_targets_base,
            &layer_pass_plan_base,
        );

        // The sun glows through bloom too, whenever it is somewhere on screen. Every point along the sun's direction lands
        // on the same spot on screen, so one step along it is enough to find where.
        let sun_clip_position = camera_view_proj * (camera_position + Vec3::from_slice(&scene.lighting.sun_direction[..3])).extend(1.0);
        let sun_ndc_position = sun_clip_position.truncate().truncate() / sun_clip_position.w;
        let is_sun_on_screen = scene.should_draw_sky
            && scene.lighting.sun_color_and_intensity[3] > 0.0
            && sun_clip_position.w > 0.0
            && sun_ndc_position.abs().max_element() < SUN_GLOW_REACH_NDC;
        // Glowing objects and the sun spread their glow over the base layer, which its composite adds as it puts the layer on
        // the picture, before the always-on-top layer draws over both. A frame with nothing glowing skips all of this.
        let draws_bloom = is_sun_on_screen || !pass_instance_buffers_are_empty(&glowing_pass_instance_buffers);
        if draws_bloom {
            let mut glow_pass = begin_single_color_pass(
                &mut render_frame.encoder,
                "Glow Pass",
                &render_targets.bloom_level_texture_views[0],
                None,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                None,
                None,
            );
            // Bloom level 0 is half the picture's size, so the glow draws the same part of the picture at half the size.
            let (x, y, width, height) = clamped_viewport.unwrap_or((0, 0, render_targets.width, render_targets.height));
            glow_pass.set_viewport(x as f32 / 2.0, y as f32 / 2.0, width as f32 / 2.0, height as f32 / 2.0, 0.0, 1.0);
            // The glow pipelines read the scene's depth where the others read the lighting.
            draw_pass_instances(
                &mut glow_pass,
                camera_bind_group,
                &render_targets.glow_scene_depth_bind_group,
                &self.glow_render_pipeline,
                None,
                &self.skinned_glow_render_pipeline,
                None,
                &self.brick_buffer,
                &self.ball_buffer,
                &self.cylinder_buffer,
                &self.mesh_geometry_buffers,
                &self.skinned_mesh_joint_matrix_buffers,
                &self.fallback_joint_matrix_buffer,
                &self.material_bind_group_of_key,
                default_material_texture_key,
                &glowing_pass_instance_buffers,
            );
            draw_pass_labels(
                &mut glow_pass,
                camera_bind_group,
                &render_targets.glow_scene_depth_bind_group,
                &self.fallback_joint_matrix_buffer,
                &self.label_glow_render_pipeline,
                None,
                &self.label_geometry_buffer_of_cache_key,
                self.label_font_atlas_gpu.as_ref().map(|atlas| &atlas.bind_group),
                None,
                &glowing_pass_instance_buffers,
            );
            if is_sun_on_screen {
                glow_pass.set_pipeline(&self.sun_glow_render_pipeline);
                glow_pass.set_bind_group(0, camera_bind_group, &[]);
                glow_pass.set_bind_group(1, &self.lighting_bind_group, &[]);
                glow_pass.set_bind_group(2, &render_targets.glow_scene_depth_bind_group, &[]);
                glow_pass.draw(0..3, 0..1);
            }
            drop(glow_pass);
            for (level, bloom_downsample_bind_group) in render_targets.bloom_downsample_bind_groups.iter().enumerate() {
                let mut pass = begin_single_color_pass(
                    &mut render_frame.encoder,
                    "Bloom Downsample Pass",
                    &render_targets.bloom_level_texture_views[level + 1],
                    None,
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    None,
                    None,
                );
                pass.set_pipeline(&self.bloom_downsample_render_pipeline);
                pass.set_bind_group(0, bloom_downsample_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            for (level, bloom_upsample_bind_group) in render_targets.bloom_upsample_bind_groups.iter().enumerate().rev() {
                let mut pass = begin_single_color_pass(
                    &mut render_frame.encoder,
                    "Bloom Upsample Pass",
                    &render_targets.bloom_level_texture_views[level],
                    None,
                    wgpu::LoadOp::Load,
                    None,
                    None,
                );
                pass.set_pipeline(&self.bloom_upsample_render_pipeline);
                pass.set_bind_group(0, bloom_upsample_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        draw_fullscreen_composite_pass(
            &mut render_frame.encoder,
            layer_pass_plan_base.oit_composite_pass_label,
            layer_pass_targets_base.composite_output_color_view,
            layer_pass_plan_base.oit_composite_color_load,
            clamped_viewport,
            if draws_bloom {
                &self.oit_composite_with_bloom_pipeline
            } else {
                layer_pass_pipelines.oit_composite_render_pipeline
            },
            layer_pass_targets_base.oit_composite_bind_group,
            draws_bloom.then_some(&render_targets.bloom_composite_bind_group),
        );

        // This draws the always-on-top layer with the same pass flow, then alpha-composites it over the base layer.
        // The layer is usually empty, and every pass clears its targets on begin, so skipping it entirely saves
        // four full-resolution passes (MSAA clear+resolve, depth replay, OIT, fullscreen composite) with no visual change.
        let is_front_layer_empty = pass_instance_buffers_are_empty(&opaque_pass_instance_buffers_always_on_top)
            && pass_instance_buffers_are_empty(&transparent_pass_instance_buffers_always_on_top);
        if !is_front_layer_empty {
            draw_layer_with_oit(
                &mut render_frame.encoder,
                clamped_viewport,
                &layer_shared_draw_inputs,
                &layer_pass_pipelines,
                &layer_pass_targets_front,
                &layer_pass_plan_front,
            );
            draw_fullscreen_composite_pass(
                &mut render_frame.encoder,
                layer_pass_plan_front.oit_composite_pass_label,
                layer_pass_targets_front.composite_output_color_view,
                layer_pass_plan_front.oit_composite_color_load,
                clamped_viewport,
                layer_pass_pipelines.oit_composite_render_pipeline,
                layer_pass_targets_front.oit_composite_bind_group,
                None,
            );
        }

        // Selection stays visible through occluders, matching the desktop editor's overlay behavior.
        if !pass_instance_buffers_are_empty(&selected_pass_instance_buffers) {
            let mut pass = begin_single_color_pass(
                &mut render_frame.encoder,
                "Selection Mask Pass",
                &render_targets.selection_mask_texture_view,
                None,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                None,
                None,
            );
            set_pass_viewport(&mut pass, clamped_viewport);
            draw_pass_instances(
                &mut pass,
                camera_bind_group,
                &self.lighting_bind_group,
                &self.selection_mask_render_pipeline,
                None,
                &self.skinned_selection_mask_render_pipeline,
                None,
                &self.brick_buffer,
                &self.ball_buffer,
                &self.cylinder_buffer,
                &self.mesh_geometry_buffers,
                &self.skinned_mesh_joint_matrix_buffers,
                &self.fallback_joint_matrix_buffer,
                &self.material_bind_group_of_key,
                default_material_texture_key,
                &selected_pass_instance_buffers,
            );
            draw_pass_labels(
                &mut pass,
                camera_bind_group,
                &self.lighting_bind_group,
                &self.fallback_joint_matrix_buffer,
                &self.label_selection_mask_render_pipeline,
                None,
                &self.label_geometry_buffer_of_cache_key,
                self.label_font_atlas_gpu.as_ref().map(|atlas| &atlas.bind_group),
                Some(&LabelImageDrawInputs {
                    render_pipeline: &self.label_image_selection_mask_render_pipeline,
                    depth_render_pipeline: None,
                    quad_buffer: &self.label_image_buffer,
                    bind_group_of_data_id: &self.label_image_bind_group_of_data_id,
                }),
                &selected_pass_instance_buffers,
            );
            drop(pass);
            draw_fullscreen_composite_pass(
                &mut render_frame.encoder,
                "Selection Outline Pass",
                view,
                wgpu::LoadOp::Load,
                clamped_viewport,
                &self.selection_outline_pipeline,
                &render_targets.selection_outline_bind_group,
                None,
            );
        }
        render_frame.did_render_world = true;
    }
}
