use game_types::{
    consts::MESH_DATA_CURRENT_VERSION,
    data_types::MeshData,
    object_types::{
        ClockInfo, ClockState, InterpolationStyle, LiveObject, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PropertyAnimationInfo,
        PropertyAnimatorInfo, PropertyKeyframeInfo,
    },
    stored_world_type::StoredWorld,
};
use glam::Vec3;
use render_core::{
    computePropertyValue::computePropertyOverridesOfObjectIdAtTime, computeSkeletonPose::computeSkeletonPoseOfIdAtTime,
    dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world, renderable::Renderable,
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;

const ANIMATED_MESH: &str = include_str!("../../../tests/fixtures/asset-preview-animated-mesh.json");

fn animated_world() -> anyhow::Result<StoredWorld> {
    let mut asset: serde_json::Value = serde_json::from_str(ANIMATED_MESH)?;
    dehydrated_gltf_to_stored_world("Animated mesh", serde_json::from_value(asset["Mesh"].take())?)
}

fn animated_source() -> anyhow::Result<(StoredWorldRenderable, ObjectId, Vec<ObjectId>)> {
    let world = animated_world()?;
    let source = StoredWorldRenderable::new(world)?;
    let mut animators = source
        .objects()
        .iter()
        .filter_map(|(id, object)| match &object.info {
            ObjectInfo::SkAnimator(animator) => Some((*id, ObjectId::from(animator.skeletonId.unwrap()))),
            _ => None,
        })
        .collect::<Vec<_>>();
    animators.sort();
    let (_, skeleton_id) = animators[0];
    Ok((
        source,
        skeleton_id,
        animators
            .into_iter()
            .filter(|(_, skeleton)| *skeleton == skeleton_id)
            .map(|(id, _)| id)
            .collect(),
    ))
}

fn joint_x(source: &StoredWorldRenderable, skeleton_id: ObjectId, time_ms: f64) -> f32 {
    let objects = source.getRenderSnapshot().render_object_of_id;
    let poses = computeSkeletonPoseOfIdAtTime(&objects, time_ms, Some(time_ms));
    poses[&skeleton_id].jointMatrixOfJointIndex[0].transform_point3(Vec3::ZERO).x
}

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "expected {expected}, got {actual}");
}

#[test]
fn preview_clock_controls_update_rendered_poses_and_survive_camera_changes() -> anyhow::Result<()> {
    let (mut source, skeleton, animators) = animated_source()?;
    let ObjectInfo::SkAnimator(animator) = &source.objects()[&animators[0]].info else {
        unreachable!()
    };
    let clock_id = ObjectId::from(animator.clockId.unwrap());
    assert_close(joint_x(&source, skeleton, 500.0), 0.2);
    source.pause_clock(clock_id, 500.0, Some(500.0))?;
    assert_close(joint_x(&source, skeleton, 750.0), 0.2);
    source.set_clock_speed(clock_id, -2.0, 750.0, Some(750.0))?;
    assert_close(joint_x(&source, skeleton, 1500.0), 0.2);
    source.play_clock(clock_id, 0.5, ObjectLocation::Client, 1500.0, Some(1500.0))?;
    assert_close(joint_x(&source, skeleton, 1625.0), 0.1);
    source.set_clock_speed(clock_id, 0.0, 1625.0, Some(1625.0))?;
    let ObjectInfo::Clock(clock) = &source.objects()[&clock_id].info else {
        unreachable!()
    };
    assert_eq!(clock.clockState, ClockState::Paused { animationTimeSecs: 0.25 });
    assert_close(joint_x(&source, skeleton, 1800.0), 0.1);
    Ok(())
}

#[test]
fn preview_crossfades_retarget_from_current_weight_without_reverting_on_camera_changes() -> anyhow::Result<()> {
    let (mut source, skeleton, animators) = animated_source()?;
    source.smoothly_change_weights(
        [(animators[0], 0.0), (animators[1], 100.0)],
        1000.0,
        InterpolationStyle::Linear,
        ObjectLocation::Client,
        0.0,
        Some(0.0),
    )?;
    // The moving animation is at x=.2; its weight and the x=.8 pose each contribute half.
    assert_close(joint_x(&source, skeleton, 500.0), 0.5);
    source.smoothly_change_weights(
        [(animators[0], 100.0), (animators[1], 0.0)],
        1000.0,
        InterpolationStyle::Linear,
        ObjectLocation::Client,
        500.0,
        Some(500.0),
    )?;
    assert_close(joint_x(&source, skeleton, 500.0), 0.5);
    // At one second the moving animation loops to x=0; the pose now contributes one quarter.
    assert_close(joint_x(&source, skeleton, 1000.0), 0.2);
    assert_close(joint_x(&source, skeleton, 1500.0), 0.2);
    source.smoothly_change_weights(
        [(animators[0], 0.0)],
        0.0,
        InterpolationStyle::Step,
        ObjectLocation::Client,
        1500.0,
        Some(1500.0),
    )?;
    assert_close(joint_x(&source, skeleton, 1500.0), 0.0);
    assert!(source.pause_clock(animators[0], 0.0, Some(0.0)).is_err());
    assert!(
        source
            .smoothly_change_weights(
                [(animators[0], 100.0), (skeleton, 100.0)],
                0.0,
                InterpolationStyle::Linear,
                ObjectLocation::Client,
                1500.0,
                Some(1500.0)
            )
            .is_err()
    );
    assert_close(joint_x(&source, skeleton, 1500.0), 0.0);
    Ok(())
}

#[test]
fn browser_fixture_contains_real_skinned_mesh_data() -> anyhow::Result<()> {
    let (version, mesh): (u64, MeshData) = bincode::deserialize(include_bytes!("../../../tests/fixtures/asset-preview-animated-mesh.bin"))?;
    assert_eq!(version, MESH_DATA_CURRENT_VERSION);
    let MeshData::SkinnedMeshData {
        vertices,
        indices,
        jointNameOfIdx,
        inverseBindMatrixOfJointName,
    } = mesh
    else {
        panic!("Expected skinned mesh")
    };
    assert_eq!(vertices.len(), 4);
    assert_eq!(indices.len(), 6);
    assert_eq!(jointNameOfIdx, ["root"]);
    assert_eq!(inverseBindMatrixOfJointName["root"], glam::Mat4::IDENTITY.to_cols_array());
    Ok(())
}

#[test]
fn preview_property_animators_share_clock_and_weight_controls() -> anyhow::Result<()> {
    fn add_object(stored: &mut StoredWorld, parent: ObjectId, info: ObjectInfo) -> ObjectId {
        let id = stored.objectOfId.len() + 1;
        stored.objectOfId.insert(
            id,
            LiveObject {
                id,
                name: format!("test-{id}"),
                info,
            },
        );
        stored.childrenOfId.entry(parent).or_default().push(id);
        stored.childrenOfId.insert(id, Vec::new());
        id
    }
    let mut stored = animated_world()?;
    let root = stored.rootObjectId;
    let (mesh_id, _) = stored
        .objectOfId
        .iter()
        .find(|(_, object)| match object.info {
            ObjectInfo::Mesh(_) => true,
            _ => false,
        })
        .unwrap();
    let mesh_id = *mesh_id;
    let clock_id = add_object(
        &mut stored,
        root,
        ObjectInfo::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Client,
            },
            shouldLoop: false,
            speed: 1.0,
        }),
    );
    let mut animators = Vec::new();
    for value in [0.0, 1.0] {
        let animation_id = add_object(
            &mut stored,
            root,
            ObjectInfo::PropertyAnimation(PropertyAnimationInfo {
                duration: 1.0,
                interpolationStyle: InterpolationStyle::Linear,
            }),
        );
        for time in [0.0, 1.0] {
            add_object(
                &mut stored,
                animation_id,
                ObjectInfo::PropertyKeyframe(PropertyKeyframeInfo { time, value: time * value }),
            );
        }
        animators.push(add_object(
            &mut stored,
            root,
            ObjectInfo::PropertyAnimator(PropertyAnimatorInfo {
                targetObjectId: Some(ObjectIdPointer(mesh_id)),
                propertyPath: vec!["visualSettings".to_string(), "opacity".to_string()],
                propertyAnimationId: Some(ObjectIdPointer(animation_id)),
                clockId: Some(ObjectIdPointer(clock_id)),
                priority: 0,
                weight: if value == 0.0 { 100.0 } else { 0.0 },
                weightTransition: None,
            }),
        ));
    }
    let mut source = StoredWorldRenderable::new(stored)?;
    source.pause_clock(clock_id, 500.0, Some(500.0))?;
    source.smoothly_change_weights(
        [(animators[0], 0.0), (animators[1], 100.0)],
        1000.0,
        InterpolationStyle::Linear,
        ObjectLocation::Client,
        500.0,
        Some(500.0),
    )?;
    let path = vec!["visualSettings".to_string(), "opacity".to_string()];
    let snapshot = source.getRenderSnapshot();
    let overrides = computePropertyOverridesOfObjectIdAtTime(&snapshot.property_overrides_of_id, &snapshot.rotation_base_degrees_of_id, 1000.0, Some(1000.0));
    assert_close(overrides[&mesh_id][&path], 0.25);
    let overrides = computePropertyOverridesOfObjectIdAtTime(&snapshot.property_overrides_of_id, &snapshot.rotation_base_degrees_of_id, 1500.0, Some(1500.0));
    assert_close(overrides[&mesh_id][&path], 0.5);
    source.play_clock(clock_id, 0.5, ObjectLocation::Client, 1500.0, Some(1500.0))?;
    // A retained capture keeps the paused clock; a new capture sees the resumed animation.
    let old_overrides =
        computePropertyOverridesOfObjectIdAtTime(&snapshot.property_overrides_of_id, &snapshot.rotation_base_degrees_of_id, 1750.0, Some(1750.0));
    assert_close(old_overrides[&mesh_id][&path], 0.5);
    let resumed_snapshot = source.getRenderSnapshot();
    let overrides = computePropertyOverridesOfObjectIdAtTime(
        &resumed_snapshot.property_overrides_of_id,
        &resumed_snapshot.rotation_base_degrees_of_id,
        1750.0,
        Some(1750.0),
    );
    assert_close(overrides[&mesh_id][&path], 0.75);
    Ok(())
}
