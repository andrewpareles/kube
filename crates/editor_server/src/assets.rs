use crate::database;
use anyhow::{Result, ensure};
use game_types::{
    consts::{DEHYDRATED_GLTF_CURRENT_VERSION, STORED_WORLD_CURRENT_VERSION},
    data_types::{DataId, DataKind, MeshData, MeshVertexInternal, TextureData},
    object_types::{MaterialInfo, material::AlphaMode},
    stored_world_type::StoredMaterial,
};
use glam::Vec3;
use profanity::has_profanity_in_asset_name;
use render_core::dehydrated_gltf::{DehydratedAnimation, DehydratedGltf, DehydratedMaterial, DehydratedMesh, DehydratedSkeleton};
use shared::{
    api::protocol_for_assets::{Asset, AssetPublishData, AssetPublishRequest, AssetSearchRequest, AssetSummary, LoadedMaterial, UploadedDataWrapper},
    game::{
        assets::{AssetData, StoredAssetObject},
        gltf_converter::{LoadedGltf, LoadedMaterialAlphaMode, LoadedMeshData, LoadedMeshTextures, LoadedTexture},
        texture_converter::convert_rgba8_pixels_to_texture_data,
    },
    s3_storage::{PublicFile, download_public_file, upload_public_file},
};
use std::collections::{HashMap, hash_map::Entry};
use uuid::Uuid;

// -------------------------- Asset handlers --------------------------

// The authenticated editor calls these functions directly; publishing still checks ownership of existing entries.
pub async fn publish_asset(uploaded_by_profile_id: &str, request: AssetPublishRequest) -> Result<Asset> {
    ensure!(!request.name.trim().is_empty(), "Asset name cannot be empty");
    // Held to the same word list a username is, and refused rather than censored, because a name
    // with hashes in it is not a name anybody meant to give. Checked here rather than in the editor
    // so it cannot be skipped, and so the word list stays off the machines people upload from.
    ensure!(!has_profanity_in_asset_name(&request.name), "Asset name is not allowed");
    ensure!(!request.asset_id.is_some_and(|id| id <= 0), "Asset id must be positive");
    request.data.validate()?;
    if let Some(asset_id) = request.asset_id {
        let publisher = database::asset_publisher(asset_id).await?.ok_or_else(|| anyhow::anyhow!("Asset not found"))?;
        ensure!(publisher.uploaded_by == uploaded_by_profile_id, "This asset is not yours");
        ensure!(publisher.asset_type == request.data.asset_type(), "Asset type cannot change");
    }
    // Asset publishing owns the upload: store dependencies first, then publish the complete definition.
    let data = dehydrate_publish_data(request.data, uploaded_by_profile_id).await?;
    let asset_uuid = Uuid::new_v4();
    upload_public_file(&PublicFile::Asset { asset_uuid }, serde_json::to_vec(&data)?).await?;
    let mut asset_json = database::insert_asset_version(request.asset_id, &request.name, data.asset_type(), asset_uuid, uploaded_by_profile_id).await?;
    asset_json["data_json"] = serde_json::to_value(&data)?;
    Asset::fromJson(asset_json)
}

// Stores everything the data depends on and returns the definition that points at it. An upload that
// isn't published as an asset is stored the same way and added to the world without an asset entry.
pub async fn dehydrate_publish_data(data: AssetPublishData, uploaded_by_profile_id: &str) -> Result<AssetData> {
    let data = match data {
        AssetPublishData::Mesh(gltf) => AssetData::Mesh(dehydrateGltf(gltf, uploaded_by_profile_id).await?),
        AssetPublishData::Texture(texture) => {
            AssetData::Texture(upload_data_file(&DataKind::TextureData, bincode::serialize(&texture)?, uploaded_by_profile_id).await?)
        }
        AssetPublishData::Sound(sound) => {
            AssetData::Sound(upload_data_file(&DataKind::SoundData, bincode::serialize(&sound)?, uploaded_by_profile_id).await?)
        }
        AssetPublishData::Sfx(sound) => {
            AssetData::Sfx(upload_data_file(&DataKind::SoundData, bincode::serialize(&sound)?, uploaded_by_profile_id).await?)
        }
        AssetPublishData::Skybox(texture) => {
            AssetData::Skybox(upload_data_file(&DataKind::TextureData, bincode::serialize(&texture)?, uploaded_by_profile_id).await?)
        }
        AssetPublishData::Object(object_json) => AssetData::Object(serde_json::from_str::<StoredAssetObject>(&object_json)?),
        AssetPublishData::Material(material) => AssetData::Material(StoredMaterial {
            worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
            material: dehydrate_loaded_material(material, uploaded_by_profile_id).await?,
        }),
    };
    data.validate()?;
    Ok(data)
}

pub async fn search_assets(request: &AssetSearchRequest) -> Result<Vec<AssetSummary>> {
    // TODO: AI-PICKED-VALUE: Limit asset search pages to 100 entries to bound API and tool responses.
    ensure!(request.limit > 0 && request.limit <= 100, "Asset search limit must be between 1 and 100");
    database::search_asset_metadata(request).await
}

// The row says which file holds the asset's latest version, and the file is read from the CDN.
pub async fn download_asset(asset_id: i64) -> Result<Option<Asset>> {
    ensure!(asset_id > 0, "Asset id must be positive");
    let Some(mut asset_json) = database::latest_asset_row(asset_id).await? else {
        return Ok(None);
    };
    let asset_uuid = asset_json["asset_uuid"].as_str().ok_or_else(|| anyhow::anyhow!("Asset {asset_id} has no file"))?;
    asset_json["data_json"] = read_asset_file(Uuid::parse_str(asset_uuid)?).await?;
    Asset::fromJson(asset_json).map(Some)
}

// An asset version's file holds its data as it was published; readers bring it up to date.
pub async fn read_asset_file(asset_uuid: Uuid) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&download_public_file(&PublicFile::Asset { asset_uuid }).await?)?)
}

// -------------------------- Asset storage --------------------------

// Called internally by asset publishing and uploads; raw assets have no separate HTTP endpoint.
// The file is uploaded before its row is written, so a row never names a file that isn't there.
pub async fn upload_data_file(data_kind: &DataKind, data: Vec<u8>, uploaded_by_profile_id: &str) -> anyhow::Result<DataId> {
    let data_id = database::next_data_id().await?;
    upload_public_file(&PublicFile::Data { data_kind: *data_kind, data_id }, data).await?;
    database::insert_data_file(data_kind, data_id, uploaded_by_profile_id).await?;
    Ok(data_id)
}

// -------------------------- Material uploads --------------------------

// Color is stored as sRGB and every other map as data, so each map gets the compression that suits it.
async fn dehydrate_loaded_material(material: LoadedMaterial, uploaded_by_profile_id: &str) -> Result<MaterialInfo> {
    let LoadedMaterial {
        albedo,
        metallic,
        roughness,
        normal,
        alphaMode,
        isDoubleSided,
    } = material;
    let upload = async |texture: Option<LoadedTexture>, is_srgb: bool| -> Result<Option<DataId>> {
        let Some(LoadedTexture { rgba8Pixels, width, height }) = texture else {
            return Ok(None);
        };
        let texture_data = tokio::task::spawn_blocking(move || convert_rgba8_pixels_to_texture_data(rgba8Pixels, width, height, is_srgb)).await??;
        let bytes = bincode::serialize(&UploadedDataWrapper::<TextureData>::new(texture_data))?;
        Ok(Some(upload_data_file(&DataKind::TextureData, bytes, uploaded_by_profile_id).await?))
    };
    Ok(MaterialInfo {
        albedoTextureDataId: upload(albedo, true).await?,
        metallicTextureDataId: upload(metallic, false).await?,
        roughnessTextureDataId: upload(roughness, false).await?,
        normalTextureDataId: upload(normal, false).await?,
        alphaMode,
        isDoubleSided,
    })
}

// -------------------------- glTF uploads --------------------------

const MAX_MESH_BOX_SIZE: f32 = 10.0;

// This uploads each referenced texture once and returns the material data ids that
// the client needs to rebuild the same material objects later.
async fn dehydrateMaterial(
    loadedTextures: LoadedMeshTextures,
    loadedTextureOfIndex: &HashMap<usize, LoadedTexture>,
    textureDataIdOfImageIndexAndIsSrgb: &mut HashMap<(usize, bool), DataId>,
    uploaded_by_profile_id: &str,
) -> Result<Option<DehydratedMaterial>> {
    let albedoTextureDataId = if let Some(image_index) = loadedTextures.baseColor {
        Some(
            uploadTextureDataIfNeeded(
                image_index,
                true,
                loadedTextureOfIndex,
                textureDataIdOfImageIndexAndIsSrgb,
                uploaded_by_profile_id,
            )
            .await?,
        )
    } else {
        None
    };
    let metallicTextureDataId = if let Some(image_index) = loadedTextures.metallic {
        Some(
            uploadTextureDataIfNeeded(
                image_index,
                false,
                loadedTextureOfIndex,
                textureDataIdOfImageIndexAndIsSrgb,
                uploaded_by_profile_id,
            )
            .await?,
        )
    } else {
        None
    };
    let roughnessTextureDataId = if let Some(image_index) = loadedTextures.roughness {
        Some(
            uploadTextureDataIfNeeded(
                image_index,
                false,
                loadedTextureOfIndex,
                textureDataIdOfImageIndexAndIsSrgb,
                uploaded_by_profile_id,
            )
            .await?,
        )
    } else {
        None
    };
    let normalTextureDataId = if let Some(image_index) = loadedTextures.normal {
        Some(
            uploadTextureDataIfNeeded(
                image_index,
                false,
                loadedTextureOfIndex,
                textureDataIdOfImageIndexAndIsSrgb,
                uploaded_by_profile_id,
            )
            .await?,
        )
    } else {
        None
    };

    let alphaMode = match loadedTextures.alphaMode {
        LoadedMaterialAlphaMode::Opaque => AlphaMode::Opaque,
        LoadedMaterialAlphaMode::Blend => AlphaMode::Blend,
        LoadedMaterialAlphaMode::Mask { cutoff } => AlphaMode::Mask {
            cutoff: cutoff.clamp(0.0, 1.0),
        },
    };
    let material_info = MaterialInfo {
        albedoTextureDataId,
        metallicTextureDataId,
        roughnessTextureDataId,
        normalTextureDataId,
        alphaMode,
        isDoubleSided: loadedTextures.isDoubleSided,
    };
    let did_upload_any_texture = material_info.albedoTextureDataId.is_some()
        || material_info.metallicTextureDataId.is_some()
        || material_info.roughnessTextureDataId.is_some()
        || material_info.normalTextureDataId.is_some();
    let is_alpha_mode_opaque = match material_info.alphaMode {
        AlphaMode::Opaque => true,
        AlphaMode::Mask { .. } | AlphaMode::Blend => false,
    };
    if did_upload_any_texture || !is_alpha_mode_opaque || material_info.isDoubleSided {
        Ok(Some(DehydratedMaterial {
            name: loadedTextures.name,
            material_info,
        }))
    } else {
        Ok(None)
    }
}

async fn uploadTextureDataIfNeeded(
    image_index: usize,
    is_srgb: bool,
    loadedTextureOfIndex: &HashMap<usize, LoadedTexture>,
    textureDataIdOfImageIndexAndIsSrgb: &mut HashMap<(usize, bool), DataId>,
    uploaded_by_profile_id: &str,
) -> Result<DataId> {
    if let Some(texture_data_id) = textureDataIdOfImageIndexAndIsSrgb.get(&(image_index, is_srgb)) {
        return Ok(*texture_data_id);
    }

    let texture_source = loadedTextureOfIndex
        .get(&image_index)
        .ok_or_else(|| anyhow::anyhow!("Texture image index not found: {}", image_index))?;
    let pixels = texture_source.rgba8Pixels.clone();
    let (width, height) = (texture_source.width, texture_source.height);
    let texture_data = tokio::task::spawn_blocking(move || convert_rgba8_pixels_to_texture_data(pixels, width, height, is_srgb)).await??;
    let bytes = bincode::serialize(&UploadedDataWrapper::<TextureData>::new(texture_data))?;
    let texture_data_id = upload_data_file(&DataKind::TextureData, bytes, uploaded_by_profile_id).await?;
    textureDataIdOfImageIndexAndIsSrgb.insert((image_index, is_srgb), texture_data_id);
    Ok(texture_data_id)
}

fn get_vertices_bounding_box<PerVertexData>(vertices: &[MeshVertexInternal<PerVertexData>]) -> Option<(Vec3, Vec3)> {
    let first = vertices.first()?;
    let mut min = first.position;
    let mut max = first.position;
    for vertex in vertices.iter().skip(1) {
        min = min.min(vertex.position);
        max = max.max(vertex.position);
    }
    Some((min, max))
}

// uploads and returns a dehydrated (dataid-only) description of the gltf
pub async fn dehydrateGltf(loadedGltf: LoadedGltf, uploaded_by_profile_id: &str) -> Result<DehydratedGltf> {
    loadedGltf.validate()?;
    let LoadedGltf {
        blender_id,
        meshes,
        loadedTextureOfIndex,
    } = loadedGltf;
    let mut dehydrated_meshes: Vec<DehydratedMesh> = Vec::with_capacity(meshes.len());
    let mut dehydratedSkeletonOfSkeletonIndex: HashMap<usize, DehydratedSkeleton> = HashMap::new();
    let mut textureDataIdOfImageIndexAndIsSrgb: HashMap<(usize, bool), DataId> = HashMap::new();
    let mut import_bounds: Option<(Vec3, Vec3)> = None;

    // Build one import bounding box up front so split primitives share one scale and stay aligned.
    for loaded_mesh in meshes.iter() {
        let mesh_bounds = match loaded_mesh {
            LoadedMeshData::LoadedStaticMeshData { vertices, .. } => get_vertices_bounding_box(vertices),
            LoadedMeshData::LoadedSkinnedMeshData { vertices, .. } => get_vertices_bounding_box(vertices),
        };
        let Some((mesh_min, mesh_max)) = mesh_bounds else {
            continue;
        };
        match &mut import_bounds {
            Some((import_min, import_max)) => {
                *import_min = (*import_min).min(mesh_min);
                *import_max = (*import_max).max(mesh_max);
            }
            None => import_bounds = Some((mesh_min, mesh_max)),
        }
    }

    let import_scale = if let Some((import_min, import_max)) = import_bounds {
        let import_size = import_max - import_min;
        let import_max_dimension = import_size.x.max(import_size.y).max(import_size.z);
        if import_max_dimension <= 0.0 || import_max_dimension <= MAX_MESH_BOX_SIZE {
            1.0
        } else {
            MAX_MESH_BOX_SIZE / import_max_dimension
        }
    } else {
        1.0
    };

    // Upload the raw data first, then return the minimal dehydrated metadata the client needs.
    for mesh_data in meshes {
        match mesh_data {
            LoadedMeshData::LoadedStaticMeshData {
                name,
                vertices,
                indices,
                loadedTextures,
            } => {
                let material = dehydrateMaterial(
                    loadedTextures,
                    &loadedTextureOfIndex,
                    &mut textureDataIdOfImageIndexAndIsSrgb,
                    uploaded_by_profile_id,
                )
                .await?;
                let mesh_data = MeshData::StaticMeshData { vertices, indices };
                let bytes = bincode::serialize(&UploadedDataWrapper::<MeshData>::new(mesh_data))?;
                let mesh_data_id = upload_data_file(&DataKind::MeshData, bytes, uploaded_by_profile_id).await?;
                dehydrated_meshes.push(DehydratedMesh {
                    name,
                    mesh_data_id,
                    skinned_skeleton_index: None,
                    material,
                });
            }
            LoadedMeshData::LoadedSkinnedMeshData {
                name,
                vertices,
                indices,
                loadedTextures,
                skeleton_index,
                meshJointNameOfIdx,
                jointOfIndex,
                inverseBindMatrixOfJointName,
                animations,
            } => {
                let material = dehydrateMaterial(
                    loadedTextures,
                    &loadedTextureOfIndex,
                    &mut textureDataIdOfImageIndexAndIsSrgb,
                    uploaded_by_profile_id,
                )
                .await?;
                let mesh_data = MeshData::SkinnedMeshData {
                    vertices,
                    indices,
                    jointNameOfIdx: meshJointNameOfIdx,
                    inverseBindMatrixOfJointName,
                };
                let bytes = bincode::serialize(&UploadedDataWrapper::<MeshData>::new(mesh_data))?;
                let mesh_data_id = upload_data_file(&DataKind::MeshData, bytes, uploaded_by_profile_id).await?;

                match dehydratedSkeletonOfSkeletonIndex.entry(skeleton_index) {
                    Entry::Occupied(_) => {}
                    Entry::Vacant(dehydrated_skeleton_entry) => {
                        let mut dehydrated_animations: Vec<DehydratedAnimation> = Vec::with_capacity(animations.len());
                        for animation in animations {
                            dehydrated_animations.push(DehydratedAnimation {
                                name: animation.name,
                                sk_animation_info: animation.sk_animation_info,
                                sk_keyframes: animation.sk_keyframes,
                            });
                        }
                        dehydrated_skeleton_entry.insert(DehydratedSkeleton {
                            skeleton_index,
                            jointOfIndex,
                            animations: dehydrated_animations,
                        });
                    }
                }

                dehydrated_meshes.push(DehydratedMesh {
                    name,
                    mesh_data_id,
                    skinned_skeleton_index: Some(skeleton_index),
                    material,
                });
            }
        }
    }

    let mut dehydrated_skeletons: Vec<DehydratedSkeleton> = dehydratedSkeletonOfSkeletonIndex.into_values().collect();
    dehydrated_skeletons.sort_by_key(|dehydrated_skeleton| dehydrated_skeleton.skeleton_index);

    Ok(DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id,
        import_scale,
        dehydrated_meshes,
        dehydrated_skeletons,
    })
}
