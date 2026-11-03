mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world;
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use shared::game::world::World;

fn add_named_script(world: &mut World, object_id: ObjectId, parent_id: ObjectId, name: &str, raw_code: &str) {
    world.addObjectWithId(
        object_id,
        name.to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        parent_id,
    );
    world.markScriptAsReadyToRun(object_id, raw_code.to_string());
}

#[tokio::test]
async fn queue_microtask_throw_does_not_stop_later_callbacks() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "queue_microtask_throw_script",
        r#"
queueMicrotask(() => {
    throw new Error("queueMicrotask failed");
});
"#,
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    add_named_script(
        &mut world,
        3,
        root_object_id,
        "queue_microtask_followup_script",
        r#"
game.world.name = "microtask_kept_running";
"#,
    );

    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "microtask_kept_running");

    Ok(())
}

#[tokio::test]
async fn top_level_stop_script_does_not_stop_later_scripts() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "stop_script",
        r#"
await stopScript();
game.world.name = "after_stop_script";
"#,
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "root");

    add_named_script(
        &mut world,
        3,
        root_object_id,
        "stop_script_followup_script",
        r#"
game.world.name = "followup_ran";
"#,
    );

    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "followup_ran");

    Ok(())
}

#[tokio::test]
async fn timer_throw_does_not_stop_later_timers() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "timer_throw_script",
        r#"
setTimeout(() => {
    throw new Error("timer failed");
}, 0);
"#,
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    add_named_script(
        &mut world,
        3,
        root_object_id,
        "timer_followup_script",
        r#"
game.world.name = "timer_kept_running";
"#,
    );

    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "timer_kept_running");

    Ok(())
}

#[tokio::test]
async fn event_listener_throw_does_not_stop_later_script_code() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "event_listener_throw_script",
        r#"
const eventTarget = new EventTarget();
eventTarget.addEventListener("test", () => {
    throw new Error("event listener failed");
});
eventTarget.dispatchEvent(new Event("test"));
"#,
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    add_named_script(
        &mut world,
        3,
        root_object_id,
        "event_listener_followup_script",
        r#"
game.world.name = "event_listener_kept_running";
"#,
    );

    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "event_listener_kept_running");

    Ok(())
}
