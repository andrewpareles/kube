#[path = "../src/api_error.rs"]
#[allow(dead_code)]
mod api_error;
#[path = "../src/assets.rs"]
#[allow(dead_code)]
mod assets;
#[path = "../src/auth.rs"]
#[allow(dead_code)]
mod auth;
#[path = "../../shared/tests/helpers/character_kit.rs"]
mod character_kit;
#[path = "../src/database.rs"]
#[allow(dead_code)]
mod database;

use assets::publish_asset;
use character_kit::default_character_asset_object;
use shared::api::protocol_for_assets::{AssetPublishData, AssetPublishRequest};

// The DefaultCharacter that games download from assets for their players.

const KUBESTER_PROFILE_ID: &str = "5cd943b5-704d-4720-ba0f-c0e3c10ba72f"; // user 17
const ASSET_ID: i64 = 154;

// Publishes a new version of the DefaultCharacter as user 17, after changing its scripts or its character. Run it against prod:
// SUPABASE_URL=... SUPABASE_SECRET_KEY=... S3_ACCESS_KEY_ID=... S3_SECRET_ACCESS_KEY=... cargo test -p editor_server --test publish_default_character -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn publish_default_character() -> anyhow::Result<()> {
    let asset = publish_asset(
        KUBESTER_PROFILE_ID,
        AssetPublishRequest {
            asset_id: Some(ASSET_ID),
            name: "DefaultCharacter".to_string(),
            data: AssetPublishData::Object(serde_json::to_string(&default_character_asset_object()?)?),
        },
    )
    .await?;
    println!("published asset {} (version {})", asset.asset_id, asset.version_num);
    Ok(())
}
