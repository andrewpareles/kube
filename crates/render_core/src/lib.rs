#![allow(non_snake_case)]
//! Rendering code used by both the native client and WebGPU previews.

pub mod camera;
pub mod collect_animation_content;
pub mod computeBlendedWeights;
pub mod computePropertyValue;
pub mod computeSkeletonInfo;
pub mod computeSkeletonPose;
pub mod create_gpu_primitive_shape;
pub mod create_gpu_texture;
pub mod culling;
pub mod dehydrated_gltf;
pub mod dehydrated_gltf_to_stored_world;
pub mod detail_levels;
pub mod fonts;
pub mod label_text_mesh;
pub mod material_to_stored_world;
pub mod picking;
pub mod preview_lighting;
pub mod render_scene;
pub mod renderable;
pub mod renderer;
pub mod renderer_types;
pub mod skybox_to_stored_world;
