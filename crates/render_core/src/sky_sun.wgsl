struct CameraMatrixBufferContent {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraMatrixBufferContent;

struct LightingBufferContent {
    sun_direction: vec4<f32>,
    sun_color_and_intensity: vec4<f32>,
    
    ambient_top_color_and_intensity: vec4<f32>,
    ambient_bottom_color_and_padding: vec4<f32>,

    sky_top_color_and_padding: vec4<f32>,
    sky_horizon_color_and_padding: vec4<f32>,
    sky_bottom_color_and_padding: vec4<f32>,

    ambient_occlusion_settings: vec4<f32>,
    sky_reflection_settings: vec4<f32>,
    display_settings: vec4<f32>,
};

@group(1) @binding(0)
var<uniform> lighting: LightingBufferContent;

// The scene's depth, for the glow pass, which draws at half the picture's size and so has to test it by hand. Keep the
// binding synced with GLOW_SCENE_DEPTH_BINDING in renderer.rs.
@group(2) @binding(8)
var scene_depth_texture: texture_2d<f32>;

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc_position: vec2<f32>,
};

const EPSILON: f32 = 1e-4;
// TODO: AI-PICKED-VALUE: This puts the sky sun safely inside the camera far plane while keeping the screen position direction-based.
const SUN_SKY_DISTANCE: f32 = 512.0;
// TODO: AI-PICKED-VALUE: This makes the sun large enough to see in editor view without dominating the whole sky.
const SUN_CORE_RADIUS_NDC: f32 = 0.075;
// TODO: AI-PICKED-VALUE: The sun hands bloom a soft disk about twice its own size, which bloom spreads into a glow about as
// wide as the ring the sky used to paint around it.
const SUN_GLOW_RADIUS_NDC: f32 = 0.15;
const SUN_CORE_COLOR: vec3<f32> = vec3<f32>(1.0, 0.96, 0.82);

fn normalize_or_fallback(value: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    if (length(value) <= EPSILON) {
        return fallback;
    }
    return normalize(value);
}

// This estimates the camera aspect ratio from the view-projection rows so the sun stays round on wide viewports.
fn camera_aspect() -> f32 {
    let clip_x_scale = length(vec3<f32>(camera.view_proj[0].x, camera.view_proj[1].x, camera.view_proj[2].x));
    let clip_y_scale = length(vec3<f32>(camera.view_proj[0].y, camera.view_proj[1].y, camera.view_proj[2].y));
    return clip_y_scale / max(clip_x_scale, EPSILON);
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.ndc_position = out.position.xy;
    return out;
}

// How far this pixel is from the middle of the sun on screen, in units of half the picture's height. The sun behind the
// camera is infinitely far away.
fn screen_distance_to_sun(ndc_position: vec2<f32>) -> f32 {
    let direction_to_sun = normalize_or_fallback(lighting.sun_direction.xyz, vec3<f32>(0.0, 1.0, 0.0));
    let sun_world_position = camera.camera_position.xyz + direction_to_sun * SUN_SKY_DISTANCE;
    let sun_clip_position = camera.view_proj * vec4<f32>(sun_world_position, 1.0);
    if (sun_clip_position.w <= EPSILON) {
        return 1e9;
    }
    let sun_ndc_position = sun_clip_position.xy / sun_clip_position.w;
    return length(vec2<f32>((ndc_position.x - sun_ndc_position.x) * camera_aspect(), ndc_position.y - sun_ndc_position.y));
}

// The sun's disk. Its glow comes from bloom, which fs_glow feeds.
@fragment
fn fs_main(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let sun_core_alpha = 1.0 - smoothstep(SUN_CORE_RADIUS_NDC * 0.7, SUN_CORE_RADIUS_NDC, screen_distance_to_sun(in.ndc_position));
    let sun_color = max(lighting.sun_color_and_intensity.rgb, vec3<f32>(0.0));
    let sun_display_color = mix(sun_color * 0.8, SUN_CORE_COLOR, sun_core_alpha);
    return vec4<f32>(sun_display_color, sun_core_alpha * clamp(lighting.sun_color_and_intensity.a, 0.0, 1.0));
}

// The sun's glow, drawn into bloom level 0 for bloom to spread. Only the part of the sun that shows through the sky glows,
// and bloom spills that glow over whatever hides the rest, the way the sun peeks around a roof.
@fragment
fn fs_glow(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let sun_glow = 1.0 - smoothstep(0.0, SUN_GLOW_RADIUS_NDC, screen_distance_to_sun(in.ndc_position));
    if (sun_glow <= 0.0) {
        return vec4<f32>(0.0);
    }
    // Each pixel of the glow pass covers four of the scene's depth texels, and the sky shows in the ones nothing wrote.
    let last_texel = vec2<i32>(textureDimensions(scene_depth_texture)) - 1;
    let first_texel = vec2<i32>(floor(in.position.xy)) * 2;
    var sky_texel_count = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            if (textureLoad(scene_depth_texture, min(first_texel + vec2<i32>(x, y), last_texel), 0).r <= 0.0) {
                sky_texel_count += 1.0;
            }
        }
    }
    let visible_sun = clamp(lighting.sun_color_and_intensity.a, 0.0, 1.0) * sky_texel_count / 4.0;
    let sun_glow_color = mix(max(lighting.sun_color_and_intensity.rgb, vec3<f32>(0.0)), SUN_CORE_COLOR, 0.5);
    return vec4<f32>(sun_glow_color * sun_glow * visible_sun, 1.0);
}
