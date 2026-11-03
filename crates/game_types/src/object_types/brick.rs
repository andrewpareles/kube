use serde::{Deserialize, Serialize};

use crate::object_types::shared::{ObjectIdPointer, VisualSettings};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BrickMaterialSettings {
    pub materialInfoId: ObjectIdPointer,
    pub tileSizeU: f32,
    pub tileSizeV: f32,
    pub tileOffsetU: f32,
    pub tileOffsetV: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrickInfo {
    pub material: Option<BrickMaterialSettings>,

    pub visualSettings: VisualSettings,
    // size, physicsToggles3D, and physicsInfo3D are not here directly — they're owned by the physics engine and treated differently in snapshots.
}

pub struct BrickInfoTrackedFields {
    pub material: Option<BrickMaterialSettings>,
}
