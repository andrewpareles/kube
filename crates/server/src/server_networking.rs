use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::database::{ValidatedPlayAuth, end_player_session, get_play_auth_hash_of_profile_id_batch, insert_game_chats, validate_play_auth};
use crate::playtest_auth::{PlaytestAuth, PlaytestServerStatus};
use crate::posthog::capture_posthog_event;
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bincode::Options;
use game_types::{
    consts::{
        CLIENT_SERVER_PROTOCOL_VERSION, CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC, JITTER_MARGIN_MS, MAX_POSE_LAG_MS, MAX_UNSHIFTED_POSE_LAG_MS,
        SERVER_MAX_IDLE_TIMEOUT_SECS, SERVER_SIM_AND_SEND_SECS,
    },
    object_delta::is_valid_player_metadata,
    object_types::{LocalGroupInfo, ObjectBeingAdded, ObjectId, ObjectInfo, ObjectKind, Orientation3D, PhysicsInfo3D, PlayerInfo, player::UserId},
};
use profanity::censor;
use quinn::{Connection, Endpoint, SendStream, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shared::protocol::{
    bincode_config::get_bincode_config,
    messages::{
        ClientAckMessage, ClientGameChatMessage, ClientHandshakeTCP, ClientInputMessage, ClientMessageTCP, ClientMessageUDP, ClientPingMessage,
        GAME_CHAT_HISTORY_LIMIT, GAME_CHAT_MAX_MESSAGE_CHARS, GameChatEntry, GameChatMessage, ServerFnCallMessage, ServerFunctionResultMessage,
        ServerGameChatMessage, ServerGameChatResultMessage, ServerHandshakeTCP, ServerMessageUDP, ServerPlayerPingsMessage, ServerPongMessage,
        ServerSnapshotMessage,
    },
    quic_cert::QUIC_SERVER_CERT_DER_BYTES,
};
use shared::{
    conversions::time_conversions::{get_accurate_elapsed_time, get_accurate_elapsed_time_at},
    env::env_vars::{GAME_ID, QUIC_SERVER_KEY_DER_BASE64},
    game::world::{JsCallbackInputs, World, WorldDirtyObjectFlags},
    helpers::{
        hashmap_heap::HashMapHeap, hashmap_queue::HashMapQueue, hashmap_with_reverse::HashMapWithReverse, iterator_round_robin::IteratorRoundRobin,
        multi_thread_hashmap::MultiThreadHashMap,
    },
    protocol::{
        messages::{
            ForeignCharacterDelta, ForeignPlayerDelta, InputNum, MessageNum, NetworkedCoords, ObjectAddDelta, ObjectDelta, ObjectModifyDelta,
            ObjectModifyFlagId, ObjectModifyFlags, ServerInitialWorldMessage, ServerScriptRawCodeMessage,
        },
        quic_send_receive_helpers::{
            MAX_TCP_MESSAGE_BYTES, awaitDatagramMessage, awaitTcpMessage, awaitTcpMessageMaxBytes, getPingMs, sendDatagramMessage, sendTcpMessage,
        },
    },
    player_profile::PlayerProfile,
};
use tokio::sync::{Mutex, RwLock, Semaphore, broadcast, mpsc, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

// ----------------- Constants -----------------

pub const GAME_CHAT_SUBMISSION_INTERVAL: Duration = Duration::from_millis(500); // TODO: AI-PICKED-VALUE: Process at most two messages per second from each connection, using stream backpressure.

const REVALIDATE_USERS_POLL_SECS: u64 = 10; // kick players with a changed play_auth

// What a snapshot costs before any changes go in. It leaves 2 bytes for the change count to grow past 250.
fn empty_snapshot_size(message_num: MessageNum, live_server_sim_ms: f64) -> u64 {
    let empty_snapshot = ServerMessageUDP::Snapshot(ServerSnapshotMessage {
        message_num,
        live_server_sim_ms,
        deltaOfObjectId: HashMap::new(),
        deltaOfObjectId2: HashMap::new(),
    });
    let Ok(size) = get_bincode_config().serialized_size(&empty_snapshot) else {
        panic!("Failed to serialize empty snapshot to get initial message size");
    };
    size + 2
}

enum SizeCheckResult {
    Skip,
    OverLimit,
    Ok,
}

impl SizeCheckResult {
    fn checkMessageSize(curr_message_size: &mut u64, max_message_bytes: u64, object_id: ObjectId, delta: &ObjectDelta) -> SizeCheckResult {
        let bincode_config = get_bincode_config();

        let (Ok(object_id_size), Ok(delta_size)) = (bincode_config.serialized_size(&object_id), bincode_config.serialized_size(delta)) else {
            log::error!("Error serializing delta, this is very unexpected/bad");
            return SizeCheckResult::Skip; // should never happen....
        };

        let entry_size = object_id_size + delta_size;
        if entry_size > max_message_bytes {
            // a delta this big does not fit in a message even on its own, so it gets skipped every tick until the connection allows bigger messages
            // the only object fields big enough to do this are ones with no size limit: a PropertyAnimator's keyframes, and an object's name.
            log::error!(
                "Object needs {entry_size} bytes, which is over this connection's {max_message_bytes} byte message limit, so it can't be sent to this user"
            );
            return SizeCheckResult::Skip;
        }
        *curr_message_size += entry_size;
        if *curr_message_size > max_message_bytes {
            return SizeCheckResult::OverLimit;
        }
        SizeCheckResult::Ok
    }
}

// ----------------- Server (public) -----------------

// tx/rx'd to main game loop
pub struct ServerForeignCharacterDTO {
    pub came_from_player_object_id: ObjectId, // trusted
    pub est_live_server_sim_ms: f64,          // trusted (estimated on client and validated on server)

    pub physicsInfo3D: PhysicsInfo3D, // untrusted - new physics
    pub teleportCount: u64,           // untrusted
}

// tx/rx'd to main game loop
pub struct ServerForeignPlayerDTO {
    pub came_from_player_object_id: ObjectId,     // trusted
    pub foreign_player_delta: ForeignPlayerDelta, // untrusted, but checked: a finite camera, and metadata that's JSON under the size limit
}

struct DirtyDeltaFlags {
    dirty_adds: HashSet<ObjectId>,
    dirty_modifies: HashMapQueue<ObjectId, ObjectModifyFlags>, // since these can be added to many times, they need to be ordered so one doesnt steal priority (could randomize but thatd be worse)
    dirty_foreign_characters: HashMapQueue<ObjectId, ForeignCharacterDelta>,
    dirty_foreign_players: HashMapQueue<ObjectId, ForeignPlayerDelta>,
    dirty_removes: HashSet<ObjectId>,
}

struct UnackedDeltaFlags {
    unacked_adds: HashMapWithReverse<ObjectId, MessageNum>, // objectid -> latest_message_num where it was sent
    unacked_modifies: HashMapWithReverse<(ObjectId, ObjectModifyFlagId), MessageNum>, // (objectid, field) -> latest_message_num where it was sent
    unacked_removes: HashMapWithReverse<ObjectId, MessageNum>, // objectid -> latest_message_num where it was sent
}

enum SnapshotSendState {
    WaitingForReady,
    Ready { next_message_num: MessageNum }, // message # incrementor for the user
}

struct UserConnectionInfo {
    // connection stuff
    connection: Connection, // QUIC connection

    // controlled by server
    snapshot_send_state: SnapshotSendState,
    time_of_unacked_message_num: HashMapHeap<MessageNum, f64>, // fast access to the oldest unack'd message
    // the newest snapshot that adds this player or their character, moves their character, or changes their player info.
    // Their poses count once they say they applied it. Message numbers start at 1, so 0 means none yet
    poses_wait_for_message_num: MessageNum,

    dirty: DirtyDeltaFlags,     // objects that are not yet sent, but should be
    unacked: UnackedDeltaFlags, // objects that are sent and awaiting an ack
}

pub struct PlayerPoseLag {
    smoothed_lag_ms: f32,
    prev_recv_live_server_wall_ms: f64,
    latest_est_live_server_sim_ms: f64,
}

impl PlayerPoseLag {
    pub fn new(recv_live_server_wall_ms: f64) -> Self {
        Self {
            smoothed_lag_ms: 0.0,
            prev_recv_live_server_wall_ms: recv_live_server_wall_ms,
            latest_est_live_server_sim_ms: 0.0,
        }
    }

    pub fn shift_stamp(&mut self, est_live_server_sim_ms: f64, recv_live_server_wall_ms: f64) -> f64 {
        // Validate client's estiamted time by clamping it
        let est_live_server_sim_ms = if est_live_server_sim_ms.is_finite() {
            est_live_server_sim_ms.max(recv_live_server_wall_ms - MAX_POSE_LAG_MS as f64)
        } else {
            recv_live_server_wall_ms
        };
        // this tells players to show a laggy player as even more behind than the normal CLIENT_WORLD_DELAY amount
        let ms_since_prev_recv = (recv_live_server_wall_ms - self.prev_recv_live_server_wall_ms) as f32;
        self.prev_recv_live_server_wall_ms = recv_live_server_wall_ms;
        let max_lag_decrease_ms = CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC * ms_since_prev_recv / 1000.0;
        self.smoothed_lag_ms = ((recv_live_server_wall_ms - est_live_server_sim_ms) as f32).max(self.smoothed_lag_ms - max_lag_decrease_ms);
        let est_live_server_sim_ms = est_live_server_sim_ms + (self.smoothed_lag_ms - MAX_UNSHIFTED_POSE_LAG_MS).max(0.0) as f64;
        // Validated time:
        let est_live_server_sim_ms = est_live_server_sim_ms.min(recv_live_server_wall_ms).max(self.latest_est_live_server_sim_ms);
        self.latest_est_live_server_sim_ms = est_live_server_sim_ms;
        est_live_server_sim_ms
    }
}

// Production players use account reservations; playtest players claim temporary tickets.
#[derive(Clone)]
pub enum ServerMode {
    Official,
    Playtest(Arc<PlaytestAuth>),
}

pub struct Server {
    pub server_mode: ServerMode,
    auth_info_of_player_object_id: RwLock<HashMap<ObjectId, (String, String)>>, // (play_auth_hash, profile_id)
    userConnectionInfoOfPlayerObjectId: RwLock<HashMap<ObjectId, UserConnectionInfo>>,
    connection_of_player_object_id_arc: Arc<MultiThreadHashMap<ObjectId, Connection>>,
    startTime: Instant,
    new_user_mutex: Mutex<()>,
    game_chat_arc: Arc<Mutex<ServerGameChat>>,
    player_cleanup_tasks_tracker: TaskTracker,
}

impl Server {
    pub fn new(
        startTime: Instant,
        connection_of_player_object_id_arc: Arc<MultiThreadHashMap<ObjectId, Connection>>,
        server_mode: ServerMode,
        player_cleanup_tasks_tracker: TaskTracker,
        js_callback_inputs_tx: Option<mpsc::UnboundedSender<JsCallbackInputs>>,
    ) -> Self {
        Self {
            server_mode,
            auth_info_of_player_object_id: RwLock::new(HashMap::new()),
            userConnectionInfoOfPlayerObjectId: RwLock::new(HashMap::new()),
            connection_of_player_object_id_arc,
            startTime,
            new_user_mutex: Mutex::new(()),
            game_chat_arc: Arc::new(Mutex::new(ServerGameChat::new(js_callback_inputs_tx))),
            player_cleanup_tasks_tracker,
        }
    }

    async fn insertAuthInfoOfPlayerObjectId(&self, player_object_id: ObjectId, play_auth_hash: String, profile_id: String) {
        self.auth_info_of_player_object_id
            .write()
            .await
            .insert(player_object_id, (play_auth_hash, profile_id));
    }

    async fn removeAuthInfo(&self, player_object_id: ObjectId) -> Option<(String, String)> {
        self.auth_info_of_player_object_id.write().await.remove(&player_object_id)
    }

    pub fn get_live_server_wall_ms(&self) -> f64 {
        get_accurate_elapsed_time(self.startTime)
    }

    pub fn get_live_server_sim_ms_at(&self, tick_instant: Instant) -> f64 {
        get_accurate_elapsed_time_at(self.startTime, tick_instant)
    }

    // ---------------------------- Inputs to ignore ----------------------------

    // we ignore if there is an unacked change about the object they control which they have yet to ACK.
    // A player ACKs a snapshot as soon as it arrives, before applying it, so a pose made before latest_applied_message_num
    // reached that change is ignored too.
    pub async fn shouldIgnoreForeignCharacterDTO(
        &self,
        world: &World,
        foreign_character_dto: &ServerForeignCharacterDTO,
        latest_applied_message_num: MessageNum,
    ) -> bool {
        fn userStillNeedsToACK(user: &UserConnectionInfo, object_id: ObjectId, flag_id: ObjectModifyFlagId) -> bool {
            user.dirty.dirty_adds.contains(&object_id)
                || user.dirty.dirty_modifies.get(&object_id).is_some_and(|flags| flag_id.is_in(flags))
                || user.unacked.unacked_adds.contains_key(&object_id)
                || user.unacked.unacked_modifies.contains_key(&(object_id, flag_id))
        }

        // get player and character id
        let player_object_id = foreign_character_dto.came_from_player_object_id;
        let Some(character_object_id) = world.getPlayerCharacterObject(player_object_id) else {
            return false;
        };

        let users_lock = self.userConnectionInfoOfPlayerObjectId.read().await;
        let should_ignore_foreign_character_dto = match users_lock.get(&player_object_id) {
            None => true, // impossible
            Some(user) => {
                // TODO: ModifiedObjectInfo should eventually be reduced down to just characterObjectId so the server doesnt ignore messages if we just changed eg their head. but it's ok for now.
                let is_owed_character_move = userStillNeedsToACK(user, character_object_id, ObjectModifyFlagId::ModifiedPhysicsInfo3D);
                let is_owed_player_info = userStillNeedsToACK(user, player_object_id, ObjectModifyFlagId::ModifiedObjectInfo);
                let is_pose_from_before_own_change = latest_applied_message_num < user.poses_wait_for_message_num;
                is_owed_character_move || is_owed_player_info || is_pose_from_before_own_change
            }
        };
        drop(users_lock);

        should_ignore_foreign_character_dto
    }

    // ---------------------------- Send snapshots ----------------------------

    pub async fn sendNewSnapshotToAllUsers(
        &self,
        WorldDirtyObjectFlags {
            addedObjectIds,
            modifiedObjectFlags,
            removedObjectIds,
        }: WorldDirtyObjectFlags,
        foreign_character_dtos: HashMap<ObjectId, ServerForeignCharacterDTO>,
        foreign_player_dtos: HashMap<ObjectId, ServerForeignPlayerDTO>,
        new_script_deltas: HashMap<ObjectId, String>,
        live_server_sim_ms: f64,
        world: &World,
    ) {
        let mut users_lock = self.userConnectionInfoOfPlayerObjectId.write().await;

        Self::markObjectsAsDirtyForAllUsers(
            &mut users_lock,
            addedObjectIds,
            modifiedObjectFlags,
            foreign_character_dtos,
            foreign_player_dtos,
            removedObjectIds,
        );

        // send snapshots or initial worlds to all users
        let live_server_wall_ms = self.get_live_server_wall_ms();
        for (&player_object_id, user) in users_lock.iter_mut() {
            let SnapshotSendState::Ready { next_message_num } = &mut user.snapshot_send_state else {
                continue;
            };
            let Some(max_message_bytes) = user.connection.max_datagram_size() else {
                continue;
            };
            let message_num = *next_message_num;

            // two-way ping ms
            let USER_ROUND_TRIP_MS = (user.connection.rtt().as_secs_f64() * 1000.0) as f32;
            // messages sent before this time have waited long enough that their ack is considered lost
            let resend_if_sent_before_ms = live_server_wall_ms - (JITTER_MARGIN_MS + USER_ROUND_TRIP_MS) as f64;

            // Get deltas and update their dirty/unacked states: first resend anything whose ack is
            // considered lost (oldest first), then fill the remaining packet space with fresh dirty deltas.
            let mut deltaOfObjectId = HashMap::new();
            let mut curr_message_size = empty_snapshot_size(message_num, live_server_sim_ms);
            Self::getAndRefreshNewDeltasFromUnacked(
                &mut user.dirty,
                &mut user.unacked,
                &mut user.time_of_unacked_message_num,
                message_num,
                resend_if_sent_before_ms,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                max_message_bytes as u64,
            );
            Self::getAndRemoveNewDeltasFromDirty(
                &mut user.dirty,
                &mut user.unacked,
                &mut user.time_of_unacked_message_num,
                message_num,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                max_message_bytes as u64,
            );

            // update remaining fields
            *next_message_num += 1;

            // if the message had anything that should be acked and resent (aka not a foreign character or player delta), add it to the unack'd heap
            let did_send_reliable_delta = user.unacked.unacked_adds.contains_value(&message_num)
                || user.unacked.unacked_modifies.contains_value(&message_num)
                || user.unacked.unacked_removes.contains_value(&message_num);
            if did_send_reliable_delta {
                user.time_of_unacked_message_num.insert(message_num, live_server_wall_ms);
            }

            // delete any user-specific objects from deltaOfObjectId and put them in the instant-apply deltaOfObjectId2
            let mut deltaOfObjectId2 = HashMap::new();
            let character_object_id = world.getPlayerCharacterObject(player_object_id);
            let own_object_ids = [Some(player_object_id), character_object_id];
            // the same changes shouldIgnoreForeignCharacterDTO waits to be ACKed: an add of the player or their character,
            // a move of the character, or a change to the player's info
            let mut should_poses_wait_for_snapshot = false;
            for own_object_id in own_object_ids.into_iter().flatten() {
                let removed_delta = deltaOfObjectId.remove(&own_object_id);
                match removed_delta {
                    None => {}
                    Some(ObjectDelta::Modify(own_modify)) => {
                        should_poses_wait_for_snapshot |= match Some(own_object_id) == character_object_id {
                            true => own_modify.modifiedPhysicsInfo3D.is_some(),
                            false => own_modify.modifiedObjectInfo.is_some(),
                        };
                        deltaOfObjectId2.insert(own_object_id, own_modify);
                    }
                    Some(own_add_or_remove) => {
                        if let ObjectDelta::Add(_) = own_add_or_remove {
                            should_poses_wait_for_snapshot = true;
                        }
                        deltaOfObjectId.insert(own_object_id, own_add_or_remove);
                    }
                }
            }
            if should_poses_wait_for_snapshot {
                user.poses_wait_for_message_num = message_num;
            }

            // build snapshot
            let snapshot = ServerSnapshotMessage {
                message_num,
                live_server_sim_ms,
                deltaOfObjectId,
                deltaOfObjectId2,
            };

            // send snapshot to user via QUIC datagram
            let snapshot_message = ServerMessageUDP::Snapshot(snapshot);
            match sendDatagramMessage(&user.connection, &snapshot_message) {
                Ok(()) => (),
                Err(e) => {
                    log::warn!("Error sending snapshot to user: {}. The user may have disconnected.", e);
                    continue;
                }
            };
        }

        let script_raw_code_connections: Vec<Connection> = users_lock
            .values()
            .map(|user_connection_info| user_connection_info.connection.clone())
            .collect();
        drop(users_lock);

        // send raw code of any scripts that were just added, over dedicated reliable streams
        for connection in script_raw_code_connections {
            for (object_id, raw_code) in &new_script_deltas {
                let mut send_stream_script_raw_code = match connection.open_uni().await {
                    Ok(send_stream_script_raw_code) => send_stream_script_raw_code,
                    Err(error) => {
                        log::warn!("Error opening script raw code stream: {}", error);
                        break;
                    }
                };
                let send_result = sendTcpMessage(
                    &mut send_stream_script_raw_code,
                    &ServerScriptRawCodeMessage {
                        object_id: *object_id,
                        raw_code: raw_code.clone(),
                    },
                )
                .await;
                if let Err(error) = send_result {
                    log::warn!("Error sending script raw code to user: {}", error);
                    break;
                }
                let _ = send_stream_script_raw_code.finish();
            }
        }
    }

    // ---------------------------- Send player pings ----------------------------

    // Every player's ping, as measured on their connection, sent to every player. Pings change slowly, so this goes out once a second.
    pub async fn sendPlayerPingsToAllUsers(&self) {
        let users_lock = self.userConnectionInfoOfPlayerObjectId.read().await;
        let player_pings_message = ServerMessageUDP::PlayerPings(ServerPlayerPingsMessage {
            pingMsOfPlayerObjectId: users_lock
                .iter()
                .map(|(&player_object_id, user)| (player_object_id, getPingMs(&user.connection)))
                .collect(),
        });
        for user in users_lock.values() {
            let SnapshotSendState::Ready { .. } = user.snapshot_send_state else {
                continue;
            };
            if let Err(error) = sendDatagramMessage(&user.connection, &player_pings_message) {
                log::warn!("Error sending player pings to user: {}", error);
            }
        }
        drop(users_lock);
    }

    // returns None if object was removed (or in other cases that should never happen)
    fn buildAddDelta(object_id: ObjectId, world: &World) -> Option<ObjectDelta> {
        // if the object was removed, return None
        if !world.hasObject(object_id) {
            return None;
        }
        match world.buildAddDelta(object_id) {
            Ok(delta) => Some(ObjectDelta::Add(delta)),
            Err(error) => {
                log::error!("buildAddDelta: {error}");
                None
            }
        }
    }

    fn buildModifyDelta(object_id: ObjectId, flags: &ObjectModifyFlags, world: &World) -> ObjectDelta {
        let Ok(mut delta) = world.buildModifyDelta(object_id, flags) else {
            // if the object was removed, just send the user *something* to get them to ACK so we can send the remove message
            return ObjectDelta::Modify(ObjectModifyDelta {
                modifiedObjectInfo: None,
                modifiedPrimitiveSettings: None,
                modifiedPhysicsToggles3D: None,
                modifiedPhysicsInfo3D: None,
                modifiedParent: None,
                modifiedName: None,
            });
        };
        // If the server explicitly marked physics dirty, always send that exact transform even for player-controlled objects.
        // Foreign character motion already goes through the separate ForeignCharacter path and does not use this flag.
        if let Some(networked_physics_info_3d) = &mut delta.modifiedPhysicsInfo3D
            && let Some(relative_rigid_group_location) = world.physics.getSubitemRelativeLocation3D(object_id)
        {
            networked_physics_info_3d.coords = NetworkedCoords::SubitemRelativeCoords(relative_rigid_group_location);
        }
        ObjectDelta::Modify(delta)
    }

    fn buildForeignCharacterDelta(foreign_character_delta: &ForeignCharacterDelta) -> ObjectDelta {
        ObjectDelta::ForeignCharacter(foreign_character_delta.clone())
    }

    fn buildRemoveDelta() -> ObjectDelta {
        ObjectDelta::Remove(())
    }

    fn onUnackedChange(
        unacked: &UnackedDeltaFlags,
        time_of_unacked_message_num: &mut HashMapHeap<MessageNum, f64>,
        old_message_num: Option<MessageNum>,
        new_message_num: MessageNum,
    ) {
        let Some(old_message_num) = old_message_num else {
            return;
        };
        if old_message_num == new_message_num {
            return;
        }

        // When an object moves from one message number to another, drop the old message number from the heap as soon as nothing still points at it.
        let is_old_message_num_still_used = unacked.unacked_adds.contains_value(&old_message_num)
            || unacked.unacked_modifies.contains_value(&old_message_num)
            || unacked.unacked_removes.contains_value(&old_message_num);
        if !is_old_message_num_still_used {
            time_of_unacked_message_num.remove(&old_message_num);
        }
    }

    fn getAndRemoveNewDeltasFromDirty(
        dirty: &mut DirtyDeltaFlags,
        unacked: &mut UnackedDeltaFlags,
        time_of_unacked_message_num: &mut HashMapHeap<MessageNum, f64>,
        message_num: MessageNum,
        world: &World,
        deltaOfObjectId: &mut HashMap<ObjectId, ObjectDelta>,
        curr_message_size: &mut u64,
        max_message_bytes: u64,
    ) {
        // create iterator
        // TODO: could use 3d proximity to player based on `let playerInfo3D = world.getPlayerBodyVec3(user.playerObjectId);`, would need to cache rough object sizes in physics
        const ADD_IDX: usize = 0;
        const MODIFY_IDX: usize = 1;
        const FOREIGN_CHARACTER_IDX: usize = 2;
        const REMOVE_IDX: usize = 3;
        const FOREIGN_PLAYER_IDX: usize = 4;
        let mut picker = IteratorRoundRobin::<5, ObjectId>::new([
            Box::new(dirty.dirty_adds.iter().copied()),
            Box::new(dirty.dirty_modifies.keys_oldest_first().copied()),
            Box::new(dirty.dirty_foreign_characters.keys_oldest_first().copied()),
            Box::new(dirty.dirty_removes.iter().copied()),
            Box::new(dirty.dirty_foreign_players.keys_oldest_first().copied()),
        ]);

        // gather object ids to remove from dirty
        let mut dirty_addeds_to_remove = Vec::new();
        let mut dirty_modifieds_to_remove = Vec::new();
        let mut dirty_foreign_characters_to_remove = Vec::new();
        let mut dirty_removeds_to_remove = Vec::new();
        let mut dirty_foreign_players_to_remove = Vec::new();

        while *curr_message_size < max_message_bytes {
            let Some((picker_index, object_id)) = picker.next() else {
                break;
            };

            // an object that already carries a resent delta in this message keeps its dirty entry for a later message
            if deltaOfObjectId.contains_key(&object_id) {
                continue;
            }

            match picker_index {
                ADD_IDX => {
                    // build the delta
                    let Some(delta) = Self::buildAddDelta(object_id, world) else {
                        // should never happen since removing an object also removes it from dirty
                        log::error!("getAndRemoveNewDeltasFromDirty buildAddDelta: object {object_id} not found");
                        continue;
                    };

                    // check if we should stop
                    match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                        SizeCheckResult::OverLimit => break,
                        SizeCheckResult::Skip => continue,
                        SizeCheckResult::Ok => {}
                    }

                    // add to unack'd
                    let old_message_num = unacked.unacked_adds.insert(object_id, message_num);
                    Self::onUnackedChange(unacked, time_of_unacked_message_num, old_message_num, message_num);

                    // mark to remove from dirty
                    dirty_addeds_to_remove.push(object_id);

                    // add to return val
                    deltaOfObjectId.insert(object_id, delta);
                }
                MODIFY_IDX => {
                    // if unacked add exists, we cannot modify it yet, skip
                    if unacked.unacked_adds.contains_key(&object_id) {
                        continue;
                    }

                    // build the delta
                    let Some(flags) = dirty.dirty_modifies.get(&object_id) else {
                        continue; // should never happen
                    };
                    let delta = Self::buildModifyDelta(object_id, flags, world);

                    // check if we should stop
                    match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                        SizeCheckResult::OverLimit => break,
                        SizeCheckResult::Skip => continue,
                        SizeCheckResult::Ok => {}
                    }

                    // add to unack'd
                    for flag_id in ObjectModifyFlagId::iter() {
                        if flag_id.is_in(flags) {
                            let old_message_num = unacked.unacked_modifies.insert((object_id, flag_id), message_num);
                            Self::onUnackedChange(unacked, time_of_unacked_message_num, old_message_num, message_num);
                        }
                    }

                    // mark to remove from dirty
                    dirty_modifieds_to_remove.push(object_id);

                    // add to return val
                    deltaOfObjectId.insert(object_id, delta);
                }
                FOREIGN_CHARACTER_IDX => {
                    // if unacked add exists, we cannot modify it yet, skip
                    if unacked.unacked_adds.contains_key(&object_id) {
                        continue;
                    }

                    let Some(foreign_character_delta) = dirty.dirty_foreign_characters.get(&object_id) else {
                        continue;
                    };
                    let delta = Self::buildForeignCharacterDelta(foreign_character_delta);

                    match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                        SizeCheckResult::OverLimit => break,
                        SizeCheckResult::Skip => continue,
                        SizeCheckResult::Ok => {}
                    }

                    dirty_foreign_characters_to_remove.push(object_id);
                    deltaOfObjectId.insert(object_id, delta);
                }
                FOREIGN_PLAYER_IDX => {
                    // if unacked add exists, we cannot modify it yet, skip
                    if unacked.unacked_adds.contains_key(&object_id) {
                        continue;
                    }

                    let Some(foreign_player_delta) = dirty.dirty_foreign_players.get(&object_id) else {
                        continue;
                    };
                    let delta = ObjectDelta::ForeignPlayer(foreign_player_delta.clone());

                    match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                        SizeCheckResult::OverLimit => break,
                        SizeCheckResult::Skip => continue,
                        SizeCheckResult::Ok => {}
                    }

                    dirty_foreign_players_to_remove.push(object_id);
                    deltaOfObjectId.insert(object_id, delta);
                }
                REMOVE_IDX => {
                    // if unacked add exists, we cannot remove it yet, skip
                    if unacked.unacked_adds.contains_key(&object_id) {
                        continue;
                    }
                    // if unacked modify exists, we cannot remove it yet, skip
                    fn object_is_an_unacked_modify(unacked: &UnackedDeltaFlags, object_id: ObjectId) -> bool {
                        for flagId in ObjectModifyFlagId::iter() {
                            if unacked.unacked_modifies.contains_key(&(object_id, flagId)) {
                                return true;
                            }
                        }
                        false
                    }
                    if object_is_an_unacked_modify(unacked, object_id) {
                        continue;
                    }

                    // build the delta
                    let delta = Self::buildRemoveDelta();

                    // check if we should stop
                    match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                        SizeCheckResult::OverLimit => break,
                        SizeCheckResult::Skip => continue,
                        SizeCheckResult::Ok => {}
                    }

                    // add to unack'd
                    let old_message_num = unacked.unacked_removes.insert(object_id, message_num);
                    Self::onUnackedChange(unacked, time_of_unacked_message_num, old_message_num, message_num);

                    // mark to remove from dirty
                    dirty_removeds_to_remove.push(object_id);

                    // add to return val
                    deltaOfObjectId.insert(object_id, delta);
                }
                _ => {
                    break;
                }
            }
        }

        drop(picker);

        // remove from dirty
        for object_id in dirty_addeds_to_remove {
            dirty.dirty_adds.remove(&object_id);
        }
        for object_id in dirty_modifieds_to_remove {
            dirty.dirty_modifies.remove(&object_id);
        }
        for object_id in dirty_foreign_characters_to_remove {
            dirty.dirty_foreign_characters.remove(&object_id);
        }
        for object_id in dirty_removeds_to_remove {
            dirty.dirty_removes.remove(&object_id);
        }
        for object_id in dirty_foreign_players_to_remove {
            dirty.dirty_foreign_players.remove(&object_id);
        }
    }

    fn clear_dirty_modify_flags(dirty_modifies: &mut HashMapQueue<ObjectId, ObjectModifyFlags>, object_id: ObjectId, flags_to_clear: &ObjectModifyFlags) {
        let Some(dirty_flags) = dirty_modifies.get_mut(&object_id) else {
            return;
        };

        // Clear only the dirty fields that were just resent from the current world state.
        for flag_id in ObjectModifyFlagId::iter() {
            if !flag_id.is_in(flags_to_clear) {
                continue;
            }
            match flag_id {
                ObjectModifyFlagId::ModifiedObjectInfo => dirty_flags.modifiedObjectInfo = false,
                ObjectModifyFlagId::ModifiedPhysicsToggles3D => dirty_flags.modifiedPhysicsToggles3D = false,
                ObjectModifyFlagId::ModifiedPhysicsInfo3D => dirty_flags.modifiedPhysicsInfo3D = false,
                ObjectModifyFlagId::ModifiedPrimitiveSettings => dirty_flags.modifiedPrimitiveSettings = false,
                ObjectModifyFlagId::ModifiedParent => dirty_flags.modifiedParent = false,
                ObjectModifyFlagId::ModifiedName => dirty_flags.modifiedName = false,
            }
        }

        if dirty_flags.is_empty() {
            dirty_modifies.remove(&object_id);
        }
    }

    fn getAndRefreshNewDeltasFromUnacked(
        dirty: &mut DirtyDeltaFlags,
        unacked: &mut UnackedDeltaFlags,
        time_of_unacked_message_num: &mut HashMapHeap<MessageNum, f64>,
        message_num: MessageNum,
        resend_if_sent_before_ms: f64,
        world: &World,
        deltaOfObjectId: &mut HashMap<ObjectId, ObjectDelta>,
        curr_message_size: &mut u64,
        max_message_bytes: u64,
    ) {
        // resend formerly-unack'd objects in priority of when they were last sent, using the unacked heap

        // it is ok to pop from the heap and then re-add the new message in its place
        loop {
            // stopping conditions for popping from the heap
            if *curr_message_size >= max_message_bytes {
                break;
            }
            let Some((old_message_num, oldest_unacked_msg_time)) = time_of_unacked_message_num.get_min_key() else {
                break;
            };
            // Only resend messages old enough that their ack is considered lost. Resending a message whose ack
            // is still in flight re-numbers its entries, which invalidates that ack and livelocks the resend loop.
            if *oldest_unacked_msg_time > resend_if_sent_before_ms {
                break;
            }

            // unacked data structures to refresh as having just been sent now
            let mut unacked_addeds_to_update = Vec::new();
            let mut unacked_modifieds_to_update = Vec::new();
            let mut unacked_removeds_to_update = Vec::new();

            'message_is_done: {
                // ADD
                if let Some(existing_unacked_add_ids) = unacked.unacked_adds.get_keys(&old_message_num) {
                    for &object_id in existing_unacked_add_ids {
                        // build the delta
                        // if the object was removed since we first sent the add, what should we do here?
                        // option 1: switch to sending a remove
                        // option 2: send a dummy add so the client will ACK the add and then we can start sending modifies/removes (subsequent state is already coded for this)
                        // andrew - we go with option 2
                        let delta = match Self::buildAddDelta(object_id, world) {
                            Some(delta) => delta,
                            None => {
                                // send a dummy add delta
                                log::info!("sending dummy add delta for object {object_id} because it was removed since we first sent the add");
                                ObjectDelta::Add(ObjectAddDelta {
                                    name: "DUMMY_ADD".to_string(),
                                    objectBeingAdded: ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
                                    parentId: world.getRootObjectId(),
                                    initSubitemRelativeLocation3D: None,
                                    teleportCount: 0,
                                })
                            }
                        };

                        // check if we should stop
                        match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                            SizeCheckResult::OverLimit => break 'message_is_done,
                            SizeCheckResult::Skip => continue,
                            SizeCheckResult::Ok => {}
                        }

                        dirty.dirty_adds.remove(&object_id);

                        // mark to re-insert into unack'd
                        unacked_addeds_to_update.push(object_id);

                        // add to return val
                        deltaOfObjectId.insert(object_id, delta);
                    }
                }

                // MODIFY
                if let Some(keys) = unacked.unacked_modifies.get_keys(&old_message_num) {
                    // collect flags of object id before we can continue
                    let mut flags_of_object_id = HashMap::new();
                    for (object_id, flag_id) in keys.iter().copied() {
                        let flags = flags_of_object_id.entry(object_id).or_insert_with(ObjectModifyFlags::default);
                        match flag_id {
                            ObjectModifyFlagId::ModifiedObjectInfo => flags.modifiedObjectInfo = true,
                            ObjectModifyFlagId::ModifiedPhysicsToggles3D => flags.modifiedPhysicsToggles3D = true,
                            ObjectModifyFlagId::ModifiedPhysicsInfo3D => flags.modifiedPhysicsInfo3D = true,
                            ObjectModifyFlagId::ModifiedPrimitiveSettings => flags.modifiedPrimitiveSettings = true,
                            ObjectModifyFlagId::ModifiedParent => flags.modifiedParent = true,
                            ObjectModifyFlagId::ModifiedName => flags.modifiedName = true,
                        }
                    }

                    for (object_id, flags) in flags_of_object_id {
                        // build the delta
                        let delta = Self::buildModifyDelta(object_id, &flags, world);

                        // check if we should stop
                        match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                            SizeCheckResult::OverLimit => break 'message_is_done,
                            SizeCheckResult::Skip => continue,
                            SizeCheckResult::Ok => {}
                        }

                        // remove from dirty
                        Self::clear_dirty_modify_flags(&mut dirty.dirty_modifies, object_id, &flags);

                        // mark to re-insert into unack'd
                        unacked_modifieds_to_update.push((object_id, flags));

                        // add to return val
                        deltaOfObjectId.insert(object_id, delta);
                    }
                }

                // REMOVE
                if let Some(keys) = unacked.unacked_removes.get_keys(&old_message_num) {
                    for &object_id in keys {
                        // build the delta
                        let delta = Self::buildRemoveDelta();

                        // check if we should stop
                        match SizeCheckResult::checkMessageSize(curr_message_size, max_message_bytes, object_id, &delta) {
                            SizeCheckResult::OverLimit => break 'message_is_done,
                            SizeCheckResult::Skip => continue,
                            SizeCheckResult::Ok => {}
                        }

                        // remove from dirty
                        dirty.dirty_removes.remove(&object_id);

                        // mark to re-insert into unack'd
                        unacked_removeds_to_update.push(object_id);

                        // add to return val
                        deltaOfObjectId.insert(object_id, delta);
                    }
                }
            }

            // VERY IMPORTANT - avoid infinite loops if we made no progress this step
            let should_break = unacked_addeds_to_update.is_empty() && unacked_modifieds_to_update.is_empty() && unacked_removeds_to_update.is_empty();

            // re-insert into unack'd adds/modifies/removes
            for object_id in unacked_addeds_to_update {
                let old_message_num = unacked.unacked_adds.insert(object_id, message_num);
                Self::onUnackedChange(unacked, time_of_unacked_message_num, old_message_num, message_num);
            }
            for (object_id, flags) in unacked_modifieds_to_update {
                for flag_id in ObjectModifyFlagId::iter() {
                    if flag_id.is_in(&flags) {
                        let old_message_num = unacked.unacked_modifies.insert((object_id, flag_id), message_num);
                        Self::onUnackedChange(unacked, time_of_unacked_message_num, old_message_num, message_num);
                    }
                }
            }
            for object_id in unacked_removeds_to_update {
                let old_message_num = unacked.unacked_removes.insert(object_id, message_num);
                Self::onUnackedChange(unacked, time_of_unacked_message_num, old_message_num, message_num);
            }

            if should_break {
                break;
            }
        }
    }

    fn markObjectsAsDirtyForAllUsers(
        users: &mut HashMap<ObjectId, UserConnectionInfo>,
        mut newAddedObjectIds: HashSet<ObjectId>,
        mut newModifiedObjectFlags: HashMap<ObjectId, ObjectModifyFlags>,
        mut newForeignCharacterDTOs: HashMap<ObjectId, ServerForeignCharacterDTO>,
        mut newForeignPlayerDTOs: HashMap<ObjectId, ServerForeignPlayerDTO>,
        newRemovedObjectIds: HashSet<ObjectId>,
    ) {
        // TODO: can eventually optimize by having one shared set of objids, and each user only stores a bitset indexing it or something.

        for (user_player_object_id, user) in users.iter_mut() {
            // for each removed object, add to dirty, and cleanup dirty/unacked if we should

            for object_id in newRemovedObjectIds.iter() {
                // remove from dirty if it was removed (both modified/add)
                user.dirty.dirty_adds.remove(object_id);
                user.dirty.dirty_modifies.remove(object_id);
                user.dirty.dirty_foreign_characters.remove(object_id);
                user.dirty.dirty_foreign_players.remove(object_id);

                // do not try to modify/add this tick if it was removed
                newAddedObjectIds.remove(object_id);
                newModifiedObjectFlags.remove(object_id);
                newForeignCharacterDTOs.remove(object_id);
                newForeignPlayerDTOs.remove(object_id);

                // set this remove as dirty
                user.dirty.dirty_removes.insert(*object_id);
            }

            // for each added object
            for object_id in newAddedObjectIds.iter() {
                // don't try to modify it. adding auto-sends the whole thing (modified)
                user.dirty.dirty_modifies.remove(object_id);
                user.dirty.dirty_foreign_characters.remove(object_id);
                user.dirty.dirty_foreign_players.remove(object_id);

                // do not try to modify this tick if it was added
                newModifiedObjectFlags.remove(object_id);
                newForeignCharacterDTOs.remove(object_id);
                newForeignPlayerDTOs.remove(object_id);

                // set this add as dirty
                user.dirty.dirty_adds.insert(*object_id);
            }

            // for each foreign character. we iter thru this for each user so into_iter doesnt work
            let iter = newForeignCharacterDTOs.iter();
            for (&object_id, foreign_character_dto) in iter {
                // skip sending it back to the player who sent it
                if foreign_character_dto.came_from_player_object_id == *user_player_object_id {
                    continue;
                }
                user.dirty.dirty_foreign_characters.insert(
                    object_id,
                    ForeignCharacterDelta {
                        physicsInfo3D: foreign_character_dto.physicsInfo3D.clone(),
                        est_live_server_sim_ms: foreign_character_dto.est_live_server_sim_ms,
                        teleportCount: foreign_character_dto.teleportCount,
                    },
                );
            }

            // for each player's camera and metadata
            for (&player_object_id, foreign_player_dto) in newForeignPlayerDTOs.iter() {
                // skip sending it back to the player who sent it
                if foreign_player_dto.came_from_player_object_id == *user_player_object_id {
                    continue;
                }
                user.dirty
                    .dirty_foreign_players
                    .insert(player_object_id, foreign_player_dto.foreign_player_delta.clone());
            }

            // for each modified object
            for (&object_id, modified_flags) in newModifiedObjectFlags.iter() {
                user.dirty
                    .dirty_modifies
                    .get_or_insert_with(object_id, ObjectModifyFlags::default)
                    .merge_with(modified_flags);
            }
        }
    }

    async fn revalidateEachPlayerPlayAuth(&self) {
        // The auth lock isn't held across the request, since a joining player waits for it while holding the world lock.
        let checked_auth_info_of_player_object_id = self.auth_info_of_player_object_id.read().await.clone();

        let Ok(play_auth_hash_of_profile_id) =
            get_play_auth_hash_of_profile_id_batch(checked_auth_info_of_player_object_id.values().map(|(_, profile_id)| profile_id.as_str())).await
        else {
            log::error!("Failed revalidating play auth against supabase");
            return;
        };

        let auth_info_of_player_object_id = self.auth_info_of_player_object_id.read().await;
        let users_lock = self.userConnectionInfoOfPlayerObjectId.read().await;
        for (player_object_id, user_connection_info) in users_lock.iter() {
            const DISCONNECT_STR: &str = "you've been kicked from this game! this can happen if you join a different game or log out.";
            let disconnect_log = match auth_info_of_player_object_id.get(player_object_id) {
                // a player who joined during the request is checked next time
                Some(auth_info) if checked_auth_info_of_player_object_id.get(player_object_id) != Some(auth_info) => continue,
                Some((play_auth_hash, profile_id)) => match play_auth_hash_of_profile_id.get(profile_id) {
                    Some(current_play_auth_hash) if current_play_auth_hash == play_auth_hash => continue,
                    Some(_) => "Closing connection because user play auth changed",
                    None => "Closing connection because user no longer has a stored play auth",
                },
                None => "Closing connection because user is missing in-memory auth info",
            };

            log::info!("{}: player {}", disconnect_log, player_object_id);
            Self::closeConnection(user_connection_info.connection.clone(), DISCONNECT_STR);
        }
    }

    // ---------------------------- Start accepting connections ----------------------------

    pub async fn startAcceptingConnectionsForever(
        self: Arc<Self>,
        worldArc: Arc<RwLock<World>>,
        foreign_character_dto_tx: mpsc::UnboundedSender<(ServerForeignCharacterDTO, MessageNum)>, // with the input's latest_applied_message_num
        foreign_player_dto_tx: mpsc::UnboundedSender<ServerForeignPlayerDTO>,
        server_js_fn_request_tx: mpsc::UnboundedSender<(ObjectId, ServerFnCallMessage, oneshot::Sender<anyhow::Result<String>>)>,
        kill_server_cancel_token: CancellationToken,
        server_port: u16,
    ) -> anyhow::Result<()> {
        let bind_address: SocketAddr = format!("0.0.0.0:{server_port}").parse()?;
        log::info!("Game networking server listening on {}", bind_address);

        // Only an official server has anywhere to write to: a playtest runs on somebody's machine,
        // with no service credentials and no server id.
        if let ServerMode::Official = self.server_mode {
            tokio::spawn({
                let game_chat_arc = self.game_chat_arc.clone();
                async move { log_game_chat_messages_forever(&game_chat_arc).await }
            });
        }

        let (cert_chain, priv_key) = loadQuicCertAndKey()?;
        let mut transport = quinn::TransportConfig::default();
        transport.datagram_receive_buffer_size(Some(1024 * 1024));
        transport.datagram_send_buffer_size(1024 * 1024);
        transport.max_idle_timeout(Some(quinn::IdleTimeout::try_from(std::time::Duration::from_secs(
            SERVER_MAX_IDLE_TIMEOUT_SECS,
        ))?));

        let mut server_config = ServerConfig::with_single_cert(cert_chain, priv_key)?;
        server_config.transport = Arc::new(transport);

        let endpoint = Endpoint::server(server_config, bind_address)?;
        if let ServerMode::Playtest(auth_arc) = &self.server_mode {
            auth_arc.set_status(PlaytestServerStatus::Running);
        }

        if let ServerMode::Official = self.server_mode {
            // This prevents a player from keeping an old game session after the website has issued them a new play auth for a different game.
            tokio::spawn({
                let selfArc = self.clone();
                let kill_server_cancel_token = kill_server_cancel_token.clone();
                async move {
                    let mut poll_ticker = tokio::time::interval(std::time::Duration::from_secs(REVALIDATE_USERS_POLL_SECS));
                    poll_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        tokio::select! {
                            _ = kill_server_cancel_token.cancelled() => break,
                            _ = poll_ticker.tick() => selfArc.revalidateEachPlayerPlayAuth().await,
                        }
                    }
                }
            });
        }

        loop {
            let kill_server_cancel_token = kill_server_cancel_token.clone();
            // await for a new incoming QUIC connection request
            let incoming_handshake = tokio::select! {
                // kill signal received, break free from the loop
                _ = kill_server_cancel_token.cancelled() => {
                    log::info!("Kill signal received, shutting down networking accept loop");
                    endpoint.close(0u32.into(), b"Server stopped.");
                    endpoint.wait_idle().await;
                    return Ok(());
                }
                // get incoming connection
                incoming = endpoint.accept() => {
                    match incoming {
                        Some(incoming) => incoming,
                        // None means endpoint was closed, break free from the loop
                        None => {
                            log::info!("Networking endpoint closed, stopping accept loop");
                            return Ok(());
                        }
                    }
                }
            };

            // handle the new user connection in a new task so we can await
            let selfArc = self.clone();
            let worldArc = worldArc.clone();
            let foreign_character_dto_tx = foreign_character_dto_tx.clone();
            let foreign_player_dto_tx = foreign_player_dto_tx.clone();
            let server_js_fn_request_tx = server_js_fn_request_tx.clone();
            tokio::spawn(async move {
                // this whole block of code is a critical section that should be executed by 1 user at a time
                let new_user_mutex_lock = selfArc.new_user_mutex.lock().await;

                // await handshake or fail/stop
                let connection = match incoming_handshake.await {
                    Ok(conn) => conn,
                    Err(e) => return log::error!("Incoming QUIC handshake ended before connection establishment: {}", e),
                };
                let remote_addr = connection.remote_address();
                log::info!("Accepted QUIC connection from {}", remote_addr);

                // await TCP or fail/stop
                log::info!("Connection {}: waiting for initial bidirectional handshake stream", remote_addr);
                let (mut handshake_send_stream, mut handshake_recv_stream) = match connection.accept_bi().await {
                    Ok((send_stream, recv_stream)) => (send_stream, recv_stream),
                    Err(e) => return Self::closeConnection(connection, &format!("The game had an issue accepting your connection. {}", e)),
                };
                log::info!("Connection {}: received initial handshake stream", remote_addr);

                // wait for the client's hello message
                log::info!("Connection {}: waiting for hello message", remote_addr);
                let client_hello = match awaitTcpMessage::<ClientHandshakeTCP>(&mut handshake_recv_stream).await {
                    Ok(ClientHandshakeTCP::Hello(client_hello)) => client_hello,
                    Ok(_) => return Self::closeConnection(connection, &format!("The server expected a hello message, but got another message.")),
                    Err(e) => return Self::closeConnection(connection, &format!("The server had an issue reading your message. {}", e)),
                };
                log::info!("Connection {}: received hello message", remote_addr);

                // check for version mismatch - reject connection if the server protocol version does not match the client protocol version
                let client_protocol_version = client_hello.client_server_protocol_version;
                let server_protocol_version = CLIENT_SERVER_PROTOCOL_VERSION;
                if client_protocol_version != server_protocol_version {
                    // depending on whether client is newer (typical) or server is newer (shouldnt really happen), give different message
                    let version_mismatch_message = if client_protocol_version > server_protocol_version {
                        "Sorry, the server's code is outdated. Please try another server."
                    } else {
                        "Please restart to update your client."
                    };

                    let client_version = client_hello.client_app_version;
                    let server_version = env!("CARGO_PKG_VERSION");

                    return Self::closeConnection(
                        connection,
                        &format!("Version mismatch: your client is {client_version}, but the server is {server_version}. {version_mismatch_message}",),
                    );
                }

                // Every desktop connection must carry a website-issued play token.
                let (username, userid, profile, auth_info) = match &selfArc.server_mode {
                    ServerMode::Playtest(auth_arc) => match auth_arc.claim(&client_hello.play_auth).await {
                        Ok(player) => (player.username, player.user_id, player.profile, None),
                        Err(error) => return Self::closeConnection(connection, &error.to_string()),
                    },
                    ServerMode::Official => match validate_play_auth(&client_hello.play_auth).await {
                        Ok(Some(validated_play_auth)) => {
                            let ValidatedPlayAuth {
                                profile_id,
                                user_id,
                                username,
                                profile,
                                play_auth_hash,
                            } = validated_play_auth;
                            (username, Some(user_id), profile, Some((play_auth_hash, profile_id)))
                        }
                        Err(error) => {
                            return Self::closeConnection(connection, &format!("Invalid play auth token; please launch from the website again. {error}"));
                        }
                        Ok(None) => return Self::closeConnection(connection, "Invalid play auth token; please launch from the website again."),
                    },
                };
                log::info!("Connection {}: validated userid={:?}", remote_addr, userid);

                // if userid already exists, close connection
                // A playtest lets that account's next player in as a guest instead, since one account can run several test players.
                let world_lock = worldArc.read().await;
                let is_account_in_game = userid.is_some()
                    && world_lock._kindOfObjectId.get_keys(&ObjectKind::Player).is_some_and(|player_object_ids| {
                        player_object_ids
                            .iter()
                            .any(|player_object_id| match world_lock.getObject(*player_object_id).map(|object| &object.info) {
                                Ok(ObjectInfo::Player(player_info)) => player_info.userid == userid,
                                _ => false,
                            })
                    });
                drop(world_lock);
                let (username, userid, profile) = match (is_account_in_game, &selfArc.server_mode) {
                    (false, _) => (username, userid, profile),
                    (true, ServerMode::Playtest(_)) => ("Guest".to_string(), None, PlayerProfile::GUEST),
                    (true, ServerMode::Official) => {
                        log::warn!("Connection {}: rejecting duplicate in-game connection for userid {:?}", remote_addr, userid);
                        return Self::closeConnection(connection, "You're already in this game!");
                    }
                };

                // get the stored world string without the player in it - that caused a lot of very confusing behavior since addObject marks as dirty already
                // they get the same stored world everyone else has. updating the player later sends them and everyone else the new player object.
                log::info!("Connection {}: serializing initial world for userid {:?}", remote_addr, userid);
                // This is so players joining at the same time don't both get processed together. Objects not sent out yet would reach this player twice,
                // in their world and again from the next tick as new objects, so the join waits for a tick to send them.
                // Held until the player is registered below, or a tick in between would send its changes to everyone but them.
                let mut world_lock = loop {
                    let world_lock = worldArc.write().await;
                    // This always ends: every tick sends out everything added before it, and between ticks only a join adds an object, so each tick lets at least one waiting join in.
                    if world_lock.addedObjectIds.is_empty() {
                        break world_lock;
                    }
                    drop(world_lock);
                    tokio::time::sleep(Duration::from_secs_f32(SERVER_SIM_AND_SEND_SECS)).await;
                };
                let stored_world_string = match world_lock.toStoredWorldString() {
                    Ok(stored_world_string) => stored_world_string,
                    Err(e) => {
                        let err_str = format!("The game had an issue when gathering the world for you. {}", e);
                        Self::closeConnection(connection, &err_str);
                        return log::error!("{}", err_str);
                    }
                };
                let initialTeleportCountOfObjectId = world_lock.teleportCountOfObjectId.clone();
                let template_data_ids = world_lock.getTemplateDataIds();

                // this is detected by addObject callback and sent next to everyone including the player themself
                let root_object_id = world_lock.getRootObjectId();
                let player_object_id = world_lock.addObject(
                    username.clone(),
                    ObjectBeingAdded::Player(PlayerInfo {
                        username: username.clone(),
                        userid,
                        preferredColor: profile.preferredColor,
                        equippedItemId: profile.equippedItemId,
                        headObjectId: None,      // gets set later by script
                        characterObjectId: None, // gets set later by script
                    }),
                    root_object_id,
                );
                log::info!(
                    "Connection {}: inserted player object {:?} for userid {:?}",
                    remote_addr,
                    player_object_id,
                    userid
                );
                if let Some((play_auth_hash, profile_id)) = auth_info {
                    selfArc.insertAuthInfoOfPlayerObjectId(player_object_id, play_auth_hash, profile_id).await;
                }

                // Tell the chat the player joined. The cleanup task below tells it they left.
                let mut game_chat_lock = selfArc.game_chat_arc.lock().await;
                game_chat_lock.add_entry(GameChatEntry::PlayerJoined { username: username.clone() });
                drop(game_chat_lock);

                // add user connection metadata to this struct
                let mut users_lock = selfArc.userConnectionInfoOfPlayerObjectId.write().await;
                users_lock.insert(
                    player_object_id,
                    UserConnectionInfo {
                        connection: connection.clone(), // this is fine to clone, connections are cheap
                        snapshot_send_state: SnapshotSendState::WaitingForReady,
                        dirty: DirtyDeltaFlags {
                            dirty_adds: HashSet::new(),
                            dirty_modifies: HashMapQueue::new(),
                            dirty_foreign_characters: HashMapQueue::new(),
                            dirty_foreign_players: HashMapQueue::new(),
                            dirty_removes: HashSet::new(),
                        },
                        unacked: UnackedDeltaFlags {
                            unacked_modifies: HashMapWithReverse::new(),
                            unacked_adds: HashMapWithReverse::new(),
                            unacked_removes: HashMapWithReverse::new(),
                        },
                        time_of_unacked_message_num: HashMapHeap::new(),
                        poses_wait_for_message_num: 0,
                    },
                );
                drop(users_lock);
                drop(world_lock);
                log::info!(
                    "Connection {}: registered server-side connection metadata for player {}",
                    remote_addr,
                    player_object_id
                );

                // add to player_object_id -> connection map
                selfArc.connection_of_player_object_id_arc.insert(player_object_id, connection.clone());
                log::info!("Connection {}: inserted connection map entry for player {}", remote_addr, player_object_id);

                // Record that they joined. Sent in the background so the join doesn't wait on PostHog.
                if let ServerMode::Official = selfArc.server_mode
                    && let Some(userid) = userid
                {
                    tokio::spawn(async move {
                        let properties = serde_json::json!({ "game_id": *GAME_ID });
                        if let Err(e) = capture_posthog_event("app-game-join", userid, properties).await {
                            log::error!(
                                "Connection {}: failed to send app-game-join to posthog (player {}): {}",
                                remote_addr,
                                player_object_id,
                                e
                            );
                        }
                    });
                }

                // add a task that cleans up after a connection is closed.
                selfArc.player_cleanup_tasks_tracker.spawn({
                    let connection = connection.clone();
                    let worldArc = worldArc.clone();
                    let selfArc = selfArc.clone();
                    let username = username.clone();
                    let remote_addr = remote_addr;
                    let joined_at = Instant::now();
                    async move {
                        // await the connection to be closed to execute the code
                        tokio::select! {
                            _ = kill_server_cancel_token.cancelled() => {
                                // if we shut down the server
                                Self::closeConnection(connection, "The server was shut down.");
                                log::info!("Connection {} player {}: kill signal received, running cleanup", remote_addr, player_object_id);
                            }
                            _ = connection.closed() => {
                                // if the connection was simply closed by us somewhere else, or if the client disconnected
                                log::info!("Connection {}: transport closed for player {}", remote_addr, player_object_id);
                            }
                        }

                        // delete the connection metadata from users
                        let removed_user_info = selfArc.userConnectionInfoOfPlayerObjectId.write().await.remove(&player_object_id);
                        if removed_user_info.is_none() {
                            log::error!(
                                "Connection {}: user connection info not found in users metadata for player {}",
                                remote_addr,
                                player_object_id
                            );
                        };
                        let auth_info = selfArc.removeAuthInfo(player_object_id).await;

                        // remove the connection from the player_object_id -> connection map
                        selfArc.connection_of_player_object_id_arc.remove(&player_object_id);
                        log::info!("Connection {}: removed connection map entry for player {}", remote_addr, player_object_id);

                        // remove the player object from world
                        let mut world_lock = worldArc.write().await;
                        let removed_player_object = world_lock.removeObject(player_object_id);
                        drop(world_lock);
                        match removed_player_object {
                            Ok(()) => log::info!("Connection {}: removed player object {:?}", remote_addr, player_object_id),
                            Err(e) => log::error!("Connection {}: failed to remove player object {:?}: {}", remote_addr, player_object_id, e),
                        }
                        let mut game_chat_lock = selfArc.game_chat_arc.lock().await;
                        game_chat_lock.add_entry(GameChatEntry::PlayerLeft { username });
                        drop(game_chat_lock);

                        // Delete the player's reserved running session row in Supabase, unless they are already in another game (aka have a different play auth).
                        let Some((play_auth_hash, profile_id)) = auth_info else {
                            return;
                        };
                        if let Err(e) = end_player_session(profile_id.as_str(), play_auth_hash.as_str()).await {
                            log::error!(
                                "Connection {}: failed to delete running player session in supabase (player {}): {}",
                                remote_addr,
                                player_object_id,
                                e
                            );
                        } else {
                            log::info!(
                                "Connection {}: deleted running player session in supabase for player {}",
                                remote_addr,
                                player_object_id
                            );
                        }

                        // Record how long they played. Only official games get here, so playtests are never counted.
                        let Some(userid) = userid else {
                            return;
                        };
                        let properties = serde_json::json!({ "game_id": *GAME_ID, "duration_seconds": joined_at.elapsed().as_secs() });
                        if let Err(e) = capture_posthog_event("app-game-leave", userid, properties).await {
                            log::error!(
                                "Connection {}: failed to send app-game-leave to posthog (player {}): {}",
                                remote_addr,
                                player_object_id,
                                e
                            );
                        }
                    }
                });

                // serialize + send entire world state to the client
                log::info!("Connection {}: sending initial world to player {}", remote_addr, player_object_id);
                let initial_world_message = ServerHandshakeTCP::InitialWorld(ServerInitialWorldMessage {
                    stored_world_string,
                    live_server_wall_ms: selfArc.get_live_server_wall_ms(),
                    player_object_id,
                    teleportCountOfObjectId: initialTeleportCountOfObjectId,
                    template_data_ids,
                });
                match sendTcpMessage(&mut handshake_send_stream, &initial_world_message).await {
                    Ok(()) => log::info!("Connection {}: initial world sent to player {}", remote_addr, player_object_id),
                    Err(e) => return log::error!("Error writing initial world message: {}", e),
                }
                let _ = handshake_send_stream.finish();

                // await the user to say they are ready to start receiving snapshots
                log::info!("Connection {}: waiting for ReadyForUDPMessages from player {}", remote_addr, player_object_id);
                let ack_message = match awaitTcpMessage::<ClientHandshakeTCP>(&mut handshake_recv_stream).await {
                    Ok(ack_message) => ack_message,
                    Err(e) => {
                        return Self::closeConnection(
                            connection,
                            &format!("The server had an issue reading your message. Please try connecting again. {}", e),
                        );
                    }
                };
                let ClientHandshakeTCP::ReadyForUDPMessages = ack_message else {
                    return Self::closeConnection(connection, "The server had an issue readying you up. Please try connecting again.");
                };
                drop(handshake_recv_stream);
                log::info!("Connection {}: received ReadyForUDPMessages from player {}", remote_addr, player_object_id);
                let mut users_lock = selfArc.userConnectionInfoOfPlayerObjectId.write().await;
                let Some(user_connection_info) = users_lock.get_mut(&player_object_id) else {
                    return Self::closeConnection(
                        connection,
                        "The server lost track of your connection while readying you up. Please try connecting again.",
                    );
                };
                user_connection_info.snapshot_send_state = SnapshotSendState::Ready { next_message_num: 1 };
                drop(users_lock);
                log::info!("Connection {}: snapshot stream marked ready for player {}", remote_addr, player_object_id);

                drop(new_user_mutex_lock);
                log::info!("Connection {}: handshake complete for player {}", remote_addr, player_object_id);

                // start a task to listen to all server-function requests from the client
                // Route chat and script streams by their first message without delaying gameplay input.
                start_receiving_client_messages_tcp_in_bg(
                    connection.clone(),
                    selfArc.game_chat_arc.clone(),
                    player_object_id,
                    userid,
                    username,
                    server_js_fn_request_tx.clone(),
                );

                // now, enter loop that receives game input messages via UDP datagrams
                let mut latest_input_num: InputNum = 0;
                let mut player_pose_lag = PlayerPoseLag::new(selfArc.get_live_server_wall_ms());

                loop {
                    // wait for the next datagram from the user
                    let message = match awaitDatagramMessage::<ClientMessageUDP>(&connection).await {
                        Ok(message) => message,
                        Err(e) => {
                            if let Some(connection_error) = e.downcast_ref::<quinn::ConnectionError>() {
                                return log::info!("Player disconnected: {}, {}", player_object_id, connection_error);
                            }
                            log::error!("Error reading datagram: {}", e);
                            continue;
                        }
                    };

                    // interpet / respond to the message
                    match message {
                        ClientMessageUDP::Input(input_msg) => {
                            let ClientInputMessage {
                                latest_applied_message_num,
                                foreign_player_delta,
                                foreign_character_delta,
                                input_num,
                            } = input_msg;

                            if input_num <= latest_input_num {
                                continue;
                            } else {
                                latest_input_num = input_num;
                            }

                            // when the player simulated this input. Its camera, metadata and pose all get this one time
                            let est_live_server_sim_ms =
                                player_pose_lag.shift_stamp(foreign_player_delta.est_live_server_sim_ms, selfArc.get_live_server_wall_ms());

                            // send the player's camera and metadata to main game loop, whether or not it has a character
                            let camera_orientation = foreign_player_delta.cameraOrientation;
                            let _ = foreign_player_dto_tx.send(ServerForeignPlayerDTO {
                                came_from_player_object_id: player_object_id,
                                foreign_player_delta: ForeignPlayerDelta {
                                    cameraOrientation: if camera_orientation.theta.is_finite() && camera_orientation.phi.is_finite() {
                                        camera_orientation
                                    } else {
                                        Orientation3D::Z()
                                    },
                                    playerMetadata: foreign_player_delta.playerMetadata.filter(|json| is_valid_player_metadata(json)),
                                    est_live_server_sim_ms,
                                },
                            });

                            // send foreign character to main game loop
                            if let Some(foreign_character_delta) = foreign_character_delta {
                                // the player only ever moves its own object. it doesnt even identify the object it controls
                                let foreign_character_dto = ServerForeignCharacterDTO {
                                    came_from_player_object_id: player_object_id,
                                    est_live_server_sim_ms,

                                    physicsInfo3D: foreign_character_delta.physicsInfo3D,
                                    teleportCount: foreign_character_delta.teleportCount,
                                };
                                let _ = foreign_character_dto_tx.send((foreign_character_dto, latest_applied_message_num));
                            }
                            log::debug!("Received input from player {}", player_object_id);
                        }
                        // The player sends this the moment a snapshot arrives, so a slow play loop never holds it up.
                        ClientMessageUDP::Ack(ClientAckMessage { message_num }) => {
                            // get all needed locks
                            let mut users_of_id_lock = selfArc.userConnectionInfoOfPlayerObjectId.write().await;
                            let user = match users_of_id_lock.get_mut(&player_object_id) {
                                Some(user) => user,
                                None => return log::error!("Could not get user on user lock"),
                            };

                            // apply ACKs at the end if there was a success - simply remove from unacked
                            user.unacked.unacked_adds.remove_all_with_value(message_num);
                            user.unacked.unacked_removes.remove_all_with_value(message_num);
                            user.unacked.unacked_modifies.remove_all_with_value(message_num);
                            user.time_of_unacked_message_num.remove(&message_num);
                            drop(users_of_id_lock);
                        }
                        ClientMessageUDP::Ping(ClientPingMessage { t0_client_send_ms }) => {
                            // build pong message
                            let response = ServerMessageUDP::Pong(ServerPongMessage {
                                t0_client_send_ms,
                                t1_server_recv_ms: selfArc.get_live_server_wall_ms(),
                            });
                            // respond with pong message
                            match sendDatagramMessage(&connection, &response) {
                                Ok(()) => (),
                                Err(e) => {
                                    log::error!("Error sending pong to user: {}", e);
                                    continue;
                                }
                            }
                            log::debug!("Responded to ping from player {}: {:?}", player_object_id, t0_client_send_ms);
                        }
                    };
                }
            });
        }
    }

    fn closeConnection(connection: Connection, reason: &str) {
        log::warn!("Closing connection {}: {}", connection.remote_address(), reason);
        connection.close(quinn::VarInt::from_u32(500), reason.as_bytes());
    }
}

// ----------------- GameChat -----------------

// One room belongs to one running server. Publishing and subscribing happen under the same
// lock so a joining player sees every message once, in history or in the live stream.
pub struct ServerGameChat {
    history: VecDeque<GameChatEntry>,
    entry_tx: broadcast::Sender<GameChatEntry>,
    // Scripts are told about a message the same way physics and player join tell them about theirs.
    js_callback_inputs_tx: Option<mpsc::UnboundedSender<JsCallbackInputs>>,
}

impl ServerGameChat {
    pub fn new(js_callback_inputs_tx: Option<mpsc::UnboundedSender<JsCallbackInputs>>) -> Self {
        let (entry_tx, _) = broadcast::channel(GAME_CHAT_HISTORY_LIMIT);
        Self {
            history: VecDeque::new(),
            entry_tx,
            js_callback_inputs_tx,
        }
    }

    pub fn subscribe(&self) -> (VecDeque<GameChatEntry>, broadcast::Receiver<GameChatEntry>) {
        (self.history.clone(), self.entry_tx.subscribe())
    }

    pub fn add_entry(&mut self, entry: GameChatEntry) {
        if self.history.len() == GAME_CHAT_HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.history.push_back(entry.clone());
        let _ = self.entry_tx.send(entry);
    }

    // The authenticated connection supplies the user ID; clients supply only the text.
    // Keep its verified username with the message so history still identifies players who have left.
    pub fn submit(&mut self, userid: UserId, username: &str, message: String) -> Result<()> {
        let message = message.trim();
        ensure!(!message.is_empty(), "Chat messages cannot be empty.");
        ensure!(message.chars().count() <= GAME_CHAT_MAX_MESSAGE_CHARS, "Chat message is too long.");
        ensure!(
            !message.chars().any(char::is_control),
            "Chat messages must be a single line without control characters."
        );
        // Swear words turn into hashes here, before the message reaches history or any subscriber,
        // so there is one place to get this right instead of one per delivery path. The filter is the
        // same one the website checks usernames and game titles with, and it keeps the message the
        // same length, so the limit checked above stays true.
        let message = censor(message);
        let game_chat_message = GameChatMessage {
            time_live_server_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().try_into()?,
            userid,
            username: username.to_owned(),
            message,
        };
        if let Some(js_callback_inputs_tx) = self.js_callback_inputs_tx.as_ref() {
            let _ = js_callback_inputs_tx.send(JsCallbackInputs::GameChatMessageSent {
                userid: game_chat_message.userid,
                username: game_chat_message.username.clone(),
                message: game_chat_message.message.clone(),
                time_live_server_ms: game_chat_message.time_live_server_ms,
            });
        }
        self.add_entry(GameChatEntry::Message(game_chat_message));
        Ok(())
    }
}

// Chat is logged by reading the room like any other subscriber, so a slow or failing database write
// can never hold a message up on its way to players. Whatever arrives while a write is in flight is
// written together in the next one, because a busy server makes messages faster than it is worth
// making requests.
pub async fn log_game_chat_messages_forever(game_chat: &Mutex<ServerGameChat>) {
    let game_chat_lock = game_chat.lock().await;
    let (_, mut entry_rx) = game_chat_lock.subscribe();
    drop(game_chat_lock);

    // Only messages players typed are logged, not join and leave notices.
    loop {
        let mut game_chat_messages = match entry_rx.recv().await {
            Ok(GameChatEntry::Message(game_chat_message)) => vec![game_chat_message],
            Ok(GameChatEntry::PlayerJoined { .. } | GameChatEntry::PlayerLeft { .. }) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
            // The room keeps only its most recent messages, so a log this far behind has lost the
            // rest. Say how many rather than leaving a silent gap in the log.
            Err(broadcast::error::RecvError::Lagged(lost_message_count)) => {
                log::error!("Game chat log fell behind and lost {lost_message_count} messages");
                continue;
            }
        };
        loop {
            match entry_rx.try_recv() {
                Ok(GameChatEntry::Message(game_chat_message)) => game_chat_messages.push(game_chat_message),
                Ok(GameChatEntry::PlayerJoined { .. } | GameChatEntry::PlayerLeft { .. }) => {}
                Err(broadcast::error::TryRecvError::Lagged(lost_message_count)) => {
                    log::error!("Game chat log fell behind and lost {lost_message_count} messages");
                }
                Err(_) => break,
            }
        }
        if let Err(error) = insert_game_chats(&game_chat_messages).await {
            log::error!("Failed writing game chat to the log: {error}");
        }
    }
}

// Each connection reads and writes independently: a slow receiver never holds the room lock
// or stalls other players. An error ends only this chat stream.
// This subscription only sends history and broadcasts; submissions arrive as separate requests.
pub async fn send_game_chat_messages(game_chat: &Mutex<ServerGameChat>, mut send_stream: SendStream) -> Result<()> {
    let game_chat_lock = game_chat.lock().await;
    let (history, mut entry_rx) = game_chat_lock.subscribe();
    drop(game_chat_lock);

    let stopped = send_stream.stopped();
    let send_messages = async {
        sendTcpMessage(&mut send_stream, &ServerGameChatMessage::History(history)).await?;
        loop {
            let entry = entry_rx.recv().await?;
            sendTcpMessage(&mut send_stream, &ServerGameChatMessage::Entry(entry)).await?;
        }
    };
    tokio::select! {
        result = send_messages => result,
        result = stopped => {
            result?;
            Ok(())
        },
    }
}

// ----------------- Reliable client messages -----------------

// Called after authentication. The first message selects the handler for each independent stream;
// a stalled or failed stream cannot stop accepting other streams or close the connection.
pub fn start_receiving_client_messages_tcp_in_bg(
    connection: Connection,
    game_chat_arc: Arc<Mutex<ServerGameChat>>,
    player_object_id: ObjectId,
    userid: Option<UserId>,
    username: String,
    server_js_fn_request_tx: mpsc::UnboundedSender<(ObjectId, ServerFnCallMessage, oneshot::Sender<anyhow::Result<String>>)>,
) {
    tokio::spawn(async move {
        // A player gets one active chat subscription, even if they open additional streams.
        let game_chat_slot_arc = Arc::new(Semaphore::new(1));
        // Keep the submission limit per connection, even when requests use different streams.
        let next_game_chat_submission_arc = Arc::new(Mutex::new(tokio::time::Instant::now()));
        loop {
            // wait for the next incoming server-function stream
            let (mut send_stream, mut recv_stream) = match connection.accept_bi().await {
                Ok(stream) => stream,
                Err(error) => {
                    log::info!("Client stream loop stopped for player {}: {}", player_object_id, error);
                    return;
                }
            };

            tokio::spawn({
                let connection = connection.clone();
                let game_chat_arc = game_chat_arc.clone();
                let game_chat_slot_arc = game_chat_slot_arc.clone();
                let next_game_chat_submission_arc = next_game_chat_submission_arc.clone();
                let username = username.clone();
                let server_js_fn_request_tx = server_js_fn_request_tx.clone();
                async move {
                    let handle_stream = async {
                        let message = awaitTcpMessageMaxBytes::<ClientMessageTCP>(&mut recv_stream, MAX_TCP_MESSAGE_BYTES).await?;
                        drop(recv_stream);
                        match message {
                            ClientMessageTCP::SubscribeToGameChat => {
                                let _game_chat_permit = game_chat_slot_arc.try_acquire()?;
                                return send_game_chat_messages(&game_chat_arc, send_stream).await;
                            }
                            ClientMessageTCP::ServerFnCall(server_function_call_message) => {
                                // wait for the next incoming server-function call message

                                // send the server-function call to the js runtime to run
                                let (server_js_fn_oneshot_tx, server_js_fn_oneshot_rx) = oneshot::channel::<anyhow::Result<String>>();
                                server_js_fn_request_tx.send((player_object_id, server_function_call_message, server_js_fn_oneshot_tx))?;

                                // wait for the js runtime result
                                let result = server_js_fn_oneshot_rx.await?.map_err(|error| error.to_string());

                                // send the server-function result back to the client
                                sendTcpMessage(&mut send_stream, &ServerFunctionResultMessage { result }).await?;
                            }
                            ClientMessageTCP::GameChat(ClientGameChatMessage { message }) => {
                                let result = match userid {
                                    None => Err("Only signed-in players can chat.".to_string()),
                                    Some(userid) => {
                                        let mut next_game_chat_submission_lock = next_game_chat_submission_arc.lock().await;
                                        tokio::time::sleep_until(*next_game_chat_submission_lock).await;
                                        *next_game_chat_submission_lock = tokio::time::Instant::now() + GAME_CHAT_SUBMISSION_INTERVAL;
                                        drop(next_game_chat_submission_lock);

                                        let mut game_chat_lock = game_chat_arc.lock().await;
                                        let result = game_chat_lock.submit(userid, &username, message).map_err(|error| error.to_string());
                                        drop(game_chat_lock);
                                        result
                                    }
                                };
                                sendTcpMessage(&mut send_stream, &ServerGameChatResultMessage { result }).await?;
                            }
                        }
                        send_stream.finish()?;
                        Ok::<(), anyhow::Error>(())
                    };
                    let result = tokio::select! {
                        _ = connection.closed() => return,
                        result = handle_stream => result,
                    };
                    if let Err(error) = result {
                        log::warn!("Client stream stopped for player {}: {}", player_object_id, error);
                    }
                }
            });
        }
    });
}

// The desktop client already trusts the bundled repo cert, so the server must use that same identity.
fn loadQuicCertAndKey() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_chain = vec![CertificateDer::from(QUIC_SERVER_CERT_DER_BYTES)];
    let key_der_bytes = BASE64_STANDARD.decode(QUIC_SERVER_KEY_DER_BASE64.as_str())?;
    let priv_key = PrivateKeyDer::try_from(key_der_bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("parse QUIC_SERVER_KEY_DER_BASE64 private key der: {error}")))?;
    Ok((cert_chain, priv_key))
}

// ----------------- Helpers -----------------

// // we draw a radius around the object and use that as a rough distance score.
// fn getDistanceScore(user_position: Vec3, world: &World, object_id: ObjectId) -> Option<f32> {
//     pub fn getBoundingRadiusSquared(object: &LiveObject) -> Option<f32> {
//         match &object.info {
//             ObjectInfo::Ball(ball) => Some(ball.radius * ball.radius),
//             ObjectInfo::Brick(brick) => Some(0.25 * brick.size.length_squared()),
//             // TODO: better bound
//             ObjectInfo::RigidGroup(_) => Some(1.0),

//             // TODO: improve these if relevant
//             ObjectInfo::Mesh(_)
//             | ObjectInfo::Player(_)
//             | ObjectInfo::Clock(_)
//             | ObjectInfo::LocalGroup(_)
//             | ObjectInfo::Script(_)
//             | ObjectInfo::SkAnimator(_)
//             | ObjectInfo::SoundPlayer(_) => None,
//         }
//     }
//     fn getScore(p: Vec3, c: Vec3, r2: f32) -> f32 {
//         // note that Distance from point p to sphere at center c with radius r is max(0, |p-c| - r).
//         // score is not Distance^2 here, instead it's |p-c|^2 - r^2, a rough proxy for it that monotonically increases outside the sphere.
//         let d2 = (p - c).length_squared();
//         return f32::max(0.0, d2 - r2);
//     }
//     let obj = world.getObject(object_id).ok()?;
//     let physicsInfo3D = world.getObjectPhysicsInfo3D(object_id)?;
//     let r2 = getBoundingRadiusSquared(&obj)?;
//     Some(getScore(user_position, physicsInfo3D.location.position, r2))
// }

#[cfg(test)]
mod tests {
    use super::*;
    use game_types::consts::SERVER_SIM_AND_SEND_SECS;
    use shared::game::world::WorldControlMode;

    // any world will do as a root to hang objects off; this one is small and already in the repo
    const SMALL_WORLD: &str = include_str!("../../../game-pigball.kube");
    // the most a QUIC datagram can carry before the connection discovers it can send bigger packets
    const STARTING_MAX_MESSAGE_BYTES: u64 = 1162;

    // Drives the real send loop for one client: resends first, then fresh dirty adds, with acks modelled
    // as arriving a full round trip after the message that carried them - which is when they really arrive,
    // since a client acks on its next input. Returns how many distinct objects the client actually got.
    fn simulate_snapshot_loop(pending_object_count: usize, round_trip_ms: f32, message_count: usize) -> (usize, usize) {
        let mut world = World::fromStoredWorldString(SMALL_WORLD, WorldControlMode::Server, None).unwrap();
        let root_object_id = world.getRootObjectId();

        let mut dirty = DirtyDeltaFlags {
            dirty_adds: (0..pending_object_count)
                .map(|object_index| {
                    world.addObject(
                        format!("pending-{object_index}"),
                        ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
                        root_object_id,
                    )
                })
                .collect(),
            dirty_modifies: HashMapQueue::new(),
            dirty_foreign_characters: HashMapQueue::new(),
            dirty_foreign_players: HashMapQueue::new(),
            dirty_removes: HashSet::new(),
        };
        let mut unacked = UnackedDeltaFlags {
            unacked_modifies: HashMapWithReverse::new(),
            unacked_adds: HashMapWithReverse::new(),
            unacked_removes: HashMapWithReverse::new(),
        };
        let mut time_of_unacked_message_num = HashMapHeap::new();

        let mut received_by_client: HashSet<ObjectId> = HashSet::new();
        let mut in_flight: Vec<(MessageNum, f64)> = Vec::new();
        let mut total_deltas_sent = 0usize;

        for message_index in 0..message_count {
            let message_num = message_index as MessageNum + 1;
            let now_ms = message_index as f64 * SERVER_SIM_AND_SEND_SECS as f64 * 1000.0;

            let acked: Vec<MessageNum> = in_flight
                .iter()
                .filter(|(_, sent_at_ms)| now_ms - sent_at_ms >= round_trip_ms as f64)
                .map(|(acked_message_num, _)| *acked_message_num)
                .collect();
            in_flight.retain(|(_, sent_at_ms)| now_ms - sent_at_ms < round_trip_ms as f64);
            for acked_message_num in acked {
                unacked.unacked_adds.remove_all_with_value(acked_message_num);
                unacked.unacked_removes.remove_all_with_value(acked_message_num);
                unacked.unacked_modifies.remove_all_with_value(acked_message_num);
                time_of_unacked_message_num.remove(&acked_message_num);
            }

            let resend_if_sent_before_ms = now_ms - (JITTER_MARGIN_MS + round_trip_ms) as f64;
            let mut deltaOfObjectId = HashMap::new();
            let mut curr_message_size = empty_snapshot_size(message_num, now_ms);
            Server::getAndRefreshNewDeltasFromUnacked(
                &mut dirty,
                &mut unacked,
                &mut time_of_unacked_message_num,
                message_num,
                resend_if_sent_before_ms,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                STARTING_MAX_MESSAGE_BYTES,
            );
            Server::getAndRemoveNewDeltasFromDirty(
                &mut dirty,
                &mut unacked,
                &mut time_of_unacked_message_num,
                message_num,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                STARTING_MAX_MESSAGE_BYTES,
            );

            total_deltas_sent += deltaOfObjectId.len();
            received_by_client.extend(deltaOfObjectId.keys().copied());
            let did_send_reliable_delta = unacked.unacked_adds.contains_value(&message_num)
                || unacked.unacked_modifies.contains_value(&message_num)
                || unacked.unacked_removes.contains_value(&message_num);
            if did_send_reliable_delta {
                time_of_unacked_message_num.insert(message_num, now_ms);
                in_flight.push((message_num, now_ms));
            }
        }
        (received_by_client.len(), total_deltas_sent)
    }

    // Same loop, but an object is added to dirty partway through - the way a player's character is created
    // by the game's script a moment after they join. Reports whether that late object ever reached the
    // client, plus a per-second trace of how the packet budget was actually spent.
    fn simulate_late_object(pending_object_count: usize, round_trip_ms: f32, message_count: usize, add_at_message: usize, trace: bool) -> bool {
        let mut world = World::fromStoredWorldString(SMALL_WORLD, WorldControlMode::Server, None).unwrap();
        let root_object_id = world.getRootObjectId();
        let mut dirty = DirtyDeltaFlags {
            dirty_adds: (0..pending_object_count)
                .map(|i| world.addObject(format!("pending-{i}"), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root_object_id))
                .collect(),
            dirty_modifies: HashMapQueue::new(),
            dirty_foreign_characters: HashMapQueue::new(),
            dirty_foreign_players: HashMapQueue::new(),
            dirty_removes: HashSet::new(),
        };
        let late_object_id = world.addObject("char_body".to_string(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root_object_id);
        let mut unacked = UnackedDeltaFlags {
            unacked_modifies: HashMapWithReverse::new(),
            unacked_adds: HashMapWithReverse::new(),
            unacked_removes: HashMapWithReverse::new(),
        };
        let mut time_of_unacked_message_num = HashMapHeap::new();
        let mut received: HashSet<ObjectId> = HashSet::new();
        let mut in_flight: Vec<(MessageNum, f64)> = Vec::new();
        let (mut resent_this_second, mut fresh_this_second, mut acks_that_matched) = (0usize, 0usize, 0usize);

        for message_index in 0..message_count {
            let message_num = message_index as MessageNum + 1;
            let now_ms = message_index as f64 * SERVER_SIM_AND_SEND_SECS as f64 * 1000.0;
            if message_index == add_at_message {
                dirty.dirty_adds.insert(late_object_id);
            }
            let acked: Vec<MessageNum> = in_flight.iter().filter(|(_, t)| now_ms - t >= round_trip_ms as f64).map(|(n, _)| *n).collect();
            in_flight.retain(|(_, t)| now_ms - t < round_trip_ms as f64);
            for n in acked {
                if time_of_unacked_message_num.remove(&n).is_some() {
                    acks_that_matched += 1;
                }
                unacked.unacked_adds.remove_all_with_value(n);
                unacked.unacked_removes.remove_all_with_value(n);
                unacked.unacked_modifies.remove_all_with_value(n);
            }
            let resend_if_sent_before_ms = now_ms - (JITTER_MARGIN_MS + round_trip_ms) as f64;
            let mut deltaOfObjectId = HashMap::new();
            let mut curr_message_size = empty_snapshot_size(message_num, now_ms);
            Server::getAndRefreshNewDeltasFromUnacked(
                &mut dirty,
                &mut unacked,
                &mut time_of_unacked_message_num,
                message_num,
                resend_if_sent_before_ms,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                STARTING_MAX_MESSAGE_BYTES,
            );
            let resent = deltaOfObjectId.len();
            Server::getAndRemoveNewDeltasFromDirty(
                &mut dirty,
                &mut unacked,
                &mut time_of_unacked_message_num,
                message_num,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                STARTING_MAX_MESSAGE_BYTES,
            );
            resent_this_second += resent;
            fresh_this_second += deltaOfObjectId.len() - resent;
            received.extend(deltaOfObjectId.keys().copied());
            if unacked.unacked_adds.contains_value(&message_num)
                || unacked.unacked_modifies.contains_value(&message_num)
                || unacked.unacked_removes.contains_value(&message_num)
            {
                time_of_unacked_message_num.insert(message_num, now_ms);
                in_flight.push((message_num, now_ms));
            }
            if trace && (message_index + 1) % 60 == 0 {
                println!(
                    "    second {:>2}: {fresh_this_second:>4} new objects sent, {resent_this_second:>4} resends, {acks_that_matched:>3} acks matched something, still pending {}",
                    (message_index + 1) / 60,
                    dirty.dirty_adds.len()
                );
                resent_this_second = 0;
                fresh_this_second = 0;
                acks_that_matched = 0;
            }
        }
        received.contains(&late_object_id)
    }

    // A player who joins while a world is still being built has a few thousand objects queued as dirty
    // adds, their own character among them. Getting through that backlog must not depend on how far away
    // they are: the resend guard has to leave a message alone until its ack could no longer be in flight,
    // and an ack takes a full round trip. Waiting only the one way ping put every message on a connection
    // over ~100ms into permanent resend, and since resends are filled before fresh deltas the client
    // stopped receiving new objects for good - stranded partway through the world with no body.
    // The reported failure exactly: a character created a moment after the join, behind a world-build
    // backlog. It is not that the character is late in the queue - once the resend loop owns the whole
    // packet the dirty queue stops being serviced at all, so it is never sent even once.
    #[test]
    fn a_character_created_after_a_busy_join_still_reaches_its_player() {
        const PENDING: usize = 1956;
        const MESSAGES: usize = 30 * 60;
        const ADD_AT: usize = 60; // one second after the join, like the game script does

        for round_trip_ms in [40.0f32, 160.0] {
            let arrived = simulate_late_object(PENDING, round_trip_ms, MESSAGES, ADD_AT, false);
            assert!(
                arrived,
                "a character created after a busy join must still reach its player at {round_trip_ms}ms RTT"
            );
        }
    }

    #[test]
    fn a_client_receives_a_large_backlog_however_far_away_it_is() {
        // roughly what one real game queues for someone who joins while its world build is still running
        const PENDING: usize = 1956;
        const MESSAGES: usize = 30 * 60; // 30 seconds, far longer than anyone waits

        for round_trip_ms in [20.0f32, 60.0, 120.0, 160.0, 200.0, 400.0] {
            let (received, deltas_sent) = simulate_snapshot_loop(PENDING, round_trip_ms, MESSAGES);
            println!("  RTT {round_trip_ms:>6.0}ms -> {received:>5}/{PENDING} objects received, {deltas_sent:>6} deltas sent");
            assert_eq!(received, PENDING, "a client at {round_trip_ms}ms RTT must still receive the whole backlog");
            // every wasted resend is packet space a new object did not get
            assert!(
                deltas_sent < PENDING * 3,
                "resending {deltas_sent} times for {PENDING} objects at {round_trip_ms}ms RTT means the resend guard is firing too early"
            );
        }
    }

    #[test]
    fn characters_take_turns_when_they_dont_all_fit_in_a_snapshot() {
        const CHARACTERS: usize = 40;
        const TICKS: usize = 60;
        let world = World::fromStoredWorldString(SMALL_WORLD, WorldControlMode::Server, None).unwrap();
        let physicsInfo3D = world.objectOfId.keys().find_map(|&object_id| world.getObjectPhysicsInfo3D(object_id)).unwrap();
        let character_object_ids: Vec<ObjectId> = (0..CHARACTERS).map(|index| 1000 + index * 7).collect();
        let mut dirty = DirtyDeltaFlags {
            dirty_adds: HashSet::new(),
            dirty_modifies: HashMapQueue::new(),
            dirty_foreign_characters: HashMapQueue::new(),
            dirty_foreign_players: HashMapQueue::new(),
            dirty_removes: HashSet::new(),
        };
        let mut unacked = UnackedDeltaFlags {
            unacked_modifies: HashMapWithReverse::new(),
            unacked_adds: HashMapWithReverse::new(),
            unacked_removes: HashMapWithReverse::new(),
        };
        let mut time_of_unacked_message_num = HashMapHeap::new();

        let mut fit_in_one_snapshot = 0;
        let mut ticks_waiting_of_object_id: HashMap<ObjectId, usize> = character_object_ids.iter().map(|&object_id| (object_id, 0)).collect();
        let mut longest_wait = 0;
        for tick in 0..TICKS {
            for &object_id in &character_object_ids {
                dirty.dirty_foreign_characters.insert(
                    object_id,
                    ForeignCharacterDelta {
                        physicsInfo3D: physicsInfo3D.clone(),
                        est_live_server_sim_ms: 0.0,
                        teleportCount: 0,
                    },
                );
            }
            let message_num = tick as MessageNum + 1;
            let mut deltaOfObjectId = HashMap::new();
            let mut curr_message_size = empty_snapshot_size(message_num, 0.0);
            Server::getAndRemoveNewDeltasFromDirty(
                &mut dirty,
                &mut unacked,
                &mut time_of_unacked_message_num,
                message_num,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                STARTING_MAX_MESSAGE_BYTES,
            );
            fit_in_one_snapshot = deltaOfObjectId.len();
            for (object_id, ticks_waiting) in ticks_waiting_of_object_id.iter_mut() {
                *ticks_waiting = if deltaOfObjectId.contains_key(object_id) { 0 } else { *ticks_waiting + 1 };
                longest_wait = longest_wait.max(*ticks_waiting);
            }
        }
        assert!(fit_in_one_snapshot < CHARACTERS, "the characters must not all fit in one snapshot");
        assert!(
            longest_wait < CHARACTERS.div_ceil(fit_in_one_snapshot),
            "with {fit_in_one_snapshot} of {CHARACTERS} characters fitting per snapshot, one waited {longest_wait} snapshots in a row"
        );
    }

    // Many small changes, then a few big ones. With big ones there's no slack, so every byte has to be counted.
    #[test]
    fn a_full_snapshot_fits_in_one_datagram() {
        for name_length in [150, 580] {
            let mut world = World::fromStoredWorldString(SMALL_WORLD, WorldControlMode::Server, None).unwrap();
            let root_object_id = world.getRootObjectId();
            let long_name = "x".repeat(name_length);
            let mut dirty = DirtyDeltaFlags {
                dirty_adds: (0..20)
                    .map(|index| world.addObject(format!("{long_name}{index}"), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root_object_id))
                    .collect(),
                dirty_modifies: HashMapQueue::new(),
                dirty_foreign_characters: HashMapQueue::new(),
                dirty_foreign_players: HashMapQueue::new(),
                dirty_removes: HashSet::new(),
            };
            let mut unacked = UnackedDeltaFlags {
                unacked_modifies: HashMapWithReverse::new(),
                unacked_adds: HashMapWithReverse::new(),
                unacked_removes: HashMapWithReverse::new(),
            };
            let mut time_of_unacked_message_num = HashMapHeap::new();
            let message_num: MessageNum = 70_000; // late in a session, so the number takes 5 bytes

            let mut deltaOfObjectId = HashMap::new();
            let mut curr_message_size = empty_snapshot_size(message_num, 0.0);
            Server::getAndRemoveNewDeltasFromDirty(
                &mut dirty,
                &mut unacked,
                &mut time_of_unacked_message_num,
                message_num,
                &world,
                &mut deltaOfObjectId,
                &mut curr_message_size,
                STARTING_MAX_MESSAGE_BYTES,
            );

            let snapshot = ServerMessageUDP::Snapshot(ServerSnapshotMessage {
                message_num,
                live_server_sim_ms: 0.0,
                deltaOfObjectId,
                deltaOfObjectId2: HashMap::new(),
            });
            let snapshot_bytes = get_bincode_config().serialize(&snapshot).unwrap().len() as u64;
            assert!(
                snapshot_bytes <= STARTING_MAX_MESSAGE_BYTES,
                "a {snapshot_bytes} byte snapshot of {name_length} byte names doesn't fit in a {STARTING_MAX_MESSAGE_BYTES} byte datagram"
            );
            assert!(
                snapshot_bytes > STARTING_MAX_MESSAGE_BYTES - (name_length as u64 + 20),
                "a {snapshot_bytes} byte snapshot of {name_length} byte names had room for another change"
            );
        }
    }

    #[test]
    fn a_players_camera_and_metadata_go_out_on_their_own() {
        let world = World::fromStoredWorldString(SMALL_WORLD, WorldControlMode::Server, None).unwrap();
        let player_object_id: ObjectId = 5000;
        let mut dirty = DirtyDeltaFlags {
            dirty_adds: HashSet::new(),
            dirty_modifies: HashMapQueue::new(),
            dirty_foreign_characters: HashMapQueue::new(),
            dirty_foreign_players: HashMapQueue::new(),
            dirty_removes: HashSet::new(),
        };
        dirty.dirty_foreign_players.insert(
            player_object_id,
            ForeignPlayerDelta {
                cameraOrientation: Orientation3D::Z(),
                playerMetadata: Some(r#""waving""#.to_string()),
                est_live_server_sim_ms: 0.0,
            },
        );
        let mut unacked = UnackedDeltaFlags {
            unacked_modifies: HashMapWithReverse::new(),
            unacked_adds: HashMapWithReverse::new(),
            unacked_removes: HashMapWithReverse::new(),
        };
        let mut time_of_unacked_message_num = HashMapHeap::new();

        let mut deltaOfObjectId = HashMap::new();
        let mut curr_message_size = empty_snapshot_size(1, 0.0);
        Server::getAndRemoveNewDeltasFromDirty(
            &mut dirty,
            &mut unacked,
            &mut time_of_unacked_message_num,
            1,
            &world,
            &mut deltaOfObjectId,
            &mut curr_message_size,
            STARTING_MAX_MESSAGE_BYTES,
        );

        let Some(ObjectDelta::ForeignPlayer(sent)) = deltaOfObjectId.get(&player_object_id) else {
            panic!("the player's camera and metadata should go out filed under the player");
        };
        assert_eq!(sent.playerMetadata.as_deref(), Some(r#""waving""#));
        assert_eq!(dirty.dirty_foreign_players.len(), 0, "a sent camera and metadata leave the queue");
    }
}
