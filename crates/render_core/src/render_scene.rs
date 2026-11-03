//! Convert game render snapshots into GPU scene inputs. Both native games and browser previews use this path.
//! Resolved scene inputs shared by desktop games and browser previews. Loading, physics, and animation live with the caller.
//! GPU layouts shared by native rendering and the browser preview.
//! These are the Rust buffer layouts, vertex attributes, and constants consumed by WGSL shaders.
//! Field types, offsets, alignment, and numeric constants must match the corresponding shader inputs;
//! these definitions do not generate or automatically synchronize the WGSL declarations.
//! Moved from the desktop renderer so scene conversion and both renderers use the same definitions.

use crate::{
    computeSkeletonInfo::computeInverseBindMatrices,
    computeSkeletonPose::{SkeletonPose, computeFollowedSkJointMatrixGivenSkJointToFollow},
    renderer_types::{RenderLabelContent, RenderObject, RenderSkeletonInfo},
};
use bytemuck::{Pod, Zeroable};
use game_types::{
    data_types::{DataId, MeshData},
    object_types::{
        BallMaterialSettings, BrickMaterialSettings, CylinderMaterialSettings, MaterialInfo, MeshMaterialSettings, ObjectId, ObjectIdPointer,
        WorldLightingInfo,
        label::{BillboardMode, CONSTANT_SIZE_REFERENCE_DISTANCE},
        material::AlphaMode,
        shared::Color3,
    },
};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

// ------------------------------ Shader Inputs ------------------------------

pub const MAX_JOINTS: usize = 128; // standard max number of joints per rig
pub const SHADOW_CASCADE_COUNT: usize = 5;
// Keep these values synced with scene.wgsl MATERIAL_ALPHA_MODE_* constants.
// The fragment shader reads the numeric alpha mode from a uniform buffer.
pub const MATERIAL_ALPHA_MODE_OPAQUE: u32 = 0;
pub const MATERIAL_ALPHA_MODE_MASK: u32 = 1;
pub const MATERIAL_ALPHA_MODE_BLEND: u32 = 2;
// Keep these values synced with scene.wgsl INSTANCE_TEXTURE_MAPPING_* constants.
pub const INSTANCE_TEXTURE_MAPPING_MESH_UV: u32 = 0;
pub const INSTANCE_TEXTURE_MAPPING_LOCAL_BOX: u32 = 1;
pub const INSTANCE_TEXTURE_MAPPING_LOCAL_SPHERICAL: u32 = 2;
pub const INSTANCE_TEXTURE_MAPPING_LOCAL_CYLINDRICAL: u32 = 3;

// ------------------------ Define structs used in wgsl files -----------------------
// and @location(n) "Attributes" and "Layouts"

// VertexContent and how to read it
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize)]
pub struct VertexContent {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4], // tangents are redundant information from the mesh's UVs and normals, but need to be computed from their neighbors so are computed and stored here for efficiency
}

pub const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = [
    // @location(0) position vec3<f32>
    wgpu::VertexAttribute {
        offset: 0,
        shader_location: 0,
        format: wgpu::VertexFormat::Float32x3,
    },
    // @location(1) normal vec3<f32>
    wgpu::VertexAttribute {
        offset: 12,
        shader_location: 1,
        format: wgpu::VertexFormat::Float32x3,
    },
    // @location(9) uv vec2<f32>
    wgpu::VertexAttribute {
        offset: 24,
        shader_location: 9,
        format: wgpu::VertexFormat::Float32x2,
    },
    // @location(10) tangent vec4<f32> (xyz=tangent, w=bitangent sign)
    wgpu::VertexAttribute {
        offset: 32,
        shader_location: 10,
        format: wgpu::VertexFormat::Float32x4,
    },
];

// SkinnedVertexContent and how to read it
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinnedVertexContent {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
    pub joint_indices: [u32; 4],
    pub joint_weights: [f32; 4],
}

pub const SKINNED_VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 6] = [
    // @location(0) position vec3<f32>
    wgpu::VertexAttribute {
        offset: 0,
        shader_location: 0,
        format: wgpu::VertexFormat::Float32x3,
    },
    // @location(1) normal vec3<f32>
    wgpu::VertexAttribute {
        offset: 12,
        shader_location: 1,
        format: wgpu::VertexFormat::Float32x3,
    },
    // @location(9) uv vec2<f32>
    wgpu::VertexAttribute {
        offset: 24,
        shader_location: 9,
        format: wgpu::VertexFormat::Float32x2,
    },
    // @location(10) tangent vec4<f32> (xyz=tangent, w=bitangent sign)
    wgpu::VertexAttribute {
        offset: 32,
        shader_location: 10,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(7) joint_indices
    wgpu::VertexAttribute {
        offset: 48,
        shader_location: 7,
        format: wgpu::VertexFormat::Uint32x4,
    },
    // @location(8) joint_weights
    wgpu::VertexAttribute {
        offset: 64,
        shader_location: 8,
        format: wgpu::VertexFormat::Float32x4,
    },
];
// InstanceContent + how to read it
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceContent {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub texture_mapping_mode: u32,
    // Billboard mode plus the INSTANCE_FLAG_* bits. This used to be padding, so the layout is unchanged.
    pub instance_flags: u32,
    // 0-1: how much the instance shines in its own color and glows onto its surroundings. Also carved out of padding.
    pub glow: f32,
    pub _texture_mapping_padding: u32,
    pub texture_tile_transform: [f32; 4],
    pub local_projection_scale: [f32; 4],
}

pub const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = [
    // @location(2) model matrix row 0
    wgpu::VertexAttribute {
        offset: 0,
        shader_location: 2,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(3) model matrix row 1
    wgpu::VertexAttribute {
        offset: 16,
        shader_location: 3,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(4) model matrix row 2
    wgpu::VertexAttribute {
        offset: 32,
        shader_location: 4,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(5) model matrix row 3
    wgpu::VertexAttribute {
        offset: 48,
        shader_location: 5,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(6) instance color vec4<f32>
    wgpu::VertexAttribute {
        offset: 64,
        shader_location: 6,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(11) texture mapping mode u32
    wgpu::VertexAttribute {
        offset: 80,
        shader_location: 11,
        format: wgpu::VertexFormat::Uint32,
    },
    // @location(12) texture tile transform vec4<f32>
    wgpu::VertexAttribute {
        offset: 96,
        shader_location: 12,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(13) local projection scale vec4<f32>
    wgpu::VertexAttribute {
        offset: 112,
        shader_location: 13,
        format: wgpu::VertexFormat::Float32x4,
    },
    // @location(14) instance flags u32
    wgpu::VertexAttribute {
        offset: 84,
        shader_location: 14,
        format: wgpu::VertexFormat::Uint32,
    },
    // @location(15) glow f32
    wgpu::VertexAttribute {
        offset: 88,
        shader_location: 15,
        format: wgpu::VertexFormat::Float32,
    },
];

// TODO: continue the pattern of struct then ATTRIBUTES?
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct CameraMatrixBufferContent {
    pub view_proj: [[f32; 4]; 4],
    pub inverse_view_proj: [[f32; 4]; 4],
    // The camera the player looks through. The shadow pass projects with the sun's matrix but still
    // fills these two in from the player's camera, because a billboard has to face the player and a
    // constant-size instance has to be sized against the player's view for its shadow to match it.
    pub camera_position: [f32; 4],
    // Row 3 of that player camera's view-projection, which dotted with a world position gives its
    // clip w: the view space depth perspective divides by.
    pub view_clip_w_row: [f32; 4],
}

// glam matrices are column major, so row 3 is the w component of each column.
pub fn clip_w_row_of_view_proj(view_proj: Mat4) -> [f32; 4] {
    [view_proj.x_axis.w, view_proj.y_axis.w, view_proj.z_axis.w, view_proj.w_axis.w]
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LightingBufferContent {
    pub sun_direction: [f32; 4],
    pub sun_color_and_intensity: [f32; 4],
    pub ambient_top_color_and_intensity: [f32; 4],
    pub ambient_bottom_color_and_padding: [f32; 4],
    pub sky_top_color_and_padding: [f32; 4],
    pub sky_horizon_color_and_padding: [f32; 4],
    pub sky_bottom_color_and_padding: [f32; 4],
    pub ambient_occlusion_settings: [f32; 4],
    pub sky_reflection_settings: [f32; 4],
    pub display_settings: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct ShadowBufferContent {
    pub light_view_proj: [[[f32; 4]; 4]; SHADOW_CASCADE_COUNT],
    // x: half the world width the cascade covers, y: the world width of one of its texels.
    pub cascade_settings: [[f32; 4]; SHADOW_CASCADE_COUNT],
    // x: receiver depth bias and y: receiver normal bias, in world units. z: how far every shadow edge blurs, in world
    // units. w: world distance per unit of shadow depth.
    pub settings: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SkyboxBufferContent {
    pub settings: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct JointMatrixBufferContent {
    pub matrices: [[[f32; 4]; 4]; MAX_JOINTS],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MaterialAlphaModeBufferContent {
    pub alpha_mode: u32,
    pub alpha_cutoff: f32,
    pub _padding0: u32,
    pub _padding1: u32,
}

// ------------------------------ Render Scene Types ------------------------------

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MaterialTextureDataIds {
    pub basecolor_texture_data_id: Option<DataId>,
    pub metallic_texture_data_id: Option<DataId>,
    pub roughness_texture_data_id: Option<DataId>,
    pub normal_texture_data_id: Option<DataId>,
    pub alpha_mode: u32,
    pub is_double_sided: bool,
    pub alpha_cutoff_bits: u32, // f32 cannot be used as a Hash/Eq, use its raw bits as the key
}
impl Default for MaterialTextureDataIds {
    fn default() -> Self {
        Self {
            basecolor_texture_data_id: None,
            metallic_texture_data_id: None,
            roughness_texture_data_id: None,
            normal_texture_data_id: None,
            alpha_mode: 0,
            is_double_sided: false,
            alpha_cutoff_bits: 0,
        }
    }
}

pub enum RenderGeometry {
    Ball,
    Brick,
    Cylinder,
    Mesh { data_id: DataId, skeleton_id: Option<ObjectId> },
}
pub struct RenderInstance {
    pub object_id: ObjectId,
    pub geometry: RenderGeometry,
    pub content: InstanceContent,
    pub material: MaterialTextureDataIds,
    pub is_always_on_top: bool,
}
// The client hands over finished pixels, so the renderer never needs to know what a font is.
#[derive(Serialize, Deserialize)]
pub struct FontAtlas {
    pub version: u64,
    pub width: u32,
    pub height: u32,
    // One byte per texel of how far it is from a letter's edge: 0.5 on the edge, more inside, less outside. Each level is
    // half the size of the one before. The shader takes color from the instance.
    pub mip_levels: Vec<Vec<u8>>,
}

// Tessellated text in layout units, anchored at the origin.
#[derive(Serialize, Deserialize)]
pub struct LabelGeometry {
    pub cache_key: u64,
    pub vertices: Vec<VertexContent>,
    pub indices: Vec<u32>,
    // Leading indices form the background panel, drawn as its own instance so it can take its own color.
    pub background_index_count: u32,
}

pub struct RenderLabel {
    pub object_id: ObjectId,
    pub content: LabelContent,
    pub is_always_on_top: bool,
}

pub enum LabelContent {
    // The background panel's instance first, then the text's, so each can take its own color.
    Text {
        geometry: Arc<LabelGeometry>,
        atlas: Arc<FontAtlas>,
        instances: [InstanceContent; 2],
        draws_background: bool,
    },
    // Drawn on the shared label image quad, sized by its instance.
    Image {
        texture_data_id: DataId,
        instance: InstanceContent,
    },
}

pub struct RenderScene {
    // The caller supplies selection for this frame; it never changes saved object properties.
    pub selected_object_ids: HashSet<ObjectId>,
    pub instances: Vec<RenderInstance>,
    pub labels: Vec<RenderLabel>,
    pub joint_matrices: Vec<(DataId, ObjectId, JointMatrixBufferContent)>,
    pub lighting: LightingBufferContent,
    pub sun_shadow_softness: f32,
    pub skybox_texture_data_id: Option<DataId>,
    pub skybox_texture_opacity: f32,
    pub should_draw_sky: bool,
}

// ------------------------------ Scene Conversion ------------------------------

// ------------------------------ Game materials and scene transforms ------------------------------

// Objects, cameras, and labels share this world transform after physics interpolation and animation.
// Primitive sizes, mesh scales, and label offsets are applied by the renderer afterward.
pub fn computeRenderObjectTransform(
    render_object_of_id: &HashMap<ObjectId, RenderObject>,
    skeleton_pose_of_id: &HashMap<ObjectId, SkeletonPose>,
    object_id: ObjectId,
) -> Option<Mat4> {
    // A label carries a copy of its followed object's location, so it has to be smoothed
    // on identical terms or it lags and sinks into the surface it sits on.
    // Resolve that location from the target here instead of storing and interpolating a separate copy.
    let render_object = match render_object_of_id.get(&object_id)? {
        RenderObject::RenderLabel(label) => render_object_of_id.get(&label.physicalObjectId)?,
        RenderObject::Mesh(mesh) => render_object_of_id.get(&mesh.physicalObjectId)?,
        render_object => render_object,
    };
    let transform = render_object.getTransform()?;
    if let Some(sk_joint_to_follow) = transform.sk_joint_to_follow_arc.as_deref()
        && let Some(transform) = computeFollowedSkJointMatrixGivenSkJointToFollow(render_object_of_id, skeleton_pose_of_id, sk_joint_to_follow)
    {
        return Some(transform);
    }
    // Welded parts inherit the body's final transform instead of moving along independent world-space curves.
    if let Some((group_id, local_location)) = &transform.rigid_group_to_follow {
        let RenderObject::RigidGroup(group) = render_object_of_id.get(group_id)? else {
            return None;
        };
        let group_location = &group.physicsInfo3D.location;
        return Some(
            Mat4::from_rotation_translation(group_location.quaternion, group_location.position)
                * Mat4::from_rotation_translation(local_location.quaternion, local_location.position),
        );
    }
    Some(Mat4::from_rotation_translation(
        transform.physicsInfo3D.location.quaternion,
        transform.physicsInfo3D.location.position,
    ))
}

fn insert_material_texture_data_ids(texture_data_ids: &mut HashSet<DataId>, material_info: &MaterialInfo) {
    if let Some(albedo_texture_data_id) = material_info.albedoTextureDataId {
        texture_data_ids.insert(albedo_texture_data_id);
    }
    if let Some(metallic_texture_data_id) = material_info.metallicTextureDataId {
        texture_data_ids.insert(metallic_texture_data_id);
    }
    if let Some(roughness_texture_data_id) = material_info.roughnessTextureDataId {
        texture_data_ids.insert(roughness_texture_data_id);
    }
    if let Some(normal_texture_data_id) = material_info.normalTextureDataId {
        texture_data_ids.insert(normal_texture_data_id);
    }
}

fn material_alpha_mode_key(alpha_mode: AlphaMode) -> (u32, u32) {
    match alpha_mode {
        AlphaMode::Blend => (MATERIAL_ALPHA_MODE_BLEND, 0.0f32.to_bits()),
        AlphaMode::Opaque => (MATERIAL_ALPHA_MODE_OPAQUE, 0.0f32.to_bits()),
        AlphaMode::Mask { cutoff } => (MATERIAL_ALPHA_MODE_MASK, cutoff.clamp(0.0, 1.0).to_bits()),
    }
}

fn material_texture_key_of_material_info(material_info: &MaterialInfo) -> MaterialTextureDataIds {
    let (alpha_mode, alpha_cutoff_bits) = material_alpha_mode_key(material_info.alphaMode);
    MaterialTextureDataIds {
        basecolor_texture_data_id: material_info.albedoTextureDataId,
        metallic_texture_data_id: material_info.metallicTextureDataId,
        roughness_texture_data_id: material_info.roughnessTextureDataId,
        normal_texture_data_id: material_info.normalTextureDataId,
        alpha_mode,
        alpha_cutoff_bits,
        is_double_sided: material_info.isDoubleSided,
    }
}

fn material_info_of_material_pointer(
    render_object_of_id: &HashMap<ObjectId, RenderObject>,
    material_info_id: Option<ObjectIdPointer>,
) -> Option<&MaterialInfo> {
    let material_object_id: ObjectId = material_info_id.map(|id| id.into())?;
    let Some(render_object) = render_object_of_id.get(&material_object_id) else {
        return None;
    };
    let RenderObject::Material(material_info) = render_object else {
        return None;
    };
    Some(material_info)
}

fn material_texture_key_of_material_pointer(
    render_object_of_id: &HashMap<ObjectId, RenderObject>,
    material_info_id: Option<ObjectIdPointer>,
    default_material_texture_key: MaterialTextureDataIds,
) -> MaterialTextureDataIds {
    let Some(material_info) = material_info_of_material_pointer(render_object_of_id, material_info_id) else {
        return default_material_texture_key;
    };
    material_texture_key_of_material_info(material_info)
}

fn material_info_id_of_ball_material(ball_material: &Option<BallMaterialSettings>) -> Option<ObjectIdPointer> {
    ball_material.as_ref().map(|ball_material| ball_material.materialInfoId)
}

fn material_info_id_of_brick_material(brick_material: &Option<BrickMaterialSettings>) -> Option<ObjectIdPointer> {
    brick_material.as_ref().map(|brick_material| brick_material.materialInfoId)
}

fn material_info_id_of_cylinder_material(cylinder_material: &Option<CylinderMaterialSettings>) -> Option<ObjectIdPointer> {
    cylinder_material.as_ref().map(|cylinder_material| cylinder_material.materialInfoId)
}

fn material_info_id_of_mesh_material(mesh_material_settings: &Option<MeshMaterialSettings>) -> Option<ObjectIdPointer> {
    mesh_material_settings
        .as_ref()
        .map(|mesh_material_settings| mesh_material_settings.materialInfoId)
}

// Bricks use local box mapping when they have material settings and fall back to mesh UVs otherwise.
fn brick_instance_texture_mapping_mode_and_tile_transform(brick_material: &Option<BrickMaterialSettings>) -> (u32, [f32; 4]) {
    let Some(brick_material) = brick_material else {
        return (INSTANCE_TEXTURE_MAPPING_MESH_UV, [0.0, 0.0, 0.0, 0.0]);
    };

    (
        INSTANCE_TEXTURE_MAPPING_LOCAL_BOX,
        [
            brick_material.tileSizeU,
            brick_material.tileSizeV,
            brick_material.tileOffsetU,
            brick_material.tileOffsetV,
        ],
    )
}

// Balls use spherical mapping when they have material settings and fall back to mesh UVs otherwise.
fn ball_instance_texture_mapping_mode_and_tile_transform(ball_material: &Option<BallMaterialSettings>) -> (u32, [f32; 4]) {
    let Some(ball_material) = ball_material else {
        return (INSTANCE_TEXTURE_MAPPING_MESH_UV, [0.0, 0.0, 0.0, 0.0]);
    };

    (
        INSTANCE_TEXTURE_MAPPING_LOCAL_SPHERICAL,
        [
            ball_material.tileSizeU,
            ball_material.tileSizeV,
            ball_material.tileOffsetU,
            ball_material.tileOffsetV,
        ],
    )
}

// Cylinders use cylindrical mapping when they have material settings and fall back to mesh UVs otherwise.
fn cylinder_instance_texture_mapping_mode_and_tile_transform(cylinder_material: &Option<CylinderMaterialSettings>) -> (u32, [f32; 4]) {
    let Some(cylinder_material) = cylinder_material else {
        return (INSTANCE_TEXTURE_MAPPING_MESH_UV, [0.0, 0.0, 0.0, 0.0]);
    };

    (
        INSTANCE_TEXTURE_MAPPING_LOCAL_CYLINDRICAL,
        [
            cylinder_material.tileSizeU,
            cylinder_material.tileSizeV,
            cylinder_material.tileOffsetU,
            cylinder_material.tileOffsetV,
        ],
    )
}

// Every color a world stores is authored the way a color picker shows it: sRGB, like a CSS hex color
// divided by 255. Lighting adds and multiplies light, which only works on linear values, so each color is
// decoded here before it reaches a shader. Color textures get the same decode from their sRGB GPU format,
// which is what keeps a flat color and a texture of the same hex looking the same.
// The fourth value rides along unchanged: an opacity, an intensity, or padding.
fn decode_srgb_color(color: &Color3, w: f32) -> [f32; 4] {
    let decode = |channel: f32| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    [decode(color.r), decode(color.g), decode(color.b), w]
}

// This keeps the instance buffer construction readable now that every drawable carries mapping data.
fn instance_content(
    model: [[f32; 4]; 4],
    color: [f32; 4],
    texture_mapping_mode: u32,
    texture_tile_transform: [f32; 4],
    local_projection_scale: Vec3,
    instance_flags: u32,
    glow: f32,
) -> InstanceContent {
    InstanceContent {
        model,
        color,
        texture_mapping_mode,
        instance_flags,
        glow,
        _texture_mapping_padding: 0,
        texture_tile_transform,
        local_projection_scale: [local_projection_scale.x, local_projection_scale.y, local_projection_scale.z, 0.0],
    }
}

pub fn collect_mesh_data_ids(render_object_of_id: &HashMap<ObjectId, RenderObject>) -> HashSet<DataId> {
    let mut mesh_data_ids = HashSet::new();

    for render_object in render_object_of_id.values() {
        let RenderObject::Mesh(mesh) = render_object else {
            continue;
        };
        mesh_data_ids.insert(mesh.meshDataId);
    }

    mesh_data_ids
}

pub fn world_lighting_info_of_render_object_of_id(render_object_of_id: &HashMap<ObjectId, RenderObject>) -> WorldLightingInfo {
    let mut world_lighting_info = WorldLightingInfo::default();
    let mut world_lighting_object_id = None;
    for (object_id, render_object) in render_object_of_id.iter() {
        let RenderObject::WorldLighting(next_world_lighting_info) = render_object else {
            continue;
        };

        let should_use_world_lighting = match world_lighting_object_id {
            Some(world_lighting_object_id) => object_id < &world_lighting_object_id,
            None => true,
        };
        if !should_use_world_lighting {
            continue;
        }

        world_lighting_object_id = Some(*object_id);
        world_lighting_info = *next_world_lighting_info;
    }
    world_lighting_info
}

pub fn collect_material_texture_data_ids(render_object_of_id: &HashMap<ObjectId, RenderObject>) -> HashSet<DataId> {
    let mut texture_data_ids: HashSet<DataId> = HashSet::new();

    for render_object in render_object_of_id.values() {
        let material_info = match render_object {
            RenderObject::Ball(ball) => material_info_of_material_pointer(render_object_of_id, material_info_id_of_ball_material(&ball.material)),
            RenderObject::Brick(brick) => material_info_of_material_pointer(render_object_of_id, material_info_id_of_brick_material(&brick.material)),
            RenderObject::Cylinder(cylinder) => {
                material_info_of_material_pointer(render_object_of_id, material_info_id_of_cylinder_material(&cylinder.material))
            }
            RenderObject::Mesh(mesh) => material_info_of_material_pointer(render_object_of_id, material_info_id_of_mesh_material(&mesh.material)),
            RenderObject::RenderLabel(label) => {
                if let RenderLabelContent::Image { textureDataId, .. } = label.content {
                    texture_data_ids.insert(textureDataId);
                }
                None
            }
            _ => None,
        };
        let Some(material_info) = material_info else {
            continue;
        };
        insert_material_texture_data_ids(&mut texture_data_ids, material_info);
    }

    texture_data_ids
}

fn render_skeleton_info_of_skeleton_id(render_object_of_id: &HashMap<ObjectId, RenderObject>, skeleton_id: ObjectId) -> Option<&RenderSkeletonInfo> {
    let Some(render_object) = render_object_of_id.get(&skeleton_id) else {
        return None;
    };
    let RenderObject::Skeleton(render_skeleton) = render_object else {
        return None;
    };
    Some(render_skeleton.renderSkeletonInfoArc.as_ref())
}

fn build_lighting_buffer_content(world_lighting_info: WorldLightingInfo) -> LightingBufferContent {
    let sun_direction = world_lighting_info.sunOrientation.to_unit_direction();
    LightingBufferContent {
        sun_direction: [sun_direction.x, sun_direction.y, sun_direction.z, 0.0],
        sun_color_and_intensity: decode_srgb_color(&world_lighting_info.sunColor, world_lighting_info.sunIntensity),
        ambient_top_color_and_intensity: decode_srgb_color(&world_lighting_info.ambientTopColor, world_lighting_info.ambientIntensity),
        ambient_bottom_color_and_padding: decode_srgb_color(&world_lighting_info.ambientBottomColor, 0.0),
        sky_top_color_and_padding: decode_srgb_color(&world_lighting_info.skyTopColor, 0.0),
        sky_horizon_color_and_padding: decode_srgb_color(&world_lighting_info.skyHorizonColor, 0.0),
        sky_bottom_color_and_padding: decode_srgb_color(&world_lighting_info.skyBottomColor, 0.0),
        ambient_occlusion_settings: [
            world_lighting_info.ambientOcclusionIntensity,
            world_lighting_info.ambientOcclusionRadius,
            world_lighting_info.ambientOcclusionSunlightStrength,
            0.0,
        ],
        sky_reflection_settings: [world_lighting_info.skyReflectionStrength, 0.0, 0.0, 0.0],
        display_settings: [world_lighting_info.vibrance, 0.0, 0.0, 0.0],
    }
}

pub fn build_joint_matrix_buffer_content(
    render_skeleton_info: &RenderSkeletonInfo,
    pose: &SkeletonPose,
    joint_name_of_idx: &[String],
    inverse_bind_matrix_of_joint_name: &HashMap<String, [f32; 16]>,
) -> JointMatrixBufferContent {
    let identity = Mat4::IDENTITY.to_cols_array_2d();
    let mut joint_matrix = JointMatrixBufferContent {
        matrices: [identity; MAX_JOINTS],
    };
    let num_joints = joint_name_of_idx.len().min(MAX_JOINTS);
    let jointIndexOfName = &render_skeleton_info.jointIndexOfName;

    // if any imported inverse bind matrix is missing, fall back to recomputing it from the starting bind pose.
    let inverseBindMatrixOfJoint = if inverse_bind_matrix_of_joint_name.is_empty() {
        computeInverseBindMatrices(&render_skeleton_info.jointOfIndex)
    } else {
        let mut inverseBindMatrixOfJoint = Vec::with_capacity(render_skeleton_info.jointOfIndex.len());
        let mut is_missing_inverse_bind_matrix = false;
        for joint in &render_skeleton_info.jointOfIndex {
            let Some(inverse_bind_matrix) = inverse_bind_matrix_of_joint_name.get(&joint.name) else {
                is_missing_inverse_bind_matrix = true;
                break;
            };
            inverseBindMatrixOfJoint.push(Mat4::from_cols_array(inverse_bind_matrix));
        }

        if is_missing_inverse_bind_matrix {
            computeInverseBindMatrices(&render_skeleton_info.jointOfIndex)
        } else {
            inverseBindMatrixOfJoint
        }
    };

    for mesh_joint_idx in 0..num_joints {
        let joint_name = joint_name_of_idx[mesh_joint_idx].as_str();
        // joint idx
        let Some(joint_idx) = jointIndexOfName.get(joint_name).copied() else {
            continue;
        };
        // posed matrix
        let Some(posed_joint_matrix) = pose.jointMatrixOfJointIndex.get(joint_idx).copied() else {
            continue;
        };
        let Some(inverse_bind_matrix) = inverseBindMatrixOfJoint.get(joint_idx).copied() else {
            continue;
        };
        let skin_matrix = posed_joint_matrix * inverse_bind_matrix;
        joint_matrix.matrices[mesh_joint_idx] = skin_matrix.to_cols_array_2d();
    }
    joint_matrix
}
// Keep these values synced with scene.wgsl INSTANCE_FLAG_* constants.
const INSTANCE_FLAG_BILLBOARD: u32 = 1;
const INSTANCE_FLAG_CONSTANT_SIZE: u32 = 2;

// CPU geometry queries must match scene.wgsl build_model_matrix, including off-axis constant-size labels.
pub fn instance_model_for_view(instance: &InstanceContent, view_proj: Mat4) -> Mat4 {
    let model = Mat4::from_cols_array_2d(&instance.model);
    let is_billboarding = instance.instance_flags & INSTANCE_FLAG_BILLBOARD != 0;
    let is_constant_size = instance.instance_flags & INSTANCE_FLAG_CONSTANT_SIZE != 0;
    if !is_billboarding && !is_constant_size {
        return model;
    }
    let translation = model.w_axis.truncate();
    let clip_w_row = view_proj.row(3);
    let multiplier = if is_constant_size {
        clip_w_row.dot(translation.extend(1.0)).abs().max(1e-4) / CONSTANT_SIZE_REFERENCE_DISTANCE
    } else {
        1.0
    };
    if !is_billboarding {
        return Mat4::from_cols(model.x_axis * multiplier, model.y_axis * multiplier, model.z_axis * multiplier, model.w_axis);
    }
    let scale = Vec3::new(
        model.x_axis.truncate().length(),
        model.y_axis.truncate().length(),
        model.z_axis.truncate().length(),
    ) * multiplier;
    let forward = -clip_w_row.truncate().normalize();
    let right = Vec3::Y.cross(forward).normalize();
    let up = forward.cross(right);
    Mat4::from_cols(
        (right * scale.x).extend(0.0),
        (up * scale.y).extend(0.0),
        (forward * scale.z).extend(0.0),
        translation.extend(1.0),
    )
}

// Labels sample the atlas straight through mesh UVs, so the tile transform is inert; this names that.
const LABEL_TEXTURE_TILE_TRANSFORM: [f32; 4] = [1.0, 1.0, 0.0, 0.0];

// Asset loading stays with the host. Missing meshes are omitted until the host supplies their decoded data.
pub fn build_render_scene(
    render_object_of_id: &HashMap<ObjectId, RenderObject>,
    mesh_data_of_id: &HashMap<DataId, Arc<MeshData>>,
    skeleton_pose_of_id: Option<&HashMap<ObjectId, SkeletonPose>>,
) -> RenderScene {
    let world_lighting_info = world_lighting_info_of_render_object_of_id(render_object_of_id);
    let mut scene = RenderScene {
        selected_object_ids: HashSet::new(),
        instances: Vec::new(),
        labels: Vec::new(),
        joint_matrices: Vec::new(),
        lighting: build_lighting_buffer_content(world_lighting_info),
        sun_shadow_softness: world_lighting_info.sunShadowSoftness,
        skybox_texture_data_id: world_lighting_info.skyboxTextureDataId,
        skybox_texture_opacity: world_lighting_info.skyboxTextureOpacity,
        should_draw_sky: true,
    };
    let default_material_texture_key = MaterialTextureDataIds::default();
    let empty_pose_map = HashMap::new();
    let skeleton_pose_of_id = skeleton_pose_of_id.unwrap_or(&empty_pose_map);
    let mut needed_skinned_mesh_joint_matrix_buffer_keys = HashSet::new();
    for (object_id, render_object) in render_object_of_id {
        match render_object {
            RenderObject::Ball(ball) => {
                let visualSettings = &ball.visualSettings;
                let scale = Vec3::splat(ball.primitive.radius.abs());
                let (texture_mapping_mode, texture_tile_transform) = ball_instance_texture_mapping_mode_and_tile_transform(&ball.material);
                let Some(transform) = computeRenderObjectTransform(render_object_of_id, skeleton_pose_of_id, *object_id) else {
                    continue;
                };
                let transform = transform * Mat4::from_scale(scale);
                let color = &visualSettings.color;
                let alpha = visualSettings.opacity.clamp(0.0, 1.0);
                let is_always_on_top = visualSettings.isAlwaysOnTop;
                if alpha == 0.0 {
                    continue;
                }
                let material_texture_data_ids = material_texture_key_of_material_pointer(
                    render_object_of_id,
                    material_info_id_of_ball_material(&ball.material),
                    default_material_texture_key,
                );
                let model = transform.to_cols_array_2d();
                let instance = instance_content(
                    model,
                    decode_srgb_color(color, alpha),
                    texture_mapping_mode,
                    texture_tile_transform,
                    scale,
                    0,
                    visualSettings.glow.clamp(0.0, 1.0),
                );
                scene.instances.push(RenderInstance {
                    object_id: *object_id,
                    geometry: RenderGeometry::Ball,
                    content: instance,
                    material: material_texture_data_ids,
                    is_always_on_top,
                });
            }
            RenderObject::Cylinder(cylinder) => {
                let visualSettings = &cylinder.visualSettings;
                let scale = Vec3::new(
                    cylinder.primitive.radius.abs(),
                    cylinder.primitive.radius.abs(),
                    cylinder.primitive.length.abs(),
                );
                let (texture_mapping_mode, texture_tile_transform) = cylinder_instance_texture_mapping_mode_and_tile_transform(&cylinder.material);
                let Some(transform) = computeRenderObjectTransform(render_object_of_id, skeleton_pose_of_id, *object_id) else {
                    continue;
                };
                let transform = transform * Mat4::from_scale(scale);
                let color = &visualSettings.color;
                let alpha = visualSettings.opacity.clamp(0.0, 1.0);
                let is_always_on_top = visualSettings.isAlwaysOnTop;
                if alpha == 0.0 {
                    continue;
                }
                let material_texture_data_ids = material_texture_key_of_material_pointer(
                    render_object_of_id,
                    material_info_id_of_cylinder_material(&cylinder.material),
                    default_material_texture_key,
                );
                let model = transform.to_cols_array_2d();
                let instance = instance_content(
                    model,
                    decode_srgb_color(color, alpha),
                    texture_mapping_mode,
                    texture_tile_transform,
                    scale,
                    0,
                    visualSettings.glow.clamp(0.0, 1.0),
                );
                scene.instances.push(RenderInstance {
                    object_id: *object_id,
                    geometry: RenderGeometry::Cylinder,
                    content: instance,
                    material: material_texture_data_ids,
                    is_always_on_top,
                });
            }
            RenderObject::Brick(brick) => {
                let visualSettings = &brick.visualSettings;
                let size = brick.primitive.size.abs();
                let (texture_mapping_mode, texture_tile_transform) = brick_instance_texture_mapping_mode_and_tile_transform(&brick.material);
                let Some(transform) = computeRenderObjectTransform(render_object_of_id, skeleton_pose_of_id, *object_id) else {
                    continue;
                };
                let transform = transform * Mat4::from_scale(size);
                let color = &visualSettings.color;
                let alpha = visualSettings.opacity.clamp(0.0, 1.0);
                let is_always_on_top = visualSettings.isAlwaysOnTop;
                if alpha == 0.0 {
                    continue;
                }
                let material_texture_data_ids = material_texture_key_of_material_pointer(
                    render_object_of_id,
                    material_info_id_of_brick_material(&brick.material),
                    default_material_texture_key,
                );
                let model = transform.to_cols_array_2d();
                let instance = instance_content(
                    model,
                    decode_srgb_color(color, alpha),
                    texture_mapping_mode,
                    texture_tile_transform,
                    size,
                    0,
                    visualSettings.glow.clamp(0.0, 1.0),
                );
                scene.instances.push(RenderInstance {
                    object_id: *object_id,
                    geometry: RenderGeometry::Brick,
                    content: instance,
                    material: material_texture_data_ids,
                    is_always_on_top,
                });
            }
            RenderObject::Mesh(mesh) => {
                let visualSettings = &mesh.visualSettings;
                let scale = mesh.scale; // dont add a max because it is often very small
                let Some(transform) = computeRenderObjectTransform(render_object_of_id, skeleton_pose_of_id, *object_id) else {
                    continue;
                };
                let transform = transform * Mat4::from_scale(scale);
                let color = &visualSettings.color;
                let alpha = visualSettings.opacity.clamp(0.0, 1.0);
                let model = transform.to_cols_array_2d();
                let (texture_mapping_mode, texture_tile_transform) = (INSTANCE_TEXTURE_MAPPING_MESH_UV, [0.0, 0.0, 0.0, 0.0]);
                let is_always_on_top = visualSettings.isAlwaysOnTop;
                if alpha == 0.0 {
                    continue;
                }

                // if the mesh was not loaded yet, skip it
                let data_id = mesh.meshDataId;
                let material_texture_data_ids = material_texture_key_of_material_pointer(
                    render_object_of_id,
                    material_info_id_of_mesh_material(&mesh.material),
                    default_material_texture_key,
                );

                let Some(mesh_data) = mesh_data_of_id.get(&data_id) else {
                    continue;
                };
                let is_skinned = match mesh_data.as_ref() {
                    MeshData::SkinnedMeshData { .. } => true,
                    MeshData::StaticMeshData { .. } => false,
                };

                let skeleton_id = mesh.skin_skeleton_id;

                if is_skinned {
                    let Some(skeleton_id) = skeleton_id else {
                        continue;
                    };
                    needed_skinned_mesh_joint_matrix_buffer_keys.insert((data_id, skeleton_id));
                }

                let instance = instance_content(
                    model,
                    decode_srgb_color(color, alpha),
                    texture_mapping_mode,
                    texture_tile_transform,
                    scale,
                    0,
                    visualSettings.glow.clamp(0.0, 1.0),
                );
                scene.instances.push(RenderInstance {
                    object_id: *object_id,
                    geometry: RenderGeometry::Mesh { data_id, skeleton_id },
                    content: instance,
                    material: material_texture_data_ids,
                    is_always_on_top,
                });
            }
            RenderObject::RenderLabel(label) => {
                let visualSettings = &label.visualSettings;
                let alpha = visualSettings.opacity.clamp(0.0, 1.0);
                if alpha == 0.0 {
                    continue;
                }
                // Billboarded: world offset, and the shader rebuilds the basis so rotation here is moot.
                // Not billboarded: stuck to the object, riding its rotation with the offset in its frame.
                let Some(followed_transform) = computeRenderObjectTransform(render_object_of_id, skeleton_pose_of_id, *object_id) else {
                    continue;
                };
                let (_, followed_rotation, followed_position) = followed_transform.to_scale_rotation_translation();
                let is_billboarding = label.billboardMode != BillboardMode::None;
                let (position, rotation) = if is_billboarding {
                    (followed_position + label.offsetFromObject, Quat::IDENTITY)
                } else {
                    (
                        followed_position + followed_rotation * label.offsetFromObject,
                        followed_rotation * label.rotationFromObject,
                    )
                };
                // Everything the vertex shader needs to know about how an object orients and sizes itself.
                // Only labels orient themselves against the camera, so nothing else ever sets these bits.
                let mut instance_flags = if is_billboarding { INSTANCE_FLAG_BILLBOARD } else { 0 };
                if label.isConstantSize {
                    instance_flags |= INSTANCE_FLAG_CONSTANT_SIZE;
                }
                let content = match &label.content {
                    RenderLabelContent::Text {
                        scale,
                        backgroundColor,
                        backgroundOpacity,
                        geometry,
                        atlas,
                    } => {
                        let scale = if scale.is_finite() && *scale > 0.0 { *scale } else { f32::EPSILON };
                        let model = (Mat4::from_rotation_translation(rotation, position) * Mat4::from_scale(Vec3::splat(scale))).to_cols_array_2d();
                        let color = &visualSettings.color;
                        let background_color = backgroundColor.unwrap_or(Color3::BLACK);
                        let background_alpha = (backgroundOpacity.clamp(0.0, 1.0)) * alpha;
                        let draws_background = backgroundColor.is_some() && background_alpha > 0.0 && geometry.background_index_count > 0;

                        let instance_contents = [
                            instance_content(
                                model,
                                decode_srgb_color(&background_color, background_alpha),
                                INSTANCE_TEXTURE_MAPPING_MESH_UV,
                                LABEL_TEXTURE_TILE_TRANSFORM,
                                Vec3::ONE,
                                instance_flags,
                                0.0,
                            ),
                            instance_content(
                                model,
                                decode_srgb_color(color, alpha),
                                INSTANCE_TEXTURE_MAPPING_MESH_UV,
                                LABEL_TEXTURE_TILE_TRANSFORM,
                                Vec3::ONE,
                                instance_flags,
                                visualSettings.glow.clamp(0.0, 1.0),
                            ),
                        ];
                        LabelContent::Text {
                            geometry: geometry.clone(),
                            atlas: atlas.clone(),
                            instances: instance_contents,
                            draws_background,
                        }
                    }
                    RenderLabelContent::Image { textureDataId, width, height } => {
                        if !(width.is_finite() && *width > 0.0 && height.is_finite() && *height > 0.0) {
                            continue;
                        }
                        let model =
                            (Mat4::from_rotation_translation(rotation, position) * Mat4::from_scale(Vec3::new(*width, *height, 1.0))).to_cols_array_2d();
                        LabelContent::Image {
                            texture_data_id: *textureDataId,
                            instance: instance_content(
                                model,
                                [1.0, 1.0, 1.0, alpha],
                                INSTANCE_TEXTURE_MAPPING_MESH_UV,
                                LABEL_TEXTURE_TILE_TRANSFORM,
                                Vec3::ONE,
                                instance_flags,
                                0.0,
                            ),
                        }
                    }
                };
                scene.labels.push(RenderLabel {
                    object_id: *object_id,
                    content,
                    is_always_on_top: visualSettings.isAlwaysOnTop,
                });
            }
            RenderObject::Material(_) | RenderObject::Skeleton(_) | RenderObject::RigidGroup(_) | RenderObject::WorldLighting(_) => {}
        }
    }
    for (mesh_data_id, skeleton_id) in needed_skinned_mesh_joint_matrix_buffer_keys {
        let Some(pose) = skeleton_pose_of_id.get(&skeleton_id) else {
            continue;
        };
        let Some(mesh_data) = mesh_data_of_id.get(&mesh_data_id) else {
            continue;
        };
        let MeshData::SkinnedMeshData {
            jointNameOfIdx,
            inverseBindMatrixOfJointName,
            ..
        } = mesh_data.as_ref()
        else {
            continue;
        };
        let Some(render_skeleton_info) = render_skeleton_info_of_skeleton_id(render_object_of_id, skeleton_id) else {
            continue;
        };

        // update existing joint matrix buffer, or create a new one
        let joint_matrix = build_joint_matrix_buffer_content(render_skeleton_info, pose, jointNameOfIdx.as_slice(), inverseBindMatrixOfJointName);
        scene.joint_matrices.push((mesh_data_id, skeleton_id, joint_matrix));
    }
    scene
}
