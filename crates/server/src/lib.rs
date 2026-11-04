#![allow(non_snake_case)]

mod posthog;
mod server;
mod server_networking;
pub use server::start_running_server_in_bg;
pub use server_networking::{
    GAME_CHAT_SUBMISSION_INTERVAL, PlayerPoseLag, SERVER_FN_CALL_INTERVAL, Server, ServerForeignCharacterDTO, ServerGameChat, send_game_chat_messages,
    start_receiving_client_messages_tcp_in_bg,
};

pub mod database;
pub mod playtest_auth;
pub use server_networking::ServerMode;
