use std::{
    collections::{HashMap, HashSet},
    net::UdpSocket,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use game_types::{
    consts::{CLIENT_SERVER_PROTOCOL_VERSION, STORED_WORLD_CURRENT_VERSION},
    data_types::DataIds,
    object_types::{ObjectId, ObjectInfo, ObjectKind, Orientation3D, PhysicsInfo3D, PlayerInfo, shared::Color3},
    stored_world_type::StoredWorld,
};
use glam::{Quat, Vec3};
use quinn::{ClientConfig, Connection, Endpoint};
use rustls::pki_types::CertificateDer;
use server::{ServerMode, playtest_auth::PlaytestAuth, start_running_server_in_bg};
use shared::game::world::World;
use shared::player_profile::PlayerProfile;
use shared::protocol::{
    messages::{
        ClientAckMessage, ClientFunctionCallMessage, ClientFunctionResultMessage, ClientHandshakeTCP, ClientHelloMessage, ClientInputMessage, ClientMessageTCP,
        ClientMessageUDP, ForeignCharacterDelta, ForeignPlayerDelta, InputNum, MessageNum, NetworkedCoords, ObjectDelta, ServerFnCallMessage,
        ServerFunctionResultMessage, ServerHandshakeTCP, ServerInitialWorldMessage, ServerMessageUDP, ServerScriptRawCodeMessage,
    },
    quic_cert::{QUIC_CERT_SERVER_NAME, QUIC_SERVER_CERT_DER_BYTES},
    quic_send_receive_helpers::{MAX_TCP_MESSAGE_BYTES, awaitDatagramMessage, awaitTcpMessageMaxBytes, sendDatagramMessage, sendTcpMessage, sendTcpRequest},
};
use tokio::sync::RwLock;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

// Players join a real playtest server over local QUIC, the way the desktop client does.
// The server uses the real cert and key, so run these with: infisical run --env=dev -- cargo test -p server --test join_tests

// A world holding just these scripts, as (name, Client or Server, code written as a JSON string's contents).
fn stored_world(scripts: &[(&str, &str, &str)]) -> String {
    let script_id = |index: usize| index + 2;
    let script_objects: String = scripts
        .iter()
        .enumerate()
        .map(|(index, (name, location, _))| {
            format!(
                r#","{id}":{{"id":{id},"name":"{name}","info":{{"Script":{{"objectLocation":"{location}"}}}}}}"#,
                id = script_id(index)
            )
        })
        .collect();
    let script_children: String = (0..scripts.len()).map(|index| format!(r#","{}":[]"#, script_id(index))).collect();
    let root_children = (0..scripts.len()).map(|index| script_id(index).to_string()).collect::<Vec<_>>().join(",");
    let script_codes = scripts
        .iter()
        .enumerate()
        .map(|(index, (_, _, code))| format!(r#""{}":"{code}""#, script_id(index)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"worldSchemaVersion":{STORED_WORLD_CURRENT_VERSION},"fallback_camera_location":{{"position":[0.0,0.0,0.0],"orientation":{{"theta":0.0,"phi":0.0}}}},"rootObjectId":1,"objectOfId":{{"1":{{"id":1,"name":"root","info":{{"LocalGroup":{{}}}}}}{script_objects}}},"childrenOfId":{{"1":[{root_children}]{script_children}}},"scriptRawCodeOfId":{{{script_codes}}},"physicsInfo3DOfId":{{}},"physicsToggles3DOfId":{{}},"primitiveSettingsOfId":{{}}}}"#
    )
}

async fn start_playtest_server(stored_world: &str) -> (u16, Arc<PlaytestAuth>, CancellationToken, Arc<RwLock<World>>) {
    let server_port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let auth_arc = Arc::new(PlaytestAuth::default());
    let cancel_token = CancellationToken::new();
    let world_arc = start_running_server_in_bg(
        stored_world,
        cancel_token.clone(),
        TaskTracker::new(),
        Instant::now(),
        server_port,
        ServerMode::Playtest(auth_arc.clone()),
        None,
    )
    .unwrap();
    auth_arc.wait_until_running().await.unwrap();
    (server_port, auth_arc, cancel_token, world_arc)
}

fn client_endpoint() -> Endpoint {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(QUIC_SERVER_CERT_DER_BYTES)).unwrap();
    let mut client_endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client_endpoint.set_default_client_config(ClientConfig::with_root_certificates(Arc::new(roots)).unwrap());
    client_endpoint
}

// Returns the connection, which must stay open for the player to stay in the game, and the player object the server made.
async fn join(client_endpoint: &Endpoint, server_port: u16, join_token: String) -> (Connection, ObjectId) {
    let (connection, initial_world) = join_with_initial_world(client_endpoint, server_port, join_token).await;
    (connection, initial_world.player_object_id)
}

async fn join_with_initial_world(client_endpoint: &Endpoint, server_port: u16, join_token: String) -> (Connection, ServerInitialWorldMessage) {
    let connection = client_endpoint
        .connect(([127, 0, 0, 1], server_port).into(), QUIC_CERT_SERVER_NAME)
        .unwrap()
        .await
        .unwrap();
    let (mut send_stream, mut recv_stream) = connection.open_bi().await.unwrap();
    let hello = ClientHelloMessage {
        client_server_protocol_version: CLIENT_SERVER_PROTOCOL_VERSION,
        client_app_version: "test".to_string(),
        play_auth: join_token,
    };
    sendTcpMessage(&mut send_stream, &ClientHandshakeTCP::Hello(hello)).await.unwrap();
    let ServerHandshakeTCP::InitialWorld(initial_world) = awaitTcpMessageMaxBytes::<ServerHandshakeTCP>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES)
        .await
        .unwrap();
    sendTcpMessage(&mut send_stream, &ClientHandshakeTCP::ReadyForUDPMessages).await.unwrap();
    (connection, initial_world)
}

// A playtest player is their account's player unless that account already has one in the game. Guests have no account.
#[tokio::test]
async fn a_second_playtest_player_from_one_account_joins_as_a_guest() {
    let (server_port, auth_arc, cancel_token, world_arc) = start_playtest_server(&stored_world(&[])).await;

    let account_profile = || PlayerProfile {
        preferredColor: Some(Color3 { r: 1.0, g: 0.5, b: 0.0 }),
        equippedItemId: Some(20),
    };
    let account_token = auth_arc.reserve("Kubester".to_string(), Some(17), account_profile()).await;
    let same_account_token = auth_arc.reserve("Kubester".to_string(), Some(17), account_profile()).await;
    let guest_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let client_endpoint = client_endpoint();
    let (_account_connection, account_player) = join(&client_endpoint, server_port, account_token).await;
    let (_same_account_connection, same_account_player) = join(&client_endpoint, server_port, same_account_token).await;
    let (_guest_connection, guest_player) = join(&client_endpoint, server_port, guest_token).await;

    let world_lock = world_arc.read().await;
    let account_of_player = |player_object_id: ObjectId| match &world_lock.getObject(player_object_id).unwrap().info {
        ObjectInfo::Player(PlayerInfo {
            username,
            userid,
            preferredColor,
            equippedItemId,
            ..
        }) => (username.clone(), *userid, preferredColor.map(|Color3 { r, g, b }| [r, g, b]), *equippedItemId),
        _ => panic!("{player_object_id} should be a player"),
    };
    assert_eq!(
        account_of_player(account_player),
        ("Kubester".to_string(), Some(17), Some([1.0, 0.5, 0.0]), Some(20))
    );
    assert_eq!(account_of_player(same_account_player), ("Guest".to_string(), None, None, None));
    assert_eq!(account_of_player(guest_player), ("Guest".to_string(), None, None, None));
    drop(world_lock);
    cancel_token.cancel();
}

// Builds a sound and cuts it into a template when the game starts, so the sound was never in any world a player is sent.
const SOUND_TEMPLATE_SCRIPT: &str = r#"const sound = game.objectService.createObject('Sound', { type: 'soundPlayer', soundDataId: 77, clockId: null, volume: 1 }, game.world.id);\ngame.templateService.cutObjectToTemplate(sound.id, 'sound');\n"#;

// Templates are never sent to players, so a joining player is told what data they use and can load it before one is pasted.
#[tokio::test]
async fn a_joining_player_is_told_what_data_the_server_templates_use() {
    let world = stored_world(&[("soundTemplate", "Server", SOUND_TEMPLATE_SCRIPT)]);
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&world).await;
    let join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let (_connection, initial_world) = join_with_initial_world(&client_endpoint(), server_port, join_token).await;

    let expected_data_ids = DataIds {
        sound_data_ids: HashSet::from([77]),
        ..DataIds::default()
    };
    assert_eq!(initial_world.template_data_ids, expected_data_ids);
    cancel_token.cancel();
}

const CLIENT_FUNCTIONS_SCRIPT: &str = r#"'use client'\nexport const greet = async (playerObjectId: number, greeting: string) => greeting;\n"#;
// The player's client is only reachable once they finish joining, so the server waits a moment before calling it.
const GREETER_SCRIPT: &str = r#"import { greet } from './clientFunctions';\ngame.objectService.addObjectCreatedListener((player) => {\n  if (player.type === 'player') setTimeout(() => greet(player.id, 'hello ' + player.username).catch(() => {}), 500);\n}, 'player');\n"#;

// A server script calls one player's client function with that player's object id, and the call reaches that player's connection.
#[tokio::test]
async fn a_server_script_reaches_each_players_client_by_their_player_object_id() {
    let world = stored_world(&[("clientFunctions", "Client", CLIENT_FUNCTIONS_SCRIPT), ("greeter", "Server", GREETER_SCRIPT)]);
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&world).await;
    let client_endpoint = client_endpoint();
    let mut connections = Vec::new();
    for (username, user_id) in [("Kubester", Some(17)), ("Guest", None)] {
        let join_token = auth_arc.reserve(username.to_string(), user_id, PlayerProfile::GUEST).await;
        let (connection, player_object_id) = join(&client_endpoint, server_port, join_token).await;
        let (mut send_stream, mut recv_stream) = tokio::time::timeout(Duration::from_secs(10), connection.accept_bi())
            .await
            .expect("the server should call this player's client function")
            .unwrap();
        let call = awaitTcpMessageMaxBytes::<ClientFunctionCallMessage>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES)
            .await
            .unwrap();
        assert!(
            call.js_args.contains(&format!(r#"[{player_object_id},"hello {username}"]"#)),
            "{username}'s client should get their own call: {}",
            call.js_args
        );
        sendTcpMessage(
            &mut send_stream,
            &ClientFunctionResultMessage {
                result: Ok(r#"{"json":"ok"}"#.to_string()),
            },
        )
        .await
        .unwrap();
        connections.push(connection);
    }
    cancel_token.cancel();
}

fn server_fn_call(fn_id: &str) -> ClientMessageTCP {
    ClientMessageTCP::ServerFnCall(ServerFnCallMessage {
        fn_id: fn_id.to_string(),
        js_args: r#"{"json":[]}"#.to_string(),
    })
}

const WAITING_SERVER_FUNCTIONS_SCRIPT: &str = r#"'use server'\nexport const waitForever = async () => { await new Promise(() => {}); };\nexport const waitForTick = async () => { const caller = game.playerService.getServerFnCaller(); await tick(); return caller; };\n"#;

// The server runs a call up to its first await and moves on, so a call that never finishes doesn't stop the game.
#[tokio::test]
async fn a_server_function_that_never_finishes_does_not_stop_the_game() {
    let world = stored_world(&[("waits", "Server", WAITING_SERVER_FUNCTIONS_SCRIPT)]);
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&world).await;
    let join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let (connection, player_object_id) = join(&client_endpoint(), server_port, join_token).await;

    let (mut wait_forever_send, _wait_forever_recv) = connection.open_bi().await.unwrap();
    sendTcpMessage(&mut wait_forever_send, &server_fn_call("waitForever#root/waits")).await.unwrap();
    wait_forever_send.finish().unwrap();
    let response: ServerFunctionResultMessage =
        tokio::time::timeout(Duration::from_secs(5), sendTcpRequest(&connection, &server_fn_call("waitForTick#root/waits")))
            .await
            .expect("a call that awaits the next tick should finish")
            .unwrap();
    assert_eq!(response.result.unwrap(), format!(r#"{{"json":{player_object_id}}}"#));
    cancel_token.cancel();
}

// Takes 10 ms, more than a tick gives to starting calls, and returns the time of the tick it started on.
const SLOW_SERVER_FUNCTION_SCRIPT: &str =
    r#"'use server'\nexport const slow = () => {\n  const end = Date.now() + 10;\n  while (Date.now() < end) {}\n  return getTime();\n};\n"#;

// Once a tick has spent its time starting calls, the rest wait for the next tick, so many calls at once can't hold up the game.
#[tokio::test]
async fn calls_that_fill_a_tick_start_on_the_next_tick() {
    let world = stored_world(&[("slow", "Server", SLOW_SERVER_FUNCTION_SCRIPT)]);
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&world).await;
    let client_endpoint = client_endpoint();
    let mut connections = Vec::new();
    for _ in 0..2 {
        let join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
        connections.push(join(&client_endpoint, server_port, join_token).await.0);
    }

    let slow_call = server_fn_call("slow#root/slow");
    let (first_response, second_response) = tokio::join!(
        sendTcpRequest::<_, ServerFunctionResultMessage>(&connections[0], &slow_call),
        sendTcpRequest::<_, ServerFunctionResultMessage>(&connections[1], &slow_call),
    );
    let started_at = |response: anyhow::Result<ServerFunctionResultMessage>| response.unwrap().result.unwrap();
    assert_ne!(
        started_at(first_response),
        started_at(second_response),
        "the second call should start on a later tick"
    );
    cancel_token.cancel();
}

// Gives each player a character at x = 5.
const SPAWNER_SCRIPT: &str = r#"game.objectService.addObjectCreatedListener((player) => {\n  if (player.type !== 'player') return;\n  const v = (x, y, z) => ({ x, y, z });\n  const character = game.objectService.createObject('Character', { type: 'brick', material: null, opacity: 1, color: { r: 1, g: 1, b: 1 }, isAlwaysOnTop: false, size: v(1, 1, 1), isWalkthrough: false, isPushable: true, density: 1, location: { position: v(5, 0, 0), rotation: v(0, 0, 0) }, movement: { velocity: v(0, 0, 0), angularVelocity: v(0, 0, 0) } }, game.world.id);\n  player.characterObjectId = character.id;\n}, 'player');\n"#;
// Moves every character to x = 5 on every tick.
const MOVER_SCRIPT: &str = r#"while (true) {\n  for (const player of game.playerService.getAllPlayers()) {\n    if (player.characterObjectId !== null) game.objectService.moveObjectTo(player.characterObjectId, { x: 5, y: 0, z: 0 });\n  }\n  await tick();\n}\n"#;

// A pose the player sends lands in a tick where the server also moves their character, so the tick ends where the
// server put it: the player's pose is never remembered, or sent to watchers, after the server's move.
#[tokio::test]
async fn a_players_pose_is_dropped_in_a_tick_where_the_server_moves_their_character() {
    let (server_port, auth_arc, cancel_token, world_arc) =
        start_playtest_server(&stored_world(&[("spawner", "Server", SPAWNER_SCRIPT), ("mover", "Server", MOVER_SCRIPT)])).await;
    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (connection, player_object_id) = join(&client_endpoint(), server_port, join_token).await;

    // acking each snapshot right away means the server takes the player's next pose instead of waiting for an ack
    let far_away_pose = ForeignCharacterDelta {
        physicsInfo3D: PhysicsInfo3D::new(Vec3::new(-1000.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        est_live_server_sim_ms: f64::INFINITY, // the server stamps it with when it arrives
        teleportCount: 0,
    };
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut input_num = 0;
    while Instant::now() < deadline {
        let Ok(Ok(ServerMessageUDP::Snapshot(snapshot))) =
            tokio::time::timeout(Duration::from_millis(100), awaitDatagramMessage::<ServerMessageUDP>(&connection)).await
        else {
            continue;
        };
        input_num += 1;
        sendDatagramMessage(
            &connection,
            &ClientMessageUDP::Ack(ClientAckMessage {
                message_num: snapshot.message_num,
            }),
        )
        .unwrap();
        let input = ClientInputMessage {
            input_num,
            latest_applied_message_num: snapshot.message_num,
            foreign_player_delta: ForeignPlayerDelta {
                cameraOrientation: Orientation3D::Z(),
                playerMetadata: None,
                est_live_server_sim_ms: f64::INFINITY,
            },
            foreign_character_delta: Some(far_away_pose.clone()),
        };
        sendDatagramMessage(&connection, &ClientMessageUDP::Input(input)).unwrap();
    }

    let world_lock = world_arc.read().await;
    let character_object_id = world_lock
        .getPlayerCharacterObject(player_object_id)
        .expect("the script should give the player a character");
    let history = &world_lock.characterPoseHistoryOfObjectId[&character_object_id];
    let mut remembered_xs = Vec::new();
    let mut entry_est_live_server_sim_ms = f64::NEG_INFINITY;
    while let Some((next_entry_est_live_server_sim_ms, pose)) = history.find_first_entry_after(entry_est_live_server_sim_ms) {
        let NetworkedCoords::WorldCoords(physics_info_3d) = &pose.coords else {
            panic!("a character's history is in world coordinates");
        };
        remembered_xs.push(physics_info_3d.location.position.x);
        entry_est_live_server_sim_ms = *next_entry_est_live_server_sim_ms;
    }
    assert!(!remembered_xs.is_empty(), "the server's moves should be remembered");
    assert!(
        remembered_xs.iter().all(|&x| x == 5.0),
        "only the server's moves should be remembered: {remembered_xs:?}"
    );
    drop(world_lock);
    cancel_token.cancel();
}

// The server makes the player's character, and the player hasn't sent a pose for it yet. Watchers already see it
// standing where it was made, so the history starts there instead of having nothing.
#[tokio::test]
async fn a_character_the_server_makes_is_remembered_before_its_player_sends_a_pose() {
    let (server_port, auth_arc, cancel_token, world_arc) = start_playtest_server(&stored_world(&[("spawner", "Server", SPAWNER_SCRIPT)])).await;
    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (_connection, player_object_id) = join(&client_endpoint(), server_port, join_token).await;

    let deadline = Instant::now() + Duration::from_secs(5);
    let character_object_id = loop {
        if let Some(character_object_id) = world_arc.read().await.getPlayerCharacterObject(player_object_id) {
            break character_object_id;
        }
        assert!(Instant::now() < deadline, "the script should give the player a character");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    let world_lock = world_arc.read().await;
    let (_, pose) = world_lock
        .characterPoseHistoryOfObjectId
        .get(&character_object_id)
        .and_then(|history| history.find_last_entry_at_or_before(f64::INFINITY))
        .expect("the history should start where the server made the character");
    let NetworkedCoords::WorldCoords(physics_info_3d) = &pose.coords else {
        panic!("a character's history is in world coordinates");
    };
    assert_eq!(physics_info_3d.location.position.x, 5.0);
    drop(world_lock);
    cancel_token.cancel();
}

// A player ACKs each snapshot the moment it arrives, before their play loop applies it. So a pose they made before applying
// the snapshot that gave them their character is still ignored, even with nothing left to ACK, and their pose counts once
// their input says they applied it.
#[tokio::test]
async fn a_players_pose_counts_once_their_input_says_they_applied_their_character() {
    let (server_port, auth_arc, cancel_token, world_arc) = start_playtest_server(&stored_world(&[("spawner", "Server", SPAWNER_SCRIPT)])).await;
    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (connection, player_object_id) = join(&client_endpoint(), server_port, join_token).await;

    // ACK every snapshot as it arrives, like the desktop client's datagram task, and remember the newest one
    let latest_received_message_num_arc = Arc::new(AtomicU64::new(0));
    tokio::spawn({
        let connection = connection.clone();
        let latest_received_message_num_arc = latest_received_message_num_arc.clone();
        async move {
            while let Ok(server_message) = awaitDatagramMessage::<ServerMessageUDP>(&connection).await {
                let ServerMessageUDP::Snapshot(snapshot) = server_message else {
                    continue;
                };
                let ack = ClientMessageUDP::Ack(ClientAckMessage {
                    message_num: snapshot.message_num,
                });
                let _ = sendDatagramMessage(&connection, &ack);
                latest_received_message_num_arc.fetch_max(snapshot.message_num, Ordering::SeqCst);
            }
        }
    });

    // wait for the character, then give the snapshots carrying it time to arrive and be ACKed
    let deadline = Instant::now() + Duration::from_secs(5);
    let character_object_id = loop {
        if let Some(character_object_id) = world_arc.read().await.getPlayerCharacterObject(player_object_id) {
            break character_object_id;
        }
        assert!(Instant::now() < deadline, "the script should give the player a character");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    tokio::time::sleep(Duration::from_millis(300)).await;

    let send_far_away_pose = |input_num: InputNum, latest_applied_message_num: MessageNum| {
        let input = ClientInputMessage {
            input_num,
            latest_applied_message_num,
            foreign_player_delta: ForeignPlayerDelta {
                cameraOrientation: Orientation3D::Z(),
                playerMetadata: None,
                est_live_server_sim_ms: f64::INFINITY,
            },
            foreign_character_delta: Some(ForeignCharacterDelta {
                physicsInfo3D: PhysicsInfo3D::new(Vec3::new(-1000.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                est_live_server_sim_ms: f64::INFINITY, // the server stamps it with when it arrives
                teleportCount: 0,
            }),
        };
        sendDatagramMessage(&connection, &ClientMessageUDP::Input(input)).unwrap();
    };

    // made before the play loop applied any snapshot
    send_far_away_pose(1, 0);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let character_x = world_arc.read().await.getObjectPhysicsInfo3D(character_object_id).unwrap().location.position.x;
    assert_eq!(character_x, 5.0, "a pose made before the player applied their character must be ignored");

    // made after the play loop applied every snapshot that arrived
    send_far_away_pose(2, latest_received_message_num_arc.load(Ordering::SeqCst));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let character_x = world_arc.read().await.getObjectPhysicsInfo3D(character_object_id).unwrap().location.position.x;
    assert_eq!(character_x, -1000.0, "a pose made after the player applied their character should count");
    cancel_token.cancel();
}

// A pose with a NaN reaches the other players and the pose history scripts read, and a NaN rotation can land in the world
// every new player is sent, as JSON null, so no new player could read it. So the server drops a pose with a NaN, as it would
// a saved world holding one, and still takes the player's next good pose.
#[tokio::test]
async fn a_pose_with_a_nan_is_dropped_and_new_players_can_still_join() {
    let (server_port, auth_arc, cancel_token, world_arc) = start_playtest_server(&stored_world(&[("spawner", "Server", SPAWNER_SCRIPT)])).await;
    let client_endpoint = client_endpoint();
    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (connection, player_object_id) = join(&client_endpoint, server_port, join_token).await;

    // ACK every snapshot as it arrives, like the desktop client's datagram task, and remember the newest one
    let latest_received_message_num_arc = Arc::new(AtomicU64::new(0));
    tokio::spawn({
        let connection = connection.clone();
        let latest_received_message_num_arc = latest_received_message_num_arc.clone();
        async move {
            while let Ok(server_message) = awaitDatagramMessage::<ServerMessageUDP>(&connection).await {
                let ServerMessageUDP::Snapshot(snapshot) = server_message else {
                    continue;
                };
                let ack = ClientMessageUDP::Ack(ClientAckMessage {
                    message_num: snapshot.message_num,
                });
                let _ = sendDatagramMessage(&connection, &ack);
                latest_received_message_num_arc.fetch_max(snapshot.message_num, Ordering::SeqCst);
            }
        }
    });

    // wait for the character, then give the snapshots carrying it time to arrive and be ACKed
    let deadline = Instant::now() + Duration::from_secs(5);
    let character_object_id = loop {
        if let Some(character_object_id) = world_arc.read().await.getPlayerCharacterObject(player_object_id) {
            break character_object_id;
        }
        assert!(Instant::now() < deadline, "the script should give the player a character");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    tokio::time::sleep(Duration::from_millis(300)).await;

    let send_pose = |input_num: InputNum, position: Vec3, quaternion: Quat| {
        let input = ClientInputMessage {
            input_num,
            latest_applied_message_num: latest_received_message_num_arc.load(Ordering::SeqCst),
            foreign_player_delta: ForeignPlayerDelta {
                cameraOrientation: Orientation3D::Z(),
                playerMetadata: None,
                est_live_server_sim_ms: f64::INFINITY,
            },
            foreign_character_delta: Some(ForeignCharacterDelta {
                physicsInfo3D: PhysicsInfo3D::new(position, quaternion, Vec3::ZERO, Vec3::ZERO),
                est_live_server_sim_ms: f64::INFINITY, // the server stamps it with when it arrives
                teleportCount: 0,
            }),
        };
        sendDatagramMessage(&connection, &ClientMessageUDP::Input(input)).unwrap();
    };

    // a NaN position, then a NaN rotation where the character already stands, the order that got a NaN into the world
    send_pose(1, Vec3::new(f32::NAN, 0.0, 0.0), Quat::IDENTITY);
    tokio::time::sleep(Duration::from_millis(150)).await;
    send_pose(2, Vec3::new(5.0, 0.0, 0.0), Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0));
    tokio::time::sleep(Duration::from_millis(150)).await;

    let world_lock = world_arc.read().await;
    let physics_info_3d = world_lock.getObjectPhysicsInfo3D(character_object_id).unwrap();
    assert!(physics_info_3d.validate().is_ok(), "a pose with a NaN must be dropped: {physics_info_3d:?}");
    let history = &world_lock.characterPoseHistoryOfObjectId[&character_object_id];
    let mut entry_est_live_server_sim_ms = f64::NEG_INFINITY;
    while let Some((next_entry_est_live_server_sim_ms, pose)) = history.find_first_entry_after(entry_est_live_server_sim_ms) {
        let NetworkedCoords::WorldCoords(physics_info_3d) = &pose.coords else {
            panic!("a character's history is in world coordinates");
        };
        assert!(physics_info_3d.validate().is_ok(), "a NaN pose was remembered: {physics_info_3d:?}");
        entry_est_live_server_sim_ms = *next_entry_est_live_server_sim_ms;
    }
    drop(world_lock);
    let new_join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let (_new_connection, initial_world) = join_with_initial_world(&client_endpoint, server_port, new_join_token).await;
    serde_json::from_str::<StoredWorld>(&initial_world.stored_world_string).expect("a new player should be able to read the world they're sent");

    // the player's good poses still count
    send_pose(3, Vec3::new(-1000.0, 0.0, 0.0), Quat::IDENTITY);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let character_x = world_arc.read().await.getObjectPhysicsInfo3D(character_object_id).unwrap().location.position.x;
    assert_eq!(character_x, -1000.0, "a good pose after it should count");
    cancel_token.cancel();
}

// Two players join at the same moment. Neither may then get an object as new that its world on joining already had, since a
// client keeps its copy of an object it has, and the changes riding on that new-object message would be lost on its screen.
#[tokio::test]
async fn players_joining_at_once_never_get_an_object_twice() {
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&stored_world(&[])).await;
    let client_endpoint = client_endpoint();
    let first_join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let second_join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let (first_join, second_join) = tokio::join!(
        join_with_initial_world(&client_endpoint, server_port, first_join_token),
        join_with_initial_world(&client_endpoint, server_port, second_join_token),
    );

    for (connection, initial_world) in [first_join, second_join] {
        let stored_world: StoredWorld = serde_json::from_str(&initial_world.stored_world_string).unwrap();
        let mut object_ids_sent_twice = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            let Ok(Ok(ServerMessageUDP::Snapshot(snapshot))) =
                tokio::time::timeout(Duration::from_millis(100), awaitDatagramMessage::<ServerMessageUDP>(&connection)).await
            else {
                continue;
            };
            for (object_id, delta) in snapshot.deltaOfObjectId {
                if let ObjectDelta::Add(_) = delta
                    && stored_world.objectOfId.contains_key(&object_id)
                {
                    object_ids_sent_twice.push(object_id);
                }
            }
        }
        assert!(
            object_ids_sent_twice.is_empty(),
            "player {} got objects it already had as new: {object_ids_sent_twice:?}",
            initial_world.player_object_id
        );
    }
    cancel_token.cancel();
}

// Joining only waits on the joining player's own connection, so a player who stops partway through can't keep anyone else out.
#[tokio::test]
async fn a_player_who_stops_partway_through_joining_does_not_keep_others_out() {
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&stored_world(&[])).await;
    let client_endpoint = client_endpoint();

    // one connects and never says hello
    let _silent_connection = client_endpoint
        .connect(([127, 0, 0, 1], server_port).into(), QUIC_CERT_SERVER_NAME)
        .unwrap()
        .await
        .unwrap();

    // one gets their world and never says they're ready
    let unready_join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let unready_connection = client_endpoint
        .connect(([127, 0, 0, 1], server_port).into(), QUIC_CERT_SERVER_NAME)
        .unwrap()
        .await
        .unwrap();
    let (mut unready_send_stream, mut unready_recv_stream) = unready_connection.open_bi().await.unwrap();
    let hello = ClientHelloMessage {
        client_server_protocol_version: CLIENT_SERVER_PROTOCOL_VERSION,
        client_app_version: "test".to_string(),
        play_auth: unready_join_token,
    };
    sendTcpMessage(&mut unready_send_stream, &ClientHandshakeTCP::Hello(hello)).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        awaitTcpMessageMaxBytes::<ServerHandshakeTCP>(&mut unready_recv_stream, MAX_TCP_MESSAGE_BYTES),
    )
    .await
    .expect("a player should get their world while another is stuck joining")
    .unwrap();

    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    tokio::time::timeout(Duration::from_secs(5), join(&client_endpoint, server_port, join_token))
        .await
        .expect("a player should get all the way in while others are stuck joining");
    cancel_token.cancel();
}

// Pastes a fresh copy of the Client script named note every tick, so every tick sends players a new script's code.
const NOTE_PASTER_SCRIPT: &str = r#"const note = game.objectService.getObjectByName('note');\ngame.templateService.cutObjectToTemplate(note.id, 'note');\nwhile (true) {\n  game.templateService.pasteTemplate('note', game.world.id);\n  await tick();\n}\n"#;

// A script's code goes to each player on a stream of its own, which only opens once that player lets it. A player who never
// lets one open can't hold up the tick, which holds the world lock, so the game keeps running and others keep getting code.
// The player who never takes it is disconnected.
#[tokio::test]
async fn a_player_who_never_takes_script_code_does_not_stop_the_game() {
    let world = stored_world(&[("note", "Client", "const note = 'hello';"), ("notePaster", "Server", NOTE_PASTER_SCRIPT)]);
    let (server_port, auth_arc, cancel_token, world_arc) = start_playtest_server(&world).await;

    // a player who reads every script's code as it comes, like the desktop client
    let join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let (connection, _) = join(&client_endpoint(), server_port, join_token).await;
    let (raw_code_tx, mut raw_code_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn({
        let connection = connection.clone();
        async move {
            while let Ok(mut recv_stream) = connection.accept_uni().await {
                if let Ok(script_raw_code_message) = awaitTcpMessageMaxBytes::<ServerScriptRawCodeMessage>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await {
                    let _ = raw_code_tx.send(script_raw_code_message.raw_code);
                }
            }
        }
    });

    // a player who lets the server open no streams at all
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(QUIC_SERVER_CERT_DER_BYTES)).unwrap();
    let mut stuck_client_config = ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut stuck_transport = quinn::TransportConfig::default();
    stuck_transport.max_concurrent_uni_streams(0u32.into());
    stuck_client_config.transport_config(Arc::new(stuck_transport));
    let mut stuck_client_endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    stuck_client_endpoint.set_default_client_config(stuck_client_config);
    let stuck_join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (stuck_connection, _) = tokio::time::timeout(Duration::from_secs(5), join(&stuck_client_endpoint, server_port, stuck_join_token))
        .await
        .expect("the player should get in");

    // the game keeps pasting notes
    let mut num_scripts = Vec::new();
    for _ in 0..2 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let world_lock = tokio::time::timeout(Duration::from_secs(1), world_arc.read())
            .await
            .expect("the tick should not keep the world locked");
        let script_ids = world_lock._kindOfObjectId.get_keys(&ObjectKind::Script);
        num_scripts.push(script_ids.map_or(0, |script_ids| script_ids.len()));
        drop(world_lock);
    }
    assert!(num_scripts[1] > num_scripts[0], "the game should keep pasting notes: {num_scripts:?}");

    // the other player keeps getting each new note's code
    while raw_code_rx.try_recv().is_ok() {}
    let raw_code = tokio::time::timeout(Duration::from_secs(1), raw_code_rx.recv())
        .await
        .expect("the other player should keep getting new notes' code")
        .unwrap();
    assert!(raw_code.contains("hello"), "{raw_code}");

    let close_reason = tokio::time::timeout(Duration::from_secs(15), stuck_connection.closed())
        .await
        .expect("the player who never takes script code should be disconnected");
    assert!(close_reason.to_string().contains("scripts"), "{close_reason}");
    assert!(connection.close_reason().is_none(), "the other player should still be connected");
    cancel_token.cancel();
}

// Every tick makes a brick, and deletes the brick it made five ticks before.
const BRICK_MAKER_SCRIPT: &str = r#"const madeIds = [];\nconst v = (x, y, z) => ({ x, y, z });\nwhile (true) {\n  const brick = game.objectService.createObject('Brick', { type: 'brick', material: null, opacity: 1, color: { r: 1, g: 1, b: 1 }, isAlwaysOnTop: false, size: v(1, 1, 1), isWalkthrough: false, isPushable: false, density: 1, location: { position: v(0, 0, 0), rotation: v(0, 0, 0) }, movement: { velocity: v(0, 0, 0), angularVelocity: v(0, 0, 0) } }, game.world.id);\n  madeIds.push(brick.id);\n  if (madeIds.length > 5) game.objectService.deleteObject(madeIds.shift());\n  await tick();\n}\n"#;

// A player whose play loop is slow sends inputs far apart, but ACKs each snapshot the moment it arrives. So every ACK
// reaches the server in time, and it sends each new brick and each delete once, instead of again and again until an
// input carrying the ACK lands. That loop kept a slow Windows player getting the same deletes for a minute.
#[tokio::test]
async fn a_player_with_a_slow_play_loop_gets_each_change_once() {
    let (server_port, auth_arc, cancel_token, _world_arc) = start_playtest_server(&stored_world(&[("brickMaker", "Server", BRICK_MAKER_SCRIPT)])).await;
    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (connection, _) = join(&client_endpoint(), server_port, join_token).await;

    const INPUT_INTERVAL: Duration = Duration::from_millis(250); // a play loop that manages four ticks a second
    let mut num_adds_of_object_id: HashMap<ObjectId, usize> = HashMap::new();
    let mut num_removes_of_object_id: HashMap<ObjectId, usize> = HashMap::new();
    let mut latest_applied_message_num = 0;
    let mut input_num = 0;
    let mut next_input_at = Instant::now() + INPUT_INTERVAL;
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if Instant::now() >= next_input_at {
            next_input_at += INPUT_INTERVAL;
            input_num += 1;
            let input = ClientInputMessage {
                input_num,
                latest_applied_message_num,
                foreign_player_delta: ForeignPlayerDelta {
                    cameraOrientation: Orientation3D::Z(),
                    playerMetadata: None,
                    est_live_server_sim_ms: f64::INFINITY,
                },
                foreign_character_delta: None,
            };
            sendDatagramMessage(&connection, &ClientMessageUDP::Input(input)).unwrap();
        }
        let Ok(Ok(ServerMessageUDP::Snapshot(snapshot))) =
            tokio::time::timeout(Duration::from_millis(20), awaitDatagramMessage::<ServerMessageUDP>(&connection)).await
        else {
            continue;
        };
        let ack = ClientMessageUDP::Ack(ClientAckMessage {
            message_num: snapshot.message_num,
        });
        sendDatagramMessage(&connection, &ack).unwrap();
        latest_applied_message_num = latest_applied_message_num.max(snapshot.message_num);
        for (object_id, delta) in snapshot.deltaOfObjectId {
            match delta {
                ObjectDelta::Add(_) => *num_adds_of_object_id.entry(object_id).or_default() += 1,
                ObjectDelta::Remove(_) => *num_removes_of_object_id.entry(object_id).or_default() += 1,
                _ => {}
            }
        }
    }

    assert!(
        num_removes_of_object_id.len() >= 10,
        "the script should keep making and deleting bricks, but only {} were deleted",
        num_removes_of_object_id.len()
    );
    let object_ids_sent_again: Vec<ObjectId> = num_adds_of_object_id
        .iter()
        .chain(num_removes_of_object_id.iter())
        .filter(|&(_, &num_sends)| num_sends > 1)
        .map(|(&object_id, _)| object_id)
        .collect();
    assert!(
        object_ids_sent_again.is_empty(),
        "these objects' adds or deletes were sent more than once: {object_ids_sent_again:?}"
    );
    cancel_token.cancel();
}

// Every player keeps getting everyone's ping, once a second.
#[tokio::test]
async fn every_player_gets_everyones_ping_once_a_second() {
    let (server_port, auth_arc, cancel_token, _) = start_playtest_server(&stored_world(&[])).await;
    let client_endpoint = client_endpoint();
    let first_join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let second_join_token = auth_arc.reserve("Guest".to_string(), None, PlayerProfile::GUEST).await;
    let (first_connection, first_player_object_id) = join(&client_endpoint, server_port, first_join_token).await;
    let (second_connection, second_player_object_id) = join(&client_endpoint, server_port, second_join_token).await;

    for connection in [&first_connection, &second_connection] {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut num_player_pings_received = 0;
        while num_player_pings_received < 2 && Instant::now() < deadline {
            if let Ok(Ok(ServerMessageUDP::PlayerPings(player_pings))) =
                tokio::time::timeout(Duration::from_millis(100), awaitDatagramMessage::<ServerMessageUDP>(connection)).await
                && player_pings.pingMsOfPlayerObjectId.contains_key(&first_player_object_id)
                && player_pings.pingMsOfPlayerObjectId.contains_key(&second_player_object_id)
            {
                num_player_pings_received += 1;
            }
        }
        assert_eq!(num_player_pings_received, 2, "each player should keep getting both players' pings");
    }
    cancel_token.cancel();
}

// Names each player after the ping a server script reads for them, once it has one.
const PING_NAMER_SCRIPT: &str = r#"while (true) {\n  for (const player of game.playerService.getAllPlayers()) {\n    const ping = game.playerService.getPlayerPing(player.id);\n    if (ping !== null) player.name = 'ping-' + ping;\n  }\n  await tick();\n}\n"#;

// A server script reads a player's ping off their connection.
#[tokio::test]
async fn a_server_script_reads_a_players_ping() {
    let (server_port, auth_arc, cancel_token, world_arc) = start_playtest_server(&stored_world(&[("ping_namer", "Server", PING_NAMER_SCRIPT)])).await;
    let join_token = auth_arc.reserve("Kubester".to_string(), Some(17), PlayerProfile::GUEST).await;
    let (_connection, player_object_id) = join(&client_endpoint(), server_port, join_token).await;

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut player_name = String::new();
    while !player_name.starts_with("ping-") && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let world_lock = world_arc.read().await;
        player_name = world_lock.getObject(player_object_id).map(|player| player.name.clone()).unwrap_or_default();
        drop(world_lock);
    }
    assert!(
        player_name.strip_prefix("ping-").is_some_and(|ping_ms| ping_ms.parse::<u16>().is_ok()),
        "the script should name the player after a number of milliseconds: {player_name}"
    );
    cancel_token.cancel();
}
