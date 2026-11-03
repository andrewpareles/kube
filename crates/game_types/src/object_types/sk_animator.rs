use crate::object_types::{ObjectIdPointer, WeightTransition};
use serde::{Deserialize, Serialize};

// ------------ Animations - SkAnimator ------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkAnimatorInfo {
    // specification
    pub skeletonId: Option<ObjectIdPointer>,
    pub skAnimationId: Option<ObjectIdPointer>,

    pub clockId: Option<ObjectIdPointer>,

    // weight
    // there are 2 kinds of weight: target weight (here), and computed weight based on priority. target = the cap for computed.
    pub priority: u8,                               // 0-10. Weights are added till they reach 100.
    pub weight: f32,                                // 0.0 - 100.0, target weight for this animation.
    pub weightTransition: Option<WeightTransition>, // if set, the effective weight is sampled from this until it completes.
}

pub struct SkAnimatorInfoTrackedFields {
    pub clockId: Option<ObjectIdPointer>,
    pub skeletonId: Option<ObjectIdPointer>,
    pub skAnimationId: Option<ObjectIdPointer>,
}
