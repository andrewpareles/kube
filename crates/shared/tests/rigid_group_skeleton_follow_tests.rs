mod helpers;

use game_types::object_types::sk_animator::SkAnimatorInfo;
use game_types::object_types::{
    BallInfo, BrickInfo, ClockInfo, ClockState, InterpolationStyle, Location3D, Movement3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectLocation,
    PhysicsInfo3D, PhysicsToggles3D, RigidGroupInfo, SkAnimationInfo, SkAttachmentInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo, SkeletonInfo,
    VisualSettings,
};
use game_types::physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, PrimitiveSettings};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_brick, add_player, make_world, make_world_controlled_by_player, physics_info};
use shared::game::physics::SetOrForce;

const DT_SECS: f32 = 1.0 / 60.0;

fn add_sk_attachment(
    world: &mut shared::game::world::World,
    object_id: ObjectId,
    parent_id: ObjectId,
    attached_object_id: ObjectId,
    joint_name: &str,
    offset_from_joint: Location3D,
) {
    world.addObjectWithId(
        object_id,
        format!("joint-attachment-{object_id}"),
        ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
            objectId: Some(ObjectIdPointer(attached_object_id)),
            jointName: joint_name.to_string(),
            offsetFromJoint: offset_from_joint,
        }),
        parent_id,
    );
}

fn add_sk_keyframe_with_pose(
    world: &mut shared::game::world::World,
    keyframe_object_id: ObjectId,
    pose_object_id: ObjectId,
    animation_object_id: ObjectId,
    time: f32,
    joint_name: &str,
    local_pose: Location3D,
) {
    world.addObjectWithId(
        keyframe_object_id,
        format!("keyframe-{keyframe_object_id}"),
        ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time }),
        animation_object_id,
    );
    world.addObjectWithId(
        pose_object_id,
        format!("pose-{pose_object_id}"),
        ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
            jointName: joint_name.to_string(),
            localPose: local_pose,
        }),
        keyframe_object_id,
    );
}

#[test]
fn rigid_group_child_follows_attachment_offset_on_server_step() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 9100;
    let rigid_group_object_id = 9200;
    let child_object_id = 9201;

    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        9101,
        "hand".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(1.0, 0.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );

    world.addObjectWithId(
        rigid_group_object_id,
        "rigid-group".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
        ),
        root_object_id,
    );
    world.addObjectWithId(
        child_object_id,
        "payload".to_string(),
        ObjectBeingAdded::Ball(
            BallInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BallPrimitiveSettings { radius: 1.0 },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        rigid_group_object_id,
    );
    add_sk_attachment(
        &mut world,
        9202,
        root_object_id,
        child_object_id,
        "hand",
        Location3D {
            position: Vec3::new(0.0, 2.0, 0.0),
            quaternion: Quat::IDENTITY,
        },
    );

    world.step_server(0, 0.0, 0.0)?;

    let child_physics_info_3d = world
        .getObjectPhysicsInfo3D(child_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected rigid-group child physics info"))?;
    assert_eq!(child_physics_info_3d.location.position, Vec3::new(1.0, 2.0, 0.0));
    assert_eq!(child_physics_info_3d.location.quaternion, Quat::IDENTITY);

    Ok(())
}

// Moving a rigid group must carry its posed parts with it: SkKeyframePoses are relative to the group,
// not fixed world positions.
#[test]
fn rigid_group_carries_its_pose_when_it_moves() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 9950;
    let hand_joint_object_id = 9951;
    let rigid_group_object_id = 9952;
    let child_object_id = 9953;

    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        hand_joint_object_id,
        "hand".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(1.0, 0.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        rigid_group_object_id,
        "rigid-group".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::new(5.0, 2.0, 0.0),
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
        ),
        root_object_id,
    );
    world.addObjectWithId(
        child_object_id,
        "payload".to_string(),
        ObjectBeingAdded::Ball(
            BallInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BallPrimitiveSettings { radius: 1.0 },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::new(5.0, 2.0, 0.0),
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        rigid_group_object_id,
    );
    add_sk_attachment(
        &mut world,
        9954,
        root_object_id,
        child_object_id,
        "hand",
        Location3D {
            position: Vec3::new(0.0, 2.0, 0.0),
            quaternion: Quat::IDENTITY,
        },
    );

    // group (5,2,0) * hand bind (1,0,0) * offset (0,2,0) = (6,4,0)
    world.step_server(0, 0.0, 0.0)?;
    let child_physics_info_3d = world
        .getObjectPhysicsInfo3D(child_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected rigid-group child physics info"))?;
    assert_eq!(child_physics_info_3d.location.position, Vec3::new(6.0, 4.0, 0.0));

    // Move the group: the posed child must follow instead of snapping back.
    world.setObjectPositionOffsetRecursive(rigid_group_object_id, Vec3::new(-2.0, 1.0, 3.0), SetOrForce::Force)?;
    world.step_server(0, 0.0, 0.0)?;
    let child_physics_info_3d = world
        .getObjectPhysicsInfo3D(child_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected rigid-group child physics info after moving"))?;
    assert_eq!(child_physics_info_3d.location.position, Vec3::new(4.0, 5.0, 3.0));

    Ok(())
}

#[test]
fn rigid_group_child_follows_client_and_server_clock_references() -> anyhow::Result<()> {
    fn assert_vec3_near(actual: Vec3, expected: Vec3) {
        let delta = actual - expected;
        assert!(delta.length() < 0.001, "expected {expected:?}, got {actual:?}, delta {delta:?}");
    }

    let mut world = make_world_controlled_by_player(999)?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 9300;
    let root_joint_object_id = 9301;
    let hand_joint_object_id = 9302;
    let rigid_group_object_id = 9400;
    let child_object_id = 9401;
    let clock_object_id = 9500;
    let sk_animation_object_id = 9501;
    let sk_animator_object_id = 9502;

    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        root_joint_object_id,
        "Root".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        hand_joint_object_id,
        "hand".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        root_joint_object_id,
    );
    world.addObjectWithId(
        rigid_group_object_id,
        "rigid-group".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
        ),
        root_object_id,
    );
    world.addObjectWithId(
        child_object_id,
        "hand".to_string(),
        ObjectBeingAdded::Ball(
            BallInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BallPrimitiveSettings { radius: 1.0 },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        rigid_group_object_id,
    );
    add_sk_attachment(
        &mut world,
        9507,
        root_object_id,
        child_object_id,
        "hand",
        Location3D {
            position: Vec3::new(1.0, 0.0, 0.0),
            quaternion: Quat::IDENTITY,
        },
    );
    world.addObjectWithId(
        clock_object_id,
        "clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Client,
            },
            shouldLoop: false,
            speed: 1.0,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        sk_animation_object_id,
        "animation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Step,
        }),
        root_object_id,
    );
    add_sk_keyframe_with_pose(
        &mut world,
        9503,
        9504,
        sk_animation_object_id,
        0.0,
        "hand",
        Location3D {
            position: Vec3::ZERO,
            quaternion: Quat::IDENTITY,
        },
    );
    add_sk_keyframe_with_pose(
        &mut world,
        9505,
        9506,
        sk_animation_object_id,
        1.0,
        "hand",
        Location3D {
            position: Vec3::ZERO,
            quaternion: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        },
    );
    world.addObjectWithId(
        sk_animator_object_id,
        "animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            clockId: Some(ObjectIdPointer(clock_object_id)),
            skAnimationId: Some(ObjectIdPointer(sk_animation_object_id)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );

    world.step_physics(0, 0.0, 0.0, Some(1000.0))?;

    let child_physics_info_3d = world
        .getObjectPhysicsInfo3D(child_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected rigid-group child physics info"))?;
    assert_vec3_near(child_physics_info_3d.location.position, Vec3::new(0.0, 1.0, 0.0));

    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: false,
            speed: 1.0,
        },
    )?;
    world.step_physics(0, 0.0, 1000.0, None)?;

    let child_physics_info_3d = world
        .getObjectPhysicsInfo3D(child_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected rigid-group child physics info after server-time clock"))?;
    assert_vec3_near(child_physics_info_3d.location.position, Vec3::new(0.0, 1.0, 0.0));

    Ok(())
}

#[test]
fn rigid_group_skeleton_follow_updates_raycast_queries_on_physics_step() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 9600;
    let root_joint_object_id = 9601;
    let surface_joint_object_id = 9602;
    let rigid_group_object_id = 9700;
    let child_object_id = 9701;
    let clock_object_id = 9800;
    let sk_animation_object_id = 9801;
    let sk_animator_object_id = 9802;
    let ball_object_id = 9900;

    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        root_joint_object_id,
        "Root".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        surface_joint_object_id,
        "surface".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(0.0, -100.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        root_joint_object_id,
    );
    world.addObjectWithId(
        rigid_group_object_id,
        "rigid-group".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
        ),
        root_object_id,
    );
    world.addObjectWithId(
        child_object_id,
        "surface".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings {
                size: Vec3::new(2.0, 2.0, 2.0),
            },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::new(0.0, -100.0, 0.0),
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        rigid_group_object_id,
    );
    add_sk_attachment(
        &mut world,
        9807,
        root_object_id,
        child_object_id,
        "surface",
        Location3D {
            position: Vec3::ZERO,
            quaternion: Quat::IDENTITY,
        },
    );
    world.addObjectWithId(
        clock_object_id,
        "clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Client,
            },
            shouldLoop: false,
            speed: 1.0,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        sk_animation_object_id,
        "animation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Step,
        }),
        root_object_id,
    );
    add_sk_keyframe_with_pose(
        &mut world,
        9803,
        9804,
        sk_animation_object_id,
        0.0,
        "surface",
        Location3D {
            position: Vec3::ZERO,
            quaternion: Quat::IDENTITY,
        },
    );
    add_sk_keyframe_with_pose(
        &mut world,
        9805,
        9806,
        sk_animation_object_id,
        1.0,
        "surface",
        Location3D {
            position: Vec3::new(0.0, 100.0, 0.0),
            quaternion: Quat::IDENTITY,
        },
    );
    world.addObjectWithId(
        sk_animator_object_id,
        "animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            clockId: Some(ObjectIdPointer(clock_object_id)),
            skAnimationId: Some(ObjectIdPointer(sk_animation_object_id)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        ball_object_id,
        "ball".to_string(),
        ObjectBeingAdded::Ball(
            BallInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BallPrimitiveSettings { radius: 1.0 },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::new(0.0, 1.75, 0.0),
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: true,
                density: 1.0,
            },
        ),
        root_object_id,
    );

    world.step_physics(1, 1.0 / 60.0, 0.0, Some(1000.0))?;

    let child_physics_info_3d = world
        .getObjectPhysicsInfo3D(child_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected rigid-group child physics info"))?;
    assert_eq!(child_physics_info_3d.location.position, Vec3::ZERO);

    let collider_hit = world.castRay(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y, 10.0, false, &|id| id == ball_object_id);
    assert!(
        collider_hit.is_some_and(|hit| hit.object_id == child_object_id),
        "The posed rigid-group child collider must be queryable on the same physics step"
    );

    Ok(())
}

// A rigid group following a one-joint skeleton, with a 2x2x2 brick posed on the joint at `part_offset` from the group.
// An unpushable brick makes the group a kinematic body.
fn add_rigid_group_with_posed_brick(
    world: &mut shared::game::world::World,
    first_object_id: ObjectId,
    position: Vec3,
    part_offset: Vec3,
    is_pushable: bool,
) -> (ObjectId, ObjectId) {
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = first_object_id;
    let rigid_group_object_id = first_object_id + 2;
    let part_object_id = first_object_id + 3;
    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        first_object_id + 1,
        "joint".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: part_offset,
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        rigid_group_object_id,
        "rigid-group".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            },
            physics_info(position),
        ),
        root_object_id,
    );
    world.addObjectWithId(
        part_object_id,
        "part".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings { size: Vec3::splat(2.0) },
            physics_info(position + part_offset),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: is_pushable,
                density: 1.0,
            },
        ),
        rigid_group_object_id,
    );
    add_sk_attachment(
        world,
        first_object_id + 4,
        root_object_id,
        part_object_id,
        "joint",
        Location3D {
            position: Vec3::ZERO,
            quaternion: Quat::IDENTITY,
        },
    );
    (rigid_group_object_id, part_object_id)
}

// A kinematic rigid group moves to the pose it was given over the next step. Its posed parts were placed from that
// pending pose but measured against the old one, so while the group moved they ran one step of motion ahead of it.
#[test]
fn a_moving_kinematic_rigid_group_keeps_its_posed_parts_in_place() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let part_offset = Vec3::new(1.0, 2.0, 0.0);
    let (rigid_group_object_id, part_object_id) = add_rigid_group_with_posed_brick(&mut world, 9970, Vec3::new(5.0, 20.0, 0.0), part_offset, false);

    for step_index in 1..=10 {
        let group_position = Vec3::new(5.0, 20.0 - 0.3 * step_index as f32, 0.0);
        world.setObjectNextPhysicsInfo3D(rigid_group_object_id, physics_info(group_position))?;
        world.step_server(1, DT_SECS, 0.0)?;

        let part_position = world
            .getObjectPhysicsInfo3D(part_object_id)
            .ok_or_else(|| anyhow::anyhow!("Expected the posed part's physics info"))?
            .location
            .position;
        let expected_position = group_position + part_offset;
        assert!(
            (part_position - expected_position).length() < 0.001,
            "step {step_index}: the part must stay on its joint, expected {expected_position:?}, got {part_position:?}"
        );
    }
    Ok(())
}

// Another player's character lands on ours, and both follow their skeletons. Its parts ran a step ahead of it, so
// they were already touching ours when the step moved it down again, and ours got shoved into the floor.
#[test]
fn a_skeleton_character_landing_on_ours_does_not_shove_it_into_the_floor() -> anyhow::Result<()> {
    let our_player_object_id: ObjectId = 9980;
    let floor_object_id: ObjectId = 9981;
    let mut world = make_world_controlled_by_player(our_player_object_id)?;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, floor_object_id, root_object_id, Vec3::new(0.0, -1.0, 0.0), false)?;
    world.setPrimitiveSettings(
        floor_object_id,
        PrimitiveSettings::Brick(BrickPrimitiveSettings {
            size: Vec3::new(20.0, 2.0, 20.0),
        }),
        true,
    )?;
    let (our_rigid_group_object_id, _) = add_rigid_group_with_posed_brick(&mut world, 9982, Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO, true);
    add_player(&mut world, our_player_object_id, root_object_id, Some(our_rigid_group_object_id))?;
    let (their_rigid_group_object_id, _) = add_rigid_group_with_posed_brick(&mut world, 9990, Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, false);

    let our_y = |world: &shared::game::world::World| {
        world
            .getObjectPhysicsInfo3D(our_rigid_group_object_id)
            .map(|physics_info| physics_info.location.position.y)
            .ok_or_else(|| anyhow::anyhow!("Expected our character's physics info"))
    };
    for _ in 0..30 {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    let rest_y = our_y(&world)?;

    // their character falls at 18 m/s and stops exactly on top of ours
    let landing_y = rest_y + 2.0;
    let mut their_y: f32 = 10.0;
    let mut lowest_y = rest_y;
    for _ in 0..60 {
        their_y = (their_y - 0.3).max(landing_y);
        world.setObjectNextPhysicsInfo3D(their_rigid_group_object_id, physics_info(Vec3::new(0.0, their_y, 0.0)))?;
        world.step_physics(1, DT_SECS, 0.0, None)?;
        lowest_y = lowest_y.min(our_y(&world)?);
    }
    assert!(
        lowest_y > rest_y - 0.02,
        "our character must not be shoved into the floor: it rested at {rest_y} and got down to {lowest_y}"
    );
    Ok(())
}
