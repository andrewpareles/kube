// This is the sky every world starts with. It paints a three stop vertical gradient over the sky pixels,
// so a world with no skybox texture still gets a real sky instead of one flat color. An equirectangular
// skybox, if the world has one, draws on top of this afterwards and can let it show through where the
// texture is transparent.

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

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc_position: vec2<f32>,
};

// TODO: AI-PICKED-VALUE: Below one, this pulls the blend toward the horizon color, which keeps the pale band
// near the horizon tight instead of washing out the whole upper half of the sky.
const SKY_GRADIENT_FALLOFF: f32 = 0.45;

// The sky goes through the same tone curve and dithering as every lit surface in scene.wgsl, so the ground
// and the sky meet at the horizon without a seam. Keep these functions identical to the copies there.
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

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.ndc_position = out.position.xy;
    return out;
}

// The camera projection is reverse Z with an infinite far plane, so ndc z 1 is the near plane and
// ndc z 0 is infinitely far away. Unprojecting at 0 gives a point at infinity, whose w is zero, and
// dividing by that w produces infinities that poison the direction. Unproject at the near plane
// instead, which is the same thing build_ray_from_view_proj_ndc does on the Rust side.
fn world_direction_for_ndc(ndc_position: vec2<f32>) -> vec3<f32> {
    let world_position_h = camera.inverse_view_proj * vec4<f32>(ndc_position, 1.0, 1.0);
    let world_position = world_position_h.xyz / world_position_h.w;
    return world_position - camera.camera_position.xyz;
}

// Shiny surfaces reflect this same sky. Keep this function identical to the copy in scene.wgsl.
fn sky_gradient_color(direction: vec3<f32>) -> vec3<f32> {
    // Looking up blends the horizon color toward the top color, and looking down blends it toward the
    // bottom color. Both halves start from the horizon color, which is what puts the bright band on the eye line.
    let height = clamp(direction.y, -1.0, 1.0);
    let blend = pow(abs(height), SKY_GRADIENT_FALLOFF);
    let far_color = select(lighting.sky_bottom_color_and_padding.rgb, lighting.sky_top_color_and_padding.rgb, height >= 0.0);
    return mix(lighting.sky_horizon_color_and_padding.rgb, far_color, blend);
}

@fragment
fn fs_main(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let direction = normalize(world_direction_for_ndc(in.ndc_position));
    return vec4<f32>(convert_light_to_display_color(sky_gradient_color(direction), in.position.xy), 1.0);
}
