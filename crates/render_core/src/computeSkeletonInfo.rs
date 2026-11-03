//! Skeleton hierarchy and bind-pose math shared by games and stored scene previews.
use crate::renderer_types::{RenderSkJoint, RenderSkeletonInfo};
use game_types::object_types::{LiveObject, Location3D, ObjectId, ObjectInfo};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};

pub fn computeJointIndexOfName(joints: &[RenderSkJoint]) -> HashMap<String, usize> {
    let mut jointIndexOfName = HashMap::new();
    for (joint_index, joint) in joints.iter().enumerate() {
        jointIndexOfName.entry(joint.name.clone()).or_insert(joint_index);
    }
    jointIndexOfName
}

pub fn computeJointDefaultLocalLocation3D(joint: &RenderSkJoint) -> Location3D {
    let rest_pose = &joint.rest_pose;
    Location3D {
        position: rest_pose.position,
        quaternion: rest_pose.quaternion,
    }
}

fn computeGlobalBindMatrices(joints: &[RenderSkJoint]) -> Vec<Mat4> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum JointDepthVisitState {
        Unvisited,
        Visiting,
        Visited,
    }

    fn fillJointDepth(joints: &[RenderSkJoint], jointDepthOfJoint: &mut [usize], visitStateOfJoint: &mut [JointDepthVisitState], joint_index: usize) -> usize {
        match visitStateOfJoint[joint_index] {
            JointDepthVisitState::Visited => return jointDepthOfJoint[joint_index],
            JointDepthVisitState::Visiting => return 0,
            JointDepthVisitState::Unvisited => {}
        }

        visitStateOfJoint[joint_index] = JointDepthVisitState::Visiting;

        let joint = &joints[joint_index];
        let joint_depth = match joint.parent_joint_index {
            Some(parent_joint_index) if parent_joint_index < joints.len() => {
                fillJointDepth(joints, jointDepthOfJoint, visitStateOfJoint, parent_joint_index) + 1
            }
            _ => 0,
        };

        jointDepthOfJoint[joint_index] = joint_depth;
        visitStateOfJoint[joint_index] = JointDepthVisitState::Visited;
        joint_depth
    }

    let mut jointDepthOfJoint = vec![0usize; joints.len()];
    let mut visitStateOfJoint = vec![JointDepthVisitState::Unvisited; joints.len()];
    for joint_index in 0..joints.len() {
        fillJointDepth(joints, &mut jointDepthOfJoint, &mut visitStateOfJoint, joint_index);
    }

    let mut jointIndexOrder = (0..joints.len()).collect::<Vec<_>>();
    jointIndexOrder.sort_by_key(|&joint_index| jointDepthOfJoint[joint_index]);

    let mut globalBindMatrixOfJoint = vec![Mat4::IDENTITY; joints.len()];
    for joint_index in jointIndexOrder {
        let joint = &joints[joint_index];
        let rest_pose = &joint.rest_pose;
        let localBindMatrix = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_pose.quaternion, rest_pose.position);
        globalBindMatrixOfJoint[joint_index] = match joint.parent_joint_index {
            Some(parent_joint_index) if parent_joint_index < joints.len() => globalBindMatrixOfJoint[parent_joint_index] * localBindMatrix,
            _ => localBindMatrix,
        };
    }

    globalBindMatrixOfJoint
}

pub fn computeInverseBindMatrices(joints: &[RenderSkJoint]) -> Vec<Mat4> {
    let globalBindMatrixOfJoint = computeGlobalBindMatrices(joints);
    globalBindMatrixOfJoint.into_iter().map(|globalBindMatrix| globalBindMatrix.inverse()).collect()
}

// This turns the Skeleton subtree into the dense joint data the renderer and animation code expect.
pub fn computeRenderSkeletonInfo(
    object_of_id: &HashMap<ObjectId, LiveObject>,
    child_ids: &impl Fn(ObjectId) -> Vec<ObjectId>,
    skeleton_id: ObjectId,
) -> RenderSkeletonInfo {
    fn sortedJointChildIds(object_of_id: &HashMap<ObjectId, LiveObject>, child_ids: &impl Fn(ObjectId) -> Vec<ObjectId>, parent_id: ObjectId) -> Vec<ObjectId> {
        let children = child_ids(parent_id);

        let mut joint_child_ids = Vec::new();
        for child_id in children.iter().copied() {
            let Some(child_object) = object_of_id.get(&child_id) else {
                continue;
            };
            let ObjectInfo::SkJoint(_) = &child_object.info else {
                continue;
            };
            joint_child_ids.push(child_id);
        }

        // Keep the dense runtime joint order stable by following object id order.
        // Imported Skeletons create joints in their original jointOfIndex order, and
        // animation channels still target that order.
        joint_child_ids.sort_unstable();
        joint_child_ids
    }

    fn pushJointAndChildren(
        object_of_id: &HashMap<ObjectId, LiveObject>,
        child_ids: &impl Fn(ObjectId) -> Vec<ObjectId>,
        skeleton_id: ObjectId,
        joint_id: ObjectId,
        parent_joint_index: Option<usize>,
        did_visit_joint_name: &mut HashSet<String>,
        jointOfIndex: &mut Vec<RenderSkJoint>,
        jointIndexOfObjectId: &mut HashMap<ObjectId, usize>,
    ) {
        let Some(joint_object) = object_of_id.get(&joint_id) else {
            return;
        };
        let ObjectInfo::SkJoint(sk_joint_info) = &joint_object.info else {
            return;
        };

        if !did_visit_joint_name.insert(joint_object.name.clone()) {
            log::error!("Skeleton {} has duplicate joint name {}", skeleton_id, joint_object.name);
            return;
        }

        let joint_index = jointOfIndex.len();
        jointOfIndex.push(RenderSkJoint {
            name: joint_object.name.clone(),
            parent_joint_index,
            rest_pose: sk_joint_info.restPose.clone(),
        });
        jointIndexOfObjectId.insert(joint_id, joint_index);

        for child_joint_id in sortedJointChildIds(object_of_id, child_ids, joint_id) {
            pushJointAndChildren(
                object_of_id,
                child_ids,
                skeleton_id,
                child_joint_id,
                Some(joint_index),
                did_visit_joint_name,
                jointOfIndex,
                jointIndexOfObjectId,
            );
        }
    }

    let mut did_visit_joint_name = HashSet::new();
    let mut jointOfIndex = Vec::new();
    let mut jointIndexOfObjectId = HashMap::new();
    for root_joint_id in sortedJointChildIds(object_of_id, child_ids, skeleton_id) {
        pushJointAndChildren(
            object_of_id,
            child_ids,
            skeleton_id,
            root_joint_id,
            None,
            &mut did_visit_joint_name,
            &mut jointOfIndex,
            &mut jointIndexOfObjectId,
        );
    }

    let jointIndexOfName = computeJointIndexOfName(&jointOfIndex);
    RenderSkeletonInfo {
        jointOfIndex,
        jointIndexOfName,
        jointIndexOfObjectId,
    }
}
