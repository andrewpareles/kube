struct CameraMatrixBufferContent {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
};

struct SkyboxBufferContent {
    settings: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraMatrixBufferContent;

@group(1) @binding(0)
var skybox_texture: texture_2d<f32>;

@group(1) @binding(1)
var skybox_sampler: sampler;

@group(1) @binding(2)
var<uniform> skybox: SkyboxBufferContent;

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc_position: vec2<f32>,
};

const PI: f32 = 3.14159265359;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.ndc_position = out.position.xy;
    return out;
}

fn world_direction_for_ndc(ndc_position: vec2<f32>) -> vec3<f32> {
    let world_position_h = camera.inverse_view_proj * vec4<f32>(ndc_position, 1.0, 1.0);
    let world_position = world_position_h.xyz / world_position_h.w;
    return world_position - camera.camera_position.xyz;
}

// Shiny surfaces reflect the skybox too. Keep this function identical to the copy in scene.wgsl.
fn equirectangular_uv(direction: vec3<f32>) -> vec2<f32> {
    let clamped_y = clamp(direction.y, -1.0, 1.0);
    let u = atan2(direction.x, direction.z) / (2.0 * PI) + 0.5;
    let v = 0.5 - asin(clamped_y) / PI;
    return vec2<f32>(u, v);
}

fn wrapped_longitude_gradient(du: f32) -> f32 {
    return du - floor(du + 0.5);
}

@fragment
fn fs_main(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let direction = normalize(world_direction_for_ndc(in.ndc_position));
    let uv = equirectangular_uv(direction);
    let uv_dx = dpdx(uv);
    let uv_dy = dpdy(uv);
    let texture_color = textureSampleGrad(
        skybox_texture,
        skybox_sampler,
        uv,
        vec2<f32>(wrapped_longitude_gradient(uv_dx.x), uv_dx.y),
        vec2<f32>(wrapped_longitude_gradient(uv_dy.x), uv_dy.y),
    );
    let skybox_alpha = texture_color.a * clamp(skybox.settings.x, 0.0, 1.0);
    return vec4<f32>(texture_color.rgb, skybox_alpha);
}
