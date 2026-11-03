use game_types::{
    object_delta::{ObjectAddDelta, ObjectDelta, ObjectModifyDelta},
    object_types::{ObjectBeingAdded, ObjectId, ObjectInfo},
    stored_world_type::StoredWorld,
};
use render_core::{
    renderable::{RenderSnapshot, Renderable},
    renderer_types::{RenderObject, RenderSkeleton, RenderSkeletonInfo},
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;
use std::{collections::HashMap, sync::Arc};

// Skeletons 2 and 18 each have one joint and two animations. Skeleton 2's joint is 3, and pose 6 is in its first animation.
fn animated_world() -> anyhow::Result<StoredWorldRenderable> {
    let mut item: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/asset-preview-animated-object.json"))?;
    let stored: StoredWorld = serde_json::from_value(item["Object"].take())?;
    StoredWorldRenderable::new(stored)
}

fn drawn_skeleton(snapshot: &RenderSnapshot, skeleton_id: ObjectId) -> &RenderSkeleton {
    let RenderObject::Skeleton(skeleton) = &snapshot.render_object_of_id[&skeleton_id] else {
        panic!("object {skeleton_id} is not drawn as a skeleton");
    };
    skeleton
}

fn skeleton_info_arc(snapshot: &RenderSnapshot, skeleton_id: ObjectId) -> Arc<RenderSkeletonInfo> {
    drawn_skeleton(snapshot, skeleton_id).renderSkeletonInfoArc.clone()
}

fn num_poses(snapshot: &RenderSnapshot, skeleton_id: ObjectId) -> usize {
    drawn_skeleton(snapshot, skeleton_id)
        .render_sk_animations
        .iter()
        .flat_map(|animation| &animation.skKeyframes)
        .map(|keyframe| keyframe.skKeyframePoses.len())
        .sum()
}

fn renamed(name: &str) -> ObjectDelta {
    ObjectDelta::Modify(ObjectModifyDelta {
        modifiedObjectInfo: None,
        modifiedPhysicsToggles3D: None,
        modifiedPhysicsInfo3D: None,
        modifiedPrimitiveSettings: None,
        modifiedParent: None,
        modifiedName: Some(name.to_string()),
    })
}

// Drawing the same world again, as every camera move does, reuses each skeleton's data instead of rebuilding it.
#[test]
fn drawing_again_reuses_each_skeletons_data() -> anyhow::Result<()> {
    let world = animated_world()?;
    let first = world.getRenderSnapshot();
    let second = world.getRenderSnapshot();
    for skeleton_id in [2, 18] {
        assert!(Arc::ptr_eq(&skeleton_info_arc(&first, skeleton_id), &skeleton_info_arc(&second, skeleton_id)));
    }
    Ok(())
}

// Renaming a joint, or adding one under another joint, rebuilds that skeleton's data and no other skeleton's.
#[test]
fn a_joint_change_rebuilds_only_its_skeleton() -> anyhow::Result<()> {
    let mut world = animated_world()?;
    let before = world.getRenderSnapshot();

    world.apply_object_deltas(vec![(3, renamed("hip"))], HashMap::new())?;
    let after_rename = world.getRenderSnapshot();
    assert!(skeleton_info_arc(&after_rename, 2).jointIndexOfName.contains_key("hip"));
    assert!(Arc::ptr_eq(&skeleton_info_arc(&before, 18), &skeleton_info_arc(&after_rename, 18)));

    let ObjectInfo::SkJoint(joint_info) = world.objects()[&3].info.clone() else {
        unreachable!()
    };
    let tail = ObjectAddDelta {
        name: "tail".to_string(),
        objectBeingAdded: ObjectBeingAdded::SkJoint(joint_info),
        parentId: 3,
        initSubitemRelativeLocation3D: None,
        teleportCount: 0,
    };
    world.apply_object_deltas(vec![(500, ObjectDelta::Add(tail))], HashMap::new())?;
    let after_add = world.getRenderSnapshot();
    assert!(skeleton_info_arc(&after_add, 2).jointIndexOfName.contains_key("tail"));
    assert!(Arc::ptr_eq(&skeleton_info_arc(&before, 18), &skeleton_info_arc(&after_add, 18)));
    Ok(())
}

// Removing a keyframe pose rebuilds its animation, while the other skeleton's animations and both skeletons keep their data.
#[test]
fn removing_a_pose_rebuilds_only_its_animation() -> anyhow::Result<()> {
    let mut world = animated_world()?;
    let before = world.getRenderSnapshot();
    world.apply_object_deltas(vec![(6, ObjectDelta::Remove(()))], HashMap::new())?;
    let after = world.getRenderSnapshot();
    assert_eq!(num_poses(&after, 2), num_poses(&before, 2) - 1);
    assert_eq!(num_poses(&after, 18), num_poses(&before, 18));
    for skeleton_id in [2, 18] {
        assert!(Arc::ptr_eq(&skeleton_info_arc(&before, skeleton_id), &skeleton_info_arc(&after, skeleton_id)));
    }
    Ok(())
}
