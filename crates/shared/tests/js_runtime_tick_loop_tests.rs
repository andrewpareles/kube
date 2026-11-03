mod helpers;

use helpers::world_test_helpers::{add_script, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use tokio::sync::mpsc;

// A `while (true) { ...; await tick(); }` script must advance exactly one iteration per game tick
// (runPendingJS). This used to silently never resume: ticks were long-pending Rust ops whose
// wakers never reached the op driver again, so every tick loop hung forever after its first
// iteration.
#[tokio::test]
async fn tick_loop_advances_one_iteration_per_pending_js_run() -> anyhow::Result<()> {
    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
let n = 0;
while (true) {
    n += 1;
    game.world.name = "n-" + n;
    await tick();
}
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert_eq!(world.getObject(root_object_id)?.name, "n-1", "the loop must run its first iteration at startup");

    for (pump_index, live_server_sim_ms) in [16.0_f64, 32.0, 48.0].into_iter().enumerate() {
        runPendingJS(
            &mut js_runtime,
            &mut world,
            live_server_sim_ms,
            None,
            &script_manager_rc,
            &mut js_callback_inputs_rx,
        )
        .await?;
        assert_eq!(
            world.getObject(root_object_id)?.name,
            format!("n-{}", pump_index + 2),
            "the loop must advance exactly one iteration per tick"
        );
    }

    Ok(())
}

// Independent async work in one script must not replace another task's pending tick.
#[tokio::test]
async fn concurrent_tick_waits_in_one_script_all_resume_once_per_tick() -> anyhow::Result<()> {
    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
let left = 0;
let right = 0;
let joined = 0;
const updateName = () => { game.world.name = `${left}-${right}-${joined}`; };
const runLeft = async () => {
    while (true) {
        await tick();
        left += 1;
        updateName();
    }
};
const runRight = async () => {
    while (true) {
        await tick();
        right += 1;
        updateName();
    }
};
runLeft();
runRight();
updateName();
while (true) {
    await Promise.all([tick(), tick()]);
    joined += 1;
    updateName();
}
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    assert_eq!(
        world.getObject(root_object_id)?.name,
        "0-0-0",
        "tick waits must stay pending until the next game tick"
    );

    for (pump_index, live_server_sim_ms) in [16.0_f64, 32.0, 48.0].into_iter().enumerate() {
        runPendingJS(
            &mut js_runtime,
            &mut world,
            live_server_sim_ms,
            None,
            &script_manager_rc,
            &mut js_callback_inputs_rx,
        )
        .await?;
        let expected_count = pump_index + 1;
        assert_eq!(
            world.getObject(root_object_id)?.name,
            format!("{expected_count}-{expected_count}-{expected_count}"),
            "both independent loops and Promise.all must advance exactly once per tick"
        );
    }

    Ok(())
}
