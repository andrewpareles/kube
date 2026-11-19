//! Playing a stored world in a browser page, with no networking, scripts or audio yet.
//!
//! The page owns the canvas and the input events. This crate owns the world: it steps the same fixed-rate physics the
//! desktop client steps, moves the local player's character from the keys being held, and draws the result through
//! `render_core`.

#![allow(non_snake_case)]

// The walking direction and the camera turning are plain math over game types, so they build and test without a browser.
#[cfg(target_arch = "wasm32")]
mod session;
pub mod simulation;

#[cfg(target_arch = "wasm32")]
pub use session::BrowserPlaySession;
