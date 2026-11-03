mod helpers;

use game_types::object_types::ObjectId;
use helpers::world_test_helpers::{add_script, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use shared::game::world::JsCallbackInputs;

const SCRIPT_OBJECT_ID: ObjectId = 2;

fn game_chat_message(username: &str, message: &str, time_live_server_ms: u64) -> JsCallbackInputs {
    JsCallbackInputs::GameChatMessageSent {
        userid: 17,
        username: username.to_string(),
        message: message.to_string(),
        time_live_server_ms,
    }
}

#[tokio::test]
async fn game_chat_service_listeners_receive_every_message_in_the_order_it_was_sent() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx.clone());

    let root_object_id = world.getRootObjectId();
    add_script(&mut world, SCRIPT_OBJECT_ID, root_object_id)?;

    // The script checks each message itself and renames itself once it is satisfied, because an
    // object name drops the punctuation a chat message is full of. What a censored message looks
    // like is covered by the server's own chat tests, so the text here stays plain.
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"
const heard = [];
game.gameChatService.addGameChatListener((gameChatMessage) => {
  heard.push(gameChatMessage);
  if (heard.length === 2) {
    const [first, second] = heard;
    if (first.username !== "Alice" || first.userid !== 17 || first.message !== "hello" || first.time_live_server_ms !== 1000) {
      throw new Error("first chat message did not arrive intact: " + JSON.stringify(first));
    }
    if (second.username !== "Bob" || second.message !== "good game" || second.time_live_server_ms !== 1001) {
      throw new Error("second chat message did not arrive intact: " + JSON.stringify(second));
    }
    script.name = "heardBothMessagesInOrder";
  }
  if (heard.length === 3 && heard[2].message === "still here") {
    script.name = "heardTheLaterMessageToo";
  }
});
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // Both land in the same batch. A channel that kept only the newest value would lose the first,
    // which is the whole reason chat messages are queued one at a time.
    js_callback_inputs_tx.send(game_chat_message("Alice", "hello", 1000))?;
    js_callback_inputs_tx.send(game_chat_message("Bob", "good game", 1001))?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    assert_eq!(world.getObject(SCRIPT_OBJECT_ID)?.name, "heardBothMessagesInOrder");

    // A message sent in a later tick reaches the same listener.
    js_callback_inputs_tx.send(game_chat_message("Carol", "still here", 1002))?;
    runPendingJS(&mut js_runtime, &mut world, 32.0, Some(32.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    assert_eq!(world.getObject(SCRIPT_OBJECT_ID)?.name, "heardTheLaterMessageToo");

    Ok(())
}

// Removing a listener is the half of the service nothing covered. A script that listens for a while
// and then stops has to actually stop, and taking one listener off must leave the others alone.
#[tokio::test]
async fn a_removed_listener_stops_hearing_while_the_others_carry_on() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx.clone());

    let root_object_id = world.getRootObjectId();
    add_script(&mut world, SCRIPT_OBJECT_ID, root_object_id)?;

    // The script keeps its own tallies and writes them into its name, which is the only way this
    // test can read back what happened inside the runtime.
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"
let leavingHeard = 0;
let stayingHeard = 0;
const leaving = () => { leavingHeard += 1; };
const staying = () => { stayingHeard += 1; };
game.gameChatService.addGameChatListener(leaving);
game.gameChatService.addGameChatListener(staying);
// Removing something that was never added must not take one of these two off with it.
game.gameChatService.removeGameChatListener(() => {});
game.gameChatService.addGameChatListener((gameChatMessage) => {
  if (gameChatMessage.message === "stop listening") {
    game.gameChatService.removeGameChatListener(leaving);
  }
  script.name = "leaving" + leavingHeard + "staying" + stayingHeard;
});
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // Both listeners hear this one, and the name is written after they have.
    js_callback_inputs_tx.send(game_chat_message("Alice", "hello", 1000))?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    assert_eq!(world.getObject(SCRIPT_OBJECT_ID)?.name, "leaving1staying1");

    // This message takes one listener off, after every listener has already been called for it.
    js_callback_inputs_tx.send(game_chat_message("Bob", "stop listening", 1001))?;
    runPendingJS(&mut js_runtime, &mut world, 32.0, Some(32.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    assert_eq!(world.getObject(SCRIPT_OBJECT_ID)?.name, "leaving2staying2");

    // Only the one that stayed hears anything from here on.
    js_callback_inputs_tx.send(game_chat_message("Carol", "still here", 1002))?;
    runPendingJS(&mut js_runtime, &mut world, 48.0, Some(48.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    assert_eq!(world.getObject(SCRIPT_OBJECT_ID)?.name, "leaving2staying3");

    Ok(())
}
