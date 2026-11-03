mod helpers;

use game_types::object_types::{Location3D, ObjectBeingAdded, SkJointInfo, SkeletonInfo};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::make_world;

#[test]
fn skeleton_builds_render_skeleton_info_from_joint_subtree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 9100;

    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        9101,
        "hip".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(1.0, 2.0, 3.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        9102,
        "chest".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(4.0, 5.0, 6.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        9101,
    );
    world.addObjectWithId(
        9103,
        "head".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(7.0, 8.0, 9.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        9102,
    );

    let render_skeleton_info_arc = world
        .getRenderSkeletonInfoArc(skeleton_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected skeleton render skeleton info"))?;
    let render_skeleton_info = render_skeleton_info_arc.as_ref();

    assert_eq!(render_skeleton_info.jointOfIndex.len(), 3);
    assert_eq!(render_skeleton_info.jointOfIndex[0].name, "hip");
    assert_eq!(render_skeleton_info.jointOfIndex[0].parent_joint_index, None);
    assert_eq!(render_skeleton_info.jointOfIndex[1].name, "chest");
    assert_eq!(render_skeleton_info.jointOfIndex[1].parent_joint_index, Some(0));
    assert_eq!(render_skeleton_info.jointOfIndex[2].name, "head");
    assert_eq!(render_skeleton_info.jointOfIndex[2].parent_joint_index, Some(1));
    assert_eq!(render_skeleton_info.jointIndexOfName.get("hip"), Some(&0));
    assert_eq!(render_skeleton_info.jointIndexOfName.get("chest"), Some(&1));
    assert_eq!(render_skeleton_info.jointIndexOfName.get("head"), Some(&2));

    Ok(())
}

#[test]
fn skeleton_keeps_joint_order_stable_when_names_would_sort_differently() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 9200;

    world.addObjectWithId(
        skeleton_object_id,
        "skeleton".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        9201,
        "z_root".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(1.0, 0.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        9202,
        "aaa_child".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(2.0, 0.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        9201,
    );
    world.addObjectWithId(
        9203,
        "m_root".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: Vec3::new(3.0, 0.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );

    let render_skeleton_info_arc = world
        .getRenderSkeletonInfoArc(skeleton_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected skeleton render skeleton info"))?;
    let render_skeleton_info = render_skeleton_info_arc.as_ref();

    assert_eq!(render_skeleton_info.jointOfIndex[0].name, "z_root");
    assert_eq!(render_skeleton_info.jointOfIndex[1].name, "aaa_child");
    assert_eq!(render_skeleton_info.jointOfIndex[2].name, "m_root");
    assert_eq!(render_skeleton_info.jointOfIndex[1].parent_joint_index, Some(0));

    Ok(())
}
