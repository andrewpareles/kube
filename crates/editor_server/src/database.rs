use crate::{
    api_error::{ApiError, internal_error},
    auth::BrowserAccess,
};
use axum::http::StatusCode;
use game_types::data_types::{DataId, DataKind};
use linux_sandbox::MeshSourceFormat;
use render_core::dehydrated_gltf::BlenderId;
use serde::Deserialize;
use serde_json::{Value, json};
use shared::api::protocol_for_assets::{AssetSearchRequest, AssetSummary};
use shared::env::env_vars::{ARBITRIUM_REQUEST_ID, EDITOR_PLAYTEST_PORT, GAME_ID};
use shared::game::assets::AssetType;
use shared::player_profile::PlayerProfile;
use shared::postgrest;
use shared::s3_storage::{PrivateFile, download_private_file, upload_private_file};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

// Every database operation the editor makes. They go through PostgREST, so an editor holds no database connection, and what has
// to happen in one transaction is a function in 0092_editor_database_functions.sql. EditorDatabase's methods are checked against
// this editor's deployment, which a dev editor doesn't have.
pub struct EditorDatabase {
    pub is_dev: bool,
}

#[derive(Deserialize)]
pub struct WorldFiles {
    pub creator_id: String,
    pub editor_world_uuid: Option<String>,
    pub published_world_uuid: Option<String>,
}

#[derive(Deserialize)]
pub struct GameSettings {
    pub game_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub suggested_min_players: i32,
    pub max_players: i32,
    pub is_published: Option<bool>,
}
const GAME_SETTINGS_COLUMNS: &str = "game_id,name,description,suggested_min_players,max_players,is_published";

#[derive(Deserialize)]
pub struct AssetPublisher {
    pub uploaded_by: String,
    pub asset_type: AssetType,
}

#[derive(Deserialize)]
pub struct AssetVersion {
    pub uploaded_by: String,
    pub asset_uuid: String,
}

#[derive(Deserialize)]
pub struct CatalogItem {
    pub catalog_item_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_listed: bool,
}
const CATALOG_ITEM_COLUMNS: &str = "catalog_item_id,name,description,is_listed";

impl EditorDatabase {
    pub fn new(is_dev: bool) -> Self {
        Self { is_dev }
    }

    // The deployment the functions check, which a dev editor doesn't have.
    fn request_id(&self) -> Option<&'static str> {
        (!self.is_dev).then(|| ARBITRIUM_REQUEST_ID.as_str())
    }

    // Seed once from this game's publication or the original default world.
    pub async fn load_world(&self) -> Result<(String, String), ApiError> {
        let files = self.world_files().await?;
        if let Some(world_uuid) = files.editor_world_uuid {
            let world_uuid = Uuid::parse_str(&world_uuid).map_err(internal_error)?;
            let bytes = download_private_file(&PrivateFile::EditorWorld { world_uuid }).await.map_err(internal_error)?;
            return Ok((files.creator_id, String::from_utf8(bytes).map_err(internal_error)?));
        }
        let world = match files.published_world_uuid {
            Some(world_uuid) => {
                let world_uuid = Uuid::parse_str(&world_uuid).map_err(internal_error)?;
                let bytes = download_private_file(&PrivateFile::GameWorld { world_uuid }).await.map_err(internal_error)?;
                String::from_utf8(bytes).map_err(internal_error)?
            }
            None => include_str!("../initial-draft-gameworld.kube").to_string(),
        };
        self.save_world(&world).await?;
        Ok((files.creator_id, world))
    }

    pub async fn world_files(&self) -> Result<WorldFiles, ApiError> {
        postgrest::call::<Vec<WorldFiles>>("editor_load_world", json!({"input_game_id": *GAME_ID, "input_request_id": self.request_id()}))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| internal_error("editor_load_world returned no row"))
    }

    // The world goes to a new file before the game row points at it. An editor that was replaced while uploading fails the
    // check and leaves its file unused.
    pub async fn save_world(&self, world: &str) -> Result<(), ApiError> {
        let world_uuid = Uuid::new_v4();
        upload_private_file(&PrivateFile::EditorWorld { world_uuid }, world.as_bytes().to_vec())
            .await
            .map_err(internal_error)?;
        self.set_editor_world(world_uuid).await
    }

    pub async fn set_editor_world(&self, world_uuid: Uuid) -> Result<(), ApiError> {
        postgrest::call::<()>(
            "editor_save_world",
            json!({"input_game_id": *GAME_ID, "input_request_id": self.request_id(), "input_world_uuid": world_uuid.to_string()}),
        )
        .await?;
        Ok(())
    }

    // Checked on every operation, including changes after startup.
    pub async fn require_browser_access(&self, access: &BrowserAccess) -> Result<(), ApiError> {
        let (profile_id, invite_token) = match access {
            BrowserAccess::Creator { profile_id } => (Some(profile_id.as_str()), None),
            BrowserAccess::Guest { invite_token } => (None, Some(invite_token.as_str())),
        };
        postgrest::call::<String>(
            "editor_browser_access",
            json!({"input_game_id": *GAME_ID, "input_request_id": self.request_id(), "input_profile_id": profile_id, "input_invite_token": invite_token}),
        )
        .await?;
        Ok(())
    }

    // Only adding a playtest player needs the externally reachable UDP address.
    pub async fn playtest_address(&self) -> Result<(String, i32), ApiError> {
        let Some(request_id) = self.request_id() else {
            return Ok(("127.0.0.1".into(), EDITOR_PLAYTEST_PORT.parse().expect("playtest port")));
        };
        #[derive(Deserialize)]
        struct PlaytestAddress {
            public_ip: String,
            playtest_external_port: i32,
        }
        let address = postgrest::from("running_editor_servers")
            .select("public_ip,playtest_external_port")
            .eq("request_id", request_id)
            .eq("game_id", *GAME_ID)
            .get::<PlaytestAddress>()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::new(StatusCode::GONE, "Editor server stopped. Reopen the editor."))?;
        Ok((address.public_ip, address.playtest_external_port))
    }

    // Remove only this deployment's records. A replacement editor must retain its ownership.
    pub async fn remove_deployment(&self) -> Result<(), ApiError> {
        let Some(request_id) = self.request_id() else {
            return Ok(());
        };
        postgrest::call::<()>("editor_remove_deployment", json!({"input_game_id": *GAME_ID, "input_request_id": request_id})).await?;
        Ok(())
    }
}

// -------------------------- Players --------------------------

pub async fn player_profile(profile_id: &str) -> Result<(String, i64, PlayerProfile), ApiError> {
    #[derive(Deserialize)]
    struct Profile {
        username: String,
        user_id: i64,
        preferred_color: Option<String>,
        equipped_catalog_item_id: Option<i64>,
    }
    let profile = postgrest::from("user_profiles")
        .select("username,user_id,preferred_color,equipped_catalog_item_id")
        .eq("id", profile_id)
        .get::<Profile>()
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Profile not found."))?;
    let player_profile = PlayerProfile::from_profile_row(profile.preferred_color, profile.equipped_catalog_item_id).map_err(internal_error)?;
    Ok((profile.username, profile.user_id, player_profile))
}

pub async fn profile_email_and_username(profile_id: &str) -> anyhow::Result<(String, String)> {
    #[derive(Deserialize)]
    struct Profile {
        email: Option<String>,
        username: String,
    }
    let profile = postgrest::from("user_profiles")
        .select("email,username")
        .eq("id", profile_id)
        .get::<Profile>()
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Profile not found."))?;
    let email = profile.email.ok_or_else(|| anyhow::anyhow!("Profile has no email."))?;
    Ok((email, profile.username))
}

// -------------------------- Game settings and publishing --------------------------

pub async fn game_settings() -> anyhow::Result<Option<GameSettings>> {
    Ok(postgrest::from("game_metadata")
        .select(GAME_SETTINGS_COLUMNS)
        .eq("game_id", *GAME_ID)
        .get::<GameSettings>()
        .await?
        .into_iter()
        .next())
}

// Only the supplied settings change, and an empty description clears it. This never changes which published version plays.
pub async fn set_game_settings(
    name: Option<String>,
    description: Option<String>,
    suggested_min_players: Option<i64>,
    max_players: Option<i64>,
    is_published: Option<bool>,
) -> anyhow::Result<Option<GameSettings>> {
    let mut changes = serde_json::Map::new();
    if let Some(name) = name {
        changes.insert("name".into(), json!(name));
    }
    if let Some(description) = description {
        changes.insert("description".into(), json!((!description.is_empty()).then_some(description)));
    }
    if let Some(suggested_min_players) = suggested_min_players {
        changes.insert("suggested_min_players".into(), json!(suggested_min_players));
    }
    if let Some(max_players) = max_players {
        changes.insert("max_players".into(), json!(max_players));
    }
    if let Some(is_published) = is_published {
        changes.insert("is_published".into(), json!(is_published));
    }
    Ok(postgrest::from("game_metadata")
        .select(GAME_SETTINGS_COLUMNS)
        .eq("game_id", *GAME_ID)
        .update_returning::<GameSettings>(changes)
        .await?
        .into_iter()
        .next())
}

pub async fn insert_game_world(world_uuid: Uuid) -> anyhow::Result<i64> {
    #[derive(Deserialize)]
    struct InsertedGameWorld {
        version_num: i64,
    }
    postgrest::from("game_worlds")
        .select("version_num")
        .insert_returning::<InsertedGameWorld>(json!({"game_id": *GAME_ID, "world_uuid": world_uuid.to_string()}))
        .await?
        .into_iter()
        .next()
        .map(|game_world| game_world.version_num)
        .ok_or_else(|| anyhow::anyhow!("Gameworld insert returned no rows"))
}

pub async fn set_game_screenshot(screenshot_uuid: Uuid) -> anyhow::Result<()> {
    postgrest::from("game_metadata")
        .eq("game_id", *GAME_ID)
        .update(json!({"screenshot_uuid": screenshot_uuid.to_string()}))
        .await?;
    Ok(())
}

pub async fn game_ai_thumbnail_uuid() -> anyhow::Result<Option<String>> {
    #[derive(Deserialize)]
    struct AiThumbnail {
        ai_thumbnail_uuid: Option<String>,
    }
    Ok(postgrest::from("game_metadata")
        .select("ai_thumbnail_uuid")
        .eq("game_id", *GAME_ID)
        .get::<AiThumbnail>()
        .await?
        .into_iter()
        .next()
        .and_then(|AiThumbnail { ai_thumbnail_uuid }| ai_thumbnail_uuid))
}

pub async fn set_game_ai_thumbnail(ai_thumbnail_uuid: Uuid) -> anyhow::Result<()> {
    postgrest::from("game_metadata")
        .eq("game_id", *GAME_ID)
        .update(json!({"ai_thumbnail_uuid": ai_thumbnail_uuid.to_string()}))
        .await?;
    Ok(())
}

// -------------------------- Assets --------------------------

pub async fn asset_publisher(asset_id: i64) -> anyhow::Result<Option<AssetPublisher>> {
    Ok(postgrest::from("asset_metadata")
        .select("uploaded_by,asset_type")
        .eq("asset_id", asset_id)
        .limit(1)
        .get::<AssetPublisher>()
        .await?
        .into_iter()
        .next())
}

pub async fn insert_asset_version(asset_id: Option<i64>, name: &str, asset_type: AssetType, asset_uuid: Uuid, uploaded_by: &str) -> anyhow::Result<Value> {
    let mut row = json!({"name": name, "asset_type": asset_type, "asset_uuid": asset_uuid.to_string(), "uploaded_by": uploaded_by});
    if let Some(asset_id) = asset_id {
        row["asset_id"] = asset_id.into();
    }
    postgrest::from("asset_metadata")
        .select("asset_id,version_num,name,created_at,asset_type")
        .insert_returning::<Value>(row)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Asset insert returned no row"))
}

pub async fn latest_asset_version(asset_id: i64) -> anyhow::Result<Option<AssetVersion>> {
    Ok(postgrest::from("asset_metadata")
        .select("uploaded_by,asset_uuid")
        .eq("asset_id", asset_id)
        .order_desc("version_num")
        .limit(1)
        .get::<AssetVersion>()
        .await?
        .into_iter()
        .next())
}

pub async fn latest_asset_row(asset_id: i64) -> anyhow::Result<Option<Value>> {
    Ok(postgrest::from("asset_metadata")
        .select("asset_id,version_num,name,created_at,asset_type,asset_uuid")
        .eq("asset_id", asset_id)
        .order_desc("version_num")
        .limit(1)
        .get::<Value>()
        .await?
        .into_iter()
        .next())
}

pub async fn search_asset_metadata(request: &AssetSearchRequest) -> anyhow::Result<Vec<AssetSummary>> {
    Ok(postgrest::call(
        "search_asset_metadata",
        json!({"input_query": request.query, "input_asset_type": request.asset_type, "input_offset": request.offset, "input_limit": request.limit}),
    )
    .await?)
}

pub async fn record_asset_import(asset_id: i64, version_num: i64) -> anyhow::Result<()> {
    postgrest::call::<()>("record_asset_import", json!({"input_asset_id": asset_id, "input_version_num": version_num})).await?;
    Ok(())
}

// -------------------------- File ids --------------------------

// A file named by an integer id is uploaded before the row that records it, so its id is taken from the counter first.
pub async fn next_data_id() -> anyhow::Result<DataId> {
    Ok(postgrest::call("next_data_id", json!({})).await?)
}

pub async fn next_catalog_item_id() -> anyhow::Result<i64> {
    Ok(postgrest::call("next_catalog_item_id", json!({})).await?)
}

pub async fn next_blender_id() -> anyhow::Result<BlenderId> {
    Ok(postgrest::call("next_blender_id", json!({})).await?)
}

// -------------------------- Data files and model sources --------------------------

pub async fn insert_data_file(data_kind: &DataKind, data_id: DataId, uploaded_by: &str) -> anyhow::Result<()> {
    postgrest::from("data_metadata")
        .insert(json!({"data_type": data_kind.supabase_data_type_str(), "data_id": data_id, "uploaded_by": uploaded_by}))
        .await?;
    Ok(())
}

// How many of these meshes the profile uploaded.
pub async fn owned_mesh_data_count(profile_id: &str, mesh_data_ids: &[i64]) -> anyhow::Result<usize> {
    Ok(postgrest::from("data_metadata")
        .select("data_id")
        .eq("data_type", "mesh_data")
        .eq("uploaded_by", profile_id)
        .is_in("data_id", mesh_data_ids)
        .get::<serde::de::IgnoredAny>()
        .await?
        .len())
}

pub async fn insert_model_source(blender_id: BlenderId, uploaded_by: &str, source_format: MeshSourceFormat) -> anyhow::Result<()> {
    postgrest::from("blender_metadata")
        .insert(json!({"blender_id": blender_id, "uploaded_by": uploaded_by, "source_format": source_format}))
        .await?;
    Ok(())
}

pub async fn model_source_format(blender_id: BlenderId, uploaded_by: &str) -> anyhow::Result<Option<MeshSourceFormat>> {
    #[derive(Deserialize)]
    struct ModelSource {
        source_format: MeshSourceFormat,
    }
    Ok(postgrest::from("blender_metadata")
        .select("source_format")
        .eq("blender_id", blender_id)
        .eq("uploaded_by", uploaded_by)
        .get::<ModelSource>()
        .await?
        .into_iter()
        .next()
        .map(|model_source| model_source.source_format))
}

// -------------------------- The catalog --------------------------

pub async fn publish_catalog_item(
    catalog_item_id: i64,
    creator_id: &str,
    name: &str,
    description: Option<&str>,
    is_listed: bool,
    mesh_data_ids: &[i64],
) -> anyhow::Result<CatalogItem> {
    Ok(postgrest::call(
        "publish_catalog_item",
        json!({
            "input_catalog_item_id": catalog_item_id,
            "input_creator_id": creator_id,
            "input_name": name,
            "input_description": description,
            "input_is_listed": is_listed,
            "input_mesh_data_ids": mesh_data_ids,
        }),
    )
    .await?)
}

// Only the supplied fields change, and an empty description clears it. Only the item's creator can edit it.
pub async fn edit_catalog_item(
    catalog_item_id: i64,
    creator_id: &str,
    name: Option<String>,
    description: Option<String>,
    is_listed: Option<bool>,
) -> anyhow::Result<Option<CatalogItem>> {
    let mut changes = serde_json::Map::new();
    if let Some(name) = name {
        changes.insert("name".into(), json!(name));
    }
    if let Some(description) = description {
        changes.insert("description".into(), json!((!description.is_empty()).then_some(description)));
    }
    if let Some(is_listed) = is_listed {
        changes.insert("is_listed".into(), json!(is_listed));
    }
    changes.insert("updated_at".into(), json!(OffsetDateTime::now_utc().format(&Rfc3339)?));
    Ok(postgrest::from("catalog_metadata")
        .select(CATALOG_ITEM_COLUMNS)
        .eq("catalog_item_id", catalog_item_id)
        .eq("creator_id", creator_id)
        .update_returning::<CatalogItem>(changes)
        .await?
        .into_iter()
        .next())
}
