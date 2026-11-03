use crate::object_types::shared::{ObjectIdPointer, VisualSettings};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BallMaterialSettings {
    pub materialInfoId: ObjectIdPointer,
    pub tileSizeU: f32,
    pub tileSizeV: f32,
    pub tileOffsetU: f32,
    pub tileOffsetV: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BallInfo {
    pub material: Option<BallMaterialSettings>,

    pub visualSettings: VisualSettings,
    // size, physicsToggles3D, and physicsInfo3D are not here directly — they're owned by the physics engine and treated differently in snapshots.
}

pub struct BallInfoTrackedFields {
    pub material: Option<BallMaterialSettings>,
}
