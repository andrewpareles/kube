use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::{
    data_types::DataId,
    object_types::shared::{ObjectIdPointer, VisualSettings},
};

// // equivalent of Ball/Brick/CylinderPrimitiveSettings, except purely visual so lives on MeshInfo
// #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
// pub struct MeshVisualSettings {
//     pub meshDataId: DataId,
//     pub scale: Vec3,
// }

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MeshMaterialSettings {
    pub materialInfoId: ObjectIdPointer,
}

// ------------ MeshInfo ------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshInfo {
    pub meshDataId: DataId,                        // data id of the mesh
    pub scale: Vec3,                               // visual scale of the mesh (mesh data id does not store scale)
    pub material: Option<MeshMaterialSettings>,    // material settings the mesh uses (mesh data id does not store textures)
    pub followSkeletonId: Option<ObjectIdPointer>, // animate using this Skeleton (only allowed if meshdata = skinned)
    pub visualSettings: VisualSettings,

    pub physicalObjectId: Option<ObjectIdPointer>, // follow this object's location and orientation
}

pub struct MeshInfoTrackedFields {
    pub followSkeletonId: Option<ObjectIdPointer>,
    pub physicalObjectId: Option<ObjectIdPointer>,
    pub material: Option<MeshMaterialSettings>,
}
