use crate::object_types::shared::ObjectIdPointer;
use glam::Vec3;
use serde::{Deserialize, Serialize};

// ------------ PivotInfo ------------

// The point its object turns and scales around. It applies to PropertyAnimator rotation, rotateObjectBy, rotateObjectTo,
// scaleObjectBy, and the rotate_recursively and scale_recursively tools. It does not apply to writing location.rotation
// directly, the editor's rotation fields, physics spin, skeletons, size animators, or creating objects; those still use
// the object's own position. It does not apply if an object is inside another object that was scaled recursively.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PivotInfo {
    pub physicalObjectId: Option<ObjectIdPointer>,
    pub offsetFromObject: Vec3, // in the object's own coordinates, so the point moves and turns with the object
}

pub struct PivotInfoTrackedFields {
    pub physicalObjectId: Option<ObjectIdPointer>,
}
