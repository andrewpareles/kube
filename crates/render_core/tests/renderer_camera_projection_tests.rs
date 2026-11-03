use glam::Vec3;
use render_core::{
    camera::{
        CAMERA_NEAR_PLANE, DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix, build_ray_from_camera_viewport_point, build_ray_from_view_proj_ndc,
    },
    renderer::SimpleViewportRegion,
};

// TODO: AI-PICKED-VALUE: This is far enough to catch a finite 1000-unit camera plane coming back.
const VERY_FAR_VISIBLE_TEST_DISTANCE: f32 = 1_000_000.0;
// TODO: AI-PICKED-VALUE: Projection math should be stable well inside this small float margin.
const PROJECTION_FLOAT_EPSILON: f32 = 0.000001;

#[test]
fn camera_projection_keeps_very_far_center_point_inside_depth_range() {
    let view_proj = build_camera_view_proj_matrix(Vec3::ZERO, Vec3::NEG_Z, 16.0 / 9.0, DEFAULT_CAMERA_FOV_DEGREES);

    // The near plane should stay at the front of the reverse-Z depth range.
    let projected_near = view_proj.project_point3(Vec3::new(0.0, 0.0, -CAMERA_NEAR_PLANE));
    assert!(
        projected_near.x.abs() < PROJECTION_FLOAT_EPSILON
            && projected_near.y.abs() < PROJECTION_FLOAT_EPSILON
            && (projected_near.z - 1.0).abs() < PROJECTION_FLOAT_EPSILON,
        "near point should project to the center front of reverse-Z depth, got {:?}",
        projected_near
    );

    // Very far objects should remain inside the depth range instead of being clipped by a finite far plane.
    let projected_far = view_proj.project_point3(Vec3::new(0.0, 0.0, -VERY_FAR_VISIBLE_TEST_DISTANCE));
    let expected_far_depth = CAMERA_NEAR_PLANE / VERY_FAR_VISIBLE_TEST_DISTANCE;
    assert!(
        projected_far.x.abs() < PROJECTION_FLOAT_EPSILON
            && projected_far.y.abs() < PROJECTION_FLOAT_EPSILON
            && projected_far.z > 0.0
            && projected_far.z < 1.0
            && (projected_far.z - expected_far_depth).abs() < PROJECTION_FLOAT_EPSILON,
        "very far point should stay inside reverse-Z depth, got {:?}",
        projected_far
    );
}

#[test]
fn camera_ray_from_ndc_stays_finite_with_infinite_reverse_z_projection() {
    let view_proj = build_camera_view_proj_matrix(Vec3::ZERO, Vec3::NEG_Z, 16.0 / 9.0, DEFAULT_CAMERA_FOV_DEGREES);
    let Some((center_ray_origin, center_ray_direction)) = build_ray_from_view_proj_ndc(view_proj, 0.0, 0.0) else {
        panic!("center camera ray should be valid");
    };

    // The picking ray starts on the finite near plane and points through the clicked pixel.
    assert!(center_ray_origin.is_finite(), "center ray origin should be finite, got {:?}", center_ray_origin);
    assert!(
        (center_ray_origin - Vec3::new(0.0, 0.0, -CAMERA_NEAR_PLANE)).length() < PROJECTION_FLOAT_EPSILON,
        "center ray origin should be on the near plane, got {:?}",
        center_ray_origin
    );
    assert!(
        center_ray_direction.is_finite(),
        "center ray direction should be finite, got {:?}",
        center_ray_direction
    );
    assert!(
        (center_ray_direction.length() - 1.0).abs() < PROJECTION_FLOAT_EPSILON && (center_ray_direction - Vec3::NEG_Z).length() < PROJECTION_FLOAT_EPSILON,
        "center ray direction should be normalized and point forward, got {:?}",
        center_ray_direction
    );

    let Some((off_center_ray_origin, off_center_ray_direction)) = build_ray_from_view_proj_ndc(view_proj, 0.5, -0.25) else {
        panic!("off-center camera ray should be valid");
    };
    assert!(
        off_center_ray_origin.is_finite(),
        "off-center ray origin should be finite, got {:?}",
        off_center_ray_origin
    );
    assert!(
        off_center_ray_direction.is_finite(),
        "off-center ray direction should be finite, got {:?}",
        off_center_ray_direction
    );
    assert!(
        (off_center_ray_direction.length() - 1.0).abs() < PROJECTION_FLOAT_EPSILON
            && off_center_ray_direction.x > 0.0
            && off_center_ray_direction.y < 0.0
            && off_center_ray_direction.z < 0.0,
        "off-center ray direction should point through the clicked side of the view, got {:?}",
        off_center_ray_direction
    );
}

// The mouse ray and the editor's click picking both go through this, so a viewport point has to land on the same ray
// as the matching NDC point, including when the viewport is a sub-rect of the window like the editor's play tab.
#[test]
fn camera_ray_from_viewport_point_matches_the_same_point_in_ndc() {
    let viewport_region = SimpleViewportRegion {
        x: 100,
        y: 50,
        width: 800,
        height: 400,
    };
    let aspect_ratio = viewport_region.width as f32 / viewport_region.height as f32;
    let view_proj = build_camera_view_proj_matrix(Vec3::ZERO, Vec3::NEG_Z, aspect_ratio, DEFAULT_CAMERA_FOV_DEGREES);

    // the middle of the viewport is NDC (0, 0), even though it is nowhere near the middle of the window
    let Some((center_ray_origin, center_ray_direction)) =
        build_ray_from_camera_viewport_point(Vec3::ZERO, Vec3::NEG_Z, DEFAULT_CAMERA_FOV_DEGREES, viewport_region, 500.0, 250.0)
    else {
        panic!("viewport center ray should be valid");
    };
    let Some((expected_center_ray_origin, expected_center_ray_direction)) = build_ray_from_view_proj_ndc(view_proj, 0.0, 0.0) else {
        panic!("center NDC ray should be valid");
    };
    assert!(
        (center_ray_origin - expected_center_ray_origin).length() < PROJECTION_FLOAT_EPSILON
            && (center_ray_direction - expected_center_ray_direction).length() < PROJECTION_FLOAT_EPSILON,
        "viewport center should match NDC (0, 0), got origin {:?} direction {:?}",
        center_ray_origin,
        center_ray_direction
    );

    // a quarter across and a quarter down from the top-left corner is NDC (-0.5, 0.5): window space counts y downward, NDC counts it upward
    let Some((corner_ray_origin, corner_ray_direction)) =
        build_ray_from_camera_viewport_point(Vec3::ZERO, Vec3::NEG_Z, DEFAULT_CAMERA_FOV_DEGREES, viewport_region, 300.0, 150.0)
    else {
        panic!("off-center viewport ray should be valid");
    };
    let Some((expected_corner_ray_origin, expected_corner_ray_direction)) = build_ray_from_view_proj_ndc(view_proj, -0.5, 0.5) else {
        panic!("off-center NDC ray should be valid");
    };
    assert!(
        (corner_ray_origin - expected_corner_ray_origin).length() < PROJECTION_FLOAT_EPSILON
            && (corner_ray_direction - expected_corner_ray_direction).length() < PROJECTION_FLOAT_EPSILON,
        "viewport point should match NDC (-0.5, 0.5), got origin {:?} direction {:?}",
        corner_ray_origin,
        corner_ray_direction
    );

    // points outside the viewport have no ray at all, instead of being clamped onto its edge
    assert!(
        build_ray_from_camera_viewport_point(Vec3::ZERO, Vec3::NEG_Z, DEFAULT_CAMERA_FOV_DEGREES, viewport_region, 99.0, 250.0).is_none(),
        "a point left of the viewport should have no ray"
    );
    assert!(
        build_ray_from_camera_viewport_point(Vec3::ZERO, Vec3::NEG_Z, DEFAULT_CAMERA_FOV_DEGREES, viewport_region, 500.0, 450.0).is_none(),
        "a point below the viewport should have no ray"
    );
}

// The fov is the vertical angle in degrees, so a point half the fov above the view's center lands on the top edge. The mouse
// ray takes the same fov, or clicks would land somewhere other than what is drawn under the cursor.
#[test]
fn camera_fov_is_the_vertical_angle_for_drawing_and_the_mouse_ray() {
    let viewport_region = SimpleViewportRegion {
        x: 0,
        y: 0,
        width: 1600,
        height: 900,
    };
    let aspect_ratio = viewport_region.width as f32 / viewport_region.height as f32;
    for fov_degrees in [30.0_f32, DEFAULT_CAMERA_FOV_DEGREES, 110.0] {
        let top_edge_point = Vec3::new(0.0, (fov_degrees * 0.5).to_radians().tan(), -1.0);
        let projected_top_edge_point = build_camera_view_proj_matrix(Vec3::ZERO, Vec3::NEG_Z, aspect_ratio, fov_degrees).project_point3(top_edge_point);
        assert!(
            (projected_top_edge_point.y - 1.0).abs() < 1e-5,
            "fov {fov_degrees}: half the fov up should be the top edge, got {projected_top_edge_point:?}"
        );

        let Some((_, top_ray_direction)) = build_ray_from_camera_viewport_point(Vec3::ZERO, Vec3::NEG_Z, fov_degrees, viewport_region, 800.0, 0.0) else {
            panic!("fov {fov_degrees}: the top middle of the viewport should have a ray");
        };
        assert!(
            (top_ray_direction - top_edge_point.normalize()).length() < 1e-5,
            "fov {fov_degrees}: the mouse at the top edge should point half the fov up, got {top_ray_direction:?}"
        );
    }
}
