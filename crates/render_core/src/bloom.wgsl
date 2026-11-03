// Bloom spreads the glow of glowing objects onto their surroundings. The glow pass draws only glowing objects, at half the
// picture's size, into the first level. This shrinks that level to half size again and again, then grows it back one step
// at a time, adding each step onto the one above it, so the glow ends up blurred both a little and a lot at once. The base
// layer's composite in oit_composite.wgsl grows the first level once more as it adds it to the picture. The shrink and grow
// filters are the dual filtering kernels from Marius Bjørge, "Bandwidth-Efficient Rendering" (SIGGRAPH 2015), which blur
// widely from very few samples.

@group(0) @binding(0)
var source_texture: texture_2d<f32>;
@group(0) @binding(1)
var source_sampler: sampler;

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// Half size: the texel under this one weighs four times, and the four diagonal half-texels around it once each.
@fragment
fn fs_downsample(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    let half_texel = 0.5 / vec2<f32>(textureDimensions(source_texture));
    let diagonal = vec2<f32>(half_texel.x, -half_texel.y);
    var sum = textureSample(source_texture, source_sampler, in.uv).rgb * 4.0;
    sum += textureSample(source_texture, source_sampler, in.uv - half_texel).rgb;
    sum += textureSample(source_texture, source_sampler, in.uv + half_texel).rgb;
    sum += textureSample(source_texture, source_sampler, in.uv - diagonal).rgb;
    sum += textureSample(source_texture, source_sampler, in.uv + diagonal).rgb;
    return vec4<f32>(sum / 8.0, 1.0);
}

// Double size: eight samples on a diamond one level texel out, the diagonal ones weighing twice. Keep this function
// identical to the copy in oit_composite.wgsl.
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

@fragment
fn fs_upsample(in: VertexShaderOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(sample_grown(source_texture, source_sampler, in.uv), 1.0);
}
