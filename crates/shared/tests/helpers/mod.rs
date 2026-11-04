pub mod asset_test_helpers;
pub mod world_test_helpers;

use deno_core::JsRuntime;
use game_types::object_types::ObjectId;
use shared::game::jsRuntime::{ScriptManager, pollServerFnCall, runPendingJS, startRegisteredServerFn};
use shared::game::world::World;
use std::{cell::RefCell, rc::Rc, task::Poll, time::Duration};

// Tests of JSON-only results read SuperJSON's payload; richer values are checked in the JS runtime.
#[allow(dead_code)]
pub fn remote_json_result<T: serde::de::DeserializeOwned>(encoded: &str) -> anyhow::Result<T> {
    let result: serde_json::Value = serde_json::from_str(encoded)?;
    Ok(T::deserialize(&result["json"])?)
}

// Runs a 'use server' call the way the server's tick does: it starts, then the event loop runs until the call settles.
#[allow(dead_code)]
pub async fn call_server_fn(
    js_runtime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    server_fn_caller_object_id: ObjectId,
    fn_id: &str,
    js_args: &str,
) -> anyhow::Result<String> {
    let mut server_fn_call = startRegisteredServerFn(
        js_runtime,
        world,
        est_live_server_sim_ms,
        script_manager_rc,
        server_fn_caller_object_id,
        fn_id,
        js_args,
    )?;
    let (_js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    loop {
        runPendingJS(js_runtime, world, est_live_server_sim_ms, None, script_manager_rc, &mut js_callback_inputs_rx).await?;
        if let Poll::Ready(result) = pollServerFnCall(js_runtime, &mut server_fn_call) {
            return result;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}
