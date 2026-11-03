mod helpers;

use helpers::world_test_helpers::make_world;
use serde_json::Value;
use shared::{
    api::protocol_for_assets::Asset,
    game::{assets::add_asset_to_world, migrateStoredWorld::CHARACTER_CONTROLLER_FOLDER_NAME},
};

// Rows copied from production assets, exactly as the database returns them. "All Object Types" was published at world schema
// 75 and holds one of nearly every kind of object, including a skeleton with keyframes, and the two materials are a blended one and
// an opaque one. Asset rows keep the version they were published at and are migrated as they are read, so every row published so
// far has to keep loading.
const PUBLISHED_ASSET_ROWS: &str = include_str!("fixtures/published_asset_rows.json");

#[test]
fn published_asset_rows_still_load_and_add_to_a_game() -> anyhow::Result<()> {
    let rows: Vec<Value> = serde_json::from_str(PUBLISHED_ASSET_ROWS)?;
    for row in rows {
        let Asset { name, data_json, .. } = Asset::fromJson(row)?;
        let mut world = make_world()?;
        let object_count = world.objectOfId.len();
        add_asset_to_world(&mut world, name.clone(), data_json)?;
        assert!(world.objectOfId.len() > object_count, "{name} added nothing");
        // An asset object is a piece of a world rather than a game, so it never brings a CharacterController of its own.
        assert!(
            world.getObjectIdsByName(CHARACTER_CONTROLLER_FOLDER_NAME).is_empty(),
            "{name} added a CharacterController"
        );
    }
    Ok(())
}
