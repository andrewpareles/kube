use crate::object_types::Location3D;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkJointInfo {
    pub restPose: Location3D, // relative to parent joint
}
