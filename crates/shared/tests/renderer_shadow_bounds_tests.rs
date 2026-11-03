use game_types::object_types::{Orientation3D, WorldLightingInfo, shared::Color3};
use glam::{Mat4, Quat, Vec3};
use render_core::{render_scene::SHADOW_CASCADE_COUNT, renderer::build_shadow_cascade_view_projections};

const CLOSE_CASCADE_INDEX: usize = 0;
const STEP_UP_CASCADE_INDEX: usize = 1;
const MEDIUM_CASCADE_INDEX: usize = 2;
const FAR_CASCADE_INDEX: usize = 3;
const VERY_FAR_CASCADE_INDEX: usize = SHADOW_CASCADE_COUNT - 1;
// TODO: AI-PICKED-VALUE: Keep this synced with renderer.rs shadow cascade texture sizes while tests cannot read private constants.
const SHADOW_TEST_TEXTURE_SIZE: f32 = 2048.0;
// TODO: AI-PICKED-VALUE: This keeps edge probes just inside the cascade edge so float roundoff does not decide the test.
const CLOSE_CASCADE_EDGE_TEST_MARGIN_NDC: f32 = 0.001;
// TODO: AI-PICKED-VALUE: This requires the close cascade to stay several times more detailed than medium, without tying the test to private constants.
const CLOSE_DETAIL_RATIO_TO_MEDIUM_MAX: f32 = 0.6;
// TODO: AI-PICKED-VALUE: This catches accidental loss of the original HD near-shadow region.
const CLOSE_SELECTED_WORLD_RADIUS_MAX: f32 = 20.0;
// TODO: AI-PICKED-VALUE: Matrix inversion around low sun angles adds small float error, but this is still far below one shadow texel.
const TEXEL_CENTER_PHASE_EPSILON: f32 = 0.03;
// TODO: AI-PICKED-VALUE: A low sun puts casters this far up-sun barely a hundred units away horizontally, which is
// well inside an ordinary world, so every cascade has to keep them.
const DISTANT_CASTER_UP_SUN_DISTANCE: f32 = 1000.0;

fn sunken_temple_world_lighting_info() -> WorldLightingInfo {
    WorldLightingInfo {
        sunOrientation: Orientation3D {
            theta: 3.5953782,
            phi: 3.3335788,
        },
        sunColor: Color3 { r: 1.0, g: 0.5, b: 0.0 },
        sunIntensity: 19.4,
        sunShadowSoftness: 2.0,
        ambientTopColor: Color3 { r: 0.58, g: 1.0, b: 1.0 },
        ambientBottomColor: Color3 { r: 1.0, g: 1.0, b: 0.65 },
        ambientIntensity: 0.6,
        ambientOcclusionIntensity: 0.7,
        ambientOcclusionRadius: 1.0,
        ambientOcclusionSunlightStrength: 0.5,
        skyTopColor: Color3 { r: 1.0, g: 1.0, b: 0.58 },
        skyHorizonColor: Color3 { r: 0.9, g: 0.95, b: 1.0 },
        skyBottomColor: Color3 { r: 0.2, g: 0.18, b: 0.16 },
        skyboxTextureDataId: Some(13),
        skyboxTextureOpacity: 0.0,
        skyReflectionStrength: 1.0,
        vibrance: 0.2,
    }
}

fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
    let delta = (actual - expected).abs();
    assert!(
        delta.x < 0.0001 && delta.y < 0.0001 && delta.z < 0.0001,
        "{} expected {:?}, got {:?}",
        label,
        expected,
        actual
    );
}

fn assert_f32_close(actual: f32, expected: f32, label: &str) {
    assert!((actual - expected).abs() < 0.0001, "{} expected {:?}, got {:?}", label, expected, actual);
}

fn is_inside_shadow_projection(view_proj: Mat4, point: Vec3) -> bool {
    let projected = view_proj.project_point3(point);
    projected.x.abs() <= 1.0 && projected.y.abs() <= 1.0 && projected.z >= 0.0 && projected.z <= 1.0
}

fn projection_world_radius_at_center(view_proj: Mat4, shadow_center: Vec3) -> f32 {
    let projected_center = view_proj.project_point3(shadow_center);
    let projection_world_center = view_proj.inverse().project_point3(Vec3::new(0.0, 0.0, projected_center.z));
    let projection_world_x_edge = view_proj.inverse().project_point3(Vec3::new(1.0, 0.0, projected_center.z));
    (projection_world_x_edge - projection_world_center).length()
}

fn projection_center_and_axes(view_proj: Mat4, shadow_center: Vec3) -> [Vec3; 3] {
    let projected_center = view_proj.project_point3(shadow_center);
    let projection_world_center = view_proj.inverse().project_point3(Vec3::new(0.0, 0.0, projected_center.z));
    let projection_world_x_edge = view_proj.inverse().project_point3(Vec3::new(1.0, 0.0, projected_center.z));
    let projection_world_y_edge = view_proj.inverse().project_point3(Vec3::new(0.0, 1.0, projected_center.z));
    let x_axis = (projection_world_x_edge - projection_world_center).normalize();
    let y_axis = (projection_world_y_edge - projection_world_center).normalize();
    [projection_world_center, x_axis, y_axis]
}

#[test]
fn close_shadow_cascade_keeps_meaningfully_finer_scale_than_step_up() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let close_world_radius = projection_world_radius_at_center(shadow_cascades[CLOSE_CASCADE_INDEX], shadow_center);
    let step_up_world_radius = projection_world_radius_at_center(shadow_cascades[STEP_UP_CASCADE_INDEX], shadow_center);

    assert!(close_world_radius < step_up_world_radius * CLOSE_DETAIL_RATIO_TO_MEDIUM_MAX);
}

#[test]
fn close_shadow_cascade_preserves_hd_near_region() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let close_world_radius = projection_world_radius_at_center(shadow_cascades[CLOSE_CASCADE_INDEX], shadow_center);

    assert!(close_world_radius < CLOSE_SELECTED_WORLD_RADIUS_MAX);
}

#[test]
fn shadow_cascades_have_close_step_up_overlap() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let close_shadow_cascade = shadow_cascades[CLOSE_CASCADE_INDEX];
    let step_up_shadow_cascade = shadow_cascades[STEP_UP_CASCADE_INDEX];
    let close_projected_center = close_shadow_cascade.project_point3(shadow_center);
    let close_edge_test_radius = 1.0 - CLOSE_CASCADE_EDGE_TEST_MARGIN_NDC;

    for close_light_edge in [
        Vec3::new(close_edge_test_radius, close_edge_test_radius, close_projected_center.z),
        Vec3::new(-close_edge_test_radius, close_edge_test_radius, close_projected_center.z),
        Vec3::new(close_edge_test_radius, -close_edge_test_radius, close_projected_center.z),
        Vec3::new(-close_edge_test_radius, -close_edge_test_radius, close_projected_center.z),
        Vec3::new(close_edge_test_radius, close_projected_center.y, close_projected_center.z),
    ] {
        let close_edge_world_point = close_shadow_cascade.inverse().project_point3(close_light_edge);
        assert!(is_inside_shadow_projection(close_shadow_cascade, close_edge_world_point));
        assert!(is_inside_shadow_projection(step_up_shadow_cascade, close_edge_world_point));
    }
}

#[test]
fn shadow_cascades_are_deterministic_and_follow_the_shadow_center() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let same_shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let moved_shadow_cascades = build_shadow_cascade_view_projections(
        world_lighting_info.sunOrientation.to_unit_direction(),
        shadow_center + Vec3::new(512.0, 0.0, 0.0),
    );

    assert_eq!(shadow_cascades.len(), SHADOW_CASCADE_COUNT);
    for cascade_index in 0..SHADOW_CASCADE_COUNT {
        assert_eq!(
            shadow_cascades[cascade_index].to_cols_array(),
            same_shadow_cascades[cascade_index].to_cols_array()
        );
        assert_ne!(
            shadow_cascades[cascade_index].to_cols_array(),
            moved_shadow_cascades[cascade_index].to_cols_array()
        );
    }
}

#[test]
fn shadow_cascade_texel_centers_are_aligned() {
    fn texel_size_at_center(view_proj: Mat4, shadow_center: Vec3) -> f32 {
        let [projection_world_center, _, _] = projection_center_and_axes(view_proj, shadow_center);
        let projected_center = view_proj.project_point3(shadow_center);
        let projection_world_x_edge = view_proj.inverse().project_point3(Vec3::new(1.0, 0.0, projected_center.z));
        let projection_world_radius = (projection_world_x_edge - projection_world_center).length();
        projection_world_radius * 2.0 / SHADOW_TEST_TEXTURE_SIZE
    }

    fn assert_center_phase(value: f32, label: &str) {
        let nearest_center_index = (value - 0.5).round();
        let nearest_center_value = nearest_center_index + 0.5;
        let nearest_edge_value = value.round();
        assert!(
            (value - nearest_center_value).abs() < TEXEL_CENTER_PHASE_EPSILON,
            "{} expected texel center phase, got {}",
            label,
            value
        );
        assert!((value - nearest_edge_value).abs() > 0.4, "{} was aligned to an edge instead of a center", label);
    }

    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let [close_projection_center, close_x_axis, close_y_axis] = projection_center_and_axes(shadow_cascades[CLOSE_CASCADE_INDEX], shadow_center);
    let close_texel_size = texel_size_at_center(shadow_cascades[CLOSE_CASCADE_INDEX], shadow_center);

    for cascade_index in [STEP_UP_CASCADE_INDEX, MEDIUM_CASCADE_INDEX, FAR_CASCADE_INDEX, VERY_FAR_CASCADE_INDEX] {
        let [cascade_projection_center, cascade_x_axis, cascade_y_axis] = projection_center_and_axes(shadow_cascades[cascade_index], shadow_center);
        let cascade_texel_size = texel_size_at_center(shadow_cascades[cascade_index], shadow_center);
        let cascade_positive_texel_center = cascade_projection_center + cascade_x_axis * cascade_texel_size * 0.5 + cascade_y_axis * cascade_texel_size * 0.5;
        let x_phase_in_close_texels = (cascade_positive_texel_center - close_projection_center).dot(close_x_axis) / close_texel_size;
        let y_phase_in_close_texels = (cascade_positive_texel_center - close_projection_center).dot(close_y_axis) / close_texel_size;

        assert_vec3_close(cascade_x_axis, close_x_axis, "x axis");
        assert_vec3_close(cascade_y_axis, close_y_axis, "y axis");
        assert_center_phase(x_phase_in_close_texels, "x phase");
        assert_center_phase(y_phase_in_close_texels, "y phase");
    }
}

#[test]
fn shadow_cascades_keep_close_detail_without_using_large_unrelated_models() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let central_golden_sand = Mat4::from_scale_rotation_translation(Vec3::new(72.0, 0.5, 16.0), Quat::IDENTITY, Vec3::new(0.0, 0.25, 0.0));
    let spawn_pad = Mat4::from_scale_rotation_translation(Vec3::new(6.0, 0.3, 6.0), Quat::IDENTITY, Vec3::new(0.0, 0.6, -1.0));
    let central_golden_sand_top = central_golden_sand.transform_point3(Vec3::new(0.0, 0.5, -1.0));
    let spawn_pad_top = spawn_pad.transform_point3(Vec3::new(0.0, 0.5, 0.0));

    let shadow_center = spawn_pad.transform_point3(Vec3::ZERO);
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let same_shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let far_shadow_cascades = build_shadow_cascade_view_projections(
        world_lighting_info.sunOrientation.to_unit_direction(),
        shadow_center + Vec3::new(512.0, 0.0, 0.0),
    );

    assert_eq!(shadow_cascades.len(), SHADOW_CASCADE_COUNT);
    assert_eq!(
        shadow_cascades[CLOSE_CASCADE_INDEX].to_cols_array(),
        same_shadow_cascades[CLOSE_CASCADE_INDEX].to_cols_array()
    );
    assert_eq!(
        shadow_cascades[STEP_UP_CASCADE_INDEX].to_cols_array(),
        same_shadow_cascades[STEP_UP_CASCADE_INDEX].to_cols_array()
    );
    assert_eq!(
        shadow_cascades[MEDIUM_CASCADE_INDEX].to_cols_array(),
        same_shadow_cascades[MEDIUM_CASCADE_INDEX].to_cols_array()
    );
    assert_eq!(
        shadow_cascades[FAR_CASCADE_INDEX].to_cols_array(),
        same_shadow_cascades[FAR_CASCADE_INDEX].to_cols_array()
    );
    assert_ne!(
        shadow_cascades[CLOSE_CASCADE_INDEX].to_cols_array(),
        far_shadow_cascades[CLOSE_CASCADE_INDEX].to_cols_array()
    );
    assert!(is_inside_shadow_projection(shadow_cascades[CLOSE_CASCADE_INDEX], central_golden_sand_top));
    assert!(is_inside_shadow_projection(shadow_cascades[CLOSE_CASCADE_INDEX], spawn_pad_top));
    assert!(!is_inside_shadow_projection(
        shadow_cascades[CLOSE_CASCADE_INDEX],
        shadow_center + Vec3::new(160.0, 0.0, 0.0)
    ));
    assert!(is_inside_shadow_projection(
        shadow_cascades[FAR_CASCADE_INDEX],
        shadow_center + Vec3::new(160.0, 0.0, 0.0)
    ));
}

#[test]
fn every_shadow_cascade_records_casters_far_up_sun_of_the_shadow_center() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let sun_direction = world_lighting_info.sunOrientation.to_unit_direction();
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    // A caster this far up-sun still drops its shadow on the shadow center, so a cascade that clips it away leaves a
    // lit hole that slides along with the shadow center as the player walks.
    let distant_caster_position = shadow_center + sun_direction * DISTANT_CASTER_UP_SUN_DISTANCE;

    for (cascade_index, shadow_cascade) in shadow_cascades.iter().enumerate() {
        let projected_caster = shadow_cascade.project_point3(distant_caster_position);
        assert!(
            projected_caster.z >= 0.0 && projected_caster.z <= 1.0,
            "cascade {} clips a caster {} up-sun of the shadow center, at depth {}",
            cascade_index,
            DISTANT_CASTER_UP_SUN_DISTANCE,
            projected_caster.z
        );
    }
}

#[test]
fn every_shadow_cascade_shares_one_depth_range_so_cascade_handoffs_keep_the_same_casters() {
    let world_lighting_info = sunken_temple_world_lighting_info();
    let shadow_center = Vec3::new(0.0, 0.6, -1.0);
    let sun_direction = world_lighting_info.sunOrientation.to_unit_direction();
    let shadow_cascades = build_shadow_cascade_view_projections(world_lighting_info.sunOrientation.to_unit_direction(), shadow_center);
    let close_caster_position = shadow_center + sun_direction * DISTANT_CASTER_UP_SUN_DISTANCE;
    let close_caster_depth = shadow_cascades[CLOSE_CASCADE_INDEX].project_point3(close_caster_position).z;

    for (cascade_index, shadow_cascade) in shadow_cascades.iter().enumerate() {
        let projected_caster = shadow_cascade.project_point3(close_caster_position);
        assert_f32_close(projected_caster.z, close_caster_depth, &format!("cascade {cascade_index} caster depth"));
    }
}
