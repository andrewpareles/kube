use anyhow::{Result, bail};
use game_types::{consts::DEHYDRATED_GLTF_CURRENT_VERSION, migrateDehydratedGltf};
use render_core::dehydrated_gltf::DehydratedGltf;

use crate::s3_storage::{PublicFile, download_public_file};

// A catalog item is a Mesh in its own file, named by its id, and a game names the item's object by that id too, so loading
// one needs only the CDN. Like any asset that's read, its Mesh is brought up to the current format.
pub async fn download_catalog_item(catalog_item_id: i64) -> Result<(String, DehydratedGltf)> {
    let mut data_json: serde_json::Value = serde_json::from_slice(&download_public_file(&PublicFile::Catalog { catalog_item_id }).await?)?;
    let Some(mesh_json) = data_json.get_mut("Mesh") else {
        bail!("Catalog item {catalog_item_id} is not a Mesh.");
    };
    migrateDehydratedGltf::updateToVersion(mesh_json, DEHYDRATED_GLTF_CURRENT_VERSION)?;
    Ok((catalog_item_id.to_string(), serde_json::from_value(mesh_json.take())?))
}
