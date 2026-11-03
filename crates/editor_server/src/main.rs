mod ai_playtest_server;
mod api_error;
mod asset_uploads;
mod assets;
mod auth;
mod database;
mod editor;
mod editor_history;
mod editor_world_tracker;
mod editor_sync;
mod endpoints;
mod model_sources;
mod playtest_process;
mod publishing;
mod screenshot;

use ai_playtest_server::{AI_PLAYTEST_SERVER, HeadlessPlaytestHandlerImpl};
use playtest_process::{RUN_PLAYTEST_ARG, run_playtest};
use screenshot::{DRAW_SCREENSHOT_ARG, draw_png_for_editor};
use database::EditorDatabase;
use editor::EditorServer;
use editor_history::EditorHistory;
use editor_sync::EditorSync;
use endpoints::router;
use game_types::editor_sync::EditorStatus;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use shared::{
    api::protocol_for_chat::ClientEditorToolHandlers,
    env::{env_vars::*, initialize_env::initialize_env},
    game::{
        data_fetcher::DataFetcherArc,
        jsRuntime::initializeV8,
        world::{World, WorldControlMode},
    },
};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{Mutex, Notify, RwLock, watch},
};
use tokio_util::sync::CancellationToken;

// How long the background save waits after the first change, so a burst of edits becomes one write. Shutdown saves whatever
// came after the last one.
const SERVER_SAVE_INTERVAL: Duration = Duration::from_secs(30);

async fn wait_for_termination() {
    #[cfg(unix)]
    {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        terminate.recv().await;
    }
    #[cfg(not(unix))]
    std::future::pending::<()>().await;
}

// V8 is set up before tokio starts its threads, so that every thread can run scripts.
fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some(DRAW_SCREENSHOT_ARG) {
        return draw_png_for_editor();
    }
    initializeV8();
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async_main())
}

// Rust owns worlds, tools, persistence, and lifecycle. Its Node child serves AI SDK chat directly.
async fn async_main() -> anyhow::Result<()> {
    initialize_env();
    env_logger::init();
    if std::env::args().nth(1).as_deref() == Some(RUN_PLAYTEST_ARG) {
        return run_playtest().await;
    }
    let is_dev = std::env::args().any(|arg| arg == "--dev");
    let shutdown_token = CancellationToken::new();
    if is_dev {
        // The dev launcher owns stdin. Losing that pipe also closes this editor and its playtests.
        let shutdown_token = shutdown_token.clone();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            shutdown_token.cancel();
        });
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(EDITOR_SERVER_SECRET.as_bytes())?;
    mac.update(ARBITRIUM_REQUEST_ID.as_bytes());
    let token = mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
    let database = EditorDatabase::new(is_dev);
    // Edgegap may start the process before allocation has committed its starting row.
    let startup_deadline = Instant::now() + Duration::from_secs(120);
    let (creator_id, stored_world) = loop {
        anyhow::ensure!(
            Instant::now() < startup_deadline,
            "Editor allocation was not committed before startup timed out."
        );
        match database.load_world().await {
            Ok(initial) => break initial,
            Err(error) if error.status == axum::http::StatusCode::GONE => {
                tokio::select! {
                    _ = shutdown_token.cancelled() => anyhow::bail!("Editor stopped during startup."),
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
            Err(error) => return Err(error.into()),
        }
    };
    let mut current_world = World::fromStoredWorldString(&stored_world, WorldControlMode::Editor, None)?;
    let loaded_world_value = serde_json::to_value(current_world.toStoredWorld())?;
    let hostname = if is_dev { "127.0.0.1" } else { "0.0.0.0" };
    let listener = tokio::net::TcpListener::bind(format!("{hostname}:{}", EDITOR_SERVER_PORT.as_str())).await?;

    // The child serves chat on its own port; stdout confirms readiness before Rust accepts work.
    let mut node = tokio::process::Command::new("node")
        .args(["--import", "tsx", "src/index.ts"])
        .args(is_dev.then_some("--dev"))
        .current_dir("node")
        .env("EDITOR_SERVER_PORT", EDITOR_SERVER_PORT.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    // Child::wait closes its stdin; keep the shutdown pipe open until we explicitly stop Node.
    let node_stdin = node.stdin.take().expect("piped Node stdin");
    let mut stdout = BufReader::new(node.stdout.take().expect("piped Node stdout"));
    let mut ready = String::new();
    tokio::time::timeout(Duration::from_secs(30), stdout.read_line(&mut ready)).await??;
    anyhow::ensure!(ready == "ready\n", "Node worker exited before it was ready.");
    // Node's later output goes to our stdout, so it reaches the logs instead of piling up unread in Node.
    tokio::spawn(async move {
        if let Err(error) = tokio::io::copy(&mut stdout, &mut tokio::io::stdout()).await {
            log::error!("Could not forward the Node worker's output: {error}");
        }
    });
    let changes_notify_arc = Arc::new(Notify::new());
    current_world.changes_notify_arc = Some(changes_notify_arc.clone());
    let (editor_status_tx, mut editor_status_rx) = watch::channel(EditorStatus::default());
    let editor_arc = Arc::new(EditorServer {
        owner_id: creator_id,
        token,
        database,
        http,
        history: Mutex::new(EditorHistory::new(&mut current_world)),
        current_world_arc: Arc::new(RwLock::new(current_world)),
        sync: EditorSync::new(editor_status_rx.clone()),
        editor_status_tx,
        data_fetcher: DataFetcherArc::new(),
        user_playtest_server: Mutex::new(None),
        tool_lock: Arc::new(Mutex::new(())),
        last_activity_at: Mutex::new(Instant::now()),
        last_publish_at: Mutex::new(None),
        should_refresh_ai_thumbnail: AtomicBool::new(false),
        pause_reason: Mutex::new(None),
        browser_access_cache: Mutex::new(HashMap::new()),
        unsaved_changes_notify: Notify::new(),
        last_saved_world: Mutex::new(loaded_world_value),
        shutdown_token,
    });
    ClientEditorToolHandlers::set_curr(Some(Arc::new(ClientEditorToolHandlers {
        screenshot_handler: editor_arc.clone(),
        assets_handler: editor_arc.clone(),
        publish_handler: editor_arc.clone(),
        game_settings_handler: editor_arc.clone(),
        catalog_handler: editor_arc.clone(),
        script_files_handler: editor_arc.clone(),
        headless_playtest_handler: Arc::new(HeadlessPlaytestHandlerImpl),
    })));
    // Distinct dev URLs prevent an old browser tab from editing a different game after a switch.
    let editor_path = if is_dev {
        format!("/{}", ARBITRIUM_REQUEST_ID.as_str())
    } else {
        String::new()
    };
    // World setters and worker lifecycle changes wake the same publication loop.
    let streaming_editor_arc = editor_arc.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = streaming_editor_arc.shutdown_token.cancelled() => break,
                _ = changes_notify_arc.notified() => {},
                result = editor_status_rx.changed() => { if result.is_err() { break; } },
            }
            if let Err(error) = streaming_editor_arc.broadcast_changes().await {
                log::error!("Editor synchronization failed: {error}");
                streaming_editor_arc.shutdown_token.cancel();
            }
        }
    });

    // Saving runs here so browser edits and AI tools never wait for the database, and so a burst of
    // edits becomes one write instead of one per edit.
    let saving_editor_arc = editor_arc.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = saving_editor_arc.shutdown_token.cancelled() => break,
                _ = saving_editor_arc.unsaved_changes_notify.notified() => {},
            }
            tokio::select! {
                _ = saving_editor_arc.shutdown_token.cancelled() => break,
                _ = tokio::time::sleep(SERVER_SAVE_INTERVAL) => {},
            }
            let world_lock = saving_editor_arc.current_world_arc.read().await;
            let saved = saving_editor_arc.save_world(&world_lock).await;
            drop(world_lock);
            if let Err(error) = saved {
                log::error!("Could not save editor world: {error}");
            }
        }
    });
    let routes = router(editor_arc.clone(), &editor_path);
    axum::serve(listener, routes)
        .with_graceful_shutdown(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            let started_at = Instant::now();
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    _ = wait_for_termination() => break,
                    _ = editor_arc.shutdown_token.cancelled() => break,
                    result = node.wait() => { log::error!("Node worker stopped: {result:?}"); break; }
                    _ = interval.tick() => {
                        // A running reply, tool, or playtest counts as activity, so the fifteen minutes start once it ends.
                        let is_chat_running = editor_arc.editor_status_tx.borrow().is_chat_running;
                        let is_tool_running = editor_arc.tool_lock.try_lock().is_err();
                        if is_chat_running || is_tool_running || editor_arc.has_playtest_players().await {
                            *editor_arc.last_activity_at.lock().await = Instant::now();
                        }
                        // TODO: AI-PICKED-VALUE: Keep an editor for fifteen minutes after its last operation.
                        if editor_arc.last_activity_at.lock().await.elapsed() > Duration::from_secs(15 * 60) {
                            *editor_arc.pause_reason.lock().await = Some("Paused due to inactivity");
                            break;
                        }
                        if started_at.elapsed() > Duration::from_secs(14 * 60 * 60) {
                            *editor_arc.pause_reason.lock().await = Some("Paused after 14 hours");
                            break;
                        }
                    }
                }
            }
            editor_arc.shutdown_token.cancel();
            // Cancel chat and tools before the final save. Node exits once it has saved the chat.
            drop(node_stdin);
            let tool_lock = editor_arc.tool_lock.lock().await;
            editor_arc.stop_user_playtest_server().await;
            AI_PLAYTEST_SERVER.stop_all();
            let world_lock = editor_arc.current_world_arc.read().await;
            if let Err(error) = editor_arc.save_world(&world_lock).await {
                log::error!("Final world save failed: {error}");
            }
            drop(world_lock);
            drop(tool_lock);
            if tokio::time::timeout(Duration::from_secs(30), node.wait()).await.is_err() {
                let _ = node.kill().await;
            }
            if let Err(error) = editor_arc.database.remove_deployment().await {
                log::error!("Editor database cleanup failed: {error}");
            }
            if !is_dev {
                // Attempt Edgegap cleanup even if the database cleanup failed.
                match editor_arc
                    .http
                    .delete(ARBITRIUM_DELETE_URL.as_str())
                    .header("Authorization", ARBITRIUM_DELETE_TOKEN.as_str())
                    .send()
                    .await
                {
                    Ok(response)
                        if response.status().is_success()
                            || response.status() == reqwest::StatusCode::NOT_FOUND
                            || response.status() == reqwest::StatusCode::GONE => {}
                    Ok(response) => log::error!("Editor deployment cleanup failed: {}", response.status()),
                    Err(error) => log::error!("Editor deployment cleanup failed: {error}"),
                }
            }
        })
        .await?;
    Ok(())
}
