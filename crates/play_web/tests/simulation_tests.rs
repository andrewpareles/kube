use game_types::{
    consts::{CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK, CLIENT_SIM_AND_SEND_SECS},
    object_types::Orientation3D,
};
use glam::Vec3;
use play_web::simulation::{HeldMovementKeys, turnedCameraOrientation, walkingVelocity};
use shared::game::world::World;

const WALKING_SPEED: f32 = 7.0;
const STEP_MS: f32 = CLIENT_SIM_AND_SEND_SECS * 1000.0;

fn heldKeys(is_forward: bool, is_backward: bool, is_left: bool, is_right: bool) -> HeldMovementKeys {
    HeldMovementKeys {
        is_forward,
        is_backward,
        is_left,
        is_right,
    }
}

// -------------------------- Walking --------------------------

#[test]
fn forward_walks_along_the_direction_the_camera_faces() {
    let facing_plus_z = Orientation3D { theta: 0.0, phi: 0.0 };
    let velocity = walkingVelocity(facing_plus_z, heldKeys(true, false, false, false), WALKING_SPEED);
    assert!(
        (velocity - Vec3::new(0.0, 0.0, WALKING_SPEED)).length() < 1e-4,
        "walking forward should head +Z: {velocity:?}"
    );

    let facing_plus_x = Orientation3D {
        theta: std::f32::consts::FRAC_PI_2,
        phi: 0.0,
    };
    let velocity = walkingVelocity(facing_plus_x, heldKeys(true, false, false, false), WALKING_SPEED);
    assert!(
        (velocity - Vec3::new(WALKING_SPEED, 0.0, 0.0)).length() < 1e-4,
        "walking forward should turn with the camera: {velocity:?}"
    );
}

#[test]
fn the_camera_pitch_never_walks_the_character_up_or_down() {
    let looking_at_the_floor = Orientation3D {
        theta: 0.0,
        phi: std::f32::consts::FRAC_PI_2 - 0.02,
    };
    let velocity = walkingVelocity(looking_at_the_floor, heldKeys(true, false, false, false), WALKING_SPEED);
    assert!(
        velocity.y.abs() < 1e-5,
        "walking forward should stay level however far the camera looks down: {velocity:?}"
    );
    assert!(
        (velocity.length() - WALKING_SPEED).abs() < 1e-4,
        "looking down should not shorten the walk: {velocity:?}"
    );
}

#[test]
fn walking_diagonally_is_no_faster_than_walking_straight() {
    let facing_plus_z = Orientation3D { theta: 0.0, phi: 0.0 };
    let straight = walkingVelocity(facing_plus_z, heldKeys(true, false, false, false), WALKING_SPEED);
    let diagonal = walkingVelocity(facing_plus_z, heldKeys(true, false, false, true), WALKING_SPEED);
    assert!(
        (diagonal.length() - straight.length()).abs() < 1e-4,
        "two keys should not add up to a sprint: {diagonal:?}"
    );
    // Facing +Z with up +Y puts the player's right along -X, which is the same right the character controller walks along.
    assert!(
        diagonal.x < 0.0 && diagonal.z > 0.0,
        "forward and right should head between the two: {diagonal:?}"
    );
}

#[test]
fn opposite_keys_cancel_out_and_letting_go_stops() {
    let facing_plus_z = Orientation3D { theta: 0.0, phi: 0.0 };
    assert!(
        walkingVelocity(facing_plus_z, heldKeys(true, true, false, false), WALKING_SPEED).length() < 1e-5,
        "holding forward and backward together should stand still"
    );
    assert!(
        walkingVelocity(facing_plus_z, heldKeys(false, false, false, false), WALKING_SPEED).length() < 1e-5,
        "holding nothing should ask for no walking"
    );
}

// -------------------------- Mouse look --------------------------

#[test]
fn dragging_the_mouse_right_turns_the_camera_right_and_up_looks_up() {
    let facing_plus_z = Orientation3D { theta: 0.0, phi: 0.0 };
    let turned_right = turnedCameraOrientation(facing_plus_z, 100.0, 0.0);
    assert!(
        turned_right.theta < 0.0,
        "moving the pointer right should turn the camera clockwise seen from above: {turned_right:?}"
    );
    let turned_down = turnedCameraOrientation(facing_plus_z, 0.0, 100.0);
    assert!(
        turned_down.phi > 0.0,
        "moving the pointer down should look down, since a positive pitch faces down: {turned_down:?}"
    );
}

#[test]
fn the_camera_stops_at_straight_up_and_straight_down() {
    let facing_plus_z = Orientation3D { theta: 0.0, phi: 0.0 };
    let far_up = turnedCameraOrientation(facing_plus_z, 0.0, -100_000.0);
    let far_down = turnedCameraOrientation(facing_plus_z, 0.0, 100_000.0);
    assert!(
        far_up.phi > -std::f32::consts::FRAC_PI_2 && far_down.phi < std::f32::consts::FRAC_PI_2,
        "pitch should stay short of vertical"
    );
    assert!(
        far_up.phi.is_finite() && far_down.phi.is_finite(),
        "a long drag should still leave a usable camera"
    );
}

// -------------------------- The step budget --------------------------

#[test]
fn a_frame_runs_the_steps_that_came_due_since_the_last_one() {
    assert_eq!(
        World::get_num_physics_steps_to_take(0.0, 0.0, STEP_MS, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK),
        0,
        "no time passed, so nothing steps"
    );
    assert_eq!(
        World::get_num_physics_steps_to_take(0.0, STEP_MS as f64, STEP_MS, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK),
        1,
        "one step's worth of time runs one step"
    );
    assert_eq!(
        World::get_num_physics_steps_to_take(0.0, 2.5 * STEP_MS as f64, STEP_MS, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK),
        3,
        "two and a half steps' worth of time rounds up so the world keeps up with the clock"
    );
}

#[test]
fn a_stalled_frame_drops_the_time_it_missed_instead_of_fast_forwarding() {
    assert_eq!(
        World::get_num_physics_steps_to_take(0.0, 200.0, STEP_MS, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK),
        CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK,
        "a 200ms stall should not run twelve steps, because the world would visibly speed up afterwards"
    );
}

#[test]
fn a_frame_that_arrives_before_its_time_steps_backwards_never_steps() {
    assert_eq!(
        World::get_num_physics_steps_to_take(100.0, 50.0, STEP_MS, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK),
        0,
        "a clock that went backwards should hold the world still rather than run it in reverse"
    );
}
