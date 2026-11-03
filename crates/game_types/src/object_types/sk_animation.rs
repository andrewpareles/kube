use crate::object_types::InterpolationStyle;
use serde::{Deserialize, Serialize};

// ------------ Animations - SkAnimation ------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkAnimationInfo {
    pub duration: f32,
    pub interpolationStyle: InterpolationStyle,
}
