use crate::screenshot::{ScreenshotRequest, draw_png_in_child_process};
use anyhow::{Context, Result};
use client::app_runner::AppRunnerInputs;
use client::play_runner::get_play_app_runner_inputs_and_start_bg_tasks;
use client::play_runner::input_snapshot::PlayInputState;
use client::play_runner::play_renderer::{PlayRunnerInputs, place_play_camera, pose_render_objects};
use client::splash_runner::SplashScreenContent;
use game_types::{consts::MAX_PLAYTEST_CLIENTS, object_types::ObjectKind};
use render_core::camera::DEFAULT_CAMERA_FOV_DEGREES;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use server::{ServerMode, playtest_auth::PlaytestAuth, start_running_server_in_bg};
use shared::game::data_fetcher::DataFetcherArc;
use shared::game::jsRuntime::{JsConsoleLogCallback, JsConsoleLogEntry};
use shared::game::world::World;
use shared::player_profile::PlayerProfile;
use std::{future::Future, io::Write, net::UdpSocket, path::Path, process::Stdio, sync::Arc, time::Duration, time::Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, RwLock, mpsc, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

// Started with this argument, the editor_server binary runs one playtest: its server and the headless players the AI adds.
// A crash in a game's scripts or in the engine then ends the playtest instead of the editor.
pub const RUN_PLAYTEST_ARG: &str = "run-playtest";

// TODO: AI-PICKED-VALUE: Nothing the editor and a playtest send each other comes near 256 MB, so a longer message means the stream is broken.
const MAX_MESSAGE_BYTES: u32 = 256 * 1024 * 1024;
// TODO: AI-PICKED-VALUE: A stopped playtest gets five seconds to close its players' connections before it is killed.
const PLAYTEST_STOP_TIMEOUT: Duration = Duration::from_secs(5);

// -------------------------- What the editor and a playtest send each other --------------------------

// The first message a playtest process reads. Without a port, its server takes a free one.
#[derive(Serialize, Deserialize)]
struct PlaytestStart {
    stored_world_string: String,
    server_port: Option<u16>,
}

// A playtest answers its requests one at a time, in order, each with the value named here.
#[derive(Serialize, Deserialize)]
pub enum PlaytestRequest {
    // The token a person's desktop client joins with.
    AddPlayer {
        username: String,
        user_id: Option<i64>,
        profile: PlayerProfile,
    },
    // The new headless player's client id.
    StartClient,
    // Nothing.
    StopClient {
        client_id: u8,
    },
    // A PNG of what the player sees, with the UI their client scripts show.
    CaptureScreenshot {
        client_id: u8,
        width: u32,
        height: u32,
    },
    // Whether anyone is playing.
    HasPlayers,
}

#[derive(Serialize, Deserialize)]
enum PlaytestMessage {
    Log {
        source: PlaytestLogSource,
        log_level: String,
        message: String,
    },
    // The bincode of the value the request names.
    Answer(Result<Vec<u8>, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaytestLogSource {
    Server,
    Client { client_id: u8 },
}

impl PlaytestLogSource {
    pub fn to_text(self) -> String {
        match self {
            PlaytestLogSource::Server => "server".to_string(),
            PlaytestLogSource::Client { client_id } => format!("client_{client_id}"),
        }
    }

    pub fn from_text(source_text: &str) -> Option<Self> {
        // Server
        if source_text == "server" {
            return Some(PlaytestLogSource::Server);
        }
        // Client
        let client_id = source_text.strip_prefix("client_")?;
        let client_id = client_id.parse::<u8>().ok()?;
        Some(PlaytestLogSource::Client { client_id })
    }
}

// Each message is its bincode, after its length.
async fn write_message(writer: &mut (impl AsyncWrite + Unpin), message: &impl Serialize) -> Result<()> {
    let bytes = bincode::serialize(message)?;
    writer.write_u32(u32::try_from(bytes.len())?).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_message<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin)) -> Result<T> {
    let length = reader.read_u32().await?;
    anyhow::ensure!(length <= MAX_MESSAGE_BYTES, "A playtest message said it was {length} bytes long.");
    let mut bytes = vec![0; length as usize];
    reader.read_exact(&mut bytes).await?;
    Ok(bincode::deserialize(&bytes)?)
}

// -------------------------- The editor's side --------------------------

struct PlaytestRequestPipe {
    stdin: ChildStdin,
    answer_rx: mpsc::UnboundedReceiver<Result<Vec<u8>, String>>,
}

// A playtest running in its own process. Dropping it stops the process.
pub struct PlaytestProcess {
    // Taken when the playtest is told to stop: its process stops once its stdin closes.
    request_pipe: Mutex<Option<PlaytestRequestPipe>>,
    stop_reason_rx: watch::Receiver<Option<String>>,
    stop_token: CancellationToken,
}

impl PlaytestProcess {
    // Returns once the server takes players, with the port it took.
    pub async fn start(
        program: &Path,
        stored_world_string: String,
        server_port: Option<u16>,
        on_log: impl Fn(PlaytestLogSource, String, String) + Send + 'static,
    ) -> Result<(Self, u16)> {
        let mut child = Command::new(program)
            .arg(RUN_PLAYTEST_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = child.stdin.take().context("The playtest process has no stdin.")?;
        let mut stdout = child.stdout.take().context("The playtest process has no stdout.")?;
        write_message(
            &mut stdin,
            &PlaytestStart {
                stored_world_string,
                server_port,
            },
        )
        .await?;

        // Hands answers to requests and lines to on_log until the process exits, then says why it stopped.
        let (answer_tx, answer_rx) = mpsc::unbounded_channel();
        let (stop_reason_tx, stop_reason_rx) = watch::channel(None);
        let stop_token = CancellationToken::new();
        let reader_stop_token = stop_token.clone();
        tokio::spawn(async move {
            let kill_deadline = async {
                reader_stop_token.cancelled().await;
                tokio::time::sleep(PLAYTEST_STOP_TIMEOUT).await;
            };
            tokio::pin!(kill_deadline);
            loop {
                let message = tokio::select! {
                    _ = &mut kill_deadline => {
                        let _ = child.start_kill();
                        break;
                    }
                    message = read_message(&mut stdout) => message,
                };
                match message {
                    Ok(PlaytestMessage::Log { source, log_level, message }) => on_log(source, log_level, message),
                    Ok(PlaytestMessage::Answer(answer)) => {
                        let _ = answer_tx.send(answer);
                    }
                    Err(_) => break,
                }
            }
            let stop_reason = match child.wait().await {
                Ok(status) => format!("The playtest stopped ({status})."),
                Err(error) => format!("The playtest stopped: {error}"),
            };
            stop_reason_tx.send_replace(Some(stop_reason));
        });

        let process = Self {
            request_pipe: Mutex::new(Some(PlaytestRequestPipe { stdin, answer_rx })),
            stop_reason_rx,
            stop_token,
        };
        let mut request_pipe_lock = process.request_pipe.lock().await;
        let answer = match request_pipe_lock.as_mut() {
            Some(request_pipe) => request_pipe.answer_rx.recv().await,
            None => None,
        };
        drop(request_pipe_lock);
        let server_port = process.decode_answer(answer).await?;
        Ok((process, server_port))
    }

    pub async fn request<T: DeserializeOwned>(&self, request: PlaytestRequest) -> Result<T> {
        let mut request_pipe_lock = self.request_pipe.lock().await;
        let answer = match request_pipe_lock.as_mut() {
            Some(request_pipe) => match write_message(&mut request_pipe.stdin, &request).await {
                Ok(()) => request_pipe.answer_rx.recv().await,
                Err(_) => None,
            },
            None => None,
        };
        drop(request_pipe_lock);
        self.decode_answer(answer).await
    }

    // No answer means the process stopped, and the error says why.
    async fn decode_answer<T: DeserializeOwned>(&self, answer: Option<Result<Vec<u8>, String>>) -> Result<T> {
        let Some(answer) = answer else {
            anyhow::bail!(self.stopped().await);
        };
        Ok(bincode::deserialize(&answer.map_err(anyhow::Error::msg)?)?)
    }

    // Lets the playtest close its players' connections, then waits for the process to exit.
    pub async fn stop(&self) {
        self.request_pipe.lock().await.take();
        self.stop_token.cancel();
        self.stopped().await;
    }

    pub fn has_stopped(&self) -> bool {
        self.stop_reason_rx.borrow().is_some()
    }

    // Says why the process stopped, once it has. It does not borrow the process, so it can wait past it.
    pub fn stopped(&self) -> impl Future<Output = String> + Send + 'static {
        let mut stop_reason_rx = self.stop_reason_rx.clone();
        async move {
            match stop_reason_rx.wait_for(Option::is_some).await {
                Ok(stop_reason) => stop_reason.clone().unwrap_or_default(),
                Err(_) => "The playtest stopped.".to_string(),
            }
        }
    }
}

impl Drop for PlaytestProcess {
    fn drop(&mut self) {
        self.request_pipe.get_mut().take();
        self.stop_token.cancel();
    }
}

// -------------------------- The playtest process's side --------------------------

// The server and every headless player of one playtest.
struct Playtest {
    server_cancel_token: CancellationToken,
    server_port: u16,
    auth_arc: Arc<PlaytestAuth>,
    server_world_arc: Arc<RwLock<World>>,
    data_fetcher: DataFetcherArc, // Every client shares one data fetcher so the same meshes and textures are only downloaded once
    clients: Vec<(u8, PlayRunnerInputs)>, // (client_id, what a window would draw from) for clients that are fully connected
}

// Holding stdout's lock for a whole message keeps the server's and every player's thread from splitting each other's messages.
fn send_message(message: &PlaytestMessage) {
    let Ok(bytes) = bincode::serialize(message) else {
        return;
    };
    let mut stdout_lock = std::io::stdout().lock();
    let _ = stdout_lock
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|()| stdout_lock.write_all(&bytes))
        .and_then(|()| stdout_lock.flush());
    drop(stdout_lock);
}

fn send_log(source: PlaytestLogSource, log_level: String, message: String) {
    send_message(&PlaytestMessage::Log { source, log_level, message });
}

// Get the callback to use when there is a new console.log on this source
fn get_console_log_callback(source: PlaytestLogSource) -> JsConsoleLogCallback {
    Arc::new(move |console_log_entry: JsConsoleLogEntry| send_log(source, console_log_entry.log_level, console_log_entry.message))
}

// Create connection err listener for this source and return its tx. A headless client has no splash screen to
// show connection errors on, so they become error log entries instead of being thrown away. the channel is not
// Ours: starting a play client requires one.
fn create_connection_err_listener(source: PlaytestLogSource) -> mpsc::UnboundedSender<AppRunnerInputs> {
    let (connection_err_tx, mut connection_err_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(app_runner_inputs) = connection_err_rx.recv().await {
            let AppRunnerInputs::Splash(SplashScreenContent { title_text, sub_text, .. }) = app_runner_inputs else {
                continue;
            };
            send_log(source, "error".to_string(), format!("{title_text}: {}", sub_text.unwrap_or_default()));
        }
    });
    connection_err_tx
}

fn get_all_connected_client_ids_text(clients: &[(u8, PlayRunnerInputs)]) -> String {
    if clients.is_empty() {
        return "none".to_string();
    }
    clients
        .iter()
        .map(|(client_id, _)| PlaytestLogSource::Client { client_id: *client_id }.to_text())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Playtest {
    async fn start(
        PlaytestStart {
            stored_world_string,
            server_port,
        }: PlaytestStart,
    ) -> Result<Self> {
        let server_port = match server_port {
            Some(server_port) => server_port,
            // Let the OS hand us a free port by binding one and letting it go, so the test server never fights the editor's own play server for a port
            None => UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port(),
        };
        let server_cancel_token = CancellationToken::new();
        let auth_arc = Arc::new(PlaytestAuth::default());
        let server_world_arc = start_running_server_in_bg(
            &stored_world_string,
            server_cancel_token.clone(),
            TaskTracker::new(),
            Instant::now(),
            server_port,
            ServerMode::Playtest(auth_arc.clone()),
            Some(get_console_log_callback(PlaytestLogSource::Server)),
        )
        .context("The playtest server could not start.")?;
        auth_arc.wait_until_running().await?;
        Ok(Self {
            server_cancel_token,
            server_port,
            auth_arc,
            server_world_arc,
            data_fetcher: DataFetcherArc::new(),
            clients: Vec::new(),
        })
    }

    async fn answer(&mut self, request: PlaytestRequest) -> Result<Vec<u8>> {
        Ok(match request {
            PlaytestRequest::AddPlayer { username, user_id, profile } => bincode::serialize(&self.auth_arc.reserve(username, user_id, profile).await)?,
            PlaytestRequest::StartClient => bincode::serialize(&self.start_client().await?)?,
            PlaytestRequest::StopClient { client_id } => bincode::serialize(&self.stop_client(client_id)?)?,
            PlaytestRequest::CaptureScreenshot { client_id, width, height } => bincode::serialize(&self.capture_screenshot(client_id, width, height).await?)?,
            PlaytestRequest::HasPlayers => bincode::serialize(&self.has_players().await)?,
        })
    }

    // Connect one more headless player to the server and hand back the id the AI uses to talk about it.
    async fn start_client(&mut self) -> Result<u8> {
        let client_id = (1..=MAX_PLAYTEST_CLIENTS as u8)
            .find(|client_id| !self.clients.iter().any(|(connected_client_id, _)| connected_client_id == client_id))
            .ok_or_else(|| anyhow::anyhow!("a playtest can only have {MAX_PLAYTEST_CLIENTS} players connected at once"))?;
        let source = PlaytestLogSource::Client { client_id };
        let join_token = self.auth_arc.reserve(format!("Player {client_id}"), None, PlayerProfile::GUEST).await;
        let app_runner_inputs = get_play_app_runner_inputs_and_start_bg_tasks(
            "127.0.0.1".to_string(),
            self.server_port,
            join_token,
            self.data_fetcher.clone(),
            None, // Headless clients take no audio device
            Some(get_console_log_callback(source)),
            create_connection_err_listener(source),
        )
        .await;
        let AppRunnerInputs::PlayMode(play_runner_inputs) = app_runner_inputs else {
            let error_text = match app_runner_inputs {
                AppRunnerInputs::Splash(SplashScreenContent { title_text, sub_text, .. }) => format!("{title_text}: {}", sub_text.unwrap_or_default()),
                _ => "the test client did not start in play mode".to_string(),
            };
            return Err(anyhow::anyhow!("test client {client_id} could not connect: {error_text}"));
        };
        self.clients.push((client_id, play_runner_inputs));
        send_log(source, "info".to_string(), format!("test client {client_id} connected"));
        Ok(client_id)
    }

    // Make one player leave the game, the way a real player quitting would. The server and the other players keep running.
    fn stop_client(&mut self, client_id: u8) -> Result<()> {
        let source = PlaytestLogSource::Client { client_id };
        let client_index = self
            .clients
            .iter()
            .position(|(connected_client_id, _)| *connected_client_id == client_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} is not connected to the playtest. connected: {}",
                    source.to_text(),
                    get_all_connected_client_ids_text(&self.clients)
                )
            })?;
        let (_client_id, play_runner_inputs) = self.clients.remove(client_index);
        play_runner_inputs.kill_play_cancel_token.cancel();
        send_log(source, "info".to_string(), format!("test client {client_id} quit the game"));
        Ok(())
    }

    // What one player sees right now: their own camera, with the UI their client scripts show.
    async fn capture_screenshot(&self, client_id: u8, width: u32, height: u32) -> Result<Vec<u8>> {
        let Some((_, play_runner_inputs)) = self.clients.iter().find(|(connected_client_id, _)| *connected_client_id == client_id) else {
            return Err(anyhow::anyhow!(
                "{} is not connected to the playtest. connected: {}",
                PlaytestLogSource::Client { client_id }.to_text(),
                get_all_connected_client_ids_text(&self.clients)
            ));
        };
        let render_snapshot = play_runner_inputs.render_snapshot_rx.borrow().clone();
        let camera_zoom = *play_runner_inputs.blocked_camera_zoom_rx.borrow();
        let time_estimator_reader = &play_runner_inputs.time_estimator_reader;
        let redraw_live_client_wall_ms = time_estimator_reader.get_live_client_wall_ms();
        let redraw_dely_client_wall_ms = redraw_live_client_wall_ms - render_snapshot.snapshot_client_world_delay_ms as f64;
        let (render_object_of_id, skeleton_pose_of_id) = pose_render_objects(
            &render_snapshot,
            &render_snapshot,
            redraw_live_client_wall_ms,
            redraw_dely_client_wall_ms,
            time_estimator_reader.get_est_server_ms_at(redraw_dely_client_wall_ms),
        );
        let (camera_position, camera_target_position, _) = place_play_camera(
            render_snapshot.find_camera_target_position(&render_object_of_id, &skeleton_pose_of_id),
            &render_snapshot
                .camera_limits
                .locked_orientation
                .unwrap_or(PlayInputState::new().camera_orientation),
            camera_zoom,
            render_snapshot.camera_limits.offset,
            render_snapshot.fallback_camera_location,
        );
        let request = ScreenshotRequest::new(
            &self.data_fetcher,
            render_object_of_id,
            skeleton_pose_of_id,
            Some(render_snapshot.gui_root_of_script_gui_key.clone()),
            camera_position,
            camera_target_position,
            render_snapshot.camera_limits.fov_degrees.unwrap_or(DEFAULT_CAMERA_FOV_DEGREES),
            width,
            height,
        )
        .await?;
        draw_png_in_child_process(&std::env::current_exe()?, &request).await
    }

    async fn has_players(&self) -> bool {
        let world_lock = self.server_world_arc.read().await;
        let has_players = world_lock
            ._kindOfObjectId
            .get_keys(&ObjectKind::Player)
            .is_some_and(|players| !players.is_empty());
        drop(world_lock);
        has_players
    }

    // Takes the playtest by value because cancelling the tokens is the end of it, there is nothing left to run afterwards
    async fn stop(self) {
        for (_client_id, play_runner_inputs) in &self.clients {
            play_runner_inputs.kill_play_cancel_token.cancel();
        }
        self.server_cancel_token.cancel();
        let _ = self.auth_arc.wait_until_stopped().await;
    }
}

// The playtest process: start the server from the first message, answer requests until the editor closes stdin, then stop.
pub async fn run_playtest() -> Result<()> {
    // Linux kills this process first when memory runs out.
    let _ = std::fs::write("/proc/self/oom_score_adj", "1000");
    let mut stdin = tokio::io::stdin();
    let mut playtest = match Playtest::start(read_message(&mut stdin).await?).await {
        Ok(playtest) => playtest,
        Err(error) => {
            send_message(&PlaytestMessage::Answer(Err(error.to_string())));
            return Ok(());
        }
    };
    send_message(&PlaytestMessage::Answer(
        bincode::serialize(&playtest.server_port).map_err(|error| error.to_string()),
    ));
    loop {
        let request = tokio::select! {
            _ = playtest.server_cancel_token.cancelled() => break,
            request = read_message(&mut stdin) => request,
        };
        let Ok(request) = request else {
            break;
        };
        let answer = playtest.answer(request).await.map_err(|error| error.to_string());
        send_message(&PlaytestMessage::Answer(answer));
    }
    playtest.stop().await;
    Ok(())
}
