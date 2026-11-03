use crate::editor_history::{EditorHistoryDirection, EditorTurnObjectChange};
use crate::{
    api_error::{ApiError, internal_error},
    asset_uploads::{convert_uploaded_asset_to_publish_data, convert_uploaded_material_to_publish_data},
    assets::{dehydrate_publish_data, download_asset},
    auth::{BrowserAccess, require_server_token},
    database,
    editor::EditorServer,
    model_sources::{download_model_source, save_model_source},
    playtest_process::PlaytestRequest,
};
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Multipart, Path, Query, Request, State, WebSocketUpgrade,
        multipart::MultipartError,
        rejection::BytesRejection,
        ws::{CloseFrame, Message, WebSocket, close_code},
    },
    http::{HeaderMap, HeaderName, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use game_types::{
    consts::AI_CHAT_COMPACTION_INPUT_TOKENS,
    editor_sync::EditorClientAction,
    object_types::{ObjectId, shared::CameraLocation},
};
use linux_sandbox::MeshSourceFormat;
use serde::Deserialize;
use serde_json::{Value, json};
use shared::{
    api::{
        protocol_for_assets::AssetPublishData,
        protocol_for_chat::{EDITOR_TOOL_REGISTRY, ToolCall, ToolResultFinal},
        system_prompt::SYSTEM_PROMPT,
    },
    env::env_vars::{SITE_URL, SUPABASE_SECRET_KEY, SUPABASE_URL},
    game::{
        assets::{AssetType, publish_to_assets},
        world::{World, WorldControlMode},
    },
    player_profile::PlayerProfile,
};
use std::{
    process::Stdio,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tower_http::cors::CorsLayer;

// The browser names its own editing session here, so cross-origin requests must be allowed to send it.
const EDITOR_SESSION_HEADER: HeaderName = HeaderName::from_static("x-editor-session");

// TODO: AI-PICKED-VALUE: Close unauthenticated world sockets after thirty seconds.
const WORLD_SOCKET_AUTH_TIMEOUT: Duration = Duration::from_secs(30);
// TODO: AI-PICKED-VALUE: Recheck world subscriptions once a minute.
const BROWSER_ACCESS_CHECK_INTERVAL: Duration = Duration::from_secs(60);
// TODO: AI-PICKED-VALUE: Trust a passed access check for 30 seconds, so undo and edits skip the login and
// database round trips. A revoked invite or a signed-out login keeps working for at most that long.
const BROWSER_ACCESS_CACHE_DURATION: Duration = Duration::from_secs(30);

// Rust owns worlds, playtests, and tools. The browser calls Node directly for AI SDK chat.
async fn require_editor_access(State(editor_arc): State<Arc<EditorServer>>, mut request: Request, next: Next) -> Result<Response, ApiError> {
    let authorization = request.headers().get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).unwrap_or("");
    let access = authenticate_browser(&editor_arc, authorization).await?;
    request.extensions_mut().insert(access);
    Ok(next.run(request).await)
}

// Supabase checks a login and says which account it belongs to.
async fn get_profile_id_of_login(editor: &EditorServer, authorization: &str) -> Result<String, ApiError> {
    #[derive(Deserialize)]
    struct SupabaseUser {
        id: String,
    }
    let response = editor
        .http
        .get(format!("{}/auth/v1/user", SUPABASE_URL.as_str()))
        .header("apikey", SUPABASE_SECRET_KEY.as_str())
        .header(header::AUTHORIZATION, authorization)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "Please log in again."));
    }
    Ok(response.json::<SupabaseUser>().await?.id)
}

// HTTP requests and the world socket use the same creator login or game invitation check.
async fn authenticate_browser(editor: &EditorServer, authorization: &str) -> Result<BrowserAccess, ApiError> {
    if editor.shutdown_token.is_cancelled() {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "The editor is shutting down."));
    }
    let cached = editor
        .browser_access_cache
        .lock()
        .await
        .get(authorization)
        .filter(|(_, checked_at)| checked_at.elapsed() < BROWSER_ACCESS_CACHE_DURATION)
        .map(|(access, _)| access.clone());
    if let Some(access) = cached {
        *editor.last_activity_at.lock().await = Instant::now();
        return Ok(access);
    }
    let access = if let Some(invite_token) = authorization.strip_prefix("Invite ").filter(|token| !token.is_empty()) {
        BrowserAccess::Guest {
            invite_token: invite_token.into(),
        }
    } else {
        if !authorization.starts_with("Bearer ") || authorization.len() <= 7 {
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "Please log in or use an invite link."));
        }
        BrowserAccess::Creator {
            profile_id: get_profile_id_of_login(editor, authorization).await?,
        }
    };
    editor.database.require_browser_access(&access).await?;
    let mut cache = editor.browser_access_cache.lock().await;
    cache.retain(|_, (_, checked_at)| checked_at.elapsed() < BROWSER_ACCESS_CACHE_DURATION);
    cache.insert(authorization.to_string(), (access.clone(), Instant::now()));
    drop(cache);
    *editor.last_activity_at.lock().await = Instant::now();
    Ok(access)
}

async fn require_worker(State(editor_arc): State<Arc<EditorServer>>, request: Request, next: Next) -> Result<Response, ApiError> {
    require_server_token(request.headers(), &editor_arc.token)?;
    if editor_arc.shutdown_token.is_cancelled() {
        return Err(ApiError::new(StatusCode::GONE, "The editor is shutting down."));
    }
    if request.uri().path() != "/health" {
        *editor_arc.last_activity_at.lock().await = Instant::now();
    }
    Ok(next.run(request).await)
}

// The browser sends its Bearer login or Invite credential first. The server replies with the complete shared
// EditorState, then binary EditorServerDelta batches for world and worker state changes.
async fn editor_server_websocket(State(editor_arc): State<Arc<EditorServer>>, upgrade: WebSocketUpgrade) -> Response {
    async fn stream_world(editor: &EditorServer, socket: &mut WebSocket) -> anyhow::Result<()> {
        let authorization = tokio::time::timeout(WORLD_SOCKET_AUTH_TIMEOUT, socket.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("World connection closed before authentication"))??;
        let Message::Text(authorization) = authorization else {
            anyhow::bail!("Expected browser authorization");
        };
        let access = authenticate_browser(editor, &authorization).await?;
        let (initial_state, mut deltas_rx) = editor.connect_client().await?;
        let initial_json = serde_json::to_string(&initial_state)?;
        socket.send(Message::Text(initial_json.into())).await?;
        let mut access_check = tokio::time::interval(BROWSER_ACCESS_CHECK_INTERVAL);
        access_check.tick().await;
        loop {
            tokio::select! {
                _ = access_check.tick() => editor.database.require_browser_access(&access).await?,
                // A missed broadcast closes the socket; continuing would leave the browser out of sync.
                deltas = deltas_rx.recv() => socket.send(Message::Binary(deltas?)).await?,
                incoming = socket.recv() => match incoming {
                    None | Some(Ok(Message::Close(_))) => return Ok(()),
                    // After its credential, the browser sends a message whenever the person does something.
                    Some(Ok(Message::Text(_))) => *editor.last_activity_at.lock().await = Instant::now(),
                    Some(message) => { message?; }
                },
            }
        }
    }
    upgrade.on_upgrade(move |mut socket| async move {
        tokio::select! {
            _ = editor_arc.shutdown_token.cancelled() => {},
            result = stream_world(&editor_arc, &mut socket) => {
                if let Err(error) = result {
                    log::warn!("World connection closed: {error}");
                }
                return;
            },
        }
        let pause_reason = *editor_arc.pause_reason.lock().await;
        if let Some(pause_reason) = pause_reason {
            let close_frame = CloseFrame {
                code: close_code::AWAY,
                reason: pause_reason.into(),
            };
            if let Err(error) = socket.send(Message::Close(Some(close_frame))).await {
                log::warn!("Could not tell the browser the editor paused: {error}");
            }
        }
    })
}

// Every guest shares one invitation token, so the browser names its own editing session and the
// history attributes entries to that. One tab is one undo lane, for the creator and guests alike.
fn read_editor_session(headers: &HeaderMap) -> Result<String, ApiError> {
    let session = headers.get(EDITOR_SESSION_HEADER).and_then(|value| value.to_str().ok()).unwrap_or("");
    if session.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "This request is missing its editor session."));
    }
    Ok(session.to_string())
}

// A browser or MCP client sends files with browser access, and what they add is one undo step of their session. The
// AI chat sends files from its sandbox with the deployment credential, and what it adds belongs to its reply.
fn read_author(access: Option<Extension<BrowserAccess>>, headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    access.map(|_| read_editor_session(headers)).transpose()
}

// Browser property edits and script saves use the same validated mutation path.
// Finish persistence and publication even if the initiating browser disconnects.
async fn apply_client_action(
    State(editor_arc): State<Arc<EditorServer>>,
    headers: HeaderMap,
    Json(action): Json<EditorClientAction>,
) -> Result<Json<Value>, ApiError> {
    let author = read_editor_session(&headers)?;
    let seq = tokio::spawn(async move { editor_arc.apply_client_action(author, action).await })
        .await
        .map_err(internal_error)??;
    Ok(Json(json!({ "seq": seq })))
}

// Undo and redo belong to the session that asked. The changed world reaches every browser through
// the ordinary broadcast; the step's description and the objects it put back come back here, and this
// browser selects them.
async fn take_history_step(
    State(editor_arc): State<Arc<EditorServer>>,
    headers: HeaderMap,
    Path(direction): Path<EditorHistoryDirection>,
) -> Result<Json<Value>, ApiError> {
    let author = read_editor_session(&headers)?;
    let step = tokio::spawn(async move { editor_arc.take_history_step(&author, direction).await })
        .await
        .map_err(internal_error)??;
    Ok(Json(match step {
        Some(step) => json!({ "description": step.description, "object_ids": step.object_ids }),
        None => json!({ "description": null, "object_ids": [] }),
    }))
}

// The game's fallback camera: the thumbnail is taken from it, the editor opens at it, and the
// published game falls back to it.
async fn set_fallback_camera(State(editor_arc): State<Arc<EditorServer>>, Json(camera): Json<CameraLocation>) -> Result<StatusCode, ApiError> {
    if !(camera.position.is_finite() && camera.orientation.theta.is_finite() && camera.orientation.phi.is_finite()) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "The camera must be finite."));
    }
    editor_arc.lock_world_for_edit().await?.fallback_camera_location = camera;
    editor_arc.unsaved_changes_notify.notify_one();
    Ok(StatusCode::NO_CONTENT)
}

// The AI redraws the thumbnail at the next publish, from that publish's screenshot.
async fn refresh_ai_thumbnail(State(editor_arc): State<Arc<EditorServer>>) -> StatusCode {
    editor_arc.should_refresh_ai_thumbnail.store(true, Ordering::Relaxed);
    StatusCode::NO_CONTENT
}

// Explicit saves use the authoritative world and finish even if this browser closes.
// Use the existing publish operation, including its thumbnail request, without an AI turn.
async fn publish_game(state: State<Arc<EditorServer>>) -> Result<Json<ToolResultFinal>, ApiError> {
    run_tool_call(
        state,
        Json(ToolCall {
            name: "publish".into(),
            arguments: json!({}),
        }),
    )
    .await
}

#[derive(Deserialize)]
struct ChatRunning {
    is_chat_running: bool,
}

// Node owns chat execution and reports its active reply through the common state stream. Ending a
// turn answers with the objects it changed.
async fn set_chat_status(State(editor_arc): State<Arc<EditorServer>>, Json(update): Json<ChatRunning>) -> Json<Vec<EditorTurnObjectChange>> {
    Json(editor_arc.set_chat_status(update.is_chat_running).await)
}

// -------------------------- Assets and world files --------------------------

#[derive(Deserialize)]
struct AddAssetRequest {
    asset_id: i64,
}

// The browser adds an asset the same way the AI's asset_download tool does, as one undo step.
async fn asset_download_into_world(
    State(editor_arc): State<Arc<EditorServer>>,
    headers: HeaderMap,
    Json(AddAssetRequest { asset_id }): Json<AddAssetRequest>,
) -> Result<Json<Value>, ApiError> {
    let author = read_editor_session(&headers)?;
    if asset_id <= 0 {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Asset id must be positive."));
    }
    let asset = download_asset(asset_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Asset not found."))?;
    let object_id = tokio::spawn(async move {
        let object_id = editor_arc.insert_asset(Some(author), asset.name, asset.data_json).await?;
        editor_arc.record_asset_import(asset_id, asset.version_num).await.map_err(internal_error)?;
        Ok::<_, ApiError>(object_id)
    })
    .await
    .map_err(internal_error)??;
    Ok(Json(json!({ "object_id": object_id })))
}

// TODO: AI-PICKED-VALUE: Uploaded images and sounds may be up to 16 MiB; models keep the 8 MiB GLB limit.
const MAX_UPLOAD_ASSET_BYTES: usize = 16 * 1024 * 1024;
// Stop an import after ten minutes, so a stuck conversion or upload fails instead of holding up the AI chat or a browser forever.
// Large textures take seconds to a few minutes to compress.
const ASSET_IMPORT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

// An import that runs past the timeout is stopped where it is, so it never adds anything to the world afterwards.
async fn with_import_timeout<T>(import: impl Future<Output = Result<T, ApiError>>) -> Result<T, ApiError> {
    tokio::time::timeout(ASSET_IMPORT_TIMEOUT, import)
        .await
        .map_err(|_| ApiError::new(StatusCode::GATEWAY_TIMEOUT, "The import took longer than ten minutes, so it was stopped."))?
}

// An upload's data is stored as the game's creator and added like any asset. With should_publish it is also published to
// assets, and with an asset_id it is that asset's next version.
async fn add_upload_to_world(
    editor_arc: &EditorServer,
    author: Option<String>,
    name: String,
    data: AssetPublishData,
    asset_id: Option<i64>,
    should_publish: bool,
) -> Result<(ObjectId, Option<i64>), ApiError> {
    let bad_request = |error: anyhow::Error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string());
    let (data, asset_id) = match should_publish {
        true => {
            let asset = publish_to_assets(asset_id, &name, data).await.map_err(bad_request)?;
            (asset.data_json, Some(asset.asset_id))
        }
        false => {
            if asset_id.is_some() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "Only a published upload can be an asset's next version.",
                ));
            }
            (dehydrate_publish_data(data, &editor_arc.owner_id).await.map_err(bad_request)?, None)
        }
    };
    let object_id = editor_arc.insert_asset(author, name, data).await?;
    Ok((object_id, asset_id))
}

#[derive(Deserialize)]
struct AssetImportQuery {
    kind: AssetType,
    name: String,
    asset_id: Option<i64>,
    should_publish: bool,
}

async fn asset_import(
    State(editor_arc): State<Arc<EditorServer>>,
    access: Option<Extension<BrowserAccess>>,
    headers: HeaderMap,
    Query(AssetImportQuery {
        kind,
        name,
        asset_id,
        should_publish,
    }): Query<AssetImportQuery>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<Value>, ApiError> {
    let author = read_author(access, &headers)?;
    let body = body.map_err(|rejection| match rejection.status() {
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Files must be at most {} MiB.", MAX_UPLOAD_ASSET_BYTES / 1024 / 1024),
        ),
        status => ApiError::new(status, rejection.body_text()),
    })?;
    // Converting, publishing, and adding finish together even if this browser disconnects, so the
    // conversion lock is held for as long as the conversion runs.
    let (object_id, asset_id) = tokio::spawn(with_import_timeout(async move {
        let uploaded_file = body.clone();
        let mut data = convert_uploaded_asset_to_publish_data(kind, &name, Vec::from(body))
            .await
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
        // A published model keeps its file as its source, which an AI downloads to edit the model.
        if should_publish && let AssetPublishData::Mesh(loaded_gltf) = &mut data {
            let blender_id = save_model_source(&editor_arc.owner_id, MeshSourceFormat::Glb, Vec::from(uploaded_file))
                .await
                .map_err(internal_error)?;
            loaded_gltf.blender_id = Some(blender_id);
        }
        add_upload_to_world(&editor_arc, author, name, data, asset_id, should_publish).await
    }))
    .await
    .map_err(internal_error)??;
    Ok(Json(json!({ "object_id": object_id, "asset_id": asset_id })))
}

// TODO: AI-PICKED-VALUE: A material sends up to four images, so its request may hold four uploads' worth of bytes.
const MAX_UPLOAD_MATERIAL_BYTES: usize = 4 * MAX_UPLOAD_ASSET_BYTES;

#[derive(Deserialize)]
struct AssetImportMaterialQuery {
    name: String,
    asset_id: Option<i64>,
    should_publish: bool,
}

// A material's images arrive as form fields named after the maps they fill.
async fn asset_import_material(
    State(editor_arc): State<Arc<EditorServer>>,
    access: Option<Extension<BrowserAccess>>,
    headers: HeaderMap,
    Query(AssetImportMaterialQuery {
        name,
        asset_id,
        should_publish,
    }): Query<AssetImportMaterialQuery>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let author = read_author(access, &headers)?;
    let read_multipart_error = |error: MultipartError| match error.status() {
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("A material's files must be at most {} MiB together.", MAX_UPLOAD_MATERIAL_BYTES / 1024 / 1024),
        ),
        status => ApiError::new(status, error.body_text()),
    };
    let (mut albedo, mut metallic, mut roughness, mut normal) = (None, None, None, None);
    while let Some(field) = multipart.next_field().await.map_err(read_multipart_error)? {
        let image_bytes = match field.name() {
            Some("albedo") => &mut albedo,
            Some("metallic") => &mut metallic,
            Some("roughness") => &mut roughness,
            Some("normal") => &mut normal,
            _ => return Err(ApiError::new(StatusCode::BAD_REQUEST, "Unknown material texture.")),
        };
        *image_bytes = Some(Vec::from(field.bytes().await.map_err(read_multipart_error)?));
    }
    let albedo = albedo.ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "A material needs a color texture."))?;
    // As with uploaded files, converting, publishing, and adding finish together even if this browser disconnects.
    let (object_id, asset_id) = tokio::spawn(with_import_timeout(async move {
        let data = convert_uploaded_material_to_publish_data(&name, albedo, metallic, roughness, normal)
            .await
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
        add_upload_to_world(&editor_arc, author, name, data, asset_id, should_publish).await
    }))
    .await
    .map_err(internal_error)??;
    Ok(Json(json!({ "object_id": object_id, "asset_id": asset_id })))
}

#[derive(Deserialize)]
struct AssetDownloadSourceQuery {
    asset_id: i64,
}

// The file a model asset was made from, so an AI can edit it and import it again. Only the game's creator's models have one.
async fn asset_download_source(
    State(editor_arc): State<Arc<EditorServer>>,
    Query(AssetDownloadSourceQuery { asset_id }): Query<AssetDownloadSourceQuery>,
) -> Result<Response, ApiError> {
    let bad_request = |error: anyhow::Error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string());
    let blender_id = editor_arc.read_asset_blender_id(asset_id).await.map_err(bad_request)?;
    let (format, bytes) = download_model_source(&editor_arc.owner_id, blender_id)
        .await
        .map_err(bad_request)?;
    let content_type = match format {
        MeshSourceFormat::Glb => "model/gltf-binary",
        MeshSourceFormat::Blend => "application/x-blender",
    };
    Ok(([(header::CONTENT_TYPE, content_type)], bytes).into_response())
}

// The browser downloads the world from its own copy, and an MCP client, which has none, downloads it here.
async fn download_world(State(editor_arc): State<Arc<EditorServer>>) -> Result<String, ApiError> {
    editor_arc.current_world_arc.read().await.toStoredWorldString().map_err(internal_error)
}

// The file is checked and loaded before the live world is touched, then replaces it in one step.
async fn upload_world(State(editor_arc): State<Arc<EditorServer>>, world: String) -> Result<Json<Value>, ApiError> {
    tokio::spawn(async move {
        let uploaded_world = tokio::task::spawn_blocking(move || World::fromStoredWorldString(&world, WorldControlMode::Editor, None))
            .await
            .map_err(internal_error)?
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
        editor_arc.upload_world(uploaded_world).await
    })
    .await
    .map_err(internal_error)??;
    Ok(Json(json!({})))
}

// -------------------------- Original Rust tools --------------------------

async fn run_tool_call(State(editor_arc): State<Arc<EditorServer>>, Json(call): Json<ToolCall>) -> Result<Json<ToolResultFinal>, ApiError> {
    let result = editor_arc
        .run_tool_call(call)
        .await?
        .await
        .map_err(|_| ApiError::new(StatusCode::GONE, "The editor stopped before the tool finished."))?;
    Ok(Json(result))
}

async fn start_playtest(State(editor_arc): State<Arc<EditorServer>>) -> Result<StatusCode, ApiError> {
    editor_arc.start_user_playtest_server().await.map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn stop_playtest(State(editor_arc): State<Arc<EditorServer>>) -> StatusCode {
    editor_arc.stop_user_playtest_server().await;
    StatusCode::NO_CONTENT
}
// -------------------------- User playtest players --------------------------

// A guest who is logged in sends their login, so their player plays as their account.
#[derive(Deserialize)]
struct AddPlayerRequest {
    login_token: Option<String>,
}

async fn add_player(
    State(editor_arc): State<Arc<EditorServer>>,
    Extension(access): Extension<BrowserAccess>,
    Json(AddPlayerRequest { login_token }): Json<AddPlayerRequest>,
) -> Result<Response, ApiError> {
    let (public_ip, port) = editor_arc.database.playtest_address().await?;
    let profile_id = match (access, login_token) {
        (BrowserAccess::Creator { profile_id }, _) => Some(profile_id),
        (BrowserAccess::Guest { .. }, Some(login_token)) => Some(get_profile_id_of_login(&editor_arc, &format!("Bearer {login_token}")).await?),
        (BrowserAccess::Guest { .. }, None) => None,
    };
    let (username, user_id, player_profile) = match profile_id {
        Some(profile_id) => {
            let (username, user_id, player_profile) = database::player_profile(&profile_id).await?;
            (username, Some(user_id), player_profile)
        }
        // Each playtest join still gets its own player ID and one-use token.
        None => ("Guest".into(), None, PlayerProfile::GUEST),
    };
    let server_lock = editor_arc.user_playtest_server.lock().await;
    let process = server_lock
        .as_ref()
        .filter(|process| !process.has_stopped())
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "Start a playtest first."))?;
    let join_token: String = process
        .request(PlaytestRequest::AddPlayer {
            username,
            user_id,
            profile: player_profile,
        })
        .await
        .map_err(internal_error)?;
    drop(server_lock);
    let mut url = reqwest::Url::parse("kube-new://").map_err(internal_error)?;
    url.query_pairs_mut()
        .append_pair("mode", "playtest")
        .append_pair("public_ip", &public_ip)
        .append_pair("port", &port.to_string())
        .append_pair("join_token", &join_token);
    if editor_arc.database.is_dev {
        // Launch the client built alongside this dev editor, bypassing the installed app's URL handler.
        let executable = std::env::current_exe()
            .map_err(internal_error)?
            .with_file_name(format!("client{}", std::env::consts::EXE_SUFFIX));
        let mut child = tokio::process::Command::new(executable)
            .arg("--open-separately")
            .arg(url.as_str())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(internal_error)?;
        tokio::spawn(async move {
            if let Err(error) = child.wait().await {
                log::error!("Could not wait for dev playtest client: {error}");
            }
        });
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    Ok(Json(json!({"launch_url": url.as_str()})).into_response())
}

// All public routes are here. Tool routes use a separate deployment credential, never browser login.
pub fn router(editor_arc: Arc<EditorServer>, editor_path: &str) -> Router {
    // Browsers and MCP clients send and fetch model and asset files here, and the AI chat does the same under /local-only.
    let files = Router::new()
        .route("/asset-download-source", get(asset_download_source))
        .route("/asset-import", post(asset_import).layer(DefaultBodyLimit::max(MAX_UPLOAD_ASSET_BYTES)))
        .route(
            "/asset-import-material",
            post(asset_import_material).layer(DefaultBodyLimit::max(MAX_UPLOAD_MATERIAL_BYTES)),
        );
    let browser = Router::new()
        .route("/editor-client-action", post(apply_client_action))
        .route("/editor-history/{direction}", post(take_history_step))
        .route("/publish-game", post(publish_game))
        .route("/set-fallback-camera", post(set_fallback_camera))
        .route("/refresh-ai-thumbnail", post(refresh_ai_thumbnail))
        .route("/asset-download", post(asset_download_into_world))
        .merge(files.clone())
        .route("/download-world", get(download_world))
        .route("/upload-world", post(upload_world))
        .route("/playtest/start", post(start_playtest))
        .route("/playtest/stop", post(stop_playtest))
        .route("/playtest/add-player", post(add_player))
        .route_layer(middleware::from_fn_with_state(editor_arc.clone(), require_editor_access));
    let health = Router::new()
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .route_layer(middleware::from_fn_with_state(editor_arc.clone(), require_worker));
    // Browser sockets authenticate in their first message rather than an HTTP Authorization header.
    let public = browser.merge(health).route("/editor-server-websocket", any(editor_server_websocket));
    let public = if editor_path.is_empty() {
        public
    } else {
        Router::new().nest(editor_path, public)
    };
    // Local-only routes use the deployment credential; the URL prefix alone does not restrict access.
    let tools = Router::new()
        .route(
            "/local-only/tools",
            get(|| async {
                let tools = EDITOR_TOOL_REGISTRY
                    .tool_specs()
                    .iter()
                    .map(|spec| {
                        json!({
                            "name": spec.tool_name, "description": spec.description, "inputSchema": spec.parameters_json,
                        })
                    })
                    .collect::<Vec<_>>();
                Json(json!({"system_prompt": SYSTEM_PROMPT, "compaction_input_tokens": AI_CHAT_COMPACTION_INPUT_TOKENS, "tools": tools}))
            }),
        )
        .route("/local-only/run-tool-call", post(run_tool_call))
        .route("/local-only/chat-status", post(set_chat_status))
        .route(
            "/local-only/chat-turn-changes",
            get(|State(editor_arc): State<Arc<EditorServer>>| async move { Json(editor_arc.read_chat_turn_changes().await) }),
        )
        .nest("/local-only", files)
        .route_layer(middleware::from_fn_with_state(editor_arc.clone(), require_worker));
    public
        .merge(tools)
        // TODO: AI-PICKED-VALUE: Allow full world payloads up to 64 MiB.
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(SITE_URL.parse::<header::HeaderValue>().expect("SITE_URL header"))
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, EDITOR_SESSION_HEADER])
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS]),
        )
        .with_state(editor_arc)
}
