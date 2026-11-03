// The sky gradient reads its three color stops out of the lighting uniform, which several shaders declare
// separately. If renderer.rs and any of those .wgsl declarations drift apart, every field after the drift
// reads the wrong bytes and the sky silently renders the wrong colors, so this checks they still match.

const SHADER_TYPES_SOURCE: &str = include_str!("../src/render_scene.rs");
const SCENE_SHADER_SOURCE: &str = include_str!("../src/scene.wgsl");
const SKY_SUN_SHADER_SOURCE: &str = include_str!("../src/sky_sun.wgsl");
const SKY_GRADIENT_SHADER_SOURCE: &str = include_str!("../src/sky_gradient.wgsl");
const AMBIENT_OCCLUSION_SHADER_SOURCE: &str = include_str!("../src/ambient_occlusion.wgsl");
const SKYBOX_SHADER_SOURCE: &str = include_str!("../src/skybox.wgsl");
const BLOOM_SHADER_SOURCE: &str = include_str!("../src/bloom.wgsl");
const OIT_COMPOSITE_SHADER_SOURCE: &str = include_str!("../src/oit_composite.wgsl");

// This pulls the field names out of a `struct Name { ... }` block, skipping comment lines, so a Rust struct
// and a WGSL struct can be compared as two lists of names in declaration order.
fn function_code(source: &str, declaration: &str) -> String {
    let start = source.find(declaration).unwrap_or_else(|| panic!("{declaration} should exist"));
    let end = start + source[start..].find("\n}\n").unwrap_or_else(|| panic!("{declaration} should be closed"));
    source[start..end].to_string()
}

fn field_names_of_struct(source: &str, struct_declaration: &str) -> Vec<String> {
    let after_name = source
        .split_once(struct_declaration)
        .unwrap_or_else(|| panic!("{struct_declaration} should exist"))
        .1;
    let body = after_name.split_once('}').unwrap_or_else(|| panic!("{struct_declaration} should be closed")).0;

    let mut field_names = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let Some((field_name, _)) = line.split_once(':') else {
            continue;
        };
        field_names.push(field_name.trim().trim_start_matches("pub ").trim().to_string());
    }
    field_names
}

#[test]
fn every_shader_declares_the_same_lighting_buffer_fields_as_the_renderer() {
    let renderer_field_names = field_names_of_struct(SHADER_TYPES_SOURCE, "pub struct LightingBufferContent {");
    assert!(
        renderer_field_names.contains(&"sky_top_color_and_padding".to_string())
            && renderer_field_names.contains(&"sky_horizon_color_and_padding".to_string())
            && renderer_field_names.contains(&"sky_bottom_color_and_padding".to_string()),
        "the lighting buffer should carry the three sky gradient stops, got {renderer_field_names:?}"
    );

    for (shader_name, shader_source) in [
        ("scene.wgsl", SCENE_SHADER_SOURCE),
        ("sky_sun.wgsl", SKY_SUN_SHADER_SOURCE),
        ("sky_gradient.wgsl", SKY_GRADIENT_SHADER_SOURCE),
        ("ambient_occlusion.wgsl", AMBIENT_OCCLUSION_SHADER_SOURCE),
    ] {
        let shader_field_names = field_names_of_struct(shader_source, "struct LightingBufferContent {");
        assert_eq!(
            shader_field_names, renderer_field_names,
            "{shader_name} declares different LightingBufferContent fields than render_scene.rs"
        );
    }
}

// WGSL cannot share a function between files, so the tone curve and dithering are copied into every shader that
// draws lit color. If one copy changes without the other, the sky and the ground stop matching at the horizon,
// so this checks the copies are still identical.
#[test]
fn every_shader_that_draws_lit_color_uses_the_same_tone_curve() {
    let tone_curve_code = |source: &str| {
        let start = source.find("const TONE_CURVE_COMPRESSION_START").expect("the tone curve should exist");
        let last_function = source
            .find("fn convert_light_to_display_color")
            .expect("the display color conversion should exist");
        let end = last_function + source[last_function..].find("\n}\n").expect("the display color conversion should be closed");
        source[start..end].to_string()
    };
    assert_eq!(
        tone_curve_code(SKY_GRADIENT_SHADER_SOURCE),
        tone_curve_code(SCENE_SHADER_SOURCE),
        "sky_gradient.wgsl and scene.wgsl no longer share the same tone curve and dithering"
    );
}

// Shiny surfaces reflect the sky, so scene.wgsl carries copies of how the sky is drawn. If a copy drifts, reflections
// stop matching the sky they claim to show.
#[test]
fn reflections_read_the_sky_the_same_way_the_sky_is_drawn() {
    let falloff_line = |source: &str| {
        source
            .lines()
            .find(|line| line.starts_with("const SKY_GRADIENT_FALLOFF"))
            .expect("the gradient falloff should exist")
            .to_string()
    };
    assert_eq!(
        function_code(SKY_GRADIENT_SHADER_SOURCE, "fn sky_gradient_color("),
        function_code(SCENE_SHADER_SOURCE, "fn sky_gradient_color("),
        "sky_gradient.wgsl and scene.wgsl no longer draw the same gradient"
    );
    assert_eq!(falloff_line(SKY_GRADIENT_SHADER_SOURCE), falloff_line(SCENE_SHADER_SOURCE));
    assert_eq!(
        function_code(SKYBOX_SHADER_SOURCE, "fn equirectangular_uv("),
        function_code(SCENE_SHADER_SOURCE, "fn equirectangular_uv("),
        "skybox.wgsl and scene.wgsl no longer map directions onto the skybox the same way"
    );
    let renderer_field_names = field_names_of_struct(SHADER_TYPES_SOURCE, "pub struct SkyboxBufferContent {");
    for (shader_name, shader_source) in [("scene.wgsl", SCENE_SHADER_SOURCE), ("skybox.wgsl", SKYBOX_SHADER_SOURCE)] {
        assert_eq!(
            field_names_of_struct(shader_source, "struct SkyboxBufferContent {"),
            renderer_field_names,
            "{shader_name} declares different SkyboxBufferContent fields than render_scene.rs"
        );
    }
}

// The base layer's composite grows the finished bloom to the picture's size with the same filter bloom.wgsl grows each level
// with, so oit_composite.wgsl carries a copy of it. If the copies drift, the finest glow stops matching the rest of it.
#[test]
fn the_composite_grows_the_bloom_the_same_way_bloom_does() {
    assert_eq!(
        function_code(OIT_COMPOSITE_SHADER_SOURCE, "fn sample_grown("),
        function_code(BLOOM_SHADER_SOURCE, "fn sample_grown("),
        "oit_composite.wgsl and bloom.wgsl no longer grow the bloom the same way"
    );
}
