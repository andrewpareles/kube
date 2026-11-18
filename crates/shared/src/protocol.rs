pub mod bincode_config;
pub mod messages;
pub mod quic_cert;
// QUIC carries the game between the servers and the desktop client. A browser reaches the game some other way, so the
// helpers that read and write QUIC streams stay native.
#[cfg(not(target_arch = "wasm32"))]
pub mod quic_send_receive_helpers;
// The connection the game's netcode talks through, whichever transport carries it.
#[cfg(not(target_arch = "wasm32"))]
pub mod player_connection;
