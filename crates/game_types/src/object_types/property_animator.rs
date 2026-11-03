use crate::object_types::{ObjectIdPointer, WeightTransition};
use serde::{Deserialize, Serialize};

// ------------ Animations - PropertyAnimator ------------

// general purpose - smoothly changes 1 continuous (non-integer) number over time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropertyAnimatorInfo {
    // specification
    pub targetObjectId: Option<ObjectIdPointer>,
    pub propertyPath: Vec<String>,
    pub propertyAnimationId: Option<ObjectIdPointer>,

    pub clockId: Option<ObjectIdPointer>,

    // weight
    pub priority: u8,                               // 0-10, weights are added until they reach 100.
    pub weight: f32,                                // 0.0 - 100.0, target weight for this animation.
    pub weightTransition: Option<WeightTransition>, // if set, the effective weight is sampled from this until it completes.
}

pub struct PropertyAnimatorInfoTrackedFields {
    pub targetObjectId: Option<ObjectIdPointer>,
    pub clockId: Option<ObjectIdPointer>,
    pub propertyAnimationId: Option<ObjectIdPointer>,
}
