//! Playing a real game in a browser page.
//!
//! The page owns the canvas and the input events. This crate joins a game server over WebTransport, keeps the client
//! side of the netcode (the handshake, inputs, acks and the snapshot buffer), steps the same fixed-rate physics the
//! desktop client steps, moves the local player's character from the keys being held, and draws the result through
//! `render_core`. Scripts and audio are not in the browser yet.

#![allow(non_snake_case)]

// The walking direction and the camera turning are plain math over game types, so they build and test without a browser.
#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
mod session;
pub mod simulation;

#[cfg(target_arch = "wasm32")]
pub use session::BrowserPlaySession;
