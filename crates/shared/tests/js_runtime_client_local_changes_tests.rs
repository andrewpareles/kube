mod helpers;

use deno_core::JsRuntime;
use game_types::object_delta::ObjectAddDelta;
use game_types::object_types::{
    ObjectBeingAdded, ObjectId, ObjectInfo, ObjectKind, ObjectLocation, ScriptInfo,
    shared::{TOP_CLIENT_OBJECT_ID, is_client_object_id},
};
use glam::Vec3;
use helpers::world_test_helpers::{add_ball, add_brick, add_player, make_world_controlled_by_player};
use shared::game::jsRuntime::{JsConsoleLogEntry, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::world::World;
use std::sync::Arc;
use std::sync::mpsc;

const PLAYER_OBJECT_ID: ObjectId = 99;
const CHARACTER_OBJECT_ID: ObjectId = 2;
const SCRIPT_OBJECT_ID: ObjectId = 3;
const BRICK_OBJECT_ID: ObjectId = 4;

// A client world where the local player's character is a ball, next to a brick the server made.
fn make_client_world() -> anyhow::Result<World> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 10.0, 0.0), true)?;
    add_brick(&mut world, BRICK_OBJECT_ID, root_object_id, Vec3::ZERO, false)?;
    world.flushDirtyObjects();
    Ok(world)
}

struct RunningScript {
    _js_runtime: JsRuntime,
    console_log_entry_rx: mpsc::Receiver<JsConsoleLogEntry>,
}

async fn run_client_script(world: &mut World, raw_code: &str) -> anyhow::Result<RunningScript> {
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "client_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(SCRIPT_OBJECT_ID, raw_code.to_string());

    let (console_log_entry_tx, console_log_entry_rx) = mpsc::channel();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await?;
    Ok(RunningScript {
        _js_runtime: js_runtime,
        console_log_entry_rx,
    })
}

fn take_console_errors(running_script: &RunningScript) -> Vec<String> {
    running_script
        .console_log_entry_rx
        .try_iter()
        .filter(|console_log_entry| console_log_entry.log_level == "error")
        .map(|console_log_entry| console_log_entry.message)
        .collect()
}

// A Client Script can change any object, but only in its own world: nothing is marked to be sent anywhere.
#[tokio::test]
async fn a_client_script_changes_objects_only_in_its_own_world() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let running_script = run_client_script(
        &mut world,
        &format!(
            r#"
const brick = game.objectService.getObjectById({BRICK_OBJECT_ID});
brick.opacity = 0.25;
game.objectService.moveObjectTo({BRICK_OBJECT_ID}, {{ x: 1, y: 2, z: 3 }});
game.objectService.setObjectPushable({BRICK_OBJECT_ID}, true);
"#
        ),
    )
    .await?;
    assert_eq!(take_console_errors(&running_script), Vec::<String>::new());

    let ObjectInfo::Brick(brick_info) = &world.getObject(BRICK_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("Expected the brick"));
    };
    assert_eq!(brick_info.visualSettings.opacity, 0.25);
    let Some(physics_info_3d) = world.getObjectPhysicsInfo3D(BRICK_OBJECT_ID) else {
        return Err(anyhow::anyhow!("Expected the brick's physics"));
    };
    assert!((physics_info_3d.location.position - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
    let Some(physics_toggles_3d) = world.getObjectPhysicsToggles3D(BRICK_OBJECT_ID) else {
        return Err(anyhow::anyhow!("Expected the brick's physics toggles"));
    };
    assert!(physics_toggles_3d.isPushable);

    let dirty = world.flushDirtyObjects();
    assert!(!dirty.modifiedObjectFlags.contains_key(&BRICK_OBJECT_ID), "a client marks nothing to send");
    Ok(())
}

// Which character or head a player has belongs to the server.
#[tokio::test]
async fn a_client_script_cannot_change_which_character_a_player_has() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let running_script = run_client_script(
        &mut world,
        &format!(
            r#"
try {{
    game.objectService.getObjectById({PLAYER_OBJECT_ID}).characterObjectId = {BRICK_OBJECT_ID};
}} catch (error) {{
    console.error(String(error));
}}
"#
        ),
    )
    .await?;

    let console_errors = take_console_errors(&running_script);
    assert!(
        console_errors
            .iter()
            .any(|console_error| console_error.contains("Only the server can change a player's character or head")),
        "{console_errors:?}"
    );
    assert_eq!(world.getPlayerCharacterObject(PLAYER_OBJECT_ID), Some(CHARACTER_OBJECT_ID));
    Ok(())
}

fn only_object_named(world: &World, name: &str) -> anyhow::Result<ObjectId> {
    match world.getObjectIdsByName(name).as_slice() {
        [object_id] => Ok(*object_id),
        object_ids => Err(anyhow::anyhow!("Expected one object named {name}, found {object_ids:?}")),
    }
}

fn brick_opacity(world: &World) -> anyhow::Result<f32> {
    let ObjectInfo::Brick(brick_info) = &world.getObject(BRICK_OBJECT_ID)?.info else {
        return Err(anyhow::anyhow!("Expected the brick"));
    };
    Ok(brick_info.visualSettings.opacity)
}

// A Client Script's own objects get ids counting down from the top of the range, so they never take one the server hands
// out. The client simulates them like its character, dynamic when pushable, and marks nothing to send.
#[tokio::test]
async fn a_client_script_makes_objects_with_its_own_ids_and_simulates_them() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let root_object_id = world.getRootObjectId();
    let running_script = run_client_script(
        &mut world,
        &format!(
            r#"
const copy = game.objectService.cloneObject({BRICK_OBJECT_ID}, "localCopy");
game.objectService.setObjectPushable(copy.id, true);
game.objectService.createObject("localFolder", {{ type: "group" }}, {root_object_id});
"#
        ),
    )
    .await?;
    assert_eq!(take_console_errors(&running_script), Vec::<String>::new());

    let copy_object_id = only_object_named(&world, "localCopy")?;
    let folder_object_id = only_object_named(&world, "localFolder")?;
    assert_eq!(copy_object_id, TOP_CLIENT_OBJECT_ID);
    assert_eq!(folder_object_id, TOP_CLIENT_OBJECT_ID - 1);
    assert!(
        world.physics.canSetObjectVelocity(copy_object_id),
        "the client simulates its own pushable brick"
    );
    assert!(!world.physics.canSetObjectVelocity(BRICK_OBJECT_ID), "the server's brick stays the server's");
    assert!(world.flushDirtyObjects().addedObjectIds.is_empty(), "a client marks nothing to send");
    Ok(())
}

// A Client Script's tween is one of its own objects. It plays on the client's clock, and the client deletes it once it has
// played out.
#[tokio::test]
async fn a_client_tween_plays_on_the_client_and_the_client_deletes_it() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let running_script = run_client_script(
        &mut world,
        &format!(r#"game.animationService.tweenTo({BRICK_OBJECT_ID}, ["opacity"], 0, 1000, "Linear");"#),
    )
    .await?;
    assert_eq!(take_console_errors(&running_script), Vec::<String>::new());
    let tween_ids: Vec<ObjectId> = world._kindOfObjectId.get_keys(&ObjectKind::Tween).into_iter().flatten().copied().collect();
    let [tween_id] = tween_ids.as_slice() else {
        return Err(anyhow::anyhow!("Expected one tween, found {tween_ids:?}"));
    };
    assert!(is_client_object_id(*tween_id));

    world.step_physics(1, 1.0 / 60.0, 500.0, Some(500.0))?;
    assert!((brick_opacity(&world)? - 0.5).abs() < 1e-4, "halfway on the client's clock");
    world.step_physics(1, 1.0 / 60.0, 1000.0, Some(1000.0))?;
    assert!(brick_opacity(&world)?.abs() < 1e-4, "at the target");
    assert!(!world.hasObject(*tween_id), "the client deletes its own tween once it has played out");
    Ok(())
}

// A client can delete the server's objects for itself. They stay deleted: the server's later adds of them, or of new objects
// inside them, are dropped. Nothing is marked to send.
#[tokio::test]
async fn a_client_script_deletes_objects_only_in_its_own_world() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let brick_add_delta = world.buildAddDelta(BRICK_OBJECT_ID)?;
    let running_script = run_client_script(&mut world, &format!("game.objectService.deleteObject({BRICK_OBJECT_ID});")).await?;
    assert_eq!(take_console_errors(&running_script), Vec::<String>::new());
    assert!(!world.hasObject(BRICK_OBJECT_ID));
    assert!(world.flushDirtyObjects().removedObjectIds.is_empty(), "a client marks nothing to send");

    world.addObjectWithIdFromDelta(BRICK_OBJECT_ID, brick_add_delta.clone());
    assert!(!world.hasObject(BRICK_OBJECT_ID), "the server sending it again doesn't bring it back");
    let new_object_id = 5;
    world.addObjectWithIdFromDelta(
        new_object_id,
        ObjectAddDelta {
            parentId: BRICK_OBJECT_ID,
            ..brick_add_delta
        },
    );
    assert!(!world.hasObject(new_object_id), "nor does a new object the server adds inside it");
    Ok(())
}

// Players belong to the server, and so does a client's own character, so a client can't delete any player or its own character,
// or anything one sits inside. Another player's character it can delete for itself.
#[tokio::test]
async fn a_client_script_cannot_delete_a_player_or_its_own_character() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let root_object_id = world.getRootObjectId();
    let other_player_object_id = 98;
    let other_character_object_id = 6;
    add_ball(&mut world, other_character_object_id, root_object_id, Vec3::ZERO, true)?;
    add_player(&mut world, other_player_object_id, root_object_id, Some(other_character_object_id))?;
    let running_script = run_client_script(
        &mut world,
        &format!(
            r#"
for (const objectId of [{PLAYER_OBJECT_ID}, {CHARACTER_OBJECT_ID}, {other_player_object_id}, {root_object_id}, {other_character_object_id}]) {{
    try {{
        game.objectService.deleteObject(objectId);
    }} catch (error) {{
        console.error(String(error));
    }}
}}
"#
        ),
    )
    .await?;

    let console_errors = take_console_errors(&running_script);
    let refusals = console_errors
        .iter()
        .filter(|console_error| console_error.contains("Only the server can delete a player or your own character"))
        .count();
    assert_eq!(refusals, 4, "{console_errors:?}");
    for object_id in [PLAYER_OBJECT_ID, CHARACTER_OBJECT_ID, other_player_object_id] {
        assert!(world.hasObject(object_id), "{object_id} was deleted");
    }
    assert!(
        !world.hasObject(other_character_object_id),
        "another player's character can be deleted for this client"
    );
    Ok(())
}

// A client keeps its own templates, and they're never sent anywhere. Cutting a server object removes it from this client only,
// and it stays removed like a delete. Pasting makes objects with the client's own ids. The same objects are off limits as for
// a delete.
#[tokio::test]
async fn a_client_script_cuts_and_pastes_its_own_templates() -> anyhow::Result<()> {
    let mut world = make_client_world()?;
    let root_object_id = world.getRootObjectId();
    let brick_name = world.getObject(BRICK_OBJECT_ID)?.name.clone();
    let running_script = run_client_script(
        &mut world,
        &format!(
            r#"
game.templateService.cutObjectToTemplate({BRICK_OBJECT_ID}, "brick");
game.templateService.pasteTemplate("brick", {root_object_id});
game.templateService.pasteTemplate("brick", {root_object_id});
for (const tryIt of [
    () => game.templateService.pasteTemplate("madeOnTheServer", {root_object_id}),
    () => game.templateService.cutObjectToTemplate({CHARACTER_OBJECT_ID}, "me"),
]) {{
    try {{
        tryIt();
    }} catch (error) {{
        console.error(String(error));
    }}
}}
"#
        ),
    )
    .await?;

    let console_errors = take_console_errors(&running_script);
    assert_eq!(console_errors.len(), 2, "{console_errors:?}");
    assert!(console_errors[0].contains("template madeOnTheServer was not found"), "{console_errors:?}");
    assert!(
        console_errors[1].contains("Only the server can delete a player or your own character"),
        "{console_errors:?}"
    );
    assert!(world.hasObject(CHARACTER_OBJECT_ID));

    assert!(!world.hasObject(BRICK_OBJECT_ID));
    assert!(
        world.serverObjectIdsDeletedOnClient.contains(&BRICK_OBJECT_ID),
        "a cut server object stays removed, like a delete"
    );
    let mut pasted_object_ids = world.getObjectIdsByName(&brick_name);
    pasted_object_ids.sort();
    assert_eq!(pasted_object_ids, vec![TOP_CLIENT_OBJECT_ID - 1, TOP_CLIENT_OBJECT_ID]);
    Ok(())
}
