#[path = "../src/api_error.rs"]
mod api_error;
#[path = "../src/auth.rs"]
#[allow(dead_code)]
mod auth;
#[path = "../src/database.rs"]
#[allow(dead_code)]
mod database;

use api_error::ApiError;
use auth::BrowserAccess;
use axum::http::StatusCode;
use database::EditorDatabase;
use game_types::data_types::DataKind;
use linux_sandbox::MeshSourceFormat;
use serde::Deserialize;
use serde_json::{Value, json};
use shared::api::protocol_for_assets::AssetSearchRequest;
use shared::env::env_vars::{ARBITRIUM_REQUEST_ID, EDITOR_PLAYTEST_PORT, GAME_ID};
use shared::game::assets::AssetType;
use shared::postgrest::{self, PostgrestError};
use uuid::Uuid;

async fn insert(table: &str, row: Value) {
    postgrest::from(table).insert(row).await.unwrap();
}

async fn add_profile(username: &str) -> String {
    let profile_id = Uuid::new_v4().to_string();
    insert("user_profiles", json!({"id": profile_id, "username": username})).await;
    profile_id
}

fn failure<T>(result: Result<T, ApiError>) -> (StatusCode, String) {
    let error = result.err().expect("the call succeeded");
    (error.status, error.message)
}

// Run by tests/postgrest_clients.database.test.ts, which starts PostgREST over a throwaway copy of production's schema and sets
// the variables these read. Every call in database.rs goes through it, and the deployment's calls as a dev editor too.
#[tokio::test]
#[ignore = "needs the PostgREST that tests/postgrest_clients.database.test.ts starts"]
async fn the_editor_database_calls_work_through_postgrest() {
    let editor = EditorDatabase::new(false);
    let dev_editor = EditorDatabase::new(true);
    let game_id = *GAME_ID;
    let request_id = ARBITRIUM_REQUEST_ID.as_str();
    let stopped = (StatusCode::GONE, "Editor server stopped. Reopen the editor.".to_string());

    assert_eq!(failure(editor.world_files().await), (StatusCode::NOT_FOUND, "Game not found.".to_string()));
    let creator_id = add_profile("creator").await;
    insert(
        "game_metadata",
        json!({"game_id": game_id, "name": "Game", "creator_id": creator_id, "max_players": 8}),
    )
    .await;
    assert_eq!(failure(editor.world_files().await), stopped);
    let files = dev_editor.world_files().await.unwrap();
    assert_eq!(
        (files.creator_id.as_str(), files.editor_world_uuid, files.published_world_uuid),
        (creator_id.as_str(), None, None)
    );

    let editor_world_uuid = Uuid::new_v4();
    assert_eq!(failure(editor.set_editor_world(editor_world_uuid).await), stopped);
    insert("starting_editor_servers", json!({"request_id": request_id, "game_id": game_id})).await;
    editor.set_editor_world(editor_world_uuid).await.unwrap();
    insert("game_worlds", json!({"game_id": game_id, "world_uuid": Uuid::new_v4().to_string()})).await;
    let published_world_uuid = Uuid::new_v4();
    assert_eq!(database::insert_game_world(published_world_uuid).await.unwrap(), 2);
    let files = editor.world_files().await.unwrap();
    assert_eq!(
        (files.editor_world_uuid, files.published_world_uuid),
        (Some(editor_world_uuid.to_string()), Some(published_world_uuid.to_string()))
    );
    #[derive(Deserialize)]
    struct Thumbnails {
        screenshot_uuid: Option<String>,
        ai_thumbnail_uuid: Option<String>,
    }
    let thumbnails = async || {
        let Thumbnails {
            screenshot_uuid,
            ai_thumbnail_uuid,
        } = postgrest::from("game_metadata")
            .select("screenshot_uuid,ai_thumbnail_uuid")
            .eq("game_id", game_id)
            .get::<Thumbnails>()
            .await
            .unwrap()
            .remove(0);
        (screenshot_uuid, ai_thumbnail_uuid)
    };
    let first_screenshot_uuid = Uuid::new_v4();
    database::set_game_screenshot(first_screenshot_uuid).await.unwrap();
    assert_eq!(thumbnails().await, (Some(first_screenshot_uuid.to_string()), None));
    assert_eq!(database::game_ai_thumbnail_uuid().await.unwrap(), None);
    let ai_thumbnail_uuid = Uuid::new_v4();
    database::set_game_ai_thumbnail(ai_thumbnail_uuid).await.unwrap();
    // A publish after the game has an AI thumbnail keeps it.
    let screenshot_uuid = Uuid::new_v4();
    database::set_game_screenshot(screenshot_uuid).await.unwrap();
    assert_eq!(thumbnails().await, (Some(screenshot_uuid.to_string()), Some(ai_thumbnail_uuid.to_string())));
    assert_eq!(database::game_ai_thumbnail_uuid().await.unwrap(), Some(ai_thumbnail_uuid.to_string()));

    let creator = BrowserAccess::Creator {
        profile_id: creator_id.clone(),
    };
    assert_eq!(failure(editor.require_browser_access(&creator).await), stopped);
    postgrest::from("starting_editor_servers").eq("request_id", request_id).delete().await.unwrap();
    insert(
        "running_editor_servers",
        json!({"request_id": request_id, "game_id": game_id, "editor_url": "https://editor.example", "public_ip": "203.0.113.7", "playtest_external_port": 30123}),
    )
    .await;
    editor.require_browser_access(&creator).await.unwrap();
    insert("editor_share_auth", json!({"game_id": game_id, "invite_token": "invite-1"})).await;
    editor
        .require_browser_access(&BrowserAccess::Guest {
            invite_token: "invite-1".into(),
        })
        .await
        .unwrap();
    let refused = (
        StatusCode::FORBIDDEN,
        "You do not have access to this editor. The invite link may have been revoked.".to_string(),
    );
    assert_eq!(
        failure(
            editor
                .require_browser_access(&BrowserAccess::Guest {
                    invite_token: "invite-2".into()
                })
                .await
        ),
        refused
    );
    let stranger_id = add_profile("stranger").await;
    assert_eq!(
        failure(
            editor
                .require_browser_access(&BrowserAccess::Creator {
                    profile_id: stranger_id.clone()
                })
                .await
        ),
        refused
    );
    assert_eq!(
        failure(
            editor
                .require_browser_access(&BrowserAccess::Creator {
                    profile_id: "not-a-uuid".into()
                })
                .await
        ),
        (StatusCode::INTERNAL_SERVER_ERROR, "The editor request failed.".to_string())
    );

    assert_eq!(editor.playtest_address().await.unwrap(), ("203.0.113.7".to_string(), 30123));
    assert_eq!(
        dev_editor.playtest_address().await.unwrap(),
        ("127.0.0.1".to_string(), EDITOR_PLAYTEST_PORT.parse().unwrap())
    );

    postgrest::from("user_profiles")
        .eq("id", &creator_id)
        .update(json!({"preferred_color": "#ff0000"}))
        .await
        .unwrap();
    let (username, user_id, profile) = database::player_profile(&creator_id).await.unwrap();
    assert_eq!((username.as_str(), user_id > 0), ("creator", true));
    assert_eq!(
        serde_json::to_value(profile).unwrap(),
        json!({"preferredColor": {"r": 1.0, "g": 0.0, "b": 0.0}, "equippedItemId": null})
    );
    assert_eq!(
        failure(database::player_profile(&Uuid::new_v4().to_string()).await),
        (StatusCode::NOT_FOUND, "Profile not found.".to_string())
    );

    let settings = database::game_settings().await.unwrap().unwrap();
    assert_eq!(
        (
            settings.game_id,
            settings.name.as_str(),
            settings.description,
            settings.suggested_min_players,
            settings.max_players,
            settings.is_published
        ),
        (game_id, "Game", None, 1, 8, Some(false))
    );
    let settings = database::set_game_settings(Some("Race".into()), Some("Fast cars".into()), Some(2), Some(12), Some(true))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            settings.name.as_str(),
            settings.description.as_deref(),
            settings.suggested_min_players,
            settings.max_players,
            settings.is_published
        ),
        ("Race", Some("Fast cars"), 2, 12, Some(true))
    );
    let settings = database::set_game_settings(None, Some(String::new()), None, None, None).await.unwrap().unwrap();
    assert_eq!((settings.name.as_str(), settings.description, settings.suggested_min_players, settings.max_players), ("Race", None, 2, 12));

    let first_version = database::insert_asset_version(None, "Red Hat", AssetType::Mesh, Uuid::new_v4(), &creator_id)
        .await
        .unwrap();
    let asset_id = first_version["asset_id"].as_i64().unwrap();
    let latest_asset_uuid = Uuid::new_v4();
    let latest_version = database::insert_asset_version(Some(asset_id), "Red Hat", AssetType::Mesh, latest_asset_uuid, &creator_id)
        .await
        .unwrap();
    assert_eq!(
        (
            &first_version["version_num"],
            &latest_version["asset_id"],
            &latest_version["version_num"],
            &latest_version["asset_type"]
        ),
        (&json!(1), &json!(asset_id), &json!(2), &json!("Mesh"))
    );
    let publisher = database::asset_publisher(asset_id).await.unwrap().unwrap();
    assert_eq!((publisher.uploaded_by, publisher.asset_type), (creator_id.clone(), AssetType::Mesh));
    assert!(database::asset_publisher(asset_id + 1000).await.unwrap().is_none());
    let latest = database::latest_asset_version(asset_id).await.unwrap().unwrap();
    assert_eq!((latest.uploaded_by, latest.asset_uuid), (creator_id.clone(), latest_asset_uuid.to_string()));
    assert!(database::latest_asset_version(asset_id + 1000).await.unwrap().is_none());
    let latest_row = database::latest_asset_row(asset_id).await.unwrap().unwrap();
    assert_eq!(
        (&latest_row["version_num"], &latest_row["asset_uuid"]),
        (&json!(2), &json!(latest_asset_uuid.to_string()))
    );
    assert!(database::latest_asset_row(asset_id + 1000).await.unwrap().is_none());
    let search = |query: &str, asset_type: Option<AssetType>| AssetSearchRequest {
        query: query.into(),
        asset_type,
        offset: 0,
        limit: 10,
    };
    let found = database::search_asset_metadata(&search("hat", Some(AssetType::Mesh))).await.unwrap();
    assert_eq!(
        found
            .iter()
            .map(|asset| (asset.asset_id, asset.version_num, asset.name.as_str()))
            .collect::<Vec<_>>(),
        [(asset_id, 2, "Red Hat")]
    );
    assert!(
        database::search_asset_metadata(&search("hat", Some(AssetType::Sound)))
            .await
            .unwrap()
            .is_empty()
    );
    database::record_asset_import(asset_id, 2).await.unwrap();
    database::record_asset_import(asset_id, 2).await.unwrap();
    #[derive(Deserialize)]
    struct ImportCount {
        import_count: i64,
    }
    let import_counts = postgrest::from("asset_metadata")
        .select("import_count")
        .eq("asset_id", asset_id)
        .order_desc("version_num")
        .get::<ImportCount>()
        .await
        .unwrap();
    assert_eq!(import_counts.iter().map(|row| row.import_count).collect::<Vec<_>>(), [2, 0]);

    let next_ids = [
        database::next_data_id().await.unwrap(),
        database::next_data_id().await.unwrap(),
        database::next_catalog_item_id().await.unwrap(),
        database::next_catalog_item_id().await.unwrap(),
        database::next_blender_id().await.unwrap(),
        database::next_blender_id().await.unwrap(),
    ];
    assert!(next_ids[0] < next_ids[1] && next_ids[2] < next_ids[3] && next_ids[4] < next_ids[5]);

    let add_mesh_data = async |uploaded_by: &str| {
        let data_id = database::next_data_id().await.unwrap();
        database::insert_data_file(&DataKind::MeshData, data_id, uploaded_by).await.unwrap();
        data_id
    };
    let own_mesh_data_ids = [add_mesh_data(&creator_id).await, add_mesh_data(&creator_id).await];
    let stranger_mesh_data_id = add_mesh_data(&stranger_id).await;
    let mixed_mesh_data_ids = [own_mesh_data_ids[0], own_mesh_data_ids[1], stranger_mesh_data_id];
    assert_eq!(database::owned_mesh_data_count(&creator_id, &mixed_mesh_data_ids).await.unwrap(), 2);

    let blender_id = database::next_blender_id().await.unwrap();
    database::insert_model_source(blender_id, &creator_id, MeshSourceFormat::Glb).await.unwrap();
    assert!(matches!(
        database::model_source_format(blender_id, &creator_id).await.unwrap(),
        Some(MeshSourceFormat::Glb)
    ));
    assert!(database::model_source_format(blender_id, &stranger_id).await.unwrap().is_none());

    let catalog_item_id = database::next_catalog_item_id().await.unwrap();
    let error = database::publish_catalog_item(catalog_item_id, &creator_id, "Hat", None, true, &mixed_mesh_data_ids)
        .await
        .err()
        .unwrap()
        .downcast::<PostgrestError>()
        .unwrap();
    assert_eq!(
        (error.user_status(), error.message.as_str()),
        (
            Some(StatusCode::FORBIDDEN),
            "Only meshes this game's creator made can be published to the catalog."
        )
    );
    let item = database::publish_catalog_item(catalog_item_id, &creator_id, "Hat", Some("A red hat"), true, &own_mesh_data_ids)
        .await
        .unwrap();
    assert_eq!(
        (item.catalog_item_id, item.name.as_str(), item.description.as_deref(), item.is_listed),
        (catalog_item_id, "Hat", Some("A red hat"), true)
    );
    let item = database::edit_catalog_item(catalog_item_id, &creator_id, Some("Cap".into()), Some(String::new()), Some(false))
        .await
        .unwrap()
        .unwrap();
    assert_eq!((item.name.as_str(), item.description, item.is_listed), ("Cap", None, false));
    assert!(
        database::edit_catalog_item(catalog_item_id, &stranger_id, Some("Stolen".into()), None, None)
            .await
            .unwrap()
            .is_none()
    );

    insert("starting_editor_servers", json!({"request_id": "replacement-editor", "game_id": game_id})).await;
    editor.remove_deployment().await.unwrap();
    dev_editor.remove_deployment().await.unwrap();
    assert_eq!(failure(editor.require_browser_access(&creator).await), stopped);
    #[derive(Deserialize)]
    struct Deployment {
        request_id: String,
    }
    let deployments = postgrest::from("starting_editor_servers")
        .select("request_id")
        .eq("game_id", game_id)
        .get::<Deployment>()
        .await
        .unwrap();
    assert_eq!(
        deployments.iter().map(|row| row.request_id.as_str()).collect::<Vec<_>>(),
        ["replacement-editor"]
    );

    let error = postgrest::call::<Value>("editor_load_world", json!({"input_game_id": -1, "input_request_id": null}))
        .await
        .unwrap_err();
    assert_eq!((error.user_status(), error.message.as_str()), (Some(StatusCode::NOT_FOUND), "Game not found."));
    let error = postgrest::call::<()>("no_such_function", json!({})).await.unwrap_err();
    assert_eq!(error.user_status(), None);
    assert!(
        error.to_string().starts_with("POST rpc/no_such_function failed (status 404 Not Found): "),
        "{error}"
    );
}
