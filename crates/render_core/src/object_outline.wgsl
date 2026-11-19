// Object outlines, drawn around the outside of each outlined object's silhouette on screen.
//
// renderer.rs first draws every outlined object of a layer into outline_mask, which holds, for each pixel, the nearest
// outlined object there: its outline number plus one (0 where there is none), its depth, and its distance from the camera.
// Then these full screen passes run:
// 1. fs_seed marks the pixels on the edge of each outlined object.
// 2. fs_jump_flood runs a few times with shrinking steps, so each pixel ends up knowing the nearest edge pixel it can show
//    an outline from.
// 3. fs_opaque_outline_depth and fs_opaque_outline draw the solid part of each outline into the layer's opaque depth and
//    color, before the layer's objects draw. An object's own edge then draws over the inside of its outline, which keeps
//    the seam between them smooth, and anything nearer hides the outline.
// 4. fs_transparent_outline adds the rest, the soft outer edge and outlines that are see-through, to the layer's
//    transparent (OIT) pass, so it blends with glass and other transparent things in any order.

@group(0) @binding(0) var outline_mask: texture_2d<f32>;
// Two texels per outline: (linear color, opacity), then (thickness in pixels, 1 if the object is transparent, 0, 0).
@group(0) @binding(1) var outline_settings: texture_2d<f32>;
// Each pixel's nearest edge pixel: (x, y, 1 if there is one, 0).
@group(0) @binding(2) var nearest_edge: texture_2d<f32>;

struct JumpFloodStep {
    step_pixels: i32,
};
@group(1) @binding(0) var<uniform> jump_flood_step: JumpFloodStep;

// Keep this synced with OUTLINE_SETTINGS_TEXTURE_WIDTH in renderer.rs.
const OUTLINE_SETTINGS_TEXTURE_WIDTH: u32 = 1024u;
// TODO: AI-PICKED-VALUE: The solid part of an outline sits this much deeper than its object's edge, as a fraction of the
// reversed depth (about 1% farther from the camera), so the object's own edge always draws over the outline's inside.
const OUTLINE_DEPTH_PUSH_BACK: f32 = 0.99;

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

// One pixel of outline_mask. outline_number_plus_one is 0 where no outlined object covers the pixel.
struct MaskTexel {
    outline_number_plus_one: f32,
    depth: f32, // reversed, so nearer is larger
    distance_to_camera: f32,
};

fn load_mask_texel(texel: vec2<i32>) -> MaskTexel {
    let mask_size = vec2<i32>(textureDimensions(outline_mask));
    if (any(texel < vec2<i32>(0)) || any(texel >= mask_size)) {
        return MaskTexel(0.0, 0.0, 0.0);
    }
    let value = textureLoad(outline_mask, texel, 0);
    return MaskTexel(value.r, value.g, value.b);
}

// An edge pixel's outline shows on a pixel when no outlined object covers that pixel, or when a different outlined
// object farther away covers it. An object never outlines itself, so a mesh's arm in front of its body gets no line.
fn can_show_outline_of(pixel: MaskTexel, edge: MaskTexel) -> bool {
    return pixel.outline_number_plus_one == 0.0 || (pixel.outline_number_plus_one != edge.outline_number_plus_one && pixel.depth < edge.depth);
}

// ---------------------------------- Finding each pixel's nearest edge ----------------------------------

// A pixel of an outlined object is on its edge when a pixel next to it could show its outline.
@fragment
fn fs_seed(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(frag_coord.xy);
    let mask_texel = load_mask_texel(texel);
    if (mask_texel.outline_number_plus_one == 0.0) {
        return vec4<f32>(0.0);
    }
    let neighbor_offsets = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var i = 0; i < 4; i++) {
        if (can_show_outline_of(load_mask_texel(texel + neighbor_offsets[i]), mask_texel)) {
            return vec4<f32>(vec2<f32>(texel), 1.0, 0.0);
        }
    }
    return vec4<f32>(0.0);
}

// Jump flooding: each pixel looks at the nearest edges its neighbors one step away found, and keeps the nearest one it can
// show an outline from. The steps shrink by half each pass, so a handful of passes reaches farther than any outline.
@fragment
fn fs_jump_flood(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(frag_coord.xy);
    let edge_size = vec2<i32>(textureDimensions(nearest_edge));
    let mask_texel = load_mask_texel(texel);
    var best = vec4<f32>(0.0);
    var best_distance_squared = 3.4e38;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let neighbor = texel + vec2<i32>(x, y) * jump_flood_step.step_pixels;
            if (any(neighbor < vec2<i32>(0)) || any(neighbor >= edge_size)) {
                continue;
            }
            let candidate = textureLoad(nearest_edge, neighbor, 0);
            if (candidate.z == 0.0) {
                continue;
            }
            let offset = candidate.xy - vec2<f32>(texel);
            let distance_squared = dot(offset, offset);
            // A pixel inside an object keeps its own edges too, which lets its neighbors find them through it.
            let is_own_edge = all(candidate.xy == vec2<f32>(texel));
            if (distance_squared < best_distance_squared && (is_own_edge || can_show_outline_of(mask_texel, load_mask_texel(vec2<i32>(candidate.xy))))) {
                best = candidate;
                best_distance_squared = distance_squared;
            }
        }
    }
    return best;
}

// ---------------------------------- Drawing the outline ----------------------------------

struct OutlinePixel {
    color: vec3<f32>,
    alpha: f32,
    depth: f32,
    distance_to_camera: f32,
    is_object_transparent: bool,
};

// How much outline covers this pixel. The edge pixel's center is about half a pixel inside the silhouette, so a pixel whose
// center is the thickness plus one pixel away from it is fully past the outline, and the last pixel fades for smooth edges.
fn outline_pixel_at(frag_coord: vec4<f32>) -> OutlinePixel {
    let texel = vec2<i32>(frag_coord.xy);
    let edge = textureLoad(nearest_edge, texel, 0);
    if (edge.z == 0.0) {
        return OutlinePixel(vec3<f32>(0.0), 0.0, 0.0, 0.0, false);
    }
    // An edge pixel is only partly covered by its object, so the outline fills it too, under the object, leaving no gap
    // between the object's soft edge and the outline.
    let is_edge_pixel = all(edge.xy == vec2<f32>(texel));
    let edge_mask_texel = load_mask_texel(vec2<i32>(edge.xy));
    if (!is_edge_pixel && !can_show_outline_of(load_mask_texel(texel), edge_mask_texel)) {
        return OutlinePixel(vec3<f32>(0.0), 0.0, 0.0, 0.0, false);
    }
    let first_settings_texel = (u32(edge_mask_texel.outline_number_plus_one) - 1u) * 2u;
    let color_and_opacity = textureLoad(outline_settings, vec2<u32>(first_settings_texel % OUTLINE_SETTINGS_TEXTURE_WIDTH, first_settings_texel / OUTLINE_SETTINGS_TEXTURE_WIDTH), 0);
    let second_settings_texel = first_settings_texel + 1u;
    let thickness_and_transparency = textureLoad(outline_settings, vec2<u32>(second_settings_texel % OUTLINE_SETTINGS_TEXTURE_WIDTH, second_settings_texel / OUTLINE_SETTINGS_TEXTURE_WIDTH), 0);
    let distance_to_edge = distance(edge.xy, vec2<f32>(texel));
    let coverage = clamp(thickness_and_transparency.x + 1.0 - distance_to_edge, 0.0, 1.0);
    return OutlinePixel(
        color_and_opacity.rgb,
        color_and_opacity.a * coverage,
        edge_mask_texel.depth,
        edge_mask_texel.distance_to_camera,
        thickness_and_transparency.y != 0.0,
    );
}

// The solid part of an opaque object's outline is drawn as if it were opaque scenery.
fn is_solid_outline(outline_pixel: OutlinePixel) -> bool {
    return outline_pixel.alpha >= 1.0 && !outline_pixel.is_object_transparent;
}

@fragment
fn fs_opaque_outline_depth(@builtin(position) frag_coord: vec4<f32>) -> @builtin(frag_depth) f32 {
    let outline_pixel = outline_pixel_at(frag_coord);
    if (!is_solid_outline(outline_pixel)) {
        discard;
    }
    return outline_pixel.depth * OUTLINE_DEPTH_PUSH_BACK;
}

struct OpaqueOutlineOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_opaque_outline(@builtin(position) frag_coord: vec4<f32>) -> OpaqueOutlineOutput {
    let outline_pixel = outline_pixel_at(frag_coord);
    if (!is_solid_outline(outline_pixel)) {
        discard;
    }
    return OpaqueOutlineOutput(vec4<f32>(outline_pixel.color, 1.0), outline_pixel.depth * OUTLINE_DEPTH_PUSH_BACK);
}

// Keep these synced with OitFragmentOutput, OIT_MAX_ACCUM_PER_FRAGMENT and compute_oit_weight in scene.wgsl, so outlines
// weigh the same as every other transparent surface.
struct OitFragmentOutput {
    @location(0) accum: vec4<f32>,
    @location(1) weight: f32,
    @builtin(frag_depth) depth: f32,
};

const OIT_MAX_ACCUM_PER_FRAGMENT: f32 = 1024.0;

fn compute_oit_weight(color: vec3<f32>, alpha: f32, distance_to_camera: f32) -> f32 {
    let color_max = max(max(color.r, color.g), color.b);
    let coverage = max(min(1.0, color_max * alpha), alpha);
    let depth_weight = clamp(0.03 / (1e-5 + pow(distance_to_camera / 200.0, 4.0)), 1e-2, 3e3);
    let brightness = max(color_max, 1.0);
    return coverage * min(depth_weight, OIT_MAX_ACCUM_PER_FRAGMENT / brightness);
}

@fragment
fn fs_transparent_outline(@builtin(position) frag_coord: vec4<f32>) -> OitFragmentOutput {
    let outline_pixel = outline_pixel_at(frag_coord);
    if (outline_pixel.alpha <= 0.0 || is_solid_outline(outline_pixel)) {
        discard;
    }
    let weight = compute_oit_weight(outline_pixel.color, outline_pixel.alpha, outline_pixel.distance_to_camera);
    var out: OitFragmentOutput;
    out.accum = vec4<f32>(outline_pixel.color * outline_pixel.alpha * weight, outline_pixel.alpha);
    out.weight = outline_pixel.alpha * weight;
    out.depth = outline_pixel.depth;
    return out;
}
