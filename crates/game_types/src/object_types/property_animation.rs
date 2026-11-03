use crate::object_types::InterpolationStyle;
use serde::{Deserialize, Serialize};

// ------------ Animations - PropertyAnimation ------------

// It should contain a keyframe PropertyKeyframe child for each timestep in your animation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropertyAnimationInfo {
    pub duration: f32,
    pub interpolationStyle: InterpolationStyle,
}
