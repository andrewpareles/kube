mod helpers;

use helpers::world_test_helpers::make_world;
use shared::game::jsRuntime::runTemporaryScript;

// A temp script that awaits a tick used to abort the whole process.
//
// deno_core hands any async op still Pending after its inline poll to `deno_unsync::spawn`, which is
// only sound on a current-thread runtime. On a multi-thread one it panics inside the `extern "C"` v8
// callback — a non-unwinding panic, so the process aborts outright and takes any unsaved world with
// it. In release the assert is compiled out and it silently ships the v8 isolate to another thread
// instead, which is worse.
//
// `await tick()` genuinely cannot succeed in a temporary script: nothing pumps
// `signal_all_awaiting_next_ticks` there, so the watchdog is right to stop it. The point of these
// tests is that it now *reports* that, rather than killing the process before the watchdog can.
// A test that merely returns Err is a test that survived.
const AWAIT_TICK_SCRIPT: &str = "console.log('before tick')\nawait tick()\nconsole.log('after tick')\n";

// Note the flavor argument. Plain `#[tokio::test]` is current-thread, which is exactly why every
// existing js_runtime test passed while the editor died: the editor reaches runTemporaryScript from a
// multi-thread worker (the chat tab and the dev bridge both `tokio::spawn`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn temporary_script_awaiting_a_tick_fails_cleanly_on_a_multi_thread_caller() -> anyhow::Result<()> {
    let mut world = make_world()?;

    let run_result = runTemporaryScript(&mut world, AWAIT_TICK_SCRIPT.to_string());

    let error = run_result.expect_err("await tick() cannot resolve in a temporary script, so this must be an error");
    assert!(error.to_string().contains("did not finish"), "expected the watchdog's message, got: {error}");
    Ok(())
}

// Same guarantee with no ambient tokio runtime at all. `block_in_place` would panic here, which is
// why the fix owns a thread rather than borrowing the caller's runtime.
#[test]
fn temporary_script_awaiting_a_tick_fails_cleanly_with_no_ambient_runtime() -> anyhow::Result<()> {
    let mut world = make_world()?;

    let run_result = runTemporaryScript(&mut world, AWAIT_TICK_SCRIPT.to_string());

    assert!(run_result.is_err(), "await tick() cannot resolve in a temporary script");
    Ok(())
}

// The ordinary path still works from a multi-thread caller, so the dedicated thread did not break
// the common case the editor actually uses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronous_temporary_script_still_runs_on_a_multi_thread_caller() -> anyhow::Result<()> {
    let mut world = make_world()?;

    let console_logs = runTemporaryScript(&mut world, "console.log('hello from a temp script')\n".to_string())?;

    let logged = console_logs.iter().map(|entry| entry.message.trim().to_string()).collect::<Vec<_>>();
    assert!(
        logged.iter().any(|message| message == "hello from a temp script"),
        "expected the script's log line, got {logged:?}"
    );
    Ok(())
}

// Bulk edits must reach their last line even when they need more time than a gameplay tick.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn temporary_script_finishes_a_thousand_object_build_beyond_the_gameplay_tick_limit() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let initial_object_count = world.objectOfId.len();
    let console_logs = runTemporaryScript(
        &mut world,
        r#"
const startedAt = Date.now();
for (let index = 0; index < 1000; index += 1) {
  game.objectService.createObject(`bulk-ball-${index}`, {
    type: "ball",
    material: null,
    opacity: 1,
    color: { r: 0.25, g: 0.5, b: 0.75 },
    isAlwaysOnTop: false,
    radius: 1,
    location: {
      position: { x: index * 3, y: 1, z: 0 },
      rotation: { x: 0, y: 0, z: 0 },
    },
    movement: {
      velocity: { x: 0, y: 0, z: 0 },
      angularVelocity: { x: 0, y: 0, z: 0 },
    },
    isWalkthrough: false,
    isPushable: false,
    density: 1,
  }, game.world.id);
  await Promise.resolve();
}
// Even on a fast machine, this command must exceed the old 100 ms cutoff before finishing.
while (Date.now() - startedAt < 250) {}
console.log("finished all 1000 objects");
"#
        .to_string(),
    )?;

    assert_eq!(world.objectOfId.len(), initial_object_count + 1000);
    for index in 0..1000 {
        assert_eq!(world.getObjectIdsByName(&format!("bulk-ball-{index}")).len(), 1);
    }
    assert!(console_logs.iter().any(|entry| entry.message.trim() == "finished all 1000 objects"));
    assert!(world.getObjectIdsByName("__temporary_ai_script").is_empty());
    Ok(())
}
