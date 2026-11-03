mod helpers;

use game_types::object_types::{
    ClockInfo, ClockState, ObjectBeingAdded, ObjectId, ObjectInfo, ObjectLocation, ScriptInfo, WeightTransition, sk_animator::SkAnimatorInfo,
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_ball, add_brick, add_local_group, add_player, make_world_controlled_by_player};
use shared::game::{
    jsRuntime::{JsRuntimeInitialization, ScriptManager, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS},
    physics::SetPhysicsInfo3D,
    world::{JsCallbackInputs, World},
};
use std::{cell::RefCell, rc::Rc};
use tokio::sync::mpsc;

// This is the exact walk controller shipped in world 35 (v13+): it must stay byte-identical to
// what the world embeds, so the tests here exercise the shipped script, not a copy that drifts.
const WALK_CLIENT_SCRIPT: &str = include_str!("fixtures/walk_client.js");

const CHARACTER_ROOT_OBJECT_ID: ObjectId = 10;
const BODY_OBJECT_ID: ObjectId = 11;
const WALK_CLOCK_OBJECT_ID: ObjectId = 12;
const WALK_ANIMATOR_OBJECT_ID: ObjectId = 13;
const JUMP_ANIMATOR_OBJECT_ID: ObjectId = 14;
const GROUND_OBJECT_ID: ObjectId = 15;
const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;

const TICK_MS: f32 = 16.0;

// Builds the minimal character every walk test needs: a root group holding the body, a
// "Walk-Clock", and a "Walk-Animator" starting at weight 0 (idle), plus the player that
// controls the body. The names must match what the walk controller looks up.
fn add_walk_character(world: &mut World) -> anyhow::Result<()> {
    let root_object_id = world.getRootObjectId();
    add_local_group(world, CHARACTER_ROOT_OBJECT_ID, root_object_id)?;
    add_ball(world, BODY_OBJECT_ID, CHARACTER_ROOT_OBJECT_ID, Vec3::ZERO, true)?;
    world.addObjectWithId(
        WALK_CLOCK_OBJECT_ID,
        "Walk-Clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            shouldLoop: true,
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            speed: 1.0,
        }),
        CHARACTER_ROOT_OBJECT_ID,
    );
    world.addObjectWithId(
        WALK_ANIMATOR_OBJECT_ID,
        "Walk-Animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: None,
            clockId: None,
            skAnimationId: None,
            priority: 1,
            weight: 0.0,
            weightTransition: None,
        }),
        CHARACTER_ROOT_OBJECT_ID,
    );
    add_player(world, PLAYER_OBJECT_ID, root_object_id, Some(BODY_OBJECT_ID))?;
    Ok(())
}

fn add_walk_script(world: &mut World, object_location: ObjectLocation, raw_code: &str) {
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "walk_client".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: object_location,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(SCRIPT_OBJECT_ID, raw_code.to_string());
}

// Body positions for the jump tests: the ground brick's top is at y=-1 and the ball body has
// radius 1, so GROUNDED_POSITION leaves a half-millimeter gap — inside the engine's 1cm
// walkable-support check the walk script makes with game.objectService.getGroundSupport. AIRBORNE_POSITION is far
// above it.
const GROUNDED_POSITION: Vec3 = Vec3::new(0.0, 0.005, 0.0);
const AIRBORNE_POSITION: Vec3 = Vec3::new(0.0, 10.0, 0.0);

// The character body with a "Jump-Animator" sibling (weight 0, priority above the walk animator)
// and an unpushable ground brick the body can stand on.
fn add_jump_kit_and_ground(world: &mut World) -> anyhow::Result<()> {
    world.addObjectWithId(
        JUMP_ANIMATOR_OBJECT_ID,
        "Jump-Animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: None,
            clockId: None,
            skAnimationId: None,
            priority: 3,
            weight: 0.0,
            weightTransition: None,
        }),
        CHARACTER_ROOT_OBJECT_ID,
    );
    let root_object_id = world.getRootObjectId();
    add_brick(world, GROUND_OBJECT_ID, root_object_id, Vec3::new(0.0, -2.0, 0.0), false)?;
    Ok(())
}

fn set_body_velocity(world: &mut World, velocity: Vec3) -> anyhow::Result<()> {
    set_body_position_and_velocity(world, Vec3::ZERO, velocity)
}

fn set_body_position_and_velocity(world: &mut World, position: Vec3, velocity: Vec3) -> anyhow::Result<()> {
    world.forcePhysicsInfo3D(
        BODY_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(position),
            rotation: Some(Quat::IDENTITY),
            velocity: Some(velocity),
            angularVelocity: Some(Vec3::ZERO),
        },
        false,
    );
    Ok(())
}

// The effective weight right now, and the value the current fade is heading toward (the
// animator's base weight when no fade is running).
fn animator_weight_now_and_target(world: &World, animator_object_id: ObjectId, live_sim_ms: f64) -> anyhow::Result<(f32, f32)> {
    let ObjectInfo::SkAnimator(animator_info) = &world.getObject(animator_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected an animator at object {animator_object_id}"));
    };
    let effective_weight = WeightTransition::readValueWithFallback(animator_info.weightTransition, animator_info.weight, live_sim_ms, Some(live_sim_ms));
    let target_weight = animator_info
        .weightTransition
        .map(|transition| transition.finalValue)
        .unwrap_or(animator_info.weight);
    Ok((effective_weight, target_weight))
}

fn walk_weight_now_and_target(world: &World, live_sim_ms: f64) -> anyhow::Result<(f32, f32)> {
    animator_weight_now_and_target(world, WALK_ANIMATOR_OBJECT_ID, live_sim_ms)
}

struct WalkHarness {
    world: World,
    js_runtime: deno_core::JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    tick_index: u32,
}

impl WalkHarness {
    async fn start(world: World, object_location: ObjectLocation, raw_code: &str) -> anyhow::Result<Self> {
        Self::start_with_setup(world, object_location, raw_code, |_| Ok(())).await
    }

    async fn start_with_setup(
        mut world: World,
        object_location: ObjectLocation,
        raw_code: &str,
        setup_before_scripts: impl FnOnce(&mut World) -> anyhow::Result<()>,
    ) -> anyhow::Result<Self> {
        add_walk_character(&mut world)?;
        add_walk_script(&mut world, object_location, raw_code);
        setup_before_scripts(&mut world)?;

        let (_js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

        Ok(Self {
            world,
            js_runtime,
            script_manager_rc,
            js_callback_inputs_rx,
            tick_index: 0,
        })
    }

    fn live_sim_ms(&self) -> f64 {
        self.tick_index as f64 * TICK_MS as f64
    }

    async fn run_ticks(&mut self, tick_count: u32) -> anyhow::Result<()> {
        for _ in 0..tick_count {
            self.tick_index += 1;
            let live_sim_ms = self.live_sim_ms();
            // In production the physics step refreshes the raycast query pipeline every tick;
            // these tests skip physics stepping, so refresh it here for the grounded raycast.
            let _ = self.world.physics.step(0.0);
            runPendingJS(
                &mut self.js_runtime,
                &mut self.world,
                live_sim_ms,
                Some(live_sim_ms),
                &self.script_manager_rc,
                &mut self.js_callback_inputs_rx,
            )
            .await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The shipped client-side controller (fixtures/walk_client.js).
// ---------------------------------------------------------------------------------------------

async fn start_client_walk_harness() -> anyhow::Result<WalkHarness> {
    WalkHarness::start(make_world_controlled_by_player(PLAYER_OBJECT_ID)?, ObjectLocation::Client, WALK_CLIENT_SCRIPT).await
}

fn hud_walking_text(harness: &WalkHarness) -> anyhow::Result<String> {
    let script_manager = harness.script_manager_rc.borrow();
    let Some(gui_root) = script_manager
        .gui_root_of_script_gui_key()
        .iter()
        .find(|((script_object_id, gui_key), _)| *script_object_id == SCRIPT_OBJECT_ID && gui_key == "walking_hud")
        .map(|(_, gui_root)| gui_root)
    else {
        return Err(anyhow::anyhow!("walking_hud GUI root not found"));
    };
    fn collect_texts(children: &[shared::game::gui_types::GuiSerializedNode], texts: &mut Vec<String>) {
        for child in children {
            match child {
                shared::game::gui_types::GuiSerializedNode::Element(element_node) => collect_texts(&element_node.children, texts),
                shared::game::gui_types::GuiSerializedNode::Text(text_node) => texts.push(text_node.text.clone()),
            }
        }
    }
    let mut texts = Vec::new();
    collect_texts(&gui_root.children, &mut texts);
    Ok(texts.join(""))
}

async fn tell_client_its_character(harness: &mut WalkHarness) -> anyhow::Result<()> {
    let fn_id = format!("setLocalCharacterFromServer#root/walk_client");
    let client_js_fn = {
        let script_manager = harness.script_manager_rc.borrow();
        script_manager
            .get_client_js_fn(fn_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("Client function not found: {fn_id}"))?
            .clone()
    };
    let live_sim_ms = harness.live_sim_ms();
    shared::game::jsRuntime::callJsFn(
        &mut harness.js_runtime,
        &mut harness.world,
        live_sim_ms,
        Some(live_sim_ms),
        &client_js_fn,
        &format!(r#"{{"json":[{BODY_OBJECT_ID}]}}"#),
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn client_walk_controller_fades_the_walk_animation_in_and_out() -> anyhow::Result<()> {
    let mut harness = start_client_walk_harness().await?;

    // Idle at start: no fade has been issued.
    harness.run_ticks(5).await?;
    let (effective_weight, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0);
    assert_eq!(effective_weight, 0.0);

    // Walk at full speed: the fade to 100 starts within a couple of ticks and completes.
    set_body_velocity(&mut harness.world, Vec3::new(9.0, 0.0, 0.0))?;
    harness.run_ticks(20).await?;
    let (effective_weight, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(target_weight, 100.0, "moving must fade the walk animation in");
    assert!((effective_weight - 100.0).abs() < 1.0, "fade-in must complete, got {effective_weight}");

    // The walk clock must have been scaled to the movement speed (9 / WALK_FULL_SPEED = 1.0,
    // reached via the smoothed ramp, so just check it moved off a stale partial value).
    let ObjectInfo::Clock(walk_clock_info) = &harness.world.getObject(WALK_CLOCK_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("Expected the walk clock"));
    };
    assert!(
        (walk_clock_info.speed - 1.0).abs() < 0.06,
        "walk clock speed must approach full pace, got {}",
        walk_clock_info.speed
    );

    // Stop cleanly: the fade to 0 must start and complete — the stuck-walking regression check.
    set_body_velocity(&mut harness.world, Vec3::ZERO)?;
    harness.run_ticks(40).await?;
    let (effective_weight, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0, "stopping must fade the walk animation out");
    assert!(effective_weight < 1.0, "fade-out must complete, got {effective_weight}");
    Ok(())
}

#[tokio::test]
async fn client_walk_controller_stops_even_with_residual_idle_velocity() -> anyhow::Result<()> {
    let mut harness = start_client_walk_harness().await?;

    set_body_velocity(&mut harness.world, Vec3::new(9.0, 0.0, 0.0))?;
    harness.run_ticks(20).await?;

    // The same residual reading that wedged v12 (0.45 > its 0.3 stop threshold) must not wedge
    // the shipped controller, whose stop threshold (0.6) sits above idle physics noise.
    set_body_velocity(&mut harness.world, Vec3::new(0.45, 0.0, 0.0))?;
    harness.run_ticks(40).await?;
    let (effective_weight, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0, "residual idle velocity must not keep the walk animation on");
    assert!(effective_weight < 1.0);
    Ok(())
}

#[tokio::test]
async fn client_walk_controller_stays_walking_through_key_spam() -> anyhow::Result<()> {
    let mut harness = start_client_walk_harness().await?;

    // 5Hz key spam at 60Hz ticks: 6 ticks pressed (full speed), 6 ticks released (stopped).
    // The smoothed speed stays inside the hysteresis band, so after the initial fade-in the walk
    // state must never flip back off mid-spam.
    set_body_velocity(&mut harness.world, Vec3::new(9.0, 0.0, 0.0))?;
    harness.run_ticks(6).await?;
    let (_, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(target_weight, 100.0, "the first press must start the walk animation");

    for _ in 0..10 {
        set_body_velocity(&mut harness.world, Vec3::ZERO)?;
        for _ in 0..6 {
            harness.run_ticks(1).await?;
            let (_, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
            assert_eq!(
                target_weight, 100.0,
                "key spam must not flip the walk animation off (tick {})",
                harness.tick_index
            );
        }
        set_body_velocity(&mut harness.world, Vec3::new(9.0, 0.0, 0.0))?;
        for _ in 0..6 {
            harness.run_ticks(1).await?;
            let (_, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
            assert_eq!(
                target_weight, 100.0,
                "key spam must not flip the walk animation off (tick {})",
                harness.tick_index
            );
        }
    }

    // And a real stop after the spam still fades out.
    set_body_velocity(&mut harness.world, Vec3::ZERO)?;
    harness.run_ticks(40).await?;
    let (effective_weight, target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0);
    assert!(effective_weight < 1.0);
    Ok(())
}

#[tokio::test]
async fn jump_fall_animation_fades_in_when_airborne_and_out_on_landing() -> anyhow::Result<()> {
    let mut harness = WalkHarness::start_with_setup(
        make_world_controlled_by_player(PLAYER_OBJECT_ID)?,
        ObjectLocation::Client,
        WALK_CLIENT_SCRIPT,
        add_jump_kit_and_ground,
    )
    .await?;

    // Standing on the ground: the fall animation stays off.
    set_body_position_and_velocity(&mut harness.world, GROUNDED_POSITION, Vec3::ZERO)?;
    harness.run_ticks(5).await?;
    let (effective_weight, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0);
    assert_eq!(effective_weight, 0.0);

    // A brief 3-tick hop stays inside the airborne debounce: no fall animation.
    set_body_position_and_velocity(&mut harness.world, AIRBORNE_POSITION, Vec3::ZERO)?;
    harness.run_ticks(3).await?;
    let (_, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0, "a momentary airborne blip must not trigger the fall animation");
    set_body_position_and_velocity(&mut harness.world, GROUNDED_POSITION, Vec3::ZERO)?;
    harness.run_ticks(2).await?;

    // Sustained air time: after the debounce the fall animation fades in. Check a mid-fade
    // sample first — the fade must be a visible interpolation, not a snap to 100.
    set_body_position_and_velocity(&mut harness.world, AIRBORNE_POSITION, Vec3::ZERO)?;
    harness.run_ticks(10).await?;
    let (effective_weight, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 100.0, "staying off the ground must fade the fall animation in");
    assert!(
        effective_weight > 5.0 && effective_weight < 95.0,
        "mid-fade the fall weight must be partial (interpolating), got {effective_weight}"
    );
    harness.run_ticks(20).await?;
    let (effective_weight, _) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert!((effective_weight - 100.0).abs() < 1.0, "fall fade-in must complete, got {effective_weight}");

    // Landing registers immediately and the quicker 150ms fade-out starts — check a partial
    // mid-fade sample before completion.
    set_body_position_and_velocity(&mut harness.world, GROUNDED_POSITION, Vec3::ZERO)?;
    harness.run_ticks(4).await?;
    let (effective_weight, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0, "landing must fade the fall animation out");
    assert!(
        effective_weight > 5.0 && effective_weight < 95.0,
        "mid-fade the landing weight must be partial (interpolating), got {effective_weight}"
    );
    harness.run_ticks(20).await?;
    let (effective_weight, _) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert!(effective_weight < 1.0, "fall fade-out must complete, got {effective_weight}");

    // The walk animator was never touched by any of this (the body stayed at zero velocity).
    let (_, walk_target_weight) = walk_weight_now_and_target(&harness.world, harness.live_sim_ms())?;
    assert_eq!(walk_target_weight, 0.0);
    Ok(())
}

#[tokio::test]
async fn re_jumping_mid_landing_fade_waits_for_the_fade_out_to_finish() -> anyhow::Result<()> {
    let mut harness = WalkHarness::start_with_setup(
        make_world_controlled_by_player(PLAYER_OBJECT_ID)?,
        ObjectLocation::Client,
        WALK_CLIENT_SCRIPT,
        add_jump_kit_and_ground,
    )
    .await?;

    // Get fully airborne.
    set_body_position_and_velocity(&mut harness.world, AIRBORNE_POSITION, Vec3::ZERO)?;
    harness.run_ticks(35).await?;
    let (effective_weight, _) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert!((effective_weight - 100.0).abs() < 1.0);

    // Land and let the fade-out run partway.
    set_body_position_and_velocity(&mut harness.world, GROUNDED_POSITION, Vec3::ZERO)?;
    harness.run_ticks(4).await?;
    let (mid_landing_weight, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0);
    assert!(
        mid_landing_weight > 5.0 && mid_landing_weight < 95.0,
        "landing fade should be mid-flight, got {mid_landing_weight}"
    );

    // Jump again mid-fade-out: the fade-out must NOT be interrupted — it keeps its 0 target and
    // runs to completion even after the airborne state has committed.
    set_body_position_and_velocity(&mut harness.world, AIRBORNE_POSITION, Vec3::ZERO)?;
    harness.run_ticks(5).await?;
    let (_, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0, "the landing fade-out must finish before the next fade-in may begin");

    // Once the fade-out completes, the fade-in begins — from exactly 0, not from a partial value.
    harness.run_ticks(4).await?;
    let ObjectInfo::SkAnimator(animator_info) = &harness.world.getObject(JUMP_ANIMATOR_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("Expected the jump animator"));
    };
    let transition = animator_info
        .weightTransition
        .ok_or_else(|| anyhow::anyhow!("Expected the fade-in transition"))?;
    assert_eq!(transition.finalValue, 100.0, "the next jump's fade-in must begin after the fade-out completed");
    assert_eq!(transition.initValue, 0.0, "the fade-in must start from the fully-ended fade-out");

    harness.run_ticks(25).await?;
    let (effective_weight, _) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert!((effective_weight - 100.0).abs() < 1.0, "the fade-in must complete, got {effective_weight}");
    Ok(())
}

// Holding space bounces with only a couple of ticks of ground contact per landing. Every touch
// must run the full cycle: the quick fade-out completes first, then the next fade-in starts from
// zero and completes while airborne — the animation always ends before the next one begins.
#[tokio::test]
async fn held_space_bouncing_completes_a_full_fall_cycle_each_bounce() -> anyhow::Result<()> {
    let mut harness = WalkHarness::start_with_setup(
        make_world_controlled_by_player(PLAYER_OBJECT_ID)?,
        ObjectLocation::Client,
        WALK_CLIENT_SCRIPT,
        add_jump_kit_and_ground,
    )
    .await?;

    // First jump: get fully airborne.
    set_body_position_and_velocity(&mut harness.world, AIRBORNE_POSITION, Vec3::ZERO)?;
    harness.run_ticks(35).await?;

    for bounce in 0..4 {
        // ~2 ticks of ground contact starts the fade-out.
        set_body_position_and_velocity(&mut harness.world, GROUNDED_POSITION, Vec3::ZERO)?;
        harness.run_ticks(2).await?;
        let (_, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
        assert_eq!(target_weight, 0.0, "bounce {bounce}: touching the ground must start the fade-out");

        // Back in the air: shortly after, the fade-out is still finishing (not interrupted) ...
        set_body_position_and_velocity(&mut harness.world, AIRBORNE_POSITION, Vec3::ZERO)?;
        harness.run_ticks(4).await?;
        let (_, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
        assert_eq!(target_weight, 0.0, "bounce {bounce}: the fade-out must run to completion first");

        // ... and by the end of the hop the fade-in has started from zero and fully re-engaged.
        harness.run_ticks(32).await?;
        let (effective_weight, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
        assert_eq!(target_weight, 100.0, "bounce {bounce}: the next fade-in must have begun");
        assert!(
            (effective_weight - 100.0).abs() < 1.0,
            "bounce {bounce}: the fall pose must be fully re-engaged before the next landing, got {effective_weight}"
        );
    }

    // A real landing still fades all the way out.
    set_body_position_and_velocity(&mut harness.world, GROUNDED_POSITION, Vec3::ZERO)?;
    harness.run_ticks(15).await?;
    let (effective_weight, target_weight) = animator_weight_now_and_target(&harness.world, JUMP_ANIMATOR_OBJECT_ID, harness.live_sim_ms())?;
    assert_eq!(target_weight, 0.0);
    assert!(effective_weight < 1.0);
    Ok(())
}

#[tokio::test]
async fn walking_hud_shows_the_local_players_walking_boolean() -> anyhow::Result<()> {
    let mut harness = start_client_walk_harness().await?;
    harness.run_ticks(2).await?;

    // Before the server delivers the local character id, the HUD shows false.
    assert_eq!(hud_walking_text(&harness)?, "walking: false");

    tell_client_its_character(&mut harness).await?;

    set_body_velocity(&mut harness.world, Vec3::new(9.0, 0.0, 0.0))?;
    harness.run_ticks(10).await?;
    assert_eq!(hud_walking_text(&harness)?, "walking: true");

    set_body_velocity(&mut harness.world, Vec3::ZERO)?;
    harness.run_ticks(40).await?;
    assert_eq!(hud_walking_text(&harness)?, "walking: false");
    Ok(())
}
