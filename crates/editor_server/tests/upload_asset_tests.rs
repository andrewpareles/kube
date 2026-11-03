#[path = "../src/asset_uploads.rs"]
mod asset_uploads;

use game_types::object_types::material::AlphaMode;
use asset_uploads::{convert_uploaded_asset_to_publish_data, convert_uploaded_material_to_publish_data};
use image::{ImageFormat, RgbaImage};
use serde_json::{Value, json};
use shared::{
    api::protocol_for_assets::AssetPublishData,
    game::{
        assets::AssetType,
        gltf_converter::{LoadedMeshData, LoadedTexture},
    },
};
use std::io::Cursor;

// The first Ogg page as an encoder writes it: the page header, one segment, then the packet.
fn ogg_page(packet: &[u8]) -> Vec<u8> {
    let mut page = b"OggS".to_vec();
    page.extend([0, 2]);
    page.extend([0; 20]);
    page.extend([1, packet.len() as u8]);
    page.extend(packet);
    page
}

fn silent_wav(seconds: f64) -> Vec<u8> {
    let sample_rate: u32 = 8000;
    let data_len = (seconds * f64::from(sample_rate)) as u32 * 2;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data_len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(sample_rate.to_le_bytes());
    wav.extend((sample_rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(data_len.to_le_bytes());
    wav.extend(vec![0; data_len as usize]);
    wav
}

fn png(width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    RgbaImage::from_pixel(width, height, image::Rgba([200, 40, 10, 255])).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn glb(document: Value, binary: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(&document).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut data = binary.to_vec();
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let mut bytes = b"glTF".to_vec();
    for value in [2, (28 + json.len() + data.len()) as u32, json.len() as u32, 0x4e4f534a] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(json);
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(0x004e4942u32.to_le_bytes());
    bytes.extend(data);
    bytes
}

fn triangle_glb() -> Vec<u8> {
    let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let document = json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }],
        "bufferViews": [{ "buffer": 0, "byteLength": positions.len() }],
        "buffers": [{ "byteLength": positions.len() }],
    });
    glb(document, &positions)
}

async fn upload_error(kind: AssetType, name: &str, bytes: Vec<u8>) -> String {
    match convert_uploaded_asset_to_publish_data(kind, name, bytes).await {
        Ok(_) => panic!("The upload was accepted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn an_ogg_vorbis_file_is_kept_byte_for_byte() -> anyhow::Result<()> {
    let bytes = ogg_page(b"\x01vorbis\x00\x00\x00\x00\x02\x44\xac\x00\x00");
    let AssetPublishData::Sfx(sound) = convert_uploaded_asset_to_publish_data(AssetType::Sound, "Beep", bytes.clone()).await? else {
        anyhow::bail!("Expected sound data");
    };
    let bytes_in_wrapper = bincode::serialize(&sound)?;
    assert!(bytes_in_wrapper.ends_with(&bytes), "The sound is stored exactly as uploaded");
    Ok(())
}

// Other formats are stored as Ogg Vorbis at their own rate and channel count, which the Vorbis identification
// header right after the first Ogg page's header records.
#[tokio::test]
async fn an_mp3_wav_or_flac_sound_becomes_ogg_vorbis() -> anyhow::Result<()> {
    let sounds = [
        (include_bytes!("fixtures/upload-sound.mp3").to_vec(), 2, 44100),
        (include_bytes!("fixtures/upload-sound.wav").to_vec(), 1, 22050),
        (include_bytes!("fixtures/upload-sound.flac").to_vec(), 1, 48000),
    ];
    for (bytes, channel_count, sample_rate) in sounds {
        let AssetPublishData::Sfx(sound) = convert_uploaded_asset_to_publish_data(AssetType::Sound, "Beep", bytes).await? else {
            anyhow::bail!("Expected sound data");
        };
        let ogg: Vec<u8> = serde_json::from_value(serde_json::to_value(&sound)?["data"]["oggVorbisData"].take())?;
        let packet_start = 27 + usize::from(ogg[26]);
        assert_eq!(&ogg[packet_start..packet_start + 7], b"\x01vorbis");
        assert_eq!(ogg[packet_start + 11], channel_count);
        assert_eq!(u32::from_le_bytes(ogg[packet_start + 12..packet_start + 16].try_into()?), sample_rate);
    }
    Ok(())
}

#[tokio::test]
async fn a_sound_under_10_seconds_is_sfx_and_a_longer_one_is_sound() -> anyhow::Result<()> {
    let AssetPublishData::Sfx(_) = convert_uploaded_asset_to_publish_data(AssetType::Sound, "Short", silent_wav(9.5)).await? else {
        anyhow::bail!("A 9.5 second sound should be SFX");
    };
    let AssetPublishData::Sound(_) = convert_uploaded_asset_to_publish_data(AssetType::Sound, "Long", silent_wav(10.0)).await? else {
        anyhow::bail!("A 10 second sound should be Sound");
    };
    Ok(())
}

#[tokio::test]
async fn a_sound_that_is_not_vorbis_mp3_wav_or_flac_is_refused() {
    for bytes in [ogg_page(b"OpusHead\x01\x02"), b"ID3\x04\x00 an mp3".to_vec(), b"OggS".to_vec(), Vec::new()] {
        assert!(
            upload_error(AssetType::Sound, "Beep", bytes)
                .await
                .contains("Ogg Vorbis, MP3, WAV, or FLAC")
        );
    }
}

// The JPEG and WebP files are checked in rather than encoded here, so they decode only if the editor's
// own build can read those formats.
#[tokio::test]
async fn a_png_jpeg_or_webp_image_becomes_a_color_texture() -> anyhow::Result<()> {
    let images = [
        png(8, 4)?,
        include_bytes!("fixtures/upload-texture.jpg").to_vec(),
        include_bytes!("fixtures/upload-texture.webp").to_vec(),
    ];
    for bytes in images {
        let AssetPublishData::Texture(texture) = convert_uploaded_asset_to_publish_data(AssetType::Texture, "Brick wall", bytes).await? else {
            anyhow::bail!("Expected texture data");
        };
        let texture = serde_json::to_value(&texture)?;
        assert_eq!((texture["data"]["width"].as_u64(), texture["data"]["height"].as_u64()), (Some(8), Some(4)));
        assert_eq!(texture["data"]["isSrgb"], true);
    }
    Ok(())
}

#[tokio::test]
async fn a_glb_model_becomes_mesh_data() -> anyhow::Result<()> {
    let AssetPublishData::Mesh(gltf) = convert_uploaded_asset_to_publish_data(AssetType::Mesh, "Triangle", triangle_glb()).await? else {
        anyhow::bail!("Expected mesh data");
    };
    let [LoadedMeshData::LoadedStaticMeshData { vertices, indices, .. }] = gltf.meshes.as_slice() else {
        anyhow::bail!("Expected one static mesh");
    };
    assert_eq!((vertices.len(), indices.as_slice()), (3, [0, 1, 2].as_slice()));
    Ok(())
}

// Only the header is read before an oversized image is refused, so its pixels are never decoded.
#[tokio::test]
async fn an_image_over_2048_pixels_on_a_side_or_in_another_format_is_refused() -> anyhow::Result<()> {
    assert!(upload_error(AssetType::Texture, "Wide", png(2049, 1)?).await.contains("2048 pixels"));
    assert!(
        upload_error(AssetType::Texture, "Gif", b"GIF89a\x01\x00\x01\x00".to_vec())
            .await
            .contains("PNG, JPEG, or WebP")
    );
    Ok(())
}

// A skybox wraps around the whole sky, so it must be a 2:1 panorama, and it may be twice as wide as a texture.
#[tokio::test]
async fn a_skybox_is_a_panorama_at_most_4096_pixels_wide() -> anyhow::Result<()> {
    let AssetPublishData::Skybox(texture) = convert_uploaded_asset_to_publish_data(AssetType::Skybox, "Dusk", png(16, 8)?).await? else {
        anyhow::bail!("Expected skybox data");
    };
    let texture = serde_json::to_value(&texture)?;
    assert_eq!((texture["data"]["width"].as_u64(), texture["data"]["height"].as_u64()), (Some(16), Some(8)));
    assert_eq!(texture["data"]["isSrgb"], true);
    assert!(upload_error(AssetType::Skybox, "Square", png(8, 8)?).await.contains("This image is 8 x 8."));
    assert!(upload_error(AssetType::Skybox, "Wide", png(4098, 1)?).await.contains("4096 x 2048"));
    Ok(())
}

#[tokio::test]
async fn a_bad_name_model_or_kind_is_refused() -> anyhow::Result<()> {
    assert!(upload_error(AssetType::Texture, "shit", png(1, 1)?).await.contains("not allowed"));
    assert!(upload_error(AssetType::Texture, " ", png(1, 1)?).await.contains("needs a name"));
    assert!(upload_error(AssetType::Mesh, "Model", b"not a model".to_vec()).await.contains("glTF"));
    assert!(
        upload_error(AssetType::Mesh, "Model", vec![0; 9 * 1024 * 1024])
            .await
            .contains("at most 8 MiB")
    );
    assert!(upload_error(AssetType::Object, "Object", Vec::new()).await.contains("Only models"));
    Ok(())
}

async fn material_upload_error(name: &str, albedo: Vec<u8>, normal: Option<Vec<u8>>) -> String {
    match convert_uploaded_material_to_publish_data(name, albedo, None, None, normal).await {
        Ok(_) => panic!("The material was accepted"),
        Err(error) => error.to_string(),
    }
}

// Each image fills the map it was sent for at its own size, and maps that were not sent stay empty.
#[tokio::test]
async fn a_material_upload_reads_each_image_into_its_own_map() -> anyhow::Result<()> {
    let AssetPublishData::Material(material) = convert_uploaded_material_to_publish_data("Brick wall", png(8, 4)?, None, Some(png(2, 2)?), None).await?
    else {
        anyhow::bail!("Expected material data");
    };
    let size = |texture: &Option<LoadedTexture>| texture.as_ref().map(|texture| (texture.width, texture.height, texture.rgba8Pixels.len()));
    assert_eq!(
        [
            size(&material.albedo),
            size(&material.metallic),
            size(&material.roughness),
            size(&material.normal)
        ],
        [Some((8, 4, 8 * 4 * 4)), None, Some((2, 2, 2 * 2 * 4)), None]
    );
    let is_opaque = match material.alphaMode {
        AlphaMode::Opaque => true,
        AlphaMode::Blend | AlphaMode::Mask { .. } => false,
    };
    assert!(is_opaque && !material.isDoubleSided);
    Ok(())
}

// A color image with see-through pixels uploads as a blending material, so glass or a fence shows through without edits.
#[tokio::test]
async fn a_material_whose_color_image_has_see_through_pixels_blends() -> anyhow::Result<()> {
    let mut image = RgbaImage::from_pixel(4, 4, image::Rgba([90, 160, 220, 255]));
    image.put_pixel(1, 2, image::Rgba([90, 160, 220, 100]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png)?;
    let AssetPublishData::Material(material) = convert_uploaded_material_to_publish_data("Glass", bytes.into_inner(), None, None, None).await? else {
        anyhow::bail!("Expected material data");
    };
    let is_blend = match material.alphaMode {
        AlphaMode::Blend => true,
        AlphaMode::Opaque | AlphaMode::Mask { .. } => false,
    };
    assert!(is_blend);
    Ok(())
}

// A refused image is reported with the map it was sent for, since a material sends several at once.
#[tokio::test]
async fn a_material_upload_refuses_a_bad_name_or_image() -> anyhow::Result<()> {
    assert!(material_upload_error("shit", png(1, 1)?, None).await.contains("not allowed"));
    assert!(material_upload_error(" ", png(1, 1)?, None).await.contains("needs a name"));
    let oversized_normal = material_upload_error("Wall", png(1, 1)?, Some(png(2049, 1)?)).await;
    assert!(
        oversized_normal.contains("Normal texture") && oversized_normal.contains("2048 pixels"),
        "{oversized_normal}"
    );
    let gif_color = material_upload_error("Wall", b"GIF89a\x01\x00\x01\x00".to_vec(), None).await;
    assert!(gif_color.contains("Color texture") && gif_color.contains("PNG, JPEG, or WebP"), "{gif_color}");
    Ok(())
}
