mod helpers;

use game_types::object_delta::is_valid_player_metadata;
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, Orientation3D, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_brick, add_player, make_world, make_world_controlled_by_player};
use shared::game::jsRuntime::{JsConsoleLogEntry, JsRuntimeInitialization, JsRuntimePlayerPings, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::world::{World, WorldControlMode};
use std::collections::HashMap;
use std::sync::{Arc, mpsc};

const PLAYER_OBJECT_ID: ObjectId = 90;
const CHARACTER_OBJECT_ID: ObjectId = 91;
const SCRIPT_OBJECT_ID: ObjectId = 92;
const NEW_CHARACTER_OBJECT_ID: ObjectId = 93;
const JOINING_PLAYER_OBJECT_ID: ObjectId = 94;

fn make_client_world() -> anyhow::Result<World> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::ZERO, true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    Ok(world)
}

// Runs the script once, the way a world starts it, and returns the report it logged plus any errors.
async fn run_script(world: &mut World, object_location: ObjectLocation, raw_code: &str) -> anyhow::Result<(serde_json::Value, Vec<String>)> {
    run_script_with_player_pings(world, object_location, raw_code, HashMap::new()).await
}

// The same, on a client that has these pings from the server.
async fn run_script_with_player_pings(
    world: &mut World,
    object_location: ObjectLocation,
    raw_code: &str,
    ping_ms_of_player_object_id: HashMap<ObjectId, u16>,
) -> anyhow::Result<(serde_json::Value, Vec<String>)> {
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: object_location,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(SCRIPT_OBJECT_ID, raw_code.to_string());
    let (console_log_entry_tx, console_log_entry_rx) = mpsc::channel::<JsConsoleLogEntry>();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    js_runtime
        .op_state()
        .borrow_mut()
        .borrow_mut::<JsRuntimePlayerPings>()
        .set(ping_ms_of_player_object_id);
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await?;

    let mut report = serde_json::Value::Null;
    let mut errors = Vec::new();
    for console_log_entry in console_log_entry_rx.try_iter() {
        if console_log_entry.log_level == "error" {
            errors.push(console_log_entry.message);
        } else if let Some(report_json) = console_log_entry.message.strip_prefix("report:") {
            report = serde_json::from_str(report_json)?;
        }
    }
    Ok((report, errors))
}

#[tokio::test]
async fn a_client_shares_metadata_and_reads_it_back() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let (report, errors) = run_script(
        &mut world,
        ObjectLocation::Client,
        r#"
const me = game.playerService.getLocalPlayerObjectId();
const before = game.playerService.getPlayerMetadata(me);
game.playerService.setLocalPlayerMetadata({ animation: "walk", look: { x: 0, y: 0, z: 1 } });
game.playerService.setLocalPlayerMetadata({ animation: "jump", speed: 1.23456 });
console.log("report:" + JSON.stringify({ before, after: game.playerService.getPlayerMetadata(me) }));
"#,
    )
    .await?;

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(report, serde_json::json!({ "before": null, "after": { "animation": "jump", "speed": 1.235 } }));
    assert_eq!(world.getPlayerMetadata(PLAYER_OBJECT_ID), Some(r#"{"animation":"jump","speed":1.235}"#));
    Ok(())
}

#[tokio::test]
async fn only_the_client_shares_metadata_and_only_up_to_the_size_limit() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let (report, _) = run_script(
        &mut world,
        ObjectLocation::Client,
        r#"
let tooBig = null;
try { game.playerService.setLocalPlayerMetadata("x".repeat(300)); } catch (error) { tooBig = String(error); }
console.log("report:" + JSON.stringify({ tooBig }));
"#,
    )
    .await?;
    assert!(report["tooBig"].as_str().unwrap_or_default().contains("over the 256 byte limit"), "{report}");
    assert_eq!(world.getPlayerMetadata(PLAYER_OBJECT_ID), None);

    let mut world = make_world()?;
    let (report, _) = run_script(
        &mut world,
        ObjectLocation::Server,
        r#"
let onServer = null;
try { game.playerService.setLocalPlayerMetadata(1); } catch (error) { onServer = String(error); }
console.log("report:" + JSON.stringify({ onServer, nobody: game.playerService.getPlayerMetadata(12345) }));
"#,
    )
    .await?;
    assert!(
        report["onServer"].as_str().unwrap_or_default().contains("only available on the client"),
        "{report}"
    );
    assert_eq!(report["nobody"], serde_json::Value::Null);
    Ok(())
}

#[tokio::test]
async fn server_scripts_read_what_a_player_shared() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.setPlayerMetadata(PLAYER_OBJECT_ID, Some(r#"{"animation":"walk"}"#.to_string()));
    let (report, errors) = run_script(
        &mut world,
        ObjectLocation::Server,
        &format!(r#"console.log("report:" + JSON.stringify(game.playerService.getPlayerMetadata({PLAYER_OBJECT_ID})));"#),
    )
    .await?;
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(report, serde_json::json!({ "animation": "walk" }));
    Ok(())
}

#[tokio::test]
async fn scripts_read_a_players_camera_direction() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.setPlayerCameraOrientation(
        PLAYER_OBJECT_ID,
        Orientation3D {
            theta: std::f32::consts::FRAC_PI_2,
            phi: 0.0,
        },
    );
    let (report, errors) = run_script(
        &mut world,
        ObjectLocation::Server,
        &format!(
            r#"console.log("report:" + JSON.stringify({{ looking: game.playerService.getPlayerCameraDirection({PLAYER_OBJECT_ID}), nobody: game.playerService.getPlayerCameraDirection(12345) }}));"#
        ),
    )
    .await?;
    assert!(errors.is_empty(), "{errors:?}");
    let component = |axis: &str| report["looking"][axis].as_f64().unwrap_or(f64::NAN);
    assert!(
        (component("x") - 1.0).abs() < 1e-5 && component("y").abs() < 1e-5 && component("z").abs() < 1e-5,
        "a camera turned a quarter turn looks along +X: {report}"
    );
    assert_eq!(report["nobody"], serde_json::Value::Null);
    Ok(())
}

// Every client's camera starts looking the same way, so a player has a camera direction from the moment they exist.
// That includes the other players a joining client loads with its world.
#[tokio::test]
async fn a_player_looks_where_every_camera_starts_until_their_first_input() -> anyhow::Result<()> {
    let mut server_world = make_world()?;
    let root_object_id = server_world.getRootObjectId();
    add_player(&mut server_world, PLAYER_OBJECT_ID, root_object_id, None)?;
    add_player(&mut server_world, JOINING_PLAYER_OBJECT_ID, root_object_id, None)?;
    let mut joining_client_world =
        World::fromStoredWorldString(&server_world.toStoredWorldString()?, WorldControlMode::Player(JOINING_PLAYER_OBJECT_ID), None)?;
    for (world, object_location) in [(&mut server_world, ObjectLocation::Server), (&mut joining_client_world, ObjectLocation::Client)] {
        let (report, errors) = run_script(
            world,
            object_location,
            &format!(r#"console.log("report:" + JSON.stringify(game.playerService.getPlayerCameraDirection({PLAYER_OBJECT_ID})));"#),
        )
        .await?;
        assert!(errors.is_empty(), "{errors:?}");
        let component = |axis: &str| report[axis].as_f64().unwrap_or(f64::NAN);
        assert!(
            component("x").abs() < 1e-5 && component("y").abs() < 1e-5 && (component("z") - 1.0).abs() < 1e-5,
            "a camera that hasn't turned looks along +Z: {report}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn scripts_read_a_players_ping() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let (report, errors) = run_script_with_player_pings(
        &mut world,
        ObjectLocation::Client,
        &format!(
            r#"console.log("report:" + JSON.stringify({{ ping: game.playerService.getPlayerPing({PLAYER_OBJECT_ID}), nobody: game.playerService.getPlayerPing(12345) }}));"#
        ),
        HashMap::from([(PLAYER_OBJECT_ID, 42)]),
    )
    .await?;
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(report, serde_json::json!({ "ping": 42, "nobody": null }));
    Ok(())
}

#[test]
fn shared_metadata_must_be_small_json() {
    assert!(is_valid_player_metadata(r#"{"animation":"walk"}"#));
    assert!(!is_valid_player_metadata(&format!(r#""{}""#, "x".repeat(300))));
    assert!(!is_valid_player_metadata("{animation"));
}

#[test]
fn metadata_stays_with_the_player_when_the_character_changes() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    world.setLocalPlayerMetadataJson(r#""wave""#.to_string())?;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, NEW_CHARACTER_OBJECT_ID, root_object_id, Vec3::ZERO, true)?;
    let ObjectInfo::Player(mut player_info) = world.getObject(PLAYER_OBJECT_ID)?.info.clone() else {
        anyhow::bail!("the player object should be a player");
    };
    player_info.characterObjectId = Some(ObjectIdPointer(NEW_CHARACTER_OBJECT_ID));
    world.setPlayerInfo(PLAYER_OBJECT_ID, player_info.clone())?;
    assert_eq!(world.getPlayerMetadata(PLAYER_OBJECT_ID), Some(r#""wave""#));

    player_info.characterObjectId = None;
    world.setPlayerInfo(PLAYER_OBJECT_ID, player_info)?;
    world.setLocalPlayerMetadataJson(r#""bow""#.to_string())?;
    assert_eq!(world.getPlayerMetadata(PLAYER_OBJECT_ID), Some(r#""bow""#));
    Ok(())
}
