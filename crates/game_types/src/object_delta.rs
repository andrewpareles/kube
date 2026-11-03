//! Object delta types shared by game networking and the browser editor.
use crate::{
    consts::MAX_PLAYER_METADATA_BYTES,
    object_types::{Location3D, ObjectBeingAdded, ObjectId, ObjectInfo, Orientation3D, PhysicsInfo3D, PhysicsToggles3D},
    physics_primitive_types::PrimitiveSettings,
};
use serde::{Deserialize, Serialize};
// ----------------- types of delta that we send -----------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkedPhysicsInfo3D {
    pub coords: NetworkedCoords,
    pub teleportCount: u64, // how many times the object has jumped rather than moved
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkedCoords {
    WorldCoords(PhysicsInfo3D),        // location in world coordinates
    SubitemRelativeCoords(Location3D), // *relative* location to rigid group
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectModifyDelta {
    pub modifiedObjectInfo: Option<ObjectInfo>,             // TODO: make this Vec<(ObjectInfoFieldPathId_Leaf, Value)>
    pub modifiedPhysicsToggles3D: Option<PhysicsToggles3D>, // TODO: make this Vec<(PhysicsToggles3DFieldId_Leaf, Value)>
    pub modifiedPhysicsInfo3D: Option<NetworkedPhysicsInfo3D>,
    pub modifiedPrimitiveSettings: Option<PrimitiveSettings>,
    pub modifiedParent: Option<ObjectId>,
    pub modifiedName: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForeignCharacterDelta {
    pub physicsInfo3D: PhysicsInfo3D,
    // when the sender simulated this pose, estimated on the server's clock.
    // For high-lag players the server pushes it later so watchers play them further back.
    pub est_live_server_sim_ms: f64,
    pub teleportCount: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForeignPlayerDelta {
    pub cameraOrientation: Orientation3D, // the player's camera
    pub playerMetadata: Option<String>,   // the JSON the player's client shares with setLocalPlayerMetadata
    pub est_live_server_sim_ms: f64,
}

pub fn is_valid_player_metadata(json: &str) -> bool {
    json.len() <= MAX_PLAYER_METADATA_BYTES && serde_json::from_str::<serde_json::Value>(json).is_ok()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectAddDelta {
    pub name: String,
    pub objectBeingAdded: ObjectBeingAdded,
    pub parentId: ObjectId,
    pub initSubitemRelativeLocation3D: Option<Location3D>,
    pub teleportCount: u64,
}

pub type ObjectRemoveDelta = ();

// info about updating the object in the snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ObjectDelta {
    Add(ObjectAddDelta),
    Modify(ObjectModifyDelta),
    Remove(ObjectRemoveDelta),
    ForeignCharacter(ForeignCharacterDelta),
    ForeignPlayer(ForeignPlayerDelta),
}
