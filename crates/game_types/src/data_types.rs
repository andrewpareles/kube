pub use mesh_data::{MeshData, MeshVertexInternal, SkinningInfoPerVertex};
pub use sound_data::SoundData;
pub use texture_data::TextureData;

use crate::object_types::ObjectInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub mod mesh_data;
pub mod sound_data;
pub mod texture_data;

pub type DataId = i64;

// Each kind of data counts its ids separately, so the same id can be both a mesh and a texture.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DataIds {
    pub mesh_data_ids: HashSet<DataId>,
    pub texture_data_ids: HashSet<DataId>,
    pub sound_data_ids: HashSet<DataId>,
}
impl DataIds {
    pub fn insert_data_ids_of_object(&mut self, object_info: &ObjectInfo) {
        match object_info {
            ObjectInfo::Mesh(mesh_info) => {
                self.mesh_data_ids.insert(mesh_info.meshDataId);
            }
            ObjectInfo::Material(material_info) => {
                let material_texture_data_ids = [
                    material_info.albedoTextureDataId,
                    material_info.metallicTextureDataId,
                    material_info.roughnessTextureDataId,
                    material_info.normalTextureDataId,
                ];
                self.texture_data_ids.extend(material_texture_data_ids.into_iter().flatten());
            }
            ObjectInfo::LabelImage(label_image_info) => {
                self.texture_data_ids.insert(label_image_info.textureDataId);
            }
            ObjectInfo::WorldLighting(world_lighting_info) => {
                self.texture_data_ids.extend(world_lighting_info.skyboxTextureDataId);
            }
            ObjectInfo::SoundPlayer(sound_player_info) => {
                self.sound_data_ids.insert(sound_player_info.soundDataId);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DataKind {
    MeshData,
    TextureData,
    SoundData,
}

impl DataKind {
    pub fn supabase_data_type_str(&self) -> &str {
        match self {
            DataKind::MeshData => "mesh_data",
            DataKind::TextureData => "texture_data",
            DataKind::SoundData => "sound_data",
        }
    }
}
