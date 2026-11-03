use crate::object_types::{Location3D, ObjectIdPointer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkAttachmentInfo {
    pub objectId: Option<ObjectIdPointer>,
    pub jointName: String,
    pub offsetFromJoint: Location3D,
}

pub struct SkAttachmentInfoTrackedFields {
    pub objectId: Option<ObjectIdPointer>,
}
