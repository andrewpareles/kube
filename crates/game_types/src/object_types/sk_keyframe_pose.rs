use crate::object_types::Location3D;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkKeyframePoseInfo {
    pub jointName: String,
    pub localPose: Location3D,
}
