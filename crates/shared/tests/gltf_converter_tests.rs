use std::fs;

use game_types::data_types::MeshVertexInternal;
use game_types::object_types::InterpolationStyle;
use glam::{Quat, Vec2, Vec3, Vec4};
use render_core::computeSkeletonPose::computeLocalTransformOfJointAtTime;
use shared::game::gltf_converter::{LoadedGltf, LoadedMeshData, LoadedMeshTextures, parsePathIntoLoadedGltf};
use std::collections::HashMap;

#[test]
fn parse_gltf_loads_a_disk_gltf_with_an_external_buffer() -> anyhow::Result<()> {
    let temp_directory_path = std::env::temp_dir().join(format!(
        "gltf-converter-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&temp_directory_path)?;

    // This writes the smallest useful `.gltf` fixture so the file-based parser can
    // prove it resolves sidecar `.bin` data instead of only handling embedded GLB bytes.
    let gltf_path = temp_directory_path.join("triangle.gltf");
    let buffer_path = temp_directory_path.join("triangle.bin");
    let triangle_gltf = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0] }],
  "nodes": [{ "mesh": 0 }],
  "meshes": [{
    "primitives": [{
      "attributes": { "POSITION": 0 },
      "indices": 1
    }]
  }],
  "buffers": [{
    "uri": "triangle.bin",
    "byteLength": 42
  }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963 }
  ],
  "accessors": [
    {
      "bufferView": 0,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 3,
      "type": "VEC3",
      "min": [0.0, 0.0, 0.0],
      "max": [1.0, 1.0, 0.0]
    },
    {
      "bufferView": 1,
      "byteOffset": 0,
      "componentType": 5123,
      "count": 3,
      "type": "SCALAR"
    }
  ]
}"#;

    let mut triangle_buffer_bytes = Vec::new();
    for position_component in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        triangle_buffer_bytes.extend_from_slice(&position_component.to_le_bytes());
    }
    for index in [0_u16, 1, 2] {
        triangle_buffer_bytes.extend_from_slice(&index.to_le_bytes());
    }

    fs::write(&gltf_path, triangle_gltf)?;
    fs::write(&buffer_path, triangle_buffer_bytes)?;

    let loaded_gltf = parsePathIntoLoadedGltf(&gltf_path)?;
    assert_eq!(loaded_gltf.meshes.len(), 1);

    let Some(first_mesh) = loaded_gltf.meshes.first() else {
        return Err(anyhow::anyhow!("Expected one parsed mesh"));
    };
    match first_mesh {
        LoadedMeshData::LoadedStaticMeshData { vertices, indices, .. } => {
            assert_eq!(vertices.len(), 3);
            assert_eq!(indices.as_slice(), &[0, 1, 2]);
        }
        LoadedMeshData::LoadedSkinnedMeshData { .. } => {
            return Err(anyhow::anyhow!("Expected a static mesh from the triangle fixture"));
        }
    }

    // Preserve every glTF alpha mode, including MASK's specification-defined cutoff default.
    let mut material_fixture: serde_json::Value = serde_json::from_str(triangle_gltf)?;
    material_fixture["meshes"][0]["primitives"][0]["material"] = serde_json::json!(0);
    for material in [
        serde_json::json!({}),
        serde_json::json!({"alphaMode":"BLEND", "doubleSided":true}),
        serde_json::json!({"alphaMode":"MASK"}),
        serde_json::json!({"alphaMode":"MASK", "alphaCutoff":0.25}),
    ] {
        material_fixture["materials"] = serde_json::json!([material]);
        fs::write(&gltf_path, serde_json::to_vec(&material_fixture)?)?;
        let imported = parsePathIntoLoadedGltf(&gltf_path)?;
        let LoadedMeshData::LoadedStaticMeshData { loadedTextures, .. } = &imported.meshes[0] else {
            unreachable!()
        };
        assert_eq!(loadedTextures.isDoubleSided, material["doubleSided"].as_bool().unwrap_or(false));
        use shared::game::gltf_converter::LoadedMaterialAlphaMode;
        match (material["alphaMode"].as_str(), loadedTextures.alphaMode) {
            (None, LoadedMaterialAlphaMode::Opaque) | (Some("BLEND"), LoadedMaterialAlphaMode::Blend) => {}
            (Some("MASK"), LoadedMaterialAlphaMode::Mask { cutoff }) => assert_eq!(cutoff, material["alphaCutoff"].as_f64().unwrap_or(0.5) as f32),
            other => panic!("Importer changed the material alpha mode: {other:?}"),
        }
    }

    material_fixture["materials"] = serde_json::json!([{
        "alphaMode": "BLEND",
        "pbrMetallicRoughness": { "baseColorFactor": [1.0, 0.2, 0.0, 0.4], "metallicFactor": 0.25, "roughnessFactor": 0.75 }
    }]);
    fs::write(&gltf_path, serde_json::to_vec(&material_fixture)?)?;
    let imported = parsePathIntoLoadedGltf(&gltf_path)?;
    let LoadedMeshData::LoadedStaticMeshData { loadedTextures, .. } = &imported.meshes[0] else {
        unreachable!()
    };
    let flat_pixel = |image_index: Option<usize>| {
        let texture = &imported.loadedTextureOfIndex[&image_index.expect("factor material should get a flat texture")];
        assert!(texture.rgba8Pixels.chunks_exact(4).all(|pixel| pixel == &texture.rgba8Pixels[..4]));
        texture.rgba8Pixels[..4].to_vec()
    };
    assert_eq!(flat_pixel(loadedTextures.baseColor), [255, 124, 0, 102]);
    assert_eq!(loadedTextures.metallic, loadedTextures.roughness);
    assert_eq!(flat_pixel(loadedTextures.metallic), [0, 191, 64, 255]);

    fs::remove_file(buffer_path)?;
    fs::remove_file(gltf_path)?;
    fs::remove_dir(temp_directory_path)?;
    Ok(())
}

#[test]
fn parse_gltf_imports_rest_pose_for_skinned_joints() -> anyhow::Result<()> {
    let temp_directory_path = std::env::temp_dir().join(format!(
        "gltf-converter-skinned-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&temp_directory_path)?;

    // This writes a tiny skinned `.gltf` fixture so the parser has to rebuild joint local transforms from actual glTF node transforms.
    let gltf_path = temp_directory_path.join("skinned_triangle.gltf");
    let buffer_path = temp_directory_path.join("skinned_triangle.bin");
    let skinned_triangle_gltf = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0, 1] }],
  "nodes": [
    { "mesh": 0, "skin": 0 },
    { "name": "hip", "translation": [1.0, 2.0, 3.0], "children": [2] },
    { "name": "head", "translation": [4.0, 5.0, 6.0] }
  ],
  "meshes": [{
    "primitives": [{
      "attributes": {
        "POSITION": 0,
        "JOINTS_0": 1,
        "WEIGHTS_0": 2
      },
      "indices": 3
    }]
  }],
  "skins": [{
    "inverseBindMatrices": 4,
    "joints": [1, 2]
  }],
  "buffers": [{
    "uri": "skinned_triangle.bin",
    "byteLength": 244
  }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 24, "target": 34962 },
    { "buffer": 0, "byteOffset": 60, "byteLength": 48, "target": 34962 },
    { "buffer": 0, "byteOffset": 108, "byteLength": 6, "target": 34963 },
    { "buffer": 0, "byteOffset": 116, "byteLength": 128 }
  ],
  "accessors": [
    {
      "bufferView": 0,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 3,
      "type": "VEC3",
      "min": [0.0, 0.0, 0.0],
      "max": [1.0, 1.0, 0.0]
    },
    {
      "bufferView": 1,
      "byteOffset": 0,
      "componentType": 5123,
      "count": 3,
      "type": "VEC4"
    },
    {
      "bufferView": 2,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 3,
      "type": "VEC4"
    },
    {
      "bufferView": 3,
      "byteOffset": 0,
      "componentType": 5123,
      "count": 3,
      "type": "SCALAR"
    },
    {
      "bufferView": 4,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 2,
      "type": "MAT4"
    }
  ]
}"#;

    let mut skinned_triangle_buffer_bytes = Vec::new();
    for position_component in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        skinned_triangle_buffer_bytes.extend_from_slice(&position_component.to_le_bytes());
    }
    for joint_index in [0_u16, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0] {
        skinned_triangle_buffer_bytes.extend_from_slice(&joint_index.to_le_bytes());
    }
    for joint_weight in [1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        skinned_triangle_buffer_bytes.extend_from_slice(&joint_weight.to_le_bytes());
    }
    for index in [0_u16, 1, 2] {
        skinned_triangle_buffer_bytes.extend_from_slice(&index.to_le_bytes());
    }
    skinned_triangle_buffer_bytes.extend_from_slice(&[0_u8, 0_u8]);
    for inverse_bind_component in [
        1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, -2.0, -3.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        -5.0, -7.0, -9.0, 1.0,
    ] {
        skinned_triangle_buffer_bytes.extend_from_slice(&inverse_bind_component.to_le_bytes());
    }

    fs::write(&gltf_path, skinned_triangle_gltf)?;
    fs::write(&buffer_path, skinned_triangle_buffer_bytes)?;

    let loaded_gltf = parsePathIntoLoadedGltf(&gltf_path)?;
    assert_eq!(loaded_gltf.meshes.len(), 1);

    let Some(first_mesh) = loaded_gltf.meshes.first() else {
        return Err(anyhow::anyhow!("Expected one parsed mesh"));
    };
    match first_mesh {
        LoadedMeshData::LoadedSkinnedMeshData { jointOfIndex, .. } => {
            assert_eq!(jointOfIndex.len(), 2);
            assert_eq!(jointOfIndex[0].name, "hip");
            assert_eq!(jointOfIndex[0].rest_pose.position, Vec3::new(1.0, 2.0, 3.0));

            assert_eq!(jointOfIndex[1].name, "head");
            assert_eq!(jointOfIndex[1].parent_joint_index, Some(0));
            assert_eq!(jointOfIndex[1].rest_pose.position, Vec3::new(4.0, 5.0, 6.0));
        }
        LoadedMeshData::LoadedStaticMeshData { .. } => {
            return Err(anyhow::anyhow!("Expected a skinned mesh from the skinned triangle fixture"));
        }
    }

    fs::remove_file(buffer_path)?;
    fs::remove_file(gltf_path)?;
    fs::remove_dir(temp_directory_path)?;
    Ok(())
}

#[test]
fn parse_gltf_discards_joint_scale_animation_channels() -> anyhow::Result<()> {
    let temp_directory_path = std::env::temp_dir().join(format!(
        "gltf-converter-animation-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&temp_directory_path)?;

    // This writes one animated joint with both translation and scale channels so the parser has to keep the supported channel and drop the unsupported one.
    let gltf_path = temp_directory_path.join("animated_triangle.gltf");
    let buffer_path = temp_directory_path.join("animated_triangle.bin");
    let animated_triangle_gltf = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0, 1] }],
  "nodes": [
    { "mesh": 0, "skin": 0 },
    { "name": "hip" }
  ],
  "meshes": [{
    "primitives": [{
      "attributes": {
        "POSITION": 0,
        "JOINTS_0": 1,
        "WEIGHTS_0": 2
      },
      "indices": 3
    }]
  }],
  "skins": [{
    "inverseBindMatrices": 4,
    "joints": [1]
  }],
  "animations": [{
    "samplers": [
      { "input": 5, "output": 6, "interpolation": "LINEAR" },
      { "input": 5, "output": 7, "interpolation": "LINEAR" }
    ],
    "channels": [
      { "sampler": 0, "target": { "node": 1, "path": "translation" } },
      { "sampler": 1, "target": { "node": 1, "path": "scale" } }
    ]
  }],
  "buffers": [{
    "uri": "animated_triangle.bin",
    "byteLength": 236
  }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 24, "target": 34962 },
    { "buffer": 0, "byteOffset": 60, "byteLength": 48, "target": 34962 },
    { "buffer": 0, "byteOffset": 108, "byteLength": 6, "target": 34963 },
    { "buffer": 0, "byteOffset": 116, "byteLength": 64 },
    { "buffer": 0, "byteOffset": 180, "byteLength": 8 },
    { "buffer": 0, "byteOffset": 188, "byteLength": 24 },
    { "buffer": 0, "byteOffset": 212, "byteLength": 24 }
  ],
  "accessors": [
    {
      "bufferView": 0,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 3,
      "type": "VEC3",
      "min": [0.0, 0.0, 0.0],
      "max": [1.0, 1.0, 0.0]
    },
    {
      "bufferView": 1,
      "byteOffset": 0,
      "componentType": 5123,
      "count": 3,
      "type": "VEC4"
    },
    {
      "bufferView": 2,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 3,
      "type": "VEC4"
    },
    {
      "bufferView": 3,
      "byteOffset": 0,
      "componentType": 5123,
      "count": 3,
      "type": "SCALAR"
    },
    {
      "bufferView": 4,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 1,
      "type": "MAT4"
    },
    {
      "bufferView": 5,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 2,
      "type": "SCALAR",
      "min": [0.0],
      "max": [1.0]
    },
    {
      "bufferView": 6,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 2,
      "type": "VEC3"
    },
    {
      "bufferView": 7,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 2,
      "type": "VEC3"
    }
  ]
}"#;

    let mut animated_triangle_buffer_bytes = Vec::new();
    for position_component in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        animated_triangle_buffer_bytes.extend_from_slice(&position_component.to_le_bytes());
    }
    for joint_index in [0_u16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        animated_triangle_buffer_bytes.extend_from_slice(&joint_index.to_le_bytes());
    }
    for joint_weight in [1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        animated_triangle_buffer_bytes.extend_from_slice(&joint_weight.to_le_bytes());
    }
    for index in [0_u16, 1, 2] {
        animated_triangle_buffer_bytes.extend_from_slice(&index.to_le_bytes());
    }
    animated_triangle_buffer_bytes.extend_from_slice(&[0_u8, 0_u8]);
    for inverse_bind_component in [1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        animated_triangle_buffer_bytes.extend_from_slice(&inverse_bind_component.to_le_bytes());
    }
    for animation_time in [0.0_f32, 1.0] {
        animated_triangle_buffer_bytes.extend_from_slice(&animation_time.to_le_bytes());
    }
    for translation_component in [0.0_f32, 0.0, 0.0, 1.0, 2.0, 3.0] {
        animated_triangle_buffer_bytes.extend_from_slice(&translation_component.to_le_bytes());
    }
    for scale_component in [1.0_f32, 1.0, 1.0, 3.0, 4.0, 5.0] {
        animated_triangle_buffer_bytes.extend_from_slice(&scale_component.to_le_bytes());
    }

    fs::write(&gltf_path, animated_triangle_gltf)?;
    fs::write(&buffer_path, animated_triangle_buffer_bytes)?;

    let loaded_gltf = parsePathIntoLoadedGltf(&gltf_path)?;
    assert_eq!(loaded_gltf.meshes.len(), 1);

    let Some(first_mesh) = loaded_gltf.meshes.first() else {
        return Err(anyhow::anyhow!("Expected one parsed mesh"));
    };
    match first_mesh {
        LoadedMeshData::LoadedSkinnedMeshData { animations, .. } => {
            assert_eq!(animations.len(), 1);

            let animation = &animations[0];
            match animation.sk_animation_info.interpolationStyle {
                InterpolationStyle::Linear => {}
                InterpolationStyle::Step | InterpolationStyle::EaseInOut | InterpolationStyle::EaseIn | InterpolationStyle::EaseOut => {
                    return Err(anyhow::anyhow!("Expected linear glTF animation to import as Linear"));
                }
            }
            assert_eq!(animation.sk_keyframes.len(), 2);

            let first_frame = &animation.sk_keyframes[0];
            assert_eq!(first_frame.time, 0.0);
            assert_eq!(first_frame.skKeyframePoses.len(), 1);
            assert_eq!(first_frame.skKeyframePoses[0].localPose.position, Vec3::ZERO);

            let second_frame = &animation.sk_keyframes[1];
            assert_eq!(second_frame.time, 1.0);
            assert_eq!(second_frame.skKeyframePoses.len(), 1);
            assert_eq!(second_frame.skKeyframePoses[0].localPose.position, Vec3::new(1.0, 2.0, 3.0));
        }
        LoadedMeshData::LoadedStaticMeshData { .. } => {
            return Err(anyhow::anyhow!("Expected a skinned mesh from the animated triangle fixture"));
        }
    }

    fs::remove_file(buffer_path)?;
    fs::remove_file(gltf_path)?;
    fs::remove_dir(temp_directory_path)?;
    Ok(())
}

// glTF channels hold a joint's whole local pose, but keyframes store the offset from the joint's rest pose.
// The hip rests at y=1, turned 90 degrees around z, and its translation channel moves it from its rest position to (1, 1, 0).
#[test]
fn parse_gltf_stores_keyframes_as_offsets_from_the_rest_pose() -> anyhow::Result<()> {
    let temp_directory_path = std::env::temp_dir().join(format!(
        "gltf-converter-offset-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&temp_directory_path)?;

    let gltf_path = temp_directory_path.join("offset_triangle.gltf");
    let buffer_path = temp_directory_path.join("offset_triangle.bin");
    let offset_triangle_gltf = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0, 1] }],
  "nodes": [
    { "mesh": 0, "skin": 0 },
    { "name": "hip", "translation": [0.0, 1.0, 0.0], "rotation": [0.0, 0.0, 0.70710677, 0.70710677] }
  ],
  "meshes": [{
    "primitives": [{
      "attributes": {
        "POSITION": 0,
        "JOINTS_0": 1,
        "WEIGHTS_0": 2
      },
      "indices": 3
    }]
  }],
  "skins": [{
    "inverseBindMatrices": 4,
    "joints": [1]
  }],
  "animations": [{
    "samplers": [
      { "input": 5, "output": 6, "interpolation": "LINEAR" }
    ],
    "channels": [
      { "sampler": 0, "target": { "node": 1, "path": "translation" } }
    ]
  }],
  "buffers": [{
    "uri": "offset_triangle.bin",
    "byteLength": 212
  }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 24, "target": 34962 },
    { "buffer": 0, "byteOffset": 60, "byteLength": 48, "target": 34962 },
    { "buffer": 0, "byteOffset": 108, "byteLength": 6, "target": 34963 },
    { "buffer": 0, "byteOffset": 116, "byteLength": 64 },
    { "buffer": 0, "byteOffset": 180, "byteLength": 8 },
    { "buffer": 0, "byteOffset": 188, "byteLength": 24 }
  ],
  "accessors": [
    { "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
    { "bufferView": 1, "byteOffset": 0, "componentType": 5123, "count": 3, "type": "VEC4" },
    { "bufferView": 2, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC4" },
    { "bufferView": 3, "byteOffset": 0, "componentType": 5123, "count": 3, "type": "SCALAR" },
    { "bufferView": 4, "byteOffset": 0, "componentType": 5126, "count": 1, "type": "MAT4" },
    { "bufferView": 5, "byteOffset": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0] },
    { "bufferView": 6, "byteOffset": 0, "componentType": 5126, "count": 2, "type": "VEC3" }
  ]
}"#;

    let mut offset_triangle_buffer_bytes = Vec::new();
    for position_component in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        offset_triangle_buffer_bytes.extend_from_slice(&position_component.to_le_bytes());
    }
    for joint_index in [0_u16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        offset_triangle_buffer_bytes.extend_from_slice(&joint_index.to_le_bytes());
    }
    for joint_weight in [1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        offset_triangle_buffer_bytes.extend_from_slice(&joint_weight.to_le_bytes());
    }
    for index in [0_u16, 1, 2] {
        offset_triangle_buffer_bytes.extend_from_slice(&index.to_le_bytes());
    }
    offset_triangle_buffer_bytes.extend_from_slice(&[0_u8, 0_u8]);
    for inverse_bind_component in [1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        offset_triangle_buffer_bytes.extend_from_slice(&inverse_bind_component.to_le_bytes());
    }
    for animation_time in [0.0_f32, 1.0] {
        offset_triangle_buffer_bytes.extend_from_slice(&animation_time.to_le_bytes());
    }
    for translation_component in [0.0_f32, 1.0, 0.0, 1.0, 1.0, 0.0] {
        offset_triangle_buffer_bytes.extend_from_slice(&translation_component.to_le_bytes());
    }

    fs::write(&gltf_path, offset_triangle_gltf)?;
    fs::write(&buffer_path, offset_triangle_buffer_bytes)?;

    let loaded_gltf = parsePathIntoLoadedGltf(&gltf_path)?;
    let Some(LoadedMeshData::LoadedSkinnedMeshData { animations, jointOfIndex, .. }) = loaded_gltf.meshes.first() else {
        return Err(anyhow::anyhow!("Expected a skinned mesh from the offset triangle fixture"));
    };
    let [animation] = animations.as_slice() else {
        return Err(anyhow::anyhow!("Expected one animation"));
    };
    let assert_vec3_near = |actual: Vec3, expected: Vec3, label: &str| {
        assert!((actual - expected).length() < 1e-5, "{label}: expected {expected:?}, got {actual:?}");
    };

    // At its rest position the offset is zero, and the offset for (1, 1, 0) is measured in the hip's turned rest frame.
    let first_pose = &animation.sk_keyframes[0].skKeyframePoses[0].localPose;
    assert_vec3_near(first_pose.position, Vec3::ZERO, "first keyframe offset");
    assert!(
        first_pose.quaternion.angle_between(Quat::IDENTITY) < 1e-5,
        "an unanimated rotation has no offset"
    );
    let second_pose = &animation.sk_keyframes[1].skKeyframePoses[0].localPose;
    assert_vec3_near(second_pose.position, Vec3::new(0.0, -1.0, 0.0), "second keyframe offset");

    // Playing the imported keyframes puts the hip exactly where the glTF says.
    let (posed_position, posed_rotation, _scale) =
        computeLocalTransformOfJointAtTime(jointOfIndex, &animation.sk_animation_info, &animation.sk_keyframes, 1.0)[0].unwrap();
    assert_vec3_near(posed_position, Vec3::new(1.0, 1.0, 0.0), "posed hip");
    assert!(
        posed_rotation.angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)) < 1e-5,
        "the hip keeps its rest rotation"
    );

    fs::remove_file(buffer_path)?;
    fs::remove_file(gltf_path)?;
    fs::remove_dir(temp_directory_path)?;
    Ok(())
}

// Every copy of a model is drawn each frame, so an imported model has at most 20,000 triangles, and 3 vertices for each.
#[test]
fn an_imported_model_has_at_most_20000_triangles_and_60000_vertices() {
    let model = |vertex_count: usize, triangle_count: usize| LoadedGltf {
        blender_id: None,
        meshes: vec![LoadedMeshData::LoadedStaticMeshData {
            name: "mesh".to_string(),
            vertices: vec![
                MeshVertexInternal {
                    position: Vec3::ZERO,
                    normal: Vec3::Y,
                    uv: Vec2::ZERO,
                    tangent: Vec4::ZERO,
                    skinning: (),
                };
                vertex_count
            ],
            indices: [0, 1, 2].repeat(triangle_count),
            loadedTextures: LoadedMeshTextures::default(),
        }],
        loadedTextureOfIndex: HashMap::new(),
    };
    model(60_000, 20_000).validate().unwrap();
    for (vertex_count, triangle_count) in [(3, 20_001), (60_001, 1)] {
        let error = model(vertex_count, triangle_count).validate().unwrap_err().to_string();
        assert!(
            error.contains(&format!("This model has {triangle_count} triangles and {vertex_count} vertices")),
            "{error}"
        );
    }
}
