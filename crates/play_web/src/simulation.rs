use game_types::{consts::CAMERA_ROTATE_SENSITIVITY, object_types::Orientation3D};
use glam::Vec3;

// The movement keys held down right now. The browser reports each key by its code, and the page maps those to these.
#[derive(Clone, Copy)]
pub struct HeldMovementKeys {
    pub is_forward: bool,
    pub is_backward: bool,
    pub is_left: bool,
    pub is_right: bool,
}

// Where the player wants to walk, at walking speed, in the camera's horizontal frame. The camera's pitch is left out
// so looking at the floor never walks the character into it, the same way the shipped character controller does it.
pub fn walkingVelocity(camera_orientation: Orientation3D, held_keys: HeldMovementKeys, speed: f32) -> Vec3 {
    let forward = camera_orientation.to_unit_direction();
    let flat_forward = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let right = Vec3::new(-flat_forward.z, 0.0, flat_forward.x);

    let mut direction = Vec3::ZERO;
    if held_keys.is_forward {
        direction += flat_forward;
    }
    if held_keys.is_backward {
        direction -= flat_forward;
    }
    if held_keys.is_right {
        direction += right;
    }
    if held_keys.is_left {
        direction -= right;
    }
    direction.normalize_or_zero() * speed
}

// The camera after the mouse moved this far, in the same units and sensitivity the desktop client turns it.
pub fn turnedCameraOrientation(camera_orientation: Orientation3D, delta_x_pixels: f32, delta_y_pixels: f32) -> Orientation3D {
    let mut turned = camera_orientation;
    turned.rotate_as_camera(-delta_x_pixels * CAMERA_ROTATE_SENSITIVITY, delta_y_pixels * CAMERA_ROTATE_SENSITIVITY);
    turned
}
