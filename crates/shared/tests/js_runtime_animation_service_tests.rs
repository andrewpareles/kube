mod helpers;

use game_types::object_types::{
    ClockInfo, ClockState, InterpolationStyle, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PropertyAnimationInfo,
    PropertyKeyframeInfo, ScriptInfo, WeightTransition,
};
use glam::Vec3;
use helpers::world_test_helpers::{
    add_ball, add_clock, add_property_animator, add_script, add_sk_animator, make_world, make_world_controlled_by_player, make_world_for_callbacks,
};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::world::World;

fn assert_close_f32(actual: f32, expected: f32, label: &str) {
    let difference = (actual - expected).abs();
    assert!(
        difference <= 1e-5,
        "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
    );
}

async fn run_world_scripts(world: &mut shared::game::world::World, live_sim_ms: f64) -> anyhow::Result<()> {
    run_world_scripts_at(world, live_sim_ms, Some(live_sim_ms)).await
}

async fn run_world_scripts_at(world: &mut shared::game::world::World, est_live_server_sim_ms: f64, live_client_sim_ms: Option<f64>) -> anyhow::Result<()> {
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, est_live_server_sim_ms, live_client_sim_ms, &script_manager_rc).await
}

fn add_client_script(world: &mut World, object_id: ObjectId, parent_id: ObjectId) {
    world.addObjectWithId(
        object_id,
        format!("script-{object_id}"),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        parent_id,
    );
    world.markScriptAsReadyToRun(object_id, format!("return {object_id};"));
}

#[tokio::test]
async fn clock_service_pause_uses_server_time_for_server_clock() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_client_script(&mut world, script_object_id, root_object_id);
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 1000.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 2.0,
        },
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.clockService.pause({});
",
            clock_object_id
        ),
    );

    run_world_scripts_at(&mut world, 4000.0, Some(9000.0)).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    let ClockState::Paused { animationTimeSecs } = clock_info.clockState else {
        return Err(anyhow::anyhow!("Expected paused clock"));
    };
    assert_close_f32(animationTimeSecs, 6.0, "clock.clockState.animationTimeSecs");
    Ok(())
}

#[tokio::test]
async fn clock_service_pause_and_play_update_clock_state() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_client_script(&mut world, script_object_id, root_object_id);

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.clockService.pause({});
game.clockService.play({}, 0.25);
",
            clock_object_id, clock_object_id
        ),
    );

    run_world_scripts(&mut world, 250.0).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    assert_eq!(
        clock_info.clockState,
        ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Client,
        }
    );

    Ok(())
}

#[tokio::test]
async fn clock_service_set_speed_preserves_elapsed_time() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 1000.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 2.0,
        },
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
const clock = game.objectService.getObjectById({});
if (clock === null || clock.type !== 'clock') throw new Error('Expected clock');
game.clockService.setSpeed({}, -4);
",
            clock_object_id, clock_object_id
        ),
    );

    run_world_scripts(&mut world, 4000.0).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    assert_close_f32(clock_info.speed, -4.0, "clock.speed");
    let ClockState::Playing { localStartMs, timeLocation } = clock_info.clockState else {
        return Err(anyhow::anyhow!("Expected playing clock"));
    };
    assert_eq!(timeLocation, ObjectLocation::Server);
    assert_close_f32(localStartMs as f32, 5500.0, "clock.clockState.localStartMs");
    Ok(())
}

#[tokio::test]
async fn clock_service_set_speed_preserves_negative_elapsed_time() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 1000.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: -2.0,
        },
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.clockService.setSpeed({}, 4);
",
            clock_object_id
        ),
    );

    run_world_scripts(&mut world, 4000.0).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    assert_close_f32(clock_info.speed, 4.0, "clock.speed");
    let ClockState::Playing { localStartMs, timeLocation } = clock_info.clockState else {
        return Err(anyhow::anyhow!("Expected playing clock"));
    };
    assert_eq!(timeLocation, ObjectLocation::Server);
    assert_close_f32(localStartMs as f32, 5500.0, "clock.clockState.localStartMs");
    assert_close_f32(
        clock_info.clockState.readRawTimeMs(clock_info.speed, 4000.0, None) as f32,
        -6000.0,
        "clock elapsed",
    );
    Ok(())
}

#[tokio::test]
async fn clock_service_set_speed_zero_pauses_at_current_elapsed_time() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 1000.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 2.0,
        },
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
const clock = game.objectService.getObjectById({});
if (clock === null || clock.type !== 'clock') throw new Error('Expected clock');
game.clockService.setSpeed({}, 0);
",
            clock_object_id, clock_object_id
        ),
    );

    run_world_scripts(&mut world, 4000.0).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    assert_close_f32(clock_info.speed, 0.0, "clock.speed");
    let ClockState::Paused { animationTimeSecs } = clock_info.clockState else {
        return Err(anyhow::anyhow!("Expected paused clock"));
    };
    assert_close_f32(animationTimeSecs, 6.0, "clock.clockState.animationTimeSecs");
    Ok(())
}

#[tokio::test]
async fn clock_service_play_allows_negative_elapsed_time() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Paused { animationTimeSecs: 0.0 },
            shouldLoop: true,
            speed: 1.0,
        },
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.clockService.play({}, -0.25);
",
            clock_object_id
        ),
    );

    run_world_scripts(&mut world, 4000.0).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    let ClockState::Playing { localStartMs, timeLocation } = clock_info.clockState else {
        return Err(anyhow::anyhow!("Expected playing clock"));
    };
    assert_eq!(timeLocation, ObjectLocation::Server);
    assert_close_f32(localStartMs as f32, 4250.0, "clock.clockState.localStartMs");
    assert_close_f32(
        clock_info.clockState.readRawTimeMs(clock_info.speed, 4000.0, None) as f32,
        -250.0,
        "clock elapsed",
    );
    Ok(())
}

#[tokio::test]
async fn assigning_clock_speed_only_changes_speed() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2;
    let script_object_id = 3;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 1000.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 2.0,
        },
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
const clock = game.objectService.getObjectById({});
if (clock === null || clock.type !== 'clock') throw new Error('Expected clock');
clock.speed = -4;
",
            clock_object_id
        ),
    );

    run_world_scripts(&mut world, 4000.0).await?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    assert_close_f32(clock_info.speed, -4.0, "clock.speed");
    let ClockState::Playing { localStartMs, timeLocation } = clock_info.clockState else {
        return Err(anyhow::anyhow!("Expected playing clock"));
    };
    assert_eq!(timeLocation, ObjectLocation::Server);
    assert_close_f32(localStartMs as f32, 1000.0, "clock.clockState.localStartMs");
    Ok(())
}

#[tokio::test]
async fn animation_service_defaults_missing_interpolation_style_to_ease_in_out_on_client() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;
    let script_object_id = 3;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    add_client_script(&mut world, script_object_id, root_object_id);

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.animationService.smoothlyChangeWeight({}, 25, 500);
",
            animator_object_id
        ),
    );

    run_world_scripts(&mut world, 125.0).await?;

    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::SkAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected SkAnimator object"));
    };
    let Some(WeightTransition {
        startedAtMs,
        timeLocation,
        initValue,
        finalValue,
        durationMs,
        easing,
    }) = animator_info.weightTransition
    else {
        panic!("Expected animator target weight to be set");
    };

    assert_close_f32(startedAtMs as f32, 125.0, "startedAtMs");
    assert_eq!(timeLocation, ObjectLocation::Client);
    assert_eq!(easing, InterpolationStyle::EaseInOut);
    assert_close_f32(initValue, 100.0, "weightTransition.initValue");
    assert_close_f32(finalValue, 25.0, "weightTransition.finalValue");
    assert_close_f32(durationMs, 500.0, "weightTransition.durationMs");

    Ok(())
}

#[tokio::test]
async fn animation_service_smoothly_changes_weight_on_server_with_server_time() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;
    let script_object_id = 3;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.animationService.smoothlyChangeWeight({}, 75, 400);
",
            animator_object_id
        ),
    );

    run_world_scripts(&mut world, 250.0).await?;
    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::SkAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected SkAnimator object"));
    };
    let Some(WeightTransition {
        startedAtMs,
        timeLocation,
        initValue,
        finalValue,
        durationMs,
        easing,
    }) = animator_info.weightTransition
    else {
        panic!("Expected server smoothlyChangeWeight to set weightTransition");
    };
    assert_close_f32(startedAtMs as f32, 250.0, "startedAtMs");
    assert_eq!(timeLocation, ObjectLocation::Server);
    assert_eq!(easing, InterpolationStyle::EaseInOut);
    assert_close_f32(initValue, 100.0, "weightTransition.initValue");
    assert_close_f32(finalValue, 75.0, "weightTransition.finalValue");
    assert_close_f32(durationMs, 400.0, "weightTransition.durationMs");

    Ok(())
}

#[tokio::test]
async fn animation_service_restarts_from_current_sampled_weight() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;
    let script_object_id = 3;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    add_client_script(&mut world, script_object_id, root_object_id);
    {
        let animator_object = world.getObjectMut(animator_object_id)?;
        let ObjectInfo::SkAnimator(animator_info) = &mut animator_object.info else {
            return Err(anyhow::anyhow!("Expected SkAnimator object"));
        };
        animator_info.weightTransition = Some(WeightTransition {
            startedAtMs: 100.0,
            timeLocation: ObjectLocation::Client,
            initValue: 100.0,
            finalValue: 50.0,
            durationMs: 200.0,
            easing: InterpolationStyle::Linear,
        });
    }

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.animationService.smoothlyChangeWeight({}, 0, 100, \"Linear\");
",
            animator_object_id
        ),
    );

    run_world_scripts(&mut world, 200.0).await?;

    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::SkAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected SkAnimator object"));
    };
    let Some(WeightTransition {
        startedAtMs,
        timeLocation,
        initValue,
        finalValue,
        durationMs,
        easing,
    }) = animator_info.weightTransition
    else {
        panic!("Expected animator target weight to be set");
    };

    assert_close_f32(startedAtMs as f32, 200.0, "startedAtMs");
    assert_eq!(timeLocation, ObjectLocation::Client);
    assert_eq!(easing, InterpolationStyle::Linear);
    assert_close_f32(initValue, 75.0, "weightTransition.initValue");
    assert_close_f32(finalValue, 0.0, "weightTransition.finalValue");
    assert_close_f32(durationMs, 100.0, "weightTransition.durationMs");

    Ok(())
}

#[tokio::test]
async fn animation_service_allows_zero_duration_weight_transition() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;
    let script_object_id = 3;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    add_client_script(&mut world, script_object_id, root_object_id);

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.animationService.smoothlyChangeWeight({}, 25, 0);
",
            animator_object_id
        ),
    );

    run_world_scripts(&mut world, 0.0).await?;
    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::SkAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected SkAnimator object"));
    };
    let Some(WeightTransition {
        startedAtMs,
        timeLocation,
        initValue,
        finalValue,
        durationMs,
        easing,
    }) = animator_info.weightTransition
    else {
        panic!("Expected zero-duration smoothlyChangeWeight call to set an immediate weightTransition");
    };

    assert_close_f32(startedAtMs as f32, 0.0, "startedAtMs");
    assert_eq!(timeLocation, ObjectLocation::Client);
    assert_eq!(easing, InterpolationStyle::DEFAULT);
    assert_close_f32(initValue, 100.0, "weightTransition.initValue");
    assert_close_f32(finalValue, 25.0, "weightTransition.finalValue");
    assert_close_f32(durationMs, 0.0, "weightTransition.durationMs");

    Ok(())
}

#[tokio::test]
async fn animation_service_allows_property_animator_physics_path() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let animator_object_id = 3;
    let clock_object_id = 4;
    let script_object_id = 5;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_property_animator(&mut world, animator_object_id, root_object_id)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_client_script(&mut world, script_object_id, root_object_id);

    {
        let animator_object = world.getObjectMut(animator_object_id)?;
        let ObjectInfo::PropertyAnimator(animator_info) = &mut animator_object.info else {
            return Err(anyhow::anyhow!("Expected property animator object"));
        };
        animator_info.targetObjectId = Some(ObjectIdPointer(ball_object_id));
        animator_info.clockId = Some(ObjectIdPointer(clock_object_id));
        animator_info.propertyPath = vec!["movement".to_string(), "velocity".to_string(), "x".to_string()];
    }

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "
game.animationService.smoothlyChangeWeight({}, 25, 100);
",
            animator_object_id
        ),
    );

    run_world_scripts(&mut world, 0.0).await?;
    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::PropertyAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected property animator object"));
    };
    assert!(
        animator_info.weightTransition.is_some(),
        "Expected physics property path to allow weightTransition"
    );

    Ok(())
}

// tweenTo starts from the number the script reads, so a rotation starts from the degrees last written to it. A path that
// is not a number is refused before anything happens.
#[tokio::test]
async fn animation_service_tweens_from_the_value_a_script_reads() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let script_object_id = 3;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    // the Tween gets the next free id, so it must come after the ones picked here
    world.objectIdCounter = script_object_id;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const ball = game.objectService.getObjectById({ball_object_id});
ball.location.rotation = {{ x: 120, y: 30, z: 0 }};
game.animationService.tweenTo({ball_object_id}, ["location", "rotation", "x"], 170, 1000, "Linear");
try {{
    game.animationService.tweenTo({ball_object_id}, ["name"], 1, 1000);
}} catch (error) {{
    ball.name = "refused";
}}
"#
        ),
    );

    run_world_scripts(&mut world, 0.0).await?;
    assert_eq!(world.getObject(ball_object_id)?.name, "refused", "a path that is not a number must throw");

    world.step_server(1, 1.0 / 60.0, 500.0)?;
    let Some(rotation_degrees) = world.getRotationDegrees(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball rotation"));
    };
    assert_close_f32(rotation_degrees.x, 145.0, "halfway from the 120 degrees the script read");
    assert!((rotation_degrees.y - 30.0).abs() < 1e-3, "y stays at {}", rotation_degrees.y);
    Ok(())
}

#[test]
fn removing_sk_animator_with_weight_transition_succeeds() -> anyhow::Result<()> {
    let mut world = make_world_for_callbacks()?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    {
        let animator_object = world.getObjectMut(animator_object_id)?;
        let ObjectInfo::SkAnimator(animator_info) = &mut animator_object.info else {
            return Err(anyhow::anyhow!("Expected SkAnimator object"));
        };
        animator_info.weightTransition = Some(WeightTransition {
            startedAtMs: 10.0,
            timeLocation: ObjectLocation::Client,
            initValue: 20.0,
            finalValue: 30.0,
            durationMs: 40.0,
            easing: InterpolationStyle::Linear,
        });
    }

    world.removeObject(animator_object_id)?;
    assert!(!world.hasObject(animator_object_id));

    Ok(())
}

#[test]
fn finished_animator_weight_transition_sets_base_weight_and_removes_transition() -> anyhow::Result<()> {
    let mut world = make_world_for_callbacks()?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    {
        let animator_object = world.getObjectMut(animator_object_id)?;
        let ObjectInfo::SkAnimator(animator_info) = &mut animator_object.info else {
            return Err(anyhow::anyhow!("Expected SkAnimator object"));
        };
        animator_info.weightTransition = Some(WeightTransition {
            startedAtMs: 10.0,
            timeLocation: ObjectLocation::Client,
            initValue: 20.0,
            finalValue: 30.0,
            durationMs: 40.0,
            easing: InterpolationStyle::EaseOut,
        });
    }

    world.flushAndSetFinishedWeightTransitions(60.0, Some(60.0));

    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::SkAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected SkAnimator object"));
    };
    assert_close_f32(animator_info.weight, 30.0, "animator.weight");
    assert!(animator_info.weightTransition.is_none(), "Expected finished weight transition to be removed");

    Ok(())
}

#[test]
fn unfinished_animator_weight_transition_stays_on_animator() -> anyhow::Result<()> {
    let mut world = make_world_for_callbacks()?;
    let root_object_id = world.getRootObjectId();
    let animator_object_id = 2;

    add_sk_animator(&mut world, animator_object_id, root_object_id)?;
    {
        let animator_object = world.getObjectMut(animator_object_id)?;
        let ObjectInfo::SkAnimator(animator_info) = &mut animator_object.info else {
            return Err(anyhow::anyhow!("Expected SkAnimator object"));
        };
        animator_info.weightTransition = Some(WeightTransition {
            startedAtMs: 10.0,
            timeLocation: ObjectLocation::Client,
            initValue: 20.0,
            finalValue: 30.0,
            durationMs: 100.0,
            easing: InterpolationStyle::EaseOut,
        });
    }

    world.flushAndSetFinishedWeightTransitions(60.0, Some(60.0));

    let animator_object = world.getObject(animator_object_id)?;
    let ObjectInfo::SkAnimator(animator_info) = &animator_object.info else {
        return Err(anyhow::anyhow!("Expected SkAnimator object"));
    };
    assert_close_f32(animator_info.weight, 100.0, "animator.weight");
    assert!(animator_info.weightTransition.is_some(), "Expected unfinished weight transition to stay");

    Ok(())
}

// PropertyKeyframes are real objects, so a script editing one rewrites the curve the sampler reads.
// This walks the whole path: script writes keyframe.value -> afterObjectInfoChange -> refreshPropertyKeyframes
// -> the cache the animator samples from.
#[tokio::test]
async fn property_keyframe_value_written_by_a_script_changes_the_animation() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let animator_object_id = 4;
    let script_object_id = 5;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, animator_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    // this test assigns object ids by hand, so keep the animation objects in their own id range
    let property_animation_id = 1000;
    let start_keyframe_id = 1001;
    let end_keyframe_id = 1002;
    world.addObjectWithId(
        property_animation_id,
        "animation".to_string(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        start_keyframe_id,
        "start".to_string(),
        ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 0.0, value: 0.0 }),
        property_animation_id,
    );
    world.addObjectWithId(
        end_keyframe_id,
        "end".to_string(),
        ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 1.0, value: 10.0 }),
        property_animation_id,
    );

    {
        let animator_object = world.getObjectMut(animator_object_id)?;
        let ObjectInfo::PropertyAnimator(animator_info) = &mut animator_object.info else {
            return Err(anyhow::anyhow!("Expected property animator object"));
        };
        animator_info.targetObjectId = Some(ObjectIdPointer(ball_object_id));
        animator_info.propertyPath = ["location", "position", "x"].iter().map(|part| part.to_string()).collect();
        animator_info.propertyAnimationId = Some(ObjectIdPointer(property_animation_id));
        animator_info.clockId = Some(ObjectIdPointer(clock_object_id));
        animator_info.priority = 5;
        animator_info.weight = 100.0;
    }
    world.setClockInfo(
        clock_object_id,
        ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: false,
            speed: 1.0,
        },
    )?;

    // halfway along a 0..10 linear ramp
    world.step_server(1, 1.0 / 60.0, 500.0)?;
    let position_x = world
        .getObjectPhysicsInfo3D(ball_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected ball physics info"))?
        .location
        .position
        .x;
    assert_close_f32(position_x, 5.0, "position.x before the keyframe edit");

    // a script moves the start keyframe up to 4.0, so the same clock time now samples 4 + (10-4)*0.5 = 7
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const startKeyframe = game.objectService.getObjectById({start_keyframe_id});
if (startKeyframe === null || startKeyframe.type !== "propertyKeyframe") {{
    throw new Error("Expected the start propertyKeyframe");
}}
startKeyframe.value = 4.0;
"#
        ),
    );
    run_world_scripts(&mut world, 500.0).await?;

    let ObjectInfo::PropertyKeyframe(start_keyframe_info) = &world.getObject(start_keyframe_id)?.info else {
        return Err(anyhow::anyhow!("Expected the start keyframe to still be a PropertyKeyframe"));
    };
    assert_close_f32(start_keyframe_info.value, 4.0, "start keyframe value after the script");

    world.step_server(2, 1.0 / 60.0, 500.0)?;
    let position_x = world
        .getObjectPhysicsInfo3D(ball_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected ball physics info"))?
        .location
        .position
        .x;
    assert_close_f32(position_x, 7.0, "position.x after the keyframe edit");

    Ok(())
}
