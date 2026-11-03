mod helpers;

use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    object_types::{InterpolationStyle, Location3D, ObjectBeingAdded, ObjectId, ObjectInfo, PropertyAnimationInfo, PropertyKeyframeInfo, SkAnimationInfo},
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::make_world;
use render_core::{
    collect_animation_content::{collect_property_keyframes, collect_sk_keyframes},
    dehydrated_gltf::{DehydratedAnimation, DehydratedGltf, DehydratedMesh, DehydratedSkeleton},
    renderer_types::{RenderSkJoint, RenderSkKeyframe, RenderSkKeyframePose},
};
use shared::game::{
    add_dehydrated_gltf_to_world::add_dehydrated_gltf_to_world,
    world::{World, WorldControlMode},
};
use std::time::{Duration, Instant};

// TODO: AI-PICKED-VALUE: 60 joints posed at 60 keyframes is 3,600 poses. Rebuilding the animation after every pose takes
// minutes in a debug build, while rebuilding it once per change takes well under a second.
const LONG_ANIMATION_JOINT_COUNT: usize = 60;
const LONG_ANIMATION_KEYFRAME_COUNT: usize = 60;
// TODO: AI-PICKED-VALUE: Generous for a slow debug build, and still far below what rebuilding after every pose costs.
const MAX_LONG_ANIMATION_ADD_DURATION: Duration = Duration::from_secs(20);

// A rig like the phoenix asset: one skinned mesh, one skeleton, and one animation whose keyframes each pose every joint.
fn rig_gltf(joint_count: usize, keyframe_count: usize) -> DehydratedGltf {
    let joint_names: Vec<String> = (0..joint_count).map(|index| format!("joint_{index}")).collect();
    DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes: vec![DehydratedMesh {
            name: "mesh".to_string(),
            mesh_data_id: 1,
            skinned_skeleton_index: Some(0),
            material: None,
        }],
        dehydrated_skeletons: vec![DehydratedSkeleton {
            skeleton_index: 0,
            jointOfIndex: joint_names
                .iter()
                .enumerate()
                .map(|(index, name)| RenderSkJoint {
                    name: name.clone(),
                    parent_joint_index: index.checked_sub(1),
                    rest_pose: Location3D {
                        position: Vec3::ZERO,
                        quaternion: Quat::IDENTITY,
                    },
                })
                .collect(),
            animations: vec![DehydratedAnimation {
                name: "Flap".to_string(),
                sk_animation_info: SkAnimationInfo {
                    duration: 1.0,
                    interpolationStyle: InterpolationStyle::Linear,
                },
                sk_keyframes: (0..keyframe_count)
                    .map(|keyframe_index| RenderSkKeyframe {
                        time: keyframe_index as f32 / keyframe_count as f32,
                        skKeyframePoses: joint_names
                            .iter()
                            .map(|name| RenderSkKeyframePose {
                                jointName: name.clone(),
                                localPose: Location3D {
                                    position: Vec3::X * keyframe_index as f32,
                                    quaternion: Quat::IDENTITY,
                                },
                            })
                            .collect(),
                    })
                    .collect(),
            }],
        }],
    }
}

fn add_property_animation(world: &mut World, keyframe_count: usize) -> ObjectId {
    let property_animation_id = world.addObject(
        "Bob".to_string(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        world.getRootObjectId(),
    );
    for index in 0..keyframe_count {
        world.addObject(
            format!("Bob key {index}"),
            ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo {
                time: index as f32,
                value: index as f32,
            }),
            property_animation_id,
        );
    }
    property_animation_id
}

// Every animation's cached keyframes must match a rebuild from its objects as they are now. Returns how many were checked.
fn check_keyframe_caches_are_current(world: &World) -> anyhow::Result<usize> {
    let children = |id| world._childrenOfId.get(&id).into_iter().flatten().copied().collect();
    let mut checked_count = 0;
    for (id, object) in &world.objectOfId {
        let (cached, rebuilt) = match &object.info {
            ObjectInfo::SkAnimation(_) => (
                serde_json::to_value(world.renderSkKeyframesOfSkAnimationId.get(id))?,
                serde_json::to_value(Some(collect_sk_keyframes(&world.objectOfId, &children, *id)))?,
            ),
            ObjectInfo::PropertyAnimation(_) => (
                serde_json::to_value(
                    world
                        .propertyKeyframesArcOfPropertyAnimationId
                        .get(id)
                        .map(|keyframes_arc| keyframes_arc.as_ref()),
                )?,
                serde_json::to_value(Some(collect_property_keyframes(&world.objectOfId, &children, *id)))?,
            ),
            _ => continue,
        };
        assert_eq!(cached, rebuilt, "Animation {id} ({}) has stale keyframes", object.name);
        checked_count += 1;
    }
    Ok(checked_count)
}

// Regression: each added pose used to rebuild the whole animation, so adding the phoenix froze the editor for 30 seconds.
#[test]
fn adding_a_long_animation_builds_its_keyframes_once_the_whole_rig_is_in() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let started = Instant::now();
    add_dehydrated_gltf_to_world(&mut world, "Rig", rig_gltf(LONG_ANIMATION_JOINT_COUNT, LONG_ANIMATION_KEYFRAME_COUNT), root_id)?;
    let elapsed = started.elapsed();
    assert!(elapsed < MAX_LONG_ANIMATION_ADD_DURATION, "Adding the rig took {elapsed:?}");
    assert_eq!(check_keyframe_caches_are_current(&world)?, 1);
    let pose_count: usize = world
        .renderSkKeyframesOfSkAnimationId
        .values()
        .flatten()
        .map(|keyframe| keyframe.skKeyframePoses.len())
        .sum();
    assert_eq!(pose_count, LONG_ANIMATION_JOINT_COUNT * LONG_ANIMATION_KEYFRAME_COUNT);
    Ok(())
}

// Cloning, pasting a template, loading a saved world, and replacing a world each add many objects in one change.
#[test]
fn every_change_of_many_objects_leaves_keyframes_current() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let rig_id = add_dehydrated_gltf_to_world(&mut world, "Rig", rig_gltf(4, 3), root_id)?;
    add_property_animation(&mut world, 5);
    world.addClonedObjectRecursive(rig_id, "Rig copy".to_string())?;
    assert_eq!(check_keyframe_caches_are_current(&world)?, 3);

    world.cutObjectToTemplate(rig_id, "Rig template".to_string())?;
    world.pasteTemplate("Rig template", world.getRootObjectId())?;
    assert_eq!(check_keyframe_caches_are_current(&world)?, 3);

    let reloaded = World::fromStoredWorldString(&world.toStoredWorldString()?, WorldControlMode::Editor, None)?;
    assert_eq!(check_keyframe_caches_are_current(&reloaded)?, 3);

    let mut replaced = make_world()?;
    replaced.loadNewWorld(&reloaded)?;
    assert_eq!(check_keyframe_caches_are_current(&replaced)?, 3);
    Ok(())
}

// Outside a change of many objects, an edit still updates its animation's keyframes before it returns.
#[test]
fn a_single_edit_still_updates_keyframes_right_away() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_dehydrated_gltf_to_world(&mut world, "Rig", rig_gltf(4, 3), root_id)?;
    let pose_id = world
        .objectOfId
        .iter()
        .find_map(|(id, object)| match object.info {
            ObjectInfo::SkKeyframePose(_) => Some(*id),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("The rig has no poses"))?;
    world.removeObject(pose_id)?;
    assert_eq!(check_keyframe_caches_are_current(&world)?, 1);

    let property_animation_id = add_property_animation(&mut world, 2);
    world.addObject(
        "Late key".to_string(),
        ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 5.0, value: 5.0 }),
        property_animation_id,
    );
    assert_eq!(check_keyframe_caches_are_current(&world)?, 2);
    Ok(())
}
