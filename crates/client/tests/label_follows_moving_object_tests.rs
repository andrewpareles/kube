use render_core::{
    computePropertyValue::applyPropertyOverridesToRenderObjectOfId,
    computeSkeletonPose::{computeFollowedSkJointMatrixGivenSkJointToFollow, computeSkeletonPoseOfIdAtTime},
    render_scene::{FontAtlas, LabelGeometry, computeRenderObjectTransform},
    renderer_types::{
        RenderBrick, RenderLabel, RenderLabelContent, RenderObject, RenderSkJoint, RenderSkeleton, RenderSkeletonInfo, RenderTransform, SkJointToFollow,
    },
};
// A label copies the location of what it follows, so it must be smoothed between snapshots on identical
// terms. When it was not, a label sitting just off a surface sank into the moving object and flickered.

use client::play_runner::play_renderer::interpolate_render_object_of_id;
use game_types::object_types::{BillboardMode, Location3D, Movement3D, ObjectId, PhysicsInfo3D, VisualSettings};
use game_types::physics_primitive_types::BrickPrimitiveSettings;
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

const BRICK_OBJECT_ID: ObjectId = 10;
const LABEL_OBJECT_ID: ObjectId = 11;
const GROUP_OBJECT_ID: ObjectId = 12;
const SKELETON_OBJECT_ID: ObjectId = 13;

// -------------------------- Render fixtures --------------------------

fn physics_info_at(y: f32, velocity_y: f32) -> PhysicsInfo3D {
    PhysicsInfo3D {
        location: Location3D {
            position: Vec3::new(0.0, y, 0.0),
            quaternion: Quat::IDENTITY,
        },
        movement: Movement3D {
            velocity: Vec3::new(0.0, velocity_y, 0.0),
            angularVelocity: Vec3::ZERO,
        },
    }
}

fn render_object_of_id_at(y: f32, velocity_y: f32) -> HashMap<ObjectId, RenderObject> {
    let physics_info = physics_info_at(y, velocity_y);
    let mut render_object_of_id = HashMap::new();
    render_object_of_id.insert(
        BRICK_OBJECT_ID,
        RenderObject::Brick(RenderBrick {
            transform: RenderTransform {
                teleport_count: 0,
                physicsInfo3D: physics_info,
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: None,
            primitive: BrickPrimitiveSettings { size: Vec3::splat(2.0) },
        }),
    );
    render_object_of_id.insert(
        LABEL_OBJECT_ID,
        RenderObject::RenderLabel(RenderLabel {
            physicalObjectId: BRICK_OBJECT_ID,
            offsetFromObject: Vec3::new(0.0, 1.02, 0.0),
            rotationFromObject: Quat::IDENTITY,
            billboardMode: BillboardMode::Full,
            isConstantSize: false,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            content: RenderLabelContent::Text {
                scale: 0.01,
                backgroundColor: None,
                backgroundOpacity: 1.0,
                geometry: Arc::new(LabelGeometry {
                    cache_key: 1,
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    background_index_count: 0,
                }),
                atlas: Arc::new(FontAtlas {
                    version: 0,
                    width: 1,
                    height: 1,
                    mip_levels: vec![vec![0]],
                }),
            },
        }),
    );
    render_object_of_id
}

fn render_character_object_of_id_at(y: f32, velocity_y: f32) -> HashMap<ObjectId, RenderObject> {
    let mut render_object_of_id = render_object_of_id_at(y + 2.0, velocity_y);
    let group_physics_info = physics_info_at(y, velocity_y);
    let Some(RenderObject::Brick(head)) = render_object_of_id.get_mut(&BRICK_OBJECT_ID) else {
        panic!("expected the head");
    };
    head.transform.sk_joint_to_follow_arc = Some(Arc::new(SkJointToFollow {
        skeleton_id: SKELETON_OBJECT_ID,
        joint_name: "Head".to_string(),
        offset_from_joint: Location3D {
            position: Vec3::ZERO,
            quaternion: Quat::IDENTITY,
        },
        follower_group_id: GROUP_OBJECT_ID,
    }));
    render_object_of_id.insert(
        GROUP_OBJECT_ID,
        RenderObject::RigidGroup(RenderTransform {
            teleport_count: 0,
            physicsInfo3D: group_physics_info,
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        }),
    );
    render_object_of_id.insert(
        SKELETON_OBJECT_ID,
        RenderObject::Skeleton(RenderSkeleton {
            renderSkeletonInfoArc: Arc::new(RenderSkeletonInfo {
                jointOfIndex: vec![RenderSkJoint {
                    name: "Head".to_string(),
                    parent_joint_index: None,
                    rest_pose: Location3D {
                        position: Vec3::new(0.0, 2.0, 0.0),
                        quaternion: Quat::IDENTITY,
                    },
                }],
                jointIndexOfName: HashMap::from([("Head".to_string(), 0)]),
                jointIndexOfObjectId: HashMap::new(),
            }),
            render_sk_animations: Arc::new(Vec::new()),
        }),
    );
    render_object_of_id
}

fn position_of(render_object_of_id: &HashMap<ObjectId, RenderObject>, object_id: ObjectId) -> Vec3 {
    let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(render_object_of_id, 0.0, None);
    computeRenderObjectTransform(render_object_of_id, &skeleton_pose_of_id, object_id)
        .expect("object should have a render transform")
        .w_axis
        .truncate()
}

// -------------------------- Physical objects --------------------------

#[test]
fn a_label_stays_locked_to_the_moving_object_it_follows() {
    let prev_render_object_of_id = render_object_of_id_at(10.0, -1.0);
    let next_render_object_of_id = render_object_of_id_at(9.0, -1.0);

    let interpolated_render_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 133.0);

    let brick_position = position_of(&interpolated_render_object_of_id, BRICK_OBJECT_ID);
    let label_position = position_of(&interpolated_render_object_of_id, LABEL_OBJECT_ID);
    assert_eq!(
        brick_position, label_position,
        "a label must land on the same interpolated location as the object it follows",
    );

    // The smoothing must actually be doing something, or this passes with both on the raw snapshot.
    let next_position = position_of(&next_render_object_of_id, BRICK_OBJECT_ID);
    assert_ne!(brick_position, next_position, "the sample should be interpolated, not the raw snapshot");
}

// Flicker is about a whole fall, not one instant: this walks a block down through many snapshots and
// samples every interpolation window, asserting the label never drifts off the surface it sits on.
#[test]
fn a_label_never_drifts_off_a_falling_block() {
    const GRAVITY: f32 = -9.8;
    const SNAPSHOT_SPAN_MS: f32 = 50.0;
    let snapshot_span_secs = SNAPSHOT_SPAN_MS / 1000.0;

    let mut worst_drift = 0.0f32;
    let mut sample_count = 0;
    let mut fall_time_secs = 0.0f32;

    for _snapshot_index in 0..40 {
        let prev_y = 20.0 + 0.5 * GRAVITY * fall_time_secs * fall_time_secs;
        let prev_velocity_y = GRAVITY * fall_time_secs;
        let next_time_secs = fall_time_secs + snapshot_span_secs;
        let next_y = 20.0 + 0.5 * GRAVITY * next_time_secs * next_time_secs;
        let next_velocity_y = GRAVITY * next_time_secs;

        let prev_render_object_of_id = render_object_of_id_at(prev_y, prev_velocity_y);
        let next_render_object_of_id = render_object_of_id_at(next_y, next_velocity_y);

        for step in 0..=10 {
            let redraw_ms = SNAPSHOT_SPAN_MS * (step as f32 / 10.0);
            let interpolated_render_object_of_id = interpolate_render_object_of_id(
                &prev_render_object_of_id,
                &next_render_object_of_id,
                0.0,
                SNAPSHOT_SPAN_MS as f64,
                (SNAPSHOT_SPAN_MS + redraw_ms) as f64,
            );
            let brick_position = position_of(&interpolated_render_object_of_id, BRICK_OBJECT_ID);
            let label_position = position_of(&interpolated_render_object_of_id, LABEL_OBJECT_ID);
            worst_drift = worst_drift.max(brick_position.distance(label_position));
            sample_count += 1;
        }
        fall_time_secs = next_time_secs;
    }

    assert!(sample_count > 400, "expected the fall to be sampled thoroughly, got {sample_count}");
    assert!(
        worst_drift < 1e-5,
        "a label drifted {worst_drift} blocks from the falling block it is stuck to; anything above zero sinks it into the surface",
    );
}

#[test]
fn a_label_is_left_alone_when_its_object_was_moved_by_force() {
    let prev_render_object_of_id = render_object_of_id_at(10.0, -1.0);
    let mut next_render_object_of_id = render_object_of_id_at(9.0, -1.0);
    next_render_object_of_id
        .get_mut(&BRICK_OBJECT_ID)
        .unwrap()
        .getTransformMut()
        .unwrap()
        .teleport_count = 1;

    let interpolated_render_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 133.0);

    assert_eq!(
        position_of(&interpolated_render_object_of_id, LABEL_OBJECT_ID),
        position_of(&next_render_object_of_id, LABEL_OBJECT_ID),
        "an object that jumped should be shown where it jumped to, not smoothed toward it",
    );
}

// -------------------------- Animated characters --------------------------

// Animated heads and their nametags must follow the same falling body between simulation ticks.
#[test]
fn a_nametag_stays_locked_to_an_animated_head_during_a_fast_fall() {
    let span_ms = 1000.0 / 60.0;
    // The expected trajectory is calculated separately from render interpolation.
    for snapshot_index in 0..40 {
        let start_time_secs = snapshot_index as f32 * span_ms / 1000.0;
        let end_time_secs = start_time_secs + span_ms / 1000.0;
        let fall_y = |t: f32| 100.0 - 100.0 * t - 25.0 * t * t;
        let fall_velocity = |t: f32| -100.0 - 50.0 * t;
        let prev_render_object_of_id = render_character_object_of_id_at(fall_y(start_time_secs), fall_velocity(start_time_secs));
        let next_render_object_of_id = render_character_object_of_id_at(fall_y(end_time_secs), fall_velocity(end_time_secs));
        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let rendered_object_of_id = interpolate_render_object_of_id(
                &prev_render_object_of_id,
                &next_render_object_of_id,
                0.0,
                span_ms as f64,
                (span_ms * (1.0 + alpha)) as f64,
            );
            let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(&rendered_object_of_id, (span_ms * alpha) as f64, None);
            let Some(RenderObject::Brick(head)) = rendered_object_of_id.get(&BRICK_OBJECT_ID) else {
                panic!("expected the head");
            };
            let head_transform = computeFollowedSkJointMatrixGivenSkJointToFollow(
                &rendered_object_of_id,
                &skeleton_pose_of_id,
                head.transform.sk_joint_to_follow_arc.as_deref().unwrap(),
            )
            .unwrap();
            let head_position = head_transform.w_axis.truncate();
            let label_position = position_of(&rendered_object_of_id, LABEL_OBJECT_ID);
            assert!(
                head_position.distance(label_position) < 1e-5,
                "nametag drift at alpha {alpha}: head={head_position:?}, label={label_position:?}"
            );
            let expected_head_y = fall_y(start_time_secs + span_ms / 1000.0 * alpha) + 2.0;
            assert!(
                (head_position.y - expected_head_y).abs() < 3e-5,
                "the head must keep the body's falling velocity: expected {expected_head_y}, got {}",
                head_position.y
            );
        }
    }
}

#[test]
fn a_nametag_stays_on_the_animated_head_when_the_group_teleports() {
    let prev_render_object_of_id = render_character_object_of_id_at(100.0, -100.0);
    let mut next_render_object_of_id = render_character_object_of_id_at(20.0, 0.0);
    next_render_object_of_id
        .get_mut(&GROUP_OBJECT_ID)
        .unwrap()
        .getTransformMut()
        .unwrap()
        .teleport_count = 1;
    for redraw_ms in [100.0, 125.0, 150.0, 175.0, 200.0] {
        let rendered_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, redraw_ms);
        assert_eq!(position_of(&rendered_object_of_id, BRICK_OBJECT_ID), Vec3::new(0.0, 22.0, 0.0));
        assert_eq!(position_of(&rendered_object_of_id, LABEL_OBJECT_ID), Vec3::new(0.0, 22.0, 0.0));
    }
}

#[test]
fn a_label_inherits_the_rendered_property_animation_transform() {
    let mut render_object_of_id = render_object_of_id_at(10.0, 0.0);
    let property_value_of_path_of_object_id = HashMap::from([(
        BRICK_OBJECT_ID,
        HashMap::from([
            (vec!["location".to_string(), "position".to_string(), "y".to_string()], 25.0),
            (vec!["location".to_string(), "rotation".to_string(), "z".to_string()], 90.0),
        ]),
    )]);
    applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &property_value_of_path_of_object_id, &HashMap::new());
    let label_transform = computeRenderObjectTransform(&render_object_of_id, &HashMap::new(), LABEL_OBJECT_ID).unwrap();
    let expected_transform = Mat4::from_rotation_translation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(0.0, 25.0, 0.0));
    assert!(label_transform.abs_diff_eq(expected_transform, 1e-5));
}

#[test]
fn a_label_switches_targets_without_interpolating_between_them() {
    let prev_render_object_of_id = render_object_of_id_at(10.0, 0.0);
    let mut next_render_object_of_id = render_object_of_id_at(10.0, 0.0);
    next_render_object_of_id.insert(
        GROUP_OBJECT_ID,
        RenderObject::RigidGroup(RenderTransform {
            teleport_count: 0,
            physicsInfo3D: physics_info_at(50.0, 0.0),
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        }),
    );
    let Some(RenderObject::RenderLabel(label)) = next_render_object_of_id.get_mut(&LABEL_OBJECT_ID) else {
        panic!("expected a label");
    };
    label.physicalObjectId = GROUP_OBJECT_ID;
    let rendered_object_of_id = interpolate_render_object_of_id(&prev_render_object_of_id, &next_render_object_of_id, 0.0, 100.0, 125.0);
    assert_eq!(position_of(&rendered_object_of_id, LABEL_OBJECT_ID), Vec3::new(0.0, 50.0, 0.0));
}
