use anyhow::{Result, anyhow, bail, ensure};
use game_types::{
    data_types::{SoundData, TextureData},
    object_types::material::AlphaMode,
};
use linux_sandbox::{MAX_GLB_BYTES, validate_glb};
use profanity::has_profanity_in_asset_name;
use shared::{
    api::protocol_for_assets::{AssetPublishData, LoadedMaterial, UploadedDataWrapper},
    game::{
        assets::AssetType,
        gltf_converter::parseBytesIntoLoadedGltf,
        texture_converter::{convert_color_image_bytes_to_texture_data, convert_image_bytes_to_loaded_texture, convert_skybox_image_bytes_to_texture_data},
    },
};
use std::{
    io::{Cursor, ErrorKind},
    num::{NonZeroU8, NonZeroU32},
    sync::LazyLock,
};
use symphonia::core::{
    codecs::DecoderOptions, errors::Error as SymphoniaError, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
use tokio::sync::Mutex;
use vorbis_rs::VorbisEncoderBuilder;

// Converting a model or an image takes a lot of memory, so only one upload converts at a time.
static UPLOAD_CONVERSION: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// A file the browser uploads becomes the same publish data the AI's asset tools send. Everything publishing
// would refuse is refused here first, so the browser hears why instead of a failed publish.
pub async fn convert_uploaded_asset_to_publish_data(kind: AssetType, name: &str, bytes: Vec<u8>) -> Result<AssetPublishData> {
    ensure!(!name.trim().is_empty(), "The file needs a name.");
    ensure!(!has_profanity_in_asset_name(name), "That file name is not allowed.");
    let upload_conversion_lock = UPLOAD_CONVERSION.lock().await;
    let data = match kind {
        AssetType::Mesh => {
            ensure!(bytes.len() <= MAX_GLB_BYTES, "Models must be at most {} MiB.", MAX_GLB_BYTES / 1024 / 1024);
            validate_glb(&bytes)?;
            AssetPublishData::Mesh(tokio::task::spawn_blocking(move || parseBytesIntoLoadedGltf(&bytes)).await??)
        }
        AssetType::Texture => {
            let texture = tokio::task::spawn_blocking(move || convert_color_image_bytes_to_texture_data(&bytes)).await??;
            AssetPublishData::Texture(UploadedDataWrapper::<TextureData>::new(texture))
        }
        AssetType::Sound | AssetType::Sfx => {
            // An Ogg stream starts with an OggS page whose first packet is the Vorbis identification header.
            let is_ogg_vorbis = bytes.starts_with(b"OggS")
                && bytes.get(26).is_some_and(|&segment_count| {
                    let packet_start = 27 + usize::from(segment_count);
                    bytes.get(packet_start..packet_start + 7) == Some(b"\x01vorbis".as_slice())
                });
            let ogg_vorbis_data = match is_ogg_vorbis {
                true => bytes,
                false => tokio::task::spawn_blocking(move || convert_sound_bytes_to_ogg_vorbis(bytes)).await??,
            };
            let seconds = ogg_vorbis_seconds(&ogg_vorbis_data)?;
            let sound = UploadedDataWrapper::<SoundData>::new(SoundData {
                oggVorbisData: ogg_vorbis_data,
            });
            match seconds < MAX_SFX_SECONDS {
                true => AssetPublishData::Sfx(sound),
                false => AssetPublishData::Sound(sound),
            }
        }
        AssetType::Skybox => {
            let texture = tokio::task::spawn_blocking(move || convert_skybox_image_bytes_to_texture_data(&bytes)).await??;
            AssetPublishData::Skybox(UploadedDataWrapper::<TextureData>::new(texture))
        }
        AssetType::Object | AssetType::Material => bail!("Only models, textures, sounds, and skyboxes can be uploaded."),
    };
    drop(upload_conversion_lock);
    data.validate()?;
    Ok(data)
}

const MAX_SFX_SECONDS: f64 = 10.0;

fn ogg_vorbis_seconds(bytes: &[u8]) -> Result<f64> {
    let unreadable = || anyhow!("The sound's length could not be read.");
    let packet_start = 27 + usize::from(*bytes.get(26).ok_or_else(unreadable)?);
    let sample_rate = u32::from_le_bytes(bytes.get(packet_start + 12..packet_start + 16).ok_or_else(unreadable)?.try_into()?);
    ensure!(sample_rate > 0, "The sound's sample rate must be positive.");
    let mut sample_count = 0;
    let mut page_start = 0;
    while page_start < bytes.len() {
        ensure!(bytes.get(page_start..page_start + 4) == Some(b"OggS".as_slice()), "The sound's length could not be read.");
        let granule_position = i64::from_le_bytes(bytes.get(page_start + 6..page_start + 14).ok_or_else(unreadable)?.try_into()?);
        if granule_position >= 0 {
            sample_count = granule_position;
        }
        let segment_count = usize::from(*bytes.get(page_start + 26).ok_or_else(unreadable)?);
        let segment_table = bytes.get(page_start + 27..page_start + 27 + segment_count).ok_or_else(unreadable)?;
        page_start += 27 + segment_count + segment_table.iter().map(|&segment_length| usize::from(segment_length)).sum::<usize>();
    }
    Ok(sample_count as f64 / f64::from(sample_rate))
}

// The game stores every sound as Ogg Vorbis, so an MP3, WAV, or FLAC file is decoded and encoded again.
fn convert_sound_bytes_to_ogg_vorbis(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let unreadable = || anyhow!("Sounds must be Ogg Vorbis, MP3, WAV, or FLAC files.");
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    // Gapless decoding trims the silence an MP3 encoder pads the start and end with.
    let format_options = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let mut reader = symphonia::default::get_probe()
        .format(&Hint::new(), source, &format_options, &MetadataOptions::default())
        .map_err(|_| unreadable())?
        .format;
    let track = reader.default_track().ok_or_else(unreadable)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| unreadable())?;
    let sample_rate = track.codec_params.sample_rate.and_then(NonZeroU32::new).ok_or_else(unreadable)?;
    let channel_count = track
        .codec_params
        .channels
        .and_then(|channels| u8::try_from(channels.count()).ok())
        .and_then(NonZeroU8::new)
        .ok_or_else(unreadable)?;
    let mut encoder = VorbisEncoderBuilder::new(sample_rate, channel_count, Vec::new())?.build()?;
    loop {
        let packet = match reader.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Players skip a damaged frame and keep going, so converting does too.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(error.into()),
        };
        // An empty block tells the encoder the sound has ended.
        if decoded.frames() == 0 {
            continue;
        }
        let mut samples = decoded.make_equivalent::<f32>();
        decoded.convert(&mut samples);
        encoder.encode_audio_block(samples.planes().planes())?;
    }
    Ok(encoder.finish()?)
}

// A material arrives as up to four image files, one for each of its maps, and only the color map is required.
// The images are only read into pixels here. Publishing encodes the color map as color and the others as data.
pub async fn convert_uploaded_material_to_publish_data(
    name: &str,
    albedo: Vec<u8>,
    metallic: Option<Vec<u8>>,
    roughness: Option<Vec<u8>>,
    normal: Option<Vec<u8>>,
) -> Result<AssetPublishData> {
    ensure!(!name.trim().is_empty(), "The material needs a name.");
    ensure!(!has_profanity_in_asset_name(name), "That material name is not allowed.");
    let upload_conversion_lock = UPLOAD_CONVERSION.lock().await;
    let material = tokio::task::spawn_blocking(move || -> Result<LoadedMaterial> {
        let read = |label: &str, image_bytes: Vec<u8>| convert_image_bytes_to_loaded_texture(&image_bytes).map_err(|error| anyhow!("{label} texture: {error}"));
        let albedo = read("Color", albedo)?;
        // A color image with see-through pixels, like glass or a fence, only shows through when the material blends.
        let alpha_mode = match albedo.rgba8Pixels.chunks_exact(4).any(|pixel| pixel[3] < 255) {
            true => AlphaMode::Blend,
            false => AlphaMode::Opaque,
        };
        Ok(LoadedMaterial {
            albedo: Some(albedo),
            metallic: metallic.map(|image_bytes| read("Metalness", image_bytes)).transpose()?,
            roughness: roughness.map(|image_bytes| read("Roughness", image_bytes)).transpose()?,
            normal: normal.map(|image_bytes| read("Normal", image_bytes)).transpose()?,
            alphaMode: alpha_mode,
            isDoubleSided: false,
        })
    })
    .await??;
    drop(upload_conversion_lock);
    let data = AssetPublishData::Material(material);
    data.validate()?;
    Ok(data)
}
