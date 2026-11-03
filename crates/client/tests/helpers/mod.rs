use game_types::object_types::{CameraLocation, Orientation3D};
use glam::Vec3;

pub fn fallback_camera_location_for_tests() -> CameraLocation {
    CameraLocation {
        position: Vec3::new(0.0, -10.0, 0.0),
        orientation: Orientation3D {
            theta: 0.0,
            phi: std::f32::consts::FRAC_PI_4,
        },
    }
}
