use crate::ai_playtest_server::AI_PLAYTEST_SERVER;
use crate::api_error::{ApiError, internal_error};
use crate::auth::BrowserAccess;
use crate::database::{self, CatalogItem, EditorDatabase, GameSettings};
use crate::editor_history::{EditorHistory, EditorHistoryDirection, EditorHistoryStep, EditorTurnObjectChange, EditorTypingTarget};
use crate::editor_sync::EditorSync;
use crate::assets::{download_asset, publish_asset, read_asset_file, search_assets, upload_data_file};
use uuid::Uuid;
use crate::playtest_process::{PlaytestProcess, PlaytestRequest};
use crate::publishing::{publish_world, send_game_published_email, upload_ai_thumbnail, upload_thumbnail};
use crate::screenshot::{ScreenshotRequest, draw_png_in_child_process};
use anyhow::{Result, ensure};
use axum::{body::Bytes, http::StatusCode};
use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    data_types::{DataKind, MeshData},
    editor_sync::{EditorClientAction, EditorHistoryAvailability, EditorState, EditorStatus},
    migrateDehydratedGltf,
    object_types::{ObjectId, shared::CameraLocation},
};
use glam::Vec3;
use profanity::has_profanity;
use render_core::dehydrated_gltf::{BlenderId, DehydratedGltf};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shared::{
    api::{
        protocol_for_assets::{Asset, AssetSummary, AssetPublishRequest, AssetSearchRequest, UploadedDataWrapper},
        protocol_for_chat::{
            AssetsHandler, CatalogHandler, CatalogItemEditArgs, CatalogItemPublishArgs, ClientToolFuture, ClientToolTextFuture, GameSettingsHandler,
            GameVisibility, PublishHandler, ReadScriptFilesArgs, ScreenshotHandler, ScriptFilesHandler, SetGameSettingsArgs, ToolCall, ToolResultFinal,
            apply_editor_tool_call, check_catalog_item_is_a_hat, static_copy_of_rigged_mesh_data,
        },
    },
    env::env_vars::{EDITOR_CHAT_PORT, EDITOR_PLAYTEST_PORT, SITE_URL},
    game::{
        assets::{AssetData, add_asset_to_world, add_picture_to_world, picture_size_of_texture},
        data_fetcher::DataFetcherArc,
        world::World,
    },
    s3_storage::{PublicFile, upload_public_file},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, RwLock, RwLockWriteGuard, broadcast, oneshot, watch};
use tokio_util::sync::CancellationToken;

// One process owns one game's editable world. User and AI playtests take independent snapshots.
pub struct EditorServer {
    pub owner_id: String,
    pub token: String,
    pub database: EditorDatabase,
    pub http: reqwest::Client,
    pub current_world_arc: Arc<RwLock<World>>,
    pub sync: EditorSync,
    pub editor_status_tx: watch::Sender<EditorStatus>,
    pub data_fetcher: DataFetcherArc,
    pub user_playtest_server: Mutex<Option<PlaytestProcess>>,
    pub history: Mutex<EditorHistory>,
    pub tool_lock: Arc<Mutex<()>>,
    pub last_activity_at: Mutex<Instant>,
    pub last_publish_at: Mutex<Option<Instant>>,
    // Set by the editor menu's Refresh AI Thumbnail, so the next publish redraws the AI thumbnail.
    pub should_refresh_ai_thumbnail: AtomicBool,
    // Set when the editor stops itself on purpose, and shown to every browser it closes.
    pub pause_reason: Mutex<Option<&'static str>>,
    // Each login or invite header that passed the access check, and when.
    pub browser_access_cache: Mutex<HashMap<String, (BrowserAccess, Instant)>>,
    pub unsaved_changes_notify: Notify,
    pub last_saved_world: Mutex<Value>,
    pub shutdown_token: CancellationToken,
}
impl EditorServer {
    // Connection setup and publication share the world lock; callers never handle its guard.
    pub async fn connect_client(&self) -> Result<(EditorState, broadcast::Receiver<Bytes>)> {
        let mut world_lock = self.current_world_arc.write().await;
        let connection = self.sync.connect_client(&mut world_lock);
        drop(world_lock);
        connection
    }

    pub async fn broadcast_changes(&self) -> Result<()> {
        let mut world_lock = self.current_world_arc.write().await;
        let result = self.sync.broadcast_changes(&mut world_lock);
        drop(world_lock);
        result
    }

    // Every save serializes the world while its caller holds the lock, preserving edit order. A world equal to the last save
    // isn't written again. It's compared as JSON values, since its maps list their keys in a different order each time.
    pub async fn save_world(&self, world: &World) -> Result<(), ApiError> {
        let stored_world = world.toStoredWorld();
        let stored_world_value = serde_json::to_value(&stored_world).map_err(internal_error)?;
        let mut last_saved_world = self.last_saved_world.lock().await;
        if *last_saved_world == stored_world_value {
            return Ok(());
        }
        self.database.save_world(&serde_json::to_string_pretty(&stored_world).map_err(internal_error)?).await?;
        *last_saved_world = stored_world_value;
        Ok(())
    }

    // Shutdown saves the world under this same lock, so an edit either lands before that final save or is refused.
    pub async fn lock_world_for_edit(&self) -> Result<RwLockWriteGuard<'_, World>, ApiError> {
        let world_lock = self.current_world_arc.write().await;
        if self.shutdown_token.is_cancelled() {
            drop(world_lock);
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "The editor is shutting down."));
        }
        Ok(world_lock)
    }

    // One browser action is one undo step, so its deltas are validated, applied, and recorded
    // together under the world lock. Their setters wake publication once the lock is released.
    // Answers with the number of the first batch that can include the action.
    pub async fn apply_client_action(&self, author: String, action: EditorClientAction) -> Result<u64, ApiError> {
        let mut world_lock = self.lock_world_for_edit().await?;
        // Typing sends one delta per keystroke, so a run in one field continues its own entry.
        let typing = match action.deltas.as_slice() {
            [delta] => match &delta.value {
                Value::String(_) => Some(EditorTypingTarget {
                    object_id: delta.objectId,
                    section: delta.section.clone(),
                    path: delta.path.clone(),
                }),
                _ => None,
            },
            _ => None,
        };
        let mut history_lock = self.history.lock().await;
        history_lock.start_person_action(&mut world_lock);
        // Apply every delta before recording, so one action is one step even when a later delta is
        // rejected: whatever did land is still measured and still belongs to this session.
        let mut applied = Ok(());
        for delta in action.deltas {
            applied = EditorSync::apply_client_delta(&mut world_lock, delta);
            if applied.is_err() {
                break;
            }
        }
        history_lock.record_person_action(&mut world_lock, author, action.description, typing);
        self.publish_history_availability(&history_lock);
        drop(history_lock);
        let seq = self.sync.next_seq();
        drop(world_lock);
        applied.map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
        self.unsaved_changes_notify.notify_one();
        Ok(seq)
    }

    // Adding an asset from the browser is one undo step, recorded under the same locks as a
    // browser action. It skips the tool lock, so it still works while the AI is running.
    pub async fn insert_asset(&self, author: Option<String>, name: String, data: AssetData) -> Result<ObjectId, ApiError> {
        let description = format!("Add {name}");
        // A picture is sized to its texture, which has to download before the world is locked.
        if let AssetData::Texture(texture_data_id) = data {
            let picture_size = picture_size_of_texture(&self.data_fetcher, texture_data_id).await;
            return self
                .insert_objects(author, description, |world| {
                    Ok(add_picture_to_world(world, name, texture_data_id, picture_size))
                })
                .await;
        }
        self.insert_objects(author, description, |world| add_asset_to_world(world, name, data)).await
    }

    pub async fn record_asset_import(&self, asset_id: i64, version_num: i64) -> Result<()> {
        database::record_asset_import(asset_id, version_num).await
    }

    // A model asset's source is the file its latest version was made from.
    pub async fn read_asset_blender_id(&self, asset_id: i64) -> Result<BlenderId> {
        let asset = download_asset(asset_id).await?.ok_or_else(|| anyhow::anyhow!("Asset {asset_id} not found."))?;
        let blender_id = match asset.data_json {
            AssetData::Mesh(mesh) => mesh.blender_id,
            _ => None,
        };
        blender_id.ok_or_else(|| anyhow::anyhow!("Asset {asset_id} has no saved source to edit. Models uploaded before uploads kept their file can't be edited."))
    }

    // A person's addition is one undo step of their session. The AI chat's, which has no author, waits its turn with
    // the tools and belongs to its reply, the same as a tool's change.
    async fn insert_objects(
        &self,
        author: Option<String>,
        description: String,
        add_objects: impl FnOnce(&mut World) -> Result<ObjectId>,
    ) -> Result<ObjectId, ApiError> {
        let tool_lock = match author {
            Some(_) => None,
            None => Some(self.tool_lock.lock().await),
        };
        let mut world_lock = self.lock_world_for_edit().await?;
        let mut history_lock = self.history.lock().await;
        let added = match author {
            Some(author) => {
                history_lock.start_person_action(&mut world_lock);
                let added = add_objects(&mut world_lock);
                history_lock.record_person_action(&mut world_lock, author, description, None);
                added
            }
            None => {
                let reply_number = history_lock.begin_tool_call(&mut world_lock);
                let added = add_objects(&mut world_lock);
                history_lock.end_tool_call(&mut world_lock, reply_number);
                added
            }
        };
        self.publish_history_availability(&history_lock);
        drop(history_lock);
        drop(world_lock);
        drop(tool_lock);
        self.unsaved_changes_notify.notify_one();
        added.map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
    }

    // An uploaded world replaces everything under the root. Nothing before it can be undone, so every
    // session's history starts over, and the world is saved at once. Once the world is replaced the
    // upload has happened, so a failed save is left to the background save.
    pub async fn upload_world(&self, uploaded_world: World) -> Result<(), ApiError> {
        let ai_is_running = || ApiError::new(StatusCode::CONFLICT, "Uploading is off while the AI is making changes.");
        if self.editor_status_tx.borrow().is_chat_running {
            return Err(ai_is_running());
        }
        let tool_lock = self
            .tool_lock
            .try_lock()
            .map_err(|_| ApiError::new(StatusCode::CONFLICT, "Another tool is running."))?;
        let mut world_lock = self.lock_world_for_edit().await?;
        let mut history_lock = self.history.lock().await;
        // Starting a chat sets the flag before it takes the world lock, but ending one clears the flag
        // before it closes the turn, so an open turn also counts as the AI running.
        if self.editor_status_tx.borrow().is_chat_running || history_lock.is_chat_turn_open() {
            drop(history_lock);
            drop(world_lock);
            drop(tool_lock);
            return Err(ai_is_running());
        }
        world_lock.loadNewWorld(&uploaded_world).map_err(internal_error)?;
        *history_lock = EditorHistory::new(&mut world_lock);
        self.publish_history_availability(&history_lock);
        drop(history_lock);
        let saved = self.save_world(&world_lock).await;
        drop(world_lock);
        drop(tool_lock);
        if let Err(error) = saved {
            log::error!("Could not save the uploaded world: {error}");
            self.unsaved_changes_notify.notify_one();
        }
        Ok(())
    }

    // Undo and redo are ordinary world changes: they take the world lock, mutate, wake the background
    // save, and let the same publication loop broadcast to every browser including the one that asked.
    pub async fn take_history_step(&self, author: &str, direction: EditorHistoryDirection) -> Result<Option<EditorHistoryStep>, ApiError> {
        let mut world_lock = self.lock_world_for_edit().await?;
        let mut history_lock = self.history.lock().await;
        // A step that failed partway may still have changed the world, so it is saved either way.
        let step = history_lock.take_step(&mut world_lock, author, direction);
        self.publish_history_availability(&history_lock);
        drop(history_lock);
        let step = step.inspect_err(|_| self.unsaved_changes_notify.notify_one()).map_err(internal_error)?;
        if step.is_none() {
            drop(world_lock);
            return Ok(None);
        }
        drop(world_lock);
        self.unsaved_changes_notify.notify_one();
        Ok(step)
    }

    // Capturing the world at both ends of a reply means no individual tool has to know how to describe
    // its own inverse. A tool still finishing after Stop is the AI's work too, so the reply closes after it.
    // Browsers learn a reply has stopped only once it is closed.
    pub async fn set_chat_status(&self, is_chat_running: bool) -> Vec<EditorTurnObjectChange> {
        if is_chat_running {
            self.editor_status_tx.send_modify(|editor_status| editor_status.is_chat_running = true);
        }
        let tool_lock = match is_chat_running {
            true => None,
            false => {
                let tool_lock = self.tool_lock.lock().await;
                AI_PLAYTEST_SERVER.stop_all();
                Some(tool_lock)
            }
        };
        let mut world_lock = self.current_world_arc.write().await;
        let mut history_lock = self.history.lock().await;
        let object_changes = match is_chat_running {
            true => {
                history_lock.begin_chat_turn(&mut world_lock);
                Vec::new()
            }
            false => history_lock.end_chat_turn(&mut world_lock),
        };
        self.publish_history_availability(&history_lock);
        if !is_chat_running {
            self.editor_status_tx.send_modify(|editor_status| editor_status.is_chat_running = false);
        }
        drop(history_lock);
        drop(world_lock);
        drop(tool_lock);
        object_changes
    }

    // Every browser learns whether its own session has anything to undo or redo, without learning anyone
    // else's session.
    fn publish_history_availability(&self, history: &EditorHistory) {
        let history_of_session: HashMap<String, EditorHistoryAvailability> = history
            .read_availability()
            .into_iter()
            .map(|(author, availability)| (format!("{:x}", Sha256::digest(author.as_bytes())), availability))
            .collect();
        self.editor_status_tx.send_if_modified(|editor_status| {
            let is_changed = editor_status.history_of_session != history_of_session;
            if is_changed {
                editor_status.history_of_session = history_of_session;
            }
            is_changed
        });
    }

    pub async fn read_chat_turn_changes(&self) -> Vec<EditorTurnObjectChange> {
        let world_lock = self.current_world_arc.read().await;
        let history_lock = self.history.lock().await;
        history_lock.read_chat_turn_changes(&world_lock)
    }

    // The editor owns tool execution, cancellation cleanup, and saving.
    // Tools take turns: a call waits for the one before it, since callers like an MCP client cannot see it running.
    pub async fn run_tool_call(self: Arc<Self>, call: ToolCall) -> Result<oneshot::Receiver<ToolResultFinal>, ApiError> {
        let editor_arc = self;
        let tool_guard = editor_arc.tool_lock.clone().lock_owned().await;
        // Shutdown takes the tool lock before its final save, so no tool starts once shutdown has begun.
        if editor_arc.shutdown_token.is_cancelled() {
            drop(tool_guard);
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "The editor is shutting down."));
        }
        let (mut result_tx, result_rx) = oneshot::channel();
        tokio::spawn(async move {
            let mut world_lock = editor_arc.current_world_arc.write().await;
            let mut history_lock = editor_arc.history.lock().await;
            let reply_number = history_lock.begin_tool_call(&mut world_lock);
            drop(history_lock);
            drop(world_lock);
            // Drop cancelled tool futures before saving; they may own a world lock.
            let result = tokio::select! {
                _ = result_tx.closed() => None,
                _ = editor_arc.shutdown_token.cancelled() => None,
                result = apply_editor_tool_call(
                    editor_arc.current_world_arc.clone(),
                    editor_arc.data_fetcher.clone(),
                    Some(editor_arc.token.clone()),
                    call,
                ) => Some(result),
            };
            // Whatever the tool changed is the AI's, even if it was cancelled partway.
            let mut world_lock = editor_arc.current_world_arc.write().await;
            let mut history_lock = editor_arc.history.lock().await;
            history_lock.end_tool_call(&mut world_lock, reply_number);
            editor_arc.publish_history_availability(&history_lock);
            drop(history_lock);
            drop(world_lock);
            // A cancelled tool may still have changed the world, so always ask for a save.
            editor_arc.unsaved_changes_notify.notify_one();
            *editor_arc.last_activity_at.lock().await = Instant::now();
            // The next tool may start immediately; the background loop writes the world.
            drop(tool_guard);
            if let Some(result) = result {
                let _ = result_tx.send(result);
            }
        });
        Ok(result_rx)
    }

    pub async fn start_user_playtest_server(self: &Arc<Self>) -> Result<()> {
        let mut user_playtest_server_lock = self.user_playtest_server.lock().await;
        anyhow::ensure!(
            user_playtest_server_lock.as_ref().is_none_or(PlaytestProcess::has_stopped),
            "Stop the current playtest before starting another."
        );
        let stored = self.current_world_arc.read().await.toStoredWorldString()?;
        let (process, _) = PlaytestProcess::start(&std::env::current_exe()?, stored, Some(EDITOR_PLAYTEST_PORT.parse()?), |_, _, _| {}).await?;
        let stopped = process.stopped();
        *user_playtest_server_lock = Some(process);
        self.editor_status_tx.send_modify(|editor_status| editor_status.is_playtest_running = true);
        drop(user_playtest_server_lock);
        let editor_weak = Arc::downgrade(self);
        tokio::spawn(async move {
            // Observe the actual worker too, so an unexpected stop reaches every browser.
            stopped.await;
            if let Some(editor_arc) = editor_weak.upgrade() {
                let mut server_lock = editor_arc.user_playtest_server.lock().await;
                if server_lock.as_ref().is_some_and(PlaytestProcess::has_stopped) {
                    *server_lock = None;
                    editor_arc
                        .editor_status_tx
                        .send_modify(|editor_status| editor_status.is_playtest_running = false);
                }
                drop(server_lock);
            }
        });
        Ok(())
    }
    pub async fn stop_user_playtest_server(&self) {
        let mut user_playtest_server_lock = self.user_playtest_server.lock().await;
        if let Some(process) = user_playtest_server_lock.take() {
            process.stop().await;
        }
        self.editor_status_tx.send_modify(|editor_status| editor_status.is_playtest_running = false);
        drop(user_playtest_server_lock);
    }
    pub async fn has_playtest_players(&self) -> bool {
        let server_lock = self.user_playtest_server.lock().await;
        let Some(process) = server_lock.as_ref() else {
            return false;
        };
        let has_players = process.request(PlaytestRequest::HasPlayers).await.unwrap_or(false);
        drop(server_lock);
        has_players
    }
}

impl ScreenshotHandler for EditorServer {
    fn capture<'a>(&'a self, camera_position: Vec3, target: Vec3, width: u32, height: u32) -> ClientToolFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let request = ScreenshotRequest::of_editor_world(&self.current_world_arc, &self.data_fetcher, camera_position, target, width, height).await?;
            draw_png_in_child_process(&std::env::current_exe()?, &request).await
        })
    }
}

// -------------------------- Database tools --------------------------

// Keep the settings tool's JSON result unchanged for both reads and updates.
fn game_settings_text(settings: Option<GameSettings>) -> Result<String> {
    let settings = settings.ok_or_else(|| anyhow::anyhow!("Game not found."))?;
    let visibility = if settings.is_published.unwrap_or(false) {
        GameVisibility::Published
    } else {
        GameVisibility::Private
    };
    Ok(serde_json::to_string_pretty(&json!({
        "game_id": settings.game_id,
        "title": settings.name,
        "description": settings.description.unwrap_or_default(),
        "suggested_min_players": settings.suggested_min_players,
        "max_players": settings.max_players,
        "visibility": visibility,
    }))?)
}

// Game settings are game metadata. Publishing a world remains in Rust's publish tool.
impl GameSettingsHandler for EditorServer {
    fn get<'a>(&'a self) -> ClientToolTextFuture<'a> {
        Box::pin(async move { game_settings_text(database::game_settings().await?) })
    }

    fn set<'a>(&'a self, mut args: SetGameSettingsArgs) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            args.validate()?;
            ensure!(!args.title.as_deref().is_some_and(has_profanity), "That game title is not allowed.");
            ensure!(!args.description.as_deref().is_some_and(has_profanity), "That description is not allowed.");
            // When only one end of the player range changes, it is checked against the other end's current value.
            if args.suggested_min_players.is_some() != args.max_players.is_some() {
                let settings = database::game_settings().await?.ok_or_else(|| anyhow::anyhow!("Game not found."))?;
                let suggested_min_players = args.suggested_min_players.unwrap_or(i64::from(settings.suggested_min_players));
                let max_players = args.max_players.unwrap_or(i64::from(settings.max_players));
                ensure!(suggested_min_players <= max_players, "suggested_min_players can't be more than max_players.");
            }
            let is_published = args.visibility.map(|visibility| match visibility {
                GameVisibility::Private => false,
                GameVisibility::Published => true,
            });
            game_settings_text(database::set_game_settings(args.title, args.description, args.suggested_min_players, args.max_players, is_published).await?)
        })
    }
}

// Both catalog tools report the item the same way.
fn catalog_item_text(item: Option<CatalogItem>) -> Result<String> {
    let item = item.ok_or_else(|| anyhow::anyhow!("Catalog item not found, or this game's creator didn't publish it."))?;
    Ok(serde_json::to_string_pretty(&json!({
        "catalog_item_id": item.catalog_item_id,
        "url": format!("{}/catalog/{}", SITE_URL.as_str(), item.catalog_item_id),
        "name": item.name,
        "description": item.description.unwrap_or_default(),
        "is_listed": item.is_listed,
    }))?)
}

// The game's creator sells what is published here. Only assets and meshes they uploaded can be published, so nobody sells
// a copy of someone else's model.
impl CatalogHandler for EditorServer {
    fn publish<'a>(&'a self, mut args: CatalogItemPublishArgs) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            args.validate()?;
            ensure!(!has_profanity(&args.name), "That name is not allowed.");
            ensure!(!args.description.as_deref().is_some_and(has_profanity), "That description is not allowed.");
            // The item copies the asset's latest revision, brought up to the current format like any asset that's read.
            let asset_id = args.asset_id;
            let asset_version = database::latest_asset_version(asset_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Asset {asset_id} not found."))?;
            ensure!(asset_version.uploaded_by == self.owner_id, "Only the creator of an asset can sell it in the catalog.");
            let mut data_json = read_asset_file(Uuid::parse_str(&asset_version.asset_uuid)?).await?;
            let Some(mesh_json) = data_json.get_mut("Mesh") else {
                anyhow::bail!("Only Mesh assets can become catalog items.");
            };
            migrateDehydratedGltf::updateToVersion(mesh_json, DEHYDRATED_GLTF_CURRENT_VERSION)?;
            let mut mesh = serde_json::from_value::<DehydratedGltf>(mesh_json.take())?;
            mesh.validate()?;
            let source_mesh_data_ids = mesh
                .dehydrated_meshes
                .iter()
                .map(|dehydrated_mesh| dehydrated_mesh.mesh_data_id)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            // Checked before anything is copied, because the static copies below are uploaded as the game creator's own.
            ensure!(
                database::owned_mesh_data_count(&self.owner_id, &source_mesh_data_ids).await? == source_mesh_data_ids.len(),
                "Only meshes this game's creator made can be published to the catalog."
            );
            let (mesh_data_of_id, _) = self
                .data_fetcher
                .waitForMeshAndTextureData(source_mesh_data_ids.iter().copied().collect(), HashSet::new())
                .await;
            check_catalog_item_is_a_hat(&mesh, &mesh_data_of_id)?;
            // Catalog items are static, so each rigged mesh is drawn from a static copy of its geometry, uploaded for this item,
            // and the item keeps no skeleton, joints or animations.
            let mut static_mesh_data_id_of_rigged_id = HashMap::new();
            for (mesh_data_id, mesh_data) in &mesh_data_of_id {
                let Some(static_mesh_data) = static_copy_of_rigged_mesh_data(mesh_data) else {
                    continue;
                };
                let bytes = bincode::serialize(&UploadedDataWrapper::<MeshData>::new(static_mesh_data))?;
                static_mesh_data_id_of_rigged_id.insert(*mesh_data_id, upload_data_file(&DataKind::MeshData, bytes, &self.owner_id).await?);
            }
            for dehydrated_mesh in &mut mesh.dehydrated_meshes {
                if let Some(static_mesh_data_id) = static_mesh_data_id_of_rigged_id.get(&dehydrated_mesh.mesh_data_id) {
                    dehydrated_mesh.mesh_data_id = *static_mesh_data_id;
                }
                dehydrated_mesh.skinned_skeleton_index = None;
            }
            mesh.dehydrated_skeletons.clear();
            let mesh_data_ids = mesh
                .dehydrated_meshes
                .iter()
                .map(|dehydrated_mesh| dehydrated_mesh.mesh_data_id)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            // The item's file goes up under its id before the row that lists it is written.
            let catalog_item_id = database::next_catalog_item_id().await?;
            upload_public_file(&PublicFile::Catalog { catalog_item_id }, serde_json::to_vec(&AssetData::Mesh(mesh))?).await?;
            let item =
                database::publish_catalog_item(catalog_item_id, &self.owner_id, &args.name, args.description.as_deref(), args.is_listed, &mesh_data_ids)
                    .await?;
            catalog_item_text(Some(item))
        })
    }

    fn edit<'a>(&'a self, mut args: CatalogItemEditArgs) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            args.validate()?;
            ensure!(!args.name.as_deref().is_some_and(has_profanity), "That name is not allowed.");
            ensure!(!args.description.as_deref().is_some_and(has_profanity), "That description is not allowed.");
            catalog_item_text(database::edit_catalog_item(args.catalog_item_id, &self.owner_id, args.name, args.description, args.is_listed).await?)
        })
    }
}

// Reuse the asset implementation in this process; the shared tool registry needs no HTTP credentials.
impl AssetsHandler for EditorServer {
    fn publish<'a>(&'a self, request: AssetPublishRequest) -> ClientToolFuture<'a, Asset> {
        Box::pin(publish_asset(&self.owner_id, request))
    }

    fn search<'a>(&'a self, request: &'a AssetSearchRequest) -> ClientToolFuture<'a, Vec<AssetSummary>> {
        Box::pin(search_assets(request))
    }

    fn download<'a>(&'a self, asset_id: i64) -> ClientToolFuture<'a, Asset> {
        Box::pin(async move { download_asset(asset_id).await?.ok_or_else(|| anyhow::anyhow!("Asset not found")) })
    }

    fn record_import<'a>(&'a self, asset_id: i64, version_num: i64) -> ClientToolFuture<'a, ()> {
        Box::pin(self.record_asset_import(asset_id, version_num))
    }
}

// TODO: AI-PICKED-VALUE: A game can be published at most once every 5 seconds, counted from the start of each publish.
const TIME_BETWEEN_PUBLISHES: Duration = Duration::from_secs(5);

// Publishing uses the same world serialization and thumbnail upload as the desktop editor did.
impl PublishHandler for EditorServer {
    fn publish<'a>(&'a self) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            // A publish right after another one is refused, so a burst of publishes can't pile up versions and uploads.
            let mut last_publish_at_lock = self.last_publish_at.lock().await;
            ensure!(
                last_publish_at_lock.is_none_or(|last_publish_at| last_publish_at.elapsed() >= TIME_BETWEEN_PUBLISHES),
                "The game was just published. Wait a few seconds before publishing again."
            );
            *last_publish_at_lock = Some(Instant::now());
            drop(last_publish_at_lock);
            // The thumbnail is taken from the game's fallback camera.
            let CameraLocation { position, orientation } = self.current_world_arc.read().await.fallback_camera_location;
            // Capture first so a failed screenshot cannot fail a publish after creating its version.
            // TODO: AI-PICKED-VALUE: Publish thumbnails are 1920x1080, the game-card aspect ratio at full HD so they stay sharp on large and retina screens.
            let png_bytes = self.capture(position, position + orientation.to_unit_direction(), 1920, 1080).await?;
            let stored_world_string = self.current_world_arc.read().await.toStoredWorldString()?;
            let version_num = publish_world(&stored_world_string).await?;
            // The database numbers each game's versions from 1, so version 1 is the game's first publish.
            if version_num == 1 {
                let owner_id = self.owner_id.clone();
                tokio::spawn(async move {
                    if let Err(error) = send_game_published_email(&owner_id).await {
                        log::error!("Could not email the creator that their game is published: {error:?}");
                    }
                });
            }
            upload_thumbnail(png_bytes.clone()).await?;
            // A game gets an AI thumbnail on its own only once. After that it changes only at the first publish after someone refreshes it.
            // Nothing about it can fail the publish.
            let should_refresh_ai_thumbnail = self.should_refresh_ai_thumbnail.swap(false, Ordering::Relaxed);
            let http = self.http.clone();
            let token = self.token.clone();
            tokio::spawn(async move {
                let result = match (should_refresh_ai_thumbnail, database::game_ai_thumbnail_uuid().await) {
                    (true, _) | (false, Ok(None)) => upload_ai_thumbnail(&http, &token, &png_bytes).await,
                    (false, Ok(Some(_))) => Ok(()),
                    (false, Err(error)) => Err(error),
                };
                if let Err(error) = result {
                    log::error!("Could not make the AI thumbnail: {error:?}");
                }
            });
            Ok(format!("Published version {version_num}."))
        })
    }
}

// Rust reads script text; Node only typechecks snapshots through its authenticated local-only route.
impl ScriptFilesHandler for EditorServer {
    fn read_text<'a>(&'a self, args: ReadScriptFilesArgs) -> ClientToolTextFuture<'a> {
        Box::pin(async move {
            #[derive(serde::Serialize)]
            #[allow(non_snake_case)]
            struct ScriptFile {
                objectId: ObjectId,
                path: String,
                text: String,
            }
            let world_lock = self.current_world_arc.read().await;
            let mut scripts = Vec::new();
            for (id, text) in &world_lock.scriptRawCodeOfId {
                let path = world_lock.getObjectNamePath(*id)?;
                scripts.push(ScriptFile {
                    objectId: *id,
                    path,
                    text: text.clone(),
                });
            }
            drop(world_lock);
            let should_check_diagnostics = args.include_diagnostics != Some(false) || args.only_files_with_diagnostics == Some(true);
            let mut diagnostics_of_id = if should_check_diagnostics {
                // All scripts are needed to resolve imports, even when the tool requests only one path.
                self.http
                    .post(format!("http://127.0.0.1:{}/local-only/read-script-diagnostics", EDITOR_CHAT_PORT.as_str()))
                    .bearer_auth(&self.token)
                    .json(&scripts)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<HashMap<ObjectId, Vec<Value>>>()
                    .await?
            } else {
                HashMap::new()
            };
            let mut files = Vec::new();
            for script in scripts {
                if args.path_substring.as_ref().is_some_and(|path| !script.path.contains(path)) {
                    continue;
                }
                let diagnostics = if should_check_diagnostics {
                    diagnostics_of_id
                        .remove(&script.objectId)
                        .ok_or_else(|| anyhow::anyhow!("Missing script diagnostics for {}", script.objectId))?
                } else {
                    Vec::new()
                };
                if args.only_files_with_diagnostics == Some(true) && diagnostics.is_empty() {
                    continue;
                }
                let mut file = json!({"objectId":script.objectId,"scriptWorld":"editor","path":script.path});
                if args.include_text != Some(false) {
                    file["text"] = json!(script.text);
                }
                if args.include_diagnostics != Some(false) {
                    file["diagnostics"] = json!(diagnostics);
                }
                files.push(file);
            }
            Ok(serde_json::to_string(&json!({"files":files}))?)
        })
    }
}
