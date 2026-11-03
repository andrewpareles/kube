use anyhow::Result;
use game_types::{
    consts::{
        DEHYDRATED_GLTF_CURRENT_VERSION, MESH_DATA_CURRENT_VERSION, SOUND_DATA_CURRENT_VERSION, STORED_WORLD_CURRENT_VERSION, TEXTURE_DATA_CURRENT_VERSION,
    },
    data_types::{MeshData, SoundData, TextureData},
    migrateDehydratedGltf,
    migrateStoredWorld::{self, StoredWorldKind},
    object_types::material::AlphaMode,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::Cursor;

use crate::game::{
    assets::{AssetData, AssetType, StoredAssetObject},
    gltf_converter::{LoadedGltf, LoadedTexture},
};

// Asset values shared by the tools, editor handlers, and asset loaders.
#[derive(Debug, Deserialize, Serialize)]
pub struct Asset {
    pub asset_id: i64,
    pub version_num: i64,
    pub name: String,
    pub created_at: String,
    pub asset_type: AssetType,
    pub data_json: AssetData,
}

impl Asset {
    // Asset rows keep the version they were published at, so they're migrated as they're read, like a world.
    pub fn fromJson(mut assetJson: serde_json::Value) -> Result<Self> {
        if let Some(dataJson) = assetJson.get_mut("data_json") {
            if let Some(objectJson) = dataJson.get_mut("Object") {
                migrateStoredWorld::updateToVersion(objectJson, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::AssetObject)?;
            }
            if let Some(meshJson) = dataJson.get_mut("Mesh") {
                migrateDehydratedGltf::updateToVersion(meshJson, DEHYDRATED_GLTF_CURRENT_VERSION)?;
            }
            if let Some(storedMaterialJson) = dataJson.get_mut("Material") {
                migrateStoredWorld::updateStoredMaterialToVersion(storedMaterialJson, STORED_WORLD_CURRENT_VERSION)?;
            }
        }
        Ok(serde_json::from_value(assetJson)?)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AssetSummary {
    pub asset_id: i64,
    pub version_num: i64,
    pub name: String,
    pub created_at: String,
    pub asset_type: AssetType,
}

// Publish requests carry the asset itself; stored AssetData contains the resulting data ids.
// Object JSON follows the stored-world format, which cannot be decoded directly with bincode.
#[derive(Debug, Deserialize, Serialize)]
pub enum AssetPublishData {
    Mesh(LoadedGltf), // A user-uploaded mesh
    Texture(UploadedDataWrapper<TextureData>),
    Sound(UploadedDataWrapper<SoundData>),
    Object(String),
    Material(LoadedMaterial),
    Skybox(UploadedDataWrapper<TextureData>),
    Sfx(UploadedDataWrapper<SoundData>),
}

// Like a glTF upload, a material arrives as raw pixels so the editor server chooses each map's compression.
#[derive(Debug, Deserialize, Serialize)]
pub struct LoadedMaterial {
    pub albedo: Option<LoadedTexture>,
    pub metallic: Option<LoadedTexture>,
    pub roughness: Option<LoadedTexture>,
    pub normal: Option<LoadedTexture>,
    pub alphaMode: AlphaMode,
    pub isDoubleSided: bool,
}

impl AssetPublishData {
    pub fn asset_type(&self) -> AssetType {
        match self {
            Self::Mesh(_) => AssetType::Mesh,
            Self::Texture(_) => AssetType::Texture,
            Self::Sound(_) => AssetType::Sound,
            Self::Object(_) => AssetType::Object,
            Self::Material(_) => AssetType::Material,
            Self::Skybox(_) => AssetType::Skybox,
            Self::Sfx(_) => AssetType::Sfx,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Mesh(gltf) => gltf.validate()?,
            Self::Texture(texture) | Self::Skybox(texture) => texture.err_if_invalid()?,
            Self::Sound(sound) | Self::Sfx(sound) => sound.err_if_invalid()?,
            Self::Object(object_json) => serde_json::from_str::<StoredAssetObject>(object_json)?.validate()?,
            Self::Material(material) => {
                let textures = [&material.albedo, &material.metallic, &material.roughness, &material.normal];
                anyhow::ensure!(textures.iter().any(|texture| texture.is_some()), "A material needs at least one texture");
                for texture in textures.into_iter().flatten() {
                    let expected_bytes = u64::from(texture.width)
                        .checked_mul(u64::from(texture.height))
                        .and_then(|pixels| pixels.checked_mul(4));
                    anyhow::ensure!(
                        texture.width > 0 && texture.height > 0 && expected_bytes == Some(texture.rgba8Pixels.len() as u64),
                        "Invalid material texture dimensions"
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AssetPublishRequest {
    pub asset_id: Option<i64>,
    pub name: String,
    pub data: AssetPublishData,
}

#[derive(Deserialize, Serialize)]
pub struct AssetSearchRequest {
    pub query: String,
    pub asset_type: Option<AssetType>,
    pub offset: usize,
    pub limit: usize,
}

// ------------- UploadedDataWrapper -------------

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UploadedDataWrapper<T> {
    version: u64,
    data: T,
}

// This reads the wrapper version first so callers can reject stale payloads before trying to deserialize the inner schema.
fn parse_uploaded_data_from_bytes<T>(bytes: &[u8], expected_version: u64) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let mut version_cursor = Cursor::new(bytes);
    let version = bincode::deserialize_from::<_, u64>(&mut version_cursor)?;
    if version != expected_version {
        return Err(anyhow::anyhow!("Unsupported data version: expected {}, got {}", expected_version, version));
    }
    Ok(bincode::deserialize::<UploadedDataWrapper<T>>(bytes)?.data)
}

impl UploadedDataWrapper<MeshData> {
    pub fn new(data: MeshData) -> Self {
        Self {
            version: MESH_DATA_CURRENT_VERSION,
            data,
        }
    }

    pub fn err_if_invalid(&self) -> anyhow::Result<()> {
        if self.version != MESH_DATA_CURRENT_VERSION {
            return Err(anyhow::anyhow!("Unsupported data version",));
        }
        Ok(())
    }

    pub fn parse_data_from_bytes(bytes: &[u8]) -> anyhow::Result<MeshData> {
        parse_uploaded_data_from_bytes::<MeshData>(bytes, MESH_DATA_CURRENT_VERSION)
    }
}

impl UploadedDataWrapper<TextureData> {
    pub fn new(data: TextureData) -> Self {
        Self {
            version: TEXTURE_DATA_CURRENT_VERSION,
            data,
        }
    }
    pub fn err_if_invalid(&self) -> anyhow::Result<()> {
        if self.version != TEXTURE_DATA_CURRENT_VERSION {
            return Err(anyhow::anyhow!("Unsupported data version",));
        }
        Ok(())
    }
    pub fn parse_data_from_bytes(bytes: &[u8]) -> anyhow::Result<TextureData> {
        parse_uploaded_data_from_bytes::<TextureData>(bytes, TEXTURE_DATA_CURRENT_VERSION)
    }
}

impl UploadedDataWrapper<SoundData> {
    pub fn new(data: SoundData) -> Self {
        Self {
            version: SOUND_DATA_CURRENT_VERSION,
            data,
        }
    }
    pub fn err_if_invalid(&self) -> anyhow::Result<()> {
        if self.version != SOUND_DATA_CURRENT_VERSION {
            return Err(anyhow::anyhow!("Unsupported data version",));
        }
        Ok(())
    }
    pub fn parse_data_from_bytes(bytes: &[u8]) -> anyhow::Result<SoundData> {
        parse_uploaded_data_from_bytes::<SoundData>(bytes, SOUND_DATA_CURRENT_VERSION)
    }
}
