use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use bincode::Options;
use game_types::object_types::player::UserId;
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, ServerConfig};
use rustls::pki_types::PrivateKeyDer;
use server::{ServerGameChat, start_receiving_client_messages_tcp_in_bg};
use shared::game::world::JsCallbackInputs;
use shared::protocol::{
    bincode_config::get_bincode_config,
    messages::{
        ClientGameChatMessage, ClientMessageTCP, GAME_CHAT_HISTORY_LIMIT, GAME_CHAT_MAX_MESSAGE_CHARS, GameChatEntry, GameChatMessage, ServerGameChatMessage,
        ServerGameChatResultMessage,
    },
    quic_cert::QUIC_CERT_SERVER_NAME,
    quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitTcpMessageMaxBytes, sendTcpMessage, sendTcpRequest},
};
use tokio::sync::{Mutex, mpsc};

// Most tests here only submit player messages, so they read each entry as one.
fn expect_message(entry: &GameChatEntry) -> &GameChatMessage {
    let GameChatEntry::Message(message) = entry else {
        panic!("expected a player message, got {entry:?}");
    };
    message
}

#[test]
fn room_assigns_identity_and_time_and_echoes_the_same_message_to_every_subscriber() {
    let mut room = ServerGameChat::new(None);
    let (_, mut first_rx) = room.subscribe();
    let (_, mut second_rx) = room.subscribe();
    let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    room.submit(17, "Alice", "  hello 🌍  ".to_string()).unwrap();
    let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let first = first_rx.try_recv().unwrap();
    assert_eq!(first, second_rx.try_recv().unwrap());
    let first_message = expect_message(&first);
    assert_eq!(first_message.userid, 17);
    assert_eq!(first_message.username, "Alice");
    assert_eq!(first_message.message, "hello 🌍");
    assert!((before..=after).contains(&u128::from(first_message.time_live_server_ms)));
    let (history, _) = room.subscribe();
    assert_eq!(history.front(), Some(&first));
}

#[test]
fn joining_has_no_history_live_overlap_and_rooms_are_isolated() {
    let mut first_room = ServerGameChat::new(None);
    let second_room = ServerGameChat::new(None);
    first_room.submit(1, "Alice", "before join".to_string()).unwrap();
    let (history, mut live_rx) = first_room.subscribe();
    first_room.submit(2, "Bob", "after join".to_string()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(expect_message(history.front().unwrap()).message, "before join");
    assert_eq!(expect_message(&live_rx.try_recv().unwrap()).message, "after join");
    assert!(live_rx.try_recv().is_err());
    let (second_history, _) = second_room.subscribe();
    let (restarted_history, _) = ServerGameChat::new(None).subscribe();
    assert!(second_history.is_empty());
    assert!(restarted_history.is_empty());
}

#[test]
fn history_is_bounded_and_keeps_the_latest_messages_in_order() {
    let mut room = ServerGameChat::new(None);
    for index in 0..GAME_CHAT_HISTORY_LIMIT + 3 {
        room.submit(1, "Alice", index.to_string()).unwrap();
    }
    let (history, _) = room.subscribe();
    assert_eq!(history.len(), GAME_CHAT_HISTORY_LIMIT);
    assert_eq!(expect_message(history.front().unwrap()).message, "3");
    assert_eq!(expect_message(history.back().unwrap()).message, (GAME_CHAT_HISTORY_LIMIT + 2).to_string());
}

#[test]
fn validation_rejects_empty_oversized_and_control_text_without_broadcasting() {
    let mut room = ServerGameChat::new(None);
    let (_, mut live_rx) = room.subscribe();
    for text in [
        String::new(),
        " \n\t ".to_string(),
        "a\nb".to_string(),
        "a\0b".to_string(),
        "🌍".repeat(GAME_CHAT_MAX_MESSAGE_CHARS + 1),
    ] {
        assert!(room.submit(1, "Alice", text).is_err());
    }
    let (history, _) = room.subscribe();
    assert!(history.is_empty());
    assert!(live_rx.try_recv().is_err());
    room.submit(1, "Alice", "🌍".repeat(GAME_CHAT_MAX_MESSAGE_CHARS)).unwrap();
    assert_eq!(
        expect_message(&live_rx.try_recv().unwrap()).message.chars().count(),
        GAME_CHAT_MAX_MESSAGE_CHARS
    );
}

#[test]
fn profanity_is_censored_before_it_reaches_history_or_any_subscriber() {
    let mut room = ServerGameChat::new(None);
    let (_, mut live_rx) = room.subscribe();
    room.submit(1, "Alice", "fuck this lag".to_string()).unwrap();
    assert_eq!(expect_message(&live_rx.try_recv().unwrap()).message, "#### this lag");
    let (history, _) = room.subscribe();
    assert_eq!(expect_message(history.front().unwrap()).message, "#### this lag");
}

// Scripts hear about a message through the same queue physics and player join use, and they read
// the censored text rather than what was typed.
#[test]
fn scripts_are_told_about_an_accepted_message_and_hear_the_censored_text() {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut room = ServerGameChat::new(Some(js_callback_inputs_tx));
    let (_, mut live_rx) = room.subscribe();
    room.submit(17, "Alice", "fuck this lag".to_string()).unwrap();

    let broadcast_entry = live_rx.try_recv().unwrap();
    let broadcast_message = expect_message(&broadcast_entry);
    let JsCallbackInputs::GameChatMessageSent {
        userid,
        username,
        message,
        time_live_server_ms,
    } = js_callback_inputs_rx.try_recv().unwrap()
    else {
        panic!("an accepted message must reach scripts");
    };
    assert_eq!(message, "#### this lag");
    // The same message the players got, not a second one built separately.
    assert_eq!(userid, broadcast_message.userid);
    assert_eq!(username, broadcast_message.username);
    assert_eq!(time_live_server_ms, broadcast_message.time_live_server_ms);

    // A message the server refuses never reaches a script.
    assert!(room.submit(17, "Alice", String::new()).is_err());
    assert!(js_callback_inputs_rx.try_recv().is_err());
}

// Join and leave notices sit in the same history as player messages, so a late joiner sees who came
// and went. Scripts already hear about players through their own objects, so the notices skip them.
#[test]
fn join_and_leave_notices_reach_history_and_subscribers_in_order_but_not_scripts() {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut room = ServerGameChat::new(Some(js_callback_inputs_tx));
    let (_, mut live_rx) = room.subscribe();
    room.add_entry(GameChatEntry::PlayerJoined { username: "Alice".to_string() });
    room.submit(17, "Alice", "hi".to_string()).unwrap();
    room.add_entry(GameChatEntry::PlayerLeft { username: "Alice".to_string() });

    let (history, _) = room.subscribe();
    let live = [live_rx.try_recv().unwrap(), live_rx.try_recv().unwrap(), live_rx.try_recv().unwrap()];
    assert_eq!(Vec::from(history), live);
    assert_eq!(live[0], GameChatEntry::PlayerJoined { username: "Alice".to_string() });
    assert_eq!(expect_message(&live[1]).message, "hi");
    assert_eq!(live[2], GameChatEntry::PlayerLeft { username: "Alice".to_string() });

    let Ok(JsCallbackInputs::GameChatMessageSent { .. }) = js_callback_inputs_rx.try_recv() else {
        panic!("the player message must reach scripts");
    };
    assert!(js_callback_inputs_rx.try_recv().is_err());
}

#[test]
fn censoring_reads_through_spacing_and_punctuation_and_leaves_ordinary_messages_alone() {
    let mut room = ServerGameChat::new(None);
    let (_, mut live_rx) = room.subscribe();
    // Spelling a word out with spaces or dots is the usual way past a plain word list, so every one
    // of these has to come back censored, not just the word written normally.
    for text in ["shit", "sh1t", "s h i t", "s.h.i.t", "SHIT"] {
        room.submit(1, "Alice", text.to_string()).unwrap();
        let censored_entry = live_rx.try_recv().unwrap();
        let censored = &expect_message(&censored_entry).message;
        assert!(censored.chars().all(|character| character == '#'), "{text} came through as {censored}");
        // One hash per character keeps the length limit checked on the way in still true.
        assert_eq!(censored.chars().count(), text.chars().count());
    }
    // Ordinary messages have to arrive untouched, including words that contain a shorter word.
    for text in ["nice shot everyone good game", "assassin classic analysis", "hello 🌍"] {
        room.submit(1, "Alice", text.to_string()).unwrap();
        assert_eq!(expect_message(&live_rx.try_recv().unwrap()).message, text);
    }
}

#[test]
fn a_slow_subscriber_does_not_block_the_room_or_a_current_subscriber() {
    let mut room = ServerGameChat::new(None);
    let (_, mut slow_rx) = room.subscribe();
    let (_, mut current_rx) = room.subscribe();
    for index in 0..GAME_CHAT_HISTORY_LIMIT * 2 {
        room.submit(1, "Alice", index.to_string()).unwrap();
        assert_eq!(expect_message(&current_rx.try_recv().unwrap()).message, index.to_string());
    }
    let Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) = slow_rx.try_recv() else {
        panic!("a subscriber that exceeds the queue must report lag instead of silently skipping messages");
    };
}

#[test]
fn maximum_unicode_submission_fits_the_wire_limit_and_history_round_trips() {
    let config = get_bincode_config();
    let request = ClientMessageTCP::GameChat(ClientGameChatMessage {
        message: "🌍".repeat(GAME_CHAT_MAX_MESSAGE_CHARS),
    });
    let bytes = config.serialize(&request).unwrap();
    assert!(bytes.len() <= MAX_TCP_MESSAGE_BYTES as usize);
    let ClientMessageTCP::GameChat(decoded) = config.deserialize(&bytes).unwrap() else {
        panic!("expected a chat message");
    };
    assert_eq!(decoded.message, "🌍".repeat(GAME_CHAT_MAX_MESSAGE_CHARS));
    let mut room = ServerGameChat::new(None);
    room.submit(1, "Alice", decoded.message).unwrap();
    let (history, _) = room.subscribe();
    let bytes = config.serialize(&ServerGameChatMessage::History(history.clone())).unwrap();
    let ServerGameChatMessage::History(decoded) = config.deserialize(&bytes).unwrap() else {
        panic!("expected history")
    };
    assert_eq!(history, decoded);
}

// Use real local QUIC streams and the production chat loop; no database, game server, or mocks are needed.
async fn connect_game_chat(
    server_endpoint: &Endpoint,
    client_endpoint: &Endpoint,
    game_chat_arc: Arc<Mutex<ServerGameChat>>,
    userid: Option<UserId>,
    username: &'static str,
) -> (Connection, RecvStream) {
    let connecting = client_endpoint.connect(server_endpoint.local_addr().unwrap(), QUIC_CERT_SERVER_NAME).unwrap();
    let (client, server) = tokio::try_join!(connecting, async { server_endpoint.accept().await.unwrap().await }).unwrap();
    let (request_tx, _request_rx) = mpsc::unbounded_channel();
    start_receiving_client_messages_tcp_in_bg(server, game_chat_arc, 1, userid, username.to_string(), request_tx);
    let (mut send_stream, recv_stream) = client.open_bi().await.unwrap();
    sendTcpMessage(&mut send_stream, &ClientMessageTCP::SubscribeToGameChat).await.unwrap();
    send_stream.finish().unwrap();
    (client, recv_stream)
}

fn make_server_and_client_endpoints() -> (Endpoint, Endpoint) {
    let rcgen::CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(vec![QUIC_CERT_SERVER_NAME.to_string()]).unwrap();
    let server_config = ServerConfig::with_single_cert(vec![cert.der().clone()], PrivateKeyDer::try_from(key_pair.serialize_der()).unwrap()).unwrap();
    let server_endpoint = Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let mut client_endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client_endpoint.set_default_client_config(ClientConfig::with_root_certificates(Arc::new(roots)).unwrap());
    (server_endpoint, client_endpoint)
}

#[tokio::test]
async fn reliable_stream_delivers_history_echo_and_live_messages_in_order() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (server_endpoint, client_endpoint) = make_server_and_client_endpoints();
        let game_chat_arc = Arc::new(Mutex::new(ServerGameChat::new(None)));
        let (first, mut first_recv) = connect_game_chat(&server_endpoint, &client_endpoint, game_chat_arc.clone(), Some(1), "Alice").await;
        let ServerGameChatMessage::History(history) = awaitTcpMessageMaxBytes(&mut first_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected history")
        };
        assert!(history.is_empty());
        let response: ServerGameChatResultMessage = sendTcpRequest(
            &first,
            &ClientMessageTCP::GameChat(ClientGameChatMessage {
                message: "before join".to_string(),
            }),
        )
        .await
        .unwrap();
        response.result.unwrap();
        let ServerGameChatMessage::Entry(echo) = awaitTcpMessageMaxBytes(&mut first_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected echo")
        };
        assert_eq!(expect_message(&echo).userid, 1);
        assert_eq!(expect_message(&echo).username, "Alice");
        let (second, mut second_recv) = connect_game_chat(&server_endpoint, &client_endpoint, game_chat_arc.clone(), Some(2), "Bob").await;
        let ServerGameChatMessage::History(history) = awaitTcpMessageMaxBytes(&mut second_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected history")
        };
        assert_eq!(history.front(), Some(&echo));
        for text in ["next", "last"] {
            let response: ServerGameChatResultMessage =
                sendTcpRequest(&second, &ClientMessageTCP::GameChat(ClientGameChatMessage { message: text.to_string() }))
                    .await
                    .unwrap();
            response.result.unwrap();
        }
        for text in ["next", "last"] {
            let ServerGameChatMessage::Entry(one) = awaitTcpMessageMaxBytes(&mut first_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
                panic!("expected live message")
            };
            let ServerGameChatMessage::Entry(two) = awaitTcpMessageMaxBytes(&mut second_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
                panic!("expected echo")
            };
            assert_eq!(one, two);
            let one = expect_message(&one);
            assert_eq!(one.message, text);
            assert_eq!(one.userid, 2);
            assert_eq!(one.username, "Bob");
        }
        first.close(0_u32.into(), b"test complete");
        second.close(0_u32.into(), b"test complete");

        // Late joiners can identify past senders even after both original clients disconnect.
        let (late, mut late_recv) = connect_game_chat(&server_endpoint, &client_endpoint, game_chat_arc, Some(3), "Carol").await;
        let ServerGameChatMessage::History(history) = awaitTcpMessageMaxBytes(&mut late_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected history")
        };
        assert_eq!(
            history.iter().map(|entry| expect_message(entry).username.as_str()).collect::<Vec<_>>(),
            ["Alice", "Bob", "Bob"]
        );
        late.close(0_u32.into(), b"test complete");
    })
    .await
    .expect("chat stream test timed out");
}

// A guest has no account, so they can read the chat but nothing they send reaches it.
#[tokio::test]
async fn a_guest_can_read_the_chat_but_not_send_to_it() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (server_endpoint, client_endpoint) = make_server_and_client_endpoints();
        let game_chat_arc = Arc::new(Mutex::new(ServerGameChat::new(None)));
        let (guest, mut guest_recv) = connect_game_chat(&server_endpoint, &client_endpoint, game_chat_arc.clone(), None, "Guest").await;
        let ServerGameChatMessage::History(_) = awaitTcpMessageMaxBytes(&mut guest_recv, MAX_TCP_MESSAGE_BYTES).await.unwrap() else {
            panic!("expected history")
        };
        let response: ServerGameChatResultMessage = sendTcpRequest(&guest, &ClientMessageTCP::GameChat(ClientGameChatMessage { message: "hi".to_string() }))
            .await
            .unwrap();
        assert!(response.result.is_err(), "a guest's chat message should be refused");
        let (history, _) = game_chat_arc.lock().await.subscribe();
        assert!(history.is_empty(), "a guest's chat message should reach no one");
        guest.close(0_u32.into(), b"test complete");
    })
    .await
    .expect("guest chat test timed out");
}
