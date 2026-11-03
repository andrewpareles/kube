use game_types::consts::STORED_WORLD_CURRENT_VERSION;
use serde_json::{Value, json};
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};
use shared::game::world::{World, WorldControlMode};

fn pigball() -> anyhow::Result<Value> {
    Ok(serde_json::from_str(include_str!("../../../game-pigball.kube"))?)
}

// One group nested inside the next, `depth` levels below the root.
fn nested_groups(depth: usize) -> String {
    let mut object_of_id = serde_json::Map::new();
    let mut children_of_id = serde_json::Map::new();
    for id in 1..=depth + 1 {
        object_of_id.insert(id.to_string(), json!({ "id": id, "name": format!("group-{id}"), "info": { "LocalGroup": {} } }));
        let children = if id <= depth { vec![id + 1] } else { Vec::new() };
        children_of_id.insert(id.to_string(), json!(children));
    }
    json!({
        "worldSchemaVersion": STORED_WORLD_CURRENT_VERSION,
        "fallback_camera_location": { "position": [0.0, -10.0, 0.0], "orientation": { "theta": 0.0, "phi": 0.5 } },
        "objectOfId": object_of_id,
        "rootObjectId": 1,
        "childrenOfId": children_of_id,
        "scriptRawCodeOfId": {},
        "physicsInfo3DOfId": {},
        "physicsToggles3DOfId": {},
        "primitiveSettingsOfId": {},
    })
    .to_string()
}

fn load_world(stored_world_string: &str) -> anyhow::Result<World> {
    World::fromStoredWorldString(stored_world_string, WorldControlMode::Editor, None)
}

fn read_error(stored_world_string: &str) -> String {
    match load_world(stored_world_string) {
        Ok(_) => panic!("The upload was accepted"),
        Err(error) => format!("{error:#}"),
    }
}

// An older saved file migrates on the way in, and a file downloaded from a live world uploads again.
#[test]
fn a_saved_world_and_a_downloaded_world_can_be_uploaded() -> anyhow::Result<()> {
    let uploaded = load_world(include_str!("../../../game-pigball.kube"))?;
    let mut migrated = pigball()?;
    updateToVersion(&mut migrated, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;
    assert_eq!(uploaded.objectOfId.len(), migrated["objectOfId"].as_object().unwrap().len());
    let reuploaded = load_world(&uploaded.toStoredWorldString()?)?;
    assert_eq!(reuploaded.objectOfId.len(), uploaded.objectOfId.len());
    Ok(())
}

// Loading, copying, and removing a world all recurse through its hierarchy, so the deepest
// world an upload allows has to make it through every one of them. The release build the editor server
// ships as does it in a 2 MiB thread stack; a debug build needs about ten times the stack per level.
#[test]
fn the_deepest_allowed_world_uploads_and_a_deeper_one_is_refused() -> anyhow::Result<()> {
    let stack_size = if cfg!(debug_assertions) { 64 << 20 } else { 2 << 20 };
    let uploading = std::thread::Builder::new().stack_size(stack_size).spawn(|| -> anyhow::Result<()> {
        let mut world = World::fromStoredWorldString(include_str!("../../../dev-demo.kube"), WorldControlMode::Editor, None)?;
        let uploaded = load_world(&nested_groups(1024))?;
        world.loadNewWorld(&uploaded)?;
        world.toStoredWorldString()?;
        world.loadNewWorld(&load_world(include_str!("../../../game-pigball.kube"))?)?;
        Ok(())
    })?;
    uploading.join().expect("The upload thread panicked")?;

    assert!(read_error(&nested_groups(1025)).contains("nested more than 1024 deep"));
    Ok(())
}

#[test]
fn a_world_with_a_broken_transform_shape_or_camera_is_refused() -> anyhow::Result<()> {
    let mut world = pigball()?;
    let (id, _) = world["physicsInfo3DOfId"].as_object().unwrap().iter().next().unwrap();
    let id = id.clone();
    world["physicsInfo3DOfId"][&id]["location"]["quaternion"] = json!([0.0, 0.0, 0.0, 0.0]);
    assert!(read_error(&world.to_string()).contains(&format!("Object {id}: Invalid transform")));

    let mut world = pigball()?;
    world["physicsInfo3DOfId"][&id]["location"]["position"] = json!([1e39, 0.0, 0.0]);
    assert!(read_error(&world.to_string()).contains("Invalid transform"));

    let mut world = pigball()?;
    let brick_id = world["primitiveSettingsOfId"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, primitive_settings)| primitive_settings.get("Brick").is_some())
        .map(|(id, _)| id.clone())
        .unwrap();
    world["primitiveSettingsOfId"][&brick_id]["Brick"]["size"] = json!([-1.0, 1.0, 1.0]);
    assert!(read_error(&world.to_string()).contains("Invalid shape dimensions"));

    let mut world = pigball()?;
    world["fallback_camera_location"]["orientation"]["phi"] = json!(1e39);
    assert!(read_error(&world.to_string()).contains("camera must be finite"));
    Ok(())
}

// The loader skips an object it cannot build and keeps going, which would leave half a world.
#[test]
fn a_world_that_loses_an_object_while_loading_is_refused() -> anyhow::Result<()> {
    let mut world = pigball()?;
    let id = world["primitiveSettingsOfId"].as_object().unwrap().keys().next().unwrap().clone();
    world["primitiveSettingsOfId"].as_object_mut().unwrap().remove(&id);
    assert!(read_error(&world.to_string()).contains("could not be loaded"));
    Ok(())
}

#[test]
fn an_object_listed_under_two_parents_is_refused() -> anyhow::Result<()> {
    let mut world: Value = serde_json::from_str(&nested_groups(2))?;
    world["childrenOfId"]["1"] = json!([2, 3]);
    assert!(read_error(&world.to_string()).contains("Object 3 appears more than once"));
    Ok(())
}
