use std::collections::HashMap;

use game_types::object_types::{
    Location3D, ObjectId,
    shared::{euler_degrees_to_quat, quat_to_euler_degrees},
};
use glam::{Quat, Vec3};
use render_core::{
    computePropertyValue::computeRenderSkeletonInfoWithRestPoseOverrides,
    computeSkeletonInfo::computeJointIndexOfName,
    renderer_types::{RenderSkJoint, RenderSkeletonInfo},
};

const SHOULDER_OBJECT_ID: ObjectId = 7;
const ELBOW_OBJECT_ID: ObjectId = 8;

// The shoulder rests tilted 30 degrees around x, so an animator on one rotation axis has two axes to keep.
fn make_arm_skeleton_info() -> RenderSkeletonInfo {
    let jointOfIndex = vec![
        RenderSkJoint {
            name: "Shoulder".to_string(),
            parent_joint_index: None,
            rest_pose: Location3D {
                position: Vec3::new(0.0, 1.0, 0.0),
                quaternion: euler_degrees_to_quat(Vec3::new(30.0, 0.0, 0.0)),
            },
        },
        RenderSkJoint {
            name: "Elbow".to_string(),
            parent_joint_index: Some(0),
            rest_pose: Location3D {
                position: Vec3::new(0.0, -1.0, 0.0),
                quaternion: Quat::IDENTITY,
            },
        },
    ];
    let jointIndexOfName = computeJointIndexOfName(&jointOfIndex);
    RenderSkeletonInfo {
        jointOfIndex,
        jointIndexOfName,
        jointIndexOfObjectId: HashMap::from([(SHOULDER_OBJECT_ID, 0), (ELBOW_OBJECT_ID, 1)]),
    }
}

fn rest_pose_path(subfield: &str, axis: &str) -> Vec<String> {
    vec!["restPose".to_string(), subfield.to_string(), axis.to_string()]
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
fn rest_pose_overrides_replace_the_animated_axes_and_keep_the_rest() -> anyhow::Result<()> {
    let render_skeleton_info = make_arm_skeleton_info();
    let propertyOverrideOfPathOfObjectId = HashMap::from([(
        SHOULDER_OBJECT_ID,
        HashMap::from([(rest_pose_path("position", "z"), 2.0), (rest_pose_path("rotation", "y"), 90.0)]),
    )]);

    let animated_render_skeleton_info = computeRenderSkeletonInfoWithRestPoseOverrides(&render_skeleton_info, &propertyOverrideOfPathOfObjectId)
        .ok_or_else(|| anyhow::anyhow!("an animated joint must give an animated copy"))?;

    let shoulder_rest_pose = &animated_render_skeleton_info.jointOfIndex[0].rest_pose;
    assert_eq!(shoulder_rest_pose.position, Vec3::new(0.0, 1.0, 2.0));
    assert_rotation_near(
        shoulder_rest_pose.quaternion,
        euler_degrees_to_quat(Vec3::new(30.0, 90.0, 0.0)),
        "the x tilt comes from the stored rest pose",
    );
    assert_eq!(
        animated_render_skeleton_info.jointOfIndex[1].rest_pose.position, render_skeleton_info.jointOfIndex[1].rest_pose.position,
        "joints nobody animates keep their rest pose"
    );
    assert_rotation_near(
        render_skeleton_info.jointOfIndex[0].rest_pose.quaternion,
        euler_degrees_to_quat(Vec3::new(30.0, 0.0, 0.0)),
        "the stored Skeleton is never changed",
    );
    Ok(())
}

#[test]
fn a_skeleton_with_no_animated_joints_needs_no_copy() {
    let render_skeleton_info = make_arm_skeleton_info();
    let propertyOverrideOfPathOfObjectId = HashMap::from([(99, HashMap::from([(rest_pose_path("position", "x"), 5.0)]))]);

    assert!(computeRenderSkeletonInfoWithRestPoseOverrides(&render_skeleton_info, &propertyOverrideOfPathOfObjectId).is_none());
}
