mod helpers;

use game_types::consts::{DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES};
use game_types::object_types::{ObjectId, PhysicsInfo3D};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_ball, add_brick, add_rigid_group, make_world_controlled_by_player};
use shared::game::physics::SetPhysicsInfo3D;

const PLAYER_OBJECT_ID: ObjectId = 90;
const SPINNING_PLATFORM_OBJECT_ID: ObjectId = 92;
const DT_SECS: f32 = 1.0 / 60.0;
const SPIN_RADIANS_PER_SEC: f32 = 1.0; // deliberately not 60 deg/s, so a stray deg<->rad conversion shows up

#[test]
fn velocity_relative_to_ground_removes_rotating_support_motion() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let offset_body_object_id = 93;
    let left_foot_object_id = 94;
    let right_foot_object_id = 95;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, SPINNING_PLATFORM_OBJECT_ID, root_object_id, Vec3::ZERO, false)?;

    // A multi-foot character's center of mass and whichever supporting contact gets selected are
    // not vertically aligned. This reproduces the offset-foot case where contact-point subtraction
    // left a false tangential walking speed.
    add_rigid_group(&mut world, offset_body_object_id, root_object_id, Vec3::new(0.0, 2.0, 0.0), None)?;
    add_ball(&mut world, left_foot_object_id, offset_body_object_id, Vec3::new(-0.5, 2.0, 0.0), true)?;
    add_ball(&mut world, right_foot_object_id, offset_body_object_id, Vec3::new(0.5, 2.0, 0.0), true)?;
    let _ = world.physics.step(0.0);
    world.setObjectNextPhysicsInfo3D(
        SPINNING_PLATFORM_OBJECT_ID,
        PhysicsInfo3D::new(Vec3::ZERO, Quat::from_rotation_y(SPIN_RADIANS_PER_SEC * DT_SECS), Vec3::ZERO, Vec3::ZERO),
    )?;
    world.step_physics(1, DT_SECS, 0.0, None)?;

    world.forcePhysicsInfo3D(
        offset_body_object_id,
        SetPhysicsInfo3D {
            velocity: Some(Vec3::ZERO),
            angularVelocity: Some(Vec3::ZERO),
            ..Default::default()
        },
        false,
    );

    let relative_velocity = world
        .getVelocityRelativeToGround(offset_body_object_id, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
        .ok_or_else(|| anyhow::anyhow!("Expected relative velocity"))?;
    assert!(relative_velocity.length() <= 0.01, "relative velocity: {relative_velocity:?}");
    Ok(())
}
