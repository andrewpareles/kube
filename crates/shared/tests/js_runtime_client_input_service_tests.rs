mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use shared::game::jsRuntime::{JsRuntimeInitialization, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use shared::game::world::JsCallbackInputs;

const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;

#[tokio::test]
async fn client_input_service_fires_listeners_and_tracks_held_state_on_the_client() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx.clone());

    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "input_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
const events = [];
game.clientInputService.addKeyDownListener((key) => {
    events.push(`keydown_${key}_held_${game.clientInputService.isKeyDown(key)}`);
});
game.clientInputService.addKeyUpListener((key) => {
    events.push(`keyup_${key}_held_${game.clientInputService.isKeyDown(key)}`);
});
game.clientInputService.addMouseDownListener((button) => {
    events.push(`mousedown_${button}_held_${game.clientInputService.isMouseButtonDown(button)}`);
});
game.clientInputService.addMouseUpListener((button) => {
    events.push(`mouseup_${button}_held_${game.clientInputService.isMouseButtonDown(button)}`);
});
export const getEvents = () => events;
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // These simulate what the client play window pushes: presses, then releases.
    js_callback_inputs_tx.send(JsCallbackInputs::KeyDown { key: "E".to_string() })?;
    js_callback_inputs_tx.send(JsCallbackInputs::MouseDown { button: "left".to_string() })?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    js_callback_inputs_tx.send(JsCallbackInputs::KeyUp { key: "E".to_string() })?;
    js_callback_inputs_tx.send(JsCallbackInputs::MouseUp { button: "left".to_string() })?;
    runPendingJS(&mut js_runtime, &mut world, 32.0, Some(32.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let client_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(client_js_fn) = script_manager.get_client_js_fn("getEvents#root/input_script") else {
            return Err(anyhow::anyhow!("Client function not found: getEvents#root/input_script"));
        };
        client_js_fn.clone()
    };
    let events_json = callJsFn(&mut js_runtime, &mut world, 48.0, Some(48.0), &client_js_fn, r#"{"json":[]}"#).await?;
    let events: Vec<String> = helpers::remote_json_result(&events_json)?;

    // held state must already reflect the event inside its own listener
    assert_eq!(
        events,
        vec![
            "keydown_E_held_true".to_string(),
            "mousedown_left_held_true".to_string(),
            "keyup_E_held_false".to_string(),
            "mouseup_left_held_false".to_string(),
        ]
    );

    Ok(())
}
