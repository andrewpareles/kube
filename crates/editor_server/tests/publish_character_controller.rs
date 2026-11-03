#[path = "../src/api_error.rs"]
#[allow(dead_code)]
mod api_error;
#[path = "../src/assets.rs"]
#[allow(dead_code)]
mod assets;
#[path = "../src/auth.rs"]
#[allow(dead_code)]
mod auth;
#[path = "../src/database.rs"]
#[allow(dead_code)]
mod database;

use assets::publish_asset;
use shared::api::protocol_for_assets::{AssetPublishData, AssetPublishRequest};
use shared::game::assets::StoredAssetObject;
use shared::game::migrateStoredWorld::CHARACTER_CONTROLLER_FOLDER_NAME;
use shared::game::world::{World, WorldControlMode};

const KUBESTER_PROFILE_ID: &str = "5cd943b5-704d-4720-ba0f-c0e3c10ba72f"; // user 17

// The CharacterController folder exactly as the world migration adds it.
fn character_controller_asset_object() -> anyhow::Result<StoredAssetObject> {
    let world = World::fromStoredWorldString(include_str!("../initial-draft-gameworld.kube"), WorldControlMode::Editor, None)?;
    let [folder_id] = world.getObjectIdsByName(CHARACTER_CONTROLLER_FOLDER_NAME)[..] else {
        anyhow::bail!("the migrated world should have exactly one {CHARACTER_CONTROLLER_FOLDER_NAME} folder");
    };
    StoredAssetObject::from_world_object(&world, folder_id)
}

// Inserting an asset object loads it as a world, which must not run the migration and add a second controller.
#[test]
fn the_character_controller_is_a_valid_asset_object() -> anyhow::Result<()> {
    let object = character_controller_asset_object()?;
    object.validate()?;
    object.to_object_template(CHARACTER_CONTROLLER_FOLDER_NAME)?;
    Ok(())
}

// Publishes it as an asset by user 17. Run it once against prod, after the code that migrates worlds to version 76 is deployed:
// SUPABASE_URL=... SUPABASE_SECRET_KEY=... S3_ACCESS_KEY_ID=... S3_SECRET_ACCESS_KEY=... cargo test -p editor_server --test publish_character_controller -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn publish_character_controller() -> anyhow::Result<()> {
    let asset = publish_asset(
        KUBESTER_PROFILE_ID,
        AssetPublishRequest {
            asset_id: None,
            name: CHARACTER_CONTROLLER_FOLDER_NAME.to_string(),
            data: AssetPublishData::Object(serde_json::to_string(&character_controller_asset_object()?)?),
        },
    )
    .await?;
    println!("published asset {} (version {})", asset.asset_id, asset.version_num);
    Ok(())
}
