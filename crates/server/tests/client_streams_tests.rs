use std::{sync::Arc, time::Duration};

use game_types::object_types::{ObjectId, player::UserId};
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, ServerConfig};
use rustls::pki_types::PrivateKeyDer;
use server::{GAME_CHAT_SUBMISSION_INTERVAL, ServerGameChat, send_game_chat_messages, start_receiving_client_messages_tcp_in_bg};
use shared::protocol::{
    messages::{
        ClientGameChatMessage, ClientMessageTCP, GameChatEntry, ServerFnCallMessage, ServerFunctionResultMessage, ServerGameChatMessage,
        ServerGameChatResultMessage,
    },
    quic_cert::QUIC_CERT_SERVER_NAME,
    quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitTcpMessageMaxBytes, sendTcpMessage, sendTcpRequest},
};
use tokio::sync::{Mutex, mpsc, oneshot};

const PLAYER_OBJECT_ID: ObjectId = 7;
const USERID: UserId = 17;

// Exercise production stream routing over real QUIC and the real script request channel.
struct ConnectionTest {
    _server_endpoint: Endpoint,
    _client_endpoint: Endpoint,
    client: Connection,
    server: Connection,
    request_rx: mpsc::UnboundedReceiver<(ObjectId, ServerFnCallMessage, oneshot::Sender<anyhow::Result<String>>)>,
}

impl ConnectionTest {
    async fn new() -> Self {
        let rcgen::CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(vec![QUIC_CERT_SERVER_NAME.to_string()]).unwrap();
        let server_config = ServerConfig::with_single_cert(vec![cert.der().clone()], PrivateKeyDer::try_from(key_pair.serialize_der()).unwrap()).unwrap();
        let server_endpoint = Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.der().clone()).unwrap();
        let mut client_endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(ClientConfig::with_root_certificates(Arc::new(roots)).unwrap());
        let connecting = client_endpoint.connect(server_endpoint.local_addr().unwrap(), QUIC_CERT_SERVER_NAME).unwrap();
        let (client, server) = tokio::try_join!(connecting, async { server_endpoint.accept().await.unwrap().await }).unwrap();
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        start_receiving_client_messages_tcp_in_bg(
            server.clone(),
            Arc::new(Mutex::new(ServerGameChat::new(None))),
            PLAYER_OBJECT_ID,
            Some(USERID),
            "Alice".to_string(),
            request_tx,
        );
        Self {
            _server_endpoint: server_endpoint,
            _client_endpoint: client_endpoint,
            client,
            server,
            request_rx,
        }
    }

    async fn call_server_function(&mut self) {
        let message = ClientMessageTCP::ServerFnCall(ServerFnCallMessage {
            fn_id: "example".to_string(),
            // The first message now contains the full call, which can exceed the old header limit.
            js_args: format!("[\"{}\"]", "x".repeat(2048)),
        });
        let request = sendTcpRequest::<_, ServerFunctionResultMessage>(&self.client, &message);
        let handle_request = async {
            let (player_object_id, request, reply_tx) = self.request_rx.recv().await.unwrap();
            assert_eq!(player_object_id, PLAYER_OBJECT_ID);
            assert_eq!(request.fn_id, "example");
            assert_eq!(request.js_args, format!("[\"{}\"]", "x".repeat(2048)));
            reply_tx.send(Ok("42".to_string())).unwrap();
        };
        let (response, ()) = tokio::join!(request, handle_request);
        assert_eq!(response.unwrap().result.unwrap(), "42");
    }

    async fn open_game_chat(&self) -> RecvStream {
        let (mut send_stream, mut recv_stream) = self.client.open_bi().await.unwrap();
        sendTcpMessage(&mut send_stream, &ClientMessageTCP::SubscribeToGameChat).await.unwrap();
        send_stream.finish().unwrap();
        let ServerGameChatMessage::History(_) = awaitTcpMessageMaxBytes(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("chat must begin with history");
        };
        recv_stream
    }

    async fn submit_game_chat(&self, message: &str) -> Result<(), String> {
        let response: ServerGameChatResultMessage = sendTcpRequest(
            &self.client,
            &ClientMessageTCP::GameChat(ClientGameChatMessage { message: message.to_string() }),
        )
        .await
        .unwrap();
        response.result
    }

    async fn exchange_datagrams(&self) {
        self.client.send_datagram(b"game input".as_slice().into()).unwrap();
        assert_eq!(self.server.read_datagram().await.unwrap().as_ref(), b"game input");
        self.server.send_datagram(b"snapshot".as_slice().into()).unwrap();
        assert_eq!(self.client.read_datagram().await.unwrap().as_ref(), b"snapshot");
    }
}

#[tokio::test]
async fn script_calls_work_before_game_chat_starts_and_while_game_chat_is_open() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ConnectionTest::new().await;
        test.call_server_function().await;
        test.exchange_datagrams().await;
        let mut game_chat_recv = test.open_game_chat().await;
        test.call_server_function().await;
        test.submit_game_chat("hello").await.unwrap();
        let ServerGameChatMessage::Entry(GameChatEntry::Message(message)) = awaitTcpMessageMaxBytes(&mut game_chat_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap()
        else {
            panic!("expected chat echo");
        };
        assert_eq!(message.userid, USERID);
        assert_eq!(message.message, "hello");
    })
    .await
    .expect("stream routing test timed out");
}

#[tokio::test]
async fn a_rejected_game_chat_request_leaves_game_chat_script_calls_and_datagrams_working() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ConnectionTest::new().await;
        let mut game_chat_recv = test.open_game_chat().await;
        assert!(test.submit_game_chat("invalid\nmessage").await.is_err());
        test.call_server_function().await;
        test.exchange_datagrams().await;
        test.submit_game_chat("still here").await.unwrap();
        let ServerGameChatMessage::Entry(GameChatEntry::Message(message)) = awaitTcpMessageMaxBytes(&mut game_chat_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap()
        else {
            panic!("expected chat echo");
        };
        assert_eq!(message.message, "still here");
        assert!(test.client.close_reason().is_none());
    })
    .await
    .expect("chat failure isolation test timed out");
}

#[tokio::test]
async fn a_partial_message_does_not_block_other_streams() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ConnectionTest::new().await;
        let (mut stalled_send, _stalled_recv) = test.client.open_bi().await.unwrap();
        // Leave the length prefix incomplete while other streams exchange complete messages.
        stalled_send.write_all(&[0]).await.unwrap();
        test.call_server_function().await;
        let _game_chat_recv = test.open_game_chat().await;
        test.exchange_datagrams().await;
    })
    .await
    .expect("stalled stream isolation test timed out");
}

#[tokio::test]
async fn game_chat_requests_work_before_a_subscription_and_are_in_its_history() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ConnectionTest::new().await;
        test.submit_game_chat("before subscribing").await.unwrap();
        test.call_server_function().await;
        let (mut send_stream, mut recv_stream) = test.client.open_bi().await.unwrap();
        sendTcpMessage(&mut send_stream, &ClientMessageTCP::SubscribeToGameChat).await.unwrap();
        send_stream.finish().unwrap();
        let ServerGameChatMessage::History(history) = awaitTcpMessageMaxBytes(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected history");
        };
        assert_eq!(history.len(), 1);
        let GameChatEntry::Message(message) = &history[0] else {
            panic!("expected a player message");
        };
        assert_eq!(message.message, "before subscribing");
    })
    .await
    .expect("independent chat request test timed out");
}

#[tokio::test]
async fn a_duplicate_game_chat_stream_does_not_interrupt_the_existing_game_chat() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ConnectionTest::new().await;
        let mut game_chat_recv = test.open_game_chat().await;
        let (mut duplicate_send, mut duplicate_recv) = test.client.open_bi().await.unwrap();
        sendTcpMessage(&mut duplicate_send, &ClientMessageTCP::SubscribeToGameChat).await.unwrap();
        assert!(
            awaitTcpMessageMaxBytes::<ServerGameChatMessage>(&mut duplicate_recv, MAX_TCP_MESSAGE_BYTES)
                .await
                .is_err()
        );
        test.submit_game_chat("still here").await.unwrap();
        let ServerGameChatMessage::Entry(GameChatEntry::Message(message)) = awaitTcpMessageMaxBytes(&mut game_chat_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap()
        else {
            panic!("expected chat echo");
        };
        assert_eq!(message.message, "still here");
        test.call_server_function().await;
    })
    .await
    .expect("duplicate chat isolation test timed out");
}

#[tokio::test]
async fn separate_game_chat_request_streams_share_the_connection_rate_limit() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let test = ConnectionTest::new().await;
        let started = tokio::time::Instant::now();
        test.submit_game_chat("first").await.unwrap();
        test.submit_game_chat("second").await.unwrap();
        assert!(started.elapsed() >= GAME_CHAT_SUBMISSION_INTERVAL);
    })
    .await
    .expect("chat request rate limit test timed out");
}

#[tokio::test]
async fn stopping_an_idle_subscription_ends_its_sender() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let test = ConnectionTest::new().await;
        let (send_stream, _recv_stream) = test.server.open_bi().await.unwrap();
        let game_chat = Mutex::new(ServerGameChat::new(None));
        let sending = send_game_chat_messages(&game_chat, send_stream);
        let stop_subscription = async {
            let (_send_stream, mut recv_stream) = test.client.accept_bi().await.unwrap();
            let ServerGameChatMessage::History(_) = awaitTcpMessageMaxBytes(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
                panic!("expected history");
            };
            recv_stream.stop(0_u32.into()).unwrap();
        };
        let (result, ()) = tokio::join!(sending, stop_subscription);
        result.unwrap();
        test.exchange_datagrams().await;
    })
    .await
    .expect("subscription cancellation test timed out");
}
