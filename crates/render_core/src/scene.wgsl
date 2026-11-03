const MAX_JOINTS: u32 = 128u;

// -------------------------- PBR + TBN Overview --------------------------
// This shader uses a metallic-roughness PBR workflow for direct lights.
// We sample basecolor/metallic/roughness/normal textures in UV space.
// The BRDF uses GGX distribution, Smith geometry, and Schlick Fresnel.
//
// For normal mapping we use a TBN frame:
// N = mesh normal (transformed to world using inverse-transpose for non-uniform scale).
// T = mesh tangent.xyz (transformed to world).
// B = normalize(cross(N, T)) * tangent_sign, where tangent_sign = tangent.w.
// The sampled normal map value starts in tangent space and is transformed to world space by TBN.
//
// Mesh data feeding this shader comes from glTF conversion:
// - Read from glTF if present: positions, normals, tangents, UV0, indices, skin joints/weights, and material texture slots.
// - Fallback computed when missing: normals (smooth from triangles), tangents (from positions + UVs + normals), plus default UVs/joints/weights/indices.
// - Static meshes are transformed during import; normals use inverse-transpose handling there too.
// - Tangent.w stores handedness sign so this shader can reconstruct bitangent consistently.
//
// Primitives can also sample textures from object-local projection instead of mesh UVs.
// Those paths use the per-instance mapping fields below.


// -------------------------- Structs from Rust fed to this pipeline (names + fields must match) --------------------------
// Keep these values synced with renderer.rs MATERIAL_ALPHA_MODE_* constants.
const MATERIAL_ALPHA_MODE_OPAQUE: u32 = 0u;
const MATERIAL_ALPHA_MODE_MASK: u32 = 1u;
const MATERIAL_ALPHA_MODE_BLEND: u32 = 2u;
// Keep these values synced with renderer.rs INSTANCE_FLAG_* constants.
const INSTANCE_FLAG_BILLBOARD: u32 = 1u;
const INSTANCE_FLAG_CONSTANT_SIZE: u32 = 2u;
// Keep this synced with label.rs CONSTANT_SIZE_REFERENCE_DISTANCE.
const CONSTANT_SIZE_REFERENCE_DISTANCE: f32 = 10.0;
const LABEL_MIN_ALPHA: f32 = 0.002;
// Keep this synced with label_text_mesh.rs DISTANCE_FIELD_RADIUS.
const LABEL_DISTANCE_FIELD_RADIUS: f32 = 8.0;
// Keep this synced with renderer.rs OPAQUE_MULTISAMPLE_COUNT.
const OPAQUE_MULTISAMPLE_COUNT: u32 = 4u;
// Keep these values synced with renderer.rs INSTANCE_TEXTURE_MAPPING_* constants.
const INSTANCE_TEXTURE_MAPPING_MESH_UV: u32 = 0u;
const INSTANCE_TEXTURE_MAPPING_LOCAL_BOX: u32 = 1u;
const INSTANCE_TEXTURE_MAPPING_LOCAL_SPHERICAL: u32 = 2u;
const INSTANCE_TEXTURE_MAPPING_LOCAL_CYLINDRICAL: u32 = 3u;


struct CameraMatrixBufferContent {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    // The camera the player looks through, which is not always the camera this pass projects with.
    // The shadow pass projects with the sun's matrix, but a billboard still has to face the player
    // and a constant-size instance still has to be sized against the player's view, or its shadow
    // would not match the thing casting it.
    camera_position: vec4<f32>,
    // Row 3 of that same player camera's view-projection. Dotted with a world position it gives that
    // camera's clip w, which is the view space depth perspective divides by. It is only ever
    // different from view_proj's own row 3 during the shadow pass.
    view_clip_w_row: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraMatrixBufferContent;

@group(1) @binding(0)
var basecolor_texture: texture_2d<f32>;
@group(1) @binding(1)
var metallic_texture: texture_2d<f32>;
@group(1) @binding(2)
var roughness_texture: texture_2d<f32>;
@group(1) @binding(3)
var normal_texture: texture_2d<f32>;
@group(1) @binding(4)
var material_sampler: sampler;

struct MaterialAlphaModeBufferContent {
    alpha_mode: u32,
    alpha_cutoff: f32,
    _padding0: u32,
    _padding1: u32,
};

@group(1) @binding(5)
var<uniform> material_alpha_mode: MaterialAlphaModeBufferContent;

struct JointMatrixBufferContent {
    matrices: array<mat4x4<f32>, 128>,
};

@group(2) @binding(0)
var<uniform> joints: JointMatrixBufferContent;

struct LightingBufferContent {
    sun_direction: vec4<f32>,
    sun_color_and_intensity: vec4<f32>,

    ambient_top_color_and_intensity: vec4<f32>,
    ambient_bottom_color_and_padding: vec4<f32>,

    sky_top_color_and_padding: vec4<f32>,
    sky_horizon_color_and_padding: vec4<f32>,
    sky_bottom_color_and_padding: vec4<f32>,
 
    // x: intensity, y: radius, z: how much it also darkens sunlight.
    ambient_occlusion_settings: vec4<f32>,
    // x: how strongly shiny surfaces reflect the sky.
    sky_reflection_settings: vec4<f32>,
    // x: vibrance, how much further from gray dull colors are pushed.
    display_settings: vec4<f32>,
};

@group(3) @binding(0)
var<uniform> lighting: LightingBufferContent;

struct ShadowBufferContent {
    light_view_proj: array<mat4x4<f32>, 5>,
    // x: half the world width the cascade covers, y: the world width of one of its texels.
    cascade_settings: array<vec4<f32>, 5>,
    // x: receiver depth bias and y: receiver normal bias, in world units. z: how far every shadow edge blurs, in world
    // units. w: world distance per unit of shadow depth.
    settings: vec4<f32>,
};

@group(3) @binding(1)
var<uniform> shadow: ShadowBufferContent;
// One layer per cascade, from the close one out.
@group(3) @binding(2)
var shadow_depth_textures: texture_depth_2d_array;
@group(3) @binding(3)
var shadow_compare_sampler: sampler_comparison;
// Drawn by ambient_occlusion.wgsl from the opaque depth, before the opaque pass.
@group(3) @binding(4)
var ambient_occlusion_texture: texture_2d<f32>;

struct SkyboxBufferContent {
    // x: how opaque the skybox image is over the gradient. y: how many mip levels the reflected skybox image has, since
    // WebGL2 cannot ask the texture.
    settings: vec4<f32>,
};

// The world's skybox image, reflected by shiny surfaces. A world without one gets a transparent texel.
@group(3) @binding(5)
var skybox_texture: texture_2d<f32>;
@group(3) @binding(6)
var skybox_sampler: sampler;
@group(3) @binding(7)
var<uniform> skybox: SkyboxBufferContent;

// The scene's depth, for the glow pass, which draws at half the picture's size and so has to test it by hand. Only the glow
// entry points read it, from a group of their own that takes the lighting's place. Keep the binding synced with
// GLOW_SCENE_DEPTH_BINDING in renderer.rs.
@group(3) @binding(8)
var scene_depth_texture: texture_2d<f32>;

struct VertexContent {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(9) uv: vec2<f32>,
    @location(10) tangent: vec4<f32>,
};

struct SkinnedVertexContent {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(9) uv: vec2<f32>,
    @location(10) tangent: vec4<f32>,
    @location(7) joint_indices: vec4<u32>,
    @location(8) joint_weights: vec4<f32>,
};

struct InstanceContent {
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(11) texture_mapping_mode: u32,
    @location(12) texture_tile_transform: vec4<f32>,
    @location(13) local_projection_scale: vec4<f32>,
    @location(14) instance_flags: u32,
    @location(15) glow: f32,
};

// -------------------------- Items only used internally in this file --------------------------

struct VertexShaderOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) world_tangent: vec4<f32>,
    @location(6) local_position_for_projection: vec3<f32>,
    @location(7) local_normal_for_projection: vec3<f32>,
    @location(5) @interpolate(flat) local_projection_scale: vec3<f32>,
    @location(8) @interpolate(flat) texture_mapping_mode: u32,
    @location(9) @interpolate(flat) texture_tile_transform: vec4<f32>,
    @location(10) sphere_direction_for_projection: vec3<f32>,
    // These three come out of the instance's own matrix, so every vertex of an instance writes the
    // same value and there is nothing for the rasterizer to interpolate between. Marking them flat
    // says so: it hands the fragment shader that one value instead of blending three copies of it.
    @location(11) @interpolate(flat) world_normal_axis_x: vec3<f32>,
    @location(12) @interpolate(flat) world_normal_axis_y: vec3<f32>,
    @location(13) @interpolate(flat) world_normal_axis_z: vec3<f32>,
    @location(14) @interpolate(flat) glow: f32,
};

const PI: f32 = 3.14159265359;
const EPSILON: f32 = 1e-4;
const DIELECTRIC_F0: vec3<f32> = vec3<f32>(0.04, 0.04, 0.04);
// Keep this synced with render_scene.rs SHADOW_CASCADE_COUNT.
const SHADOW_CASCADE_COUNT: u32 = 5u;
// TODO: AI-PICKED-VALUE: The band a cascade fades into the next one across, at its edge, in its own NDC. Narrow keeps
// the sharper cascade active until it is nearly out of coverage.
const SHADOW_CASCADE_BLEND_BAND_NDC: f32 = 0.02;
// TODO: AI-PICKED-VALUE: Every shadow edge blurs across at least this many shadow texels, so the texels never show as
// steps.
const SHADOW_MIN_BLUR_TEXELS: f32 = 2.0;
// TODO: AI-PICKED-VALUE: The blur hands off to a coarser cascade over this last fraction of the way to it.
const SHADOW_BLUR_CASCADE_HANDOFF: f32 = 0.25;
// TODO: AI-PICKED-VALUE: 32 compares blur the edge.
const SHADOW_BLUR_TAP_COUNT: i32 = 32;
// The blur's taps fade toward the rim of a disk this much wider than the blur radius, which blurs an edge about as much as
// an even disk of the blur radius would.
const SHADOW_BLUR_DISK_SCALE: f32 = 1.5;
// Turning by the golden angle between points spreads them evenly over a disk (a Vogel spiral).
const GOLDEN_ANGLE_ROTATION: mat2x2<f32> = mat2x2<f32>(-0.73736887, 0.67549029, -0.67549029, -0.73736887);
// TODO: AI-PICKED-VALUE: How steeply a surface may rise away from the sun, per unit across, when blur taps compare against
// it. Derivatives across triangle edges can report any slope, and this keeps those from throwing the taps off.
const SHADOW_MAX_RECEIVER_SLOPE: f32 = 4.0;

struct ProjectedNormalAxes {
    world_normal_axis_x: vec3<f32>,
    world_normal_axis_y: vec3<f32>,
    world_normal_axis_z: vec3<f32>,
};

struct ProjectionFrame {
    uv: vec2<f32>,
    local_tangent: vec3<f32>,
    local_bitangent: vec3<f32>,
    local_normal: vec3<f32>,
    // How far u travels in one full turn around an atan2 based projection, or 0 for a projection with
    // no seam. atan2 jumps by exactly this much where it wraps, and that jump lands in the implicit
    // derivative, which would pick the smallest mip for the one pixel column on the seam. See
    // wrapped_uv_gradient.
    u_wrap_period: f32,
};

// Billboarding lives here rather than on the CPU so every vertex entry point gets it for free and
// nothing needs rebuilding when the camera moves.
fn build_model_matrix(i: InstanceContent) -> mat4x4<f32> {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    let is_billboarding = (i.instance_flags & INSTANCE_FLAG_BILLBOARD) != 0u;
    let is_constant_size = (i.instance_flags & INSTANCE_FLAG_CONSTANT_SIZE) != 0u;
    if (!is_billboarding && !is_constant_size) {
        return model;
    }

    let translation = model[3].xyz;
    var scale_multiplier = 1.0;
    if (is_constant_size) {
        // Perspective divides by view space depth, not distance to the camera; those differ off to the
        // side. Clip space w is exactly that depth. This reads the player camera's w row rather than
        // view_proj so the shadow pass sizes an instance the same way the pass that draws it does.
        let view_space_depth = max(abs(dot(camera.view_clip_w_row, vec4<f32>(translation, 1.0))), EPSILON);
        scale_multiplier = view_space_depth / CONSTANT_SIZE_REFERENCE_DISTANCE;
    }

    if (!is_billboarding) {
        return mat4x4<f32>(
            vec4<f32>(model[0].xyz * scale_multiplier, 0.0),
            vec4<f32>(model[1].xyz * scale_multiplier, 0.0),
            vec4<f32>(model[2].xyz * scale_multiplier, 0.0),
            vec4<f32>(translation, 1.0),
        );
    }

    let scale = vec3<f32>(length(model[0].xyz), length(model[1].xyz), length(model[2].xyz)) * scale_multiplier;
    // Looking straight down the up axis leaves the cross product undefined.
    // The camera's pitch clamp keeps world Y usable as up; its view direction is already in this buffer.
    let forward = -normalize(camera.view_clip_w_row.xyz);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward));
    let up = cross(forward, right);
    return mat4x4<f32>(
        vec4<f32>(right * scale.x, 0.0),
        vec4<f32>(up * scale.y, 0.0),
        vec4<f32>(forward * scale.z, 0.0),
        vec4<f32>(translation, 1.0),
    );
}

fn build_tangent_matrix(model: mat4x4<f32>) -> mat3x3<f32> {
    return mat3x3<f32>(
        model[0].xyz,
        model[1].xyz,
        model[2].xyz,
    );
}

fn build_inverse_transpose_matrix(matrix: mat3x3<f32>) -> mat3x3<f32> {
    let column0 = matrix[0];
    let column1 = matrix[1];
    let column2 = matrix[2];
    let cofactor0 = cross(column1, column2);
    let cofactor1 = cross(column2, column0);
    let cofactor2 = cross(column0, column1);
    let determinant_value = dot(column0, cofactor0);
    if (abs(determinant_value) <= EPSILON) {
        return matrix;
    }
    let inverse_determinant = 1.0 / determinant_value;
    return mat3x3<f32>(
        cofactor0 * inverse_determinant,
        cofactor1 * inverse_determinant,
        cofactor2 * inverse_determinant,
    );
}

fn build_normal_matrix(model: mat4x4<f32>) -> mat3x3<f32> {
    return build_inverse_transpose_matrix(build_tangent_matrix(model));
}

fn safe_nonzero(value: f32) -> f32 {
    if (abs(value) <= EPSILON) {
        return EPSILON;
    }
    return value;
}

fn normalize_or_fallback(value: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    if (length(value) <= EPSILON) {
        return fallback;
    }
    return normalize(value);
}

fn vector_or_fallback(value: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    if (length(value) <= EPSILON) {
        return fallback;
    }
    return value;
}

fn build_local_projection_position(position: vec3<f32>, local_projection_scale: vec3<f32>) -> vec3<f32> {
    return position * local_projection_scale;
}

fn build_local_projection_normal(normal: vec3<f32>, local_projection_scale: vec3<f32>) -> vec3<f32> {
    let inverse_scale = vec3<f32>(
        1.0 / safe_nonzero(local_projection_scale.x),
        1.0 / safe_nonzero(local_projection_scale.y),
        1.0 / safe_nonzero(local_projection_scale.z),
    );
    return normalize(normal * inverse_scale);
}

fn build_sphere_direction_for_projection(position: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    return normalize_or_fallback(position, normal);
}

// The projections already work at the object's true size, so their normals only need its rotation. Keeping the
// normal matrix's scale here shrank a long brick's normal along its length until a normal map's faint noise swung
// the end face's lighting in blotches.
fn build_projected_normal_axes(model_normal_matrix: mat3x3<f32>) -> ProjectedNormalAxes {
    return ProjectedNormalAxes(
        normalize_or_fallback(model_normal_matrix * vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0)),
        normalize_or_fallback(model_normal_matrix * vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0)),
        normalize_or_fallback(model_normal_matrix * vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 1.0)),
    );
}

fn build_vertex_shader_output(
    position: vec4<f32>,
    color: vec4<f32>,
    world_normal: vec3<f32>,
    uv: vec2<f32>,
    world_position: vec3<f32>,
    world_tangent: vec3<f32>,
    tangent_sign: f32,
    local_position_for_projection: vec3<f32>,
    local_normal_for_projection: vec3<f32>,
    local_projection_scale: vec3<f32>,
    sphere_direction_for_projection: vec3<f32>,
    texture_mapping_mode: u32,
    texture_tile_transform: vec4<f32>,
    projected_normal_axes: ProjectedNormalAxes,
    glow: f32,
) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let normalized_normal = normalize(world_normal);
    var normalized_tangent = world_tangent - normalized_normal * dot(normalized_normal, world_tangent);
    if (length(normalized_tangent) <= EPSILON) {
        if (abs(normalized_normal.z) < 0.999) {
            normalized_tangent = normalize(cross(vec3<f32>(0.0, 0.0, 1.0), normalized_normal));
        } else {
            normalized_tangent = normalize(cross(vec3<f32>(1.0, 0.0, 0.0), normalized_normal));
        }
    } else {
        normalized_tangent = normalize(normalized_tangent);
    }
    out.position = position;
    out.color = color;
    out.world_normal = normalized_normal;
    out.uv = uv;
    out.world_position = world_position;
    out.world_tangent = vec4<f32>(normalized_tangent, select(1.0, -1.0, tangent_sign < 0.0));
    out.local_position_for_projection = local_position_for_projection;
    out.local_normal_for_projection = local_normal_for_projection;
    out.local_projection_scale = local_projection_scale;
    out.sphere_direction_for_projection = sphere_direction_for_projection;
    out.texture_mapping_mode = texture_mapping_mode;
    out.texture_tile_transform = texture_tile_transform;
    out.world_normal_axis_x = projected_normal_axes.world_normal_axis_x;
    out.world_normal_axis_y = projected_normal_axes.world_normal_axis_y;
    out.world_normal_axis_z = projected_normal_axes.world_normal_axis_z;
    out.glow = glow;
    return out;
}

struct MaterialSample {
    basecolor: vec3<f32>,
    alpha: f32,
    metallic: f32,
    roughness: f32,
};

struct DirectionalLight {
    direction_to_light: vec3<f32>,
    color: vec3<f32>,
};

struct LitSurface {
    color: vec3<f32>,
    alpha: f32,
};

fn safe_texture_tile_size(tile_size: f32) -> f32 {
    if (abs(tile_size) <= EPSILON) {
        return EPSILON;
    }
    return tile_size;
}

fn apply_texture_tile_offset(uv: vec2<f32>, texture_tile_transform: vec4<f32>) -> vec2<f32> {
    return uv + texture_tile_transform.zw;
}

// -------------------------- Local projection UV convention --------------------------
// All three projections agree on one frame, so a texture and a normal map behave the same whichever
// shape they land on:
//   U runs left to right across the surface as seen from outside it.
//   V runs top to bottom, matching image rows: v = 0 is the top of the picture.
//   Both start at the top left corner of a flat face, and at the seam on a ball or a cylinder's side.
//   local_tangent points along +U, local_bitangent along +V, local_normal outward.
// V pointing down makes each frame left-handed (local_tangent x local_bitangent = -local_normal),
// which is what a v-down image layout on a front facing surface always gives you. It has to be the
// same sign everywhere or a normal map would push outward on one shape and inward on another.
// Tile sizes are world lengths across the surface on every shape, so tileSizeU = 2 means one tile
// every two blocks on a brick, a ball, and a cylinder alike.

fn local_box_projection(
    local_position: vec3<f32>,
    local_normal: vec3<f32>,
    local_projection_scale: vec3<f32>,
    tile_size_u: f32,
    tile_size_v: f32,
) -> ProjectionFrame {
    let abs_normal = abs(local_normal);
    let safe_tile_size_u = safe_texture_tile_size(tile_size_u);
    let safe_tile_size_v = safe_texture_tile_size(tile_size_v);
    let half_size = local_projection_scale * 0.5;
    let from_min = local_position + half_size;
    let from_max = half_size - local_position;

    // Every box face is a flat projection of a linear position, so none of them has a wrap seam.
    if (abs_normal.x >= abs_normal.y && abs_normal.x >= abs_normal.z) {
        if (local_normal.x >= 0.0) {
            return ProjectionFrame(
                vec2<f32>(from_max.z / safe_tile_size_u, from_max.y / safe_tile_size_v),
                vec3<f32>(0.0, 0.0, -1.0),
                vec3<f32>(0.0, -1.0, 0.0),
                vec3<f32>(1.0, 0.0, 0.0),
                0.0,
            );
        }
        return ProjectionFrame(
            vec2<f32>(from_min.z / safe_tile_size_u, from_max.y / safe_tile_size_v),
            vec3<f32>(0.0, 0.0, 1.0),
            vec3<f32>(0.0, -1.0, 0.0),
            vec3<f32>(-1.0, 0.0, 0.0),
            0.0,
        );
    }

    if (abs_normal.y >= abs_normal.z) {
        if (local_normal.y >= 0.0) {
            return ProjectionFrame(
                vec2<f32>(from_min.x / safe_tile_size_u, from_min.z / safe_tile_size_v),
                vec3<f32>(1.0, 0.0, 0.0),
                vec3<f32>(0.0, 0.0, 1.0),
                vec3<f32>(0.0, 1.0, 0.0),
                0.0,
            );
        }
        return ProjectionFrame(
            vec2<f32>(from_min.x / safe_tile_size_u, from_max.z / safe_tile_size_v),
            vec3<f32>(1.0, 0.0, 0.0),
            vec3<f32>(0.0, 0.0, -1.0),
            vec3<f32>(0.0, -1.0, 0.0),
            0.0,
        );
    }

    if (local_normal.z >= 0.0) {
        return ProjectionFrame(
            vec2<f32>(from_min.x / safe_tile_size_u, from_max.y / safe_tile_size_v),
            vec3<f32>(1.0, 0.0, 0.0),
            vec3<f32>(0.0, -1.0, 0.0),
            vec3<f32>(0.0, 0.0, 1.0),
            0.0,
        );
    }
    return ProjectionFrame(
        vec2<f32>(from_max.x / safe_tile_size_u, from_max.y / safe_tile_size_v),
        vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, -1.0),
        0.0,
    );
}

// This maps the unit sphere mesh to one continuous spherical UV projection. The radius comes from the
// scaled local position, which for the unit sphere mesh is exactly the ball's world radius, so u and v
// are true distances across the surface rather than a 0..1 wrap.
fn local_spherical_projection(
    sphere_direction: vec3<f32>,
    sphere_radius: f32,
    tile_size_u: f32,
    tile_size_v: f32,
) -> ProjectionFrame {
    let normalized_direction = normalize_or_fallback(sphere_direction, vec3<f32>(0.0, 0.0, 1.0));
    let safe_tile_size_u = safe_texture_tile_size(tile_size_u);
    let safe_tile_size_v = safe_texture_tile_size(tile_size_v);
    let equator_length = 2.0 * PI * max(sphere_radius, EPSILON);
    let meridian_length = PI * max(sphere_radius, EPSILON);
    let u = (0.5 + atan2(normalized_direction.x, normalized_direction.z) / (2.0 * PI)) * equator_length;
    // Straight down from the north pole, so the top of the image sits at the top of the ball.
    let v = (0.5 - asin(clamp(normalized_direction.y, -1.0, 1.0)) / PI) * meridian_length;
    let local_tangent = normalize_or_fallback(
        vec3<f32>(normalized_direction.z, 0.0, -normalized_direction.x),
        vec3<f32>(1.0, 0.0, 0.0),
    );
    let local_bitangent = normalize_or_fallback(cross(local_tangent, normalized_direction), vec3<f32>(0.0, -1.0, 0.0));
    return ProjectionFrame(
        vec2<f32>(u / safe_tile_size_u, v / safe_tile_size_v),
        local_tangent,
        local_bitangent,
        normalized_direction,
        equator_length / safe_tile_size_u,
    );
}

// This maps a cylinder whose local symmetry axis is Z.
fn local_cylindrical_projection(
    local_position: vec3<f32>,
    local_normal: vec3<f32>,
    local_projection_scale: vec3<f32>,
    tile_size_u: f32,
    tile_size_v: f32,
) -> ProjectionFrame {
    let safe_tile_size_u = safe_texture_tile_size(tile_size_u);
    let safe_tile_size_v = safe_texture_tile_size(tile_size_v);
    let abs_normal = abs(local_normal);
    // The cylinder mesh has radius 1 and length 1.
    let half_size = local_projection_scale * vec3<f32>(1.0, 1.0, 0.5);
    let from_min = local_position + half_size;
    let from_max = half_size - local_position;

    // Both caps are flat projections of a linear position, so neither has a wrap seam.
    if (abs_normal.z >= max(abs_normal.x, abs_normal.y)) {
        if (local_normal.z >= 0.0) {
            return ProjectionFrame(
                vec2<f32>(from_min.x / safe_tile_size_u, from_max.y / safe_tile_size_v),
                vec3<f32>(1.0, 0.0, 0.0),
                vec3<f32>(0.0, -1.0, 0.0),
                vec3<f32>(0.0, 0.0, 1.0),
                0.0,
            );
        }
        return ProjectionFrame(
            vec2<f32>(from_max.x / safe_tile_size_u, from_max.y / safe_tile_size_v),
            vec3<f32>(-1.0, 0.0, 0.0),
            vec3<f32>(0.0, -1.0, 0.0),
            vec3<f32>(0.0, 0.0, -1.0),
            0.0,
        );
    }

    let radial = normalize_or_fallback(vec3<f32>(local_position.x, local_position.y, 0.0), vec3<f32>(1.0, 0.0, 0.0));
    let radius = max(length(local_position.xy), EPSILON);
    let angle = atan2(radial.y, radial.x);
    // The side measures u as arc length, so one turn around it covers the full circumference.
    return ProjectionFrame(
        vec2<f32>((angle + PI) * radius / safe_tile_size_u, from_max.z / safe_tile_size_v),
        vec3<f32>(-radial.y, radial.x, 0.0),
        vec3<f32>(0.0, 0.0, -1.0),
        radial,
        2.0 * PI * radius / safe_tile_size_u,
    );
}


// -------------------------- Albedo + metallic + roughness sampling --------------------------
// These helpers are only for the non-normal textures.

// Spherical and cylindrical UVs come from atan2, which jumps by one full turn where it wraps. That jump
// lands in the implicit derivative, so the one pixel wide column sitting on the seam would ask for a mip
// covering the entire texture and come out as a blurred stripe down the back of every ball and cylinder.
// Folding the jump back into the nearest period gives that column the same gradient its neighbors have.
// skybox.wgsl does the same thing for the same reason, specialized to a period of one.
fn wrapped_uv_gradient(uv_gradient: vec2<f32>, u_wrap_period: f32) -> vec2<f32> {
    if (u_wrap_period <= EPSILON) {
        return uv_gradient;
    }
    let wrapped_u = uv_gradient.x - u_wrap_period * round(uv_gradient.x / u_wrap_period);
    return vec2<f32>(wrapped_u, uv_gradient.y);
}

// Select coordinates first, then calculate derivatives outside the mapping branch. Browser WebGPU
// requires derivatives in uniform control flow, even though the mapping mode is flat per instance.
fn sample_material_texture(
    texture_to_sample: texture_2d<f32>,
    in: VertexShaderOutput,
    projection_frame: ProjectionFrame,
) -> vec4<f32> {
    var uv = projection_frame.uv;
    if (!projection_frame_is_mesh_uv(projection_frame)) {
        uv = apply_texture_tile_offset(uv, in.texture_tile_transform);
    }
    return textureSampleGrad(
        texture_to_sample,
        material_sampler,
        uv,
        wrapped_uv_gradient(dpdx(uv), projection_frame.u_wrap_period),
        wrapped_uv_gradient(dpdy(uv), projection_frame.u_wrap_period),
    );
}

// The mesh UV path has no projection frame at all: it samples in.uv straight and takes its tangent
// frame from the interpolated mesh tangent instead. A negative wrap period marks that case, since a
// real period is a length and never negative.
const PROJECTION_FRAME_MESH_UV_WRAP_PERIOD: f32 = -1.0;

fn projection_frame_is_mesh_uv(projection_frame: ProjectionFrame) -> bool {
    return projection_frame.u_wrap_period < 0.0;
}

// This is the one place that chooses between mesh UVs and local projection. A fragment builds the
// frame once and hands it to all four sample sites: basecolor, metallic, roughness, and the normal
// map used to rebuild the whole thing, trig and all, four separate times.
fn build_projection_frame(in: VertexShaderOutput) -> ProjectionFrame {
    if (in.texture_mapping_mode == INSTANCE_TEXTURE_MAPPING_LOCAL_BOX) {
        return local_box_projection(
            in.local_position_for_projection,
            in.local_normal_for_projection,
            in.local_projection_scale,
            in.texture_tile_transform.x,
            in.texture_tile_transform.y,
        );
    }

    if (in.texture_mapping_mode == INSTANCE_TEXTURE_MAPPING_LOCAL_SPHERICAL) {
        return local_spherical_projection(
            in.sphere_direction_for_projection,
            // The ball mesh is a unit sphere, so its scaled local position is exactly one radius long.
            length(in.local_position_for_projection),
            in.texture_tile_transform.x,
            in.texture_tile_transform.y,
        );
    }

    if (in.texture_mapping_mode == INSTANCE_TEXTURE_MAPPING_LOCAL_CYLINDRICAL) {
        return local_cylindrical_projection(
            in.local_position_for_projection,
            in.local_normal_for_projection,
            in.local_projection_scale,
            in.texture_tile_transform.x,
            in.texture_tile_transform.y,
        );
    }

    // Mesh UVs, and any mapping mode this shader does not know about. The axes are placeholders that
    // projection_frame_is_mesh_uv keeps every caller from reading.
    return ProjectionFrame(
        in.uv,
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
        PROJECTION_FRAME_MESH_UV_WRAP_PERIOD,
    );
}


// For the passes that sample exactly one texture and have no frame of their own to reuse.
fn sample_non_normal_texture(texture_to_sample: texture_2d<f32>, in: VertexShaderOutput) -> vec4<f32> {
    return sample_material_texture(texture_to_sample, in, build_projection_frame(in));
}

// This collects the material values that are not part of the normal-map path.
// The normal map is sampled later in sample_world_normal.
fn discard_fragment_for_mask_alpha(basecolor_alpha: f32) {
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_MASK) {
        if (basecolor_alpha < clamp(material_alpha_mode.alpha_cutoff, 0.0, 1.0)) {
            discard;
        }
    }
}

fn sample_material_without_normal(in: VertexShaderOutput, projection_frame: ProjectionFrame) -> MaterialSample {
    let basecolor_sample = sample_material_texture(basecolor_texture, in, projection_frame);
    let metallic_sample = sample_material_texture(metallic_texture, in, projection_frame);
    let roughness_sample = sample_material_texture(roughness_texture, in, projection_frame);

    let basecolor = max(basecolor_sample.rgb * in.color.rgb, vec3<f32>(0.0));
    let alpha_mode = material_alpha_mode.alpha_mode;
    var alpha = in.color.a;
    if (alpha_mode == MATERIAL_ALPHA_MODE_MASK) {
        discard_fragment_for_mask_alpha(basecolor_sample.a);
        let alpha_cutoff = clamp(material_alpha_mode.alpha_cutoff, 0.0, 1.0);
        // The opaque MSAA path uses this alpha for alpha-to-coverage, so keep the hard cutoff,
        // then remap the surviving edge alpha into the 0..1 coverage range.
        // The remap covers the texture's own alpha only. The instance's opacity multiplies in
        // afterwards: folding it in first pushed it through the cutoff subtraction too, so a cutout
        // material set below full opacity faded out far too fast and vanished outright at the cutoff.
        let mask_coverage = clamp((basecolor_sample.a - alpha_cutoff) / max(1.0 - alpha_cutoff, EPSILON), 0.0, 1.0);
        alpha = clamp(mask_coverage * in.color.a, 0.0, 1.0);
    } else if (alpha_mode == MATERIAL_ALPHA_MODE_OPAQUE) {
        alpha = clamp(in.color.a, 0.0, 1.0);
    } else if (alpha_mode == MATERIAL_ALPHA_MODE_BLEND) {
        alpha = clamp(basecolor_sample.a * in.color.a, 0.0, 1.0);
    }
    let metallic = clamp(metallic_sample.b, 0.0, 1.0);
    let roughness = clamp(roughness_sample.g, 0.045, 1.0);
    return MaterialSample(basecolor, alpha, metallic, roughness);
}

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let alpha = roughness * roughness;
    let alpha_squared = alpha * alpha;
    let denominator = n_dot_h * n_dot_h * (alpha_squared - 1.0) + 1.0;
    return alpha_squared / max(PI * denominator * denominator, EPSILON);
}

fn geometry_schlick_ggx(n_dot_direction: f32, roughness: f32) -> f32 {
    let roughness_plus_one = roughness + 1.0;
    let k = (roughness_plus_one * roughness_plus_one) / 8.0;
    let denominator = n_dot_direction * (1.0 - k) + k;
    return n_dot_direction / max(denominator, EPSILON);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let geometry_view = geometry_schlick_ggx(n_dot_v, roughness);
    let geometry_light = geometry_schlick_ggx(n_dot_l, roughness);
    return geometry_view * geometry_light;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - cos_theta, 5.0);
}

fn evaluate_directional_light(
    directional_light: DirectionalLight,
    normal: vec3<f32>,
    view_direction: vec3<f32>,
    material: MaterialSample,
) -> vec3<f32> {
    let light_direction = normalize(directional_light.direction_to_light);
    let half_direction = normalize(view_direction + light_direction);
    let n_dot_l = max(dot(normal, light_direction), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }

    let n_dot_v = max(dot(normal, view_direction), EPSILON);
    let n_dot_h = max(dot(normal, half_direction), 0.0);
    let h_dot_v = max(dot(half_direction, view_direction), 0.0);
    let f0 = mix(DIELECTRIC_F0, material.basecolor, vec3<f32>(material.metallic));

    let fresnel = fresnel_schlick(h_dot_v, f0);
    let distribution = distribution_ggx(n_dot_h, material.roughness);
    let geometry = geometry_smith(n_dot_v, n_dot_l, material.roughness);
    let specular = (distribution * geometry * fresnel) / max(4.0 * n_dot_v * n_dot_l, EPSILON);

    let diffuse_ratio = (vec3<f32>(1.0) - fresnel) * (1.0 - material.metallic);
    let diffuse = diffuse_ratio * material.basecolor / PI;

    return (diffuse + specular) * directional_light.color * n_dot_l;
}

// -------------------------- Normal map sampling --------------------------
// These helpers are only for the normal-map path.

fn decode_tangent_space_normal(encoded_normal: vec3<f32>) -> vec3<f32> {
    return encoded_normal * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
}

fn projection_frame_normal_to_local_normal(sampled_tangent_normal: vec3<f32>, projection_frame: ProjectionFrame) -> vec3<f32> {
    let local_normal =
        projection_frame.local_tangent * sampled_tangent_normal.x +
        projection_frame.local_bitangent * sampled_tangent_normal.y +
        projection_frame.local_normal * sampled_tangent_normal.z;
    return normalize_or_fallback(local_normal, projection_frame.local_normal);
}

fn local_normal_to_world_normal(local_normal: vec3<f32>, in: VertexShaderOutput) -> vec3<f32> {
    let world_normal =
        in.world_normal_axis_x * local_normal.x +
        in.world_normal_axis_y * local_normal.y +
        in.world_normal_axis_z * local_normal.z;
    return normalize_or_fallback(world_normal, in.world_normal);
}

fn sample_world_normal(in: VertexShaderOutput, projection_frame: ProjectionFrame) -> vec3<f32> {
    // All three local projections carry a tangent frame matched to the surface they project onto, so
    // they decode the normal map straight through it. Only mesh UVs have to build a TBN out of the
    // interpolated mesh tangent.
    let sampled_tangent_normal = decode_tangent_space_normal(sample_material_texture(normal_texture, in, projection_frame).xyz);
    if (!projection_frame_is_mesh_uv(projection_frame)) {
        let local_normal = projection_frame_normal_to_local_normal(sampled_tangent_normal, projection_frame);
        return local_normal_to_world_normal(local_normal, in);
    }

    let world_normal = normalize(in.world_normal);
    var world_tangent = in.world_tangent.xyz - world_normal * dot(world_normal, in.world_tangent.xyz);
    if (length(world_tangent) <= EPSILON) {
        if (abs(world_normal.z) < 0.999) {
            world_tangent = normalize(cross(vec3<f32>(0.0, 0.0, 1.0), world_normal));
        } else {
            world_tangent = normalize(cross(vec3<f32>(1.0, 0.0, 0.0), world_normal));
        }
    } else {
        world_tangent = normalize(world_tangent);
    }
    let world_bitangent = normalize(cross(world_normal, world_tangent)) * in.world_tangent.w;
    let tangent_to_world = mat3x3<f32>(world_tangent, world_bitangent, world_normal);
    return normalize(tangent_to_world * sampled_tangent_normal);
}

// A world position's shadow map UV and depth in one cascade. The shadow projections are orthographic, so there is no
// perspective divide.
fn project_shadow_cascade(cascade_index: u32, world_position: vec3<f32>) -> vec3<f32> {
    let light_ndc_position = (shadow.light_view_proj[cascade_index] * vec4<f32>(world_position, 1.0)).xyz;
    return vec3<f32>(light_ndc_position.x * 0.5 + 0.5, 0.5 - light_ndc_position.y * 0.5, light_ndc_position.z);
}

// Cascades are addressed by a fractional position: 1.5 is halfway from the second cascade to the third. Texel sizes
// ease between neighboring cascades in log space, the way mip levels do.
fn shadow_texel_size_at(cascade_position: f32) -> f32 {
    let finer_cascade_index = u32(cascade_position);
    let coarser_cascade_index = min(finer_cascade_index + 1u, SHADOW_CASCADE_COUNT - 1u);
    let finer_texel_size = shadow.cascade_settings[finer_cascade_index].y;
    return finer_texel_size * pow(shadow.cascade_settings[coarser_cascade_index].y / finer_texel_size, fract(cascade_position));
}

fn shadow_cascade_position_of_texel_size(texel_size: f32) -> f32 {
    for (var cascade_index = 0u; cascade_index + 1u < SHADOW_CASCADE_COUNT; cascade_index = cascade_index + 1u) {
        let finer_texel_size = shadow.cascade_settings[cascade_index].y;
        let coarser_texel_size = shadow.cascade_settings[cascade_index + 1u].y;
        if (texel_size < coarser_texel_size) {
            return f32(cascade_index) + max(log(texel_size / finer_texel_size), 0.0) / log(coarser_texel_size / finer_texel_size);
        }
    }
    return f32(SHADOW_CASCADE_COUNT - 1u);
}

// The first cascade that holds this position with room around it for the blur, plus how far into that cascade's edge band
// it sits, where it fades into the next cascade. -1 when no cascade holds it.
fn shadow_cascade_position_of_coverage(world_position: vec3<f32>, blur_radius: f32) -> f32 {
    for (var cascade_index = 0u; cascade_index < SHADOW_CASCADE_COUNT; cascade_index = cascade_index + 1u) {
        let light_ndc_position = (shadow.light_view_proj[cascade_index] * vec4<f32>(world_position, 1.0)).xyz;
        let reach = max(blur_radius, 4.0 * shadow.cascade_settings[cascade_index].y);
        // The outermost cascade has nothing to hand off to, so trimming it would leave a shadowless ring around the world
        // rather than a seam between two cascades.
        let is_outermost = cascade_index + 1u == SHADOW_CASCADE_COUNT;
        let edge_distance = max(abs(light_ndc_position.x), abs(light_ndc_position.y)) + select(reach / shadow.cascade_settings[cascade_index].x, 0.0, is_outermost);
        if (edge_distance <= 1.0 && light_ndc_position.z >= 0.0 && light_ndc_position.z <= 1.0) {
            return f32(cascade_index) + select(smoothstep(1.0 - SHADOW_CASCADE_BLEND_BAND_NDC, 1.0, edge_distance), 0.0, is_outermost);
        }
    }
    return -1.0;
}

// How much of the sun the receiver sees: compares spread evenly over a disk, each against the receiver's surface extended
// out to that tap. Taps count less toward the rim, which keeps the shadow map's texel steps from rippling along soft
// edges; the disk is wider than the blur radius so the edge still blurs about that much.
fn sample_shadow_blur(cascade_index: u32, world_position: vec3<f32>, receiver_slope: vec2<f32>, blur_radius: f32) -> f32 {
    let receiver = project_shadow_cascade(cascade_index, world_position);
    let world_per_uv = 2.0 * shadow.cascade_settings[cascade_index].x;
    var direction = vec2<f32>(1.0, 0.0);
    var visibility_sum = 0.0;
    var weight_sum = 0.0;
    for (var index = 0; index < SHADOW_BLUR_TAP_COUNT; index = index + 1) {
        let distance_squared = (f32(index) + 0.5) / f32(SHADOW_BLUR_TAP_COUNT);
        let offset = sqrt(distance_squared) * SHADOW_BLUR_DISK_SCALE * blur_radius * direction;
        direction = GOLDEN_ANGLE_ROTATION * direction;
        let tap_depth = receiver.z + dot(receiver_slope, offset) / shadow.settings.w;
        let weight = (1.0 - distance_squared) * (1.0 - distance_squared);
        visibility_sum += weight * textureSampleCompareLevel(shadow_depth_textures, shadow_compare_sampler, receiver.xy + offset / world_per_uv, cascade_index, tap_depth);
        weight_sum += weight;
    }
    return visibility_sum / weight_sum;
}

// Soft sun shadows. Every shadow edge blurs by the same world distance, wherever it falls and however far it is from what
// casts it. The blur reads the cascade whose texels suit its width, easing between neighboring cascades, so it needs no
// random sampling and shows no steps.
fn sample_sun_shadow(world_position: vec3<f32>, normal: vec3<f32>, direction_to_light: vec3<f32>) -> f32 {
    let normal_to_light = max(dot(normal, direction_to_light), 0.0);
    // Normal bias grows at grazing sun angles because one shadow texel covers more distance across the receiver.
    let biased_world_position = world_position + direction_to_light * shadow.settings.x +
        normal * shadow.settings.y * sqrt(max(1.0 - normal_to_light * normal_to_light, 0.0));

    // How far the surface drops away from the sun per unit across the shadow map, so taps around the receiver compare
    // against the surface where they land. Derivatives only mean anything when the whole quad runs them together, so this runs
    // before anything branches. Every cascade looks along the same direction with the same depth range, so one slope
    // serves them all.
    let receiver_close = project_shadow_cascade(0u, biased_world_position);
    let uv_dx = dpdx(receiver_close.xy);
    let uv_dy = dpdy(receiver_close.xy);
    let depth_dx = dpdx(receiver_close.z);
    let depth_dy = dpdy(receiver_close.z);
    let uv_determinant = uv_dx.x * uv_dy.y - uv_dx.y * uv_dy.x;
    var receiver_slope = vec2<f32>(0.0);
    if (abs(uv_determinant) > 1e-3 * length(uv_dx) * length(uv_dy)) {
        let depth_gradient_uv = vec2<f32>(depth_dx * uv_dy.y - uv_dx.y * depth_dy, uv_dx.x * depth_dy - depth_dx * uv_dy.x) / uv_determinant;
        receiver_slope = depth_gradient_uv * shadow.settings.w / (2.0 * shadow.cascade_settings[0].x);
        receiver_slope *= min(SHADOW_MAX_RECEIVER_SLOPE / max(length(receiver_slope), EPSILON), 1.0);
    }

    if (normal_to_light <= 0.0) {
        // The surface itself blocks the sun. A normal map can tilt the shading normal toward the
        // light, but cannot make sunlight pass through the back of the underlying surface.
        return 0.0;
    }
    let coverage_position = shadow_cascade_position_of_coverage(biased_world_position, shadow.settings.z);
    if (coverage_position < 0.0) {
        return 1.0;
    }
    let blur_radius = max(shadow.settings.z, SHADOW_MIN_BLUR_TEXELS * shadow_texel_size_at(coverage_position));
    // The blur stays in a cascade until the next coarser one has texels no wider than half the blur, small enough that
    // the blur hides them, and only then hands off to it.
    let blur_texel_position = shadow_cascade_position_of_texel_size(blur_radius * 0.5);
    let blur_position = max(
        coverage_position,
        floor(blur_texel_position) + smoothstep(1.0 - SHADOW_BLUR_CASCADE_HANDOFF, 1.0, fract(blur_texel_position)),
    );
    let finer_cascade_index = u32(blur_position);
    let visibility = sample_shadow_blur(finer_cascade_index, biased_world_position, receiver_slope, blur_radius);
    let coarser_weight = fract(blur_position);
    // select would evaluate the second blur either way, so this stays an if.
    if (coarser_weight <= 0.0 || finer_cascade_index + 1u >= SHADOW_CASCADE_COUNT) {
        return visibility;
    }
    return mix(visibility, sample_shadow_blur(finer_cascade_index + 1u, biased_world_position, receiver_slope, blur_radius), coarser_weight);
}

// -------------------------- Light to display color --------------------------
// Lighting adds up linear light, and a sunlit surface can easily be brighter than a screen can show. Clipping each
// channel at 1 is what used to turn bright surfaces flat and pastel. The Khronos PBR Neutral curve instead leaves
// colors below about 0.76 nearly as they are, so picked colors still read true, and rolls anything brighter off
// smoothly toward white. https://github.com/KhronosGroup/ToneMapping/tree/main/PBR_Neutral
// Every shader that draws lit color into the scene runs it through this same curve, so the sky and the ground meet
// without a seam. Keep these functions identical to the copies in sky_gradient.wgsl.
const TONE_CURVE_COMPRESSION_START: f32 = 0.76;
const TONE_CURVE_DESATURATION: f32 = 0.15;

fn apply_tone_curve(color: vec3<f32>) -> vec3<f32> {
    // A small offset near black gives the darks a little more contrast.
    let lowest_channel = min(color.r, min(color.g, color.b));
    let black_offset = select(0.04, lowest_channel - 6.25 * lowest_channel * lowest_channel, lowest_channel < 0.08);
    let offset_color = color - black_offset;
    let peak = max(offset_color.r, max(offset_color.g, offset_color.b));
    if (peak < TONE_CURVE_COMPRESSION_START) {
        return offset_color;
    }
    // The brightest channel is squeezed under 1, the others scale with it so the hue holds, and very bright
    // light drifts slightly toward white the way an overexposed photo does.
    let compression_range = 1.0 - TONE_CURVE_COMPRESSION_START;
    let compressed_peak = 1.0 - compression_range * compression_range / (peak + compression_range - TONE_CURVE_COMPRESSION_START);
    let toward_white = 1.0 - 1.0 / (TONE_CURVE_DESATURATION * (peak - compressed_peak) + 1.0);
    return mix(offset_color * (compressed_peak / peak), vec3<f32>(compressed_peak), toward_white);
}

// The color target stores 256 sRGB steps per channel, which shows up as bands across slow gradients like the
// sky. Nudging each pixel by up to half a step, in a pattern that changes from pixel to pixel, trades those
// bands for noise too fine to see. The noise is interleaved gradient noise (Jimenez 2014).
fn dither_display_color(color: vec3<f32>, pixel_position: vec2<f32>) -> vec3<f32> {
    let noise = fract(52.9829189 * fract(dot(pixel_position, vec2<f32>(0.06711056, 0.00583715)))) - 0.5;
    let srgb_color = select(1.055 * pow(color, vec3<f32>(1.0 / 2.4)) - 0.055, color * 12.92, color <= vec3<f32>(0.0031308));
    let dithered_srgb_color = clamp(srgb_color + noise / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(
        pow((dithered_srgb_color + 0.055) / 1.055, vec3<f32>(2.4)),
        dithered_srgb_color / 12.92,
        dithered_srgb_color <= vec3<f32>(0.04045),
    );
}

// Vibrance pushes dull colors further from gray and already vivid ones barely at all, however bright or dark they are, so a
// world reads lively without strong colors going neon.
fn apply_vibrance(color: vec3<f32>) -> vec3<f32> {
    let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let brightest = max(color.r, max(color.g, color.b));
    let saturation = (brightest - min(color.r, min(color.g, color.b))) / max(brightest, 1e-4);
    let push_from_gray = 1.0 + clamp(lighting.display_settings.x, 0.0, 1.0) * (1.0 - saturation);
    return clamp(mix(vec3<f32>(luma), color, push_from_gray), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn convert_light_to_display_color(light: vec3<f32>, pixel_position: vec2<f32>) -> vec3<f32> {
    return dither_display_color(apply_vibrance(apply_tone_curve(max(light, vec3<f32>(0.0)))), pixel_position);
}

// Solid surfaces read the occlusion drawn from the opaque depth. Transparent surfaces sit in front of that depth,
// so it describes whatever is behind them, and they pass 1 instead.
fn sample_ambient_occlusion(in: VertexShaderOutput) -> f32 {
    return textureLoad(ambient_occlusion_texture, vec2<i32>(in.position.xy), 0).r;
}

// TODO: AI-PICKED-VALUE: Below one, this pulls the blend toward the horizon color, which keeps the pale band
// near the horizon tight instead of washing out the whole upper half of the sky.
const SKY_GRADIENT_FALLOFF: f32 = 0.45;

// Shiny surfaces reflect this same sky. Keep this function identical to the copy in sky_gradient.wgsl.
fn sky_gradient_color(direction: vec3<f32>) -> vec3<f32> {
    // Looking up blends the horizon color toward the top color, and looking down blends it toward the
    // bottom color. Both halves start from the horizon color, which is what puts the bright band on the eye line.
    let height = clamp(direction.y, -1.0, 1.0);
    let blend = pow(abs(height), SKY_GRADIENT_FALLOFF);
    let far_color = select(lighting.sky_bottom_color_and_padding.rgb, lighting.sky_top_color_and_padding.rgb, height >= 0.0);
    return mix(lighting.sky_horizon_color_and_padding.rgb, far_color, blend);
}

// Shiny surfaces reflect the skybox too. Keep this function identical to the copy in skybox.wgsl.
fn equirectangular_uv(direction: vec3<f32>) -> vec2<f32> {
    let clamped_y = clamp(direction.y, -1.0, 1.0);
    let u = atan2(direction.x, direction.z) / (2.0 * PI) + 0.5;
    let v = 0.5 - asin(clamped_y) / PI;
    return vec2<f32>(u, v);
}

// The sky seen in one direction: the gradient with the skybox image over it, the way the sky itself is drawn. Rougher
// surfaces read a blurrier mip of the image.
fn sample_sky_reflection(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let level = roughness * (skybox.settings.y - 1.0);
    let skybox_color = textureSampleLevel(skybox_texture, skybox_sampler, equirectangular_uv(direction), level);
    return mix(sky_gradient_color(direction), skybox_color.rgb, skybox_color.a * clamp(skybox.settings.x, 0.0, 1.0));
}

// How much of the sky a surface reflects toward the viewer, from its reflectance straight on, its roughness, and how
// squarely it faces the viewer. Karis's fit to the prefiltered environment BRDF, from "Physically Based Shading on
// Mobile" (2014).
fn environment_brdf(specular_color: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let scale_and_bias = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return specular_color * scale_and_bias.x + scale_and_bias.y;
}

// TODO: AI-PICKED-VALUE: Surfaces smoother than the start reflect the sky fully and ones rougher than the end reflect none
// of it, so plain bricks, at 0.5, keep only a faint trace. Reflecting it everywhere washed rough surfaces gray-blue at
// low angles and turned warm colors in shade mauve.
const SKY_REFLECTION_FADE_START_ROUGHNESS: f32 = 0.25;
const SKY_REFLECTION_FADE_END_ROUGHNESS: f32 = 0.6;

fn compute_pbr_lit_surface(in: VertexShaderOutput, is_front_facing: bool, ambient_occlusion: f32) -> LitSurface {
    // Built once and shared by all four texture samples below.
    let projection_frame = build_projection_frame(in);
    // glTF double-sided surfaces reverse the final shading normal on the back face, including normal maps.
    let face_sign = select(-1.0, 1.0, is_front_facing);
    let normal = sample_world_normal(in, projection_frame) * face_sign;
    let camera_position = camera.camera_position.xyz;
    let view_direction = normalize(camera_position - in.world_position);

    let direction_to_light = normalize_or_fallback(lighting.sun_direction.xyz, vec3<f32>(0.0, 1.0, 0.0));
    let sun_light = DirectionalLight(
        direction_to_light,
        max(lighting.sun_color_and_intensity.rgb, vec3<f32>(0.0)) * max(lighting.sun_color_and_intensity.a, 0.0),
    );

    let shadow_normal = normalize_or_fallback(in.world_normal, vec3<f32>(0.0, 1.0, 0.0)) * face_sign;
    let shadow_factor = sample_sun_shadow(in.world_position, shadow_normal, direction_to_light);
    // Alpha masking may discard this fragment. Finish normal-map and shadow derivatives first.
    let material = sample_material_without_normal(in, projection_frame);
    let direct_light = evaluate_directional_light(sun_light, normal, view_direction, material) * shadow_factor;

    let hemisphere_blend = dot(normal, vec3<f32>(0.0, 1.0, 0.0)) * 0.5 + 0.5;
    let ambient_color = mix(
        max(lighting.ambient_bottom_color_and_padding.rgb, vec3<f32>(0.0)),
        max(lighting.ambient_top_color_and_intensity.rgb, vec3<f32>(0.0)),
        hemisphere_blend,
    );
    let ambient_light = ambient_color * max(lighting.ambient_top_color_and_intensity.a, 0.0);
    let ambient = ambient_light * material.basecolor * (1.0 - material.metallic);
    // Shiny surfaces reflect the sky. A rougher one bends its reflection toward its normal and blends it into the sky
    // light, since a blurrier reflection averages more of the sky.
    let roughness_squared = material.roughness * material.roughness;
    let reflection_direction = normalize(mix(reflect(-view_direction, normal), normal, roughness_squared));
    let sky_reflection = mix(sample_sky_reflection(reflection_direction, material.roughness), ambient_light, roughness_squared);
    let specular_color = mix(DIELECTRIC_F0, material.basecolor, vec3<f32>(material.metallic));
    // Only smooth surfaces show the sky. Metal keeps its reflection however rough it is, since what it reflects is all of
    // its color.
    let sky_reflection_amount = mix(1.0 - smoothstep(SKY_REFLECTION_FADE_START_ROUGHNESS, SKY_REFLECTION_FADE_END_ROUGHNESS, material.roughness), 1.0, material.metallic);
    let reflected_sky = sky_reflection * environment_brdf(specular_color, material.roughness, max(dot(normal, view_direction), 0.0))
        * sky_reflection_amount * clamp(lighting.sky_reflection_settings.x, 0.0, 1.0);
    let sunlight_occlusion = mix(1.0, ambient_occlusion, clamp(lighting.ambient_occlusion_settings.z, 0.0, 1.0));
    let occluded_light = (ambient + reflected_sky) * ambient_occlusion + direct_light * sunlight_occlusion;
    // A glowing surface shines in its own color whatever light reaches it, like neon.
    let glowing_light = mix(occluded_light, material.basecolor, clamp(in.glow, 0.0, 1.0));
    // Opaque and transparent surfaces both leave here as display color, so transparent layers blend with the
    // opaque scene behind them in the same space the screen shows.
    let color = convert_light_to_display_color(glowing_light, in.position.xy);
    return LitSurface(color, material.alpha);
}

// Compute the skinning matrix from joint weights.
fn get_skin_matrix(joint_indices: vec4<u32>, joint_weights: vec4<f32>) -> mat4x4<f32> {
    let identity_matrix = mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0)
    );

    var skin_matrix = mat4x4<f32>(
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        vec4<f32>(0.0)
    );

    // Weights are summed as they are used, not up front, so a joint this shader cannot reach never
    // gets counted. Normalizing over all four first and then skipping some of them left the matrix
    // scaled by only the fraction that survived, which drags the vertex toward the model's origin.
    var used_weight = 0.0;
    if (joint_weights.x > 0.0 && joint_indices.x < MAX_JOINTS) {
        skin_matrix += joints.matrices[joint_indices.x] * joint_weights.x;
        used_weight += joint_weights.x;
    }
    if (joint_weights.y > 0.0 && joint_indices.y < MAX_JOINTS) {
        skin_matrix += joints.matrices[joint_indices.y] * joint_weights.y;
        used_weight += joint_weights.y;
    }
    if (joint_weights.z > 0.0 && joint_indices.z < MAX_JOINTS) {
        skin_matrix += joints.matrices[joint_indices.z] * joint_weights.z;
        used_weight += joint_weights.z;
    }
    if (joint_weights.w > 0.0 && joint_indices.w < MAX_JOINTS) {
        skin_matrix += joints.matrices[joint_indices.w] * joint_weights.w;
        used_weight += joint_weights.w;
    }

    // Nothing contributed: no weights at all, or every joint out of range. Either way an all-zero
    // matrix would collapse the vertex onto the origin, so leave it where the mesh put it.
    if (used_weight < 0.001) {
        return identity_matrix;
    }

    return skin_matrix * (1.0 / used_weight);
}

// -------------------------- Shaders used by the Rust code (fn names must match) --------------------------

@vertex
fn vs_main_static(v: VertexContent, i: InstanceContent) -> VertexShaderOutput {
    let model = build_model_matrix(i);
    let model_normal_matrix = build_normal_matrix(model);
    let model_tangent_matrix = build_tangent_matrix(model);
    let projected_normal_axes = build_projected_normal_axes(model_normal_matrix);
    let local_position_for_projection = build_local_projection_position(v.position, i.local_projection_scale.xyz);
    let local_normal_for_projection = build_local_projection_normal(v.normal, i.local_projection_scale.xyz);
    let sphere_direction_for_projection = build_sphere_direction_for_projection(v.position, v.normal);
    let world_position = (model * vec4<f32>(v.position, 1.0)).xyz;
    let clip_position = camera.view_proj * vec4<f32>(world_position, 1.0);
    let world_normal = model_normal_matrix * v.normal;
    let world_tangent = model_tangent_matrix * v.tangent.xyz;
    return build_vertex_shader_output(
        clip_position,
        i.color,
        world_normal,
        v.uv,
        world_position,
        world_tangent,
        v.tangent.w,
        local_position_for_projection,
        local_normal_for_projection,
        i.local_projection_scale.xyz,
        sphere_direction_for_projection,
        i.texture_mapping_mode,
        i.texture_tile_transform,
        projected_normal_axes,
        i.glow,
    );
}

@vertex
fn vs_main_skinned(v: SkinnedVertexContent, i: InstanceContent) -> VertexShaderOutput {
    let model = build_model_matrix(i);
    let skin_matrix = get_skin_matrix(v.joint_indices, v.joint_weights);

    // Transform position and normal in mesh space by skinning first.
    let skinned_position = skin_matrix * vec4<f32>(v.position, 1.0);
    let skin_tangent_matrix = mat3x3<f32>(
        skin_matrix[0].xyz,
        skin_matrix[1].xyz,
        skin_matrix[2].xyz,
    );
    let skin_normal_matrix = build_inverse_transpose_matrix(skin_tangent_matrix);
    let skinned_normal = skin_normal_matrix * v.normal;
    let skinned_tangent = skin_tangent_matrix * v.tangent.xyz;
    let local_position_for_projection = build_local_projection_position(skinned_position.xyz, i.local_projection_scale.xyz);
    let local_normal_for_projection = build_local_projection_normal(skinned_normal, i.local_projection_scale.xyz);
    let sphere_direction_for_projection = build_sphere_direction_for_projection(skinned_position.xyz, skinned_normal);

    let model_normal_matrix = build_normal_matrix(model);
    let model_tangent_matrix = build_tangent_matrix(model);
    let projected_normal_axes = build_projected_normal_axes(model_normal_matrix);
    let world_position = (model * skinned_position).xyz;
    let clip_position = camera.view_proj * vec4<f32>(world_position, 1.0);
    let world_normal = model_normal_matrix * skinned_normal;
    let world_tangent = model_tangent_matrix * skinned_tangent;
    return build_vertex_shader_output(
        clip_position,
        i.color,
        world_normal,
        v.uv,
        world_position,
        world_tangent,
        v.tangent.w,
        local_position_for_projection,
        local_normal_for_projection,
        i.local_projection_scale.xyz,
        sphere_direction_for_projection,
        i.texture_mapping_mode,
        i.texture_tile_transform,
        projected_normal_axes,
        i.glow,
    );
}

// For depth-only passes over materials with nothing to cut out, which need where a vertex lands and nothing else. Each
// computes its position with the same steps as the vs_main_ entry point above it, and @invariant makes the GPU round
// those steps the same way in every entry point, so these write the exact depth the full shaders do.
@vertex
fn vs_position_only_static(v: VertexContent, i: InstanceContent) -> @builtin(position) @invariant vec4<f32> {
    let world_position = (build_model_matrix(i) * vec4<f32>(v.position, 1.0)).xyz;
    return camera.view_proj * vec4<f32>(world_position, 1.0);
}

@vertex
fn vs_position_only_skinned(v: SkinnedVertexContent, i: InstanceContent) -> @builtin(position) @invariant vec4<f32> {
    let skinned_position = get_skin_matrix(v.joint_indices, v.joint_weights) * vec4<f32>(v.position, 1.0);
    let world_position = (build_model_matrix(i) * skinned_position).xyz;
    return camera.view_proj * vec4<f32>(world_position, 1.0);
}

@fragment
fn fs_main(in: VertexShaderOutput, @builtin(front_facing) is_front_facing: bool) -> @location(0) vec4<f32> {
    let lit = compute_pbr_lit_surface(in, is_front_facing, sample_ambient_occlusion(in));
    // Blend's fully opaque texels write depth here; partial texels belong exclusively to OIT.
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_BLEND && lit.alpha < 1.0) {
        discard;
    }
    return vec4<f32>(lit.color, lit.alpha);
}

// -------------------------- Alpha-masked opaque entry points --------------------------
// A cutout edge gets its softness from how many samples it covers. The obvious way to spell that is
// alpha-to-coverage, but that reads coverage out of the same alpha the color target stores, so the
// edge ended up carrying coverage in its alpha channel too. The OIT composite reads that alpha back
// as coverage and blends accordingly, which counted the same coverage twice and added a slice of the
// background on top of a resolved color that already contained it: a bright halo along every cutout.
// Writing the coverage as a sample mask instead leaves alpha free to mean "fully opaque where drawn",
// which is what the composite needs it to mean.
struct MaskedFragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(sample_mask) sample_mask: u32,
};

fn coverage_sample_mask(coverage: f32) -> u32 {
    let covered_sample_count = u32(round(clamp(coverage, 0.0, 1.0) * f32(OPAQUE_MULTISAMPLE_COUNT)));
    // Shifting by the sample count would be a full-width shift when everything is covered.
    if (covered_sample_count >= OPAQUE_MULTISAMPLE_COUNT) {
        return 0xffffffffu;
    }
    return (1u << covered_sample_count) - 1u;
}

fn masked_fragment_output(lit: LitSurface) -> MaskedFragmentOutput {
    var out: MaskedFragmentOutput;
    out.color = vec4<f32>(lit.color, 1.0);
    out.sample_mask = coverage_sample_mask(lit.alpha);
    return out;
}

@fragment
fn fs_main_mask(in: VertexShaderOutput, @builtin(front_facing) is_front_facing: bool) -> MaskedFragmentOutput {
    return masked_fragment_output(compute_pbr_lit_surface(in, is_front_facing, sample_ambient_occlusion(in)));
}

// WebGL2 has no sample mask, so the WebGL build draws each pixel of a cutout whole or not at all. A pixel stays when its
// coverage is above a noise value that changes from pixel to pixel, the same noise dither_display_color uses. That way
// edges, faint cutouts, and fading cutouts cover about as much of the picture as the sample mask would.
fn whole_pixel_masked_fragment_output(lit: LitSurface, pixel_position: vec2<f32>) -> vec4<f32> {
    let noise = fract(52.9829189 * fract(dot(pixel_position, vec2<f32>(0.06711056, 0.00583715))));
    if (lit.alpha <= noise) {
        discard;
    }
    return vec4<f32>(lit.color, 1.0);
}

@fragment
fn fs_main_mask_whole_pixels(in: VertexShaderOutput, @builtin(front_facing) is_front_facing: bool) -> @location(0) vec4<f32> {
    return whole_pixel_masked_fragment_output(compute_pbr_lit_surface(in, is_front_facing, sample_ambient_occlusion(in)), in.position.xy);
}

fn discard_fragment_for_depth(in: VertexShaderOutput) {
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_MASK) {
        let basecolor_alpha = sample_non_normal_texture(basecolor_texture, in).a;
        discard_fragment_for_mask_alpha(basecolor_alpha);
    } else if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_BLEND) {
        // Fully transparent texels must not cast shadows or hide geometry in a depth pass.
        if (sample_non_normal_texture(basecolor_texture, in).a * in.color.a <= 0.0) {
            discard;
        }
    }
}

@fragment
fn fs_depth_only(in: VertexShaderOutput) {
    discard_fragment_for_depth(in);
}

// Replay only the solid portion of Blend into the single-sample depth used by OIT and labels.
@fragment
fn fs_opaque_depth_only(in: VertexShaderOutput) {
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_BLEND) {
        if (sample_non_normal_texture(basecolor_texture, in).a * in.color.a < 1.0) {
            discard;
        }
    } else {
        discard_fragment_for_depth(in);
    }
}


// Selection masks use the same cutouts as the visible surfaces, without lighting or scene depth.
@fragment
fn fs_selection_mask(in: VertexShaderOutput) -> @location(0) f32 {
    let alpha = sample_non_normal_texture(basecolor_texture, in).a;
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_MASK) {
        discard_fragment_for_mask_alpha(alpha);
    }
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_BLEND && alpha * in.color.a <= 0.0) {
        discard;
    }
    return 1.0;
}

fn label_distance_from_edge(in: VertexShaderOutput) -> f32 {
    return sample_non_normal_texture(basecolor_texture, in).r - 0.5;
}

// Label letters are distance fields, so their edge is drawn sharp at any size: how many screen pixels one atlas texel spans
// turns the stored distance into screen pixels from the edge, and the edge softens across one of them. The derivatives run
// here, before anything that calls this can branch.
fn label_coverage(in: VertexShaderOutput) -> f32 {
    let distance_from_edge = label_distance_from_edge(in);
    let texels_per_pixel = fwidth(in.uv) * vec2<f32>(textureDimensions(basecolor_texture));
    let pixels_per_texel = 1.0 / max(0.5 * (texels_per_pixel.x + texels_per_pixel.y), 1e-6);
    return saturate(distance_from_edge * 2.0 * LABEL_DISTANCE_FIELD_RADIUS * pixels_per_texel + 0.5);
}

@fragment
fn fs_label_selection_mask(in: VertexShaderOutput) -> @location(0) f32 {
    let coverage = label_coverage(in);
    if (coverage * in.color.a <= LABEL_MIN_ALPHA) {
        discard;
    }
    return 1.0;
}

@fragment
fn fs_label_image_selection_mask(in: VertexShaderOutput) -> @location(0) f32 {
    if (textureSample(basecolor_texture, material_sampler, in.uv).a * in.color.a <= LABEL_MIN_ALPHA) {
        discard;
    }
    return 1.0;
}

// Both targets share one blend in renderer.rs OIT_BLENDING: color adds up, and alpha is multiplied by one minus each
// layer's alpha. So accum.rgb sums the weighted color, accum.a ends as how much of the scene shows through, and weight
// sums the weights.
struct OitFragmentOutput {
    @location(0) accum: vec4<f32>,
    @location(1) weight: f32,
};

// TODO: AI-PICKED-VALUE: The accumulation buffer is Rgba16Float, which saturates just past 65504, and
// the pass blends additively. Capping one fragment's contribution here leaves room for roughly sixty
// transparent layers to stack before the buffer saturates.
const OIT_MAX_ACCUM_PER_FRAGMENT: f32 = 1024.0;

// Weighted blended OIT needs a depth-aware weight so nearer transparent layers dominate farther ones.
// The color term follows McGuire/Bavoil's heuristic to avoid over-weighting very dark transparent fragments.
fn compute_oit_weight(color: vec3<f32>, alpha: f32, distance_to_camera: f32) -> f32 {
    let color_max = max(max(color.r, color.g), color.b);
    let coverage = max(min(1.0, color_max * alpha), alpha);
    let depth_weight = clamp(0.03 / (1e-5 + pow(distance_to_camera / 200.0, 4.0)), 1e-2, 3e3);
    // An HDR sun pushes color well above 1, and a bright enough fragment times a large depth weight
    // overflows the accumulation buffer to infinity. The composite then divides infinity by infinity
    // and the pixel comes out NaN. The bound goes on the weight rather than on the accumulated color
    // because the weight scales the numerator and denominator of the weighted average together;
    // clamping the color alone would quietly bias the blend toward dim fragments.
    let brightness = max(color_max, 1.0);
    return coverage * min(depth_weight, OIT_MAX_ACCUM_PER_FRAGMENT / brightness);
}

fn write_oit_fragment_output(lit: LitSurface, distance_to_camera: f32) -> OitFragmentOutput {
    let alpha = lit.alpha;
    // Fully opaque Blend texels already wrote color and depth in the opaque pass.
    if (material_alpha_mode.alpha_mode == MATERIAL_ALPHA_MODE_BLEND && alpha >= 1.0) {
        discard;
    }
    let weight = compute_oit_weight(lit.color, alpha, distance_to_camera);

    var out: OitFragmentOutput;
    out.accum = vec4<f32>(lit.color * alpha * weight, alpha);
    out.weight = alpha * weight;
    return out;
}

@fragment
fn fs_oit_transparent(in: VertexShaderOutput, @builtin(front_facing) is_front_facing: bool) -> OitFragmentOutput {
    let lit = compute_pbr_lit_surface(in, is_front_facing, 1.0);
    let distance_to_camera = distance(camera.camera_position.xyz, in.world_position);
    return write_oit_fragment_output(lit, distance_to_camera);
}

// TODO: AI-PICKED-VALUE: How much nearer than a glowing surface the scene's depth may be before it hides the glow, as a
// fraction of the surface's depth: about a millimeter at a hundred units, which covers rounding between two draws of the
// same surface.
const GLOW_DEPTH_SLACK: f32 = 1e-5;

// How much of a glow fragment the scene leaves in view, from 0 to 1. The glow pass draws at half the picture's size, so each
// of its pixels covers four of the scene's depth texels, and this counts the ones nothing nearer covers. A surface's own
// depth there differs from this fragment's by up to how much its depth changes across the pixel, so that much slack keeps
// it from hiding itself.
fn glow_visibility(position: vec4<f32>) -> f32 {
    let depth_slack = fwidth(position.z) + position.z * GLOW_DEPTH_SLACK;
    let last_texel = vec2<i32>(textureDimensions(scene_depth_texture)) - 1;
    let first_texel = vec2<i32>(floor(position.xy)) * 2;
    var visible_texel_count = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let texel = min(first_texel + vec2<i32>(x, y), last_texel);
            // Depth runs reversed here, so nearer is larger.
            if (position.z + depth_slack >= textureLoad(scene_depth_texture, texel, 0).r) {
                visible_texel_count += 1.0;
            }
        }
    }
    return visible_texel_count / 4.0;
}

// The glow a glowing instance spreads onto its surroundings, drawn into bloom level 0 for bloom.wgsl to blur: its own color,
// as strong as its glow. It tests itself against the scene's depth, so a wall in front of a glowing object hides its glow.
@fragment
fn fs_glow(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let visibility = glow_visibility(in.position);
    let material = sample_material_without_normal(in, build_projection_frame(in));
    return vec4<f32>(material.basecolor * clamp(in.glow, 0.0, 1.0) * material.alpha * visibility, 1.0);
}

// The same for a glowing label's text.
@fragment
fn fs_label_glow(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let coverage = label_coverage(in);
    let visibility = glow_visibility(in.position);
    let alpha = clamp(coverage * in.color.a, 0.0, 1.0);
    return vec4<f32>(max(in.color.rgb, vec3<f32>(0.0)) * clamp(in.glow, 0.0, 1.0) * alpha * visibility, 1.0);
}

// Unlit on purpose: text that dims at dusk is text you cannot read.
//
// Deliberately not OIT. Weighted blending averages the layers on a pixel, so a glyph over its own
// background panel comes out a mix of the two and visibly off-color. Labels are flat and drawn
// panel-then-glyphs, so plain back-to-front alpha blending is exact here.
@fragment
fn fs_label(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    // The shape comes from the atlas and the color from the instance.
    let alpha = clamp(label_coverage(in) * in.color.a, 0.0, 1.0);
    if (alpha <= LABEL_MIN_ALPHA) {
        discard;
    }
    return vec4<f32>(max(in.color.rgb, vec3<f32>(0.0)), alpha);
}

@fragment
fn fs_label_depth(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    if (label_distance_from_edge(in) < 0.0) {
        discard;
    }
    return vec4<f32>(0.0);
}

// A LabelImage, unlit like label text. The color comes from the picture and the opacity from the instance.
@fragment
fn fs_label_image(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(basecolor_texture, material_sampler, in.uv);
    let alpha = clamp(texel.a * in.color.a, 0.0, 1.0);
    if (alpha <= LABEL_MIN_ALPHA) {
        discard;
    }
    return vec4<f32>(texel.rgb, alpha);
}

// The same as fs_label_depth, for the parts of a LabelImage's picture that are fully opaque.
@fragment
fn fs_label_image_depth(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    if (textureSample(basecolor_texture, material_sampler, in.uv).a < 1.0) {
        discard;
    }
    return vec4<f32>(0.0);
}
