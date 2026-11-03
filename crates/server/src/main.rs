#![allow(non_snake_case)]

use game_types::object_types::ObjectKind;
use server::{
    ServerMode,
    database::{download_published_world, remove_running_server, remove_running_server_if_empty},
    start_running_server_in_bg,
};
use shared::{
    env::{
        env_vars::{ARBITRIUM_DELETE_TOKEN, ARBITRIUM_DELETE_URL, EDGEGAP_INTERNAL_PORT, GAME_ID, GAMEWORLD_VERSION_NUM},
        initialize_env::initialize_env,
        logging::initialize_logging,
    },
    helpers::await_shutdown_signal::await_shutdown_signal,
};
use std::time::{Duration, Instant};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

// TODO: AI-PICKED-VALUE: A server with no players for five seconds stops itself.
const EMPTY_SERVER_LIFETIME: Duration = Duration::from_secs(5);

#[tokio::main]
pub async fn main() {
    initialize_env();
    initialize_logging();

    // Choose the initial world before starting the server loop.
    let stored_world_string = match download_published_world(*GAME_ID, *GAMEWORLD_VERSION_NUM).await {
        Ok(stored_world_string) => stored_world_string,
        Err(error) => {
            log::error!("Could not load hosted stored world string: {}", error);
            return;
        }
    };
    // kill_server_cancel_token is only needed when this process should gracefully stop the server loop.
    let kill_server_cancel_token = CancellationToken::new();

    // if the server is shut down, call the server cancel token
    tokio::spawn({
        let kill_server_cancel_token = kill_server_cancel_token.clone();
        async move {
            await_shutdown_signal().await;
            kill_server_cancel_token.cancel();
        }
    });

    // start the server forever
    let server_start_time = Instant::now();
    let player_cleanup_tasks_tracker = TaskTracker::new();
    let world_arc = start_running_server_in_bg(
        &stored_world_string,
        kill_server_cancel_token.clone(),
        player_cleanup_tasks_tracker.clone(),
        server_start_time,
        *EDGEGAP_INTERNAL_PORT,
        ServerMode::Official,
        None,
    );

    // Once nobody has been in the server for EMPTY_SERVER_LIFETIME, it takes itself off the servers the website sends
    // players to, unless someone is on the way, and then asks Edgegap to stop it. Edgegap's shutdown signal ends it below.
    if let Some(world_arc) = world_arc {
        tokio::spawn(async move {
            // TODO: AI-PICKED-VALUE: Check for players every second, so a server stops 5 to 6 seconds after its last player leaves.
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            let mut last_player_at = Instant::now();
            loop {
                interval.tick().await;
                let world_lock = world_arc.read().await;
                if world_lock
                    ._kindOfObjectId
                    .get_keys(&ObjectKind::Player)
                    .is_some_and(|player_object_ids| !player_object_ids.is_empty())
                {
                    last_player_at = Instant::now();
                }
                drop(world_lock);
                if last_player_at.elapsed() < EMPTY_SERVER_LIFETIME {
                    continue;
                }
                match remove_running_server_if_empty().await {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        log::error!("Could not take this empty server off the running servers: {error}");
                        continue;
                    }
                }
                match reqwest::Client::new()
                    .delete(ARBITRIUM_DELETE_URL.as_str())
                    .header("Authorization", ARBITRIUM_DELETE_TOKEN.as_str())
                    .send()
                    .await
                {
                    Ok(response)
                        if response.status().is_success()
                            || response.status() == reqwest::StatusCode::NOT_FOUND
                            || response.status() == reqwest::StatusCode::GONE =>
                    {
                        break;
                    }
                    Ok(response) => log::error!("Empty server deployment cleanup failed: {}", response.status()),
                    Err(error) => log::error!("Empty server deployment cleanup failed: {error}"),
                }
            }
        });
    }

    // wait until ctrl+c/term or the server loop thread exits.
    kill_server_cancel_token.cancelled().await;

    log::info!("Shutting down server...");

    // This is only a best-effort fallback in case the website cleanup path misses the shutdown.
    if let Err(error) = remove_running_server().await {
        log::error!("Failed syncing running server cleanup on server shutdown: {}", error);
    }

    player_cleanup_tasks_tracker.close(); // stop accepting
    player_cleanup_tasks_tracker.wait().await;

    log::info!("Server completely exited.");
}
