mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world;
use helpers::world_test_helpers::make_world_controlled_by_player;
use shared::game::jsRuntime::{JsConsoleLogEntry, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use shared::game::world::World;
use std::sync::{Arc, Mutex};
use std::task::Poll;

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
async fn sibling_scripts_can_import_by_relative_path_while_exporting_a_background_tick_loop() -> anyhow::Result<()> {
    // Run the same startup path several times so a regression in script-start ordering is likely to surface.
    for _ in 0..8 {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let orbit_script_object_id = 2;
        let player_join_avatar_script_object_id = 3;
        let imported_script_suffix = 7_u32;

        // This uses the real root/orbit_script and root/player_join_avatar_script names that resolve to sibling module paths.
        add_named_script(
            &mut world,
            orbit_script_object_id,
            root_object_id,
            "orbit_script",
            &format!(
                "export const y = {};\nvoid (async () => {{\n    while (true) {{\n        await tick();\n    }}\n}})();",
                imported_script_suffix
            ),
        );
        add_named_script(
            &mut world,
            player_join_avatar_script_object_id,
            root_object_id,
            "player_join_avatar_script",
            "import { y } from \"./orbit_script\";\n\
             game.world.name = `imported_root_${y}`;",
        );

        // This runs both scripts through the real runtime so the import path resolution happens exactly like production.
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        let root_object = world.getObject(root_object_id)?;

        // This proves the importer ran and saw the exported value even though the exported module keeps its own tick loop alive.
        assert_eq!(root_object.name, format!("imported_root_{imported_script_suffix}"));
    }

    Ok(())
}

#[tokio::test]
async fn scripts_added_after_runtime_start_can_import_each_other_by_relative_path() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let orbit_script_object_id = 2;
    let player_join_avatar_script_object_id = 3;
    let imported_script_suffix = 11_u32;

    // This starts the real runtime with no user scripts first, then adds two sibling scripts in one later flush.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    add_named_script(
        &mut world,
        orbit_script_object_id,
        root_object_id,
        "orbit_script",
        &format!(
            "export const y = {};\nvoid (async () => {{\n    while (true) {{\n        await tick();\n    }}\n}})();",
            imported_script_suffix
        ),
    );
    add_named_script(
        &mut world,
        player_join_avatar_script_object_id,
        root_object_id,
        "player_join_avatar_script",
        "import { y } from \"./orbit_script\";\n\
         game.world.name = `late_imported_root_${y}`;",
    );

    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    runPendingJS(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, format!("late_imported_root_{imported_script_suffix}"));

    Ok(())
}

#[tokio::test]
async fn scripts_start_in_script_path_order_on_every_run() -> anyhow::Result<()> {
    // The dirty-script sets are HashSets, so before they were sorted this produced a different start
    // order on every single run, which made any order-dependent script bug impossible to reproduce.
    //
    // The order is by script path, not by object id, because the server throws the stored
    // ids away and reassign them by tree position on load. These scripts are all siblings under the
    // root, so their paths sort by name.
    let script_names_by_object_id = [(2, "alpha"), (3, "beta"), (4, "gamma"), (5, "delta"), (6, "epsilon")];
    let expected_order = ["alpha", "beta", "delta", "epsilon", "gamma"];

    for _ in 0..12 {
        let start_order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let start_order_for_callback = start_order.clone();
        let console_log_callback = Arc::new(move |entry: JsConsoleLogEntry| {
            start_order_for_callback.lock().unwrap().push(entry.message.trim().to_string());
        });

        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        // added in reverse so a run that just echoed insertion order would not pass by accident
        for (object_id, script_name) in script_names_by_object_id.iter().rev() {
            add_named_script(&mut world, *object_id, root_object_id, script_name, &format!("console.log('{script_name}');"));
        }

        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, Some(console_log_callback)).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

        let start_order = start_order.lock().unwrap().clone();
        assert_eq!(start_order, expected_order, "scripts started out of script path order");
    }

    Ok(())
}

#[tokio::test]
async fn player_runtime_with_no_client_scripts_is_idle_after_initialization() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // After the framework bundle has finished startup, an idle player runtime should not keep reporting pending work.
    let poll_result = std::future::poll_fn(|cx| match js_runtime.poll_event_loop(cx, Default::default()) {
        Poll::Ready(result) => Poll::Ready(Some(result)),
        Poll::Pending => Poll::Ready(None),
    })
    .await;

    let Some(result) = poll_result else {
        panic!("Idle player runtime still had pending JS work after startup");
    };
    result?;

    Ok(())
}
