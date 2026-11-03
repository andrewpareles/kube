mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::{add_player, add_script, make_world, make_world_controlled_by_player};
use shared::game::jsRuntime::{JsConsoleLogCallback, JsConsoleLogEntry, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};
use std::sync::{Arc, Mutex};

async fn run_world_scripts(world: &mut shared::game::world::World) -> anyhow::Result<()> {
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await
}

#[tokio::test]
async fn player_service_get_all_players_returns_live_player_objects() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let player_object_id_1 = 3;
    let player_object_id_2 = 4;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_player(&mut world, player_object_id_1, root_object_id, None)?;
    add_player(&mut world, player_object_id_2, root_object_id, None)?;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const players = game.playerService.getAllPlayers();
if (players.length !== 2) {{
    throw new Error("Expected 2 players, got " + players.length);
}}

const playerUserids = players.map((player) => player.userid).sort().join(",");
if (playerUserids !== "{0},{1}") {{
    throw new Error("Unexpected player userids: " + playerUserids);
}}

const player1 = players.find((player) => player.id === {0});
if (player1 === undefined) {{
    throw new Error("Expected player {0}");
}}
if (player1.type !== "player") {{
    throw new Error("Expected player type, got " + player1.type);
}}

const parent = player1.parent;
if (parent === null || parent.id !== game.world.id) {{
    throw new Error("Expected player parent helper to point at the world root");
}}

player1.name = "player-{0}-renamed";
"#,
            player_object_id_1, player_object_id_2
        ),
    );

    run_world_scripts(&mut world).await?;

    let player_object = world.getObject(player_object_id_1)?;
    assert_eq!(player_object.name, format!("player-{}-renamed", player_object_id_1));

    Ok(())
}

// The bug this exists to prevent: a client script that picks its player out of
// getObjectsByType("player") picks the same one on every client, so several players all act as
// whoever happens to come first. The local player has to come from the runtime, not from a guess.
#[tokio::test]
async fn player_service_get_local_player_object_id_returns_this_client_s_own_player() -> anyhow::Result<()> {
    let script_object_id = 2;
    let other_player_object_id = 3;
    let local_player_object_id = 4;

    // The local player is deliberately not the first player in the world, so a script that guessed
    // by taking the first one would fail this test.
    let mut world = make_world_controlled_by_player(local_player_object_id)?;
    let root_object_id = world.getRootObjectId();
    add_player(&mut world, other_player_object_id, root_object_id, None)?;
    add_player(&mut world, local_player_object_id, root_object_id, None)?;

    // A client script, because only a client has a local player to ask about. A client cannot write
    // object fields, so the script reports what it found through the console instead.
    world.addObjectWithId(
        script_object_id,
        "local_player_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );

    world.markScriptAsReadyToRun(
        script_object_id,
        r#"'use client'
const localPlayerObjectId = game.playerService.getLocalPlayerObjectId();
const localPlayer = game.objectService.getObjectById(localPlayerObjectId);
console.log("found:" + localPlayerObjectId + ":" + (localPlayer === null ? "none" : localPlayer.type + ":" + localPlayer.userid));
"#
        .to_string(),
    );

    let console_log_entries: Arc<Mutex<Vec<JsConsoleLogEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let console_log_entries_for_callback = console_log_entries.clone();
    let console_log_callback: JsConsoleLogCallback = Arc::new(move |entry: JsConsoleLogEntry| {
        console_log_entries_for_callback.lock().unwrap().push(entry);
    });

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, Some(console_log_callback)).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let console_log_entries = console_log_entries.lock().unwrap().clone();
    let messages = console_log_entries.iter().map(|entry| entry.message.trim()).collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| *message == format!("found:{0}:player:{0}", local_player_object_id)),
        "expected the client to identify player {} as its own, got: {:?}",
        local_player_object_id,
        messages
    );

    Ok(())
}

#[tokio::test]
async fn player_service_get_local_player_object_id_throws_on_the_server() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let player_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_player(&mut world, player_object_id, root_object_id, None)?;

    // There is no local player on the server, so asking is a mistake in the script. It has to fail
    // loudly - a null here would let server code branch on it forever and never notice.
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
try {
    const localPlayerObjectId = game.playerService.getLocalPlayerObjectId();
    console.log("no-throw:" + localPlayerObjectId);
} catch (error) {
    console.log("threw:" + error.message);
}
"#
        .to_string(),
    );

    let console_log_entries: Arc<Mutex<Vec<JsConsoleLogEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let console_log_entries_for_callback = console_log_entries.clone();
    let console_log_callback: JsConsoleLogCallback = Arc::new(move |entry: JsConsoleLogEntry| {
        console_log_entries_for_callback.lock().unwrap().push(entry);
    });

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, Some(console_log_callback)).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let console_log_entries = console_log_entries.lock().unwrap().clone();
    let messages = console_log_entries.iter().map(|entry| entry.message.trim()).collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("threw:") && message.contains("only available on the client")),
        "expected getLocalPlayerObjectId to throw on the server, got: {:?}",
        messages
    );

    Ok(())
}
