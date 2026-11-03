use glam::Vec3;

use game_types::{
    consts::{DEFAULT_CAMERA_ORIENTATION, DEFAULT_CAMERA_ZOOM},
    object_types::Orientation3D,
};
use shared::game::jsRuntime::CameraLimits;

// This keeps the camera math shared so the play loop and the renderer always place the camera the same way.
pub fn getCameraPosition(target: Vec3, orientation: &Orientation3D, camera_zoom: f32) -> Vec3 {
    let unit_forward_direction = orientation.to_unit_direction();
    let cam_offset = -unit_forward_direction * camera_zoom;
    let cam_position = target + cam_offset;
    cam_position
}

// Client scripts can offset the camera along its own right, up and forward directions, so what it follows stays off the middle of the screen whichever way it turns.
pub fn getCameraTarget(followed_position: Vec3, orientation: &Orientation3D, camera_offset: Vec3) -> Vec3 {
    let forward = orientation.to_unit_direction();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    followed_position + right * camera_offset.x + up * camera_offset.y + forward * camera_offset.z
}

// Client scripts can lock the camera's direction and limit its zoom. The play loop and the renderer both apply this before placing the camera.
pub fn getClampedCamera(camera_limits: CameraLimits, camera_orientation: Orientation3D, camera_zoom: f32) -> (Orientation3D, f32) {
    let CameraLimits {
        locked_orientation,
        min_zoom,
        max_zoom,
        ..
    } = camera_limits;
    (locked_orientation.unwrap_or(camera_orientation), camera_zoom.clamp(min_zoom, max_zoom))
}

#[derive(Clone)]
pub struct PlayInputState {
    pub camera_orientation: Orientation3D,
    pub desired_camera_zoom: f32,
    pub mouse_ray: Option<(Vec3, Vec3)>,
}
impl PlayInputState {
    pub fn new() -> Self {
        Self {
            camera_orientation: DEFAULT_CAMERA_ORIENTATION,
            desired_camera_zoom: DEFAULT_CAMERA_ZOOM,
            mouse_ray: None,
        }
    }
}
