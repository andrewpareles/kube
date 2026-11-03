use game_types::data_types::DataKind;
use shared::s3_storage::{PrivateFile, PublicFile};
use uuid::Uuid;

#[test]
fn every_kind_of_file_has_its_key_and_says_whether_it_is_gzipped() {
    let uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let public_files = [
        (PublicFile::Data { data_kind: DataKind::MeshData, data_id: 7 }, "data/mesh_data/7", true),
        (PublicFile::Data { data_kind: DataKind::TextureData, data_id: 7 }, "data/texture_data/7", false),
        (PublicFile::Data { data_kind: DataKind::SoundData, data_id: 7 }, "data/sound_data/7", false),
        (PublicFile::Catalog { catalog_item_id: 7 }, "catalog/7", true),
        (PublicFile::GameThumbnail { thumbnail_uuid: uuid }, "game-thumbnails/00000000-0000-0000-0000-000000000001", false),
        (PublicFile::Asset { asset_uuid: uuid }, "assets/00000000-0000-0000-0000-000000000001", true),
    ];
    for (file, key, is_gzipped) in public_files {
        assert_eq!(file.key(), key);
        assert_eq!(file.is_gzipped(), is_gzipped, "{key}");
        assert_eq!(file.url(), format!("https://cdn.kube.new/{key}"));
    }
    let private_files = [
        (PrivateFile::GameWorld { world_uuid: uuid }, "game-worlds/00000000-0000-0000-0000-000000000001", true),
        (PrivateFile::EditorWorld { world_uuid: uuid }, "editor-worlds/00000000-0000-0000-0000-000000000001", true),
        (PrivateFile::ChatScreenshot { game_id: 7, screenshot_uuid: uuid }, "chat-screenshots/7/00000000-0000-0000-0000-000000000001", false),
        (PrivateFile::Blender { blender_id: 7 }, "blender/7", false),
    ];
    for (file, key, is_gzipped) in private_files {
        assert_eq!(file.key(), key);
        assert_eq!(file.is_gzipped(), is_gzipped, "{key}");
    }
}

// Writes real files, so it needs the s3 feature and the editor server's S3 key in S3_ACCESS_KEY_ID and S3_SECRET_ACCESS_KEY. It
// prints the keys it wrote so they can be deleted afterwards.
#[cfg(feature = "s3")]
#[tokio::test]
#[ignore]
async fn files_round_trip_through_s3_and_the_cdn_and_are_never_overwritten() -> anyhow::Result<()> {
    use shared::s3_storage::{download_private_file, download_public_file, upload_private_file, upload_public_file};

    let bytes = br#"{"test":"s3 storage round trip"}"#.to_vec();

    let public_file = PublicFile::Asset { asset_uuid: Uuid::new_v4() };
    upload_public_file(&public_file, bytes.clone()).await?;
    println!("wrote kube-new-public/{}", public_file.key());
    assert!(upload_public_file(&public_file, b"changed".to_vec()).await.is_err());
    assert_eq!(download_public_file(&public_file).await?, bytes);

    let private_file = PrivateFile::EditorWorld { world_uuid: Uuid::new_v4() };
    upload_private_file(&private_file, bytes.clone()).await?;
    println!("wrote kube-new-private/{}", private_file.key());
    assert!(upload_private_file(&private_file, b"changed".to_vec()).await.is_err());
    assert_eq!(download_private_file(&private_file).await?, bytes);
    Ok(())
}
