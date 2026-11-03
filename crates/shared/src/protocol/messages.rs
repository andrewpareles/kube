use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

use game_types::data_types::DataIds;
pub use game_types::object_delta::{
    ForeignCharacterDelta, ForeignPlayerDelta, NetworkedCoords, NetworkedPhysicsInfo3D, ObjectAddDelta, ObjectDelta, ObjectModifyDelta, ObjectRemoveDelta,
};
use game_types::object_types::{ObjectId, player::UserId};

pub type MessageNum = u64; // message numbers created by the server
pub type InputNum = u64; // input numbers created by the client

// ----------------- Server Actions -----------------
// call info
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerFnCallMessage {
    pub fn_id: String,
    pub js_args: String, // serialized JSON of the array of args
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientFunctionCallMessage {
    pub fn_id: String,
    pub js_args: String, // serialized JSON of the array of args
}

// results
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerFunctionResultMessage {
    pub result: Result<String, String>, // serialized JSON of the result or the thrown error
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientFunctionResultMessage {
    pub result: Result<String, String>, // serialized JSON of the result or the thrown error
}

// ----------------- Client -> Server Messages -----------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientInputMessage {
    pub input_num: InputNum, // Client's input sequence number, named differently from message_num for clarity
    // the newest snapshot whose changes to this player's own objects were applied before this input's pose was made.
    // Message numbers start at 1, so 0 means none yet
    pub latest_applied_message_num: MessageNum,

    pub foreign_player_delta: ForeignPlayerDelta,
    pub foreign_character_delta: Option<ForeignCharacterDelta>, // only with a character
}

// Sent the moment a snapshot arrives, before the play loop takes it in, so the server's re-send timer never waits on a slow tick
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientAckMessage {
    pub message_num: MessageNum,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientPingMessage {
    pub t0_client_send_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientHelloMessage {
    pub client_server_protocol_version: u64,
    pub client_app_version: String,
    pub play_auth: String,
}

//  ------------ Main Client -> Server structs  ------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientHandshakeTCP {
    Hello(ClientHelloMessage),
    ReadyForUDPMessages,
}

// Reliable messages carry their contents together. GameChat and each script call use separate streams.
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessageTCP {
    SubscribeToGameChat,
    GameChat(ClientGameChatMessage),
    ServerFnCall(ServerFnCallMessage), // 'use server'
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMessageUDP {
    Input(ClientInputMessage),
    Ack(ClientAckMessage),
    Ping(ClientPingMessage),
}

// ----------------- Server -> Client Messages -----------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerInitialWorldMessage {
    pub stored_world_string: String,
    pub live_server_wall_ms: f64,   // Server time in milliseconds since game start
    pub player_object_id: ObjectId, // The player object that belongs to this client
    pub teleportCountOfObjectId: HashMap<ObjectId, u64>,
    pub template_data_ids: DataIds, // the data in the server's templates, which are never sent to players
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerScriptRawCodeMessage {
    pub object_id: ObjectId,
    pub raw_code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerPongMessage {
    pub t0_client_send_ms: f64,
    pub t1_server_recv_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerSnapshotMessage {
    pub message_num: MessageNum,
    pub live_server_sim_ms: f64, // server time in milliseconds since game start
    pub deltaOfObjectId: HashMap<ObjectId, ObjectDelta>,
    pub deltaOfObjectId2: HashMap<ObjectId, ObjectModifyDelta>, // same as above but apply immediately
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerPlayerPingsMessage {
    pub pingMsOfPlayerObjectId: HashMap<ObjectId, u16>, // every player's round trip to the server, as the server measured it
}

//  ------------ Main Server -> Client structs  ------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerHandshakeTCP {
    InitialWorld(ServerInitialWorldMessage),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerMessageUDP {
    Snapshot(ServerSnapshotMessage),
    Pong(ServerPongMessage),
    PlayerPings(ServerPlayerPingsMessage), // once a second
}

// ----------------- GameChat -----------------

// GameChat names identify messages between players; Chat names are reserved for AI conversations.

// GameChat uses its own bidirectional stream on the authenticated connection.
// It stays ordered and reliable without participating in world snapshots or script calls.
pub const GAME_CHAT_MAX_MESSAGE_CHARS: usize = 500; // TODO: AI-PICKED-VALUE: Limit each chat message to 500 Unicode characters.
pub const GAME_CHAT_HISTORY_LIMIT: usize = 100; // TODO: AI-PICKED-VALUE: Keep the latest 100 messages in memory on each server and client.

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientGameChatMessage {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerGameChatResultMessage {
    pub result: Result<(), String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameChatMessage {
    pub time_live_server_ms: u64, // Unix milliseconds assigned by the server, independent of simulation time.
    pub userid: UserId,
    pub username: String,
    pub message: String,
}

// The server adds join and leave notices to the room alongside player messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameChatEntry {
    Message(GameChatMessage),
    PlayerJoined { username: String },
    PlayerLeft { username: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerGameChatMessage {
    History(VecDeque<GameChatEntry>),
    Entry(GameChatEntry),
}

// ----------------- Helpers for keeping track of deltas and dirty objects -----------------
// ------- server side -------
// just used for tracking dirty objects internally on the server, never sent across the network
#[derive(Default, Clone)]
pub struct ObjectModifyFlags {
    pub modifiedObjectInfo: bool,       // TODO: track exact leaf fields changed (but not values)
    pub modifiedPhysicsToggles3D: bool, // TODO: track exact leaf fields changed (but not values)
    pub modifiedPhysicsInfo3D: bool,
    pub modifiedPrimitiveSettings: bool,
    pub modifiedParent: bool,
    pub modifiedName: bool,
}

impl ObjectModifyFlags {
    pub fn from_only_object_info() -> Self {
        let mut flags = Self::default();
        flags.modifiedObjectInfo = true;
        flags
    }
    pub fn from_only_physics_info3D() -> Self {
        let mut flags = Self::default();
        flags.modifiedPhysicsInfo3D = true;
        flags
    }

    pub fn merge_with(&mut self, other: &ObjectModifyFlags) {
        self.modifiedObjectInfo |= other.modifiedObjectInfo;
        self.modifiedPrimitiveSettings |= other.modifiedPrimitiveSettings;
        self.modifiedPhysicsToggles3D |= other.modifiedPhysicsToggles3D;
        self.modifiedPhysicsInfo3D |= other.modifiedPhysicsInfo3D;
        self.modifiedParent |= other.modifiedParent;
        self.modifiedName |= other.modifiedName;
    }

    pub fn is_empty(&self) -> bool {
        !self.modifiedObjectInfo
            && !self.modifiedPrimitiveSettings
            && !self.modifiedPhysicsToggles3D
            && !self.modifiedPhysicsInfo3D
            && !self.modifiedParent
            && !self.modifiedName
    }
}

// useful helpers for flags
pub fn someIffTrue<T>(bool: bool, value: impl FnOnce() -> T) -> Option<T> {
    if bool { Some(value()) } else { None }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, EnumIter)]
pub enum ObjectModifyFlagId {
    ModifiedObjectInfo,
    ModifiedPrimitiveSettings,
    ModifiedPhysicsToggles3D,
    ModifiedPhysicsInfo3D,
    ModifiedParent,
    ModifiedName,
}
impl ObjectModifyFlagId {
    pub fn iter() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
    }
    pub fn is_in(&self, flags: &ObjectModifyFlags) -> bool {
        match self {
            ObjectModifyFlagId::ModifiedObjectInfo => flags.modifiedObjectInfo,
            ObjectModifyFlagId::ModifiedPrimitiveSettings => flags.modifiedPrimitiveSettings,
            ObjectModifyFlagId::ModifiedPhysicsToggles3D => flags.modifiedPhysicsToggles3D,
            ObjectModifyFlagId::ModifiedPhysicsInfo3D => flags.modifiedPhysicsInfo3D,
            ObjectModifyFlagId::ModifiedParent => flags.modifiedParent,
            ObjectModifyFlagId::ModifiedName => flags.modifiedName,
        }
    }
}
