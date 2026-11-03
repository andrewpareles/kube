use std::{collections::VecDeque, sync::Arc, time::Duration};

use client::play_runner::game_chat_networking::start_receiving_game_chat_messages_tcp_in_bg;
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream, ServerConfig};
use rustls::pki_types::PrivateKeyDer;
use shared::protocol::{
    messages::{
        ClientGameChatMessage, ClientMessageTCP, GAME_CHAT_HISTORY_LIMIT, GameChatEntry, GameChatMessage, ServerFnCallMessage, ServerFunctionResultMessage,
        ServerGameChatMessage, ServerGameChatResultMessage,
    },
    quic_cert::QUIC_CERT_SERVER_NAME,
    quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitTcpMessageMaxBytes, sendTcpMessage},
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

// Run the production client task against a real local QUIC peer, without a game world or database.
struct GameChatConnectionTest {
    _server_endpoint: Endpoint,
    _client_endpoint: Endpoint,
    server_connection: Connection,
    client_connection: Connection,
    server_send_stream: SendStream,
    server_recv_stream: RecvStream,
    game_chat_message_tx: mpsc::Sender<ClientGameChatMessage>,
    game_chat_history_rx: watch::Receiver<VecDeque<GameChatEntry>>,
    kill_play_cancel_token: CancellationToken,
}

impl GameChatConnectionTest {
    async fn new() -> Self {
        let rcgen::CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(vec![QUIC_CERT_SERVER_NAME.to_string()]).unwrap();
        let server_config = ServerConfig::with_single_cert(vec![cert.der().clone()], PrivateKeyDer::try_from(key_pair.serialize_der()).unwrap()).unwrap();
        let server_endpoint = Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.der().clone()).unwrap();
        let mut client_endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(ClientConfig::with_root_certificates(Arc::new(roots)).unwrap());
        let connecting = client_endpoint.connect(server_endpoint.local_addr().unwrap(), QUIC_CERT_SERVER_NAME).unwrap();
        let (client_connection, server_connection) = tokio::try_join!(connecting, async { server_endpoint.accept().await.unwrap().await }).unwrap();
        // Exercise chat directly on the QUIC connection, without snapshot setup.
        let kill_play_cancel_token = CancellationToken::new();
        let (game_chat_message_tx, game_chat_history_rx) =
            start_receiving_game_chat_messages_tcp_in_bg(client_connection.clone(), kill_play_cancel_token.clone());
        let (server_send_stream, mut server_recv_stream) = server_connection.accept_bi().await.unwrap();
        let ClientMessageTCP::SubscribeToGameChat = awaitTcpMessageMaxBytes(&mut server_recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected a chat stream");
        };
        Self {
            _server_endpoint: server_endpoint,
            _client_endpoint: client_endpoint,
            server_connection,
            client_connection,
            server_send_stream,
            server_recv_stream,
            game_chat_message_tx,
            game_chat_history_rx,
            kill_play_cancel_token,
        }
    }

    async fn receive_game_chat_request(&self) -> String {
        let (mut send_stream, mut recv_stream) = self.server_connection.accept_bi().await.unwrap();
        assert_ne!(recv_stream.id(), self.server_recv_stream.id());
        let ClientMessageTCP::GameChat(ClientGameChatMessage { message }) = awaitTcpMessageMaxBytes(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap()
        else {
            panic!("expected a chat request");
        };
        assert_eq!(recv_stream.read(&mut [0]).await.unwrap(), None);
        sendTcpMessage(&mut send_stream, &ServerGameChatResultMessage { result: Ok(()) }).await.unwrap();
        send_stream.finish().unwrap();
        message
    }
}

#[tokio::test]
async fn sends_game_chat_and_keeps_receiving_bounded_history_after_the_input_is_dropped() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = GameChatConnectionTest::new().await;
        let history: VecDeque<_> = (0..GAME_CHAT_HISTORY_LIMIT)
            .map(|index| {
                GameChatEntry::Message(GameChatMessage {
                    time_live_server_ms: index as u64,
                    userid: 1,
                    username: "Alice".to_string(),
                    message: index.to_string(),
                })
            })
            .collect();
        sendTcpMessage(&mut test.server_send_stream, &ServerGameChatMessage::History(history))
            .await
            .unwrap();
        test.game_chat_history_rx
            .wait_for(|history| history.len() == GAME_CHAT_HISTORY_LIMIT)
            .await
            .unwrap();

        test.game_chat_message_tx
            .send(ClientGameChatMessage { message: "hello".to_string() })
            .await
            .unwrap();
        let message = test.receive_game_chat_request().await;
        assert_eq!(message, "hello");

        // A headless client drops its UI channels, but must keep receiving without closing the stream.
        drop(test.game_chat_message_tx);
        let echo = GameChatEntry::Message(GameChatMessage {
            time_live_server_ms: GAME_CHAT_HISTORY_LIMIT as u64,
            userid: 1,
            username: "Alice".to_string(),
            message,
        });
        sendTcpMessage(&mut test.server_send_stream, &ServerGameChatMessage::Entry(echo.clone()))
            .await
            .unwrap();
        let history = test.game_chat_history_rx.wait_for(|history| history.back() == Some(&echo)).await.unwrap();
        assert_eq!(history.len(), GAME_CHAT_HISTORY_LIMIT);
        let Some(GameChatEntry::Message(oldest)) = history.front() else {
            panic!("expected a player message");
        };
        assert_eq!(oldest.message, "1");
        drop(history);
        assert!(test.client_connection.close_reason().is_none());
        test.kill_play_cancel_token.cancel();
        assert!(test.game_chat_history_rx.wait_for(|_| false).await.is_err());
    })
    .await
    .expect("chat networking test timed out");
}

#[tokio::test]
async fn script_call_streams_remain_separate_while_game_chat_is_open() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let test = GameChatConnectionTest::new().await;
        let (mut script_send_stream, mut script_recv_stream) = test.client_connection.open_bi().await.unwrap();
        sendTcpMessage(
            &mut script_send_stream,
            &ClientMessageTCP::ServerFnCall(ServerFnCallMessage {
                fn_id: "test".to_string(),
                js_args: "[]".to_string(),
            }),
        )
        .await
        .unwrap();
        let (mut server_script_send_stream, mut server_script_recv_stream) = test.server_connection.accept_bi().await.unwrap();
        assert_ne!(server_script_recv_stream.id(), test.server_recv_stream.id());
        let ClientMessageTCP::ServerFnCall(request) = awaitTcpMessageMaxBytes(&mut server_script_recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected a script stream");
        };
        assert_eq!(request.fn_id, "test");
        assert_eq!(request.js_args, "[]");
        sendTcpMessage(
            &mut server_script_send_stream,
            &ServerFunctionResultMessage {
                result: Ok("null".to_string()),
            },
        )
        .await
        .unwrap();
        server_script_send_stream.finish().unwrap();
        let response: ServerFunctionResultMessage = awaitTcpMessageMaxBytes(&mut script_recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap();
        assert_eq!(response.result.unwrap(), "null");
        script_send_stream.finish().unwrap();

        test.game_chat_message_tx
            .send(ClientGameChatMessage {
                message: "still chatting".to_string(),
            })
            .await
            .unwrap();
        let message = test.receive_game_chat_request().await;
        assert_eq!(message, "still chatting");
        test.kill_play_cancel_token.cancel();
    })
    .await
    .expect("separate script and chat streams test timed out");
}

#[tokio::test]
async fn cancelling_an_idle_game_chat_stops_both_loops() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = GameChatConnectionTest::new().await;
        test.kill_play_cancel_token.cancel();
        test.game_chat_message_tx.closed().await;
        assert!(test.game_chat_history_rx.wait_for(|_| false).await.is_err());
    })
    .await
    .expect("chat cancellation test timed out");
}

#[tokio::test]
async fn a_failed_request_does_not_stop_game_chat_subscription_or_later_requests() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = GameChatConnectionTest::new().await;
        test.game_chat_message_tx
            .send(ClientGameChatMessage { message: "first".to_string() })
            .await
            .unwrap();
        let (mut send_stream, mut recv_stream) = test.server_connection.accept_bi().await.unwrap();
        let ClientMessageTCP::GameChat(_) = awaitTcpMessageMaxBytes(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected a chat request");
        };
        send_stream.reset(0_u32.into()).unwrap();

        test.game_chat_message_tx
            .send(ClientGameChatMessage { message: "next".to_string() })
            .await
            .unwrap();
        assert_eq!(test.receive_game_chat_request().await, "next");
        let still_receiving = GameChatEntry::Message(GameChatMessage {
            time_live_server_ms: 1_000,
            userid: 1,
            username: "Alice".to_string(),
            message: "still receiving".to_string(),
        });
        sendTcpMessage(&mut test.server_send_stream, &ServerGameChatMessage::Entry(still_receiving.clone()))
            .await
            .unwrap();
        test.game_chat_history_rx
            .wait_for(|history| history.back() == Some(&still_receiving))
            .await
            .unwrap();
        assert!(!test.game_chat_message_tx.is_closed());
        assert!(!test.kill_play_cancel_token.is_cancelled());
        test.kill_play_cancel_token.cancel();
    })
    .await
    .expect("chat request failure test timed out");
}

#[tokio::test]
async fn a_stream_failure_closes_only_game_chat_and_keeps_gameplay_connected() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = GameChatConnectionTest::new().await;
        test.server_send_stream.reset(0_u32.into()).unwrap();
        test.game_chat_message_tx.closed().await;
        assert!(test.game_chat_history_rx.wait_for(|_| false).await.is_err());
        assert!(!test.kill_play_cancel_token.is_cancelled());
        assert!(test.client_connection.close_reason().is_none());
        test.client_connection.send_datagram(b"game input".as_slice().into()).unwrap();
        assert_eq!(test.server_connection.read_datagram().await.unwrap().as_ref(), b"game input");
        test.server_connection.send_datagram(b"snapshot".as_slice().into()).unwrap();
        assert_eq!(test.client_connection.read_datagram().await.unwrap().as_ref(), b"snapshot");
    })
    .await
    .expect("chat failure test timed out");
}
