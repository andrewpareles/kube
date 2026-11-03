//! Browser WebGPU bindings for assets, using render_core::Renderer.
pub mod avatar;
pub mod stored_world_renderable_impl;
#[cfg(target_arch = "wasm32")]
mod web;
