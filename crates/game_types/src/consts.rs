//! Defaults and serialized format versions used on every platform.

use crate::object_types::Orientation3D;

// ---------------------- networking ----------------------

pub const SERVER_SIM_AND_SEND_SECS: f32 = 1.0 / 60.0;
pub const CLIENT_SIM_AND_SEND_SECS: f32 = 1.0 / 60.0; // we should probably simulate at the server's tick rate
pub const SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK: u64 = 4;
pub const CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK: u64 = 4; // client catch-up must run the character controller inside each fixed step

pub const SERVER_MAX_IDLE_TIMEOUT_SECS: u64 = 5; // time before connection closes
pub const CLIENT_MAX_IDLE_TIMEOUT_SECS: u64 = 12 * 60 * 60; // time before connection closes - let the client wait for the server for a long time, a few hours

pub const JITTER_MARGIN_MS: f32 = 50.0; // margin of error to account for jitter, may be used on server too

pub const MAX_PLAYER_METADATA_BYTES: usize = 256; // largest JSON a client can share with setLocalPlayerMetadata

// how far back the server remembers each player's poses and metadata, for getCharacterLocationAtTime and getPlayerMetadataAtTime
pub const SERVER_PLAYER_HISTORY_MS: f32 = 1000.0;

// how often the server sends every player everyone's ping
pub const SERVER_PLAYER_PINGS_INTERVAL_MS: f32 = 1000.0;

// ---------------------- client side variables ----------------------
// the adaptive client world delay itself lives in client_world_delay::ClientWorldDelay

// desired = base + ping
pub const CLIENT_WORLD_DELAY_EXTRA_LAG_MS: f32 = 33.0; // so other players' poses can arrive this much later than usual and still be in time
pub const CLIENT_WORLD_DELAY_BASE_MS: f32 =
    f32::max(SERVER_SIM_AND_SEND_SECS, CLIENT_SIM_AND_SEND_SECS) * 1000.0 + JITTER_MARGIN_MS + CLIENT_WORLD_DELAY_EXTRA_LAG_MS;
// how late a player's poses can reach the server before watchers are told to play that player further back (by the excess).
// Three ticks of the base delay stay free: the server's wait until its next send, the gap to the next pose, and jitter on the way to watchers
pub const MAX_UNSHIFTED_POSE_LAG_MS: f32 = CLIENT_WORLD_DELAY_BASE_MS - 3.0 * f32::max(SERVER_SIM_AND_SEND_SECS, CLIENT_SIM_AND_SEND_SECS) * 1000.0;
// one absurd timestamp from a client inflates that player’s lag for the rest of their session if we don't do this
// we use the same 500ms that the client's own world delay can grow for ping
pub const MAX_POSE_LAG_MS: f32 = CLIENT_WORLD_DELAY_MAX_RTT_MS;
pub const CLIENT_WORLD_DELAY_MAX_RTT_MS: f32 = 500.0; // the max amount that can be added on top of the base delay (due to ping rtt)
pub const CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC: f32 = 30.0;

// total amount of past snapshots the client needs to remember
pub const CLIENT_INTERPOLATION_WINDOW_MS: f32 = JITTER_MARGIN_MS + CLIENT_WORLD_DELAY_EXTRA_LAG_MS; // the base world delay past one tick: the longest gap between poses whose later one still arrives in time to be blended into
pub const CLIENT_TOTAL_SNAPSHOT_BUFFER_MS: f32 = CLIENT_WORLD_DELAY_BASE_MS + CLIENT_WORLD_DELAY_MAX_RTT_MS + CLIENT_INTERPOLATION_WINDOW_MS + JITTER_MARGIN_MS;

// ---------------------- deep links ----------------------
// scheme://
pub const DEEP_LINK_SCHEME: &str = "kube-new";

// ---------------------- updating ----------------------
pub const CLIENT_UPDATER_PATH: &str = "/api/get-client-update-info/{{target}}/{{arch}}/{{current_version}}";

// ---------------------- physics constants ----------------------
pub const DEFAULT_GRAVITY: f32 = 50.1;

// ---------------------- ground support constants ----------------------
// what getGroundSupport and getVelocityRelativeToGround use when a script leaves an option out
pub const DEFAULT_MAX_DISTANCE_TO_GROUND: f32 = 0.01;
pub const DEFAULT_MAX_GROUND_SLOPE_DEGREES: f32 = 55.0;

// ---------------------- animator constants ----------------------
pub const ANIMATOR_WEIGHT_MAX: f32 = 100.0;
pub const ANIMATOR_PRIORITY_MAX: u8 = 10;

// ---------------------- game controls constants ----------------------

// camera consts
pub const CAMERA_SIZE: f32 = 0.3; // for when it bumps into things
pub const DEFAULT_CAMERA_ORIENTATION: Orientation3D = Orientation3D { theta: 0.0, phi: 0.0 }; // where every client's camera starts looking
pub const DEFAULT_CAMERA_ZOOM: f32 = 10.0;
pub const MAX_CAMERA_ZOOM: f32 = 200.0;
pub const MIN_CAMERA_ZOOM: f32 = 0.2;

// camera sensitivity
pub const CAMERA_ROTATE_SENSITIVITY: f32 = 0.003;

// camera zoom - can be trackpad pinch or mouse wheel
pub const CAMERA_PINCH_ZOOM_SENSITIVITY: f32 = 20.0;
pub const CAMERA_MOUSE_WHEEL_ZOOM_STEP: f32 = 2.0;

// mouse cursor
pub const MAX_MOUSE_CURSOR_SIZE: f32 = 128.0; // TODO: AI-PICKED-VALUE: in points. On a Retina display that is 256 pixels, the biggest cursor many graphics cards can draw.

// ---------------------- AI chat ----------------------
// the AI chat summarizes its conversation once a model call reads this many input tokens
pub const AI_CHAT_COMPACTION_INPUT_TOKENS: u64 = 800_000;

// ---------------------- version numbers ----------------------
// clients are typically never out of date... only servers are old.
// bump this when a new client cannot connect with the old server code.
// if there is a version mismatch for this const, the client will not be allowed to connect to the server.
pub const CLIENT_SERVER_PROTOCOL_VERSION: u64 = 35;

// The browser editor's binary batches include script sources alongside object deltas.
pub const EDITOR_WORLD_PROTOCOL_VERSION: u64 = 25;

pub const STORED_WORLD_CURRENT_VERSION: u64 = 92; // world schema version
pub const MESH_DATA_CURRENT_VERSION: u64 = 6;
pub const DEHYDRATED_GLTF_CURRENT_VERSION: u64 = 6;
pub const SOUND_DATA_CURRENT_VERSION: u64 = 1;
pub const TEXTURE_DATA_CURRENT_VERSION: u64 = 1;

/*
---------------------- test cases ----------------------
tests:

1)
SERVER_STEP = 1.0/5.0
CLIENT_STEP = 1/60
works! client properly sees objects as smoothly interpolated. only problem is there's occasional HUGE lack of updates of another player's movements because of current bug.

2)
SERVER_SEND = 1.0/1.0
works!

3)
CLIENT_SEND = 1/10
and make CLIENT_WORLD_DELAY's f32::max(...).max(CLIENT_SEND_SECS)
works! stutters but only because of the occasional huge lack of updates of another player's movements.


*/

pub const MAX_PLAYTEST_CLIENTS: usize = 8;
