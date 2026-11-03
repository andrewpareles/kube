use crate::object_types::{ObjectIdPointer, shared::Color3};
use serde::{Deserialize, Serialize};

pub type UserId = i64; // the public id of a kube.new account

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub username: String,
    pub userid: Option<UserId>, // the account the player signed in with, or None for guests

    pub preferredColor: Option<Color3>, // read from the account's profile when the player joins, or None if they never picked one
    pub equippedItemId: Option<i64>,    // the catalog item the account's profile is wearing, read when the player joins

    // TODO: move these to a PlayerData struct inside of world? bc orientation is not really part of playerinfo, it's part of the messaging
    // pub spawnPoint: Vec3,
    // pub health: u8,
    // pub cameraOrientation: Orientation3D,
    pub headObjectId: Option<ObjectIdPointer>,
    pub characterObjectId: Option<ObjectIdPointer>, // this should point to a physical object, but we don't enforce that, you need to check when you read this
}

pub struct PlayerInfoTrackedFields {
    pub headObjectId: Option<ObjectIdPointer>,
    pub characterObjectId: Option<ObjectIdPointer>,
}
