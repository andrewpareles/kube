use serde::{Deserialize, Serialize};

use crate::renderer_types::{RenderSkJoint, RenderSkKeyframe};
use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    data_types::DataId,
    object_types::{MaterialInfo, SkAnimationInfo},
};

// Editable model sources have their own identity and storage bucket.
pub type BlenderId = i64;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DehydratedAnimation {
    pub name: String,
    pub sk_animation_info: SkAnimationInfo,
    pub sk_keyframes: Vec<RenderSkKeyframe>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DehydratedSkeleton {
    pub skeleton_index: usize,
    pub jointOfIndex: Vec<RenderSkJoint>,
    pub animations: Vec<DehydratedAnimation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DehydratedMaterial {
    pub name: String,
    pub material_info: MaterialInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DehydratedMesh {
    pub name: String,
    pub mesh_data_id: DataId,
    pub skinned_skeleton_index: Option<usize>,
    pub material: Option<DehydratedMaterial>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DehydratedGltf {
    pub version: u64,
    pub blender_id: Option<BlenderId>,
    // All imported meshes share one scale so split primitives stay aligned.
    pub import_scale: f32,
    pub dehydrated_meshes: Vec<DehydratedMesh>,
    pub dehydrated_skeletons: Vec<DehydratedSkeleton>,
}

// Loaded imports and stored asset models use the same skeleton and animation checks.
pub fn validate_skeleton_content<'a>(
    joints: &[RenderSkJoint],
    animations: impl Iterator<Item = (&'a SkAnimationInfo, &'a [RenderSkKeyframe])>,
) -> anyhow::Result<()> {
    let mut joint_names = std::collections::HashSet::new();
    for (index, joint) in joints.iter().enumerate() {
        anyhow::ensure!(joint_names.insert(&joint.name), "Repeated joint name");
        anyhow::ensure!(
            joint.parent_joint_index.is_none_or(|parent| parent < index),
            "Joint parent must precede its child"
        );
    }
    for (animation, keyframes) in animations {
        anyhow::ensure!(animation.duration.is_finite() && animation.duration >= 0.0, "Invalid animation duration");
        let mut previous_time = 0.0;
        for keyframe in keyframes {
            anyhow::ensure!(keyframe.time.is_finite() && keyframe.time >= previous_time, "Invalid keyframe time");
            previous_time = keyframe.time;
            for pose in &keyframe.skKeyframePoses {
                anyhow::ensure!(joint_names.contains(&pose.jointName), "Keyframe refers to an unknown joint");
            }
        }
    }
    Ok(())
}

impl DehydratedGltf {
    // Asset models must be complete before the importer starts adding world objects.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == DEHYDRATED_GLTF_CURRENT_VERSION,
            "Unsupported dehydrated glTF version: {}",
            self.version
        );
        anyhow::ensure!(self.blender_id.is_none_or(|id| id > 0), "Invalid Blender id");
        anyhow::ensure!(!self.dehydrated_meshes.is_empty(), "Model has no mesh parts");
        anyhow::ensure!(self.import_scale.is_finite() && self.import_scale > 0.0, "Invalid model scale");
        let mut skeleton_indices = std::collections::HashSet::new();
        for skeleton in &self.dehydrated_skeletons {
            anyhow::ensure!(skeleton_indices.insert(skeleton.skeleton_index), "Repeated skeleton index");
            validate_skeleton_content(
                &skeleton.jointOfIndex,
                skeleton
                    .animations
                    .iter()
                    .map(|animation| (&animation.sk_animation_info, animation.sk_keyframes.as_slice())),
            )?;
        }
        for mesh in &self.dehydrated_meshes {
            anyhow::ensure!(mesh.mesh_data_id > 0, "Invalid mesh data id");
            anyhow::ensure!(
                mesh.skinned_skeleton_index.is_none_or(|index| skeleton_indices.contains(&index)),
                "Mesh refers to a missing skeleton"
            );
            if let Some(DehydratedMaterial { material_info, .. }) = &mesh.material {
                for id in [
                    material_info.albedoTextureDataId,
                    material_info.metallicTextureDataId,
                    material_info.roughnessTextureDataId,
                    material_info.normalTextureDataId,
                ]
                .into_iter()
                .flatten()
                {
                    anyhow::ensure!(id > 0, "Invalid texture data id");
                }
            }
        }
        Ok(())
    }
}
