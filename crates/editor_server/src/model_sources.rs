//! A model's source file: the .glb it was uploaded as, or the .blend kube.new's older Blender tools saved. An AI downloads
//! it to edit the model.
use crate::database;
use anyhow::{Context, Result, ensure};
use linux_sandbox::MeshSourceFormat;
use render_core::dehydrated_gltf::BlenderId;
use shared::s3_storage::{PrivateFile, download_private_file, upload_private_file};

// Every source is saved under an id of its own, and only its owner can read it back.
pub async fn save_model_source(profile_id: &str, source_format: MeshSourceFormat, bytes: Vec<u8>) -> Result<BlenderId> {
    let blender_id = database::next_blender_id().await?;
    upload_private_file(&PrivateFile::Blender { blender_id }, bytes).await?;
    database::insert_model_source(blender_id, profile_id, source_format).await?;
    Ok(blender_id)
}

// Only the owner can read a private source.
pub async fn download_model_source(profile_id: &str, blender_id: BlenderId) -> Result<(MeshSourceFormat, Vec<u8>)> {
    let source_format = database::model_source_format(blender_id, profile_id)
        .await?
        .context("Blender source not found or not yours.")?;
    let bytes = download_private_file(&PrivateFile::Blender { blender_id }).await?;
    ensure!(bytes.len() <= source_format.max_bytes(), "Blender source is too large.");
    source_format.validate(&bytes)?;
    Ok((source_format, bytes))
}
