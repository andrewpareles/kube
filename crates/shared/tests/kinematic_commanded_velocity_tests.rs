mod helpers;

use game_types::object_types::{
    ClockState, InterpolationStyle, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PhysicsInfo3D, PropertyAnimationInfo,
    PropertyKeyframeInfo,
};
use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{
    add_ball, add_brick, add_clock, add_player, add_property_animator, add_rigid_group, make_world, make_world_controlled_by_player, physics_info,
};
use shared::game::physics::{SetPhysicsInfo3D, SetPhysicsToggles3D};
use shared::game::world::World;

const PLATFORM_OBJECT_ID: ObjectId = 300;
const BODY_OBJECT_ID: ObjectId = 301;
const ANIMATOR_OBJECT_ID: ObjectId = 302;
const PLAYER_OBJECT_ID: ObjectId = 320;
const CHARACTER_OBJECT_ID: ObjectId = 321;
const DT_SECS: f32 = 1.0 / 60.0;

fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!((actual - expected).abs() <= 1e-3, "{label}: actual={actual}, expected={expected}");
}

fn set_velocity(world: &mut World, object_id: ObjectId, velocity: Vec3) {
    world.setPhysicsInfo3D(
        object_id,
        SetPhysicsInfo3D {
            velocity: Some(velocity),
            ..Default::default()
        },
        false,
    );
}

fn wide_unpushable_platform(world: &mut World, object_id: ObjectId, position: Vec3) -> anyhow::Result<()> {
    let root_object_id = world.getRootObjectId();
    add_brick(world, object_id, root_object_id, position, false)?;
    world.setPrimitiveSettings(
        object_id,
        PrimitiveSettings::Brick(BrickPrimitiveSettings {
            size: Vec3::new(20.0, 2.0, 20.0),
        }),
        true,
    )?;
    Ok(())
}

// A kinematic body has no intrinsic velocity, so before this existed setting one moved it at most once.
#[test]
fn commanded_velocity_keeps_a_kinematic_object_moving() -> anyhow::Result<()> {
    let mut world = make_world()?;
    wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;

    set_velocity(&mut world, PLATFORM_OBJECT_ID, Vec3::new(3.0, 0.0, 0.0));
    for _ in 0..60 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }

    let physics_info = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?;
    assert_close(physics_info.location.position.x, 3.0, "position after one second at 3 m/s");
    // rapier derives the velocity back out of the move, so a read matches what was written
    assert_close(physics_info.movement.velocity.x, 3.0, "reported velocity");
    Ok(())
}

#[test]
fn commanded_angular_velocity_keeps_a_kinematic_object_spinning() -> anyhow::Result<()> {
    let mut world = make_world()?;
    wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;

    let spin_radians_per_sec = 1.0;
    world.setPhysicsInfo3D(
        PLATFORM_OBJECT_ID,
        SetPhysicsInfo3D {
            angularVelocity: Some(Vec3::new(0.0, spin_radians_per_sec, 0.0)),
            ..Default::default()
        },
        false,
    );
    for _ in 0..60 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }

    let physics_info = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?;
    let yaw = physics_info.location.quaternion.to_scaled_axis().y;
    assert_close(yaw, spin_radians_per_sec, "yaw after one second at 1 rad/s");
    assert_close(physics_info.movement.angularVelocity.y, spin_radians_per_sec, "reported angular velocity");
    Ok(())
}

// The whole point of deriving velocity from the move: anything standing on it comes along. Measured against the same
// scene with a stationary platform, because the absolute distance is not the interesting part - the body here is a
// ball, so friction spins it up as much as it pushes it and it never rides the platform rigidly.
#[test]
fn commanded_velocity_carries_a_body_standing_on_the_object() -> anyhow::Result<()> {
    fn body_x_after_a_second_on_a_platform_moving_at(platform_velocity_x: f32) -> anyhow::Result<f32> {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;
        add_ball(&mut world, BODY_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;

        // let the ball settle onto the platform first, so we only measure what the platform's motion does
        for _ in 0..60 {
            world.step_physics(1, DT_SECS, 0.0, None)?;
        }
        let settled_x = world
            .getObjectPhysicsInfo3D(BODY_OBJECT_ID)
            .ok_or_else(|| anyhow::anyhow!("expected body physics"))?
            .location
            .position
            .x;

        set_velocity(&mut world, PLATFORM_OBJECT_ID, Vec3::new(platform_velocity_x, 0.0, 0.0));
        for step_index in 1..=60 {
            world.step_physics(1, DT_SECS, step_index as f64 * DT_SECS as f64 * 1000.0, None)?;
        }

        let body_x = world
            .getObjectPhysicsInfo3D(BODY_OBJECT_ID)
            .ok_or_else(|| anyhow::anyhow!("expected body physics"))?
            .location
            .position
            .x;
        Ok(body_x - settled_x)
    }

    let carried = body_x_after_a_second_on_a_platform_moving_at(2.0)?;
    let not_carried = body_x_after_a_second_on_a_platform_moving_at(0.0)?;

    // a body sitting on a still platform should barely drift, and the moving one should clearly follow it
    assert!(not_carried.abs() < 0.01, "body drifted on a still platform: {not_carried}");
    assert!(carried > 0.25, "body should be carried by the moving platform, moved {carried}");
    Ok(())
}

// Andrew's case: force-moving (teleporting) an object mid-flight must not cancel its commanded velocity.
#[test]
fn force_moving_a_kinematic_object_keeps_its_commanded_velocity() -> anyhow::Result<()> {
    let mut world = make_world()?;
    wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;

    set_velocity(&mut world, PLATFORM_OBJECT_ID, Vec3::new(3.0, 0.0, 0.0));
    for _ in 0..30 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }

    // teleport it far away mid-flight
    world.forcePhysicsInfo3D(
        PLATFORM_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(100.0, 0.0, 0.0)),
            ..Default::default()
        },
        false,
    );
    for _ in 0..60 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }

    let physics_info = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?;
    assert_close(physics_info.location.position.x, 103.0, "kept moving at 3 m/s from where it was teleported to");
    assert_close(physics_info.movement.velocity.x, 3.0, "reported velocity after the teleport");
    Ok(())
}

#[test]
fn commanded_velocity_of_zero_stops_the_object() -> anyhow::Result<()> {
    let mut world = make_world()?;
    wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;

    set_velocity(&mut world, PLATFORM_OBJECT_ID, Vec3::new(3.0, 0.0, 0.0));
    for _ in 0..30 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    set_velocity(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO);
    let stopped_at_x = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?
        .location
        .position
        .x;

    for _ in 0..60 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }

    let physics_info = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?;
    assert_close(physics_info.location.position.x, stopped_at_x, "must not drift after being told to stop");
    assert_close(physics_info.movement.velocity.x, 0.0, "reported velocity after stopping");
    Ok(())
}

// A PropertyAnimator repeatedly writes the pose, so it has to get the last word over a commanded velocity.
#[test]
fn an_animator_wins_over_a_commanded_velocity() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id: ObjectId = 303;
    wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, ANIMATOR_OBJECT_ID, root_object_id)?;
    {
        let clock_object = world.getObjectMut(clock_object_id)?;
        let ObjectInfo::Clock(clock) = &mut clock_object.info else {
            return Err(anyhow::anyhow!("expected clock object"));
        };
        clock.clockState = ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Server,
        };
    }
    {
        let property_animator_object = world.getObjectMut(ANIMATOR_OBJECT_ID)?;
        let ObjectInfo::PropertyAnimator(property_animator) = &mut property_animator_object.info else {
            return Err(anyhow::anyhow!("expected property animator object"));
        };
        property_animator.targetObjectId = Some(ObjectIdPointer(PLATFORM_OBJECT_ID));
        property_animator.propertyPath = ["location", "position", "x"].iter().map(|part| part.to_string()).collect();
        property_animator.clockId = Some(ObjectIdPointer(clock_object_id));
        property_animator.priority = 5;
        property_animator.weight = 100.0;
    }
    // this test assigns object ids by hand, so keep the animation objects in their own id range
    let property_animation_id = ANIMATOR_OBJECT_ID * 1000;
    world.addObjectWithId(
        property_animation_id,
        "platform-animation".to_string(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root_object_id,
    );
    for (keyframe_index, keyframe) in [PropertyKeyframeInfo { time: 0.0, value: 10.0 }, PropertyKeyframeInfo { time: 1.0, value: 20.0 }]
        .into_iter()
        .enumerate()
    {
        world.addObjectWithId(
            property_animation_id + 1 + keyframe_index as ObjectId,
            format!("platform-keyframe-{keyframe_index}"),
            ObjectBeingAdded::PropertyKeyframe(keyframe),
            property_animation_id,
        );
    }
    {
        let property_animator_object = world.getObjectMut(ANIMATOR_OBJECT_ID)?;
        let ObjectInfo::PropertyAnimator(property_animator) = &mut property_animator_object.info else {
            return Err(anyhow::anyhow!("expected property animator object"));
        };
        property_animator.propertyAnimationId = Some(ObjectIdPointer(property_animation_id));
    }

    // a commanded 50 m/s would run x far past the animator's 10..20 curve if it were not overridden
    set_velocity(&mut world, PLATFORM_OBJECT_ID, Vec3::new(50.0, 0.0, 0.0));
    world.step_server(1, DT_SECS, 500.0)?;

    let position_x = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?
        .location
        .position
        .x;
    assert_close(position_x, 15.0, "animator owns the animated axis");
    Ok(())
}

// setObjectPushable(true) turns the body dynamic; it should keep coasting rather than stop dead.
#[test]
fn making_it_pushable_transfers_the_commanded_velocity_to_the_dynamic_body() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, BODY_OBJECT_ID, root_object_id, Vec3::new(0.0, 50.0, 0.0), false)?;

    set_velocity(&mut world, BODY_OBJECT_ID, Vec3::new(4.0, 0.0, 0.0));
    world.step_physics(1, DT_SECS, 0.0, None)?;

    world.setPhysicsToggles3D(
        BODY_OBJECT_ID,
        SetPhysicsToggles3D {
            isPushable: Some(true),
            ..Default::default()
        },
        false,
    );

    let physics_info = world
        .getObjectPhysicsInfo3D(BODY_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected body physics"))?;
    assert_close(physics_info.movement.velocity.x, 4.0, "velocity carried onto the now-dynamic body");
    Ok(())
}

// A subitem has no rigidbody of its own - its group owns the motion - so a velocity write is still ignored.
#[test]
fn a_subitem_velocity_write_is_still_ignored() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let group_object_id: ObjectId = 310;
    let subitem_object_id: ObjectId = 311;
    add_rigid_group(&mut world, group_object_id, root_object_id, Vec3::new(0.0, 5.0, 0.0), None)?;
    add_ball(&mut world, subitem_object_id, group_object_id, Vec3::new(0.0, 5.0, 0.0), true)?;

    set_velocity(&mut world, subitem_object_id, Vec3::new(9.0, 0.0, 0.0));
    let before_x = world
        .getObjectPhysicsInfo3D(subitem_object_id)
        .ok_or_else(|| anyhow::anyhow!("expected subitem physics"))?
        .location
        .position
        .x;
    for _ in 0..30 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    let after_x = world
        .getObjectPhysicsInfo3D(subitem_object_id)
        .ok_or_else(|| anyhow::anyhow!("expected subitem physics"))?
        .location
        .position
        .x;
    assert_close(after_x, before_x, "a subitem must not move from its own velocity write");
    Ok(())
}

// Streamed poses are authoritative: they set a target directly and must not register a commanded velocity,
// or the object would integrate locally on top of the positions the server already sent.
#[test]
fn a_streamed_pose_does_not_register_a_commanded_velocity() -> anyhow::Result<()> {
    let mut world = make_world()?;
    wide_unpushable_platform(&mut world, PLATFORM_OBJECT_ID, Vec3::ZERO)?;

    world.setObjectNextPhysicsInfo3D(
        PLATFORM_OBJECT_ID,
        PhysicsInfo3D::new(Vec3::new(0.5, 0.0, 0.0), Quat::IDENTITY, Vec3::new(30.0, 0.0, 0.0), Vec3::ZERO),
    )?;
    world.step_physics(1, DT_SECS, 0.0, None)?;
    let after_first_step_x = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?
        .location
        .position
        .x;
    assert_close(after_first_step_x, 0.5, "moved to the streamed target");

    // no further stream: it holds still rather than coasting at the streamed 30 m/s
    for _ in 0..30 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    let held_x = world
        .getObjectPhysicsInfo3D(PLATFORM_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected platform physics"))?
        .location
        .position
        .x;
    assert_close(held_x, 0.5, "a streamed velocity must not keep integrating locally");
    Ok(())
}

// Steps the copy of the character `num_steps` times, streaming the player standing at `streamed_position` if there is one.
fn copy_position_after_steps(world: &mut World, streamed_position: Option<Vec3>, num_steps: usize) -> anyhow::Result<Vec3> {
    for _ in 0..num_steps {
        if let Some(position) = streamed_position {
            world.setObjectNextPhysicsInfo3D(CHARACTER_OBJECT_ID, physics_info(position))?;
        }
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    let character_physics = world
        .getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("expected character physics"))?;
    Ok(character_physics.location.position)
}

// After a teleport the copy waits at the teleport spot until the player's movement arrives, follows it, and holds
// still while it is late.
fn assert_copy_follows_only_the_player(world: &mut World, spawn_position: Vec3) -> anyhow::Result<()> {
    let waiting_position = copy_position_after_steps(world, None, 10)?;
    assert!(
        (waiting_position - spawn_position).length() < 1e-3,
        "the copy must wait at the teleport spot, not keep falling: {waiting_position:?}"
    );

    let landed_position = spawn_position - Vec3::new(0.0, 0.5, 0.0);
    let streamed_position = copy_position_after_steps(world, Some(landed_position), 10)?;
    assert!(
        (streamed_position - landed_position).length() < 1e-3,
        "the copy must sit where the player's movement says: {streamed_position:?}"
    );

    let late_position = copy_position_after_steps(world, None, 10)?;
    assert!(
        (late_position - landed_position).length() < 1e-3,
        "the copy must hold still while the player's movement is late: {late_position:?}"
    );
    Ok(())
}

// The server's copy of a player's character is kinematic and follows the player's packets. A script teleporting it
// stored the velocity it had at that moment, and the copy kept moving at it: it fell through whatever was under the
// spawn while the server waited for the player to confirm the teleport, then sat a step off the player for good.
#[test]
fn a_teleport_does_not_leave_the_servers_copy_of_a_player_drifting() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::ZERO, true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    assert_eq!(
        world.getIsObjectControlledByUs(CHARACTER_OBJECT_ID),
        Some(false),
        "the server mirrors a player's character"
    );

    // the player's packets say it is falling at 50 m/s
    let falling_velocity = Vec3::new(0.0, -50.0, 0.0);
    for step_index in 1..=10 {
        let position = falling_velocity * (step_index as f32 * DT_SECS);
        world.setObjectNextPhysicsInfo3D(CHARACTER_OBJECT_ID, PhysicsInfo3D::new(position, Quat::IDENTITY, falling_velocity, Vec3::ZERO))?;
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }

    // a script respawns it, which is what `character.location.position = ...` does
    let spawn_position = Vec3::new(0.0, 10.0, 0.0);
    world.forcePhysicsInfo3D(
        CHARACTER_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(spawn_position),
            ..Default::default()
        },
        false,
    );
    assert_copy_follows_only_the_player(&mut world, spawn_position)
}

// Other clients apply the server's teleport the same way, with the velocity the server reported for the character.
#[test]
fn a_teleport_does_not_leave_another_clients_copy_of_a_player_drifting() -> anyhow::Result<()> {
    let our_player_object_id: ObjectId = 322;
    let mut world = make_world_controlled_by_player(our_player_object_id)?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::ZERO, true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    assert_eq!(
        world.getIsObjectControlledByUs(CHARACTER_OBJECT_ID),
        Some(false),
        "another player's character is mirrored"
    );

    let spawn_position = Vec3::new(0.0, 10.0, 0.0);
    world.forcePhysicsInfo3D(
        CHARACTER_OBJECT_ID,
        SetPhysicsInfo3D::from(PhysicsInfo3D::new(spawn_position, Quat::IDENTITY, Vec3::new(0.0, -50.0, 0.0), Vec3::ZERO)),
        false,
    );
    assert_copy_follows_only_the_player(&mut world, spawn_position)
}
