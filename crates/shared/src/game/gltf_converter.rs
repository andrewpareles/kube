// see scene.wgsl for the comment explaining this file

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use render_core::{
    computeSkeletonInfo::{computeJointDefaultLocalLocation3D, computeJointIndexOfName},
    dehydrated_gltf::{BlenderId, validate_skeleton_content},
    renderer_types::{RenderSkJoint, RenderSkKeyframe, RenderSkKeyframePose},
};
use serde::{Deserialize, Serialize};

use game_types::{
    data_types::{MeshVertexInternal, SkinningInfoPerVertex},
    object_types::{InterpolationStyle, SkAnimationInfo, shared::Location3D},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub enum LoadedMaterialAlphaMode {
    #[default]
    Opaque,
    Mask {
        cutoff: f32,
    },
    Blend,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoadedTexture {
    pub rgba8Pixels: Vec<u8>,
    // metadata about the rgba8Pixels:
    pub width: u32,  // in pixels
    pub height: u32, // in pixels
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LoadedMeshTextures {
    pub name: String,
    pub baseColor: Option<usize>,
    pub metallic: Option<usize>,
    pub roughness: Option<usize>,
    pub normal: Option<usize>,
    pub alphaMode: LoadedMaterialAlphaMode,
    pub isDoubleSided: bool,
}
// TODO: emissive, pub emissive: Option<(usize, [f32; 3])>,  // (LoadedTexture, [r, g, b]) // we really just want this to be grayscale, maybe convert

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoadedAnimation {
    pub name: String,
    pub sk_animation_info: SkAnimationInfo,
    pub sk_keyframes: Vec<RenderSkKeyframe>,
}

#[derive(Clone, Copy, Debug)]
enum LoadedAnimationChannelTarget {
    Translation,
    Rotation,
}

#[derive(Clone, Debug)]
struct LoadedAnimationChannel {
    appliesToJointIndex: usize,
    target: LoadedAnimationChannelTarget,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum LoadedMeshData {
    // MeshData::SkinnedMesh
    LoadedSkinnedMeshData {
        // the Blender object's name
        name: String,
        // triangles
        vertices: Vec<MeshVertexInternal<SkinningInfoPerVertex>>,
        indices: Vec<u32>,
        // textures
        loadedTextures: LoadedMeshTextures,
        // Skeleton
        skeleton_index: usize,
        meshJointNameOfIdx: Vec<String>,
        jointOfIndex: Vec<RenderSkJoint>,
        inverseBindMatrixOfJointName: HashMap<String, [f32; 16]>,

        // animation data
        animations: Vec<LoadedAnimation>,
    },
    // MeshData::StaticMesh
    LoadedStaticMeshData {
        // the Blender object's name
        name: String,
        // triangles
        vertices: Vec<MeshVertexInternal<()>>,
        indices: Vec<u32>,
        // material textures
        loadedTextures: LoadedMeshTextures,
    },
}

// internal
struct LoadedSkeleton {
    jointOfIndex: Vec<RenderSkJoint>,
    jointNameOfGltfJointIndex: Vec<String>,
    jointIndexOfGltfNodeIndex: HashMap<usize, usize>,
    inverseBindMatrixOfJointName: HashMap<String, [f32; 16]>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoadedGltf {
    pub blender_id: Option<BlenderId>,
    pub meshes: Vec<LoadedMeshData>,
    pub loadedTextureOfIndex: HashMap<usize, LoadedTexture>, // local index -> LoadedTexture, lets us avoid duplicates
}

// Every copy of a model is drawn each frame, so an imported model has at most this many triangles, and 3 vertices for each.
const MAX_MODEL_TRIANGLES: usize = 20_000;

// Validate the complete import before the API writes any of its dependencies.
impl LoadedGltf {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.blender_id.is_none_or(|id| id > 0), "Invalid Blender id");
        anyhow::ensure!(!self.meshes.is_empty(), "glTF has no meshes");
        for texture in self.loadedTextureOfIndex.values() {
            let expected_bytes = u64::from(texture.width)
                .checked_mul(u64::from(texture.height))
                .and_then(|pixels| pixels.checked_mul(4));
            anyhow::ensure!(
                texture.width > 0 && texture.height > 0 && expected_bytes == Some(texture.rgba8Pixels.len() as u64),
                "Invalid glTF texture dimensions"
            );
        }
        let mut model_vertex_count = 0;
        let mut model_triangle_count = 0;
        for mesh in &self.meshes {
            let (vertex_count, indices, textures) = match mesh {
                LoadedMeshData::LoadedStaticMeshData {
                    vertices,
                    indices,
                    loadedTextures,
                    ..
                } => (vertices.len(), indices, loadedTextures),
                LoadedMeshData::LoadedSkinnedMeshData {
                    vertices,
                    indices,
                    loadedTextures,
                    jointOfIndex,
                    animations,
                    ..
                } => {
                    validate_skeleton_content(
                        jointOfIndex,
                        animations
                            .iter()
                            .map(|animation| (&animation.sk_animation_info, animation.sk_keyframes.as_slice())),
                    )?;
                    (vertices.len(), indices, loadedTextures)
                }
            };
            anyhow::ensure!(
                vertex_count > 0 && !indices.is_empty() && indices.len() % 3 == 0,
                "glTF mesh has no complete triangles"
            );
            anyhow::ensure!(indices.iter().all(|index| (*index as usize) < vertex_count), "glTF mesh index is out of bounds");
            for index in [textures.baseColor, textures.metallic, textures.roughness, textures.normal]
                .into_iter()
                .flatten()
            {
                anyhow::ensure!(self.loadedTextureOfIndex.contains_key(&index), "Missing glTF texture {index}");
            }
            model_vertex_count += vertex_count;
            model_triangle_count += indices.len() / 3;
        }
        anyhow::ensure!(
            model_triangle_count <= MAX_MODEL_TRIANGLES && model_vertex_count <= 3 * MAX_MODEL_TRIANGLES,
            "This model has {model_triangle_count} triangles and {model_vertex_count} vertices, and a model can have at most {MAX_MODEL_TRIANGLES} triangles and {} vertices. Simplify it first, for example with Blender's Decimate modifier.",
            3 * MAX_MODEL_TRIANGLES
        );
        Ok(())
    }
}

// // this returns MULTIPLE LoadedMeshData objects:
// pub fn parseGltf<P: AsRef<Path>>(path: P) -> Result<LoadedGltf> {
//     let path_ref = path.as_ref();
//     let extension = path_ref.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_lowercase();

//     if extension != "gltf" && extension != "glb" {
//         return Err(anyhow!("Unsupported mesh format '{}'", extension));
//     }

//     let (document, buffers, images) = gltf::import(path_ref).with_context(|| format!("Failed to load glTF '{}'", path_ref.display()))?;
//     parse_loaded_gltf(document, buffers, images)
// }

// This loads a glTF or GLB from disk so `.gltf` files can resolve sidecar buffers and textures.
pub fn parsePathIntoLoadedGltf<P: AsRef<Path>>(path: P) -> Result<LoadedGltf> {
    let path_ref = path.as_ref();
    let extension = path_ref.extension().and_then(|extension| extension.to_str()).unwrap_or("").to_lowercase();
    if extension != "gltf" && extension != "glb" {
        return Err(anyhow!("Unsupported mesh format '{}'", extension));
    }
    let (document, buffers, images) = gltf::import(path_ref).with_context(|| format!("Failed to load glTF '{}'", path_ref.display()))?;
    parse_gltf(document, buffers, images)
}

pub fn parseBytesIntoLoadedGltf(glb_bytes: &[u8]) -> Result<LoadedGltf> {
    let (document, buffers, images) = gltf::import_slice(glb_bytes).context("Failed to parse glTF bytes")?;
    parse_gltf(document, buffers, images)
}

fn parse_gltf(document: gltf::Document, buffers: Vec<gltf::buffer::Data>, images: Vec<gltf::image::Data>) -> Result<LoadedGltf> {
    let mut textureSourceOfImageIndex = load_texture_sources(&images)?;
    for material in document.materials() {
        let Some(material_index) = material.index() else { continue };
        let pbr = material.pbr_metallic_roughness();
        if pbr.base_color_texture().is_none() {
            let [red, green, blue, alpha] = pbr.base_color_factor();
            let pixel = [linear_to_srgb_u8(red), linear_to_srgb_u8(green), linear_to_srgb_u8(blue), unit_to_u8(alpha)];
            textureSourceOfImageIndex.insert(images.len() + 2 * material_index, flat_texture(pixel));
        }
        if pbr.metallic_roughness_texture().is_none() {
            let pixel = [0, unit_to_u8(pbr.roughness_factor()), unit_to_u8(pbr.metallic_factor()), 255];
            textureSourceOfImageIndex.insert(images.len() + 2 * material_index + 1, flat_texture(pixel));
        }
    }
    let nodeTransformOfIndex = compute_node_world_transforms(&document);
    let mut loadedSkeletonOfSkeletonIndex: HashMap<usize, LoadedSkeleton> = HashMap::new();
    let mut didAttachAnimationsOfSkeletonIndex: HashSet<usize> = HashSet::new();
    let mut out_meshes: Vec<LoadedMeshData> = Vec::new();

    for node in document.nodes() {
        let gltf_mesh = match node.mesh() {
            Some(mesh) => mesh,
            None => continue,
        };

        let worldTransform = mesh_world_transform_for_node(&nodeTransformOfIndex, node.index());
        let skeleton_index_of_node = node.skin().map(|skin| skin.index());

        for primitive in gltf_mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }

            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<[f32; 3]> = reader.read_positions().ok_or_else(|| anyhow!("Mesh primitive is missing positions"))?.collect();
            if positions.is_empty() {
                continue;
            }

            let had_normals = reader.read_normals().is_some();
            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .map(|iter| iter.collect())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
            let had_tangents = reader.read_tangents().is_some();
            let tangents: Vec<[f32; 4]> = reader
                .read_tangents()
                .map(|iter| iter.collect())
                .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 1.0]; positions.len()]);
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|iter| iter.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let joints_iter = reader.read_joints(0);
            let has_skin_joints = joints_iter.is_some();
            let joints: Vec<[u16; 4]> = joints_iter
                .map(|iter| iter.into_u16().collect())
                .unwrap_or_else(|| vec![[0, 0, 0, 0]; positions.len()]);
            let weights_iter = reader.read_weights(0);
            let has_skin_weights = weights_iter.is_some();
            let weights: Vec<[f32; 4]> = weights_iter
                .map(|iter| iter.into_f32().collect())
                .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 0.0]; positions.len()]);

            let primitive_has_skinning = has_skin_joints && has_skin_weights;

            let local_indices: Vec<u32> = match reader.read_indices() {
                Some(iter) => iter.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if local_indices.len() % 3 != 0 {
                return Err(anyhow!("Mesh indices must be a multiple of 3 for triangles"));
            }

            let (final_positions, mut final_normals, mut final_tangents) = if primitive_has_skinning {
                (positions, normals, tangents)
            } else {
                apply_transform_to_vertices(&positions, &normals, &tangents, &worldTransform)
            };

            if !had_normals {
                compute_smooth_normals(&final_positions, &local_indices, &mut final_normals);
            }
            if !had_tangents {
                final_tangents = compute_tangents(&final_positions, &uvs, &final_normals, &local_indices);
            }

            let mut vertices_with_skinning: Vec<MeshVertexInternal<SkinningInfoPerVertex>> = Vec::with_capacity(final_positions.len());
            for index in 0..final_positions.len() {
                let normal = final_normals.get(index).copied().unwrap_or([0.0, 1.0, 0.0]);
                let uv = uvs.get(index).copied().unwrap_or([0.0, 0.0]);
                let tangent = final_tangents.get(index).copied().unwrap_or([1.0, 0.0, 0.0, 1.0]);
                let joint_indices = joints.get(index).copied().unwrap_or([0, 0, 0, 0]);
                let joint_weights = weights.get(index).copied().unwrap_or([1.0, 0.0, 0.0, 0.0]);

                vertices_with_skinning.push(MeshVertexInternal {
                    position: Vec3::from_array(final_positions[index]),
                    normal: Vec3::from_array(normal),
                    uv: Vec2::from_array(uv),
                    tangent: Vec4::from_array(tangent),
                    skinning: SkinningInfoPerVertex {
                        jointIndices: [
                            joint_indices[0] as u32,
                            joint_indices[1] as u32,
                            joint_indices[2] as u32,
                            joint_indices[3] as u32,
                        ],
                        jointWeights: joint_weights,
                    },
                });
            }

            if vertices_with_skinning.is_empty() || local_indices.is_empty() {
                continue;
            }

            // Each part keeps the names it had in Blender, and parts without names are numbered.
            let mesh_number = out_meshes.len() + 1;
            let name = node.name().map_or(format!("mesh_{mesh_number}"), str::to_string);
            let loadedTextures = load_mesh_textures(&primitive.material(), images.len(), mesh_number);

            if primitive_has_skinning {
                let Some(skeleton_index) = skeleton_index_of_node else {
                    continue;
                };
                let loaded_skeleton = loadedSkeletonOfSkeletonIndex
                    .entry(skeleton_index)
                    .or_insert_with(|| load_skeleton(skeleton_index, &document, &buffers));
                let animationsForSkeleton = if didAttachAnimationsOfSkeletonIndex.insert(skeleton_index) {
                    load_animations_for_skeleton(&document, &buffers, loaded_skeleton)
                } else {
                    Vec::new()
                };
                let jointOfIndex = loaded_skeleton.jointOfIndex.clone();
                out_meshes.push(LoadedMeshData::LoadedSkinnedMeshData {
                    name,
                    vertices: vertices_with_skinning,
                    indices: local_indices,
                    loadedTextures,
                    skeleton_index,
                    meshJointNameOfIdx: loaded_skeleton.jointNameOfGltfJointIndex.clone(),
                    jointOfIndex,
                    inverseBindMatrixOfJointName: loaded_skeleton.inverseBindMatrixOfJointName.clone(),
                    animations: animationsForSkeleton,
                });
            } else {
                let vertices_no_skin: Vec<MeshVertexInternal<()>> = vertices_with_skinning
                    .into_iter()
                    .map(|vertex| MeshVertexInternal {
                        position: vertex.position,
                        normal: vertex.normal,
                        uv: vertex.uv,
                        tangent: vertex.tangent,
                        skinning: (),
                    })
                    .collect();
                out_meshes.push(LoadedMeshData::LoadedStaticMeshData {
                    name,
                    vertices: vertices_no_skin,
                    indices: local_indices,
                    loadedTextures,
                });
            }
        }
    }

    if out_meshes.is_empty() {
        return Err(anyhow!("No triangle mesh data found in glTF file"));
    }

    Ok(LoadedGltf {
        blender_id: None,
        meshes: out_meshes,
        loadedTextureOfIndex: textureSourceOfImageIndex,
    })
}

fn load_texture_sources(images: &[gltf::image::Data]) -> Result<HashMap<usize, LoadedTexture>> {
    let mut textureSourceOfImageIndex = HashMap::new();
    for (image_index, image_data) in images.iter().enumerate() {
        let texture_source = convert_image_data_to_rgba8(image_data)?;
        textureSourceOfImageIndex.insert(image_index, texture_source);
    }
    Ok(textureSourceOfImageIndex)
}

const FLAT_FACTOR_TEXTURE_SIDE_PIXELS: u32 = 4;

fn flat_texture(pixel: [u8; 4]) -> LoadedTexture {
    LoadedTexture {
        rgba8Pixels: pixel.repeat((FLAT_FACTOR_TEXTURE_SIDE_PIXELS * FLAT_FACTOR_TEXTURE_SIDE_PIXELS) as usize),
        width: FLAT_FACTOR_TEXTURE_SIDE_PIXELS,
        height: FLAT_FACTOR_TEXTURE_SIDE_PIXELS,
    }
}

fn unit_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn linear_to_srgb_u8(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    unit_to_u8(encoded)
}

fn load_mesh_textures(material: &gltf::Material<'_>, image_count: usize, mesh_number: usize) -> LoadedMeshTextures {
    let mut material_source = LoadedMeshTextures {
        name: material.name().map_or(format!("material_{mesh_number}"), str::to_string),
        ..Default::default()
    };

    let pbr = material.pbr_metallic_roughness();

    if let Some(base_color_texture_info) = pbr.base_color_texture() {
        let base_color_image_index = base_color_texture_info.texture().source().index();
        material_source.baseColor = Some(base_color_image_index);
    } else if let Some(material_index) = material.index() {
        material_source.baseColor = Some(image_count + 2 * material_index);
    }

    if let Some(metallic_roughness_texture_info) = pbr.metallic_roughness_texture() {
        let metallic_roughness_image_index = metallic_roughness_texture_info.texture().source().index();
        material_source.metallic = Some(metallic_roughness_image_index);
        material_source.roughness = Some(metallic_roughness_image_index);
    } else if let Some(material_index) = material.index() {
        material_source.metallic = Some(image_count + 2 * material_index + 1);
        material_source.roughness = Some(image_count + 2 * material_index + 1);
    }

    if let Some(normal_texture_info) = material.normal_texture() {
        let normal_image_index = normal_texture_info.texture().source().index();
        material_source.normal = Some(normal_image_index);
    }

    material_source.isDoubleSided = material.double_sided();
    material_source.alphaMode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => LoadedMaterialAlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => LoadedMaterialAlphaMode::Mask {
            // glTF defines 0.5 when a MASK material omits alphaCutoff.
            cutoff: material.alpha_cutoff().unwrap_or(0.5).clamp(0.0, 1.0),
        },
        gltf::material::AlphaMode::Blend => LoadedMaterialAlphaMode::Blend,
    };

    // if let Some(emissive_texture_info) = material.emissive_texture() {
    //     let emissive_image_index = emissive_texture_info.texture().source().index();
    //     material_source.emissive = Some((emissive_image_index, material.emissive_factor()));
    // }

    // if let Some(occlusion_texture_info) = material.occlusion_texture() {
    //     let occlusion_image_index = occlusion_texture_info.texture().source().index();
    //     material_source.occlusion = Some((occlusion_image_index, occlusion_texture_info.strength()));
    // }

    material_source
}

fn convert_image_data_to_rgba8(image_data: &gltf::image::Data) -> Result<LoadedTexture> {
    let pixel_count = (image_data.width as usize)
        .checked_mul(image_data.height as usize)
        .ok_or_else(|| anyhow!("Texture dimensions are too large: {}x{}", image_data.width, image_data.height))?;

    let mut rgba8_pixels: Vec<u8> = Vec::with_capacity(pixel_count.saturating_mul(4));

    match image_data.format {
        gltf::image::Format::R8 => {
            for value in &image_data.pixels {
                rgba8_pixels.extend_from_slice(&[*value, *value, *value, 255]);
            }
        }
        gltf::image::Format::R8G8 => {
            ensure_expected_bytes(pixel_count, 2, image_data.pixels.len(), "R8G8")?;
            for channels in image_data.pixels.chunks_exact(2) {
                let gray = channels[0];
                let alpha = channels[1];
                rgba8_pixels.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        gltf::image::Format::R8G8B8 => {
            ensure_expected_bytes(pixel_count, 3, image_data.pixels.len(), "R8G8B8")?;
            for channels in image_data.pixels.chunks_exact(3) {
                rgba8_pixels.extend_from_slice(&[channels[0], channels[1], channels[2], 255]);
            }
        }
        gltf::image::Format::R8G8B8A8 => {
            ensure_expected_bytes(pixel_count, 4, image_data.pixels.len(), "R8G8B8A8")?;
            rgba8_pixels.extend_from_slice(&image_data.pixels);
        }
        gltf::image::Format::R16 => {
            ensure_expected_bytes(pixel_count, 2, image_data.pixels.len(), "R16")?;
            for channels in image_data.pixels.chunks_exact(2) {
                let gray = u16::from_le_bytes([channels[0], channels[1]]) as f32 / 65535.0;
                let gray = float_to_u8(gray);
                rgba8_pixels.extend_from_slice(&[gray, gray, gray, 255]);
            }
        }
        gltf::image::Format::R16G16 => {
            ensure_expected_bytes(pixel_count, 4, image_data.pixels.len(), "R16G16")?;
            for channels in image_data.pixels.chunks_exact(4) {
                let gray = u16::from_le_bytes([channels[0], channels[1]]) as f32 / 65535.0;
                let alpha = u16::from_le_bytes([channels[2], channels[3]]) as f32 / 65535.0;
                let gray = float_to_u8(gray);
                let alpha = float_to_u8(alpha);
                rgba8_pixels.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        gltf::image::Format::R16G16B16 => {
            ensure_expected_bytes(pixel_count, 6, image_data.pixels.len(), "R16G16B16")?;
            for channels in image_data.pixels.chunks_exact(6) {
                let red = u16::from_le_bytes([channels[0], channels[1]]) as f32 / 65535.0;
                let green = u16::from_le_bytes([channels[2], channels[3]]) as f32 / 65535.0;
                let blue = u16::from_le_bytes([channels[4], channels[5]]) as f32 / 65535.0;
                rgba8_pixels.extend_from_slice(&[float_to_u8(red), float_to_u8(green), float_to_u8(blue), 255]);
            }
        }
        gltf::image::Format::R16G16B16A16 => {
            ensure_expected_bytes(pixel_count, 8, image_data.pixels.len(), "R16G16B16A16")?;
            for channels in image_data.pixels.chunks_exact(8) {
                let red = u16::from_le_bytes([channels[0], channels[1]]) as f32 / 65535.0;
                let green = u16::from_le_bytes([channels[2], channels[3]]) as f32 / 65535.0;
                let blue = u16::from_le_bytes([channels[4], channels[5]]) as f32 / 65535.0;
                let alpha = u16::from_le_bytes([channels[6], channels[7]]) as f32 / 65535.0;
                rgba8_pixels.extend_from_slice(&[float_to_u8(red), float_to_u8(green), float_to_u8(blue), float_to_u8(alpha)]);
            }
        }
        gltf::image::Format::R32G32B32FLOAT => {
            ensure_expected_bytes(pixel_count, 12, image_data.pixels.len(), "R32G32B32FLOAT")?;
            for channels in image_data.pixels.chunks_exact(12) {
                let red = f32::from_le_bytes([channels[0], channels[1], channels[2], channels[3]]);
                let green = f32::from_le_bytes([channels[4], channels[5], channels[6], channels[7]]);
                let blue = f32::from_le_bytes([channels[8], channels[9], channels[10], channels[11]]);
                rgba8_pixels.extend_from_slice(&[float_to_u8(red), float_to_u8(green), float_to_u8(blue), 255]);
            }
        }
        gltf::image::Format::R32G32B32A32FLOAT => {
            ensure_expected_bytes(pixel_count, 16, image_data.pixels.len(), "R32G32B32A32FLOAT")?;
            for channels in image_data.pixels.chunks_exact(16) {
                let red = f32::from_le_bytes([channels[0], channels[1], channels[2], channels[3]]);
                let green = f32::from_le_bytes([channels[4], channels[5], channels[6], channels[7]]);
                let blue = f32::from_le_bytes([channels[8], channels[9], channels[10], channels[11]]);
                let alpha = f32::from_le_bytes([channels[12], channels[13], channels[14], channels[15]]);
                rgba8_pixels.extend_from_slice(&[float_to_u8(red), float_to_u8(green), float_to_u8(blue), float_to_u8(alpha)]);
            }
        }
    }

    Ok(LoadedTexture {
        width: image_data.width,
        height: image_data.height,
        rgba8Pixels: rgba8_pixels,
    })
}

fn ensure_expected_bytes(pixel_count: usize, bytes_per_pixel: usize, byte_count: usize, format_name: &str) -> Result<()> {
    let expected_byte_count = pixel_count
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| anyhow!("Texture byte count overflow while reading {}", format_name))?;
    if expected_byte_count != byte_count {
        return Err(anyhow!(
            "Texture byte count mismatch for {}: expected {}, got {}",
            format_name,
            expected_byte_count,
            byte_count
        ));
    }
    Ok(())
}

fn float_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn mesh_world_transform_for_node(node_transforms: &HashMap<usize, [[f32; 4]; 4]>, node_index: usize) -> [[f32; 4]; 4] {
    node_transforms.get(&node_index).copied().unwrap_or(identity_matrix())
}

fn compute_node_world_transforms(document: &gltf::Document) -> HashMap<usize, [[f32; 4]; 4]> {
    let mut transforms = HashMap::new();
    for scene in document.scenes() {
        for node in scene.nodes() {
            compute_node_transform_recursive(&node, &identity_matrix(), &mut transforms);
        }
    }
    transforms
}

fn compute_node_transform_recursive(node: &gltf::Node, parent_transform: &[[f32; 4]; 4], transforms: &mut HashMap<usize, [[f32; 4]; 4]>) {
    let local_transform = node.transform().matrix();
    let world_transform = multiply_matrices(parent_transform, &local_transform);
    transforms.insert(node.index(), world_transform);
    for child in node.children() {
        compute_node_transform_recursive(&child, &world_transform, transforms);
    }
}

fn load_animations_for_skeleton(document: &gltf::Document, buffers: &[gltf::buffer::Data], loaded_skeleton: &LoadedSkeleton) -> Vec<LoadedAnimation> {
    let jointIndexOfGltfNodeIndex = &loaded_skeleton.jointIndexOfGltfNodeIndex;
    let mut animations: Vec<LoadedAnimation> = Vec::new();

    for anim in document.animations() {
        let mut channels: Vec<LoadedAnimationChannel> = Vec::new();
        let mut frame_times = Vec::new();
        let mut max_time = 0.0f32;
        let mut interpolationStyle = InterpolationStyle::Step;

        for channel in anim.channels() {
            let target_node_idx = channel.target().node().index();
            let joint_idx = match jointIndexOfGltfNodeIndex.get(&target_node_idx).copied() {
                Some(joint_index) => joint_index,
                None => continue,
            };

            let sampler = channel.sampler();
            let reader = channel.reader(|buffer| Some(&buffers[buffer.index()]));

            let times: Vec<f32> = match reader.read_inputs() {
                Some(iter) => iter.collect(),
                None => continue,
            };
            if times.is_empty() {
                continue;
            }
            if let Some(&last_time) = times.last() {
                max_time = max_time.max(last_time);
            }
            frame_times.extend(times.iter().copied());

            let (target, raw_values): (LoadedAnimationChannelTarget, Vec<[f32; 4]>) = match reader.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Translations(iter)) => (
                    LoadedAnimationChannelTarget::Translation,
                    iter.map(|translation| [translation[0], translation[1], translation[2], 0.0]).collect(),
                ),
                Some(gltf::animation::util::ReadOutputs::Rotations(iter)) => (LoadedAnimationChannelTarget::Rotation, iter.into_f32().collect()),
                Some(gltf::animation::util::ReadOutputs::Scales(_)) => continue,
                _ => continue,
            };

            let values: Vec<[f32; 4]> = match sampler.interpolation() {
                gltf::animation::Interpolation::Step => raw_values,
                gltf::animation::Interpolation::Linear => {
                    interpolationStyle = InterpolationStyle::Linear;
                    raw_values
                }
                gltf::animation::Interpolation::CubicSpline => {
                    interpolationStyle = InterpolationStyle::Linear;
                    if raw_values.len() % 3 != 0 {
                        continue;
                    }

                    raw_values.chunks_exact(3).map(|cubic_triplet| cubic_triplet[1]).collect()
                }
            };

            if times.len() != values.len() {
                continue;
            }

            channels.push(LoadedAnimationChannel {
                appliesToJointIndex: joint_idx,
                target,
                times,
                values,
            });
        }

        if channels.is_empty() {
            continue;
        }

        frame_times.sort_by(|a, b| a.total_cmp(b));
        frame_times.dedup_by(|a, b| (*a - *b).abs() <= f32::EPSILON);

        let mut animated_joint_indices = Vec::new();
        for channel in &channels {
            if !animated_joint_indices.contains(&channel.appliesToJointIndex) {
                animated_joint_indices.push(channel.appliesToJointIndex);
            }
        }
        animated_joint_indices.sort_unstable();

        // Keep import simple: source key times become frames, and runtime interpolation handles the space between frames.
        let mut skKeyframes = Vec::with_capacity(frame_times.len());
        for time in frame_times {
            let mut skKeyframePoses = Vec::with_capacity(animated_joint_indices.len());
            for joint_index in &animated_joint_indices {
                let Some(joint) = loaded_skeleton.jointOfIndex.get(*joint_index) else {
                    continue;
                };
                let Location3D {
                    position: rest_position,
                    quaternion: rest_rotation,
                } = computeJointDefaultLocalLocation3D(joint);
                let mut pose = Location3D {
                    position: rest_position,
                    quaternion: rest_rotation,
                };
                for channel in &channels {
                    if channel.appliesToJointIndex != *joint_index {
                        continue;
                    }

                    let Some(value_index) = channel.times.iter().position(|channel_time| (*channel_time - time).abs() <= f32::EPSILON) else {
                        continue;
                    };
                    let value = channel.values[value_index];
                    match channel.target {
                        LoadedAnimationChannelTarget::Translation => {
                            pose.position = Vec3::new(value[0], value[1], value[2]);
                        }
                        LoadedAnimationChannelTarget::Rotation => {
                            pose.quaternion = Quat::from_xyzw(value[0], value[1], value[2], value[3]).normalize();
                        }
                    }
                }
                // glTF channels hold the joint's whole local pose, but keyframes store the offset from its rest pose
                let rest_rotation_inverse = rest_rotation.inverse();
                skKeyframePoses.push(RenderSkKeyframePose {
                    jointName: joint.name.clone(),
                    localPose: Location3D {
                        position: rest_rotation_inverse * (pose.position - rest_position),
                        quaternion: (rest_rotation_inverse * pose.quaternion).normalize(),
                    },
                });
            }
            skKeyframes.push(RenderSkKeyframe { time, skKeyframePoses });
        }

        let animation_name = anim.name().map(str::trim).filter(|name| !name.is_empty()).unwrap_or("animation");
        animations.push(LoadedAnimation {
            name: animation_name.to_string(),
            sk_animation_info: SkAnimationInfo {
                duration: max_time,
                interpolationStyle,
            },
            sk_keyframes: skKeyframes,
        });
    }

    animations
}

fn load_skeleton(skeleton_index: usize, document: &gltf::Document, buffers: &[gltf::buffer::Data]) -> LoadedSkeleton {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum JointVisitState {
        Unvisited,
        Visiting,
        Visited,
    }

    fn push_joint(
        joint_name: &str,
        parent_name_of_name: &HashMap<String, Option<String>>,
        rest_pose_of_name: &HashMap<String, Location3D>,
        joint_visit_state_of_name: &mut HashMap<String, JointVisitState>,
        joint_index_of_name: &mut HashMap<String, usize>,
        joints: &mut Vec<RenderSkJoint>,
    ) {
        match joint_visit_state_of_name.get(joint_name).copied().unwrap_or(JointVisitState::Unvisited) {
            JointVisitState::Visited => return,
            JointVisitState::Visiting => return,
            JointVisitState::Unvisited => {}
        }

        let Some(rest_pose) = rest_pose_of_name.get(joint_name) else {
            return;
        };
        joint_visit_state_of_name.insert(joint_name.to_string(), JointVisitState::Visiting);

        let parent_joint_index = match parent_name_of_name.get(joint_name).and_then(|parent_name| parent_name.as_ref()) {
            Some(parent_name) => {
                push_joint(
                    parent_name,
                    parent_name_of_name,
                    rest_pose_of_name,
                    joint_visit_state_of_name,
                    joint_index_of_name,
                    joints,
                );
                joint_index_of_name.get(parent_name).copied()
            }
            None => None,
        };

        let joint_index = joints.len();
        joints.push(RenderSkJoint {
            name: joint_name.to_string(),
            parent_joint_index,
            rest_pose: rest_pose.clone(),
        });
        joint_index_of_name.insert(joint_name.to_string(), joint_index);
        joint_visit_state_of_name.insert(joint_name.to_string(), JointVisitState::Visited);
    }

    let skin = document
        .skins()
        .find(|skin| skin.index() == skeleton_index)
        .expect("skin index should exist while loading skeleton");

    let gltfJoints: Vec<_> = skin.joints().collect();
    let inverseBindMatrixOfGltfJointIndex: Vec<[f32; 16]> = skin
        .reader(|buffer| Some(&buffers[buffer.index()]))
        .read_inverse_bind_matrices()
        .map(|iter| iter.map(|matrix| Mat4::from_cols_array_2d(&matrix).to_cols_array()).collect())
        .unwrap_or_default();
    let mut parentNodeIndexOfNodeIndex: HashMap<usize, usize> = HashMap::new();
    for node in document.nodes() {
        for child in node.children() {
            parentNodeIndexOfNodeIndex.insert(child.index(), node.index());
        }
    }

    let mut joint_name_count_of_base_name = HashMap::new();
    let mut jointNameOfGltfJointIndex = Vec::with_capacity(gltfJoints.len());
    let mut jointNameOfNodeIndex = HashMap::with_capacity(gltfJoints.len());
    for joint in &gltfJoints {
        let base_joint_name = joint.name().unwrap_or("joint").to_string();
        let joint_name_count = joint_name_count_of_base_name.entry(base_joint_name.clone()).or_insert(0);
        *joint_name_count += 1;
        let joint_name = if *joint_name_count == 1 {
            base_joint_name
        } else {
            format!("{base_joint_name}-{}", *joint_name_count)
        };
        jointNameOfNodeIndex.insert(joint.index(), joint_name.clone());
        jointNameOfGltfJointIndex.push(joint_name);
    }

    let mut restPoseOfName = HashMap::with_capacity(gltfJoints.len());
    let mut parentNameOfName = HashMap::with_capacity(gltfJoints.len());
    for joint in &gltfJoints {
        let (translation, rotation, _scale) = joint.transform().decomposed();
        let Some(joint_name) = jointNameOfNodeIndex.get(&joint.index()) else {
            continue;
        };
        let parent_name = parentNodeIndexOfNodeIndex
            .get(&joint.index())
            .and_then(|parent_node_index| jointNameOfNodeIndex.get(parent_node_index))
            .cloned();

        restPoseOfName.insert(
            joint_name.clone(),
            Location3D {
                position: Vec3::from_array(translation),
                quaternion: Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
            },
        );
        parentNameOfName.insert(joint_name.clone(), parent_name);
    }

    let mut joint_names = restPoseOfName.keys().map(String::as_str).collect::<Vec<_>>();
    joint_names.sort_unstable();
    let mut joint_visit_state_of_name = HashMap::new();
    let mut joint_index_of_name = HashMap::new();
    let mut joints = Vec::with_capacity(gltfJoints.len());
    for joint_name in joint_names {
        push_joint(
            joint_name,
            &parentNameOfName,
            &restPoseOfName,
            &mut joint_visit_state_of_name,
            &mut joint_index_of_name,
            &mut joints,
        );
    }
    let jointIndexOfName = computeJointIndexOfName(&joints);
    let mut inverseBindMatrixOfJointName = HashMap::with_capacity(jointNameOfGltfJointIndex.len());
    for (gltf_joint_index, joint_name) in jointNameOfGltfJointIndex.iter().enumerate() {
        let Some(inverse_bind_matrix) = inverseBindMatrixOfGltfJointIndex.get(gltf_joint_index) else {
            continue;
        };
        inverseBindMatrixOfJointName.insert(joint_name.clone(), *inverse_bind_matrix);
    }

    let mut jointIndexOfGltfNodeIndex: HashMap<usize, usize> = HashMap::with_capacity(gltfJoints.len());
    for joint in &gltfJoints {
        let Some(joint_name) = jointNameOfNodeIndex.get(&joint.index()) else {
            continue;
        };
        let Some(&joint_index) = jointIndexOfName.get(joint_name) else {
            continue;
        };
        jointIndexOfGltfNodeIndex.insert(joint.index(), joint_index);
    }

    LoadedSkeleton {
        jointOfIndex: joints,
        jointNameOfGltfJointIndex,
        jointIndexOfGltfNodeIndex,
        inverseBindMatrixOfJointName,
    }
}

fn identity_matrix() -> [[f32; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

fn multiply_matrices(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                result[i][j] += a[k][j] * b[i][k];
            }
        }
    }
    result
}

fn apply_transform_to_vertices(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    tangents: &[[f32; 4]],
    transform: &[[f32; 4]; 4],
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 4]>) {
    let mut transformed_positions = Vec::with_capacity(positions.len());
    let mut transformed_normals = Vec::with_capacity(normals.len());
    let mut transformed_tangents = Vec::with_capacity(tangents.len());

    let transform_matrix = Mat4::from_cols_array_2d(transform);
    let tangent_matrix = Mat3::from_mat4(transform_matrix);
    let normal_matrix = if tangent_matrix.determinant().abs() > 1e-8 {
        tangent_matrix.inverse().transpose()
    } else {
        tangent_matrix
    };

    for pos in positions {
        let position = Vec3::from_array(*pos);
        let transformed_position = transform_matrix * position.extend(1.0);
        transformed_positions.push([transformed_position.x, transformed_position.y, transformed_position.z]);
    }

    for norm in normals {
        let normal = Vec3::from_array(*norm);
        let transformed_normal = normal_matrix * normal;
        if transformed_normal.length_squared() > 1e-12 {
            transformed_normals.push(transformed_normal.normalize().to_array());
        } else {
            transformed_normals.push([0.0, 1.0, 0.0]);
        }
    }

    for tangent in tangents {
        let tangent_xyz = Vec3::new(tangent[0], tangent[1], tangent[2]);
        let transformed_tangent = tangent_matrix * tangent_xyz;
        let tangent_sign = if tangent[3] < 0.0 { -1.0 } else { 1.0 };
        if transformed_tangent.length_squared() > 1e-12 {
            let normalized = transformed_tangent.normalize();
            transformed_tangents.push([normalized.x, normalized.y, normalized.z, tangent_sign]);
        } else {
            transformed_tangents.push([1.0, 0.0, 0.0, tangent_sign]);
        }
    }

    (transformed_positions, transformed_normals, transformed_tangents)
}

// glTF can include author-provided normals, so this runs only as an import fallback when normals are missing.
// We do it once during conversion so MeshData stays renderer-ready and we avoid per-frame recomputation.
fn compute_smooth_normals(vertices: &[[f32; 3]], indices: &[u32], normals: &mut Vec<[f32; 3]>) {
    if normals.len() != vertices.len() {
        normals.resize(vertices.len(), [0.0, 1.0, 0.0]);
    }
    for normal in normals.iter_mut() {
        *normal = [0.0, 0.0, 0.0];
    }

    for tri in indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
            continue;
        }
        let v0 = vertices[i0];
        let v1 = vertices[i1];
        let v2 = vertices[i2];

        let edge1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let edge2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let face_normal = [
            edge1[1] * edge2[2] - edge1[2] * edge2[1],
            edge1[2] * edge2[0] - edge1[0] * edge2[2],
            edge1[0] * edge2[1] - edge1[1] * edge2[0],
        ];

        for idx in [i0, i1, i2] {
            normals[idx][0] += face_normal[0];
            normals[idx][1] += face_normal[1];
            normals[idx][2] += face_normal[2];
        }
    }

    for normal in normals.iter_mut() {
        let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if len > 1e-6 {
            normal[0] /= len;
            normal[1] /= len;
            normal[2] /= len;
        } else {
            *normal = [0.0, 1.0, 0.0];
        }
    }
}

// glTF can include tangents too, so this is also only a fallback when tangents are missing.
// We derive tangent space from triangle geometry + UVs + normals during import so shading has stable TBN data.
fn compute_tangents(positions: &[[f32; 3]], uvs: &[[f32; 2]], normals: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 4]> {
    let mut tangent_sum_of_vertex_index: Vec<Vec3> = vec![Vec3::ZERO; positions.len()];
    let mut bitangent_sum_of_vertex_index: Vec<Vec3> = vec![Vec3::ZERO; positions.len()];

    for triangle in indices.chunks_exact(3) {
        let i0 = triangle[0] as usize;
        let i1 = triangle[1] as usize;
        let i2 = triangle[2] as usize;
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        if i0 >= uvs.len() || i1 >= uvs.len() || i2 >= uvs.len() {
            continue;
        }

        let p0 = Vec3::from_array(positions[i0]);
        let p1 = Vec3::from_array(positions[i1]);
        let p2 = Vec3::from_array(positions[i2]);
        let uv0 = Vec2::from_array(uvs[i0]);
        let uv1 = Vec2::from_array(uvs[i1]);
        let uv2 = Vec2::from_array(uvs[i2]);

        let edge1 = p1 - p0;
        let edge2 = p2 - p0;
        let delta_uv1 = uv1 - uv0;
        let delta_uv2 = uv2 - uv0;
        let determinant = delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x;
        if determinant.abs() < 1e-8 {
            continue;
        }

        let inverse_determinant = 1.0 / determinant;
        let triangle_tangent = (edge1 * delta_uv2.y - edge2 * delta_uv1.y) * inverse_determinant;
        let triangle_bitangent = (edge2 * delta_uv1.x - edge1 * delta_uv2.x) * inverse_determinant;

        for i in [i0, i1, i2] {
            tangent_sum_of_vertex_index[i] += triangle_tangent;
            bitangent_sum_of_vertex_index[i] += triangle_bitangent;
        }
    }

    let mut tangents: Vec<[f32; 4]> = Vec::with_capacity(positions.len());
    for (vertex_index, tangent_sum) in tangent_sum_of_vertex_index.into_iter().enumerate() {
        let base_normal = normals.get(vertex_index).copied().map(Vec3::from_array).unwrap_or(Vec3::Y);
        let normal = if base_normal.length_squared() > 1e-12 {
            base_normal.normalize()
        } else {
            Vec3::Y
        };

        let mut tangent = tangent_sum - normal * tangent_sum.dot(normal);
        if tangent.length_squared() <= 1e-12 {
            let fallback_axis = if normal.z.abs() < 0.999 { Vec3::Z } else { Vec3::X };
            tangent = fallback_axis.cross(normal);
        }
        if tangent.length_squared() <= 1e-12 {
            tangent = Vec3::X;
        }
        tangent = tangent.normalize();

        let bitangent_sum = bitangent_sum_of_vertex_index[vertex_index];
        let handedness = if normal.cross(tangent).dot(bitangent_sum) < 0.0 { -1.0 } else { 1.0 };

        tangents.push([tangent.x, tangent.y, tangent.z, handedness]);
    }

    tangents
}
