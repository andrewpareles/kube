#[path = "../src/api_error.rs"]
#[allow(dead_code)]
mod api_error;
#[path = "../src/assets.rs"]
mod assets;
#[path = "../src/auth.rs"]
#[allow(dead_code)]
mod auth;
#[path = "../src/database.rs"]
#[allow(dead_code)]
mod database;

use assets::{dehydrateGltf, download_asset, publish_asset, search_assets};
use shared::api::protocol_for_assets::{AssetPublishData, AssetPublishRequest, AssetSearchRequest};
use shared::game::gltf_converter::LoadedGltf;

// A crude name is refused the same way a username is, before anything is uploaded.
#[tokio::test]
async fn asset_publish_rejects_a_crude_name_and_leaves_ordinary_names_alone() {
    let publish_request = |name: &str| AssetPublishRequest {
        asset_id: None,
        name: name.to_string(),
        data: AssetPublishData::Object(String::new()),
    };
    for name in ["shit", "sh1t", "s.h.i.t", "SHIT"] {
        let error = publish_asset("uploader", publish_request(name)).await.unwrap_err();
        assert!(error.to_string().contains("not allowed"), "{name} was accepted: {error}");
    }
    // These get as far as their data being read, which is how far a name the filter allows gets.
    // "asset" holds a rude word as a run of letters, and catching it would stop people naming
    // their uploads at all.
    for name in ["Tree", "asset pack", "Brick Wall", "Classic Car"] {
        let error = publish_asset("uploader", publish_request(name)).await.unwrap_err();
        assert!(!error.to_string().contains("not allowed"), "{name} was refused: {error}");
    }
}

// Direct calls must still reject invalid input before reaching Supabase or uploading assets.
#[tokio::test]
async fn asset_search_rejects_unbounded_pages() {
    for limit in [0, 101, usize::MAX] {
        let request = AssetSearchRequest {
            query: String::new(),
            asset_type: None,
            offset: 0,
            limit,
        };
        let error = search_assets(&request).await.unwrap_err();
        assert!(error.to_string().contains("limit"), "{error}");
    }
}

#[tokio::test]
async fn asset_download_rejects_invalid_ids() {
    for id in [0, -1, i64::MIN] {
        let error = download_asset(id).await.unwrap_err();
        assert!(error.to_string().contains("id must be positive"), "{error}");
    }
}

#[tokio::test]
async fn asset_publish_rejects_empty_names_and_invalid_ids() {
    for (name, asset_id, expected) in [
        (" \n", None, "name cannot be empty"),
        ("Object", Some(0), "id must be positive"),
        ("Object", Some(-1), "id must be positive"),
    ] {
        let request = AssetPublishRequest {
            asset_id,
            name: name.into(),
            data: AssetPublishData::Object("{}".into()),
        };
        let error = publish_asset("owner", request).await.unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[tokio::test]
async fn asset_publish_validates_assets_without_http_deserialization() {
    let request = AssetPublishRequest {
        asset_id: None,
        name: "Object".into(),
        data: AssetPublishData::Object("{}".into()),
    };
    let error = publish_asset("owner", request).await.unwrap_err();
    assert!(error.to_string().contains("missing field"), "{error}");
}

#[tokio::test]
async fn mesh_upload_rejects_invalid_blender_ids_before_storage_writes() {
    for blender_id in [0, -1, i64::MIN] {
        let gltf = LoadedGltf {
            blender_id: Some(blender_id),
            meshes: Vec::new(),
            loadedTextureOfIndex: std::collections::HashMap::new(),
        };
        let error = dehydrateGltf(gltf, "owner").await.unwrap_err();
        assert!(error.to_string().contains("Blender id"), "{error}");
    }
}

// A material is refused before anything is uploaded when it has no maps or a map whose pixels do not match its size.
#[tokio::test]
async fn material_publish_rejects_missing_or_malformed_maps_before_uploading() {
    use game_types::object_types::material::AlphaMode;
    use shared::{api::protocol_for_assets::LoadedMaterial, game::gltf_converter::LoadedTexture};
    let publish_request = |albedo: Option<LoadedTexture>| AssetPublishRequest {
        asset_id: None,
        name: "Grass".to_string(),
        data: AssetPublishData::Material(LoadedMaterial {
            albedo,
            metallic: None,
            roughness: None,
            normal: None,
            alphaMode: AlphaMode::Opaque,
            isDoubleSided: false,
        }),
    };
    let error = publish_asset("owner", publish_request(None)).await.unwrap_err();
    assert!(error.to_string().contains("at least one texture"), "{error}");
    let short_pixels = LoadedTexture {
        rgba8Pixels: vec![255; 4],
        width: 2,
        height: 2,
    };
    let error = publish_asset("owner", publish_request(Some(short_pixels))).await.unwrap_err();
    assert!(error.to_string().contains("Invalid material texture dimensions"), "{error}");
}
