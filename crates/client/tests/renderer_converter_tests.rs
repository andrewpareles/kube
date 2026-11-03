mod helpers;

use anyhow::Result;
use client::play_runner::play_renderer::interpolate_render_object_of_id;
use client::play_runner::render_snapshot::FullWorldRenderSnapshot;
use game_types::object_types::{
    BallInfo, BillboardMode, BrickInfo, ClockInfo, ClockState, CylinderInfo, InterpolationStyle, LabelHorizontalAlign, LabelInfo, LiveObject, LocalGroupInfo,
    Location3D, MeshInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectInfoTrackedFields, ObjectLocation, PhysicsInfo3D, PhysicsToggles3D,
    RigidGroupInfo, SkAnimationInfo, SkAttachmentInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo, SkeletonInfo, VisualSettings,
    sk_animator::SkAnimatorInfo,
};
use game_types::physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings};
use game_types::{
    consts::{CLIENT_SIM_AND_SEND_SECS, STORED_WORLD_CURRENT_VERSION},
    stored_world_type::StoredWorld,
};
use glam::{EulerRot, Mat4, Quat, Vec3};
use helpers::fallback_camera_location_for_tests;
use indexmap::IndexMap;
use render_core::{
    computePropertyValue::applyPropertyOverridesToRenderObjectOfId,
    computeSkeletonPose::{computeFollowedSkJointMatrixGivenSkJointToFollow, computeSkeletonPoseOfIdAtTime},
    render_scene::computeRenderObjectTransform,
    renderable::Renderable,
    renderer_types::RenderObject,
};
use shared::game::jsRuntime::{BuiltinMouseCursor, CameraLimits, MouseCursor};
use shared::game::physics::{SetPhysicsInfo3D, SetPhysicsToggles3D};
use shared::game::world::{World, WorldControlMode};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::watch;

fn make_world() -> Result<World> {
    let root_object_id = 1;
    let root_object = LiveObject {
        id: root_object_id,
        name: "root".to_string(),
        info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
    };

    let mut object_of_id = std::collections::HashMap::new();
    object_of_id.insert(root_object_id, root_object);

    let mut children_of_id = std::collections::HashMap::new();
    children_of_id.insert(root_object_id, Vec::new());

    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: fallback_camera_location_for_tests(),
        objectOfId: object_of_id,
        rootObjectId: root_object_id,
        childrenOfId: children_of_id,
        physicsInfo3DOfId: std::collections::HashMap::new(),
        physicsToggles3DOfId: std::collections::HashMap::new(),
        scriptRawCodeOfId: std::collections::HashMap::new(),
        primitiveSettingsOfId: std::collections::HashMap::new(),
    };

    let stored_world_string = serde_json::to_string(&stored_world)?;
    let world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;
    Ok(world)
}

fn add_ball(world: &mut World, object_id: ObjectId, parent_id: ObjectId, position: Vec3) -> Result<()> {
    let ball_info = BallInfo {
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };

    world.addObjectWithId(
        object_id,
        format!("ball-{object_id}"),
        ObjectBeingAdded::Ball(
            ball_info,
            BallPrimitiveSettings { radius: 1.0 },
            PhysicsInfo3D::new(position, glam::Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        parent_id,
    );
    Ok(())
}

fn add_cylinder(world: &mut World, object_id: ObjectId, parent_id: ObjectId, position: Vec3) -> Result<()> {
    let cylinder_info = CylinderInfo {
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };

    world.addObjectWithId(
        object_id,
        format!("cylinder-{object_id}"),
        ObjectBeingAdded::Cylinder(
            cylinder_info,
            CylinderPrimitiveSettings { radius: 1.5, length: 4.0 },
            PhysicsInfo3D::new(position, glam::Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        parent_id,
    );
    Ok(())
}

fn add_mesh(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> Result<()> {
    let mesh_info = MeshInfo {
        scale: Vec3::ONE,
        meshDataId: 456,
        followSkeletonId: None,
        physicalObjectId: None,
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };
    world.addObjectWithId(object_id, format!("mesh-{object_id}"), ObjectBeingAdded::Mesh(mesh_info), parent_id);
    Ok(())
}

fn set_mesh_physical_object_id(world: &mut World, mesh_object_id: ObjectId, physical_object_id: ObjectId) -> Result<()> {
    let old_tracked_fields = {
        let object = world.getObject(mesh_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&object.info)
    };

    {
        let object = world.getObjectMut(mesh_object_id)?;
        let ObjectInfo::Mesh(mesh_info) = &mut object.info else {
            return Err(anyhow::anyhow!("Object {} is not a mesh", mesh_object_id));
        };
        mesh_info.physicalObjectId = Some(ObjectIdPointer(physical_object_id));
    }

    let new_tracked_fields = {
        let object = world.getObject(mesh_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&object.info)
    };
    world.afterObjectInfoChange(mesh_object_id, old_tracked_fields, new_tracked_fields);
    Ok(())
}

#[test]
fn convert_world_to_render_object_of_id_ignores_walkthrough() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let ball_object_id = 90;
        let ball_position = Vec3::new(1.0, 2.0, 3.0);

        add_ball(&mut world, ball_object_id, root_object_id, ball_position)?;

        let render_object_of_id_before = world.getRenderSnapshot().render_object_of_id;
        world.setPhysicsToggles3D(
            ball_object_id,
            SetPhysicsToggles3D {
                isWalkthrough: Some(true),
                isPushable: None,
                density: None,
            },
            false,
        );
        let render_object_of_id_after = world.getRenderSnapshot().render_object_of_id;

        let Some(render_object_before) = render_object_of_id_before.get(&ball_object_id) else {
            return Err(anyhow::anyhow!("Ball render object missing before walkthrough change"));
        };
        let Some(render_object_after) = render_object_of_id_after.get(&ball_object_id) else {
            return Err(anyhow::anyhow!("Ball render object missing after walkthrough change"));
        };
        let RenderObject::Ball(render_ball_before) = render_object_before else {
            return Err(anyhow::anyhow!(
                "Object {} did not convert to RenderObject::Ball before walkthrough change",
                ball_object_id
            ));
        };
        let RenderObject::Ball(render_ball_after) = render_object_after else {
            return Err(anyhow::anyhow!(
                "Object {} did not convert to RenderObject::Ball after walkthrough change",
                ball_object_id
            ));
        };

        assert_eq!(
            render_ball_before.transform.physicsInfo3D.location.position,
            render_ball_after.transform.physicsInfo3D.location.position
        );
        assert_eq!(
            render_ball_before.transform.physicsInfo3D.location.quaternion,
            render_ball_after.transform.physicsInfo3D.location.quaternion
        );
        assert_eq!(
            render_ball_before.transform.physicsInfo3D.movement.velocity,
            render_ball_after.transform.physicsInfo3D.movement.velocity
        );
        assert_eq!(
            render_ball_before.transform.physicsInfo3D.movement.angularVelocity,
            render_ball_after.transform.physicsInfo3D.movement.angularVelocity
        );
        assert_eq!(render_ball_before.visualSettings.opacity, render_ball_after.visualSettings.opacity);
        assert_eq!(render_ball_before.visualSettings.color.r, render_ball_after.visualSettings.color.r);
        assert_eq!(render_ball_before.visualSettings.color.g, render_ball_after.visualSettings.color.g);
        assert_eq!(render_ball_before.visualSettings.color.b, render_ball_after.visualSettings.color.b);
        assert_eq!(render_ball_before.visualSettings.isAlwaysOnTop, render_ball_after.visualSettings.isAlwaysOnTop);
        assert_eq!(render_ball_before.primitive.radius, render_ball_after.primitive.radius);
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

#[test]
fn convert_world_to_render_object_of_id_converts_cylinder() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let cylinder_object_id = 95;
        let cylinder_position = Vec3::new(3.0, 4.0, 5.0);

        add_cylinder(&mut world, cylinder_object_id, root_object_id, cylinder_position)?;

        let render_object_of_id = world.getRenderSnapshot().render_object_of_id;
        let Some(render_object) = render_object_of_id.get(&cylinder_object_id) else {
            return Err(anyhow::anyhow!("Cylinder render object missing"));
        };
        let RenderObject::Cylinder(render_cylinder) = render_object else {
            return Err(anyhow::anyhow!("Object {} did not convert to RenderObject::Cylinder", cylinder_object_id));
        };

        assert_eq!(render_cylinder.transform.physicsInfo3D.location.position, cylinder_position);
        assert_eq!(render_cylinder.primitive.radius, 1.5);
        assert_eq!(render_cylinder.primitive.length, 4.0);
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

#[test]
fn convert_world_to_render_object_of_id_preserves_mesh_follow_target() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let ball_object_id = 100;
        let mesh_object_id = 101;
        let ball_position = Vec3::new(4.0, 5.0, 6.0);

        add_ball(&mut world, ball_object_id, root_object_id, ball_position)?;
        add_mesh(&mut world, mesh_object_id, root_object_id)?;
        set_mesh_physical_object_id(&mut world, mesh_object_id, ball_object_id)?;

        let render_object_of_id = world.getRenderSnapshot().render_object_of_id;
        let Some(render_object) = render_object_of_id.get(&mesh_object_id) else {
            return Err(anyhow::anyhow!("Mesh render object missing"));
        };
        let RenderObject::Mesh(render_mesh) = render_object else {
            return Err(anyhow::anyhow!("Object {} did not convert to RenderObject::Mesh", mesh_object_id));
        };

        assert_eq!(render_mesh.physicalObjectId, ball_object_id);
        assert_eq!(
            computeRenderObjectTransform(&render_object_of_id, &HashMap::new(), mesh_object_id)
                .unwrap()
                .w_axis
                .truncate(),
            ball_position
        );
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

#[test]
fn convert_world_to_render_object_of_id_copies_rigid_group_child_sk_joint_to_follow_offset() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let skeleton_object_id = 200;
        let rigid_group_object_id = 201;
        let brick_object_id = 202;
        let attachment_offset_position = Vec3::new(1.0, 2.0, 3.0);
        let attachment_offset_quaternion = Quat::from_rotation_y(0.5);
        let attachment_offset = Location3D {
            position: attachment_offset_position,
            quaternion: attachment_offset_quaternion,
        };

        world.addObjectWithId(
            skeleton_object_id,
            "skeleton".to_string(),
            ObjectBeingAdded::Skeleton(SkeletonInfo {}),
            root_object_id,
        );
        world.addObjectWithId(
            rigid_group_object_id,
            "rigid-group".to_string(),
            ObjectBeingAdded::RigidGroup(
                RigidGroupInfo {
                    followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
                },
                PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            ),
            root_object_id,
        );
        world.addObjectWithId(
            brick_object_id,
            "payload".to_string(),
            ObjectBeingAdded::Brick(
                BrickInfo {
                    material: None,
                    visualSettings: VisualSettings::TYPICAL_OPAQUE,
                },
                BrickPrimitiveSettings {
                    size: Vec3::new(2.0, 2.0, 2.0),
                },
                PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                PhysicsToggles3D {
                    isWalkthrough: false,
                    isPushable: false,
                    density: 1.0,
                },
            ),
            rigid_group_object_id,
        );
        world.addObjectWithId(
            13003,
            "attach".to_string(),
            ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
                objectId: Some(ObjectIdPointer(brick_object_id)),
                jointName: "hand".to_string(),
                offsetFromJoint: attachment_offset,
            }),
            root_object_id,
        );

        let render_object_of_id = world.getRenderSnapshot().render_object_of_id;
        let Some(render_object) = render_object_of_id.get(&brick_object_id) else {
            return Err(anyhow::anyhow!("Brick render object missing"));
        };
        let RenderObject::Brick(render_brick) = render_object else {
            return Err(anyhow::anyhow!("Object {} did not convert to RenderObject::Brick", brick_object_id));
        };
        let Some(sk_joint_to_follow) = render_brick.transform.sk_joint_to_follow_arc.as_ref() else {
            return Err(anyhow::anyhow!("Expected sk_joint_to_follow on rigid-group child brick"));
        };

        assert_eq!(sk_joint_to_follow.skeleton_id, skeleton_object_id);
        assert_eq!(sk_joint_to_follow.joint_name, "hand");
        assert_eq!(sk_joint_to_follow.offset_from_joint.position, attachment_offset_position);
        assert_eq!(sk_joint_to_follow.offset_from_joint.quaternion, attachment_offset_quaternion);
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

// This runs the exact math the play renderer uses each frame (createRenderObjectOfId ->
// computeSkeletonPoseOfIdAtTime at the estimated server time -> followed joint matrix) for a
// character with a playing Server-time animation, and checks the rendered part actually moves
// through fn(t) around the body's transform.
#[test]
fn render_pipeline_animates_followed_parts_from_a_server_time_clock() -> Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 300;
    let leg_joint_object_id = 301;
    let rigid_group_object_id = 302;
    let leg_brick_object_id = 303;
    let attachment_object_id = 304;
    let clock_object_id = 305;
    let animation_object_id = 306;
    let keyframe_start_object_id = 307;
    let keyframe_start_pose_object_id = 308;
    let keyframe_end_object_id = 309;
    let keyframe_end_pose_object_id = 310;
    let animator_object_id = 311;
    let label_object_id = 312;
    let mesh_object_id = 313;

    let body_position = Vec3::new(4.0, 1.5, -2.0);
    let leg_local_bind = Vec3::new(0.3, -0.9, 0.0);
    let leg_swing_at_end = Vec3::new(0.0, 0.0, 0.8);

    world.addObjectWithId(
        skeleton_object_id,
        "Joints".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        leg_joint_object_id,
        "Leg".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: leg_local_bind,
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        rigid_group_object_id,
        "Body".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            },
            PhysicsInfo3D::new(body_position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        ),
        root_object_id,
    );
    world.addObjectWithId(
        leg_brick_object_id,
        "Leg".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings {
                size: Vec3::new(0.3, 1.0, 0.3),
            },
            PhysicsInfo3D::new(body_position + leg_local_bind, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        rigid_group_object_id,
    );
    world.addObjectWithId(
        attachment_object_id,
        "LegSkAttachment".to_string(),
        ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
            objectId: Some(ObjectIdPointer(leg_brick_object_id)),
            jointName: "Leg".to_string(),
            offsetFromJoint: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        leg_brick_object_id,
    );
    world.addObjectWithId(
        clock_object_id,
        "WalkClock".to_string(),
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
        animation_object_id,
        "WalkAnimation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root_object_id,
    );
    for (keyframe_object_id, pose_object_id, time, pose_position) in [
        (keyframe_start_object_id, keyframe_start_pose_object_id, 0.0_f32, Vec3::ZERO),
        (keyframe_end_object_id, keyframe_end_pose_object_id, 1.0, leg_swing_at_end),
    ] {
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
                jointName: "Leg".to_string(),
                localPose: Location3D {
                    position: pose_position,
                    quaternion: Quat::IDENTITY,
                },
            }),
            keyframe_object_id,
        );
    }
    world.addObjectWithId(
        animator_object_id,
        "WalkAnimator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            skAnimationId: Some(ObjectIdPointer(animation_object_id)),
            clockId: Some(ObjectIdPointer(clock_object_id)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );

    // Follow an animated part through the real World-to-render conversion.
    world.addObjectWithId(
        label_object_id,
        "Label".to_string(),
        ObjectBeingAdded::Label(LabelInfo {
            text: "Player".to_string(),
            lineHeight: 0.5,
            maxWidth: None,
            horizontalAlign: LabelHorizontalAlign::Center,
            backgroundColor: None,
            backgroundOpacity: 1.0,
            offsetFromObject: Vec3::Y,
            rotationFromObject: Quat::IDENTITY,
            physicalObjectId: Some(ObjectIdPointer(leg_brick_object_id)),
            billboardMode: BillboardMode::Full,
            isConstantSize: false,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
        }),
        leg_brick_object_id,
    );

    add_mesh(&mut world, mesh_object_id, root_object_id)?;
    set_mesh_physical_object_id(&mut world, mesh_object_id, leg_brick_object_id)?;

    let render_object_of_id = world.getRenderSnapshot().render_object_of_id;

    // The snapshot must carry the playing animation on the Skeleton render object.
    let Some(RenderObject::Skeleton(render_skeleton)) = render_object_of_id.get(&skeleton_object_id) else {
        return Err(anyhow::anyhow!("Expected skeleton render object"));
    };
    assert_eq!(
        render_skeleton.render_sk_animations.len(),
        1,
        "the render snapshot must include the playing animation"
    );

    let Some(RenderObject::Brick(render_leg)) = render_object_of_id.get(&leg_brick_object_id) else {
        return Err(anyhow::anyhow!("Expected leg render object"));
    };
    let sk_joint_to_follow = render_leg
        .transform
        .sk_joint_to_follow_arc
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Expected sk_joint_to_follow on the leg"))?;

    // Sample the pose like the play renderer: estimated server wall clock plus a client clock.
    let rendered_leg_position_at = |server_wall_ms: f64| -> Result<Vec3> {
        let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(&render_object_of_id, server_wall_ms, Some(12345.0));
        let matrix = computeFollowedSkJointMatrixGivenSkJointToFollow(&render_object_of_id, &skeleton_pose_of_id, sk_joint_to_follow)
            .ok_or_else(|| anyhow::anyhow!("Expected followed joint matrix"))?;
        let label_transform = computeRenderObjectTransform(&render_object_of_id, &skeleton_pose_of_id, label_object_id)
            .ok_or_else(|| anyhow::anyhow!("Expected label transform"))?;
        assert!(
            label_transform.abs_diff_eq(matrix, 1e-5),
            "the label must use the part's animation at redraw time"
        );
        let mesh_transform = computeRenderObjectTransform(&render_object_of_id, &skeleton_pose_of_id, mesh_object_id)
            .ok_or_else(|| anyhow::anyhow!("Expected mesh transform"))?;
        assert!(mesh_transform.abs_diff_eq(matrix, 1e-5), "the mesh must follow the rendered joint pose");
        let (_scale, _rotation, translation) = matrix.to_scale_rotation_translation();
        Ok(translation)
    };

    let assert_vec3_near = |actual: Vec3, expected: Vec3, label: &str| {
        let delta = actual - expected;
        assert!(delta.length() < 1e-4, "{label}: expected {expected:?}, got {actual:?}");
    };

    // t=0: at the bind pose around the body; t=250ms/750ms: a quarter and three quarters through
    // the linear swing. The rendered part must move over server time and stay body-relative.
    assert_vec3_near(rendered_leg_position_at(0.0)?, body_position + leg_local_bind, "t=0");
    assert_vec3_near(
        rendered_leg_position_at(250.0)?,
        body_position + leg_local_bind + leg_swing_at_end * 0.25,
        "t=250ms",
    );
    assert_vec3_near(
        rendered_leg_position_at(750.0)?,
        body_position + leg_local_bind + leg_swing_at_end * 0.75,
        "t=750ms",
    );

    Ok(())
}

// -------------------------- Meshes and welded parts --------------------------

fn make_world_with_welded_parts() -> Result<World> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    world.addObjectWithId(
        2,
        "Body".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo { followSkeletonId: None },
            PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        ),
        root_id,
    );
    add_ball(&mut world, 3, 2, Vec3::new(10.0, 0.0, 0.0))?;
    world.addObjectWithId(
        4,
        "Brick".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings { size: Vec3::ONE },
            PhysicsInfo3D::new(Vec3::new(0.0, 10.0, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        2,
    );
    add_cylinder(&mut world, 5, 2, Vec3::new(0.0, 0.0, 10.0))?;
    add_mesh(&mut world, 6, root_id)?;
    set_mesh_physical_object_id(&mut world, 6, 3)?;
    Ok(world)
}

fn make_render_snapshot(world: &World, live_client_sim_ms: f64) -> FullWorldRenderSnapshot {
    FullWorldRenderSnapshot::new(
        world.getRenderSnapshot(),
        world,
        999,
        live_client_sim_ms,
        0.0,
        IndexMap::new(),
        CameraLimits::new(),
        MouseCursor::Builtin(BuiltinMouseCursor::Default),
        &HashMap::new(),
    )
}

#[test]
fn welded_parts_and_meshes_keep_their_offsets_while_the_group_rotates() -> Result<()> {
    let mut world = make_world_with_welded_parts()?;
    let angular_velocity = Vec3::Y * std::f32::consts::FRAC_PI_2 / 0.1;
    world.physics.forceObjectPhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            angularVelocity: Some(angular_velocity),
            ..Default::default()
        },
    );
    let prev_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    world.physics.forceObjectPhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            rotation: Some(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            ..Default::default()
        },
    );
    let next_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    for alpha in [0.25, 0.5, 0.75] {
        let rendered_object_of_id =
            interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 100.0 + alpha as f64 * 100.0);
        let expected_rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 * alpha);
        for (id, offset) in [(3, Vec3::X * 10.0), (4, Vec3::Y * 10.0), (5, Vec3::Z * 10.0), (6, Vec3::X * 10.0)] {
            let transform = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), id).unwrap();
            let expected = Mat4::from_rotation_translation(expected_rotation, expected_rotation * offset);
            assert!(transform.abs_diff_eq(expected, 1e-4), "object {id} moved within its weld at alpha {alpha}");
        }
    }
    Ok(())
}

#[test]
fn welded_parts_and_meshes_snap_with_a_teleported_group() -> Result<()> {
    let mut world = make_world_with_welded_parts()?;
    let prev_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    world.forcePhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            position: Some(Vec3::Y * 100.0),
            ..Default::default()
        },
        false,
    );
    let next_render_object_of_id = make_render_snapshot(&world, 100.0).render_snapshot.render_object_of_id;
    let rendered_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 150.0);
    for id in [2, 3, 4, 5, 6] {
        let expected = computeRenderObjectTransform(&next_render_object_of_id, &HashMap::new(), id).unwrap();
        let actual = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), id).unwrap();
        assert!(actual.abs_diff_eq(expected, 1e-5), "object {id} did not teleport with its group");
    }
    Ok(())
}

// Exercise the actual latest-only channel: the render thread never reads the teleport tick.
#[test]
fn skipped_snapshots_preserve_teleports_and_then_resume_smoothing() -> Result<()> {
    for teleported_id in [2, 3, 4, 5, 8] {
        let mut world = make_world_with_welded_parts()?;
        world.worldControlMode = WorldControlMode::Player(999);
        let root_id = world.getRootObjectId();
        add_ball(&mut world, 8, root_id, Vec3::ZERO)?;
        add_ball(&mut world, 9, root_id, Vec3::ZERO)?;
        for id in [3, 4, 5, 8, 9] {
            world.setPhysicsToggles3D(
                id,
                SetPhysicsToggles3D {
                    isPushable: Some(true),
                    ..Default::default()
                },
                false,
            );
        }
        let followed_id = if teleported_id == 2 { 3 } else { teleported_id };
        set_mesh_physical_object_id(&mut world, 6, followed_id)?;
        world.addObjectWithId(
            7,
            "Nametag".to_string(),
            ObjectBeingAdded::Label(LabelInfo {
                text: "Player".to_string(),
                lineHeight: 0.5,
                maxWidth: None,
                horizontalAlign: LabelHorizontalAlign::Center,
                backgroundColor: None,
                backgroundOpacity: 1.0,
                offsetFromObject: Vec3::Y,
                rotationFromObject: Quat::IDENTITY,
                physicalObjectId: Some(ObjectIdPointer(followed_id)),
                billboardMode: BillboardMode::Full,
                isConstantSize: false,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            }),
            root_id,
        );
        let mut prev_snapshot_arc = Arc::new(make_render_snapshot(&world, 0.0));
        let (snapshot_tx, mut snapshot_rx) = watch::channel(Arc::clone(&prev_snapshot_arc));

        // A second teleport must still be detected after the first has already been rendered.
        for jump in 0..2 {
            let start_ms = jump as f64 * 300.0;
            let destination = Vec3::Y * (100.0 + jump as f32 * 100.0);
            world.forcePhysicsInfo3D(
                teleported_id,
                SetPhysicsInfo3D {
                    position: Some(destination),
                    ..Default::default()
                },
                false,
            );
            let teleport_snapshot = make_render_snapshot(&world, start_ms + 100.0);
            assert!(snapshot_tx.send(Arc::new(teleport_snapshot)).is_ok());

            // Ordinary movement replaces the teleport snapshot before the renderer gets a chance to read it.
            world.setPhysicsInfo3D(
                teleported_id,
                SetPhysicsInfo3D {
                    position: Some(destination + Vec3::Y * 10.0),
                    ..Default::default()
                },
                false,
            );
            world.setPhysicsInfo3D(
                9,
                SetPhysicsInfo3D {
                    position: Some(Vec3::X * (jump + 1) as f32 * 20.0),
                    ..Default::default()
                },
                false,
            );
            let next_snapshot = make_render_snapshot(&world, start_ms + 200.0);
            assert!(snapshot_tx.send(Arc::new(next_snapshot)).is_ok());
            let next_snapshot_arc = Arc::clone(&snapshot_rx.borrow_and_update());
            assert_eq!(next_snapshot_arc.snapshot_live_client_sim_ms, start_ms + 200.0);
            for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let rendered_object_of_id = interpolate_render_object_of_id(
                    &prev_snapshot_arc.render_snapshot.render_object_of_id,
                    &next_snapshot_arc.render_snapshot.render_object_of_id,
                    prev_snapshot_arc.snapshot_live_client_sim_ms,
                    next_snapshot_arc.snapshot_live_client_sim_ms,
                    start_ms + 200.0 + alpha as f64 * 200.0,
                );
                for id in [teleported_id, followed_id, 6, 7] {
                    let expected = computeRenderObjectTransform(&next_snapshot_arc.render_snapshot.render_object_of_id, &HashMap::new(), id).unwrap();
                    let actual = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), id).unwrap();
                    assert!(
                        actual.abs_diff_eq(expected, 1e-5),
                        "object {id} smoothed across skipped teleport {jump} of {teleported_id}"
                    );
                }
                let control_position = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), 9)
                    .unwrap()
                    .w_axis
                    .truncate();
                let expected_control_x = 20.0 * (jump as f32 + alpha * alpha * (3.0 - 2.0 * alpha));
                assert!(
                    (control_position.x - expected_control_x).abs() < 1e-4,
                    "another object's teleport stopped ordinary smoothing"
                );
            }

            // Once both rendered snapshots are after the teleport, the body and its followers glide again.
            world.setPhysicsInfo3D(
                teleported_id,
                SetPhysicsInfo3D {
                    position: Some(destination + Vec3::Y * 30.0),
                    ..Default::default()
                },
                false,
            );
            let resumed_snapshot = make_render_snapshot(&world, start_ms + 300.0);
            assert!(snapshot_tx.send(Arc::new(resumed_snapshot)).is_ok());
            let resumed_snapshot_arc = Arc::clone(&snapshot_rx.borrow_and_update());
            let rendered_object_of_id = interpolate_render_object_of_id(
                &next_snapshot_arc.render_snapshot.render_object_of_id,
                &resumed_snapshot_arc.render_snapshot.render_object_of_id,
                start_ms + 200.0,
                start_ms + 300.0,
                start_ms + 350.0,
            );
            for id in [teleported_id, followed_id, 6, 7] {
                let before = computeRenderObjectTransform(&next_snapshot_arc.render_snapshot.render_object_of_id, &HashMap::new(), id)
                    .unwrap()
                    .w_axis
                    .truncate();
                let after = computeRenderObjectTransform(&resumed_snapshot_arc.render_snapshot.render_object_of_id, &HashMap::new(), id)
                    .unwrap()
                    .w_axis
                    .truncate();
                let actual = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), id)
                    .unwrap()
                    .w_axis
                    .truncate();
                assert!((after.y - before.y - 20.0).abs() < 1e-4, "the test must include movement after the teleport");
                assert!(actual.distance((before + after) * 0.5) < 1e-4, "object {id} did not resume smoothing");
            }
            prev_snapshot_arc = resumed_snapshot_arc;
        }
    }
    Ok(())
}

#[test]
fn welded_parts_interpolate_local_offset_changes_while_the_group_moves() -> Result<()> {
    let mut world = make_world_with_welded_parts()?;
    let prev_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    world.physics.forceObjectPhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            position: Some(Vec3::Y * 20.0),
            rotation: Some(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            ..Default::default()
        },
    );
    world.physics.setSubitemRelativeLocation3D(
        3,
        Location3D {
            position: Vec3::X * 14.0,
            quaternion: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        },
    )?;
    let next_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    let rendered_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 150.0);
    let expected = Mat4::from_rotation_translation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4), Vec3::Y * 10.0)
        * Mat4::from_rotation_translation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_4), Vec3::X * 12.0);
    for id in [3, 6] {
        let actual = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), id).unwrap();
        assert!(actual.abs_diff_eq(expected, 1e-4), "object {id} did not preserve the animated local offset");
    }
    Ok(())
}

#[test]
fn group_and_child_property_animation_share_the_final_world_transform() -> Result<()> {
    let world = make_world_with_welded_parts()?;
    let mut render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    let property_override_of_path_of_object_id = HashMap::from([
        (
            2,
            HashMap::from([
                (vec!["location".to_string(), "position".to_string(), "y".to_string()], 50.0),
                (vec!["location".to_string(), "rotation".to_string(), "y".to_string()], 90.0),
            ]),
        ),
        (
            3,
            HashMap::from([(vec!["location".to_string(), "position".to_string(), "y".to_string()], 60.0)]),
        ),
        (4, HashMap::from([(vec!["opacity".to_string()], 0.5)])),
    ]);
    applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &property_override_of_path_of_object_id, &HashMap::new());
    for (id, position) in [
        (3, Vec3::new(0.0, 60.0, -10.0)),
        (4, Vec3::new(0.0, 60.0, 0.0)),
        (5, Vec3::new(10.0, 50.0, 0.0)),
        (6, Vec3::new(0.0, 60.0, -10.0)),
    ] {
        let expected = Mat4::from_rotation_translation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), position);
        let actual = computeRenderObjectTransform(&render_object_of_id, &HashMap::new(), id).unwrap();
        assert!(
            actual.abs_diff_eq(expected, 1e-4),
            "object {id} used a different group or property-animation pose"
        );
    }
    Ok(())
}

#[test]
fn welded_property_overrides_preserve_unanimated_world_axes() -> Result<()> {
    let mut world = make_world_with_welded_parts()?;
    let group_rotation = Quat::from_euler(EulerRot::XYZ, 0.4, -0.6, 0.2);
    world.forcePhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(12.0, 30.0, -8.0)),
            rotation: Some(group_rotation),
            ..Default::default()
        },
        false,
    );
    // Use rotations around different axes so reversing the parent/local composition would fail.
    let local_rotation = Quat::from_euler(EulerRot::XYZ, -0.3, 0.1, 0.5);
    for id in [3, 4, 5] {
        world.physics.setSubitemRelativeLocation3D(
            id,
            Location3D {
                position: Vec3::new(id as f32, 2.0, -4.0),
                quaternion: local_rotation,
            },
        )?;
    }

    let mut render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    let mut property_override_of_path_of_object_id = HashMap::from([(
        2,
        HashMap::from([(vec!["location".to_string(), "position".to_string(), "y".to_string()], 50.0)]),
    )]);
    for id in [3, 4, 5] {
        property_override_of_path_of_object_id.insert(
            id,
            HashMap::from([
                (vec!["location".to_string(), "position".to_string(), "x".to_string()], 20.0),
                (vec!["location".to_string(), "rotation".to_string(), "z".to_string()], -55.0),
            ]),
        );
    }
    applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &property_override_of_path_of_object_id, &HashMap::new());

    // rotation angles apply y first, then x, then z, so this keeps the unanimated y and x and replaces z
    let (rotation_y, rotation_x, _) = (group_rotation * local_rotation).to_euler(EulerRot::YXZ);
    let expected_rotation = Quat::from_euler(EulerRot::YXZ, rotation_y, rotation_x, -55.0_f32.to_radians());
    for id in [3, 4, 5, 6] {
        let physical_id = if id == 6 { 3 } else { id };
        let mut expected_position = Vec3::new(12.0, 50.0, -8.0) + group_rotation * Vec3::new(physical_id as f32, 2.0, -4.0);
        expected_position.x = 20.0;
        let expected = Mat4::from_rotation_translation(expected_rotation, expected_position);
        let actual = computeRenderObjectTransform(&render_object_of_id, &HashMap::new(), id).unwrap();
        assert!(actual.abs_diff_eq(expected, 1e-4), "object {id} changed an unanimated world axis");
    }
    Ok(())
}

#[test]
fn reparenting_does_not_blend_offsets_from_different_groups() -> Result<()> {
    let mut world = make_world_with_welded_parts()?;
    let root_id = world.getRootObjectId();
    world.addObjectWithId(
        7,
        "Other body".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo { followSkeletonId: None },
            PhysicsInfo3D::new(Vec3::X * 100.0, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        ),
        root_id,
    );
    for parent_id in [7, root_id, 2] {
        let prev_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
        world.setParent(3, parent_id)?;
        world.physics.forceObjectPhysicsInfo3D(
            3,
            SetPhysicsInfo3D {
                position: Some(Vec3::Y * parent_id as f32),
                ..Default::default()
            },
        );
        let next_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
        let rendered_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 125.0);
        for id in [3, 6] {
            let actual = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), id).unwrap();
            assert!(
                actual.w_axis.truncate().distance(Vec3::Y * parent_id as f32) < 1e-4,
                "object {id} blended across attachment changes"
            );
        }
    }
    Ok(())
}

#[test]
fn a_mesh_uses_its_new_follow_target_immediately() -> Result<()> {
    let mut world = make_world_with_welded_parts()?;
    let prev_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    set_mesh_physical_object_id(&mut world, 6, 5)?;
    let next_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    let rendered_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 125.0);
    let mesh_transform = computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), 6).unwrap();
    assert!(mesh_transform.w_axis.truncate().distance(Vec3::Z * 10.0) < 1e-5);
    Ok(())
}

// At 120 Hz there are two frames per tick, and a tick's snapshot only reaches the renderer once the tick has run.
// Until it arrives, the ball carries on from the newest snapshot along its velocity, so it moves the same distance every frame.
#[test]
fn a_moving_ball_moves_evenly_every_frame_at_120_hz_while_each_tick_takes_10_ms() -> Result<()> {
    let ball_id = 2;
    let tick_ms = CLIENT_SIM_AND_SEND_SECS * 1000.0;
    let tick_run_ms = 10.0;
    let frame_ms = 1000.0 / 120.0;
    let speed = 30.0;
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_ball(&mut world, ball_id, root_id, Vec3::ZERO)?;
    // a pushable ball keeps the velocity it's given, like anything moving in a game
    world.setPhysicsToggles3D(
        ball_id,
        SetPhysicsToggles3D {
            isPushable: Some(true),
            ..Default::default()
        },
        false,
    );

    let mut recent_snapshots = VecDeque::new();
    let mut next_tick = 0;
    let mut drawn_xs = Vec::new();
    for frame in 0..48 {
        let redraw_live_client_wall_ms = frame as f64 * frame_ms as f64;
        // each tick's snapshot arrives once that tick has run
        while next_tick as f64 * tick_ms as f64 + tick_run_ms <= redraw_live_client_wall_ms {
            let tick_live_client_sim_ms = next_tick as f64 * tick_ms as f64;
            world.setPhysicsInfo3D(
                ball_id,
                SetPhysicsInfo3D {
                    position: Some(Vec3::X * speed * tick_live_client_sim_ms as f32 / 1000.0),
                    velocity: Some(Vec3::X * speed),
                    ..Default::default()
                },
                false,
            );
            recent_snapshots.push_back(Arc::new(make_render_snapshot(&world, tick_live_client_sim_ms)));
            next_tick += 1;
        }
        if recent_snapshots.is_empty() {
            continue;
        }
        let next_snapshot = &recent_snapshots[recent_snapshots.len() - 1];
        let prev_snapshot = &recent_snapshots[recent_snapshots.len().saturating_sub(2)];
        let rendered_object_of_id = interpolate_render_object_of_id(
            &prev_snapshot.render_snapshot.render_object_of_id,
            &next_snapshot.render_snapshot.render_object_of_id,
            prev_snapshot.snapshot_live_client_sim_ms,
            next_snapshot.snapshot_live_client_sim_ms,
            redraw_live_client_wall_ms,
        );
        drawn_xs.push(computeRenderObjectTransform(&rendered_object_of_id, &HashMap::new(), ball_id).unwrap().w_axis.x);
    }

    // once a few ticks have arrived, every frame shows the ball one frame's travel further along
    let frame_travel = speed * frame_ms / 1000.0;
    let settled_xs = &drawn_xs[8..];
    assert!(
        settled_xs.windows(2).all(|xs| (xs[1] - xs[0] - frame_travel).abs() < 1e-3),
        "the ball should move {frame_travel} every frame: {settled_xs:?}"
    );
    Ok(())
}
