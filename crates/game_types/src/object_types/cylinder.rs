use crate::object_types::shared::{ObjectIdPointer, VisualSettings};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CylinderMaterialSettings {
    pub materialInfoId: ObjectIdPointer,
    pub tileSizeU: f32,
    pub tileSizeV: f32,
    pub tileOffsetU: f32,
    pub tileOffsetV: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CylinderInfo {
    pub material: Option<CylinderMaterialSettings>,

    pub visualSettings: VisualSettings,
    // radius, length, physicsToggles3D, and physicsInfo3D are not here directly. They are owned by the physics engine and treated differently in snapshots.
}

pub struct CylinderInfoTrackedFields {
    pub material: Option<CylinderMaterialSettings>,
}
