mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_brick, make_world, make_world_controlled_by_player};
use shared::game::jsRuntime::{
    JsRuntimeInitialization, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime, pollServerFnCall, runPendingJS, startRegisteredServerFn,
};
use shared::game::world::World;
use std::{task::Poll, time::Duration};

fn add_named_script(world: &mut World, object_id: ObjectId, parent_id: ObjectId, name: &str, object_location: ObjectLocation, raw_code: &str) {
    world.addObjectWithId(
        object_id,
        name.to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: object_location,
        }),
        parent_id,
    );
    world.markScriptAsReadyToRun(object_id, raw_code.to_string());
}

// Round-trip real registered handlers so SuperJSON handles nested values and live proxies in both directions.
#[tokio::test]
async fn remote_calls_preserve_javascript_values_and_live_object_data() -> anyhow::Result<()> {
    for is_client in [false, true] {
        let mut world = if is_client { make_world_controlled_by_player(99)? } else { make_world()? };
        let root_id = world.getRootObjectId();
        add_brick(&mut world, 3, root_id, Vec3::new(1.0, 2.0, 3.0), false)?;
        let (directive, location) = if is_client {
            ("use client", ObjectLocation::Client)
        } else {
            ("use server", ObjectLocation::Server)
        };
        add_named_script(
            &mut world,
            2,
            root_id,
            "remote_results",
            location,
            &format!(
                r#"'{directive}'
export const makeArgs = () => {{
    const nested = {{ missing: undefined, empty: null, values: [undefined, null, false, 0, ""] }};
    nested.self = nested;
    const brick = game.objectService.getObjectById(3);
    return [undefined, null, nested, nested, brick, brick.location.position];
}};
export const echo = (...args) => args;
export const checkArgs = function (value = "default", nullable, nested, repeated, brick, position) {{
    if (arguments.length !== 6 || value !== "default" || nullable !== null) throw new Error("Argument count or default changed");
    if (!Object.hasOwn(nested, "missing") || nested.missing !== undefined || nested.empty !== null) throw new Error("Object fields changed");
    if (nested.values.length !== 5 || nested.values[0] !== undefined || nested.values[1] !== null || nested.values[2] !== false || nested.values[3] !== 0 || nested.values[4] !== "") throw new Error("Array values changed");
    if (nested.self !== nested || repeated !== nested) throw new Error("Object references changed");
    if (brick.id !== 3 || brick.location.position.y !== 2 || position.x !== 1 || position.y !== 2 || position.z !== 3) throw new Error("Live proxy data changed");
    if (Object.hasOwn(brick, "parent") || Object.hasOwn(brick, "getChild")) throw new Error("Non-enumerable helpers were serialized");
    return true;
}};
export const defaults = function (value = "default") {{ return [arguments.length, value]; }};
export const noReturn = () => {{}};
export const nullReturn = () => null;
export const makeMissingArg = () => [undefined];
export const first = (value) => value;
"#
            ),
        );
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        let mut call = async |export_name: &str, args: &str| -> anyhow::Result<String> {
            let fn_id = format!("{export_name}#root/remote_results");
            let function = {
                let script_manager = script_manager_rc.borrow();
                let function = if is_client {
                    script_manager.get_client_js_fn(&fn_id)
                } else {
                    script_manager.get_server_js_fn(&fn_id)
                };
                function.expect("Expected registered remote function").clone()
            };
            callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &function, args).await
        };
        let args = call("makeArgs", r#"{"json":[]}"#).await?;
        let echoed_args = call("echo", &args).await?;
        assert!(helpers::remote_json_result::<bool>(&call("checkArgs", &echoed_args).await?)?);
        let omitted = call("defaults", r#"{"json":[]}"#).await?;
        assert_eq!(helpers::remote_json_result::<serde_json::Value>(&omitted)?, serde_json::json!([0, "default"]));
        let missing_arg = call("makeMissingArg", r#"{"json":[]}"#).await?;
        let explicit_undefined = call("defaults", &missing_arg).await?;
        assert_eq!(
            helpers::remote_json_result::<serde_json::Value>(&explicit_undefined)?,
            serde_json::json!([1, "default"])
        );
        let explicit_null = call("defaults", r#"{"json":[null]}"#).await?;
        assert_eq!(helpers::remote_json_result::<serde_json::Value>(&explicit_null)?, serde_json::json!([1, null]));
        let no_return = call("noReturn", r#"{"json":[]}"#).await?;
        let null_return = call("nullReturn", r#"{"json":[]}"#).await?;
        assert_ne!(no_return, null_return);
        assert_eq!(call("first", &missing_arg).await?, no_return);
        assert_eq!(call("first", r#"{"json":[null]}"#).await?, null_return);
    }
    Ok(())
}

// The server starts a call and moves on, so a call that awaits finishes on a later tick. Two players' calls wait at once, and
// each saw its own caller when it started.
#[tokio::test]
async fn server_functions_that_await_finish_on_later_ticks() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_named_script(
        &mut world,
        2,
        root_object_id,
        "callers",
        ObjectLocation::Server,
        r#"'use server'
export const whoCalled = async () => {
    const caller = game.playerService.getServerFnCaller();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 1));
    return caller;
};
export const failAfterTick = async () => {
    await tick();
    throw new Error("failed after a tick");
};
"#,
    );
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;

    let mut server_fn_calls = Vec::new();
    for caller in [7, 8] {
        let mut server_fn_call = startRegisteredServerFn(
            &mut js_runtime,
            &mut world,
            0.0,
            &script_manager_rc,
            caller,
            "whoCalled#root/callers",
            r#"{"json":[]}"#,
        )?;
        assert!(pollServerFnCall(&mut js_runtime, &mut server_fn_call).is_pending());
        server_fn_calls.push((caller, server_fn_call));
    }
    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !server_fn_calls.is_empty() {
            runPendingJS(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc, &mut js_callback_inputs_rx).await?;
            server_fn_calls.retain_mut(|(caller, server_fn_call)| {
                let Poll::Ready(result) = pollServerFnCall(&mut js_runtime, server_fn_call) else {
                    return true;
                };
                assert_eq!(helpers::remote_json_result::<ObjectId>(&result.unwrap()).unwrap(), *caller);
                false
            });
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        anyhow::Ok(())
    })
    .await
    .expect("both calls should finish")?;

    let error = helpers::call_server_fn(
        &mut js_runtime,
        &mut world,
        0.0,
        &script_manager_rc,
        7,
        "failAfterTick#root/callers",
        r#"{"json":[]}"#,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("failed after a tick"), "{error}");
    Ok(())
}

#[tokio::test]
async fn top_level_use_server_modules_can_still_call_their_real_function_body_on_server() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "server_actions",
        ObjectLocation::Server,
        "'use server'\n\
         export const inner_fn = () => {\n\
             return 20;\n\
         };\n\
         game.world.name = `server_action_${await inner_fn()}`;",
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "server_action_20");

    Ok(())
}

#[tokio::test]
async fn server_runtime_executes_server_scripts_without_running_client_scripts() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "client_only_startup",
        ObjectLocation::Client,
        "throw new Error('client script ran on server');",
    );
    add_named_script(
        &mut world,
        3,
        root_object_id,
        "server_only_startup",
        ObjectLocation::Server,
        "game.world.name = 'server_script_ran';",
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "server_script_ran");

    Ok(())
}

#[tokio::test]
async fn player_runtime_executes_only_client_scripts_and_their_changes_stay_on_the_client() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();

    add_named_script(
        &mut world,
        2,
        root_object_id,
        "server_only_startup",
        ObjectLocation::Server,
        "throw new Error('server script ran on client');",
    );
    add_named_script(
        &mut world,
        3,
        root_object_id,
        "client_only_startup",
        ObjectLocation::Client,
        "game.world.name = 'client_script_ran';",
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let root_object = world.getObject(root_object_id)?;
    assert_eq!(root_object.name, "client_script_ran");
    assert!(world.flushDirtyObjects().modifiedObjectFlags.is_empty(), "a client marks nothing to send");

    Ok(())
}
