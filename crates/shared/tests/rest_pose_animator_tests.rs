mod helpers;

use game_types::object_types::{
    BrickInfo, ClockInfo, ClockState, InterpolationStyle, Location3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation,
    PropertyAnimationInfo, PropertyAnimatorInfo, PropertyKeyframeInfo, RigidGroupInfo, SkAttachmentInfo, SkJointInfo, SkeletonInfo, VisualSettings,
    shared::{euler_degrees_to_quat, quat_to_euler_degrees},
};
use game_types::physics_primitive_types::BrickPrimitiveSettings;
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{make_world, physics_info, physics_toggles};
use render_core::computePropertyValue::{applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime};
use render_core::computeSkeletonPose::computeSkeletonPoseOfIdAtTime;
use render_core::render_scene::computeRenderObjectTransform;
use render_core::renderable::Renderable;
use shared::game::world::World;

const SKELETON_OBJECT_ID: ObjectId = 10;
const ARM_JOINT_OBJECT_ID: ObjectId = 11;
const RIGID_GROUP_OBJECT_ID: ObjectId = 20;
const ARM_PART_OBJECT_ID: ObjectId = 21;
const ARM_ANIMATOR_OBJECT_ID: ObjectId = 34;
const ARM_REST_POSITION: Vec3 = Vec3::new(0.0, 1.0, 0.0);

// A PropertyAnimator moving the joint's rest pose up from y=1 to y=3 over one second.
fn make_world_raising_the_arm() -> anyhow::Result<World> {
    make_world_with_an_animated_rest_pose(Quat::IDENTITY, ["restPose", "position", "y"], 1.0, 3.0)
}

// A Skeleton with one joint, a part following it, and a PropertyAnimator moving one of the joint's rest pose values
// from `from_value` to `to_value` over one second.
fn make_world_with_an_animated_rest_pose(arm_rest_rotation: Quat, animated_property_path: [&str; 3], from_value: f32, to_value: f32) -> anyhow::Result<World> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SKELETON_OBJECT_ID,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        ARM_JOINT_OBJECT_ID,
        "Arm".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: ARM_REST_POSITION,
                quaternion: arm_rest_rotation,
            },
        }),
        SKELETON_OBJECT_ID,
    );
    world.addObjectWithId(
        RIGID_GROUP_OBJECT_ID,
        "body".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(SKELETON_OBJECT_ID)),
            },
            physics_info(Vec3::ZERO),
        ),
        root_object_id,
    );
    world.addObjectWithId(
        ARM_PART_OBJECT_ID,
        "Arm".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings { size: Vec3::ONE },
            physics_info(ARM_REST_POSITION),
            physics_toggles(false, 1.0),
        ),
        RIGID_GROUP_OBJECT_ID,
    );
    world.addObjectWithId(
        22,
        "arm-attachment".to_string(),
        ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
            objectId: Some(ObjectIdPointer(ARM_PART_OBJECT_ID)),
            jointName: "Arm".to_string(),
            offsetFromJoint: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        root_object_id,
    );
    world.addObjectWithId(
        30,
        "clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: false,
            speed: 1.0,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        31,
        "raise-arm".to_string(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root_object_id,
    );
    for (keyframe_object_id, time, value) in [(32, 0.0, from_value), (33, 1.0, to_value)] {
        world.addObjectWithId(
            keyframe_object_id,
            format!("keyframe-{keyframe_object_id}"),
            ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time, value }),
            31,
        );
    }
    world.addObjectWithId(
        ARM_ANIMATOR_OBJECT_ID,
        "arm-animator".to_string(),
        ObjectBeingAdded::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: Some(ObjectIdPointer(ARM_JOINT_OBJECT_ID)),
            propertyPath: animated_property_path.map(str::to_string).to_vec(),
            propertyAnimationId: Some(ObjectIdPointer(31)),
            clockId: Some(ObjectIdPointer(30)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );
    Ok(world)
}

fn stored_arm_rest_pose(world: &World) -> anyhow::Result<Location3D> {
    let ObjectInfo::SkJoint(sk_joint_info) = &world.getObject(ARM_JOINT_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("Expected the arm joint"));
    };
    Ok(sk_joint_info.restPose.clone())
}

fn assert_vec3_near(actual: Vec3, expected: Vec3, label: &str) {
    assert!((actual - expected).length() < 1e-4, "{label}: expected {expected:?}, got {actual:?}");
}

// q and -q are the same turn
fn assert_rotation_near(actual: Quat, expected: Quat, label: &str) {
    assert!(
        actual.abs_diff_eq(expected, 1e-5) || actual.abs_diff_eq(-expected, 1e-5),
        "{label}: expected {:?} degrees, got {:?} degrees",
        quat_to_euler_degrees(expected),
        quat_to_euler_degrees(actual)
    );
}

#[test]
fn an_animated_rest_pose_is_written_and_moves_the_parts_following_the_joint_in_the_same_step() -> anyhow::Result<()> {
    let mut world = make_world_raising_the_arm()?;

    world.step_physics(1, 1.0 / 60.0, 1000.0, None)?;

    assert_vec3_near(stored_arm_rest_pose(&world)?.position, Vec3::new(0.0, 3.0, 0.0), "arm joint");
    let arm_part_position = world
        .getObjectPhysicsInfo3D(ARM_PART_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected the arm part's physics"))?
        .location
        .position;
    assert_vec3_near(arm_part_position, Vec3::new(0.0, 3.0, 0.0), "arm part");
    Ok(())
}

#[test]
fn an_animated_rest_pose_keeps_its_last_value_after_the_animator_is_removed() -> anyhow::Result<()> {
    let mut world = make_world_raising_the_arm()?;
    world.step_physics(1, 1.0 / 60.0, 1000.0, None)?;

    world.removeObject(ARM_ANIMATOR_OBJECT_ID)?;
    world.step_physics(1, 1.0 / 60.0, 1100.0, None)?;

    assert_vec3_near(stored_arm_rest_pose(&world)?.position, Vec3::new(0.0, 3.0, 0.0), "arm joint");
    Ok(())
}

// The joint rests turned 30 degrees around y, and the animator pitches it past straight up, where decomposing the
// rotation it just wrote would hand back y and z turned by 180.
#[test]
fn an_animated_rest_rotation_past_straight_up_keeps_the_axes_nobody_animates() -> anyhow::Result<()> {
    let mut world = make_world_with_an_animated_rest_pose(euler_degrees_to_quat(Vec3::new(0.0, 30.0, 0.0)), ["restPose", "rotation", "x"], 0.0, 170.0)?;

    world.step_physics(1, 1.0 / 60.0, 600.0, None)?;
    world.step_physics(1, 1.0 / 60.0, 1000.0, None)?;

    assert_rotation_near(
        stored_arm_rest_pose(&world)?.quaternion,
        euler_degrees_to_quat(Vec3::new(170.0, 30.0, 0.0)),
        "arm joint",
    );
    Ok(())
}

// Scripts read an animated rest rotation as the angles its animator wrote, like an animated location.rotation, not as a
// decomposition that reports 170 degrees of pitch as 10 with y and z turned by 180.
#[test]
fn an_animated_rest_rotation_reads_back_as_the_animated_angles() -> anyhow::Result<()> {
    let mut world = make_world_with_an_animated_rest_pose(euler_degrees_to_quat(Vec3::new(0.0, 30.0, 0.0)), ["restPose", "rotation", "x"], 0.0, 170.0)?;

    world.step_physics(1, 1.0 / 60.0, 1000.0, None)?;

    let rotation_degrees = world
        .getRotationDegrees(ARM_JOINT_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected the arm joint's rotation"))?;
    assert_vec3_near(rotation_degrees, Vec3::new(170.0, 30.0, 0.0), "animated rest rotation read back");
    Ok(())
}

#[test]
fn an_animated_rest_pose_shows_where_the_renderer_draws_the_parts() -> anyhow::Result<()> {
    let mut world = make_world_raising_the_arm()?;
    // the world last wrote the rest pose at 400 ms, and the renderer draws at 500 ms
    world.step_physics(1, 1.0 / 60.0, 400.0, None)?;
    let render_snapshot = world.getRenderSnapshot();
    let mut render_object_of_id = render_snapshot.render_object_of_id;

    // the same order the renderers use: overrides first, then poses
    let propertyOverrideOfPathOfObjectId = computePropertyOverridesOfObjectIdAtTime(
        &render_snapshot.property_overrides_of_id,
        &render_snapshot.rotation_base_degrees_of_id,
        500.0,
        Some(500.0),
    );
    applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &propertyOverrideOfPathOfObjectId, &render_snapshot.pivot_offset_of_id);
    let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(&render_object_of_id, 500.0, Some(500.0));

    let arm_part_matrix = computeRenderObjectTransform(&render_object_of_id, &skeleton_pose_of_id, ARM_PART_OBJECT_ID)
        .ok_or_else(|| anyhow::anyhow!("Expected the arm part's render transform"))?;
    assert_vec3_near(arm_part_matrix.w_axis.truncate(), Vec3::new(0.0, 2.0, 0.0), "drawn arm part halfway through");
    Ok(())
}
