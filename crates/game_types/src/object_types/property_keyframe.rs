use serde::{Deserialize, Serialize};

// ------------ Animations - PropertyKeyframe ------------

// A child of PropertyAnimation. It holds the animated value at one point in time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropertyKeyframeInfo {
    pub time: f32,
    pub value: f32,
}
