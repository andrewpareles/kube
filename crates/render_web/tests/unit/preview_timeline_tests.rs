use super::StoredWorldRenderable;
use game_types::{
    object_types::{
        ClockInfo, ClockState, InterpolationStyle, LiveObject, ObjectInfo, ObjectLocation, PropertyAnimatorInfo, WeightTransition, sk_animator::SkAnimatorInfo,
    },
    stored_world_type::StoredWorld,
};

fn stored_world() -> anyhow::Result<StoredWorld> {
    let mut item: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/asset-preview-object.json"))?;
    serde_json::from_value(item["Object"].take()).map_err(Into::into)
}

#[test]
fn preview_time_rebases_both_domains_preserving_offsets_and_paused_phase() -> anyhow::Result<()> {
    let mut stored = stored_world()?;
    let transition = WeightTransition {
        startedAtMs: 50_250.0,
        timeLocation: ObjectLocation::Server,
        initValue: 25.0,
        finalValue: 75.0,
        durationMs: 500.0,
        easing: InterpolationStyle::EaseIn,
    };
    for (id, info) in [
        (
            200,
            ObjectInfo::Clock(ClockInfo {
                clockState: ClockState::Playing {
                    localStartMs: 50_000.0,
                    timeLocation: ObjectLocation::Server,
                },
                speed: 1.0,
                shouldLoop: true,
            }),
        ),
        (
            201,
            ObjectInfo::Clock(ClockInfo {
                clockState: ClockState::Playing {
                    localStartMs: 90_000.0,
                    timeLocation: ObjectLocation::Client,
                },
                speed: 1.0,
                shouldLoop: true,
            }),
        ),
        (
            202,
            ObjectInfo::SkAnimator(SkAnimatorInfo {
                skeletonId: None,
                skAnimationId: None,
                clockId: Some(200.into()),
                priority: 2,
                weight: 75.0,
                weightTransition: Some(transition),
            }),
        ),
        (
            203,
            ObjectInfo::PropertyAnimator(PropertyAnimatorInfo {
                targetObjectId: None,
                propertyPath: vec!["opacity".into()],
                propertyAnimationId: None,
                clockId: Some(201.into()),
                priority: 3,
                weight: 75.0,
                weightTransition: Some(WeightTransition {
                    startedAtMs: 90_500.0,
                    timeLocation: ObjectLocation::Client,
                    ..transition
                }),
            }),
        ),
        (
            204,
            ObjectInfo::Clock(ClockInfo {
                clockState: ClockState::Paused { animationTimeSecs: 0.75 },
                speed: 2.0,
                shouldLoop: false,
            }),
        ),
    ] {
        stored.objectOfId.insert(
            id,
            LiveObject {
                id,
                name: format!("timeline-{id}"),
                info,
            },
        );
        stored.childrenOfId.entry(stored.rootObjectId).or_default().push(id);
    }
    StoredWorldRenderable::restart_preview_timeline(&mut stored)?;
    for object in stored.objectOfId.values() {
        match &object.info {
            ObjectInfo::Clock(clock) => match clock.clockState {
                ClockState::Playing { localStartMs, .. } => assert_eq!(localStartMs, 0.0),
                ClockState::Paused { animationTimeSecs } => assert_eq!(animationTimeSecs, 0.75),
            },
            ObjectInfo::SkAnimator(animator) => assert_eq!(animator.weightTransition.unwrap().startedAtMs, 250.0),
            ObjectInfo::PropertyAnimator(animator) => assert_eq!(animator.weightTransition.unwrap().startedAtMs, 500.0),
            _ => {}
        }
    }
    StoredWorldRenderable::new(stored)?;
    Ok(())
}

#[test]
fn invalid_preview_start_time_is_rejected_without_changing_other_clocks() -> anyhow::Result<()> {
    let mut stored = stored_world()?;
    for (id, start_ms) in [(200, 50_000.0), (201, f64::NAN)] {
        stored.objectOfId.insert(
            id,
            LiveObject {
                id,
                name: format!("clock-{id}"),
                info: ObjectInfo::Clock(ClockInfo {
                    clockState: ClockState::Playing {
                        localStartMs: start_ms,
                        timeLocation: ObjectLocation::Server,
                    },
                    speed: 1.0,
                    shouldLoop: true,
                }),
            },
        );
    }
    assert!(StoredWorldRenderable::restart_preview_timeline(&mut stored).is_err());
    let ObjectInfo::Clock(clock) = &stored.objectOfId[&200].info else {
        unreachable!()
    };
    assert_eq!(
        clock.clockState,
        ClockState::Playing {
            localStartMs: 50_000.0,
            timeLocation: ObjectLocation::Server
        }
    );
    Ok(())
}
