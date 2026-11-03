use glam::{Quat, Vec3};
use {
    game_types::object_types::PhysicsInfo3D,
    shared::game::interpolate_physics::{extrapolate_physics_info_3D, interpolate_between_physics_info_3Ds_hermite},
};

const SNAPSHOT_SPAN_SECS: f32 = 0.05;

fn spinning_about_y(yaw_degrees: f32, angular_velocity_degrees_per_sec: f32) -> PhysicsInfo3D {
    PhysicsInfo3D::new(
        Vec3::ZERO,
        Quat::from_rotation_y(yaw_degrees.to_radians()),
        Vec3::ZERO,
        Vec3::new(0.0, angular_velocity_degrees_per_sec.to_radians(), 0.0),
    )
}

fn interpolated_yaw_degrees(physics_info_0: &PhysicsInfo3D, physics_info_1: &PhysicsInfo3D, alpha: f32) -> f32 {
    let interpolated = interpolate_between_physics_info_3Ds_hermite(physics_info_0, physics_info_1, SNAPSHOT_SPAN_SECS, alpha);
    interpolated.location.quaternion.to_euler(glam::EulerRot::YXZ).0.to_degrees()
}

// Two endpoints spinning at the same rate cancel out of the prediction exactly, so a believable case needs the two
// rates to differ - here a platform speeding up, where curving away from the straight arc is the whole point.
#[test]
fn hermite_predicts_rotation_when_the_derived_rotation_is_believable() {
    let physics_info_0 = spinning_about_y(0.0, 40.0);
    let physics_info_1 = spinning_about_y(3.0, 80.0);

    // straight slerp would sit at the midpoint of the arc, 1.5 degrees
    let yaw_degrees = interpolated_yaw_degrees(&physics_info_0, &physics_info_1, 0.5);
    assert!(
        (yaw_degrees - 1.0).abs() <= 1e-3,
        "expected the prediction to pull the midpoint to 1.0 degrees, got {}",
        yaw_degrees
    );
}

// A stalled frame writes a whole backlog of motion as one step, so rapier derives a spin far faster than anything
// that happened - and the snapshot beside it, already back to normal, reports nothing. Predicting from that pair used
// to bow the platform right off the arc between the two poses; it has to fall back to plain slerp instead.
#[test]
fn hermite_falls_back_to_slerp_when_the_derived_rotation_is_not_believable() {
    // 60 degrees of backlog written as a single 1/60s step derives a spin of 3600 deg/s
    let physics_info_0 = spinning_about_y(0.0, 3600.0);
    let physics_info_1 = spinning_about_y(30.0, 0.0);

    for step in 0..=10 {
        let alpha = step as f32 / 10.0;
        let yaw_degrees = interpolated_yaw_degrees(&physics_info_0, &physics_info_1, alpha);
        // the arc between the two snapshots, which is all the poses actually support
        let expected_yaw_degrees = 30.0 * alpha;
        assert!(
            (yaw_degrees - expected_yaw_degrees).abs() <= 1e-3,
            "alpha {}: yaw {} degrees left the arc between the snapshots, expected {}",
            alpha,
            yaw_degrees,
            expected_yaw_degrees
        );
    }
}

// The endpoints are known poses, never predictions, so they stay exact whatever the velocities claim.
#[test]
fn hermite_lands_exactly_on_both_snapshots() {
    let physics_info_0 = spinning_about_y(0.0, 3600.0);
    let physics_info_1 = spinning_about_y(30.0, 0.0);

    let at_start = interpolate_between_physics_info_3Ds_hermite(&physics_info_0, &physics_info_1, SNAPSHOT_SPAN_SECS, 0.0);
    let at_end = interpolate_between_physics_info_3Ds_hermite(&physics_info_0, &physics_info_1, SNAPSHOT_SPAN_SECS, 1.0);

    assert!(at_start.location.quaternion.dot(physics_info_0.location.quaternion).abs() > 1.0 - 1e-6);
    assert!(at_end.location.quaternion.dot(physics_info_1.location.quaternion).abs() > 1.0 - 1e-6);
}

// Rapier's spin is in world space, so a tilted body spinning around the world's up axis keeps its tilt while it turns,
// both while blending between two snapshots and while being carried forward past the newest one. The body speeds up,
// since equal spins at both snapshots would cancel out of the blend's prediction.
#[test]
fn a_tilted_body_turns_around_its_world_spin_axis() {
    let tilt = Quat::from_rotation_x(90.0_f32.to_radians());
    let spinning_tilted = |yaw_degrees: f32, spin_degrees_per_sec: f32| {
        PhysicsInfo3D::new(
            Vec3::ZERO,
            Quat::from_rotation_y(yaw_degrees.to_radians()) * tilt,
            Vec3::ZERO,
            Vec3::new(0.0, spin_degrees_per_sec.to_radians(), 0.0),
        )
    };
    let physics_info_0 = spinning_tilted(0.0, 40.0);
    let physics_info_1 = spinning_tilted(3.0, 80.0);

    let blended = interpolate_between_physics_info_3Ds_hermite(&physics_info_0, &physics_info_1, SNAPSHOT_SPAN_SECS, 0.5);
    let expected_blended = Quat::from_rotation_y(1.0_f32.to_radians()) * tilt;
    assert!(
        blended.location.quaternion.dot(expected_blended).abs() > 1.0 - 1e-6,
        "blended {:?}, expected {:?}",
        blended.location.quaternion,
        expected_blended
    );

    let carried = extrapolate_physics_info_3D(&physics_info_1, SNAPSHOT_SPAN_SECS * 0.5);
    let expected_carried = Quat::from_rotation_y(5.0_f32.to_radians()) * tilt;
    assert!(
        carried.location.quaternion.dot(expected_carried).abs() > 1.0 - 1e-6,
        "carried {:?}, expected {:?}",
        carried.location.quaternion,
        expected_carried
    );
}
