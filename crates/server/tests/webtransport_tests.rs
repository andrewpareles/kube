// Players join a real playtest server through its browser endpoint the way a page does: HTTP/3 with WebTransport, then
// the same handshake, datagrams and reliable streams the desktop client uses over QUIC. The endpoint's identity is
// generated here and the client accepts it by its certificate hash, which is the mechanism a browser offers.
// Run with: cargo test -p server --test webtransport_tests

use std::{
    net::UdpSocket,
    sync::Arc,
    time::{Duration, Instant},
};

use game_types::{
    consts::CLIENT_SERVER_PROTOCOL_VERSION,
    object_types::{ObjectId, Orientation3D},
};
use server::{ServerMode, WEBTRANSPORT_PATH, WebTransportServerConfig, playtest_auth::PlaytestAuth, start_running_server_in_bg};
use shared::{
    player_profile::PlayerProfile,
    protocol::{
        messages::{
            ClientAckMessage, ClientGameChatMessage, ClientHandshakeTCP, ClientHelloMessage, ClientInputMessage, ClientMessageTCP, ClientMessageUDP,
            ForeignPlayerDelta, GameChatEntry, ObjectDelta, ServerGameChatMessage, ServerGameChatResultMessage, ServerHandshakeTCP, ServerMessageUDP,
        },
        quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitDatagramMessage, awaitTcpMessageMaxBytes, sendDatagramMessage, sendTcpMessage},
    },
};
use tokio::time::timeout;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use wtransport::{ClientConfig, Endpoint, Identity, endpoint::endpoint_side, tls::Sha256Digest};

// Any world will do: these tests are about the connection, and this one is already small enough to start and stop.
const SMALL_WORLD: &str = include_str!("../../../game-pigball.kube");
const JOIN_TIMEOUT: Duration = Duration::from_secs(10);

struct TestServer {
    port: u16,
    certificate_hash: Sha256Digest,
    auth_arc: Arc<PlaytestAuth>,
    cancel_token: CancellationToken,
}

impl TestServer {
    async fn start() -> Self {
        // The socket takes a free port, then lets it go for the server to bind.
        let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let quic_port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert_ne!(port, quic_port);

        let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let certificate_hash = identity.certificate_chain().as_slice()[0].hash();
        let auth_arc = Arc::new(PlaytestAuth::default());
        let cancel_token = CancellationToken::new();
        start_running_server_in_bg(
            SMALL_WORLD,
            cancel_token.clone(),
            TaskTracker::new(),
            Instant::now(),
            quic_port,
            ServerMode::Playtest(auth_arc.clone()),
            Some(WebTransportServerConfig { identity, port }),
            None,
        )
        .unwrap();
        // Both listeners are bound before a playtest reports itself running, so the browser port answers from here on.
        auth_arc.wait_until_running().await.unwrap();
        Self {
            port,
            certificate_hash,
            auth_arc,
            cancel_token,
        }
    }

    async fn reserve_account(&self, username: &str, userid: i64) -> String {
        self.auth_arc.reserve(username.to_string(), Some(userid), PlayerProfile::GUEST).await
    }

    async fn reserve_guest(&self) -> String {
        self.auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await
    }
}

// One browser page's side of the session.
struct Browser {
    connection: wtransport::Connection,
    player_object_id: ObjectId,
}

// A page accepts the server by the certificate hash it was told to expect, which is what a browser's
// serverCertificateHashes option does.
fn browser_endpoint(server: &TestServer) -> Endpoint<endpoint_side::Client> {
    Endpoint::client(
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([server.certificate_hash.clone()])
            .build(),
    )
    .unwrap()
}

async fn join(client: &Endpoint<endpoint_side::Client>, server: &TestServer, join_token: String) -> Browser {
    let connection = timeout(JOIN_TIMEOUT, client.connect(format!("https://127.0.0.1:{}{WEBTRANSPORT_PATH}", server.port)))
        .await
        .expect("the browser session should open")
        .expect("the browser session should be accepted");

    let (mut send_stream, mut recv_stream) = connection.open_bi().await.unwrap().await.unwrap();
    sendTcpMessage(
        &mut send_stream,
        &ClientHandshakeTCP::Hello(ClientHelloMessage {
            client_server_protocol_version: CLIENT_SERVER_PROTOCOL_VERSION,
            client_app_version: "test".to_string(),
            play_auth: join_token,
        }),
    )
    .await
    .unwrap();
    let ServerHandshakeTCP::InitialWorld(initial_world) = timeout(
        JOIN_TIMEOUT,
        awaitTcpMessageMaxBytes::<ServerHandshakeTCP>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES),
    )
    .await
    .expect("the initial world should arrive")
    .unwrap();
    assert!(!initial_world.stored_world_string.is_empty(), "the server should send the world it runs");
    sendTcpMessage(&mut send_stream, &ClientHandshakeTCP::ReadyForUDPMessages).await.unwrap();
    send_stream.finish().await.unwrap();

    Browser {
        connection,
        player_object_id: initial_world.player_object_id,
    }
}

fn send_input(connection: &wtransport::Connection, input_num: u64, player_metadata: Option<String>) {
    sendDatagramMessage(
        connection,
        &ClientMessageUDP::Input(ClientInputMessage {
            input_num,
            latest_applied_message_num: 0,
            foreign_player_delta: ForeignPlayerDelta {
                cameraOrientation: Orientation3D::Z(),
                playerMetadata: player_metadata,
                est_live_server_sim_ms: 0.0,
            },
            foreign_character_delta: None,
        }),
    )
    .unwrap();
}

// A page acks every snapshot the moment it arrives, and the server holds back anything sent after an unacked change
// until it hears that change was applied.
async fn await_next_server_message(browser: &Browser) -> ServerMessageUDP {
    let message = awaitDatagramMessage::<ServerMessageUDP>(&browser.connection).await.unwrap();
    if let ServerMessageUDP::Snapshot(snapshot) = &message {
        sendDatagramMessage(
            &browser.connection,
            &ClientMessageUDP::Ack(ClientAckMessage {
                message_num: snapshot.message_num,
            }),
        )
        .unwrap();
    }
    message
}

// The first snapshot carrying this player's camera and metadata, which is how the server shows one player to another.
async fn await_foreign_player_metadata(watcher: &Browser, player_object_id: ObjectId, expected_metadata: &str) {
    timeout(JOIN_TIMEOUT, async {
        loop {
            let ServerMessageUDP::Snapshot(snapshot) = await_next_server_message(watcher).await else {
                continue;
            };
            let Some(ObjectDelta::ForeignPlayer(foreign_player_delta)) = snapshot.deltaOfObjectId.get(&player_object_id) else {
                continue;
            };
            if foreign_player_delta.playerMetadata.as_deref() == Some(expected_metadata) {
                return;
            }
        }
    })
    .await
    .expect("the other player's camera and metadata should reach this player");
}

// A chat subscription sends the room's history, then every entry after it. A connection allows one at a time, so asking
// while another is still open gives back None instead of a subscription.
async fn try_subscribe_to_chat(browser: &Browser) -> Option<wtransport::RecvStream> {
    let (mut send_stream, mut recv_stream) = browser.connection.open_bi().await.unwrap().await.unwrap();
    sendTcpMessage(&mut send_stream, &ClientMessageTCP::SubscribeToGameChat).await.unwrap();
    send_stream.finish().await.unwrap();
    let message = awaitTcpMessageMaxBytes::<ServerGameChatMessage>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await;
    match message {
        Ok(ServerGameChatMessage::History(_)) => Some(recv_stream),
        Ok(ServerGameChatMessage::Entry(_)) | Err(_) => None,
    }
}

async fn subscribe_to_chat(browser: &Browser) -> wtransport::RecvStream {
    try_subscribe_to_chat(browser)
        .await
        .expect("a new subscription should start with the room's history")
}

async fn submit_chat_message(browser: &Browser, message: &str) -> Result<(), String> {
    let (mut send_stream, mut recv_stream) = browser.connection.open_bi().await.unwrap().await.unwrap();
    sendTcpMessage(
        &mut send_stream,
        &ClientMessageTCP::GameChat(ClientGameChatMessage { message: message.to_string() }),
    )
    .await
    .unwrap();
    send_stream.finish().await.unwrap();
    awaitTcpMessageMaxBytes::<ServerGameChatResultMessage>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES)
        .await
        .unwrap()
        .result
}

#[tokio::test]
async fn a_browser_joins_over_webtransport_and_its_input_reaches_another_player() {
    let server = TestServer::start().await;
    let client = browser_endpoint(&server);

    let watcher = join(&client, &server, server.reserve_guest().await).await;
    let player = join(&client, &server, server.reserve_guest().await).await;
    assert_ne!(watcher.player_object_id, player.player_object_id);

    // A page sends an input every frame, so a datagram lost on the way is simply replaced by the next one.
    let input_sender = tokio::spawn({
        let connection = player.connection.clone();
        async move {
            let mut input_num = 0;
            loop {
                input_num += 1;
                send_input(&connection, input_num, Some(r#"{"hat":"blue"}"#.to_string()));
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    });
    // The server shows that player's camera and metadata to everyone else.
    await_foreign_player_metadata(&watcher, player.player_object_id, r#"{"hat":"blue"}"#).await;
    input_sender.abort();

    server.cancel_token.cancel();
}

#[tokio::test]
async fn a_browser_chats_with_the_room() {
    let server = TestServer::start().await;
    let client = browser_endpoint(&server);

    let speaker = join(&client, &server, server.reserve_account("Kubester", 17).await).await;
    let listener = join(&client, &server, server.reserve_guest().await).await;

    let mut listener_history = subscribe_to_chat(&listener).await;
    let mut speaker_history = subscribe_to_chat(&speaker).await;

    submit_chat_message(&speaker, "hello from the browser").await.unwrap();

    for (name, subscription) in [("the listener", &mut listener_history), ("the speaker", &mut speaker_history)] {
        let entry = timeout(
            JOIN_TIMEOUT,
            awaitTcpMessageMaxBytes::<ServerGameChatMessage>(subscription, MAX_TCP_MESSAGE_BYTES),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} should receive the message"))
        .unwrap();
        let ServerGameChatMessage::Entry(GameChatEntry::Message(game_chat_message)) = entry else {
            panic!("{name} should receive a player message, not a join or leave notice");
        };
        assert_eq!(game_chat_message.username, "Kubester");
        assert_eq!(game_chat_message.message, "hello from the browser");
    }

    server.cancel_token.cancel();
}

// Stopping a subscription has to reach the server, or that connection can never subscribe again.
#[tokio::test]
async fn a_browser_subscribes_again_after_stopping_its_chat_stream() {
    let server = TestServer::start().await;
    let client = browser_endpoint(&server);
    let browser = join(&client, &server, server.reserve_guest().await).await;

    let subscription = subscribe_to_chat(&browser).await;
    subscription.stop(wtransport::VarInt::from_u32(0));

    // The server lets go of the stream it was writing to, which takes a moment to arrive.
    timeout(JOIN_TIMEOUT, async {
        loop {
            if try_subscribe_to_chat(&browser).await.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the connection should be able to subscribe again");

    server.cancel_token.cancel();
}
