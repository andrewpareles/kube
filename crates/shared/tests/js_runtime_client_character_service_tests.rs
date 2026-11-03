mod helpers;

use deno_core::JsRuntime;
use game_types::consts::CLIENT_SIM_AND_SEND_SECS;
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_ball, add_player, make_world_controlled_by_player};
use shared::game::jsRuntime::{
    JsConsoleLogEntry, JsRuntimeInitialization, ScriptManager, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime, runJsCallbacks,
};
use shared::game::physics::SetPhysicsInfo3D;
use shared::game::world::{JsCallbackInputs, World};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;

const PLAYER_OBJECT_ID: ObjectId = 99;
const CHARACTER_OBJECT_ID: ObjectId = 2;
const SCRIPT_OBJECT_ID: ObjectId = 3;
const SCRIPT_NAME: &str = "controller_script";
const BEFORE_STEP: JsCallbackInputs = JsCallbackInputs::BeforeCharacterStep {
    character_object_id: CHARACTER_OBJECT_ID,
    dt_secs: CLIENT_SIM_AND_SEND_SECS,
};
const AFTER_STEP: JsCallbackInputs = JsCallbackInputs::AfterCharacterStep {
    character_object_id: CHARACTER_OBJECT_ID,
    dt_secs: CLIENT_SIM_AND_SEND_SECS,
};

// -------------------------- helpers --------------------------

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!((actual - expected).abs() <= 1e-3, "{label}: expected ~{expected}, got {actual}");
}

fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
    assert!((actual - expected).length() <= 1e-3, "{label}: expected ~{expected:?}, got {actual:?}");
}

// A client world where the local player's character is a ball.
fn make_client_world_with_character(character_position: Vec3, is_pushable: bool) -> anyhow::Result<World> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, character_position, is_pushable)?;
    Ok(world)
}

struct RunningScript {
    js_runtime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    console_log_entry_rx: mpsc::Receiver<JsConsoleLogEntry>,
}

async fn start_script(world: &mut World, object_location: ObjectLocation, raw_code: String) -> anyhow::Result<RunningScript> {
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        SCRIPT_NAME.to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: object_location,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(SCRIPT_OBJECT_ID, raw_code);

    let (console_log_entry_tx, console_log_entry_rx) = mpsc::channel();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await?;
    Ok(RunningScript {
        js_runtime,
        script_manager_rc,
        console_log_entry_rx,
    })
}

// Calls a function the script exported, the way the other side of the network would.
async fn call_script_fn(world: &mut World, running_script: &mut RunningScript, fn_name: &str) -> anyhow::Result<serde_json::Value> {
    let fn_id = format!("{fn_name}#root/{SCRIPT_NAME}");
    let js_fn = {
        let script_manager = running_script.script_manager_rc.borrow();
        let js_fn = script_manager.get_client_js_fn(&fn_id).or_else(|| script_manager.get_server_js_fn(&fn_id));
        js_fn.ok_or_else(|| anyhow::anyhow!("Script function not found: {fn_id}"))?.clone()
    };
    let result_json = callJsFn(&mut running_script.js_runtime, world, 0.0, Some(0.0), &js_fn, r#"{"json":[]}"#).await?;
    helpers::remote_json_result(&result_json)
}

// Runs step callbacks the way the client play loop does. getTime() inside them reads step_time_ms.
fn run_step_callbacks(world: &mut World, running_script: &mut RunningScript, step_time_ms: f64, step_input: JsCallbackInputs) -> anyhow::Result<()> {
    runJsCallbacks(
        &mut running_script.js_runtime,
        world,
        step_time_ms,
        Some(step_time_ms),
        &running_script.script_manager_rc,
        vec![step_input],
    )
}

// The calls the client play loop makes around each physics step.
fn step_like_the_play_loop(world: &mut World, running_script: &mut RunningScript, step_time_ms: f64) -> anyhow::Result<()> {
    run_step_callbacks(world, running_script, step_time_ms, BEFORE_STEP)?;
    world.step_physics(1, CLIENT_SIM_AND_SEND_SECS, step_time_ms, Some(step_time_ms))?;
    run_step_callbacks(world, running_script, step_time_ms, AFTER_STEP)
}

fn take_console_errors(running_script: &RunningScript) -> Vec<String> {
    running_script
        .console_log_entry_rx
        .try_iter()
        .filter(|console_log_entry| console_log_entry.log_level == "error")
        .map(|console_log_entry| console_log_entry.message)
        .collect()
}

// -------------------------- when step callbacks run --------------------------

#[tokio::test]
async fn step_callbacks_run_right_before_and_right_after_each_physics_step() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
const log = [];
const logStep = (phase, characterObjectId, dtSecs) => {
    const character = game.objectService.getObjectById(characterObjectId);
    log.push({ phase, characterObjectId, dtSecs, timeMs: getTime(), y: character.location.position.y });
};
game.clientCharacterService.addBeforeStepListener((characterObjectId, dtSecs) => logStep("before", characterObjectId, dtSecs));
game.clientCharacterService.addAfterStepListener((characterObjectId, dtSecs) => logStep("after", characterObjectId, dtSecs));
export const getLog = () => log;
"#
        .to_string(),
    )
    .await?;

    step_like_the_play_loop(&mut world, &mut running_script, 1000.0)?;
    step_like_the_play_loop(&mut world, &mut running_script, 1016.0)?;
    let log = call_script_fn(&mut world, &mut running_script, "getLog").await?;

    let phases: Vec<&str> = log.as_array().unwrap().iter().map(|entry| entry["phase"].as_str().unwrap()).collect();
    assert_eq!(phases, vec!["before", "after", "before", "after"]);
    for entry in log.as_array().unwrap() {
        assert_eq!(entry["characterObjectId"], serde_json::json!(CHARACTER_OBJECT_ID));
        assert_close(entry["dtSecs"].as_f64().unwrap(), 1.0 / 60.0, "dtSecs");
    }
    assert_close(log[0]["timeMs"].as_f64().unwrap(), 1000.0, "first step time");
    assert_close(log[3]["timeMs"].as_f64().unwrap(), 1016.0, "second step time");

    // the ball is falling, so each after-step callback sees the solve's new position, and the next before-step callback starts from it
    let y_of_entry = |index: usize| log[index]["y"].as_f64().unwrap();
    assert_close(y_of_entry(0), 10.0, "y before the first step");
    assert!(y_of_entry(1) < y_of_entry(0), "the first solve moved the ball down: {log}");
    assert_close(y_of_entry(2), y_of_entry(1), "y before the second step");
    assert!(y_of_entry(3) < y_of_entry(2), "the second solve moved the ball down: {log}");
    Ok(())
}

// The client play loop hands each pass's key and mouse events to scripts right before physics, so a step callback
// sees keys pressed since the last pass instead of a frame late.
#[tokio::test]
async fn key_events_handed_to_scripts_before_a_step_are_held_during_it() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
const log = [];
game.clientInputService.addKeyDownListener((key) => log.push(`keydown_${key}`));
game.clientCharacterService.addBeforeStepListener(() => {
    log.push(`step_space_held_${game.clientInputService.isKeyDown("Space")}`);
});
export const getLog = () => log;
"#
        .to_string(),
    )
    .await?;

    runJsCallbacks(
        &mut running_script.js_runtime,
        &mut world,
        1000.0,
        Some(1000.0),
        &running_script.script_manager_rc,
        vec![JsCallbackInputs::KeyDown { key: "Space".to_string() }],
    )?;
    step_like_the_play_loop(&mut world, &mut running_script, 1000.0)?;

    let log = call_script_fn(&mut world, &mut running_script, "getLog").await?;
    assert_eq!(log, serde_json::json!(["keydown_Space", "step_space_held_true"]));
    Ok(())
}

#[tokio::test]
async fn a_throwing_callback_is_reported_and_the_others_still_run() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
const calls = [];
const removeSelf = () => {
    calls.push("removeSelf");
    game.clientCharacterService.removeBeforeStepListener(removeSelf);
};
game.clientCharacterService.addBeforeStepListener(() => {
    calls.push("throws");
    throw new Error("callback broke");
});
game.clientCharacterService.addBeforeStepListener(removeSelf);
game.clientCharacterService.addBeforeStepListener(() => {
    calls.push("last");
});
export const getCalls = () => calls;
"#
        .to_string(),
    )
    .await?;

    step_like_the_play_loop(&mut world, &mut running_script, 1000.0)?;
    step_like_the_play_loop(&mut world, &mut running_script, 1016.0)?;

    // a callback removing itself doesn't skip the one after it, and it is gone by the next step
    let calls = call_script_fn(&mut world, &mut running_script, "getCalls").await?;
    assert_eq!(calls, serde_json::json!(["throws", "removeSelf", "last", "throws", "last"]));

    let errors = take_console_errors(&running_script);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().all(|error| error.contains("callback broke")), "{errors:?}");
    Ok(())
}

#[tokio::test]
async fn a_runaway_callback_is_stopped_and_the_next_step_runs() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
let shouldHang = true;
let numFinishedSteps = 0;
game.clientCharacterService.addBeforeStepListener(() => {
    if (shouldHang) {
        shouldHang = false;
        while (true) {}
    }
    numFinishedSteps += 1;
});
export const getNumFinishedSteps = () => numFinishedSteps;
"#
        .to_string(),
    )
    .await?;

    let error = step_like_the_play_loop(&mut world, &mut running_script, 1000.0).expect_err("the runaway callback should be stopped");
    assert!(error.to_string().contains("CPU time budget"), "{error}");

    step_like_the_play_loop(&mut world, &mut running_script, 1016.0)?;
    let num_finished_steps = call_script_fn(&mut world, &mut running_script, "getNumFinishedSteps").await?;
    assert_eq!(num_finished_steps, serde_json::json!(1));
    Ok(())
}

// -------------------------- the changes a callback returns --------------------------

#[tokio::test]
async fn returned_velocity_changes_add_up_and_later_callbacks_see_them() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    world.forcePhysicsInfo3D(
        CHARACTER_OBJECT_ID,
        SetPhysicsInfo3D {
            velocity: Some(Vec3::new(1.0, 0.0, 0.0)),
            angularVelocity: Some(Vec3::new(0.0, 0.0, 1.0)),
            ..Default::default()
        },
        false,
    );
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
let velocitySeenBySecondCallback = null;
game.clientCharacterService.addBeforeStepListener(() => ({ velocityChange: { x: 0, y: 0, z: 2 } }));
game.clientCharacterService.addBeforeStepListener((characterObjectId) => {
    const { x, y, z } = game.objectService.getObjectById(characterObjectId).movement.velocity;
    velocitySeenBySecondCallback = { x, y, z };
    return { velocityChange: { x: 0, y: 0, z: 1 }, angularVelocityChange: { x: 0, y: 90, z: 0 } };
});
export const getVelocitySeenBySecondCallback = () => velocitySeenBySecondCallback;
"#
        .to_string(),
    )
    .await?;

    run_step_callbacks(&mut world, &mut running_script, 1000.0, BEFORE_STEP)?;

    let movement = world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap().movement;
    assert_vec3_close(movement.velocity, Vec3::new(1.0, 0.0, 3.0), "velocity");
    // spin changes are degrees per second in JS
    assert_vec3_close(movement.angularVelocity, Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 1.0), "angular velocity");
    let velocity_seen = call_script_fn(&mut world, &mut running_script, "getVelocitySeenBySecondCallback").await?;
    assert_eq!(velocity_seen, serde_json::json!({ "x": 1, "y": 0, "z": 2 }));
    Ok(())
}

#[tokio::test]
async fn a_returned_velocity_replaces_motion_and_a_returned_position_is_not_a_jump() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    world.forcePhysicsInfo3D(
        CHARACTER_OBJECT_ID,
        SetPhysicsInfo3D {
            velocity: Some(Vec3::new(1.0, 2.0, 3.0)),
            ..Default::default()
        },
        false,
    );
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
let numRuns = 0;
game.clientCharacterService.addAfterStepListener(() => {
    numRuns += 1;
    if (numRuns === 1) return { velocity: { x: 0, y: 0, z: 4 } };
    return { position: { x: 5, y: 6, z: 7 }, velocity: { x: 0, y: 0, z: 4 }, velocityChange: { x: 1, y: 0, z: 0 } };
});
game.clientCharacterService.addAfterStepListener(() => {
    if (numRuns === 2) return { position: { x: 8, y: 9, z: 10 } };
});
"#
        .to_string(),
    )
    .await?;
    let teleport_count = world.getTeleportCount(CHARACTER_OBJECT_ID);

    // a new velocity is not a jump, so the renderer keeps smoothing the character
    run_step_callbacks(&mut world, &mut running_script, 1000.0, AFTER_STEP)?;
    let physics_info_3d = world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap();
    assert_vec3_close(physics_info_3d.movement.velocity, Vec3::new(0.0, 0.0, 4.0), "replaced velocity");
    assert_vec3_close(physics_info_3d.location.position, Vec3::new(0.0, 10.0, 0.0), "position after a velocity change");
    assert_eq!(world.getTeleportCount(CHARACTER_OBJECT_ID), teleport_count);

    // a velocity change is added to a returned velocity, a later callback's position wins, and a new position is not a jump either
    run_step_callbacks(&mut world, &mut running_script, 1016.0, AFTER_STEP)?;
    let physics_info_3d = world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap();
    assert_vec3_close(physics_info_3d.movement.velocity, Vec3::new(1.0, 0.0, 4.0), "replaced then changed velocity");
    assert_vec3_close(physics_info_3d.location.position, Vec3::new(8.0, 9.0, 10.0), "the later callback's position");
    assert_eq!(world.getTeleportCount(CHARACTER_OBJECT_ID), teleport_count);
    Ok(())
}

// Returned changes do what setting the same fields from a server script does, so an unpushable character keeps a velocity it is given, like a moving platform.
#[tokio::test]
async fn an_unpushable_character_moves_the_way_a_server_script_would_move_it() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), false)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
let hasPushed = false;
game.clientCharacterService.addBeforeStepListener(() => {
    if (hasPushed) return;
    hasPushed = true;
    return { velocity: { x: 0, y: 0, z: 6 } };
});
game.clientCharacterService.addAfterStepListener(() => ({ position: { x: 1, y: 2, z: 3 } }));
"#
        .to_string(),
    )
    .await?;

    // one push, then a second of steps: no gravity pulls it down and nothing slows it
    for step_index in 0..60 {
        let step_time_ms = 1000.0 + step_index as f64 * CLIENT_SIM_AND_SEND_SECS as f64 * 1000.0;
        run_step_callbacks(&mut world, &mut running_script, step_time_ms, BEFORE_STEP)?;
        world.step_physics(1, CLIENT_SIM_AND_SEND_SECS, step_time_ms, Some(step_time_ms))?;
    }
    assert_vec3_close(
        world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap().location.position,
        Vec3::new(0.0, 10.0, 6.0),
        "position after one second at 6 per second",
    );

    // a position moves unpushable objects too, the same way moveObjectTo does on the server
    run_step_callbacks(&mut world, &mut running_script, 2000.0, AFTER_STEP)?;
    assert_vec3_close(
        world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap().location.position,
        Vec3::new(1.0, 2.0, 3.0),
        "moved unpushable position",
    );
    Ok(())
}

// A warp pipe: teleportObjectTo in a step listener puts the character there at once, and it's the one move other screens don't blend.
#[tokio::test]
async fn a_step_listener_warps_the_character_with_teleport_object_to() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
game.clientCharacterService.addAfterStepListener((characterObjectId) => {
    game.objectService.teleportObjectTo(characterObjectId, { x: 20, y: 30, z: 40 });
});
"#
        .to_string(),
    )
    .await?;
    let teleport_count = world.getTeleportCount(CHARACTER_OBJECT_ID);

    run_step_callbacks(&mut world, &mut running_script, 1000.0, AFTER_STEP)?;
    assert_vec3_close(
        world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap().location.position,
        Vec3::new(20.0, 30.0, 40.0),
        "warped position",
    );
    assert_ne!(world.getTeleportCount(CHARACTER_OBJECT_ID), teleport_count);
    let errors = take_console_errors(&running_script);
    assert!(errors.is_empty(), "{errors:?}");
    Ok(())
}

#[tokio::test]
async fn an_async_callback_or_a_non_object_return_is_reported_and_changes_nothing() -> anyhow::Result<()> {
    let mut world = make_client_world_with_character(Vec3::new(0.0, 10.0, 0.0), true)?;
    let mut running_script = start_script(
        &mut world,
        ObjectLocation::Client,
        r#"'use client'
game.clientCharacterService.addBeforeStepListener(async () => ({ velocityChange: { x: 0, y: 0, z: 5 } }));
game.clientCharacterService.addBeforeStepListener(() => 5);
"#
        .to_string(),
    )
    .await?;

    run_step_callbacks(&mut world, &mut running_script, 1000.0, BEFORE_STEP)?;

    assert_vec3_close(
        world.getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID).unwrap().movement.velocity,
        Vec3::ZERO,
        "velocity",
    );
    let errors = take_console_errors(&running_script);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("one of yours was async"), "{errors:?}");
    assert!(errors[1].contains("must return an object of changes"), "{errors:?}");
    Ok(())
}
