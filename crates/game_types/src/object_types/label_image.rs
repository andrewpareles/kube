use crate::data_types::DataId;
use crate::object_types::label::BillboardMode;
use crate::object_types::shared::{ObjectIdPointer, VisualSettings};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

// ------------ LabelImageInfo ------------

// A picture in the world. It follows physicalObjectId and orients itself exactly like a Label.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LabelImageInfo {
    pub textureDataId: DataId,
    pub width: f32,  // world units
    pub height: f32, // world units

    pub offsetFromObject: Vec3,
    pub rotationFromObject: Quat,
    pub physicalObjectId: Option<ObjectIdPointer>,

    pub billboardMode: BillboardMode,
    pub isConstantSize: bool,
    pub visualSettings: VisualSettings,
}

pub struct LabelImageInfoTrackedFields {
    pub physicalObjectId: Option<ObjectIdPointer>,
}
