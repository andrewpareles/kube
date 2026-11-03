use game_types::consts::STORED_WORLD_CURRENT_VERSION;
use serde_json::{Value, json};
use shared::game::migrateStoredWorld::{
    CHARACTER_CONTROLLER_FOLDER_NAME, CHARACTER_CONTROLLER_SCRIPT, CHARACTER_CONTROLLER_SCRIPT_NAME, StoredWorldKind, updateToVersion,
};
use shared::game::world::{World, WorldControlMode};

const INITIAL_DRAFT_WORLD: &str = include_str!("../../editor_server/initial-draft-gameworld.kube");

fn world_json_with_objects_up_to(max_object_id: u64, world_schema_version: u64) -> Value {
    let mut stored_world: Value = serde_json::from_str(INITIAL_DRAFT_WORLD).unwrap();
    stored_world["worldSchemaVersion"] = json!(world_schema_version);
    stored_world["objectOfId"][max_object_id.to_string()] = json!({ "id": max_object_id, "name": "folder", "info": { "LocalGroup": {} } });
    stored_world["childrenOfId"]["1"] = json!([max_object_id]);
    stored_world
}

#[test]
fn new_games_start_with_the_character_controller_script() -> anyhow::Result<()> {
    let mut stored_world: Value = serde_json::from_str(INITIAL_DRAFT_WORLD)?;
    updateToVersion(&mut stored_world, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;

    let folder_id = stored_world["childrenOfId"]["1"][0].as_u64().expect("root should have the controller folder");
    assert_eq!(stored_world["objectOfId"][folder_id.to_string()]["name"], CHARACTER_CONTROLLER_FOLDER_NAME);
    assert_eq!(stored_world["objectOfId"][folder_id.to_string()]["info"], json!({ "LocalGroup": {} }));

    let script_id = stored_world["childrenOfId"][folder_id.to_string()][0]
        .as_u64()
        .expect("the folder should hold the script");
    assert_eq!(stored_world["objectOfId"][script_id.to_string()]["name"], CHARACTER_CONTROLLER_SCRIPT_NAME);
    assert_eq!(
        stored_world["objectOfId"][script_id.to_string()]["info"],
        json!({ "Script": { "objectLocation": "Client" } })
    );
    assert_eq!(stored_world["scriptRawCodeOfId"][script_id.to_string()], CHARACTER_CONTROLLER_SCRIPT);

    let world = World::fromStoredWorldString(INITIAL_DRAFT_WORLD, WorldControlMode::Editor, None)?;
    assert_eq!(world.objectOfId.len(), 3, "the root, the folder and the script should load");
    Ok(())
}

#[test]
fn the_controller_takes_ids_after_every_existing_object() -> anyhow::Result<()> {
    let mut stored_world = world_json_with_objects_up_to(42, 75);
    updateToVersion(&mut stored_world, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;

    assert_eq!(stored_world["childrenOfId"]["1"], json!([42, 43]));
    assert_eq!(stored_world["childrenOfId"]["43"], json!([44]));
    assert!(stored_world["scriptRawCodeOfId"]["44"].is_string());
    World::fromStoredWorldString(&stored_world.to_string(), WorldControlMode::Editor, None)?;
    Ok(())
}

#[test]
fn worlds_saved_with_the_controller_script_do_not_get_a_second_one() -> anyhow::Result<()> {
    let mut stored_world = world_json_with_objects_up_to(42, STORED_WORLD_CURRENT_VERSION);
    let before = stored_world.clone();
    updateToVersion(&mut stored_world, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;
    assert_eq!(stored_world, before);
    Ok(())
}

#[test]
fn a_world_that_already_has_the_controller_folder_keeps_just_that_one() -> anyhow::Result<()> {
    let mut stored_world = world_json_with_objects_up_to(42, 75);
    stored_world["objectOfId"]["42"]["name"] = json!(CHARACTER_CONTROLLER_FOLDER_NAME);
    updateToVersion(&mut stored_world, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;

    assert_eq!(stored_world["childrenOfId"]["1"], json!([42]));
    assert!(stored_world["scriptRawCodeOfId"].as_object().unwrap().is_empty());
    Ok(())
}

#[test]
fn asset_objects_never_get_the_controller() -> anyhow::Result<()> {
    let mut stored_world = world_json_with_objects_up_to(42, 75);
    updateToVersion(&mut stored_world, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::AssetObject)?;

    assert_eq!(stored_world["childrenOfId"]["1"], json!([42]));
    assert_eq!(stored_world["worldSchemaVersion"], json!(STORED_WORLD_CURRENT_VERSION));
    Ok(())
}
