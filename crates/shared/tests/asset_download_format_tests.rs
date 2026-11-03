use game_types::{
    consts::{MESH_DATA_CURRENT_VERSION, SOUND_DATA_CURRENT_VERSION, TEXTURE_DATA_CURRENT_VERSION},
    data_types::{MeshData, SoundData, TextureData},
};
use shared::game::uploaded_data::parse_uploaded_data_from_bytes;

#[test]
fn existing_browser_asset_fixtures_decode_for_desktop_playback() -> anyhow::Result<()> {
    let mesh: MeshData = parse_uploaded_data_from_bytes(include_bytes!("../../../tests/fixtures/asset-preview-mesh.bin"), MESH_DATA_CURRENT_VERSION)?;
    let MeshData::StaticMeshData { vertices, indices } = mesh else {
        panic!("Expected the static asset preview mesh");
    };
    assert!(!vertices.is_empty());
    assert!(!indices.is_empty());
    let texture: TextureData = parse_uploaded_data_from_bytes(include_bytes!("../../../tests/fixtures/asset-texture.bin"), TEXTURE_DATA_CURRENT_VERSION)?;
    assert!(texture.width > 0 && texture.height > 0);
    assert!(!texture.ktx2Data.is_empty());
    Ok(())
}

#[test]
fn sound_download_preserves_the_existing_versioned_envelope() -> anyhow::Result<()> {
    let bytes = bincode::serialize(&(
        SOUND_DATA_CURRENT_VERSION,
        SoundData {
            oggVorbisData: b"OggS test".to_vec(),
        },
    ))?;
    let sound: SoundData = parse_uploaded_data_from_bytes(&bytes, SOUND_DATA_CURRENT_VERSION)?;
    assert_eq!(sound.oggVorbisData, b"OggS test");
    assert!(parse_uploaded_data_from_bytes::<SoundData>(&bytes[..bytes.len() - 1], SOUND_DATA_CURRENT_VERSION).is_err());
    Ok(())
}

#[test]
fn stale_download_version_is_rejected_before_decoding_its_payload() {
    let bytes = (SOUND_DATA_CURRENT_VERSION + 1).to_le_bytes();
    let error = parse_uploaded_data_from_bytes::<SoundData>(&bytes, SOUND_DATA_CURRENT_VERSION).unwrap_err();
    assert!(error.to_string().contains("Unsupported data version"));
}
