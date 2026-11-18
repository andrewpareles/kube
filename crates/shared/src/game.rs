// The JS engine, the catalog and the downloaded assets are only needed where something can download and run them. The
// browser runs the world and its physics without any of that, so those modules stay native.
pub mod client_world_delay;
#[cfg(not(target_arch = "wasm32"))]
pub mod data_fetcher;
pub mod gui_types;
pub mod interpolate_physics;
#[cfg(not(target_arch = "wasm32"))]
pub mod jsRuntime;
#[cfg(not(target_arch = "wasm32"))]
pub mod jsRuntimeOps;
pub mod jsRustConversions;
#[cfg(not(target_arch = "wasm32"))]
mod js_cpu_watchdog;
pub use game_types::migrateStoredWorld;
pub mod physics;
pub mod snapshot_applier;
pub mod snapshot_buffer;
pub mod timeline;
pub mod time_estimator;
#[cfg(not(target_arch = "wasm32"))]
pub mod transpileJsScript;
pub mod uploaded_data;
pub mod world;
mod world_renderable_impl;

#[cfg(not(target_arch = "wasm32"))]
pub mod assets;

#[cfg(not(target_arch = "wasm32"))]
pub mod catalog;

pub mod add_dehydrated_gltf_to_world;

pub mod gltf_converter;

// Textures become KTX2 through libktx's C++ build, and the browser decodes its own textures in JavaScript instead.
#[cfg(not(target_arch = "wasm32"))]
pub mod texture_converter;
