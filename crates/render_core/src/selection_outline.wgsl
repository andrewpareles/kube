@group(0) @binding(0) var selection_mask: texture_2d<f32>;

struct VertexShaderOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexShaderOutput {
    var out: VertexShaderOutput;
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

// TODO: AI-PICKED-VALUE: This uses a 2-pixel blue editor border so selection stays readable without becoming a saved object property.
const SELECTION_OUTLINE_RADIUS_PIXELS: i32 = 2;
const SELECTION_OUTLINE_COLOR: vec3<f32> = vec3<f32>(0.06, 0.28, 0.85);
const SELECTION_OUTLINE_ALPHA: f32 = 0.85;

fn load_selection_mask(texel: vec2<i32>, texture_size: vec2<i32>) -> f32 {
    if (texel.x < 0 || texel.y < 0 || texel.x >= texture_size.x || texel.y >= texture_size.y) {
        return 0.0;
    }
    return textureLoad(selection_mask, texel, 0).r;
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(i32(frag_coord.x), i32(frag_coord.y));
    let texture_size = vec2<i32>(textureDimensions(selection_mask));
    let center_mask = load_selection_mask(texel, texture_size);
    let is_center_selected = center_mask > 0.5;

    for (var y_offset: i32 = -SELECTION_OUTLINE_RADIUS_PIXELS; y_offset <= SELECTION_OUTLINE_RADIUS_PIXELS; y_offset += 1) {
        for (var x_offset: i32 = -SELECTION_OUTLINE_RADIUS_PIXELS; x_offset <= SELECTION_OUTLINE_RADIUS_PIXELS; x_offset += 1) {
            let distance_squared = x_offset * x_offset + y_offset * y_offset;
            if (distance_squared == 0 || distance_squared > SELECTION_OUTLINE_RADIUS_PIXELS * SELECTION_OUTLINE_RADIUS_PIXELS) {
                continue;
            }

            let is_neighbor_selected = load_selection_mask(texel + vec2<i32>(x_offset, y_offset), texture_size) > 0.5;
            if ((is_center_selected && !is_neighbor_selected) || (!is_center_selected && is_neighbor_selected)) {
                return vec4<f32>(SELECTION_OUTLINE_COLOR, SELECTION_OUTLINE_ALPHA);
            }
        }
    }

    return vec4<f32>(0.0);
}
