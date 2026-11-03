use game_types::consts::{CLIENT_MAX_IDLE_TIMEOUT_SECS, CLIENT_SERVER_PROTOCOL_VERSION, CLIENT_WORLD_DELAY_BASE_MS};
use game_types::data_types::DataIds;
pub mod game_chat;
pub mod game_chat_networking;
pub mod input_snapshot;
pub mod mesh_camera_shapes;
pub mod mouse_cursor;
pub mod play;
pub mod play_renderer;
pub mod render_snapshot;
pub mod script_input_tracker;
pub mod snapshot_buffer;
pub mod sound_playback_manager;
pub mod thread_priority;
pub mod time_estimator;

use crate::app_runner::AppRunnerInputs;
use crate::play_runner::game_chat_networking::start_receiving_game_chat_messages_tcp_in_bg;
use crate::play_runner::input_snapshot::PlayInputState;
use crate::play_runner::mesh_camera_shapes::MeshCameraShapes;
use crate::play_runner::play::{GuiTriggeredHandler, PlayLoopInputs, getBlockedCameraZoom, start_running_play_in_bg};
use crate::play_runner::render_snapshot::FullWorldRenderSnapshot;
use crate::play_runner::sound_playback_manager::SoundPlaybackManager;
use crate::play_runner::time_estimator::{TimeEstimatorWriter, createTimeEstimator};
use indexmap::IndexMap;
use quinn::Connection;
use quinn::{ClientConfig, Endpoint, SendStream};
use render_core::renderable::Renderable;
use reqwest::header::AUTHORIZATION;
use rustls::pki_types::CertificateDer;
use shared::env::env_vars::SITE_URL;
use shared::game::data_fetcher::DataFetcherArc;
use shared::game::jsRuntime::{BuiltinMouseCursor, CameraLimits, JsConsoleLogCallback, MouseCursor};
use shared::game::world::{World, WorldControlMode};
use shared::protocol::messages::{
    ClientAckMessage, ClientFunctionCallMessage, ClientFunctionResultMessage, ClientHandshakeTCP, ClientHelloMessage, ClientMessageUDP, ClientPingMessage,
    ServerHandshakeTCP, ServerInitialWorldMessage, ServerMessageUDP, ServerPlayerPingsMessage, ServerScriptRawCodeMessage, ServerSnapshotMessage,
};
use shared::protocol::quic_cert::{QUIC_CERT_SERVER_NAME, QUIC_SERVER_CERT_DER_BYTES};
use shared::protocol::quic_send_receive_helpers::{
    MAX_INITIAL_WORLD_MESSAGE_BYTES, MAX_TCP_MESSAGE_BYTES, awaitDatagramMessage, awaitTcpMessageMaxBytes, sendDatagramMessage, sendTcpMessage,
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, interval, sleep, sleep_until};
use tokio_util::sync::CancellationToken;

pub use play_renderer::{PlayRunner, PlayRunnerInputs};

const PLAY_SESSION_CONNECTION_INFO_POLL_ATTEMPTS: u32 = 120;
const PLAY_SESSION_CONNECTION_INFO_POLL_SECS: u64 = 1;
const IDLE_PLAYER_KICK_TIMEOUT: Duration = Duration::from_secs(20 * 60);

enum PlayEnd {
    KickedForBeingIdle,
    StoppedForAnotherReason,
}

// Only desktop players run this: nothing stamps a headless AI client's input.
async fn kick_player_once_idle(
    last_input_at_rx: watch::Receiver<Instant>,
    kill_play_cancel_token: CancellationToken,
    connection_err_tx: mpsc::UnboundedSender<AppRunnerInputs>,
) -> PlayEnd {
    loop {
        let kick_at = *last_input_at_rx.borrow() + IDLE_PLAYER_KICK_TIMEOUT;
        if kick_at <= Instant::now() {
            break;
        }
        tokio::select! {
            _ = kill_play_cancel_token.cancelled() => return PlayEnd::StoppedForAnotherReason,
            _ = sleep_until(kick_at) => {}
        }
    }
    kill_play_cancel_token.cancel();
    let idle_minutes = IDLE_PLAYER_KICK_TIMEOUT.as_secs() / 60;
    let _ = connection_err_tx.send(AppRunnerInputs::from_error(
        "Disconnected",
        format!("You were idle for {idle_minutes} minutes."),
    ));
    PlayEnd::KickedForBeingIdle
}

pub async fn get_play_app_runner_inputs_and_start_bg_tasks_to_official_server(
    request_id: String,
    play_auth: String,
    connection_err_tx: mpsc::UnboundedSender<AppRunnerInputs>,
) -> AppRunnerInputs {
    #[derive(serde::Deserialize)]
    struct PlaySessionConnectionInfoResponse {
        public_ip: String,
        port: u16,
    }

    let play_session_connection_info_url = format!("{}/api/play-sessions/{request_id}/connection-info", SITE_URL.as_str());
    let play_auth_header = format!("Bearer {play_auth}");
    let http_client = reqwest::Client::new();

    // Poll the website until it has the public connection info for this reserved play session.
    let mut play_session_connection_info = None;
    for _ in 0..PLAY_SESSION_CONNECTION_INFO_POLL_ATTEMPTS {
        let response = match http_client
            .get(play_session_connection_info_url.as_str())
            .header(AUTHORIZATION, play_auth_header.as_str())
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => return AppRunnerInputs::from_error("Trouble connecting", error.to_string()),
        };

        match response.status() {
            reqwest::StatusCode::OK => {
                let loaded_play_session_connection_info = match response.json::<PlaySessionConnectionInfoResponse>().await {
                    Ok(loaded_play_session_connection_info) => loaded_play_session_connection_info,
                    Err(error) => return AppRunnerInputs::from_error("Trouble connecting", error.to_string()),
                };
                play_session_connection_info = Some(loaded_play_session_connection_info);
                break;
            }
            reqwest::StatusCode::ACCEPTED => {
                sleep(Duration::from_secs(PLAY_SESSION_CONNECTION_INFO_POLL_SECS)).await;
            }
            _ => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return AppRunnerInputs::from_error("Trouble connecting", format!("Failed waiting for server connection info: {body} {status}"));
            }
        }
    }
    let Some(play_session_connection_info) = play_session_connection_info else {
        return AppRunnerInputs::from_error("Trouble connecting", "Timed out waiting for server connection info.".to_string());
    };
    let PlaySessionConnectionInfoResponse { public_ip, port: server_port } = play_session_connection_info;

    let data_fetcher = DataFetcherArc::new();
    let sound_playback_manager = SoundPlaybackManager::new(data_fetcher.clone());

    let inputs = get_play_app_runner_inputs_and_start_bg_tasks(
        public_ip,
        server_port,
        play_auth,
        data_fetcher,
        sound_playback_manager,
        None,
        connection_err_tx.clone(),
    )
    .await;
    if let AppRunnerInputs::PlayMode(play_runner_inputs) = &inputs {
        tokio::spawn(kick_player_once_idle(
            play_runner_inputs.last_input_at_tx.subscribe(),
            play_runner_inputs.kill_play_cancel_token.clone(),
            connection_err_tx,
        ));
    }
    inputs
}

// Playtest links provide the server address and join token; each player window closes on disconnect.
pub async fn get_play_app_runner_inputs_and_start_bg_tasks_to_playtest_server(
    public_ip: String,
    port: u16,
    join_token: String,
    connection_err_tx: mpsc::UnboundedSender<AppRunnerInputs>,
) -> AppRunnerInputs {
    let data_fetcher = DataFetcherArc::new();
    let sound_playback_manager = SoundPlaybackManager::new(data_fetcher.clone());
    let inputs = get_play_app_runner_inputs_and_start_bg_tasks(
        public_ip,
        port,
        join_token,
        data_fetcher,
        sound_playback_manager,
        None,
        connection_err_tx.clone(),
    )
    .await;
    let AppRunnerInputs::PlayMode(play_runner_inputs) = &inputs else {
        return inputs;
    };
    let idle_kick = kick_player_once_idle(
        play_runner_inputs.last_input_at_tx.subscribe(),
        play_runner_inputs.kill_play_cancel_token.clone(),
        connection_err_tx,
    );
    tokio::spawn(async move {
        // This process is dedicated to one test player; networking also cancels this token on disconnect.
        // A player kicked for being idle keeps the window, which says why.
        match idle_kick.await {
            PlayEnd::KickedForBeingIdle => {}
            PlayEnd::StoppedForAnotherReason => std::process::exit(0),
        }
    });
    inputs
}

// Snapshots, pongs and every player's ping arrive as datagrams. Each snapshot is ACKed the moment it arrives, before the
// play loop takes it in. The server sends a change again when no ACK comes back within JITTER_MARGIN_MS plus a round trip,
// and sending it again gives it a new message number, so ACKing at the end of a slow tick made it send the same changes forever.
pub fn start_receiving_server_messages_udp_in_bg(
    connection: Connection,
    mut time_estimator_writer: TimeEstimatorWriter,
    kill_play_cancel_token: CancellationToken,
) -> (
    mpsc::UnboundedReceiver<ServerSnapshotMessage>,
    mpsc::UnboundedReceiver<ServerPlayerPingsMessage>,
) {
    let (snapshot_tx, snapshot_rx) = mpsc::unbounded_channel();
    let (player_pings_tx, player_pings_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let server_message = tokio::select! {
                _ = kill_play_cancel_token.cancelled() => break,
                server_message = awaitDatagramMessage::<ServerMessageUDP>(&connection) => server_message,
            };
            match server_message {
                Ok(data) => match data {
                    ServerMessageUDP::Snapshot(snapshot) => {
                        let ack = ClientMessageUDP::Ack(ClientAckMessage {
                            message_num: snapshot.message_num,
                        });
                        if let Err(error) = sendDatagramMessage(&connection, &ack) {
                            log::warn!("Error sending snapshot ACK: {}", error);
                        }
                        let _ = snapshot_tx.send(snapshot);
                    }
                    ServerMessageUDP::Pong(pong) => {
                        time_estimator_writer.process_pong(pong.t0_client_send_ms, pong.t1_server_recv_ms);
                    }
                    ServerMessageUDP::PlayerPings(player_pings) => {
                        let _ = player_pings_tx.send(player_pings);
                    }
                },
                Err(error) => {
                    log::warn!("Failed to decode server datagram: {}", error);
                    break;
                }
            }
        }
    });
    (snapshot_rx, player_pings_rx)
}

// Shared bootstrap for normal players, desktop playtests, and headless AI clients.
pub async fn get_play_app_runner_inputs_and_start_bg_tasks(
    public_ip: String,
    server_port: u16,
    play_auth: String,
    data_fetcher: DataFetcherArc,
    sound_playback_manager: Option<SoundPlaybackManager>,
    console_log_callback: Option<JsConsoleLogCallback>,
    connection_err_tx: mpsc::UnboundedSender<AppRunnerInputs>,
) -> AppRunnerInputs {
    // To stop playback, close the connection with disconnect_from_server.
    let (connection, initial_message, handshake_send_stream) = match start_connection_to_server(public_ip, server_port, play_auth).await {
        Ok(connection_info) => connection_info,
        Err(error) => return AppRunnerInputs::from_error("Trouble connecting", error.to_string()),
    };

    // Build the local client world from the initial server snapshot before background networking starts.
    let ServerInitialWorldMessage {
        stored_world_string,
        player_object_id,
        live_server_wall_ms,
        teleportCountOfObjectId,
        template_data_ids,
    } = initial_message;
    let (js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
    let (key_and_mouse_event_tx, key_and_mouse_event_rx) = mpsc::unbounded_channel();
    let world_res = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Player(player_object_id), Some(js_callback_inputs_tx));
    let mut world = match world_res {
        Ok(world) => world,
        Err(error) => return AppRunnerInputs::from_error("Error loading world", error.to_string()),
    };
    world.teleportCountOfObjectId = teleportCountOfObjectId;

    // Everything this player could see without a new download: the world they were sent, and the server's templates.
    let mut starter_data_ids = world.collectAllDataIds();
    let DataIds {
        mesh_data_ids,
        texture_data_ids,
        sound_data_ids,
    } = template_data_ids;
    starter_data_ids.mesh_data_ids.extend(mesh_data_ids);
    starter_data_ids.texture_data_ids.extend(texture_data_ids);
    starter_data_ids.sound_data_ids.extend(sound_data_ids);

    // Start the background networking tasks that keep this play session fed with snapshots and clock sync.
    let kill_play_cancel_token = CancellationToken::new();

    // start listening for the cancel token and stop the server when it's received.
    tokio::spawn({
        let connection = connection.clone();
        let kill_play_cancel_token = kill_play_cancel_token.clone();
        async move {
            kill_play_cancel_token.cancelled().await;
            // this text should never be seen, and only gets sent to the server bc it's required
            let reason_str = b"The play client stopped.";
            connection.close(quinn::VarInt::from_u32(200), reason_str);
        }
    });

    let (time_estimator_writer, time_estimator_reader) = createTimeEstimator(live_server_wall_ms);

    let initial_play_input_state = PlayInputState::new();
    let initial_live_client_wall_ms = time_estimator_reader.get_live_client_wall_ms();

    // put dummy initial items into the watches so something is there. these should be overwritten quickly
    let initial_render_snapshot = FullWorldRenderSnapshot::new(
        world.getRenderSnapshot(),
        &world,
        player_object_id,
        initial_live_client_wall_ms,
        CLIENT_WORLD_DELAY_BASE_MS,
        IndexMap::new(),
        CameraLimits::new(),
        MouseCursor::Builtin(BuiltinMouseCursor::Default),
        &HashMap::new(),
    );
    let initial_blocked_camera_zoom = getBlockedCameraZoom(
        &world,
        player_object_id,
        &initial_play_input_state,
        CameraLimits::new().offset,
        &mut MeshCameraShapes::new(),
        &|data_id| data_fetcher.getDownloadedMeshData(data_id),
        time_estimator_reader.get_est_server_ms_at(initial_live_client_wall_ms),
        initial_live_client_wall_ms,
    );

    let (script_raw_code_tx, script_raw_code_rx) = mpsc::unbounded_channel();
    let (client_js_fn_request_tx, client_js_fn_request_rx) = mpsc::unbounded_channel::<(ClientFunctionCallMessage, oneshot::Sender<anyhow::Result<String>>)>();
    let (gui_handler_trigger_tx, gui_handler_trigger_rx) = mpsc::unbounded_channel::<GuiTriggeredHandler>();
    let (volume_tx, volume_rx) = watch::channel(1.0);
    let (play_input_state_tx, play_input_state_rx) = watch::channel(initial_play_input_state);
    let (render_snapshot_tx, render_snapshot_rx) = watch::channel(Arc::new(initial_render_snapshot));
    let (blocked_camera_zoom_tx, blocked_camera_zoom_rx) = watch::channel(initial_blocked_camera_zoom);
    let (last_input_at_tx, _) = watch::channel(Instant::now());

    // start receiving snapshots
    let (snapshot_rx, player_pings_rx) = start_receiving_server_messages_udp_in_bg(connection.clone(), time_estimator_writer, kill_play_cancel_token.clone());

    // start a task to listen to all script-raw-code streams from the server
    tokio::spawn({
        let connection = connection.clone();
        let kill_play_cancel_token = kill_play_cancel_token.clone();
        async move {
            loop {
                let mut recv_stream_script_raw_code = match tokio::select! {
                    _ = kill_play_cancel_token.cancelled() => break,
                    recv_stream_script_raw_code = connection.accept_uni() => recv_stream_script_raw_code,
                } {
                    Ok(recv_stream_script_raw_code) => recv_stream_script_raw_code,
                    Err(error) => {
                        log::warn!("Failed to accept server script stream: {}", error);
                        break;
                    }
                };
                let script_raw_code_message =
                    match awaitTcpMessageMaxBytes::<ServerScriptRawCodeMessage>(&mut recv_stream_script_raw_code, MAX_TCP_MESSAGE_BYTES).await {
                        Ok(script_raw_code_message) => script_raw_code_message,
                        Err(error) => {
                            log::warn!("Failed to decode server script stream message: {}", error);
                            continue;
                        }
                    };
                if script_raw_code_tx.send(script_raw_code_message).is_err() {
                    break;
                }
            }
        }
    });

    // start a task to listen to all client-function requests from the server
    tokio::spawn({
        let connection = connection.clone();
        let kill_play_cancel_token = kill_play_cancel_token.clone();
        async move {
            loop {
                // wait for the next incoming client-function stream
                let (mut send_stream_client_js_fn, mut recv_stream_client_js_fn) = match tokio::select! {
                    _ = kill_play_cancel_token.cancelled() => break,
                    client_function_stream = connection.accept_bi() => client_function_stream,
                } {
                    Ok(client_function_stream) => client_function_stream,
                    Err(error) => {
                        log::warn!("Failed to accept server client-function stream: {}", error);
                        break;
                    }
                };

                // wait for the next incoming client-function call message
                let client_function_call_message =
                    match awaitTcpMessageMaxBytes::<ClientFunctionCallMessage>(&mut recv_stream_client_js_fn, MAX_TCP_MESSAGE_BYTES).await {
                        Ok(client_function_call_message) => client_function_call_message,
                        Err(error) => {
                            log::warn!("Failed to decode server client-function stream message: {}", error);
                            return;
                        }
                    };

                // send the client-function call to the js runtime to run
                let (client_js_fn_oneshot_tx, client_js_fn_oneshot_rx) = oneshot::channel::<anyhow::Result<String>>();
                let send_result = client_js_fn_request_tx.send((client_function_call_message, client_js_fn_oneshot_tx));
                if send_result.is_err() {
                    log::warn!("Could not queue client function call");
                    return;
                }

                // wait for the js runtime result
                let client_js_fn_result = match client_js_fn_oneshot_rx.await {
                    Ok(client_js_fn_result) => client_js_fn_result.map_err(|error| error.to_string()),
                    Err(_) => {
                        log::warn!("Client function result channel dropped");
                        return;
                    }
                };

                // send the client-function result back to the server
                let send_result = sendTcpMessage(&mut send_stream_client_js_fn, &ClientFunctionResultMessage { result: client_js_fn_result }).await;
                if let Err(error) = send_result {
                    log::warn!("Failed to send client function result: {}", error);
                    return;
                }
                let _ = send_stream_client_js_fn.finish();
            }
        }
    });

    // tell server that we're Ready to start receiving snapshots
    tokio::spawn({
        let mut handshake_send_stream = handshake_send_stream;
        async move {
            let _ = sendTcpMessage(&mut handshake_send_stream, &ClientHandshakeTCP::ReadyForUDPMessages).await;
            let _ = handshake_send_stream.finish();
        }
    });

    // GameChat starts independently and only owns its own stream.
    let (game_chat_message_tx, game_chat_history_rx) = start_receiving_game_chat_messages_tcp_in_bg(connection.clone(), kill_play_cancel_token.clone());

    // start sending pings to estimate the time
    tokio::spawn({
        let time_estimator_reader = time_estimator_reader.clone();
        let connection = connection.clone();
        let kill_play_cancel_token = kill_play_cancel_token.clone();
        async move {
            let mut ticker = interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = kill_play_cancel_token.cancelled() => break,
                    _ = ticker.tick() => {}
                }
                let t0 = time_estimator_reader.get_live_client_wall_ms();

                let ping = ClientMessageUDP::Ping(ClientPingMessage { t0_client_send_ms: t0 });
                let res = sendDatagramMessage(&connection, &ping);
                if let Err(error) = res {
                    log::warn!("Error sending ping: {}. The connection may have closed.", error);
                    break;
                }
            }
        }
    });

    // if we disconnect, change the app runner to err and stop all the running tasks we just started above
    tokio::spawn({
        let connection = connection.clone();
        let kill_play_cancel_token = kill_play_cancel_token.clone();
        async move {
            // this only fires when we didn't intentionally close the connection, aka when it's not LocallyClosed
            let disconnect_error = tokio::select! {
                _ = kill_play_cancel_token.cancelled() => return,
                disconnect_error = connection.closed() => disconnect_error,
            };
            kill_play_cancel_token.cancel();
            let err_string: String = match disconnect_error {
                quinn::ConnectionError::LocallyClosed => return, // this happens when we intentionally close the connection, so do nothing
                quinn::ConnectionError::ApplicationClosed(application_close) => String::from_utf8_lossy(&application_close.reason).into_owned(),
                other_error => other_error.to_string(),
            };
            let _ = connection_err_tx.send(AppRunnerInputs::from_error("Disconnected", err_string));
        }
    });

    start_running_play_in_bg(
        PlayLoopInputs {
            world,
            player_object_id,

            snapshot_rx,
            player_pings_rx,
            script_raw_code_rx,
            client_js_fn_request_rx,
            gui_handler_trigger_rx,
            volume_rx,
            js_callback_inputs_rx,
            key_and_mouse_event_rx,
            play_input_state_rx,

            render_snapshot_tx,
            blocked_camera_zoom_tx,

            time_estimator_reader: time_estimator_reader.clone(),
            connection: connection.clone(),
            sound_playback_manager,
            console_log_callback,
            data_fetcher: data_fetcher.clone(),
        },
        kill_play_cancel_token.clone(),
    );

    AppRunnerInputs::PlayMode(PlayRunnerInputs {
        play_input_state_tx,
        render_snapshot_rx,
        blocked_camera_zoom_rx,
        gui_handler_trigger_tx,
        volume_tx,
        key_and_mouse_event_tx,
        last_input_at_tx,

        game_chat_message_tx,
        game_chat_history_rx,

        time_estimator_reader,
        kill_play_cancel_token,
        data_fetcher,
        starter_data_ids,
    })
}

pub async fn start_connection_to_server(
    public_ip: String,
    server_port: u16,
    play_auth: String,
) -> anyhow::Result<(Connection, ServerInitialWorldMessage, SendStream)> {
    async fn create_connection(server_addr: SocketAddr) -> anyhow::Result<(Endpoint, Connection)> {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(QUIC_SERVER_CERT_DER_BYTES))?;
        let mut client_config = ClientConfig::with_root_certificates(Arc::new(roots))?;
        let mut transport = quinn::TransportConfig::default();
        transport.datagram_receive_buffer_size(Some(1024 * 1024));
        transport.datagram_send_buffer_size(1024 * 1024);
        transport.max_idle_timeout(Some(quinn::IdleTimeout::try_from(Duration::from_secs(CLIENT_MAX_IDLE_TIMEOUT_SECS))?));
        client_config.transport_config(Arc::new(transport));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);
        let connection = endpoint.connect(server_addr, QUIC_CERT_SERVER_NAME)?.await?;
        Ok((endpoint, connection))
    }

    log::info!("Starting connection to server: {public_ip}:{server_port}");
    let server_ip: IpAddr = public_ip.parse()?;
    let server_addr = SocketAddr::new(server_ip, server_port);

    log::info!("Creating connection...");
    let (_, connection) = create_connection(server_addr).await?;

    // do the handshake to get the initial world
    let (mut handshake_send_stream, mut handshake_recv_stream) = connection.open_bi().await?;
    let hello_message = ClientHandshakeTCP::Hello(ClientHelloMessage {
        client_server_protocol_version: CLIENT_SERVER_PROTOCOL_VERSION,
        client_app_version: env!("CARGO_PKG_VERSION").to_string(),
        play_auth,
    });
    sendTcpMessage(&mut handshake_send_stream, &hello_message).await?;

    // this is the initial world message, which carries the whole world - it gets the larger cap
    let handshake_message = awaitTcpMessageMaxBytes::<ServerHandshakeTCP>(&mut handshake_recv_stream, MAX_INITIAL_WORLD_MESSAGE_BYTES).await?;
    let ServerHandshakeTCP::InitialWorld(initial_world) = handshake_message;

    log::info!("Received initial world with player_object_id: {:?}", initial_world.player_object_id);
    Ok((connection, initial_world, handshake_send_stream))
}
