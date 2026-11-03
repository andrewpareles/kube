use render_core::{
    computeSkeletonPose::{
        SkeletonLocalAnimationSample, SkeletonPose, computeLocalTransformOfJointAtTime, computeSkeletonPoseAtTime, computeSkeletonPoseFromMultiSamples,
    },
    renderer_types::{RenderSkAnimation, RenderSkJoint, RenderSkKeyframe, RenderSkKeyframePose},
};
use std::f32::consts::FRAC_PI_2;

use game_types::object_types::{ClockState, InterpolationStyle, Location3D, ObjectLocation, SkAnimationInfo};
use glam::{Quat, Vec3};

fn make_joint(name: &str, parent_joint_index: Option<usize>, position: Vec3) -> RenderSkJoint {
    RenderSkJoint {
        name: name.to_string(),
        parent_joint_index,
        rest_pose: Location3D {
            position,
            quaternion: Quat::IDENTITY,
        },
    }
}

fn assert_vec3_is_close(actual: Vec3, expected: Vec3) {
    let delta = actual - expected;
    assert!(delta.length() < 0.001, "expected {expected:?}, got {actual:?}, delta {delta:?}");
}

#[test]
fn linear_sk_animation_uses_linear_alpha() {
    let joint_of_index = vec![make_joint("root", None, Vec3::ZERO)];
    let sk_animation_info = SkAnimationInfo {
        duration: 1.0,
        interpolationStyle: InterpolationStyle::Linear,
    };
    let sk_keyframes = vec![
        RenderSkKeyframe {
            time: 0.0,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: "root".to_string(),
                localPose: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
            }],
        },
        RenderSkKeyframe {
            time: 1.0,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: "root".to_string(),
                localPose: Location3D {
                    position: Vec3::new(10.0, 0.0, 0.0),
                    quaternion: Quat::IDENTITY,
                },
            }],
        },
    ];

    let local_transforms = computeLocalTransformOfJointAtTime(&joint_of_index, &sk_animation_info, &sk_keyframes, 0.25);
    let (position, _rotation, _scale) = local_transforms[0].unwrap();
    assert_vec3_is_close(position, Vec3::new(2.5, 0.0, 0.0));
}

#[test]
fn looping_skeleton_animation_wraps_negative_speed() -> anyhow::Result<()> {
    let joint_of_index = vec![make_joint("root", None, Vec3::ZERO)];
    let sk_animation_info = SkAnimationInfo {
        duration: 1.0,
        interpolationStyle: InterpolationStyle::Linear,
    };
    let sk_keyframes = vec![
        RenderSkKeyframe {
            time: 0.0,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: "root".to_string(),
                localPose: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
            }],
        },
        RenderSkKeyframe {
            time: 1.0,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: "root".to_string(),
                localPose: Location3D {
                    position: Vec3::new(10.0, 0.0, 0.0),
                    quaternion: Quat::IDENTITY,
                },
            }],
        },
    ];
    let render_skeleton_animation = RenderSkAnimation {
        skAnimationInfo: sk_animation_info,
        skKeyframes: sk_keyframes,
        clock_state: ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Client,
        },
        should_loop: true,
        speed: -1.0,
        priority: 0,
        weight: 100.0,
        weight_transition: None,
    };

    let pose = computeSkeletonPoseAtTime(&joint_of_index, &[render_skeleton_animation], 250.0, Some(250.0))?;
    let root_position = pose.jointMatrixOfJointIndex[0].transform_point3(Vec3::ZERO);

    assert_vec3_is_close(root_position, Vec3::new(7.5, 0.0, 0.0));
    Ok(())
}

fn compute_pose_for_single_rotation_channel(joint_of_index: &[RenderSkJoint], joint_name: &str) -> SkeletonPose {
    let sk_animation_info = SkAnimationInfo {
        duration: 1.0,
        interpolationStyle: InterpolationStyle::EaseInOut,
    };
    let sk_keyframes = vec![
        RenderSkKeyframe {
            time: 0.0,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: joint_name.to_string(),
                localPose: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
            }],
        },
        RenderSkKeyframe {
            time: 1.0,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: joint_name.to_string(),
                localPose: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::from_rotation_z(FRAC_PI_2),
                },
            }],
        },
    ];
    let local_transforms = computeLocalTransformOfJointAtTime(joint_of_index, &sk_animation_info, &sk_keyframes, 1.0);
    computeSkeletonPoseFromMultiSamples(
        joint_of_index,
        &[SkeletonLocalAnimationSample {
            local_transforms,
            priority: 0,
            computed_weight: 100.0,
        }],
    )
}

#[test]
fn skeleton_animation_rotation_moves_a_wing_tip_to_the_expected_height() {
    let joint_of_index = vec![make_joint("root", None, Vec3::ZERO), make_joint("wing", Some(0), Vec3::X)];

    let pose = compute_pose_for_single_rotation_channel(&joint_of_index, "wing");
    let wing_tip = pose.jointMatrixOfJointIndex[1].transform_point3(Vec3::X);

    assert_vec3_is_close(wing_tip, Vec3::new(1.0, 1.0, 0.0));
}

#[test]
fn skeleton_animation_remaps_imported_joint_order_by_name() {
    let joint_of_index = vec![make_joint("root", None, Vec3::ZERO), make_joint("wing", Some(0), Vec3::X)];

    let pose = compute_pose_for_single_rotation_channel(&joint_of_index, "wing");
    let wing_tip = pose.jointMatrixOfJointIndex[1].transform_point3(Vec3::X);

    assert_vec3_is_close(wing_tip, Vec3::new(1.0, 1.0, 0.0));
}

#[test]
fn default_pose_uses_rest_pose() {
    let joint_of_index = vec![
        make_joint("root", None, Vec3::ZERO),
        RenderSkJoint {
            name: "wing".to_string(),
            parent_joint_index: Some(0),
            rest_pose: Location3D {
                position: Vec3::X,
                quaternion: Quat::from_rotation_z(FRAC_PI_2),
            },
        },
    ];

    let pose = computeSkeletonPoseFromMultiSamples(&joint_of_index, &[]);
    let wing_origin = pose.jointMatrixOfJointIndex[1].transform_point3(Vec3::ZERO);

    assert_vec3_is_close(wing_origin, Vec3::new(1.0, 0.0, 0.0));
}

// A keyframe pose is an offset in the joint's rest frame, so a rotated rest pose turns the offset with it.
#[test]
fn a_keyframe_pose_is_applied_on_top_of_the_joint_rest_pose() {
    let joint_of_index = vec![RenderSkJoint {
        name: "arm".to_string(),
        parent_joint_index: None,
        rest_pose: Location3D {
            position: Vec3::new(0.0, 2.0, 0.0),
            quaternion: Quat::from_rotation_z(FRAC_PI_2),
        },
    }];
    let sk_animation_info = SkAnimationInfo {
        duration: 1.0,
        interpolationStyle: InterpolationStyle::Linear,
    };
    let sk_keyframes = vec![RenderSkKeyframe {
        time: 0.0,
        skKeyframePoses: vec![RenderSkKeyframePose {
            jointName: "arm".to_string(),
            localPose: Location3D {
                position: Vec3::X,
                quaternion: Quat::from_rotation_z(FRAC_PI_2),
            },
        }],
    }];

    let (position, rotation, _scale) = computeLocalTransformOfJointAtTime(&joint_of_index, &sk_animation_info, &sk_keyframes, 0.0)[0].unwrap();

    assert_vec3_is_close(position, Vec3::new(0.0, 3.0, 0.0));
    assert!(rotation.angle_between(Quat::from_rotation_z(std::f32::consts::PI)) < 0.001, "got {rotation:?}");
}

// A walk swings an arm back and a jump raises it over the head. Blending the two swings the arm forward, the way an arm really moves,
// instead of the shorter way back over the top. The joint's rest pose is turned, so the swing has to be measured from it.
#[test]
fn blending_an_arm_from_behind_to_over_the_head_swings_it_forward() {
    let rest_rotation = Quat::from_rotation_y(FRAC_PI_2);
    let joint_of_index = vec![RenderSkJoint {
        name: "arm".to_string(),
        parent_joint_index: None,
        rest_pose: Location3D {
            position: Vec3::ZERO,
            quaternion: rest_rotation,
        },
    }];
    // The arm hangs down -Y and swings forward toward +Z.
    let arm_swung_forward = |degrees: f32| Quat::from_rotation_x(-degrees.to_radians());
    let sample = |forward_degrees: f32, weight: f32| SkeletonLocalAnimationSample {
        local_transforms: vec![Some((Vec3::ZERO, rest_rotation * arm_swung_forward(forward_degrees), Vec3::ONE))],
        priority: 0,
        computed_weight: weight,
    };

    for jump_weight in [0.0, 25.0, 50.0, 75.0, 100.0] {
        let pose = computeSkeletonPoseFromMultiSamples(&joint_of_index, &[sample(-33.0, 100.0 - jump_weight), sample(160.0, jump_weight)]);
        let hand = pose.jointMatrixOfJointIndex[0].transform_point3(Vec3::NEG_Y);
        let forward_degrees = -33.0 + 193.0 * jump_weight / 100.0;
        assert_vec3_is_close(hand, rest_rotation * arm_swung_forward(forward_degrees) * Vec3::NEG_Y);
    }
}

// A character holding a lantern: the lantern animation keys only the arm, at a higher priority than the walk. The arm holds the
// lantern while the rest of the body keeps walking, instead of the whole body standing at rest.
#[test]
fn an_animation_of_just_the_arm_leaves_the_other_joints_to_lower_priorities() -> anyhow::Result<()> {
    let joint_of_index = vec![make_joint("torso", None, Vec3::ZERO), make_joint("arm", Some(0), Vec3::X)];
    let held_pose = |priority: u8, poses: Vec<(&str, Location3D)>| RenderSkAnimation {
        skAnimationInfo: SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        },
        skKeyframes: vec![RenderSkKeyframe {
            time: 0.0,
            skKeyframePoses: poses
                .into_iter()
                .map(|(joint_name, local_pose)| RenderSkKeyframePose {
                    jointName: joint_name.to_string(),
                    localPose: local_pose,
                })
                .collect(),
        }],
        clock_state: ClockState::Paused { animationTimeSecs: 0.0 },
        should_loop: true,
        speed: 1.0,
        priority,
        weight: 100.0,
        weight_transition: None,
    };
    let walk = held_pose(
        6,
        vec![
            (
                "torso",
                Location3D {
                    position: Vec3::new(0.0, 0.5, 0.0),
                    quaternion: Quat::IDENTITY,
                },
            ),
            (
                "arm",
                Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::from_rotation_z(FRAC_PI_2),
                },
            ),
        ],
    );
    let lantern = held_pose(
        9,
        vec![(
            "arm",
            Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::from_rotation_z(-FRAC_PI_2),
            },
        )],
    );

    let pose = computeSkeletonPoseAtTime(&joint_of_index, &[walk, lantern], 0.0, Some(0.0))?;
    let torso_origin = pose.jointMatrixOfJointIndex[0].transform_point3(Vec3::ZERO);
    let arm_tip = pose.jointMatrixOfJointIndex[1].transform_point3(Vec3::X);

    assert_vec3_is_close(torso_origin, Vec3::new(0.0, 0.5, 0.0));
    // The arm starts at the raised torso's (1, 0.5, 0), and the lantern turns it to point down.
    assert_vec3_is_close(arm_tip, Vec3::new(1.0, -0.5, 0.0));
    Ok(())
}
