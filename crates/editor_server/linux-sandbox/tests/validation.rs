use linux_sandbox::{MAX_BLEND_BYTES, validate_blend, validate_glb};
use serde_json::{Value, json};

#[test]
fn saved_blender_sources_reject_wrong_headers_and_oversized_files() {
    validate_blend(b"BLENDER-v304").unwrap();
    for bytes in [b"".as_slice(), b"glTF", b"compressed blend"] {
        assert!(validate_blend(bytes).is_err());
    }
    let mut oversized = vec![0; MAX_BLEND_BYTES + 1];
    oversized[..7].copy_from_slice(b"BLENDER");
    assert!(validate_blend(&oversized).is_err());
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

#[test]
fn permits_embedded_glb_but_rejects_files_and_network_resources() {
    let valid = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":4}]});
    validate_glb(&glb(valid.clone(), &[0; 4])).unwrap();
    for uri in [
        "file:///etc/passwd",
        "../../editor-secret",
        "https://example.com/model.bin",
        "data:application/octet-stream;base64,AAAA",
    ] {
        let mut external_buffer = valid.clone();
        external_buffer["buffers"][0]["uri"] = json!(uri);
        assert!(validate_glb(&glb(external_buffer, &[0; 4])).is_err(), "{uri}");
        let mut external_image = valid.clone();
        external_image["images"] = json!([{"uri":uri}]);
        assert!(validate_glb(&glb(external_image, &[0; 4])).is_err(), "{uri}");
    }
}

#[test]
fn rejects_cycles_shared_children_and_excessive_allocations() {
    for nodes in [
        json!([{"children":[0]}]),
        json!([{"children":[1]},{"children":[0]}]),
        json!([{"children":[2]},{"children":[2]},{}]),
    ] {
        let document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":4}],"nodes":nodes});
        assert!(validate_glb(&glb(document, &[0; 4])).is_err());
    }
    let document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":4}],"accessors":[{"componentType":5126,"count":2_000_001,"type":"SCALAR"}]});
    assert!(validate_glb(&glb(document, &[0; 4])).is_err());
}

#[test]
fn bounds_embedded_texture_dimensions_before_decoding() {
    use image::{ImageEncoder, codecs::png::PngEncoder};
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&vec![0u8; 2049 * 4], 2049, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":png.len()}],"bufferViews":[{"buffer":0,"byteLength":png.len()}],"images":[{"bufferView":0,"mimeType":"image/png"}]});
    let error = validate_glb(&glb(document, &png)).unwrap_err();
    assert!(error.to_string().contains("2048"), "{error}");
}
