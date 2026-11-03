mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::{make_world, make_world_controlled_by_player};
use shared::game::jsRuntime::{JsRuntimeClientWorldDelay, JsRuntimeInitialization, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime};

const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;

// getTime() must return whatever sim time the runtime was given for the current run:
// the tick's time on the server, the synced estimate on clients.
#[tokio::test]
async fn get_time_returns_the_est_live_server_sim_ms_of_the_current_run() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "get_time_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const readTime = () => getTime();
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let client_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(client_js_fn) = script_manager.get_client_js_fn("readTime#root/get_time_script") else {
            return Err(anyhow::anyhow!("Client function not found: readTime#root/get_time_script"));
        };
        client_js_fn.clone()
    };

    let first_result = callJsFn(&mut js_runtime, &mut world, 1234.5, Some(100.0), &client_js_fn, r#"{"json":[]}"#).await?;
    assert_eq!(helpers::remote_json_result::<f64>(&first_result)?, 1234.5);

    // A later run must see the newer wall clock, not a value cached from the first run.
    let second_result = callJsFn(&mut js_runtime, &mut world, 6789.0, Some(200.0), &client_js_fn, r#"{"json":[]}"#).await?;
    assert_eq!(helpers::remote_json_result::<f64>(&second_result)?, 6789.0);

    Ok(())
}

// getDelayedWorldTime() is getTime() minus the client world delay the play loop last set, and only clients have one.
#[tokio::test]
async fn get_delayed_world_time_is_get_time_minus_the_client_world_delay_and_client_only() -> anyhow::Result<()> {
    for is_client in [true, false] {
        let mut world = if is_client {
            make_world_controlled_by_player(PLAYER_OBJECT_ID)?
        } else {
            make_world()?
        };
        let root_object_id = world.getRootObjectId();
        let (directive, object_location) = if is_client {
            ("use client", ObjectLocation::Client)
        } else {
            ("use server", ObjectLocation::Server)
        };
        world.addObjectWithId(
            SCRIPT_OBJECT_ID,
            "get_time_script".to_string(),
            ObjectBeingAdded::Script(ScriptInfo {
                objectLocation: object_location,
            }),
            root_object_id,
        );
        world.markScriptAsReadyToRun(
            SCRIPT_OBJECT_ID,
            format!(
                r#"'{directive}'
export const readTimes = () => {{
    try {{ return [getTime(), getDelayedWorldTime()]; }} catch (error) {{ return error.message; }}
}};
"#
            ),
        );

        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, is_client.then_some(0.0), &script_manager_rc).await?;
        js_runtime.op_state().borrow_mut().borrow_mut::<JsRuntimeClientWorldDelay>().set(150.0);

        let read_times = {
            let script_manager = script_manager_rc.borrow();
            let read_times = if is_client {
                script_manager.get_client_js_fn("readTimes#root/get_time_script")
            } else {
                script_manager.get_server_js_fn("readTimes#root/get_time_script")
            };
            read_times.expect("Expected the registered function").clone()
        };
        let result = callJsFn(&mut js_runtime, &mut world, 1234.5, is_client.then_some(100.0), &read_times, r#"{"json":[]}"#).await?;
        let result = helpers::remote_json_result::<serde_json::Value>(&result)?;
        if is_client {
            assert_eq!(result, serde_json::json!([1234.5, 1084.5]));
        } else {
            assert!(
                result.as_str().is_some_and(|message| message.contains("only available on the client")),
                "{result}"
            );
        }
    }
    Ok(())
}

// A precise clock would let a script read this process's memory through CPU timing, so every clock scripts can read moves in
// steps of 2 ms. This also catches a deno_web upgrade that renames the op this replaces.
#[tokio::test]
async fn scripts_read_the_performance_clock_in_steps_of_2_ms() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "get_time_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const readClock = () => {
    const readings = new Set();
    const start = performance.now();
    while (readings.size < 3 && performance.now() - start < 50) {
        readings.add(performance.now());
        readings.add(performance.mark("clock").startTime);
    }
    // A short buffer must not panic the process.
    Deno.core.ops.op_now(new Uint8Array(1));
    return [...readings];
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let read_clock = {
        let script_manager = script_manager_rc.borrow();
        script_manager
            .get_client_js_fn("readClock#root/get_time_script")
            .expect("Expected the registered function")
            .clone()
    };
    let result = callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &read_clock, r#"{"json":[]}"#).await?;
    let readings = helpers::remote_json_result::<Vec<f64>>(&result)?;
    assert!(readings.len() >= 3, "{readings:?}");
    assert!(readings.iter().all(|reading| reading % 2.0 == 0.0), "{readings:?}");
    Ok(())
}
