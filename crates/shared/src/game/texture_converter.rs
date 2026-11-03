use anyhow::{Result, bail, ensure};
use image::{ImageFormat, ImageReader, RgbaImage, imageops::FilterType};
use ktx2_rw::{BasisCompressionParams, Ktx2Texture, VkFormat};
use std::io::Cursor;

use crate::game::gltf_converter::LoadedTexture;
use game_types::data_types::TextureData;

// TODO: AI-PICKED-VALUE: libktx's KTX_PACK_UASTC_LEVEL_DEFAULT. ktx2-rw's default of 0 is the fastest level, which
// still left visible 4x4 blocks in normal maps when tested.
const UASTC_QUALITY_LEVEL: u32 = 2;
// TODO: AI-PICKED-VALUE: Other platforms' texture guide suggests 256 pixels for normal, roughness, and metalness maps. UASTC
// costs a byte per pixel whatever the image holds, so a 1024-pixel data map is 1.3 MB and a 256-pixel one about 85 KB.
const MAX_DATA_MAP_SIDE_PIXELS: u32 = 256;
// TODO: AI-PICKED-VALUE: The smallest size that fills one 4x4 block of every GPU compressed format.
const FLAT_DATA_MAP_SIDE_PIXELS: u32 = 4;

// This shared helper is used by both glTF imports and standalone image uploads.
pub fn convert_rgba8_pixels_to_texture_data(mut mip_rgba8_pixels: Vec<u8>, mut width: u32, mut height: u32, is_srgb: bool) -> Result<TextureData> {
    // Data maps are stored small, since UASTC costs the same for every pixel. A map that is one value everywhere keeps
    // only a few pixels, and the rest are scaled down, since bumps and shine hold up at low resolution.
    if !is_srgb {
        if let Some(first_pixel) = mip_rgba8_pixels.get(..4).map(<[u8]>::to_vec)
            && width * height > FLAT_DATA_MAP_SIDE_PIXELS * FLAT_DATA_MAP_SIDE_PIXELS
            && mip_rgba8_pixels.chunks_exact(4).all(|pixel| pixel == first_pixel.as_slice())
        {
            mip_rgba8_pixels = first_pixel.repeat((FLAT_DATA_MAP_SIDE_PIXELS * FLAT_DATA_MAP_SIDE_PIXELS) as usize);
            (width, height) = (FLAT_DATA_MAP_SIDE_PIXELS, FLAT_DATA_MAP_SIDE_PIXELS);
        } else if width.max(height) > MAX_DATA_MAP_SIDE_PIXELS {
            let scale = MAX_DATA_MAP_SIDE_PIXELS as f32 / width.max(height) as f32;
            let scaled_width = ((width as f32 * scale).round() as u32).max(1);
            let scaled_height = ((height as f32 * scale).round() as u32).max(1);
            let image = RgbaImage::from_raw(width, height, mip_rgba8_pixels).ok_or_else(|| anyhow::anyhow!("Data map pixels do not fill {width}x{height}"))?;
            mip_rgba8_pixels = image::imageops::resize(&image, scaled_width, scaled_height, FilterType::Gaussian).into_raw();
            (width, height) = (scaled_width, scaled_height);
        }
    }

    let max_dimension = width.max(height).max(1);
    let mipLevelCount = u32::BITS - max_dimension.leading_zeros();
    let hasAlpha = mip_rgba8_pixels.chunks_exact(4).any(|pixel| pixel[3] < 255);
    let texture_vk_format = if is_srgb { VkFormat::R8G8B8A8_SRGB } else { VkFormat::R8G8B8A8_UNORM };

    let mut ktx2_texture = Ktx2Texture::create(width, height, 1, 1, 1, mipLevelCount, texture_vk_format)
        .map_err(|error| anyhow::anyhow!("Failed to create KTX2 texture: {}", error))?;

    let mut mip_width = width.max(1);
    let mut mip_height = height.max(1);
    for mip_level in 0..mipLevelCount {
        ktx2_texture
            .set_image_data(mip_level, 0, 0, &mip_rgba8_pixels)
            .map_err(|error| anyhow::anyhow!("Failed to set KTX2 mip level {} pixels: {}", mip_level, error))?;

        if mip_level + 1 >= mipLevelCount {
            continue;
        }

        let Some(mip_image) = RgbaImage::from_raw(mip_width, mip_height, mip_rgba8_pixels) else {
            return Err(anyhow::anyhow!(
                "Failed to convert mip level {} pixels into image buffer ({}x{})",
                mip_level,
                mip_width,
                mip_height
            ));
        };
        let next_mip_width = (mip_width / 2).max(1);
        let next_mip_height = (mip_height / 2).max(1);
        let next_mip_image = image::imageops::resize(&mip_image, next_mip_width, next_mip_height, FilterType::Gaussian);

        mip_rgba8_pixels = next_mip_image.into_raw();
        mip_width = next_mip_width;
        mip_height = next_mip_height;
    }

    // libktx's thread pool can freeze forever while it shuts down, most often after a tiny texture, so compression uses one thread.
    // ETC1S keeps one color per 4x4 block, which flattens the separate channels of normal, roughness, and metallic
    // maps into blocky facets. Those maps use UASTC; color keeps the much smaller ETC1S.
    let compression_params = BasisCompressionParams::builder()
        .thread_count(1)
        .uastc(!is_srgb)
        .uastc_flags(UASTC_QUALITY_LEVEL)
        .build();
    ktx2_texture
        .compress_basis(&compression_params)
        .map_err(|error| anyhow::anyhow!("Failed to Basis-compress KTX2 texture: {}", error))?;

    let ktx2Data = ktx2_texture
        .write_to_memory()
        .map_err(|error| anyhow::anyhow!("Failed to write KTX2 texture to memory: {}", error))?;

    Ok(TextureData {
        ktx2Data,
        width,
        height,
        mipLevelCount,
        isSrgb: is_srgb,
        hasAlpha,
    })
}

const MAX_UPLOADED_TEXTURE_SIDE_PIXELS: u32 = 2048;
// TODO: AI-PICKED-VALUE: A skybox may be as wide as 4096 pixels, the size of the photoreal built-in skies.
const MAX_UPLOADED_SKYBOX_WIDTH_PIXELS: u32 = 4096;

// Uploaded images are PNG, JPEG, or WebP, and their size is read from the header before any pixels are decoded.
fn read_uploaded_image_header(image_bytes: &[u8]) -> Result<(ImageFormat, u32, u32)> {
    let image_reader = ImageReader::new(Cursor::new(image_bytes)).with_guessed_format()?;
    let image_format = match image_reader.format() {
        Some(image_format @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)) => image_format,
        _ => bail!("Images must be PNG, JPEG, or WebP."),
    };
    let (width, height) = image_reader.into_dimensions()?;
    Ok((image_format, width, height))
}

// Every uploaded image is checked and read into pixels the same way, whether it becomes a color map or a data map.
pub fn convert_image_bytes_to_loaded_texture(image_bytes: &[u8]) -> Result<LoadedTexture> {
    let (image_format, width, height) = read_uploaded_image_header(image_bytes)?;
    ensure!(
        width > 0 && height > 0 && width <= MAX_UPLOADED_TEXTURE_SIDE_PIXELS && height <= MAX_UPLOADED_TEXTURE_SIDE_PIXELS,
        "Textures must be at most {MAX_UPLOADED_TEXTURE_SIDE_PIXELS} pixels per side."
    );
    let rgba8Pixels = image::load_from_memory_with_format(image_bytes, image_format)?.into_rgba8().into_raw();
    Ok(LoadedTexture { rgba8Pixels, width, height })
}

// Image files people upload become color textures. The size is read from the header first, so an
// oversized image is refused before its pixels are decoded.
pub fn convert_color_image_bytes_to_texture_data(image_bytes: &[u8]) -> Result<TextureData> {
    let LoadedTexture { rgba8Pixels, width, height } = convert_image_bytes_to_loaded_texture(image_bytes)?;
    convert_rgba8_pixels_to_texture_data(rgba8Pixels, width, height, true)
}

// A skybox is an equirectangular panorama: 360 degrees across and 180 degrees top to bottom, so twice as wide as it is tall.
// Any other shape would come out stretched around the sky.
pub fn convert_skybox_image_bytes_to_texture_data(image_bytes: &[u8]) -> Result<TextureData> {
    let (image_format, width, height) = read_uploaded_image_header(image_bytes)?;
    ensure!(
        width <= MAX_UPLOADED_SKYBOX_WIDTH_PIXELS,
        "Skyboxes must be at most {MAX_UPLOADED_SKYBOX_WIDTH_PIXELS} x {} pixels.",
        MAX_UPLOADED_SKYBOX_WIDTH_PIXELS / 2
    );
    ensure!(
        height > 0 && width == 2 * height,
        "A skybox must be a panorama twice as wide as it is tall, like 2048 x 1024. This image is {width} x {height}."
    );
    let rgba8Pixels = image::load_from_memory_with_format(image_bytes, image_format)?.into_rgba8().into_raw();
    convert_rgba8_pixels_to_texture_data(rgba8Pixels, width, height, true)
}
