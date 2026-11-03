use std::collections::HashMap;

use glam::{Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshVertexInternal<PerVertexData> {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    pub tangent: Vec4,
    pub skinning: PerVertexData,
}

// Each vertex in the mesh can be connected with up to 4 joints
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkinningInfoPerVertex {
    pub jointIndices: [u32; 4],
    pub jointWeights: [f32; 4],
}

// always update version number when changing this
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MeshData {
    // Mesh is skinned <==> can be animated (each vertex says what joint(s) it connects to)
    SkinnedMeshData {
        vertices: Vec<MeshVertexInternal<SkinningInfoPerVertex>>,
        indices: Vec<u32>,                                        // triangles
        jointNameOfIdx: Vec<String>,                              // joint index -> name (metadata from import)
        inverseBindMatrixOfJointName: HashMap<String, [f32; 16]>, // tells triangles how to transform to go onto their joint. not used when posing joints, only when moving triangles onto them.
    },
    StaticMeshData {
        vertices: Vec<MeshVertexInternal<()>>,
        indices: Vec<u32>, // triangles
    },
}
