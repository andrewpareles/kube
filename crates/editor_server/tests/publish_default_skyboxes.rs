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
use game_types::data_types::TextureData;
use serde::Deserialize;
use shared::{
    api::protocol_for_assets::{AssetPublishData, AssetPublishRequest, UploadedDataWrapper},
    game::texture_converter::convert_skybox_image_bytes_to_texture_data,
    postgrest,
};

// The built-in skies, all drawn by assets/skyboxes/generate_cloudy_skyboxes.py. See assets/skyboxes/SKYBOX_TEXTURE_SOURCES.md.

const KUBESTER_PROFILE_ID: &str = "5cd943b5-704d-4720-ba0f-c0e3c10ba72f"; // user 17

// The HD skies were published as Texture assets before skyboxes had their own asset type, and were turned into Skybox assets in place
// before asset files moved to S3, where a file is never rewritten.
const HD_SKYBOX_ASSET_IDS: [i64; 8] = [163, 164, 165, 166, 167, 168, 169, 170];

// Each is read from target/skyboxes/<preset>.png, drawn by: Blender -b --factory-startup -P generate_cloudy_skyboxes.py -- target/skyboxes <preset> 4096
const GENERATED_SKYBOX_PRESETS: [(&str, &str); 4] = [
    ("Sky - Storm Gold Horizon", "storm_gold_horizon"),
    ("Sky - Alien Green", "alien_green"),
    ("Sky - Blood Red", "blood_red"),
    ("Sky - Amber Sunset", "amber_sunset"),
];

#[derive(Deserialize)]
struct PublishedSkybox {
    asset_id: i64,
    name: String,
}

// Publishes each generated sky that user 17 has not published yet, then prints them all with their asset ids for the system prompt. Run it against prod after the release, since older editors can't read Skybox assets:
// SUPABASE_URL=... SUPABASE_SECRET_KEY=... S3_ACCESS_KEY_ID=... S3_SECRET_ACCESS_KEY=... cargo test -p editor_server --test publish_default_skyboxes -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn publish_default_skyboxes() -> anyhow::Result<()> {
    let mut published = postgrest::from("asset_metadata")
        .select("asset_id,name")
        .eq("asset_type", "Skybox")
        .eq("uploaded_by", KUBESTER_PROFILE_ID)
        .get::<PublishedSkybox>()
        .await?;
    for (name, preset) in GENERATED_SKYBOX_PRESETS {
        if published.iter().any(|skybox| skybox.name == name) {
            continue;
        }
        let image_bytes = std::fs::read(format!("{}/../../target/skyboxes/{preset}.png", env!("CARGO_MANIFEST_DIR")))?;
        let request = AssetPublishRequest {
            asset_id: None,
            name: name.to_string(),
            data: AssetPublishData::Skybox(UploadedDataWrapper::<TextureData>::new(convert_skybox_image_bytes_to_texture_data(
                &image_bytes,
            )?)),
        };
        let asset = publish_asset(KUBESTER_PROFILE_ID, request).await?;
        published.push(PublishedSkybox {
            asset_id: asset.asset_id,
            name: asset.name,
        });
    }
    let find_by_name = |name: &str| {
        published
            .iter()
            .find(|skybox| skybox.name == name)
            .ok_or_else(|| anyhow::anyhow!("{name} was not published"))
    };
    let hd = HD_SKYBOX_ASSET_IDS
        .iter()
        .map(|asset_id| {
            published
                .iter()
                .find(|skybox| skybox.asset_id == *asset_id)
                .ok_or_else(|| anyhow::anyhow!("Asset {asset_id} is not a Skybox asset"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let generated = GENERATED_SKYBOX_PRESETS
        .iter()
        .map(|(name, _)| find_by_name(name))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let entries = hd
        .iter()
        .chain(&generated)
        .map(|skybox| format!("{} ({})", skybox.name.trim_start_matches("Sky - "), skybox.asset_id))
        .collect::<Vec<_>>();
    println!("- {}", entries.join(", "));
    Ok(())
}
