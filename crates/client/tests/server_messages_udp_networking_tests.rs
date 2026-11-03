use std::{collections::HashMap, sync::Arc, time::Duration};

use client::play_runner::{start_receiving_server_messages_udp_in_bg, time_estimator::createTimeEstimator};
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use rustls::pki_types::PrivateKeyDer;
use shared::protocol::{
    messages::{ClientAckMessage, ClientMessageUDP, MessageNum, ServerMessageUDP, ServerPlayerPingsMessage, ServerPongMessage, ServerSnapshotMessage},
    quic_cert::QUIC_CERT_SERVER_NAME,
    quic_send_receive_helpers::{awaitDatagramMessage, sendDatagramMessage},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// Run the production datagram task against a real local QUIC peer, with no play loop taking the snapshots in.
struct ServerMessagesUdpTest {
    _server_endpoint: Endpoint,
    _client_endpoint: Endpoint,
    server_connection: Connection,
    snapshot_rx: mpsc::UnboundedReceiver<ServerSnapshotMessage>,
    player_pings_rx: mpsc::UnboundedReceiver<ServerPlayerPingsMessage>,
    kill_play_cancel_token: CancellationToken,
}

impl ServerMessagesUdpTest {
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
        let (time_estimator_writer, _time_estimator_reader) = createTimeEstimator(0.0);
        let kill_play_cancel_token = CancellationToken::new();
        let (snapshot_rx, player_pings_rx) =
            start_receiving_server_messages_udp_in_bg(client_connection, time_estimator_writer, kill_play_cancel_token.clone());
        Self {
            _server_endpoint: server_endpoint,
            _client_endpoint: client_endpoint,
            server_connection,
            snapshot_rx,
            player_pings_rx,
            kill_play_cancel_token,
        }
    }

    fn send_snapshot(&self, message_num: MessageNum) {
        let snapshot = ServerSnapshotMessage {
            message_num,
            live_server_sim_ms: 0.0,
            deltaOfObjectId: HashMap::new(),
            deltaOfObjectId2: HashMap::new(),
        };
        sendDatagramMessage(&self.server_connection, &ServerMessageUDP::Snapshot(snapshot)).unwrap();
    }

    async fn receive_ack(&self) -> MessageNum {
        let ClientMessageUDP::Ack(ClientAckMessage { message_num }) = awaitDatagramMessage::<ClientMessageUDP>(&self.server_connection).await.unwrap() else {
            panic!("expected a snapshot ACK");
        };
        message_num
    }
}

// The server sends a change again when its ACK comes late, so a slow play loop must not hold the ACK up.
#[tokio::test]
async fn acks_a_snapshot_before_the_play_loop_takes_it_in() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ServerMessagesUdpTest::new().await;
        test.send_snapshot(7);
        assert_eq!(test.receive_ack().await, 7);

        // nothing has taken the snapshot in, and it still waits for the play loop
        let snapshot = test.snapshot_rx.try_recv().expect("the snapshot should wait for the play loop");
        assert_eq!(snapshot.message_num, 7);
        test.kill_play_cancel_token.cancel();
    })
    .await
    .expect("snapshot ACK test timed out");
}

// Each snapshot gets its own ACK. Pongs and player pings get none, and the pings still reach the play loop.
#[tokio::test]
async fn acks_each_snapshot_and_nothing_else() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ServerMessagesUdpTest::new().await;
        let player_pings = ServerPlayerPingsMessage {
            pingMsOfPlayerObjectId: HashMap::from([(1, 20)]),
        };
        sendDatagramMessage(&test.server_connection, &ServerMessageUDP::PlayerPings(player_pings)).unwrap();
        let pong = ServerPongMessage {
            t0_client_send_ms: 0.0,
            t1_server_recv_ms: 0.0,
        };
        sendDatagramMessage(&test.server_connection, &ServerMessageUDP::Pong(pong)).unwrap();
        for message_num in [8, 9, 10] {
            test.send_snapshot(message_num);
        }

        let mut acked_message_nums = Vec::new();
        for _ in 0..3 {
            acked_message_nums.push(test.receive_ack().await);
        }
        acked_message_nums.sort(); // datagrams can arrive in any order
        assert_eq!(acked_message_nums, vec![8, 9, 10]);
        let player_pings = test.player_pings_rx.recv().await.expect("the player pings should reach the play loop");
        assert_eq!(player_pings.pingMsOfPlayerObjectId, HashMap::from([(1, 20)]));
        test.kill_play_cancel_token.cancel();
    })
    .await
    .expect("ACK for each snapshot test timed out");
}

#[tokio::test]
async fn cancelling_stops_receiving() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut test = ServerMessagesUdpTest::new().await;
        test.kill_play_cancel_token.cancel();
        assert!(test.snapshot_rx.recv().await.is_none());
        assert!(test.player_pings_rx.recv().await.is_none());
    })
    .await
    .expect("datagram cancellation test timed out");
}
