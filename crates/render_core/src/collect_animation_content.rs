// Resolve animation object graphs identically for live worlds and saved asset previews.
// Live worlds supply their indexed animator ids and cached keyframes; stored scenes collect them once.
use crate::{
    computePropertyValue::{ComputedPropertyAnimation, ComputedPropertyOverrides},
    renderer_types::{RenderSkAnimation, RenderSkKeyframe, RenderSkKeyframePose},
};
use game_types::{
    consts::ANIMATOR_WEIGHT_MAX,
    object_types::{LiveObject, ObjectId, ObjectInfo, PropertyKeyframeInfo, TweenInfo},
};
use glam::Vec3;
use std::{collections::HashMap, sync::Arc};

pub fn collect_render_sk_animations(
    object_of_id: &HashMap<ObjectId, LiveObject>,
    skeletonId: ObjectId,
    animator_ids: impl IntoIterator<Item = ObjectId>,
    keyframes_of_animation_id: &HashMap<ObjectId, Vec<RenderSkKeyframe>>,
) -> Vec<RenderSkAnimation> {
    let mut render_sk_animations = Vec::new();
    for object_id in animator_ids {
        // make sure the object pointing is an animation
        let Some(animation_obj) = object_of_id.get(&object_id) else {
            continue;
        };
        let ObjectInfo::SkAnimator(animation) = &animation_obj.info else {
            continue;
        };

        // Ensure this animation still points at the Skeleton we are collecting for.
        let Some(animation_skeleton_id) = animation.skeletonId.as_ref() else {
            continue;
        };
        let animation_skeleton_id: ObjectId = (*animation_skeleton_id).into();
        if animation_skeleton_id != skeletonId {
            continue;
        }

        // get the clock of the animation
        let Some(clockId) = animation.clockId.as_ref() else {
            continue;
        };
        let clockId: ObjectId = (*clockId).into();
        let Some(clockObj) = object_of_id.get(&clockId) else {
            continue;
        };
        let ObjectInfo::Clock(clock) = &clockObj.info else {
            continue;
        };

        let Some(skAnimationId) = animation.skAnimationId.as_ref() else {
            continue;
        };
        let skAnimationId: ObjectId = (*skAnimationId).into();
        let Some(sk_animation_object) = object_of_id.get(&skAnimationId) else {
            continue;
        };
        let ObjectInfo::SkAnimation(sk_animation_info) = &sk_animation_object.info else {
            continue;
        };
        let Some(sk_keyframes) = keyframes_of_animation_id.get(&skAnimationId) else {
            log::error!("Render animation frames cache missing for SkAnimation {}", skAnimationId);
            continue;
        };

        render_sk_animations.push(RenderSkAnimation {
            skAnimationInfo: sk_animation_info.clone(),
            skKeyframes: sk_keyframes.clone(),
            clock_state: clock.clockState,
            should_loop: clock.shouldLoop,
            speed: clock.speed,
            priority: animation.priority,
            weight: animation.weight,
            weight_transition: animation.weightTransition,
        });
    }

    render_sk_animations
}

pub fn collect_property_overrides(
    object_of_id: &HashMap<ObjectId, LiveObject>,
    property_animator_ids: impl IntoIterator<Item = ObjectId>,
    keyframes_of_animation_id: &HashMap<ObjectId, Arc<Vec<PropertyKeyframeInfo>>>,
    tween_ids: impl IntoIterator<Item = ObjectId>,
) -> HashMap<ObjectId, ComputedPropertyOverrides> {
    let mut computedPropertyOverridesOfObjectId = HashMap::new();
    for property_animator_id in property_animator_ids {
        let Some(property_animator_object) = object_of_id.get(&property_animator_id) else {
            continue;
        };
        let ObjectInfo::PropertyAnimator(property_animator) = &property_animator_object.info else {
            continue;
        };
        let Some(target_object_id) = property_animator.targetObjectId.map(ObjectId::from) else {
            continue;
        };
        if property_animator.propertyPath.is_empty() {
            continue;
        }
        let Some(clock_id) = property_animator.clockId.map(ObjectId::from) else {
            continue;
        };
        let Some(clock_object) = object_of_id.get(&clock_id) else {
            continue;
        };
        let ObjectInfo::Clock(clock) = &clock_object.info else {
            continue;
        };
        let Some(property_animation_id) = property_animator.propertyAnimationId.map(ObjectId::from) else {
            continue;
        };
        let Some(property_animation_object) = object_of_id.get(&property_animation_id) else {
            continue;
        };
        let ObjectInfo::PropertyAnimation(property_animation) = &property_animation_object.info else {
            continue;
        };
        let Some(property_keyframes_arc) = keyframes_of_animation_id.get(&property_animation_id) else {
            continue;
        };
        let computed_property_animation = ComputedPropertyAnimation {
            property_keyframes_arc: Arc::clone(property_keyframes_arc),
            duration: property_animation.duration,
            interpolationStyle: property_animation.interpolationStyle,
            clock_state: clock.clockState,
            should_loop: clock.shouldLoop,
            speed: clock.speed,
            priority: property_animator.priority,
            weight: property_animator.weight,
            weight_transition: property_animator.weightTransition,
        };
        push_computed_property_animation(
            &mut computedPropertyOverridesOfObjectId,
            target_object_id,
            property_animator.propertyPath.clone(),
            computed_property_animation,
        );
    }
    for tween_id in tween_ids {
        let Some(ObjectInfo::Tween(tween)) = object_of_id.get(&tween_id).map(|tween_object| &tween_object.info) else {
            continue;
        };
        let Some(target_object_id) = tween.targetObjectId.map(ObjectId::from) else {
            continue;
        };
        if tween.propertyPath.is_empty() {
            continue;
        }
        push_computed_property_animation(
            &mut computedPropertyOverridesOfObjectId,
            target_object_id,
            tween.propertyPath.clone(),
            computed_property_animation_of_tween(tween),
        );
    }
    computedPropertyOverridesOfObjectId
}

// A tween plays as a PropertyAnimator with two keyframes. Its priority is above every PropertyAnimator's and its weight
// is full, so it wins on its property while it plays.
pub fn computed_property_animation_of_tween(tween: &TweenInfo) -> ComputedPropertyAnimation {
    ComputedPropertyAnimation {
        property_keyframes_arc: Arc::new(vec![
            PropertyKeyframeInfo {
                time: 0.0,
                value: tween.fromValue,
            },
            PropertyKeyframeInfo {
                time: tween.duration,
                value: tween.toValue,
            },
        ]),
        duration: tween.duration,
        interpolationStyle: tween.interpolationStyle,
        clock_state: tween.clockState,
        should_loop: false,
        speed: 1.0,
        priority: u8::MAX,
        weight: ANIMATOR_WEIGHT_MAX,
        weight_transition: None,
    }
}

// Animations on the same property are blended together, so they are grouped by the property's path.
fn push_computed_property_animation(
    computedPropertyOverridesOfObjectId: &mut HashMap<ObjectId, ComputedPropertyOverrides>,
    object_id: ObjectId,
    property_path: Vec<String>,
    computed_property_animation: ComputedPropertyAnimation,
) {
    let computed_property_overrides = computedPropertyOverridesOfObjectId
        .entry(object_id)
        .or_insert_with(|| ComputedPropertyOverrides {
            computed_property_animations: Vec::new(),
        });
    let computed_property_animation_index = computed_property_overrides
        .computed_property_animations
        .iter()
        .position(|(next_property_path, _)| next_property_path == &property_path);
    match computed_property_animation_index {
        Some(computed_property_animation_index) => {
            computed_property_overrides.computed_property_animations[computed_property_animation_index]
                .1
                .push(computed_property_animation);
        }
        None => computed_property_overrides
            .computed_property_animations
            .push((property_path, vec![computed_property_animation])),
    }
}

// Each object turns and scales around at most one Pivot: the lowest-id one pointing at it.
pub fn collect_pivot_offsets(object_of_id: &HashMap<ObjectId, LiveObject>, pivot_ids: impl IntoIterator<Item = ObjectId>) -> HashMap<ObjectId, Vec3> {
    let mut pivot_of_object_id: HashMap<ObjectId, (ObjectId, Vec3)> = HashMap::new();
    for pivot_id in pivot_ids {
        let Some(pivot_object) = object_of_id.get(&pivot_id) else {
            continue;
        };
        let ObjectInfo::Pivot(pivot) = &pivot_object.info else {
            continue;
        };
        let Some(object_id) = pivot.physicalObjectId.map(ObjectId::from) else {
            continue;
        };
        let (lowest_pivot_id, offset_from_object) = pivot_of_object_id.entry(object_id).or_insert((pivot_id, pivot.offsetFromObject));
        if pivot_id < *lowest_pivot_id {
            *lowest_pivot_id = pivot_id;
            *offset_from_object = pivot.offsetFromObject;
        }
    }
    pivot_of_object_id
        .into_iter()
        .map(|(object_id, (_, offset_from_object))| (object_id, offset_from_object))
        .collect()
}

pub fn collect_sk_keyframes(
    object_of_id: &HashMap<ObjectId, LiveObject>,
    children: &impl Fn(ObjectId) -> Vec<ObjectId>,
    skAnimationId: ObjectId,
) -> Vec<RenderSkKeyframe> {
    let mut sk_keyframes = Vec::new();
    let keyframe_ids = children(skAnimationId);
    for keyframe_id in keyframe_ids {
        let Some(keyframe_object) = object_of_id.get(&keyframe_id) else {
            continue;
        };
        let ObjectInfo::SkKeyframe(sk_keyframe_info) = &keyframe_object.info else {
            continue;
        };

        let mut skKeyframePoses = Vec::new();
        let sk_keyframe_pose_ids = children(keyframe_id);
        for sk_keyframe_pose_id in sk_keyframe_pose_ids {
            let Some(sk_keyframe_pose_object) = object_of_id.get(&sk_keyframe_pose_id) else {
                continue;
            };
            let ObjectInfo::SkKeyframePose(sk_keyframe_pose_info) = &sk_keyframe_pose_object.info else {
                continue;
            };
            skKeyframePoses.push(RenderSkKeyframePose {
                jointName: sk_keyframe_pose_info.jointName.clone(),
                localPose: sk_keyframe_pose_info.localPose.clone(),
            });
        }
        sk_keyframes.push(RenderSkKeyframe {
            time: sk_keyframe_info.time,
            skKeyframePoses,
        });
    }
    sk_keyframes
}

pub fn collect_property_keyframes(
    object_of_id: &HashMap<ObjectId, LiveObject>,
    children: &impl Fn(ObjectId) -> Vec<ObjectId>,
    propertyAnimationId: ObjectId,
) -> Vec<PropertyKeyframeInfo> {
    let mut property_keyframes = Vec::new();
    let keyframe_ids = children(propertyAnimationId);
    for keyframe_id in keyframe_ids {
        let Some(keyframe_object) = object_of_id.get(&keyframe_id) else {
            continue;
        };
        let ObjectInfo::PropertyKeyframe(property_keyframe_info) = &keyframe_object.info else {
            continue;
        };
        property_keyframes.push(property_keyframe_info.clone());
    }
    property_keyframes
}
