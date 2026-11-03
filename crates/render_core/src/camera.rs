// The camera projection math, kept in one place because several different things have to agree on it exactly:
// the renderer draws the world through this matrix, and scripts use it to read the ray under the player's cursor.
// Sharing the matrix keeps the mouse ray lined up with the image on screen.

use crate::renderer::SimpleViewportRegion;
use glam::{Mat4, Vec3};

// Camera constants are shared by rendering, picking, and preview framing.
pub const DEFAULT_CAMERA_FOV_DEGREES: f32 = 70.0; // the vertical angle the camera sees
// TODO: AI-PICKED-VALUE: setFOV accepts 1 to 179 degrees, since the projection breaks down at 0 and 180.
pub const MIN_CAMERA_FOV_DEGREES: f32 = 1.0;
pub const MAX_CAMERA_FOV_DEGREES: f32 = 179.0;
pub const CAMERA_NEAR_PLANE: f32 = 0.1;

// The one camera matrix everything else is derived from. Reverse-Z with no far plane, so distant objects keep their
// depth precision instead of being clipped away.
pub fn build_camera_view_proj_matrix(camera_position: Vec3, target: Vec3, aspect: f32, fov_degrees: f32) -> Mat4 {
    let view = Mat4::look_at_rh(camera_position, target, Vec3::Y);
    let proj = Mat4::perspective_infinite_reverse_rh(fov_degrees.to_radians(), aspect, CAMERA_NEAR_PLANE);
    proj * view
}

// The ray through a normalized device point, where (0, 0) is the middle of the view and y counts upward.
// It starts on the near plane and points away from the camera. None when the camera setup makes the math blow up.
// Picking reads the matrix of the displayed frame, so camera motion between frames cannot move a hit.
// A finite depth sample avoids unprojecting the point at infinity in our reverse-Z projection.
pub fn build_ray_from_view_proj_ndc(view_proj: Mat4, ndc_x: f32, ndc_y: f32) -> Option<(Vec3, Vec3)> {
    if !view_proj.is_finite() || view_proj.determinant() == 0.0 {
        return None;
    }
    let inverse = view_proj.inverse();
    let origin = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
    let farther = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 0.5));
    let direction = (farther - origin).try_normalize()?;
    if !origin.is_finite() || !direction.is_finite() {
        return None;
    }
    Some((origin, direction))
}

// The ray under a window-space point, like the mouse cursor, measured against the rendered viewport.
// Returns None when the point is outside the viewport, because then the cursor is not pointing into the world at all.
pub fn build_ray_from_camera_viewport_point(
    camera_position: Vec3,
    target: Vec3,
    fov_degrees: f32,
    viewport_region: SimpleViewportRegion,
    point_x: f64,
    point_y: f64,
) -> Option<(Vec3, Vec3)> {
    if !viewport_region.contains_point(point_x, point_y) {
        return None;
    }
    let viewport_width = viewport_region.width.max(1) as f32;
    let viewport_height = viewport_region.height.max(1) as f32;
    let viewport_x = ((point_x as f32) - viewport_region.x as f32) / viewport_width;
    let viewport_y = ((point_y as f32) - viewport_region.y as f32) / viewport_height;
    // window space counts y downward from the top, normalized device space counts it upward from the middle
    let ndc_x = viewport_x.clamp(0.0, 1.0) * 2.0 - 1.0;
    let ndc_y = 1.0 - viewport_y.clamp(0.0, 1.0) * 2.0;
    let aspect_ratio = viewport_width / viewport_height;
    let view_proj = build_camera_view_proj_matrix(camera_position, target, aspect_ratio, fov_degrees);
    build_ray_from_view_proj_ndc(view_proj, ndc_x, ndc_y)
}
