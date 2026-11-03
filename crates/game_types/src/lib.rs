#![allow(non_snake_case)]
//! Portable game data. Runtime behavior and rendering live in their owning crates.
pub mod consts;
pub mod data_types;
pub mod editor_sync;
pub mod migrateDehydratedGltf;
pub mod migrateStoredWorld;
pub mod object_delta;
pub mod object_types;
pub mod physics_primitive_types;
pub mod stored_world_type;

pub type GameId = i64;
