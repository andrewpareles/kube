use crate::object_types::ObjectIdPointer;
use serde::{Deserialize, Serialize};

// Groups are only used to initially position elements. The physics of the children are 100% independent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RigidGroupInfo {
    // physicsInfo3D on this object, but not directly, it's special since it's handled by the physics engine and treated differently in snapshots. eventually we might want to have a PhysicsObject type/trait here and use it in the physics engine to make this clearer.
    pub followSkeletonId: Option<ObjectIdPointer>, // points to Skeleton
}

pub struct RigidGroupInfoTrackedFields {
    pub followSkeletonId: Option<ObjectIdPointer>,
}
