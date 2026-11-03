use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::computeBlendedWeights::{AnimationBlendSample, AnimationBlendWeights, computeBlendedWeights};
use crate::computeSkeletonInfo::{computeJointDefaultLocalLocation3D, computeJointIndexOfName};
use crate::renderer_types::{RenderObject, RenderSkAnimation, RenderSkJoint, RenderSkKeyframe, SkJointToFollow};
use game_types::object_types::{InterpolationStyle, Location3D, ObjectId, SkAnimationInfo, WeightTransition};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkeletonPose {
    pub jointMatrixOfJointIndex: Vec<Mat4>, // the *posed* joint matrices
}

#[derive(Clone, Debug)]
pub struct SkeletonLocalAnimationSample {
    pub local_transforms: Vec<Option<(Vec3, Quat, Vec3)>>, // None for the joints this animation doesn't key
    pub priority: u8,
    pub computed_weight: f32, // actual computed weight, not just the target weight
}

// This computes the current pose of every render Skeleton so redraw paths can share the same per-frame sampling logic.
pub fn computeSkeletonPoseOfIdAtTime(
    render_object_of_id: &HashMap<ObjectId, RenderObject>,
    server_ms: f64,
    client_ms: Option<f64>,
) -> HashMap<ObjectId, SkeletonPose> {
    let mut skeleton_pose_of_id = HashMap::new();
    for (object_id, render_object) in render_object_of_id.iter() {
        let RenderObject::Skeleton(render_skeleton) = render_object else {
            continue;
        };
        let render_skeleton_info = render_skeleton.renderSkeletonInfoArc.as_ref();
        let skeleton_pose = match computeSkeletonPoseAtTime(&render_skeleton_info.jointOfIndex, &render_skeleton.render_sk_animations, server_ms, client_ms) {
            Ok(skeleton_pose) => skeleton_pose,
            Err(error) => {
                log::error!("Error computing Skeleton pose for {}: {}", object_id, error);
                continue;
            }
        };
        skeleton_pose_of_id.insert(*object_id, skeleton_pose);
    }
    skeleton_pose_of_id
}

// This turns the resolved render Skeleton animations for one Skeleton into the blended pose at a specific time.
pub fn computeSkeletonPoseAtTime(
    jointOfIndex: &[RenderSkJoint],
    render_sk_animations: &[RenderSkAnimation],
    server_ms: f64,
    client_ms: Option<f64>,
) -> anyhow::Result<SkeletonPose> {
    let joint_count = jointOfIndex.len();
    let mut animation_samples = Vec::new();
    for render_skeleton_animation in render_sk_animations {
        let RenderSkAnimation {
            skAnimationInfo,
            skKeyframes,
            clock_state,
            should_loop,
            speed,
            priority,
            weight,
            weight_transition,
        } = render_skeleton_animation;

        let animation_time_secs = clock_state.readTimeSecs(*speed, *should_loop, skAnimationInfo.duration, server_ms, client_ms);
        let computed_weight = WeightTransition::readValueWithFallback(*weight_transition, *weight, server_ms, client_ms);
        let local_transforms = computeLocalTransformOfJointAtTime(jointOfIndex, skAnimationInfo, skKeyframes, animation_time_secs);
        if local_transforms.len() != joint_count {
            return Err(anyhow::anyhow!("Skeleton local transform count mismatch"));
        }
        animation_samples.push(SkeletonLocalAnimationSample {
            local_transforms,
            priority: *priority,
            computed_weight,
        });
    }

    Ok(computeSkeletonPoseFromMultiSamples(jointOfIndex, &animation_samples))
}

// This keeps rigid-group physics and primitive rendering on the same joint-follow math.
//
// A posed joint is not a world position: it says where the joint sits relative to the rigid group
// that follows the Skeleton. A followed part's world transform is three steps chained together:
//   (where the group is) * (where the joint sits within the group) * (the part's offset from the joint)
// Because "where the group is" is part of the chain, moving the group (player controls, scripts,
// or physics) moves the whole animated character with it.
pub fn computeFollowedSkJointMatrix(
    pose: &SkeletonPose,
    joint_index: usize,
    offset_from_joint: &Location3D,
    follower_group_location: &Location3D,
) -> Option<Mat4> {
    let sk_keyframe_pose = pose.jointMatrixOfJointIndex.get(joint_index).copied()?;
    let follower_group_matrix = Mat4::from_rotation_translation(follower_group_location.quaternion, follower_group_location.position);
    Some(follower_group_matrix * sk_keyframe_pose * Mat4::from_rotation_translation(offset_from_joint.quaternion, offset_from_joint.position))
}

pub fn computeFollowedSkJointMatrixGivenSkJointToFollow(
    render_object_of_id: &HashMap<ObjectId, RenderObject>,
    skeleton_pose_of_id: &HashMap<ObjectId, SkeletonPose>,
    sk_joint_to_follow: &SkJointToFollow,
) -> Option<Mat4> {
    let Some(render_object) = render_object_of_id.get(&sk_joint_to_follow.skeleton_id) else {
        return None;
    };
    let RenderObject::Skeleton(render_skeleton) = render_object else {
        return None;
    };
    let render_skeleton_info = render_skeleton.renderSkeletonInfoArc.as_ref();
    let joint_index = render_skeleton_info.jointIndexOfName.get(sk_joint_to_follow.joint_name.as_str()).copied()?;
    let pose = skeleton_pose_of_id.get(&sk_joint_to_follow.skeleton_id)?;
    // Followed subitems resample their pose at redraw time, so the rigid group frame they compose with must move on the same clock as everything else.
    // Read the group's interpolated transform directly so its velocity and teleports are shared by every attached part.
    let RenderObject::RigidGroup(follower_group) = render_object_of_id.get(&sk_joint_to_follow.follower_group_id)? else {
        return None;
    };
    computeFollowedSkJointMatrix(pose, joint_index, &sk_joint_to_follow.offset_from_joint, &follower_group.physicsInfo3D.location)
}

// returns TRS. This is the joint's rest pose, which every animation offsets from.
fn computeDefaultLocalTransform(joint: &RenderSkJoint) -> (Vec3, Quat, Vec3) {
    let default_location = computeJointDefaultLocalLocation3D(joint);
    (default_location.position, default_location.quaternion, Vec3::ONE)
}

// This samples one animation into local joint transforms and remaps frames onto the current runtime Skeleton by joint name.
// Keyframe poses are offsets from each joint's rest pose, so every joint starts with no offset, and joints the animation
// doesn't key get None, which leaves them to the other animations.
pub fn computeLocalTransformOfJointAtTime(
    jointOfIndex: &[RenderSkJoint],
    skAnimationInfo: &SkAnimationInfo,
    skKeyframes: &[RenderSkKeyframe],
    animationTimeSecs: f32,
) -> Vec<Option<(Vec3, Quat, Vec3)>> {
    let mut local_offsets: Vec<(Vec3, Quat, Vec3)> = vec![(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE); jointOfIndex.len()];

    let jointIndexOfName = computeJointIndexOfName(jointOfIndex);
    applySkKeyframesAtTimeToLocalTransforms(
        skKeyframes,
        animationTimeSecs,
        skAnimationInfo.interpolationStyle,
        &jointIndexOfName,
        &mut local_offsets,
    );

    // The animation keys a joint if any of its keyframes poses it. Most animations pose every joint in their first keyframe,
    // so the search stops there.
    let mut is_keyed_of_joint = vec![false; jointOfIndex.len()];
    for sk_keyframe in skKeyframes {
        for sk_keyframe_pose in &sk_keyframe.skKeyframePoses {
            if let Some(&joint_index) = jointIndexOfName.get(&sk_keyframe_pose.jointName) {
                is_keyed_of_joint[joint_index] = true;
            }
        }
        if is_keyed_of_joint.iter().all(|is_keyed| *is_keyed) {
            break;
        }
    }

    jointOfIndex
        .iter()
        .zip(local_offsets)
        .zip(is_keyed_of_joint)
        .map(|((joint, (offset_position, offset_rotation, _offset_scale)), is_keyed)| {
            is_keyed.then(|| {
                let (rest_position, rest_rotation, rest_scale) = computeDefaultLocalTransform(joint);
                (
                    rest_position + rest_rotation * offset_position,
                    (rest_rotation * offset_rotation).normalize(),
                    rest_scale,
                )
            })
        })
        .collect()
}

fn applySkKeyframesAtTimeToLocalTransforms(
    skKeyframes: &[RenderSkKeyframe],
    animationTimeSecs: f32,
    interpolationStyle: InterpolationStyle,
    jointIndexOfName: &HashMap<String, usize>,
    local_transforms: &mut [(Vec3, Quat, Vec3)],
) {
    let (previous_frame, next_frame) = skKeyframesSurroundingTime(skKeyframes, animationTimeSecs);
    let Some(previous_frame) = previous_frame else {
        return;
    };
    let next_frame = next_frame.unwrap_or(previous_frame);
    let alpha = interpolationStyle.computeAlpha(previous_frame.time, next_frame.time, animationTimeSecs);
    applyInterpolatedSkKeyframesToLocalTransforms(previous_frame, next_frame, alpha, jointIndexOfName, local_transforms);
}

fn skKeyframesSurroundingTime(skKeyframes: &[RenderSkKeyframe], animationTimeSecs: f32) -> (Option<&RenderSkKeyframe>, Option<&RenderSkKeyframe>) {
    let sample_time_secs = animationTimeSecs.max(0.0);
    let mut previous_frame: Option<&RenderSkKeyframe> = None;
    let mut next_frame: Option<&RenderSkKeyframe> = None;
    for frame in skKeyframes {
        if frame.time <= sample_time_secs {
            let should_replace = match previous_frame {
                Some(current_frame) => frame.time >= current_frame.time,
                None => true,
            };
            if should_replace {
                previous_frame = Some(frame);
            }
            continue;
        }

        let should_replace = match next_frame {
            Some(current_frame) => frame.time < current_frame.time,
            None => true,
        };
        if should_replace {
            next_frame = Some(frame);
        }
    }
    (previous_frame, next_frame)
}

fn applyLocation3DToLocalTransforms(
    jointName: &str,
    pose: &Location3D,
    jointIndexOfName: &HashMap<String, usize>,
    local_transforms: &mut [(Vec3, Quat, Vec3)],
) {
    let Some(&joint_index) = jointIndexOfName.get(jointName) else {
        return;
    };
    if joint_index >= local_transforms.len() {
        return;
    }

    let (position, quaternion, _scale) = &mut local_transforms[joint_index];
    *position = pose.position;
    *quaternion = pose.quaternion.normalize();
}

fn applyInterpolatedSkKeyframesToLocalTransforms(
    previousFrame: &RenderSkKeyframe,
    nextFrame: &RenderSkKeyframe,
    alpha: f32,
    jointIndexOfName: &HashMap<String, usize>,
    local_transforms: &mut [(Vec3, Quat, Vec3)],
) {
    let mut nextPoseOfJointName = HashMap::new();
    for next_sk_keyframe_pose in &nextFrame.skKeyframePoses {
        nextPoseOfJointName.insert(next_sk_keyframe_pose.jointName.as_str(), next_sk_keyframe_pose);
    }

    for previous_sk_keyframe_pose in &previousFrame.skKeyframePoses {
        let next_sk_keyframe_pose = nextPoseOfJointName.get(previous_sk_keyframe_pose.jointName.as_str()).copied();
        let Some(next_sk_keyframe_pose) = next_sk_keyframe_pose else {
            applyLocation3DToLocalTransforms(
                &previous_sk_keyframe_pose.jointName,
                &previous_sk_keyframe_pose.localPose,
                jointIndexOfName,
                local_transforms,
            );
            continue;
        };
        let pose = Location3D {
            position: previous_sk_keyframe_pose
                .localPose
                .position
                .lerp(next_sk_keyframe_pose.localPose.position, alpha),
            quaternion: previous_sk_keyframe_pose
                .localPose
                .quaternion
                .slerp(next_sk_keyframe_pose.localPose.quaternion, alpha)
                .normalize(),
        };
        applyLocation3DToLocalTransforms(&previous_sk_keyframe_pose.jointName, &pose, jointIndexOfName, local_transforms);
    }
}

fn computeSkeletonPoseAtDefaultFallbackPose(joints: &[RenderSkJoint]) -> SkeletonPose {
    let mut local_transforms = Vec::with_capacity(joints.len());
    for joint in joints {
        local_transforms.push(computeDefaultLocalTransform(joint));
    }
    computeSkeletonPoseFromLocalTransforms(joints, &local_transforms)
}

fn computeSkeletonPoseFromLocalTransforms(joints: &[RenderSkJoint], local_transforms: &[(Vec3, Quat, Vec3)]) -> SkeletonPose {
    let num_joints = joints.len();
    let mut depths = vec![0usize; num_joints];
    for i in 0..num_joints {
        let mut depth = 0usize;
        let mut current = Some(i);
        while let Some(current_index) = current {
            let Some(parent_joint_index) = joints[current_index].parent_joint_index else {
                break;
            };
            depth += 1;
            current = Some(parent_joint_index);
        }
        depths[i] = depth;
    }

    let mut order: Vec<usize> = (0..num_joints).collect();
    order.sort_by_key(|&i| depths[i]);

    let mut jointMatrixOfJointIndex = vec![Mat4::IDENTITY; num_joints];
    for &i in &order {
        let (t, r, s) = local_transforms[i];
        let local_mat = Mat4::from_scale_rotation_translation(s, r, t);
        let parent_joint_index = joints[i].parent_joint_index;
        jointMatrixOfJointIndex[i] = match parent_joint_index {
            Some(parent_joint_index) if parent_joint_index < num_joints => jointMatrixOfJointIndex[parent_joint_index] * local_mat,
            _ => local_mat,
        };
    }

    SkeletonPose { jointMatrixOfJointIndex }
}

pub fn computeSkeletonPoseFromMultiSamples(joints: &[RenderSkJoint], animationSamples: &[SkeletonLocalAnimationSample]) -> SkeletonPose {
    if animationSamples.is_empty() {
        return computeSkeletonPoseAtDefaultFallbackPose(joints);
    }

    let joint_count = joints.len();
    // Each joint blends by itself, among only the animations that key it. So a high-priority animation of just an arm leaves the
    // rest of the body to the lower priorities.
    let blend_weights_of_joint: Vec<AnimationBlendWeights> = (0..joint_count)
        .map(|joint_idx| {
            let blend_samples: Vec<AnimationBlendSample> = animationSamples
                .iter()
                .map(|sample| AnimationBlendSample {
                    priority: sample.priority,
                    computed_weight: sample.computed_weight,
                    is_valid: sample.local_transforms.len() == joint_count && sample.local_transforms[joint_idx].is_some(),
                })
                .collect();
            computeBlendedWeights(&blend_samples)
        })
        .collect();
    let mut translationSumOfJoint: Vec<Vec3> = vec![Vec3::ZERO; joint_count];
    let mut scaleSumOfJoint: Vec<Vec3> = vec![Vec3::ZERO; joint_count];
    // Rotations blend as turns away from the joint's rest pose, each an axis times an angle under half a circle. So a joint moving
    // between two poses never goes around the far side: an arm going from behind the body to over the head swings forward.
    let mut rotationOffsetSumOfJoint: Vec<Vec3> = vec![Vec3::ZERO; joint_count];

    for (sample_idx, sample) in animationSamples.iter().enumerate() {
        for (joint_idx, (local_transform, joint)) in sample.local_transforms.iter().zip(joints).enumerate() {
            let weight = blend_weights_of_joint[joint_idx].sample_weights[sample_idx];
            let Some((translation, rotation, scale)) = local_transform else {
                continue;
            };
            if weight <= 0.0 {
                continue;
            }
            translationSumOfJoint[joint_idx] += *translation * weight;
            scaleSumOfJoint[joint_idx] += *scale * weight;

            let (_base_translation, base_rotation, _base_scale) = computeDefaultLocalTransform(joint);
            let rotation_offset = base_rotation.inverse() * *rotation;
            let rotation_offset = if rotation_offset.w < 0.0 { -rotation_offset } else { rotation_offset };
            rotationOffsetSumOfJoint[joint_idx] += rotation_offset.to_scaled_axis() * weight;
        }
    }

    let mut blended_local_transforms: Vec<(Vec3, Quat, Vec3)> = Vec::with_capacity(joint_count);
    for (joint_idx, joint) in joints.iter().enumerate() {
        let remainingWeight = blend_weights_of_joint[joint_idx].remaining_weight;
        let totalWeight = blend_weights_of_joint[joint_idx].total_animation_weight + remainingWeight;
        let (base_translation, base_rotation, base_scale) = computeDefaultLocalTransform(joint);
        let mut translation = translationSumOfJoint[joint_idx] + base_translation * remainingWeight;
        let mut scale = scaleSumOfJoint[joint_idx] + base_scale * remainingWeight;
        let rotation = base_rotation * Quat::from_scaled_axis(rotationOffsetSumOfJoint[joint_idx] / totalWeight);

        translation /= totalWeight;
        scale /= totalWeight;
        blended_local_transforms.push((translation, rotation, scale));
    }

    computeSkeletonPoseFromLocalTransforms(joints, &blended_local_transforms)
}
