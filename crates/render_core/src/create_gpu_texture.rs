use crate::renderer::GpuTextureData;
use anyhow::Result;

// Textures stay block compressed on the GPU when the device supports it: ASTC on phones and Apple GPUs, BC7 elsewhere.
// Both use 4x4 blocks, and a block compressed texture must be a whole number of blocks, so other sizes stay RGBA.
pub fn choose_texture_upload_format(features: wgpu::Features, width: u32, height: u32, is_srgb: bool, has_alpha: bool) -> wgpu::TextureFormat {
    let is_whole_blocks = width % 4 == 0 && height % 4 == 0;
    if is_whole_blocks && features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC) {
        return wgpu::TextureFormat::Astc {
            block: wgpu::AstcBlock::B4x4,
            channel: wgpu::AstcChannel::Unorm,
        };
    }
    if is_whole_blocks && features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
        // Color maps are stored as ETC1S, which BC1 holds at half the size of BC7 when there is no alpha.
        return if is_srgb && !has_alpha {
            wgpu::TextureFormat::Bc1RgbaUnorm
        } else {
            wgpu::TextureFormat::Bc7RgbaUnorm
        };
    }
    wgpu::TextureFormat::Rgba8Unorm
}

// A block compressed mip smaller than one block still takes up a whole block.
pub fn mip_level_byte_count(upload_format: wgpu::TextureFormat, width: u32, height: u32, mip_level: u32) -> Option<usize> {
    let mip_size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
    .mip_level_size(mip_level, wgpu::TextureDimension::D2)
    .physical_size(upload_format);
    let (block_width, block_height) = upload_format.block_dimensions();
    let block_byte_count = upload_format.block_copy_size(None)?;
    usize::try_from(u64::from(mip_size.width / block_width) * u64::from(mip_size.height / block_height) * u64::from(block_byte_count)).ok()
}

pub fn create_gpu_texture_from_mip_data(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mip_data: &[Vec<u8>],
    width: u32,
    height: u32,
    upload_format: wgpu::TextureFormat,
    is_srgb: bool,
    label: &str,
) -> Result<GpuTextureData> {
    let texture_format = if is_srgb { upload_format.add_srgb_suffix() } else { upload_format };
    let mip_level_count = mip_data.len() as u32;
    if mip_level_count == 0 {
        return Err(anyhow::anyhow!("Texture has no mip levels"));
    }
    let Some(block_byte_count) = upload_format.block_copy_size(None) else {
        return Err(anyhow::anyhow!("Texture format {:?} cannot be uploaded", upload_format));
    };
    let (block_width, block_height) = upload_format.block_dimensions();

    let texture_size = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    // WebGL2 cannot view one texture as two formats, so the WebGL build keeps a second copy of a color image for its raw
    // channel values.
    let should_upload_linear_copy = cfg!(feature = "webgl") && is_srgb;
    let texture_descriptor = wgpu::TextureDescriptor {
        label: Some(label),
        size: texture_size,
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: texture_format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        // Color and material data can share an image without sharing its color conversion.
        view_formats: if should_upload_linear_copy { &[] } else { &[upload_format] },
    };
    let texture = device.create_texture(&texture_descriptor);
    let linear_texture = should_upload_linear_copy.then(|| {
        device.create_texture(&wgpu::TextureDescriptor {
            format: upload_format,
            ..texture_descriptor
        })
    });

    for (mip_level_index, mip_level_data) in mip_data.iter().enumerate() {
        let mip_level = mip_level_index as u32;
        let Some(mip_byte_count) = mip_level_byte_count(upload_format, texture_size.width, texture_size.height, mip_level) else {
            return Err(anyhow::anyhow!("Texture byte count overflow while uploading mip level {}", mip_level_index));
        };
        if mip_level_data.len() != mip_byte_count {
            return Err(anyhow::anyhow!(
                "Texture mip level {} byte count mismatch: expected {}, got {}",
                mip_level_index,
                mip_byte_count,
                mip_level_data.len(),
            ));
        }

        let mip_size = texture_size.mip_level_size(mip_level, wgpu::TextureDimension::D2).physical_size(upload_format);
        for texture in std::iter::once(&texture).chain(&linear_texture) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                mip_level_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(mip_size.width / block_width * block_byte_count),
                    rows_per_image: Some(mip_size.height / block_height),
                },
                mip_size,
            );
        }
    }

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Numeric maps must bypass sRGB decoding before filtering, rather than undoing it in the shader.
    let linear_texture_view = linear_texture.as_ref().unwrap_or(&texture).create_view(&wgpu::TextureViewDescriptor {
        format: Some(upload_format),
        ..Default::default()
    });
    Ok(GpuTextureData {
        _texture: texture,
        texture_view,
        linear_texture_view,
    })
}
