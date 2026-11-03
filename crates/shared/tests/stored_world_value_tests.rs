use shared::game::world::{World, WorldControlMode};

// The editor skips a save whose world equals the last one it wrote. The world's maps list their keys in a different order on
// every serialization, so it compares JSON values, which stay equal until something changes.
#[test]
fn an_unchanged_world_gives_an_equal_stored_value_and_an_edit_changes_it() -> anyhow::Result<()> {
    let mut world = World::fromStoredWorldString(include_str!("fixtures/default_character.kube"), WorldControlMode::Editor, None)?;
    let saved = serde_json::to_value(world.toStoredWorld())?;
    assert_eq!(serde_json::to_value(world.toStoredWorld())?, saved);
    let stored_world = world.toStoredWorld();
    let object_id = *stored_world
        .objectOfId
        .keys()
        .find(|object_id| **object_id != stored_world.rootObjectId)
        .unwrap();
    world.setObjectName(object_id, "Renamed".into())?;
    assert_ne!(serde_json::to_value(world.toStoredWorld())?, saved);
    Ok(())
}
