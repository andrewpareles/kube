use crate::object_types::{ClockState, InterpolationStyle, ObjectIdPointer};
use serde::{Deserialize, Serialize};

// ------------ Animations - tween service ------------

// A tween smoothly moves one number on its target object to toValue, and leaves it there. It plays like a PropertyAnimator
// with two keyframes, fromValue at 0 and toValue at duration, on its own clock, and wins over PropertyAnimators on the same
// number while it plays. The server deletes it once it has played out.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TweenInfo {
    pub targetObjectId: Option<ObjectIdPointer>,
    pub propertyPath: Vec<String>,
    pub fromValue: f32,
    pub toValue: f32,
    pub duration: f32, // seconds
    pub interpolationStyle: InterpolationStyle,
    pub clockState: ClockState, // playing from when the tween started
}

pub struct TweenInfoTrackedFields {
    pub targetObjectId: Option<ObjectIdPointer>,
}
