mod helpers;

use game_types::object_types::{
    ClockInfo, ClockState, InterpolationStyle, Location3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectKind, ObjectLocation,
    PropertyAnimationInfo, PropertyAnimatorInfo, PropertyKeyframeInfo, SkJointInfo, TweenInfo,
    shared::{euler_degrees_to_quat, quat_to_euler_degrees},
};
use glam::Vec3;
use helpers::world_test_helpers::{add_brick, add_skeleton, make_world, make_world_controlled_by_player};
use render_core::computePropertyValue::computePropertyOverridesOfObjectIdAtTime;
use render_core::renderable::Renderable;
use shared::game::jsRuntime::runTemporaryScript;
use shared::game::world::World;

const DT_SECS: f32 = 1.0 / 60.0;
const BRICK_OBJECT_ID: ObjectId = 3;

fn path(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|field| field.to_string()).collect()
}

fn make_world_with_a_brick() -> anyhow::Result<World> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, BRICK_OBJECT_ID, root_object_id, Vec3::ZERO, false)?;
    // a Tween gets the next free id, so it must come after the ones picked here
    world.objectIdCounter = BRICK_OBJECT_ID;
    Ok(world)
}

fn brick_opacity(world: &World) -> anyhow::Result<f32> {
    let ObjectInfo::Brick(brick) = &world.getObject(BRICK_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("Expected the brick"));
    };
    Ok(brick.visualSettings.opacity)
}

fn tween_brick_opacity(world: &mut World, current_value: f32, target_value: f32, live_server_sim_ms: f64) -> anyhow::Result<ObjectId> {
    world.tweenTo(
        BRICK_OBJECT_ID,
        path(&["opacity"]),
        current_value,
        target_value,
        1000.0,
        InterpolationStyle::Linear,
        ObjectLocation::Server,
        live_server_sim_ms,
        None,
    )
}

fn has_tweens(world: &World) -> bool {
    world
        ._kindOfObjectId
        .get_keys(&ObjectKind::Tween)
        .is_some_and(|tween_ids| !tween_ids.is_empty())
}

fn assert_near(actual: f32, expected: f32, label: &str) {
    assert!((actual - expected).abs() < 1e-4, "{label}: expected {expected}, got {actual}");
}

// A tween is a Tween object at the world root, and the server deletes it once it has played out.
#[test]
fn a_tween_moves_a_number_to_its_target_and_is_deleted_when_it_ends() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    let tween_object_id = tween_brick_opacity(&mut world, 1.0, 0.0, 0.0)?;
    assert_eq!(world.parentOfId.get(&tween_object_id), Some(&world.getRootObjectId()));

    world.step_server(1, DT_SECS, 500.0)?;
    assert_near(brick_opacity(&world)?, 0.5, "halfway");

    world.step_server(1, DT_SECS, 1000.0)?;
    assert_near(brick_opacity(&world)?, 0.0, "at the end");
    assert!(!world.hasObject(tween_object_id), "a tween that has played out is deleted");

    world.step_server(1, DT_SECS, 1500.0)?;
    assert_near(brick_opacity(&world)?, 0.0, "after the end");
    Ok(())
}

// A PropertyAnimator holding the brick at opacity 0.8 forever.
fn add_opacity_animator(world: &mut World) {
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        10,
        "clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 1.0,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        11,
        "hold".to_string(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root_object_id,
    );
    for (keyframe_object_id, time) in [(12, 0.0), (13, 1.0)] {
        world.addObjectWithId(
            keyframe_object_id,
            format!("keyframe-{keyframe_object_id}"),
            ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time, value: 0.8 }),
            11,
        );
    }
    world.addObjectWithId(
        14,
        "opacity-animator".to_string(),
        ObjectBeingAdded::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: Some(ObjectIdPointer(BRICK_OBJECT_ID)),
            propertyPath: path(&["opacity"]),
            propertyAnimationId: Some(ObjectIdPointer(11)),
            clockId: Some(ObjectIdPointer(10)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );
}

#[test]
fn a_tween_wins_over_a_property_animator_until_it_ends() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    add_opacity_animator(&mut world);
    world.step_server(1, DT_SECS, 0.0)?;
    assert_near(brick_opacity(&world)?, 0.8, "the animator before the tween");

    let animated_opacity = brick_opacity(&world)?;
    tween_brick_opacity(&mut world, animated_opacity, 0.0, 0.0)?;
    world.step_server(1, DT_SECS, 500.0)?;
    assert_near(brick_opacity(&world)?, 0.4, "halfway from the animator's 0.8 to 0");

    world.step_server(1, DT_SECS, 1000.0)?;
    assert_near(brick_opacity(&world)?, 0.0, "the tween's target");

    world.step_server(1, DT_SECS, 1100.0)?;
    assert_near(brick_opacity(&world)?, 0.8, "the animator takes back over");
    Ok(())
}

// A new tween on a number that is already tweening changes that same Tween, and starts from where it is at that moment,
// not from the last value a step wrote.
#[test]
fn retargeting_a_tween_starts_from_wherever_it_has_got_to() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    let tween_object_id = tween_brick_opacity(&mut world, 1.0, 0.0, 0.0)?;
    world.step_server(1, DT_SECS, 500.0)?;

    // at 750 ms the old tween is at 0.25, while the last step wrote 0.5
    let written_opacity = brick_opacity(&world)?;
    let retargeted_tween_object_id = tween_brick_opacity(&mut world, written_opacity, 1.0, 750.0)?;
    assert_eq!(retargeted_tween_object_id, tween_object_id, "the same Tween object is changed");

    world.step_server(1, DT_SECS, 1250.0)?;
    assert_near(brick_opacity(&world)?, 0.625, "halfway from 0.25 to 1");

    world.step_server(1, DT_SECS, 1750.0)?;
    assert_near(brick_opacity(&world)?, 1.0, "the new target");
    Ok(())
}

// Head look turns joints, and a joint pitched past straight up must keep the axes nobody tweens and read back as written.
// A Tween moves the object its targetObjectId points at, wherever the Tween itself sits, and tweenTo finds it there.
#[test]
fn a_tween_moves_its_target_object_wherever_it_sits() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    let root_object_id = world.getRootObjectId();
    let tween_id = world.addObject(
        "Tween".to_string(),
        ObjectBeingAdded::Tween(TweenInfo {
            targetObjectId: Some(ObjectIdPointer(BRICK_OBJECT_ID)),
            propertyPath: path(&["opacity"]),
            fromValue: 1.0,
            toValue: 0.0,
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
        }),
        root_object_id,
    );

    world.step_server(1, DT_SECS, 500.0)?;
    assert_near(brick_opacity(&world)?, 0.5, "halfway, moved by a Tween under the root");
    assert_eq!(
        tween_brick_opacity(&mut world, 0.5, 1.0, 500.0)?,
        tween_id,
        "tweenTo reuses the Tween aimed at the same number"
    );
    Ok(())
}

#[test]
fn a_tween_turns_a_joint_past_straight_up_and_it_reads_back_as_written() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 2;
    let joint_object_id = 5;
    add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
    world.addObjectWithId(
        joint_object_id,
        "Neck".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::ZERO,
                quaternion: euler_degrees_to_quat(Vec3::new(0.0, 30.0, 0.0)),
            },
        }),
        skeleton_object_id,
    );
    world.objectIdCounter = joint_object_id;

    world.tweenTo(
        joint_object_id,
        path(&["restPose", "rotation", "x"]),
        0.0,
        170.0,
        1000.0,
        InterpolationStyle::Linear,
        ObjectLocation::Server,
        0.0,
        None,
    )?;
    world.step_server(1, DT_SECS, 600.0)?;
    world.step_server(1, DT_SECS, 1000.0)?;

    let ObjectInfo::SkJoint(sk_joint_info) = &world.getObject(joint_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected the joint"));
    };
    let expected_quaternion = euler_degrees_to_quat(Vec3::new(170.0, 30.0, 0.0));
    let stored_quaternion = sk_joint_info.restPose.quaternion;
    assert!(
        stored_quaternion.abs_diff_eq(expected_quaternion, 1e-5) || stored_quaternion.abs_diff_eq(-expected_quaternion, 1e-5),
        "expected (170, 30, 0) degrees, got {:?}",
        quat_to_euler_degrees(stored_quaternion)
    );
    let rotation_degrees = world
        .getRotationDegrees(joint_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected the joint's rotation"))?;
    assert!(
        (rotation_degrees - Vec3::new(170.0, 30.0, 0.0)).length() < 1e-3,
        "read back {rotation_degrees:?}"
    );
    Ok(())
}

// A tween writes its number every step, so one that can't would stop physics every step. It is refused up front instead.
#[test]
fn a_tween_on_something_that_is_not_a_writable_number_is_refused() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    for property_path in [path(&["id"]), path(&["parent"]), path(&["opacity", "x"])] {
        let tween_result = world.tweenTo(
            BRICK_OBJECT_ID,
            property_path.clone(),
            1.0,
            0.0,
            1000.0,
            InterpolationStyle::Linear,
            ObjectLocation::Server,
            0.0,
            None,
        );
        assert!(tween_result.is_err(), "{property_path:?} must be refused");
    }
    let negative_duration_result = world.tweenTo(
        BRICK_OBJECT_ID,
        path(&["opacity"]),
        1.0,
        0.0,
        -1.0,
        InterpolationStyle::Linear,
        ObjectLocation::Server,
        0.0,
        None,
    );
    assert!(negative_duration_result.is_err(), "a negative duration must be refused");

    assert!(!has_tweens(&world));
    world.step_server(1, DT_SECS, 500.0)?;
    Ok(())
}

// Clients delete a tween before they step, and one that is behind might never get the tween at all, so the number a tween
// ends on is sent like any other change when the server deletes it.
#[test]
fn a_tween_that_ends_sends_the_number_it_ended_on() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    let tween_object_id = tween_brick_opacity(&mut world, 1.0, 0.0, 0.0)?;
    world.step_server(1, DT_SECS, 500.0)?;
    world.flushDirtyObjects();

    world.step_server(1, DT_SECS, 1000.0)?;
    let dirty = world.flushDirtyObjects();
    assert!(dirty.removedObjectIds.contains(&tween_object_id));
    let flags = dirty
        .modifiedObjectFlags
        .get(&BRICK_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("the brick must be sent"))?;
    let ObjectInfo::Brick(sent_brick) = world
        .buildModifyDelta(BRICK_OBJECT_ID, flags)?
        .modifiedObjectInfo
        .ok_or_else(|| anyhow::anyhow!("Expected the brick's info"))?
    else {
        return Err(anyhow::anyhow!("Expected a brick"));
    };
    assert_near(sent_brick.visualSettings.opacity, 0.0, "the opacity sent");
    Ok(())
}

// A client gets a server tween as an object, plays it on the server's clock whatever its own clock says, and leaves the
// deleting to the server.
#[test]
fn a_client_plays_a_server_tween_from_its_object() -> anyhow::Result<()> {
    let mut server_world = make_world_with_a_brick()?;
    let tween_object_id = tween_brick_opacity(&mut server_world, 1.0, 0.0, 0.0)?;
    let tween_add_delta = server_world.buildAddDelta(tween_object_id)?;

    let mut client_world = make_world_controlled_by_player(99)?;
    let root_object_id = client_world.getRootObjectId();
    add_brick(&mut client_world, BRICK_OBJECT_ID, root_object_id, Vec3::ZERO, false)?;
    client_world.addObjectWithIdFromDelta(tween_object_id, tween_add_delta);

    client_world.step_physics(1, DT_SECS, 500.0, Some(9000.0))?;
    assert_near(brick_opacity(&client_world)?, 0.5, "the client's copy halfway");

    client_world.step_physics(1, DT_SECS, 1000.0, Some(9500.0))?;
    assert_near(brick_opacity(&client_world)?, 0.0, "the client's copy at the end");
    assert!(client_world.hasObject(tween_object_id), "only the server deletes its tweens");
    Ok(())
}

#[test]
fn deleting_a_tween_stops_it_where_it_is() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    let tween_object_id = tween_brick_opacity(&mut world, 1.0, 0.0, 0.0)?;
    world.step_server(1, DT_SECS, 500.0)?;

    world.removeObject(tween_object_id)?;
    world.step_server(1, DT_SECS, 1000.0)?;
    assert_near(brick_opacity(&world)?, 0.5, "where the tween was stopped");
    Ok(())
}

// The world writes a tween's value once per step, but the renderer draws between steps.
#[test]
fn the_renderer_draws_a_tween_at_the_frame_time() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;
    tween_brick_opacity(&mut world, 1.0, 0.0, 0.0)?;
    world.step_server(1, DT_SECS, 400.0)?;

    let render_snapshot = world.getRenderSnapshot();
    let propertyOverrideOfPathOfObjectId = computePropertyOverridesOfObjectIdAtTime(
        &render_snapshot.property_overrides_of_id,
        &render_snapshot.rotation_base_degrees_of_id,
        500.0,
        Some(500.0),
    );
    let drawn_opacity = propertyOverrideOfPathOfObjectId[&BRICK_OBJECT_ID][&path(&["opacity"])];
    assert_near(drawn_opacity, 0.5, "drawn halfway, between steps");
    Ok(())
}

// The editor never plays the game, so a temporary script's tween lands on its target when the script ends.
#[test]
fn a_tween_started_by_a_temporary_script_ends_at_its_target() -> anyhow::Result<()> {
    let mut world = make_world_with_a_brick()?;

    runTemporaryScript(
        &mut world,
        format!("game.animationService.tweenTo({BRICK_OBJECT_ID}, [\"opacity\"], 0.25, 1000);\n"),
    )?;

    assert_near(brick_opacity(&world)?, 0.25, "the target");
    assert!(!has_tweens(&world));
    Ok(())
}
