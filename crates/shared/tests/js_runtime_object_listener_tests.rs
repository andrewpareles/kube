mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_ball, add_local_group, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};

const SCRIPT_OBJECT_ID: ObjectId = 2;
const GROUP_OBJECT_ID: ObjectId = 10;
const BALL_OBJECT_ID: ObjectId = 11;

#[tokio::test]
async fn object_listeners_fire_with_and_without_type_filters() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx);

    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "object_listener_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use server'
const events = [];
game.objectService.addObjectCreatedListener((object) => {
    events.push(`created_any_${object.type}_${object.id}`);
});
game.objectService.addObjectCreatedListener((object) => {
    events.push(`created_ball_${object.id}`);
}, "ball");
game.objectService.addObjectDeletedListener((objectId, objectType) => {
    events.push(`deleted_any_${objectType}_${objectId}`);
});
game.objectService.addObjectDeletedListener((objectId) => {
    events.push(`deleted_ball_${objectId}`);
}, "ball");
export const getEvents = () => events;
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;

    add_local_group(&mut world, GROUP_OBJECT_ID, root_object_id)?;
    add_ball(&mut world, BALL_OBJECT_ID, GROUP_OBJECT_ID, Vec3::new(0.0, 5.0, 0.0), false)?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, None, &script_manager_rc, &mut js_callback_inputs_rx).await?;

    // deleting the group also deletes its ball child; both fire deleted events (parent first)
    world.removeObject(GROUP_OBJECT_ID)?;
    runPendingJS(&mut js_runtime, &mut world, 32.0, None, &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let server_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(server_js_fn) = script_manager.get_server_js_fn("getEvents#root/object_listener_script") else {
            return Err(anyhow::anyhow!("Server function not found: getEvents#root/object_listener_script"));
        };
        server_js_fn.clone()
    };
    let events_json = callJsFn(&mut js_runtime, &mut world, 48.0, None, &server_js_fn, r#"{"json":[]}"#).await?;
    let events: Vec<String> = helpers::remote_json_result(&events_json)?;

    // type-filtered listeners fire before unfiltered ones
    assert_eq!(
        events,
        vec![
            // the script object itself was created after the world loaded, so the unfiltered listener hears about it
            format!("created_any_script_{SCRIPT_OBJECT_ID}"),
            format!("created_any_group_{GROUP_OBJECT_ID}"),
            format!("created_ball_{BALL_OBJECT_ID}"),
            format!("created_any_ball_{BALL_OBJECT_ID}"),
            format!("deleted_any_group_{GROUP_OBJECT_ID}"),
            format!("deleted_ball_{BALL_OBJECT_ID}"),
            format!("deleted_any_ball_{BALL_OBJECT_ID}"),
        ]
    );

    Ok(())
}
