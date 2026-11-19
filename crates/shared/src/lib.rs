#![allow(non_snake_case)]

pub mod conversions;
pub mod game;
pub mod helpers;
pub mod protocol;

// The editor, the servers and the desktop client are the only things that read saved rows, public files and
// environment variables. The browser runs the world and its physics without any of that, so these stay native.
#[cfg(not(target_arch = "wasm32"))]
pub mod env;
#[cfg(not(target_arch = "wasm32"))]
pub mod player_profile;
#[cfg(not(target_arch = "wasm32"))]
pub mod postgrest;
#[cfg(not(target_arch = "wasm32"))]
pub mod s3_storage;

#[cfg(not(target_arch = "wasm32"))]
pub mod api;
