mod helpers;

// Use the real OS clock to generate a known amount of CPU work, independent of machine speed.
#[path = "../src/game/js_cpu_watchdog.rs"]
#[allow(dead_code)]
mod js_cpu_watchdog;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world;
use js_cpu_watchdog::ThreadCpuHandle;
use shared::game::{
    jsRuntime::{JsRuntimeInitialization, callRegisteredServerFn, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS, runTemporaryScript},
    world::World,
};
use std::{sync::Arc, time::Duration};

fn add_script(world: &mut World, id: ObjectId, name: &str, code: &str) {
    world.addObjectWithId(
        id,
        name.into(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        world.getRootObjectId(),
    );
    world.markScriptAsReadyToRun(id, code.into());
}

fn burn_cpu(duration: Duration) {
    let cpu_handle = ThreadCpuHandle::for_current_thread().unwrap();
    let deadline = cpu_handle.read().unwrap() + duration;
    while cpu_handle.read().unwrap() < deadline {
        std::hint::spin_loop();
    }
}

#[tokio::test]
async fn runaway_startup_is_reported_and_next_script_runs() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(&mut world, 2, "a_runaway", "while (true) {}");
    add_script(&mut world, 3, "b_healthy", "game.world.name = 'healthy';");
    let (log_tx, log_rx) = std::sync::mpsc::channel();
    let (mut runtime, manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |entry| {
            log_tx.send(entry).unwrap();
        })),
    )
    .await?;

    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "healthy");
    let errors = log_rx.try_iter().filter(|entry| entry.log_level == "error").collect::<Vec<_>>();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("a_runaway"));
    assert!(errors[0].message.contains("CPU time budget"));
    Ok(())
}

#[tokio::test]
async fn startup_scripts_get_independent_budgets() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(&mut world, 2, "a", "console.log('burn');");
    add_script(&mut world, 3, "b", "console.log('burn'); game.world.name = 'both ran';");
    let (mut runtime, manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(|entry| {
            if entry.message.trim() == "burn" {
                burn_cpu(Duration::from_millis(60));
            }
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "both-ran");
    Ok(())
}

#[tokio::test]
async fn sleeping_other_threads_and_unresolved_promises_do_not_exhaust_budget() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(&mut world, 2, "a_sleep", "console.log('sleep'); game.world.name = 'awake';");
    add_script(&mut world, 3, "b_pending", "void (async () => { await new Promise(() => {}); })();");
    let (log_tx, log_rx) = std::sync::mpsc::channel();
    let (mut runtime, manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |entry| {
            if entry.message.trim() == "sleep" {
                // The JS thread is idle while the process as a whole uses more than its budget.
                std::thread::spawn(|| burn_cpu(Duration::from_millis(250))).join().unwrap();
            }
            log_tx.send(entry).unwrap();
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    runPendingJS(&mut runtime, &mut world, 0.0, None, &manager_rc, &mut rx).await?;
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "awake");
    assert!(log_rx.try_iter().all(|entry| entry.log_level != "error"));
    Ok(())
}

#[tokio::test]
async fn runaway_tick_is_stopped_and_runtime_can_be_reused() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(&mut world, 2, "runaway", "await tick(); while (true) {}");
    let (mut runtime, manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let error = runPendingJS(&mut runtime, &mut world, 0.0, None, &manager_rc, &mut rx).await.unwrap_err();
    assert!(error.to_string().contains("CPU time budget"), "{error}");
    add_script(&mut world, 3, "healthy", "game.world.name = 'recovered';");
    runPendingJS(&mut runtime, &mut world, 0.0, None, &manager_rc, &mut rx).await?;
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "recovered");
    Ok(())
}

#[tokio::test]
async fn calls_exclude_other_tokio_tasks_but_accumulate_cpu_across_awaits() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(
        &mut world,
        2,
        "calls",
        r#"'use server';
export const wait = async () => {
    console.log('waiting');
    await new Promise(resolve => setTimeout(resolve, 400));
    return 42;
};
export const burn = async () => {
    for (let i = 0; i < 8; i++) {
        console.log('burn');
        await new Promise(resolve => setTimeout(resolve, 1));
    }
};
export const runaway = () => { while (true) {} };
export const healthy = () => 42;
"#,
    );
    let (waiting_tx, mut waiting_rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut runtime, manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |entry| {
            if entry.message.trim() == "waiting" {
                waiting_tx.send(()).unwrap();
            }
            if entry.message.trim() == "burn" {
                burn_cpu(Duration::from_millis(35));
            }
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;

    let other_task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), waiting_rx.recv())
            .await
            .expect("The JS console callback should signal that it is waiting")
            .unwrap();
        burn_cpu(Duration::from_millis(250));
    });
    let result = callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "wait#root/calls", r#"{"json":[]}"#).await?;
    assert_eq!(helpers::remote_json_result::<i32>(&result)?, 42);
    other_task.await?;

    for name in ["burn", "runaway"] {
        let error = callRegisteredServerFn(
            &mut runtime,
            &mut world,
            0.0,
            None,
            &manager_rc,
            0,
            &format!("{name}#root/calls"),
            r#"{"json":[]}"#,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("CPU time budget"), "{error}");
        let result = callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "healthy#root/calls", r#"{"json":[]}"#).await?;
        assert_eq!(helpers::remote_json_result::<i32>(&result)?, 42);
    }
    Ok(())
}

#[test]
fn temporary_scripts_have_a_larger_budget() -> anyhow::Result<()> {
    let mut world = make_world()?;
    runTemporaryScript(
        &mut world,
        "const end = Date.now() + 300; while (Date.now() < end) {} game.world.name = 'done';".into(),
    )?;
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "done");
    Ok(())
}

// Without the stop, V8 ends the whole process when a script fills its heap, which took the editor down with it.
#[test]
fn a_temporary_script_that_fills_its_memory_is_stopped() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let error = runTemporaryScript(&mut world, "const arrays = []; for (;;) arrays.push(new Array(1e6).fill(0.5));".into()).unwrap_err();
    assert!(error.to_string().contains("memory limit"), "{error}");
    runTemporaryScript(&mut world, "game.world.name = 'recovered';".into())?;
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "recovered");
    Ok(())
}

// A game script that keeps everything it makes is stopped once its heap is full, and the game's other functions keep working.
#[tokio::test]
async fn a_leaking_script_is_stopped_and_the_runtime_can_be_reused() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(
        &mut world,
        2,
        "hoarder",
        r#"'use server';
const arrays = [];
export const hoard = () => { for (let i = 0; i < 4; i++) arrays.push(new Array(1e6).fill(0.5)); };
export const healthy = () => 42;
"#,
    );
    let (mut runtime, manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    // Each call keeps another 32 MB, so the 1 GB heap fills after about 30 calls.
    let mut memory_error = None;
    for _ in 0..100 {
        if let Err(error) = callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "hoard#root/hoarder", r#"{"json":[]}"#).await
            && error.to_string().contains("memory limit")
        {
            memory_error = Some(error);
            break;
        }
    }
    assert!(memory_error.is_some(), "the heap never filled");
    let result = callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "healthy#root/hoarder", r#"{"json":[]}"#).await?;
    assert_eq!(helpers::remote_json_result::<i32>(&result)?, 42);
    Ok(())
}

#[tokio::test]
async fn cancelling_a_pending_call_disarms_the_watchdog() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(
        &mut world,
        2,
        "cancellation",
        r#"'use server';
export const wait = async () => {
    game.world.name = 'waiting';
    await new Promise(resolve => setTimeout(resolve, 10000));
};
export const healthy = () => 42;
"#,
    );
    let (mut runtime, manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    let cancelled = tokio::time::timeout(
        Duration::from_millis(20),
        callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "wait#root/cancellation", r#"{"json":[]}"#),
    )
    .await;
    assert!(cancelled.is_err());
    assert_eq!(world.getObject(world.getRootObjectId())?.name, "waiting");

    // Work after cancellation must not trigger a delayed termination of the next call.
    burn_cpu(Duration::from_millis(250));
    let result = callRegisteredServerFn(
        &mut runtime,
        &mut world,
        0.0,
        None,
        &manager_rc,
        0,
        "healthy#root/cancellation",
        r#"{"json":[]}"#,
    )
    .await?;
    assert_eq!(helpers::remote_json_result::<i32>(&result)?, 42);
    Ok(())
}

#[tokio::test]
async fn repeated_microtask_runaways_are_stopped_without_poisoning_the_runtime() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_script(
        &mut world,
        2,
        "microtasks",
        r#"'use server';
export const runaway = async () => { while (true) { await Promise.resolve(); } };
export const healthy = () => 42;
"#,
    );
    let (mut runtime, manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;
    for _ in 0..10 {
        let error = callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "runaway#root/microtasks", r#"{"json":[]}"#)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("CPU time budget"), "{error}");
        let result = callRegisteredServerFn(&mut runtime, &mut world, 0.0, None, &manager_rc, 0, "healthy#root/microtasks", r#"{"json":[]}"#).await?;
        assert_eq!(helpers::remote_json_result::<i32>(&result)?, 42);
    }
    Ok(())
}
