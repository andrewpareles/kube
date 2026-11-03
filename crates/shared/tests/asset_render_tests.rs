#[path = "../../render_core/tests/helpers/render_capture.rs"]
mod render_capture;

mod helpers;

use anyhow::Result;
use game_types::{
    data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex},
    object_delta::{ObjectAddDelta, ObjectDelta, ObjectModifyDelta},
    object_types::{
        ClockInfo, ClockState, InterpolationStyle, LiveObject, Location3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation,
        PropertyKeyframeInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo, SkeletonInfo, WeightTransition,
    },
    stored_world_type::StoredWorld,
};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use helpers::asset_test_helpers::asset_object;
use render_core::{
    computePropertyValue::{applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime},
    computeSkeletonPose::computeSkeletonPoseOfIdAtTime,
    render_scene::{LabelContent, RenderScene, build_render_scene, computeRenderObjectTransform},
    renderable::{RenderSnapshot, Renderable},
    renderer_types::RenderObject,
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;
use shared::game::world::{World, WorldControlMode};
use std::{collections::HashMap, sync::Arc};

// Exercise real World loading and saved-scene loading with the same asset payload.
// No API or storage writes are involved.
fn animated_asset() -> Result<StoredWorld> {
    let mut stored: StoredWorld = serde_json::from_value(serde_json::to_value(asset_object(10, 20, 30)?)?)?;
    // The cylinder is welded to the skeleton-following group, with a real joint attachment.
    stored.childrenOfId.get_mut(&100).unwrap().retain(|id| *id != 102);
    stored.childrenOfId.entry(108).or_default().push(102);
    let ObjectInfo::Clock(clock) = &mut stored.objectOfId.get_mut(&103).unwrap().info else {
        unreachable!()
    };
    clock.clockState = ClockState::Playing {
        localStartMs: 0.0,
        timeLocation: ObjectLocation::Server,
    };
    clock.speed = 1.0;
    let ObjectInfo::SkKeyframe(frame) = &mut stored.objectOfId.get_mut(&115).unwrap().info else {
        unreachable!()
    };
    frame.time = 0.0;
    stored.objectOfId.insert(
        200,
        LiveObject {
            id: 200,
            name: "later pose".into(),
            info: ObjectInfo::SkKeyframe(SkKeyframeInfo { time: 1.0 }),
        },
    );
    stored.objectOfId.insert(
        201,
        LiveObject {
            id: 201,
            name: "joint pose".into(),
            info: ObjectInfo::SkKeyframePose(SkKeyframePoseInfo {
                jointName: "joint".into(),
                localPose: Location3D {
                    position: Vec3::new(1.0, 3.0, 0.0),
                    quaternion: Quat::from_rotation_z(0.8),
                },
            }),
        },
    );
    stored.childrenOfId.entry(114).or_default().push(200);
    stored.childrenOfId.insert(200, vec![201]);
    let ObjectInfo::PropertyKeyframe(frame) = &mut stored.objectOfId.get_mut(&120).unwrap().info else {
        unreachable!()
    };
    *frame = PropertyKeyframeInfo { time: 0.0, value: 0.2 };
    stored.objectOfId.insert(
        202,
        LiveObject {
            id: 202,
            name: "opacity end".into(),
            info: ObjectInfo::PropertyKeyframe(PropertyKeyframeInfo { time: 1.0, value: 0.9 }),
        },
    );
    stored.childrenOfId.entry(119).or_default().push(202);
    stored.objectOfId.insert(
        203,
        LiveObject {
            id: 203,
            name: "client clock".into(),
            info: ObjectInfo::Clock(ClockInfo {
                clockState: ClockState::Playing {
                    localStartMs: 0.0,
                    timeLocation: ObjectLocation::Client,
                },
                speed: 1.0,
                shouldLoop: true,
            }),
        },
    );
    stored.childrenOfId.entry(100).or_default().push(203);
    let ObjectInfo::PropertyAnimator(animator) = &mut stored.objectOfId.get_mut(&121).unwrap().info else {
        unreachable!()
    };
    animator.clockId = Some(203.into());
    Ok(stored)
}

fn assert_matrix_close(left: Mat4, right: Mat4) {
    for (left, right) in left.to_cols_array().into_iter().zip(right.to_cols_array()) {
        assert!((left - right).abs() < 0.0001, "{left} != {right}");
    }
}

#[test]
fn stored_and_live_sources_render_the_same_animated_object() -> Result<()> {
    let world = World::fromStoredWorldString(&serde_json::to_string(&animated_asset()?)?, WorldControlMode::Server, None)?;
    // World loading remaps ids. Serialize the loaded world so both sources have the same identities.
    let stored: StoredWorld = serde_json::from_str(&world.toStoredWorldString()?)?;
    let source = StoredWorldRenderable::new(stored)?;
    let saved_snapshot = source.getRenderSnapshot();
    let live_snapshot = world.getRenderSnapshot();
    let saved_objects = &saved_snapshot.render_object_of_id;
    let live_objects = &live_snapshot.render_object_of_id;
    assert_eq!(saved_objects.len(), live_objects.len());
    let mesh_data_of_id = HashMap::from([(
        10,
        Arc::new(MeshData::SkinnedMeshData {
            vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y]
                .into_iter()
                .map(|position| MeshVertexInternal {
                    position,
                    normal: Vec3::Z,
                    uv: Vec2::ZERO,
                    tangent: Vec4::X,
                    skinning: SkinningInfoPerVertex {
                        jointIndices: [0; 4],
                        jointWeights: [1.0, 0.0, 0.0, 0.0],
                    },
                })
                .collect(),
            indices: vec![0, 1, 2],
            jointNameOfIdx: vec!["joint".into()],
            inverseBindMatrixOfJointName: HashMap::from([("joint".into(), Mat4::IDENTITY.to_cols_array())]),
        }),
    )]);
    let mut sampled_joint_positions = Vec::new();
    for (server_ms, client_ms) in [(0.0, 0.0), (250.0, 500.0), (750.0, 1250.0), (2500.0, 3500.0)] {
        let mut saved = saved_objects.clone();
        let mut live = live_objects.clone();
        let saved_overrides = computePropertyOverridesOfObjectIdAtTime(
            &saved_snapshot.property_overrides_of_id,
            &saved_snapshot.rotation_base_degrees_of_id,
            server_ms,
            Some(client_ms),
        );
        let live_overrides = computePropertyOverridesOfObjectIdAtTime(
            &live_snapshot.property_overrides_of_id,
            &live_snapshot.rotation_base_degrees_of_id,
            server_ms,
            Some(client_ms),
        );
        assert_eq!(saved_overrides, live_overrides);
        applyPropertyOverridesToRenderObjectOfId(&mut saved, &saved_overrides, &HashMap::new());
        applyPropertyOverridesToRenderObjectOfId(&mut live, &live_overrides, &HashMap::new());
        let saved_poses = computeSkeletonPoseOfIdAtTime(&saved, server_ms, Some(client_ms));
        let live_poses = computeSkeletonPoseOfIdAtTime(&live, server_ms, Some(client_ms));
        for (id, pose) in &saved_poses {
            assert_eq!(pose.jointMatrixOfJointIndex.len(), live_poses[id].jointMatrixOfJointIndex.len());
            for (saved, live) in pose.jointMatrixOfJointIndex.iter().zip(&live_poses[id].jointMatrixOfJointIndex) {
                assert_matrix_close(*saved, *live);
            }
            sampled_joint_positions.push(pose.jointMatrixOfJointIndex[0].transform_point3(Vec3::ZERO));
        }
        for id in saved.keys() {
            let left = computeRenderObjectTransform(&saved, &saved_poses, *id);
            let right = computeRenderObjectTransform(&live, &live_poses, *id);
            assert_eq!(left.is_some(), right.is_some());
            if let (Some(left), Some(right)) = (left, right) {
                assert_matrix_close(left, right);
            }
        }
        let saved_scene = build_render_scene(&saved, &mesh_data_of_id, Some(&saved_poses));
        let live_scene = build_render_scene(&live, &mesh_data_of_id, Some(&live_poses));
        assert_eq!(saved_scene.instances.len(), 4);
        assert_eq!(saved_scene.labels.len(), 2);
        assert_eq!(saved_scene.joint_matrices.len(), 1);
        assert_eq!(bytemuck::bytes_of(&saved_scene.lighting), bytemuck::bytes_of(&live_scene.lighting));
        assert_eq!(saved_scene.skybox_texture_data_id, Some(20));
        let text_label_of = |scene: &RenderScene| {
            scene.labels.iter().find_map(|label| match &label.content {
                LabelContent::Text { geometry, atlas, .. } => Some((geometry.vertices.len(), atlas.mip_levels.clone())),
                LabelContent::Image { .. } => None,
            })
        };
        let saved_text_label = text_label_of(&saved_scene).expect("the saved scene should have its text label");
        assert_eq!(Some(saved_text_label), text_label_of(&live_scene));
    }
    assert!(
        sampled_joint_positions.windows(2).any(|pair| pair[0].distance(pair[1]) > 0.1),
        "Skeletal animation must actually move"
    );
    Ok(())
}

#[test]
fn snapshot_capture_reads_current_weights_for_both_animation_types() -> Result<()> {
    let mut world = World::fromStoredWorldString(&serde_json::to_string(&animated_asset()?)?, WorldControlMode::Server, None)?;
    let mut source = StoredWorldRenderable::new(serde_json::from_str(&world.toStoredWorldString()?)?)?;
    let live_before = world.getRenderSnapshot();
    let saved_before = source.getRenderSnapshot();

    // Change both animator types after capturing a snapshot. Their joint/keyframe definitions stay fixed.
    let mut animator_ids = Vec::new();
    for (id, object) in source.objects() {
        match &object.info {
            ObjectInfo::SkAnimator(_) | ObjectInfo::PropertyAnimator(_) => animator_ids.push(*id),
            _ => {}
        }
    }
    assert_eq!(animator_ids.len(), 2);
    for id in &animator_ids {
        let mut info = world.objectOfId[id].info.clone();
        let (weight, transition) = match &mut info {
            ObjectInfo::SkAnimator(animator) => (animator.weight, &mut animator.weightTransition),
            ObjectInfo::PropertyAnimator(animator) => (animator.weight, &mut animator.weightTransition),
            _ => unreachable!(),
        };
        let current_weight = WeightTransition::readValueWithFallback(*transition, weight, 250.0, Some(250.0));
        *transition = Some(WeightTransition {
            startedAtMs: 250.0,
            timeLocation: ObjectLocation::Client,
            initValue: current_weight,
            finalValue: 0.0,
            durationMs: 500.0,
            easing: InterpolationStyle::Linear,
        });
        match info {
            ObjectInfo::SkAnimator(animator) => world.setSkAnimatorInfo(*id, animator)?,
            ObjectInfo::PropertyAnimator(animator) => world.setPropertyAnimatorInfo(*id, animator)?,
            _ => unreachable!(),
        }
    }
    source.smoothly_change_weights(
        animator_ids.into_iter().map(|id| (id, 0.0)),
        500.0,
        InterpolationStyle::Linear,
        ObjectLocation::Client,
        250.0,
        Some(250.0),
    )?;

    let live_after = world.getRenderSnapshot();
    let saved_after = source.getRenderSnapshot();
    for (before, after) in [(&live_before, &live_after), (&saved_before, &saved_after)] {
        let before_poses = computeSkeletonPoseOfIdAtTime(&before.render_object_of_id, 1000.0, Some(1000.0));
        let after_poses = computeSkeletonPoseOfIdAtTime(&after.render_object_of_id, 1000.0, Some(1000.0));
        for (id, before_pose) in before_poses {
            assert_ne!(before_pose.jointMatrixOfJointIndex, after_poses[&id].jointMatrixOfJointIndex);
            let (RenderObject::Skeleton(before_skeleton), RenderObject::Skeleton(after_skeleton)) =
                (&before.render_object_of_id[&id], &after.render_object_of_id[&id])
            else {
                unreachable!()
            };
            assert!(Arc::ptr_eq(&before_skeleton.renderSkeletonInfoArc, &after_skeleton.renderSkeletonInfoArc));
        }
        let before_overrides =
            computePropertyOverridesOfObjectIdAtTime(&before.property_overrides_of_id, &before.rotation_base_degrees_of_id, 1000.0, Some(1000.0));
        let after_overrides =
            computePropertyOverridesOfObjectIdAtTime(&after.property_overrides_of_id, &after.rotation_base_degrees_of_id, 1000.0, Some(1000.0));
        assert_ne!(before_overrides, after_overrides);
    }
    for elapsed_ms in [250.0, 500.0, 1000.0] {
        let live_poses = computeSkeletonPoseOfIdAtTime(&live_after.render_object_of_id, elapsed_ms, Some(elapsed_ms));
        let saved_poses = computeSkeletonPoseOfIdAtTime(&saved_after.render_object_of_id, elapsed_ms, Some(elapsed_ms));
        for (id, pose) in live_poses {
            for (live, saved) in pose.jointMatrixOfJointIndex.iter().zip(&saved_poses[&id].jointMatrixOfJointIndex) {
                assert_matrix_close(*live, *saved);
            }
        }
        let live_overrides = computePropertyOverridesOfObjectIdAtTime(
            &live_after.property_overrides_of_id,
            &live_after.rotation_base_degrees_of_id,
            elapsed_ms,
            Some(elapsed_ms),
        );
        let saved_overrides = computePropertyOverridesOfObjectIdAtTime(
            &saved_after.property_overrides_of_id,
            &saved_after.rotation_base_degrees_of_id,
            elapsed_ms,
            Some(elapsed_ms),
        );
        assert_eq!(live_overrides, saved_overrides);
    }
    Ok(())
}

// Both renderers turn an object around its Pivot the way the world does: the pivot point is read off the object's pose,
// stays put, and the animated rotation turns the object around it.
fn modified(info: Option<ObjectInfo>, name: Option<&str>, parent_id: Option<ObjectId>) -> ObjectDelta {
    ObjectDelta::Modify(ObjectModifyDelta {
        modifiedObjectInfo: info,
        modifiedPhysicsToggles3D: None,
        modifiedPhysicsInfo3D: None,
        modifiedPrimitiveSettings: None,
        modifiedParent: parent_id,
        modifiedName: name.map(str::to_string),
    })
}

fn added(name: &str, object: ObjectBeingAdded, parent_id: ObjectId) -> ObjectDelta {
    ObjectDelta::Add(ObjectAddDelta {
        name: name.to_string(),
        objectBeingAdded: object,
        parentId: parent_id,
        initSubitemRelativeLocation3D: None,
        teleportCount: 0,
    })
}

// What a world draws: each skeleton's joints, and every skeleton pose and property value at a few times.
fn assert_draws_like_a_fresh_load(world: &StoredWorldRenderable, step: &str) -> Result<()> {
    let fresh = StoredWorldRenderable::new(world.stored_world().clone())?;
    let (edited, loaded) = (world.getRenderSnapshot(), fresh.getRenderSnapshot());
    let joints_of_skeleton = |snapshot: &RenderSnapshot| {
        snapshot
            .render_object_of_id
            .iter()
            .filter_map(|(id, object)| match object {
                RenderObject::Skeleton(skeleton) => Some((
                    *id,
                    (
                        skeleton.renderSkeletonInfoArc.jointIndexOfName.clone(),
                        skeleton.renderSkeletonInfoArc.jointIndexOfObjectId.clone(),
                    ),
                )),
                _ => None,
            })
            .collect::<HashMap<_, _>>()
    };
    assert_eq!(joints_of_skeleton(&edited), joints_of_skeleton(&loaded), "{step}: joints");
    for time_ms in [0.0, 250.0, 500.0, 1000.0, 1750.0] {
        let edited_poses = computeSkeletonPoseOfIdAtTime(&edited.render_object_of_id, time_ms, Some(time_ms));
        let loaded_poses = computeSkeletonPoseOfIdAtTime(&loaded.render_object_of_id, time_ms, Some(time_ms));
        assert_eq!(edited_poses.len(), loaded_poses.len(), "{step}: skeletons posed at {time_ms} ms");
        for (id, pose) in &edited_poses {
            assert_eq!(
                pose.jointMatrixOfJointIndex, loaded_poses[id].jointMatrixOfJointIndex,
                "{step}: skeleton {id} at {time_ms} ms"
            );
        }
        let property_overrides_at = |snapshot: &RenderSnapshot| {
            computePropertyOverridesOfObjectIdAtTime(
                &snapshot.property_overrides_of_id,
                &snapshot.rotation_base_degrees_of_id,
                time_ms,
                Some(time_ms),
            )
        };
        assert_eq!(
            property_overrides_at(&edited),
            property_overrides_at(&loaded),
            "{step}: property values at {time_ms} ms"
        );
    }
    Ok(())
}

// The editor applies each batch of edits to its saved world and keeps each skeleton's and animation's drawing data between
// batches. After every kind of edit to skeletons and both kinds of animation, it must draw what a fresh load of that world draws.
#[test]
fn every_kind_of_animation_edit_draws_like_a_fresh_load() -> Result<()> {
    let mut world = StoredWorldRenderable::new(animated_asset()?)?;
    let ObjectInfo::SkAnimation(sk_animation_info) = world.objects()[&114].info.clone() else {
        unreachable!()
    };
    let ObjectInfo::PropertyAnimation(property_animation_info) = world.objects()[&119].info.clone() else {
        unreachable!()
    };
    let joint = |x: f32| {
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(x, 1.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        })
    };
    let pose = |x: f32| {
        ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
            jointName: "joint".into(),
            localPose: Location3D {
                position: Vec3::new(x, 0.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        })
    };
    let moved_rest_pose = ObjectInfo::SkJoint(SkJointInfo {
        restPose: Location3D {
            position: Vec3::new(0.5, 2.0, 0.0),
            quaternion: Quat::from_rotation_x(0.3),
        },
    });
    let steps: Vec<(&str, Vec<(ObjectId, ObjectDelta)>)> = vec![
        ("rename the joint", vec![(106, modified(None, Some("hip"), None))]),
        (
            "move the joint's rest pose and rename it back",
            vec![(106, modified(Some(moved_rest_pose), Some("joint"), None))],
        ),
        (
            "add joints under the joint",
            vec![(300, added("tip", joint(1.0), 106)), (301, added("spare", joint(2.0), 106))],
        ),
        ("remove a joint", vec![(300, ObjectDelta::Remove(()))]),
        (
            "change a keyframe's time",
            vec![(200, modified(Some(ObjectInfo::SkKeyframe(SkKeyframeInfo { time: 0.5 })), None, None))],
        ),
        (
            "add a keyframe with its pose",
            vec![
                (302, added("last pose", ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time: 1.5 }), 114)),
                (303, added("joint pose", pose(4.0), 302)),
            ],
        ),
        ("remove a pose", vec![(116, ObjectDelta::Remove(()))]),
        (
            "add a second skeleton with a joint and an animation",
            vec![
                (310, added("second skeleton", ObjectBeingAdded::Skeleton(SkeletonInfo {}), 100)),
                (311, added("joint", joint(0.0), 310)),
                (312, added("second animation", ObjectBeingAdded::SkAnimation(sk_animation_info), 310)),
                (313, added("first pose", ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time: 0.0 }), 312)),
                (314, added("joint pose", pose(1.0), 313)),
            ],
        ),
        ("move a joint to the second skeleton", vec![(301, modified(None, None, Some(310)))]),
        ("move a keyframe to the second animation", vec![(302, modified(None, None, Some(312)))]),
        (
            "change a property keyframe's value",
            vec![(
                202,
                modified(Some(ObjectInfo::PropertyKeyframe(PropertyKeyframeInfo { time: 1.0, value: 0.4 })), None, None),
            )],
        ),
        (
            "add a property keyframe",
            vec![(
                320,
                added(
                    "opacity later",
                    ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 1.5, value: 0.1 }),
                    119,
                ),
            )],
        ),
        ("remove a property keyframe", vec![(120, ObjectDelta::Remove(()))]),
        (
            "move a property keyframe to a new property animation",
            vec![
                (
                    321,
                    added("second property animation", ObjectBeingAdded::PropertyAnimation(property_animation_info), 100),
                ),
                (202, modified(None, None, Some(321))),
            ],
        ),
        (
            "rename something that is not part of an animation",
            vec![(101, modified(None, Some("ball"), None))],
        ),
    ];
    for (step, deltas) in steps {
        world.apply_object_deltas(deltas, HashMap::new())?;
        assert_draws_like_a_fresh_load(&world, step)?;
    }
    let second_skeleton_ids = world.object_and_descendant_ids(310);
    world.apply_object_deltas(
        second_skeleton_ids.into_iter().map(|id| (id, ObjectDelta::Remove(()))).collect(),
        HashMap::new(),
    )?;
    assert_draws_like_a_fresh_load(&world, "remove the second skeleton and everything under it")?;
    Ok(())
}

#[test]
fn renderable_property_animations_turn_an_object_around_its_pivot() -> Result<()> {
    let pivot_offset = Vec3::new(0.0, 0.0, 1.5);
    let mut stored = animated_asset()?;
    let ObjectInfo::Pivot(pivot) = &mut stored.objectOfId.get_mut(&123).unwrap().info else {
        unreachable!()
    };
    pivot.physicalObjectId = Some(ObjectIdPointer(101));
    pivot.offsetFromObject = pivot_offset;
    let ObjectInfo::PropertyAnimator(animator) = &mut stored.objectOfId.get_mut(&121).unwrap().info else {
        unreachable!()
    };
    animator.propertyPath = vec!["location".into(), "rotation".into(), "y".into()];
    animator.weight = 100.0;
    animator.weightTransition = None;
    let ObjectInfo::PropertyAnimation(animation) = &mut stored.objectOfId.get_mut(&119).unwrap().info else {
        unreachable!()
    };
    animation.duration = 1.0;
    animation.interpolationStyle = InterpolationStyle::Linear;
    for (id, value) in [(120, 0.0), (202, 90.0)] {
        let ObjectInfo::PropertyKeyframe(frame) = &mut stored.objectOfId.get_mut(&id).unwrap().info else {
            unreachable!()
        };
        frame.value = value;
    }
    // the ball starts unrotated, so its pivot point is simply its position plus the offset
    let pivot_point = stored.physicsInfo3DOfId[&101].location.position + pivot_offset;

    let mut world = World::fromStoredWorldString(&serde_json::to_string(&stored)?, WorldControlMode::Server, None)?;
    world.step_physics(0, 1.0 / 60.0, 0.0, Some(0.0))?;
    // a server world numbers its objects again when it loads, so find the ball by name
    let ball_object_id = world.getObjectIdsByName("ball-101")[0];
    let source = StoredWorldRenderable::new(serde_json::from_str(&world.toStoredWorldString()?)?)?;
    let sources: [&dyn Renderable; 2] = [&world, &source];
    for source in sources {
        let snapshot = source.getRenderSnapshot();
        assert_eq!(snapshot.pivot_offset_of_id.get(&ball_object_id), Some(&pivot_offset));
        for (client_ms, expected_y) in [(250.0, 22.5_f32), (750.0, 67.5)] {
            let overrides = computePropertyOverridesOfObjectIdAtTime(
                &snapshot.property_overrides_of_id,
                &snapshot.rotation_base_degrees_of_id,
                2000.0,
                Some(client_ms),
            );
            let mut render_object_of_id = snapshot.render_object_of_id.clone();
            applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &overrides, &snapshot.pivot_offset_of_id);
            let Some(RenderObject::Ball(ball)) = render_object_of_id.get(&ball_object_id) else {
                unreachable!()
            };
            let rotation = Quat::from_rotation_y(expected_y.to_radians());
            let location = &ball.transform.physicsInfo3D.location;
            assert!(
                location.quaternion.dot(rotation).abs() > 1.0 - 1e-5,
                "rotation at {client_ms}ms: {:?}",
                location.quaternion
            );
            assert!(
                (location.position - (pivot_point - rotation * pivot_offset)).length() < 1e-4,
                "position at {client_ms}ms: {:?}",
                location.position
            );
        }
    }
    Ok(())
}

#[test]
fn renderable_property_animations_preserve_unanimated_rotation_axes() -> Result<()> {
    let mut stored = animated_asset()?;
    // x 30, y 15, z -20, which apply y first, then x, then z
    stored.physicsInfo3DOfId.get_mut(&101).unwrap().location.quaternion =
        Quat::from_rotation_y(15.0_f32.to_radians()) * Quat::from_rotation_x(30.0_f32.to_radians()) * Quat::from_rotation_z(-20.0_f32.to_radians());
    let ObjectInfo::PropertyAnimator(animator) = &mut stored.objectOfId.get_mut(&121).unwrap().info else {
        unreachable!()
    };
    animator.propertyPath = vec!["location".into(), "rotation".into(), "y".into()];
    animator.weight = 100.0;
    animator.weightTransition = None;
    let ObjectInfo::PropertyAnimation(animation) = &mut stored.objectOfId.get_mut(&119).unwrap().info else {
        unreachable!()
    };
    animation.duration = 1.0;
    animation.interpolationStyle = InterpolationStyle::Linear;
    for (id, value) in [(120, 15.0), (202, 75.0)] {
        let ObjectInfo::PropertyKeyframe(frame) = &mut stored.objectOfId.get_mut(&id).unwrap().info else {
            unreachable!()
        };
        frame.value = value;
    }

    let mut world = World::fromStoredWorldString(&serde_json::to_string(&stored)?, WorldControlMode::Server, None)?;
    // Capture the live world's rotation base at the animation's starting pose before saving it.
    world.step_physics(0, 1.0 / 60.0, 0.0, Some(0.0))?;
    let source = StoredWorldRenderable::new(serde_json::from_str(&world.toStoredWorldString()?)?)?;
    let sources: [&dyn Renderable; 2] = [&world, &source];
    for source in sources {
        let snapshot = source.getRenderSnapshot();
        for (client_ms, expected_y) in [(0.0, 15.0), (250.0, 30.0), (750.0, 60.0)] {
            let overrides = computePropertyOverridesOfObjectIdAtTime(
                &snapshot.property_overrides_of_id,
                &snapshot.rotation_base_degrees_of_id,
                2000.0,
                Some(client_ms),
            );
            assert_eq!(overrides.len(), 1);
            let property_override_of_path = overrides.values().next().unwrap();
            for (axis, expected) in [("x", 30.0), ("y", expected_y), ("z", -20.0)] {
                let path = vec!["location".into(), "rotation".into(), axis.into()];
                assert!((property_override_of_path[&path] - expected).abs() < 0.0001);
            }
        }
    }
    Ok(())
}

#[test]
fn saved_scene_rejects_stale_versions_and_cyclic_hierarchies() -> Result<()> {
    let mut stored = animated_asset()?;
    stored.worldSchemaVersion += 1;
    assert!(StoredWorldRenderable::new(stored).is_err());
    let mut stored = animated_asset()?;
    stored.childrenOfId.entry(102).or_default().push(stored.rootObjectId);
    assert!(StoredWorldRenderable::new(stored).is_err());
    Ok(())
}

// Multiple legal attachment pointers must resolve identically in physics, live rendering, and saved previews.
#[test]
fn attachment_selection_stays_consistent_after_removing_duplicate_attachments() -> Result<()> {
    let mut stored = animated_asset()?;
    for id in [250, 80] {
        let mut attachment = stored.objectOfId[&117].clone();
        attachment.id = id;
        attachment.name = format!("attachment-{id}");
        let ObjectInfo::SkAttachment(info) = &mut attachment.info else {
            unreachable!()
        };
        info.offsetFromJoint.position = Vec3::new(id as f32, 0.0, 0.0);
        stored.objectOfId.insert(id, attachment);
        stored.childrenOfId.entry(105).or_default().push(id);
    }
    let mut world = World::fromStoredWorldString(&serde_json::to_string(&stored)?, WorldControlMode::Server, None)?;
    // Loading remaps object ids; compare the three attachment pointers in the resulting live world.
    let mut attachment_ids: Vec<_> = world
        .objectOfId
        .values()
        .filter_map(|object| match object.info {
            ObjectInfo::SkAttachment(_) => Some(object.id),
            _ => None,
        })
        .collect();
    attachment_ids.sort_unstable();
    assert_eq!(attachment_ids.len(), 3);
    let ObjectInfo::SkAttachment(first) = &world.objectOfId[&attachment_ids[0]].info else {
        unreachable!()
    };
    let target_id = first.objectId.unwrap().into();
    for attachment_id in attachment_ids {
        let ObjectInfo::SkAttachment(expected) = &world.objectOfId[&attachment_id].info else {
            unreachable!()
        };
        let physics_attachment = world.getSkJointThatObjectFollows(target_id).unwrap();
        assert_eq!(physics_attachment.offset_from_joint.position, expected.offsetFromJoint.position);
        assert_eq!(physics_attachment.offset_from_joint.quaternion, expected.offsetFromJoint.quaternion);
        let preview = StoredWorldRenderable::new(serde_json::from_str(&world.toStoredWorldString()?)?)?;
        for source in [&world as &dyn Renderable, &preview as &dyn Renderable] {
            let snapshot = source.getRenderSnapshot();
            let transform = snapshot.render_object_of_id[&target_id].getTransform().unwrap();
            let attachment = transform.sk_joint_to_follow_arc.as_ref().unwrap();
            assert_eq!(attachment.skeleton_id, physics_attachment.skeleton_id);
            assert_eq!(attachment.joint_name, physics_attachment.joint_name);
            assert_eq!(attachment.offset_from_joint.position, physics_attachment.offset_from_joint.position);
            assert_eq!(attachment.offset_from_joint.quaternion, physics_attachment.offset_from_joint.quaternion);
            assert_eq!(attachment.follower_group_id, physics_attachment.follower_group_id);
        }
        world.removeObject(attachment_id)?;
    }
    assert!(world.getSkJointThatObjectFollows(target_id).is_none());
    let preview = StoredWorldRenderable::new(serde_json::from_str(&world.toStoredWorldString()?)?)?;
    for source in [&world as &dyn Renderable, &preview as &dyn Renderable] {
        let snapshot = source.getRenderSnapshot();
        assert!(
            snapshot.render_object_of_id[&target_id]
                .getTransform()
                .unwrap()
                .sk_joint_to_follow_arc
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn saved_asset_animations_change_real_gpu_pixels() -> Result<()> {
    use render_core::{
        camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
        renderer::Renderer,
    };
    let source = StoredWorldRenderable::new(animated_asset()?)?;
    let camera_position = Vec3::new(10.0, 10.0, 20.0);
    let target = Vec3::new(0.0, 3.0, 0.0);
    let snapshot = source.getRenderSnapshot();
    let base = &snapshot.render_object_of_id;
    let mesh_data_of_id = HashMap::from([(
        10,
        Arc::new(MeshData::SkinnedMeshData {
            vertices: vec![Vec3::ZERO, Vec3::X * 3.0, Vec3::Y * 3.0]
                .into_iter()
                .map(|position| MeshVertexInternal {
                    position,
                    normal: Vec3::Z,
                    uv: Vec2::ZERO,
                    tangent: Vec4::X,
                    skinning: SkinningInfoPerVertex {
                        jointIndices: [0; 4],
                        jointWeights: [1.0, 0.0, 0.0, 0.0],
                    },
                })
                .collect(),
            indices: vec![0, 1, 2],
            jointNameOfIdx: vec!["joint".into()],
            inverseBindMatrixOfJointName: HashMap::from([("joint".into(), Mat4::IDENTITY.to_cols_array())]),
        }),
    )]);
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, 256, 256)?;
    renderer.load_mesh_into_gpu(10, &mesh_data_of_id[&10]);
    renderer.load_texture_into_gpu(20, &[vec![128, 128, 255, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, false)?;
    let (_, camera_bind_group) = renderer.create_camera_matrix_buffer_pair(camera_position, target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let mut images = Vec::new();
    for elapsed_ms in [0.0, 750.0] {
        let mut objects = base.clone();
        let overrides = computePropertyOverridesOfObjectIdAtTime(
            &snapshot.property_overrides_of_id,
            &snapshot.rotation_base_degrees_of_id,
            elapsed_ms,
            Some(elapsed_ms),
        );
        applyPropertyOverridesToRenderObjectOfId(&mut objects, &overrides, &HashMap::new());
        let poses = computeSkeletonPoseOfIdAtTime(&objects, elapsed_ms, Some(elapsed_ms));
        let scene = build_render_scene(&objects, &mesh_data_of_id, Some(&poses));
        let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, 256, 256);
        renderer.render_scene(
            &mut frame,
            &scene,
            None,
            &camera_bind_group,
            target,
            camera_position,
            build_camera_view_proj_matrix(camera_position, target, 1.0, DEFAULT_CAMERA_FOV_DEGREES),
        );
        let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 256 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        frame.encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1024),
                    rows_per_image: Some(256),
                },
            },
            texture.size(),
        );
        renderer.finish_frame(frame);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        images.push(buffer.slice(..).get_mapped_range().to_vec());
        buffer.unmap();
    }
    assert!(renderer.device().pop_error_scope().await.is_none());
    let changed = images[0].chunks_exact(4).zip(images[1].chunks_exact(4)).filter(|(a, b)| a != b).count();
    assert!(changed > 100, "Animated asset geometry did not change rendered pixels: {changed}");
    Ok(())
}
