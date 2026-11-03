//! Validation for the model files the editor stores, and the limits the worker shares with it.
mod validation;
pub use validation::validate_glb;

use serde::{Deserialize, Serialize};

// TODO: AI-PICKED-VALUE: Bound model files, and the images the AI looks at, before parsing them.
pub const MAX_GLB_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BLEND_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
// Claude refuses every image over 2000 pixels on a side once a request has more than 20 images.
pub const MAX_AI_IMAGE_SIDE: u32 = 2000;

// A PNG bigger than MAX_AI_IMAGE_SIDE on a side is shrunk to fit it, keeping its shape. Returns the PNG and its size.
pub fn shrink_png_for_ai(png: Vec<u8>, width: u32, height: u32) -> anyhow::Result<(Vec<u8>, u32, u32)> {
    if width <= MAX_AI_IMAGE_SIDE && height <= MAX_AI_IMAGE_SIDE {
        return Ok((png, width, height));
    }
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.resize(
        MAX_AI_IMAGE_SIDE,
        MAX_AI_IMAGE_SIDE,
        image::imageops::FilterType::Lanczos3,
    );
    let mut shrunk_png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut shrunk_png), image::ImageFormat::Png)?;
    Ok((shrunk_png, image.width(), image.height()))
}

// Saved sources are reopened only inside the sandbox, with script auto-execution disabled.
pub fn validate_blend(bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        bytes.starts_with(b"BLENDER") && bytes.len() <= MAX_BLEND_BYTES,
        "Invalid or oversized uncompressed Blender source file."
    );
    Ok(())
}

// An editable model source: the .blend kube.new's older Blender tools saved, or the .glb someone uploaded.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshSourceFormat {
    Blend,
    Glb,
}

impl MeshSourceFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Blend => "blend",
            Self::Glb => "glb",
        }
    }

    pub fn max_bytes(self) -> usize {
        match self {
            Self::Blend => MAX_BLEND_BYTES,
            Self::Glb => MAX_GLB_BYTES,
        }
    }

    pub fn validate(self, bytes: &[u8]) -> anyhow::Result<()> {
        match self {
            Self::Blend => validate_blend(bytes),
            Self::Glb => validate_glb(bytes),
        }
    }
}
