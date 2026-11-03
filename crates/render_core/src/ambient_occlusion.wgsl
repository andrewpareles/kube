// Ambient occlusion darkens creases, corners, and the ground right next to objects, which is what makes things look
// like they sit on the ground instead of floating over it. It only needs the opaque depth, so it runs before the
// opaque pass, and scene.wgsl reads the blurred result.
// This is ground truth ambient occlusion as XeGTAO does it (https://github.com/GameTechDev/XeGTAO, MIT license,
// Copyright (C) 2016-2021, Intel Corporation). XeGTAO's noise and 3x3 denoiser expect frames to be averaged over time,
// which this renderer does not do, so every 4x4 block of pixels here shares one set of 16 sample directions and a 7x7
// tent blur, which is two 4x4 box blurs in a row, averages each block back out.

struct CameraMatrixBufferContent {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    view_clip_w_row: vec4<f32>,
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
    // x: intensity, y: radius, z: how much it also darkens sunlight.
    ambient_occlusion_settings: vec4<f32>,
    // x: how strongly shiny surfaces reflect the sky.
    sky_reflection_settings: vec4<f32>,
    // x: vibrance, how much further from gray dull colors are pushed.
    display_settings: vec4<f32>,
};

@group(1) @binding(0)
var scene_depth: texture_2d<f32>;
@group(1) @binding(1)
var ambient_occlusion_to_blur: texture_2d<f32>;
@group(1) @binding(2)
var<uniform> lighting: LightingBufferContent;
@group(1) @binding(3)
var view_depth_finer_mip: texture_2d<f32>;
@group(1) @binding(4)
var view_depth_mips: texture_2d<f32>;

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc_position: vec2<f32>,
};

const PI: f32 = 3.14159265359;
const HALF_PI: f32 = 1.57079632679;
// Keep this synced with renderer.rs AMBIENT_OCCLUSION_VIEW_DEPTH_MIP_COUNT.
const VIEW_DEPTH_MIP_COUNT: i32 = 5;
// Keep this synced with renderer.rs AMBIENT_OCCLUSION_SKY_VIEW_DEPTH. Sky has no depth, so it is stored this far away,
// where it can never occlude anything.
const SKY_VIEW_DEPTH: f32 = 1.0e8;
// TODO: AI-PICKED-VALUE: XeGTAO's high quality setting, 3 directions per pixel with 3 samples each way along each.
const SLICE_COUNT: i32 = 3;
const STEPS_PER_SLICE: i32 = 3;
// XeGTAO's defaults.
const FALLOFF_RANGE: f32 = 0.615;
const SAMPLE_DISTRIBUTION_POWER: f32 = 2.0;
const FINAL_VALUE_POWER: f32 = 2.2;
const DEPTH_MIP_SAMPLING_OFFSET: f32 = 3.3;
const PIXEL_TOO_CLOSE_THRESHOLD: f32 = 1.3;
// Before the blur averages it, a pixel's visibility can come out above 1, so it is stored scaled down.
const OCCLUSION_TERM_SCALE: f32 = 1.5;
// TODO: AI-PICKED-VALUE: XeGTAO's edge threshold, per pixel of distance. The blur weights a pixel down once its depth is
// this fraction of the center's depth off both the center's depth and the center's surface extended out to it.
const BLUR_EDGE_THRESHOLD: f32 = 0.011;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.ndc_position = out.position.xy;
    return out;
}

// Reverse Z with an infinite far plane stores depth as near / distance, and view_proj[3].z is near.
@fragment
fn fs_view_depth(in: VertexShaderOutput) -> @location(0) f32 {
    let depth = textureLoad(scene_depth, vec2<i32>(in.position.xy), 0).r;
    if (depth <= 0.0) {
        return SKY_VIEW_DEPTH;
    }
    return camera.view_proj[3].z / depth;
}

// Each coarser level averages 2x2 depths, dropping ones well in front of the farthest, so a thin object does not turn
// into a thick occluder once it is averaged.
@fragment
fn fs_downsample_view_depth(in: VertexShaderOutput) -> @location(0) f32 {
    let finer_pixel = vec2<i32>(in.position.xy) * 2;
    let last_finer_pixel = vec2<i32>(textureDimensions(view_depth_finer_mip)) - 1;
    let depths = vec4<f32>(
        textureLoad(view_depth_finer_mip, min(finer_pixel, last_finer_pixel), 0).r,
        textureLoad(view_depth_finer_mip, min(finer_pixel + vec2<i32>(1, 0), last_finer_pixel), 0).r,
        textureLoad(view_depth_finer_mip, min(finer_pixel + vec2<i32>(0, 1), last_finer_pixel), 0).r,
        textureLoad(view_depth_finer_mip, min(finer_pixel + vec2<i32>(1, 1), last_finer_pixel), 0).r,
    );
    let max_depth = max(max(depths.x, depths.y), max(depths.z, depths.w));
    let effect_radius = 0.75 * max(lighting.ambient_occlusion_settings.y, 1e-4);
    let falloff_range = FALLOFF_RANGE * effect_radius;
    let falloff_from = effect_radius * (1.0 - FALLOFF_RANGE);
    let weights = saturate((vec4<f32>(max_depth) - depths) * (-1.0 / falloff_range) + (falloff_from / falloff_range + 1.0));
    return dot(weights, depths) / dot(weights, vec4<f32>(1.0));
}

// View space here has x right, y up, and z the distance straight ahead.
fn view_position_at(ndc_position: vec2<f32>, depth_ahead: f32, ndc_to_view: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(ndc_position * ndc_to_view * depth_ahead, depth_ahead);
}

// A 4x4 ordered pattern holding each value 0..15 once, so every pixel in a 4x4 block starts its directions at a
// different angle and the 4x4 blur sees all of them.
fn ordered_pattern_4x4(pixel: vec2<i32>) -> f32 {
    let x = u32(pixel.x);
    let y = u32(pixel.y);
    let fine = ((x ^ y) & 1u) * 2u + (y & 1u);
    let coarse = (((x >> 1u) ^ (y >> 1u)) & 1u) * 2u + ((y >> 1u) & 1u);
    return f32(fine * 4u + coarse);
}

// Along each direction, every sample that rises above the surface raises that side's horizon, and the sky a pixel can
// see is what is left between its two horizons. Samples fade out toward the radius, and samples outside the viewport
// are skipped, so a crease near the edge of the screen keeps the horizons its visible samples found.
@fragment
fn fs_ambient_occlusion(in: VertexShaderOutput) -> @location(0) f32 {
    // Derivatives have to run before anything branches.
    let ndc_per_pixel = vec2<f32>(dpdx(in.ndc_position.x), dpdy(in.ndc_position.y));
    let intensity = lighting.ambient_occlusion_settings.x;
    let effect_radius = lighting.ambient_occlusion_settings.y;
    let pixel = vec2<i32>(in.position.xy);
    let last_pixel = vec2<i32>(textureDimensions(view_depth_mips)) - 1;
    let center_depth = textureLoad(view_depth_mips, pixel, 0).r;
    if (center_depth >= SKY_VIEW_DEPTH || intensity <= 0.0 || effect_radius <= 0.0) {
        return 1.0 / OCCLUSION_TERM_SCALE;
    }
    let ndc_to_view = 1.0 / vec2<f32>(
        length(vec3<f32>(camera.view_proj[0].x, camera.view_proj[1].x, camera.view_proj[2].x)),
        length(vec3<f32>(camera.view_proj[0].y, camera.view_proj[1].y, camera.view_proj[2].y)),
    );

    // The surface normal comes from the neighbor on each axis that is nearer in depth, so a pixel on an object's
    // edge takes its normal from its own surface and not from whatever is behind it.
    let depth_right = textureLoad(view_depth_mips, min(pixel + vec2<i32>(1, 0), last_pixel), 0).r;
    let depth_left = textureLoad(view_depth_mips, max(pixel - vec2<i32>(1, 0), vec2<i32>(0)), 0).r;
    let depth_down = textureLoad(view_depth_mips, min(pixel + vec2<i32>(0, 1), last_pixel), 0).r;
    let depth_up = textureLoad(view_depth_mips, max(pixel - vec2<i32>(0, 1), vec2<i32>(0)), 0).r;
    let step_x = select(-1.0, 1.0, abs(depth_right - center_depth) < abs(depth_left - center_depth));
    let step_y = select(-1.0, 1.0, abs(depth_down - center_depth) < abs(depth_up - center_depth));
    let surface_position = view_position_at(in.ndc_position, center_depth, ndc_to_view);
    let position_x = view_position_at(in.ndc_position + vec2<f32>(step_x * ndc_per_pixel.x, 0.0), select(depth_left, depth_right, step_x > 0.0), ndc_to_view);
    let position_y = view_position_at(in.ndc_position + vec2<f32>(0.0, step_y * ndc_per_pixel.y), select(depth_up, depth_down, step_y > 0.0), ndc_to_view);
    var normal = normalize(cross(position_x - surface_position, position_y - surface_position));
    if (dot(normal, surface_position) > 0.0) {
        normal = -normal;
    }

    // Nudged toward the camera so depth precision cannot make a flat surface occlude itself.
    let center_position = view_position_at(in.ndc_position, center_depth * 0.99999, ndc_to_view);
    let view_direction = normalize(-center_position);
    let falloff_range = FALLOFF_RANGE * effect_radius;
    let falloff_from = effect_radius * (1.0 - FALLOFF_RANGE);
    let falloff_mul = -1.0 / falloff_range;
    let falloff_add = falloff_from / falloff_range + 1.0;
    let pattern_index = ordered_pattern_4x4(pixel);
    let noise_slice = (pattern_index + 0.5) / 16.0;
    let noise_sample = fract(0.5 + pattern_index * 0.6180339887);
    let screen_radius = effect_radius / (center_depth * ndc_per_pixel.x * ndc_to_view.x);
    let min_step = PIXEL_TOO_CLOSE_THRESHOLD / screen_radius;
    // WebGL2 cannot ask a texture how many mips it has, so this works it out the way renderer.rs creates the texture.
    let view_depth_size = textureDimensions(view_depth_mips);
    let last_mip = min(f32(VIEW_DEPTH_MIP_COUNT - 1), floor(log2(f32(max(view_depth_size.x, view_depth_size.y)))));
    // XeGTAO spreads its directions evenly across the screen, which only spreads them evenly around the line of sight
    // at the center of the screen and overdarkens slanted surfaces toward the edges. These are spread evenly around the
    // line of sight, and each one's sample line on screen is where that direction projects.
    let around_view_x = normalize(vec3<f32>(1.0, 0.0, 0.0) - view_direction.x * view_direction);
    let around_view_y = cross(around_view_x, view_direction);

    var visibility = saturate((10.0 - screen_radius) / 100.0) * 0.5;
    for (var slice = 0; slice < SLICE_COUNT; slice = slice + 1) {
        let phi = (f32(slice) + noise_slice) / f32(SLICE_COUNT) * PI;
        let ortho_direction = cos(phi) * around_view_x + sin(phi) * around_view_y;
        let omega = normalize(
            (ortho_direction.xy * center_position.z - center_position.xy * ortho_direction.z) / (ndc_to_view * ndc_per_pixel),
        ) * screen_radius;
        let axis = normalize(cross(ortho_direction, view_direction));
        let projected_normal = normal - axis * dot(normal, axis);
        let projected_normal_length = max(length(projected_normal), 1e-4);
        let cos_normal = saturate(dot(projected_normal, view_direction) / projected_normal_length);
        let normal_angle = sign(dot(ortho_direction, projected_normal)) * acos(cos_normal);
        let low_horizon_cos = vec2<f32>(cos(normal_angle + HALF_PI), cos(normal_angle - HALF_PI));
        var horizon_cos = low_horizon_cos;
        for (var step = 0; step < STEPS_PER_SLICE; step = step + 1) {
            let step_noise = fract(noise_sample + f32(slice + step * STEPS_PER_SLICE) * 0.6180339887);
            let step_fraction = pow((f32(step) + step_noise) / f32(STEPS_PER_SLICE), SAMPLE_DISTRIBUTION_POWER) + min_step;
            let offset_pixels = step_fraction * omega;
            let mip = i32(round(clamp(log2(length(offset_pixels)) - DEPTH_MIP_SAMPLING_OFFSET, 0.0, last_mip)));
            let last_mip_pixel = vec2<i32>(textureDimensions(view_depth_mips, mip)) - 1;
            for (var side = 0; side < 2; side = side + 1) {
                let side_offset_pixels = round(offset_pixels) * select(-1.0, 1.0, side == 0);
                let sample_ndc = in.ndc_position + side_offset_pixels * ndc_per_pixel;
                if (any(abs(sample_ndc) > vec2<f32>(1.0))) {
                    continue;
                }
                let sample_pixel = min((pixel + vec2<i32>(side_offset_pixels)) / (1i << u32(mip)), last_mip_pixel);
                let sample_depth = textureLoad(view_depth_mips, sample_pixel, mip).r;
                let sample_delta = view_position_at(sample_ndc, sample_depth, ndc_to_view) - center_position;
                let sample_distance = max(length(sample_delta), 1e-6);
                let sample_horizon_cos = dot(sample_delta / sample_distance, view_direction);
                let weight = saturate(sample_distance * falloff_mul + falloff_add);
                horizon_cos[side] = max(horizon_cos[side], mix(low_horizon_cos[side], sample_horizon_cos, weight));
            }
        }
        let horizon_angles = vec2<f32>(acos(clamp(horizon_cos.x, -1.0, 1.0)), -acos(clamp(horizon_cos.y, -1.0, 1.0)));
        let arcs = (vec2<f32>(cos_normal) + 2.0 * horizon_angles * sin(normal_angle) - cos(2.0 * horizon_angles - normal_angle)) / 4.0;
        visibility += mix(projected_normal_length, 1.0, 0.05) * (arcs.x + arcs.y);
    }
    return max(visibility / f32(SLICE_COUNT), 0.0) / OCCLUSION_TERM_SCALE;
}

// A 7 pixel tent blur along one axis. A pixel counts less the further its depth is off both the center's depth and the
// center's surface extended out to it, so a slope or a crease still blurs but darkening does not bleed onto whatever is
// in front of it.
fn blur_visibility_along(in: VertexShaderOutput, direction: vec2<i32>) -> f32 {
    let pixel = vec2<i32>(in.position.xy);
    let last_pixel = vec2<i32>(textureDimensions(scene_depth)) - 1;
    let depth = textureLoad(scene_depth, pixel, 0).r;
    if (depth <= 0.0) {
        return 1.0 / OCCLUSION_TERM_SCALE;
    }
    let depth_after = textureLoad(scene_depth, clamp(pixel + direction, vec2<i32>(0), last_pixel), 0).r - depth;
    let depth_before = depth - textureLoad(scene_depth, clamp(pixel - direction, vec2<i32>(0), last_pixel), 0).r;
    let depth_slope = select(depth_before, depth_after, abs(depth_after) < abs(depth_before));
    var visibility_sum = 0.0;
    var weight_sum = 0.0;
    for (var step = -3; step <= 3; step = step + 1) {
        let sample_pixel = clamp(pixel + direction * step, vec2<i32>(0), last_pixel);
        let offset = f32(dot(sample_pixel - pixel, direction));
        // Reverse Z depth is linear across a flat surface on screen, so the surface extends as a straight line.
        let depth_difference = textureLoad(scene_depth, sample_pixel, 0).r - depth;
        let edge = min(abs(depth_difference), abs(depth_difference - depth_slope * offset)) / depth;
        let weight = saturate(1.25 - edge / (BLUR_EDGE_THRESHOLD * max(abs(offset), 1.0))) * f32(4 - abs(step));
        visibility_sum += textureLoad(ambient_occlusion_to_blur, sample_pixel, 0).r * weight;
        weight_sum += weight;
    }
    return visibility_sum / weight_sum;
}

@fragment
fn fs_blur_horizontal(in: VertexShaderOutput) -> @location(0) f32 {
    return blur_visibility_along(in, vec2<i32>(1, 0));
}

@fragment
fn fs_blur_vertical(in: VertexShaderOutput) -> @location(0) f32 {
    let visibility = min(blur_visibility_along(in, vec2<i32>(0, 1)) * OCCLUSION_TERM_SCALE, 1.0);
    return 1.0 - saturate(lighting.ambient_occlusion_settings.x) * (1.0 - max(pow(visibility, FINAL_VALUE_POWER), 0.03));
}
