//! The serialized world and asset object schema, independent of the running game.
use crate::{
    object_types::{CameraLocation, LiveObject, MaterialInfo, ObjectId, PhysicsInfo3D, PhysicsToggles3D},
    physics_primitive_types::PrimitiveSettings,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// StoredWorld
// always update version number when changing this
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredWorld {
    pub worldSchemaVersion: u64,
    pub fallback_camera_location: CameraLocation,

    pub objectOfId: HashMap<ObjectId, LiveObject>,
    pub rootObjectId: ObjectId,
    pub childrenOfId: HashMap<ObjectId, Vec<ObjectId>>,
    pub scriptRawCodeOfId: HashMap<ObjectId, String>,

    pub physicsInfo3DOfId: HashMap<ObjectId, PhysicsInfo3D>,
    pub physicsToggles3DOfId: HashMap<ObjectId, PhysicsToggles3D>,

    pub primitiveSettingsOfId: HashMap<ObjectId, PrimitiveSettings>,
}

// A Material asset keeps the world schema version it was published at, so a later change to MaterialInfo
// brings it up to date as it is read, the same way it does a Material inside a saved world.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredMaterial {
    pub worldSchemaVersion: u64,
    pub material: MaterialInfo,
}
