mod helpers;

use game_types::consts::DEFAULT_MAX_GROUND_SLOPE_DEGREES;
use game_types::object_types::{ObjectId, PhysicsInfo3D};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_ball, add_brick, add_rigid_group, make_world};
use shared::game::physics::SetPhysicsInfo3D;

const PLATFORM_OBJECT_ID: ObjectId = 200;
const BODY_OBJECT_ID: ObjectId = 201;
const DT_SECS: f32 = 1.0 / 60.0;

fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!((actual - expected).abs() <= 1e-4, "{label} mismatch: actual={actual}, expected={expected}");
}

#[test]
fn real_step_moves_kinematic_target_and_derives_its_velocity() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let target_velocity = Vec3::new(3.0, 0.0, 0.0);
    let target_angular_velocity_y = 1.5;

    add_brick(&mut world, PLATFORM_OBJECT_ID, root_object_id, Vec3::ZERO, false)?;
    world.setObjectNextPhysicsInfo3D(
        PLATFORM_OBJECT_ID,
        PhysicsInfo3D::new(
            target_velocity * DT_SECS,
            Quat::from_rotation_y(target_angular_velocity_y * DT_SECS),
            Vec3::ZERO,
            Vec3::ZERO,
        ),
    )?;

    world.step_physics(0, DT_SECS, 0.0, None)?;
    let before_real_step = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected platform physics"))?;
    // a kinematic object reads back as the pose it is headed for, so its position already shows the target here.
    // what proves nothing has been simulated is the velocity - rapier only derives that during a real step.
    assert_close(before_real_step.movement.velocity.x, 0.0, "derived velocity before real step");

    world.step_physics(1, DT_SECS, 0.0, None)?;
    let after_real_step = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected platform physics"))?;
    assert_close(after_real_step.location.position.x, target_velocity.x * DT_SECS, "position after real step");
    assert_close(after_real_step.movement.velocity.x, target_velocity.x, "derived linear velocity");
    assert_close(
        after_real_step.movement.angularVelocity.y,
        target_angular_velocity_y,
        "derived angular velocity",
    );
    Ok(())
}

#[test]
fn shape_queries_use_simulated_pose_until_pending_move_is_applied() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_brick(&mut world, PLATFORM_OBJECT_ID, root_id, Vec3::ZERO, false)?;
    add_brick(&mut world, BODY_OBJECT_ID, root_id, Vec3::Y * 3.0, false)?;
    let _ = world.physics.step(0.0);

    let target = Vec3::new(10.0, 3.0, 0.0);
    world.setPhysicsInfo3D(
        BODY_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(target),
            ..Default::default()
        },
        false,
    );
    let pose = world.getObjectPhysicsInfo3D(BODY_OBJECT_ID).expect("Expected body physics");
    assert_eq!(pose.location.position, target);
    let hit = world
        .physics
        .castObjectShapeInDirection(BODY_OBJECT_ID, Vec3::ZERO, Vec3::NEG_Y, 2.0, false, &|_| false)
        .expect("Expected the platform below the current simulated pose");
    assert_eq!(hit.object_id, PLATFORM_OBJECT_ID);
    assert_close(hit.distance, 1.0, "distance before pending move");

    world.step_physics(1, DT_SECS, DT_SECS as f64 * 1000.0, None)?;
    assert!(
        world
            .physics
            .castObjectShapeInDirection(BODY_OBJECT_ID, Vec3::ZERO, Vec3::NEG_Y, 2.0, false, &|_| false)
            .is_none()
    );
    Ok(())
}

#[test]
fn moving_kinematic_platform_carries_a_dynamic_body() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_brick(&mut world, PLATFORM_OBJECT_ID, root_object_id, Vec3::ZERO, false)?;
    add_ball(&mut world, BODY_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    for _ in 0..30 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    let support_hit = world
        .getGroundSupport(BODY_OBJECT_ID, 0.1, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
        .ok_or_else(|| anyhow::anyhow!("Expected body support"))?;
    assert_eq!(support_hit.object_id, PLATFORM_OBJECT_ID);

    let starting_body_x = world
        .getObjectPhysicsInfo3D(BODY_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected body physics"))?
        .location
        .position
        .x;
    let platform_velocity = Vec3::new(2.0, 0.0, 0.0);
    for step_index in 1..=60 {
        world.setObjectNextPhysicsInfo3D(
            PLATFORM_OBJECT_ID,
            PhysicsInfo3D::new(platform_velocity * (step_index as f32 * DT_SECS), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        )?;
        world.step_physics(1, DT_SECS, step_index as f64 * DT_SECS as f64 * 1000.0, None)?;
    }

    let final_body_x = world
        .getObjectPhysicsInfo3D(BODY_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected body physics"))?
        .location
        .position
        .x;
    assert!(
        final_body_x - starting_body_x > 0.25,
        "moving platform should carry the body, moved {}",
        final_body_x - starting_body_x
    );
    Ok(())
}

// A pushable group that has already been through a step can't turn into a Fixed body when it gets anchored parts: Rapier
// would keep it awake and re-place it through its center of mass every step, and these off-center parts make that land a
// rounding step away from where the group was put.
#[test]
fn an_awake_group_that_gets_anchored_parts_stays_exactly_where_it_is() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let group_object_id: ObjectId = 210;
    add_rigid_group(&mut world, group_object_id, root_object_id, Vec3::new(53.0, 2.0, 1.0), None)?;
    world.physics.update_collisions();

    add_brick(&mut world, 211, group_object_id, Vec3::new(50.0, 5.0, 4.0), false)?;
    add_brick(&mut world, 212, group_object_id, Vec3::new(55.0, 5.0, 2.0), false)?;
    world.forcePhysicsInfo3D(
        group_object_id,
        SetPhysicsInfo3D {
            rotation: Some(Quat::from_rotation_y(0.7)),
            ..Default::default()
        },
        false,
    );
    let placed_location = world.getObjectPhysicsInfo3D(group_object_id).expect("group has physics").location;

    for step_index in 1..=10 {
        world.physics.update_collisions();
        world.step_physics(1, DT_SECS, step_index as f64 * DT_SECS as f64 * 1000.0, None)?;
    }
    let location = world.getObjectPhysicsInfo3D(group_object_id).expect("group has physics").location;
    assert_eq!(location.position, placed_location.position);
    assert_eq!(location.quaternion, placed_location.quaternion);
    Ok(())
}
