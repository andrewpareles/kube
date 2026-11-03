@group(0) @binding(0) var scene_color: texture_2d<f32>;
@group(0) @binding(1) var oit_accum: texture_2d<f32>;
@group(0) @binding(2) var oit_weight: texture_2d<f32>;
// Only the base layer's composite reads these: bloom level 0, holding the finished glow at half the picture's size.
@group(1) @binding(0) var bloom_texture: texture_2d<f32>;
@group(1) @binding(1) var bloom_sampler: sampler;

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
};

// The largest value Rgba16Float can hold, which is what the accumulation buffer saturates to.
const OIT_ACCUM_SATURATION: f32 = 65504.0;
// TODO: AI-PICKED-VALUE: How bright the finished glow is. Growing the bloom back adds every level onto the next, so the
// glow next to an object sums about half of its brightness from each of the six levels; this scales that back down.
const BLOOM_STRENGTH: f32 = 0.2;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

// One layer's opaque color with its transparent layers blended over it.
fn composite_layer(frag_coord: vec4<f32>) -> vec4<f32> {
    let texel = vec2<i32>(i32(frag_coord.x), i32(frag_coord.y));

    let scene = textureLoad(scene_color, texel, 0);
    // scene.wgsl bounds what one fragment can contribute, but the pass accumulates additively and a
    // deep enough stack of transparency can still saturate Rgba16Float. Pulling infinity back to a
    // finite value here keeps the divide below from returning NaN: a blown out pixel reads as bright,
    // where a NaN one reads as a hole.
    let accum = min(textureLoad(oit_accum, texel, 0), vec4<f32>(OIT_ACCUM_SATURATION));
    let weight = min(textureLoad(oit_weight, texel, 0).r, OIT_ACCUM_SATURATION);
    let reveal = clamp(accum.a, 0.0, 1.0);

    let trans_color = select(vec3<f32>(0.0, 0.0, 0.0), accum.rgb / weight, weight > 0.00001);
    let scene_alpha = clamp(scene.a, 0.0, 1.0);
    let trans_alpha = 1.0 - reveal;
    let out_alpha = scene_alpha + (1.0 - scene_alpha) * trans_alpha;
    let out_color_premultiplied = scene.rgb * reveal + trans_color * trans_alpha;
    let out_color = select(vec3<f32>(0.0, 0.0, 0.0), out_color_premultiplied / out_alpha, out_alpha > 0.00001);

    return vec4<f32>(out_color, out_alpha);
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    return composite_layer(frag_coord);
}

fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(color, vec3<f32>(1.0 / 2.4)) - 0.055, color * 12.92, color <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return select(pow((color + 0.055) / 1.055, vec3<f32>(2.4)), color / 12.92, color <= vec3<f32>(0.04045));
}

// Without a sky the picture is see-through, and the page blends it over whatever is behind it, premultiplied in sRGB. Blending into the
// sRGB target premultiplies in linear light instead, which leaves partly covered edge pixels brighter than they are covered: a light
// outline around everything. This returns the color that, blended over a see-through pixel, stores its sRGB color times its alpha. Fully
// covered pixels, which is every pixel under a sky, come back unchanged.
fn premultiplied_in_srgb(color: vec4<f32>) -> vec4<f32> {
    if (color.a <= 0.0 || color.a >= 1.0) {
        return color;
    }
    let srgb = linear_to_srgb(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    return vec4<f32>(srgb_to_linear(srgb * color.a) / color.a, color.a);
}

@fragment
fn fs_main_see_through(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    return premultiplied_in_srgb(composite_layer(frag_coord));
}

// Double size: eight samples on a diamond one level texel out, the diagonal ones weighing twice. Keep this function
// identical to the copy in bloom.wgsl.
fn sample_grown(level_texture: texture_2d<f32>, level_sampler: sampler, uv: vec2<f32>) -> vec3<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(level_texture));
    let half_texel = texel * 0.5;
    var sum = textureSample(level_texture, level_sampler, uv + vec2<f32>(-texel.x, 0.0)).rgb;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(texel.x, 0.0)).rgb;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(0.0, -texel.y)).rgb;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(0.0, texel.y)).rgb;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(-half_texel.x, -half_texel.y)).rgb * 2.0;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(half_texel.x, -half_texel.y)).rgb * 2.0;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(-half_texel.x, half_texel.y)).rgb * 2.0;
    sum += textureSample(level_texture, level_sampler, uv + vec2<f32>(half_texel.x, half_texel.y)).rgb * 2.0;
    return sum / 12.0;
}

// The base layer's composite, which also adds the finished glow. The base layer covers the whole picture, so adding to its
// color adds on top of everything under it. Bloom level 0 is half the picture's size, so each picture pixel reads it at
// half its own coordinates.
@fragment
fn fs_main_with_bloom(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let layer_color = composite_layer(frag_coord);
    let bloom_uv = frag_coord.xy / (2.0 * vec2<f32>(textureDimensions(bloom_texture)));
    return premultiplied_in_srgb(vec4<f32>(layer_color.rgb + sample_grown(bloom_texture, bloom_sampler, bloom_uv) * BLOOM_STRENGTH, layer_color.a));
}
