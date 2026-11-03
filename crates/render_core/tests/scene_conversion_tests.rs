use game_types::{
    data_types::MeshData,
    object_types::{
        BrickMaterialSettings, CylinderMaterialSettings, MaterialInfo, MeshMaterialSettings, PhysicsInfo3D, VisualSettings, WorldLightingInfo,
        material::AlphaMode, shared::Color3,
    },
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings},
};
use glam::{Mat4, Quat, Vec3};
use render_core::{
    render_scene::{
        INSTANCE_TEXTURE_MAPPING_LOCAL_BOX, INSTANCE_TEXTURE_MAPPING_LOCAL_CYLINDRICAL, INSTANCE_TEXTURE_MAPPING_MESH_UV, MATERIAL_ALPHA_MODE_MASK,
        RenderGeometry, build_render_scene,
    },
    renderer_types::{RenderBall, RenderBrick, RenderCylinder, RenderMesh, RenderObject, RenderTransform},
};
use std::{collections::HashMap, sync::Arc};

// ------------------------------ Scene Conversion ------------------------------

// A single CPU snapshot exercises the same conversion used by the desktop adapter and WebGPU demo.
// Nonuniform and negative dimensions distinguish shape sizing from object placement and UV projection.
#[test]
fn scene_conversion_resolves_shapes_materials_lighting_and_selection() {
    let transform = RenderTransform {
        physicsInfo3D: PhysicsInfo3D::new(Vec3::new(3.0, 4.0, 5.0), Quat::from_rotation_y(0.7), Vec3::ZERO, Vec3::ZERO),
        teleport_count: 0,
        rigid_group_to_follow: None,
        sk_joint_to_follow_arc: None,
    };
    let visual_settings = VisualSettings::TYPICAL_OPAQUE;
    let objects = HashMap::from([
        (
            1,
            RenderObject::Brick(RenderBrick {
                transform: transform.clone(),
                visualSettings: visual_settings.clone(),
                primitive: BrickPrimitiveSettings {
                    size: Vec3::new(-2.0, 4.0, 6.0),
                },
                material: Some(BrickMaterialSettings {
                    materialInfoId: 10.into(),
                    tileSizeU: 2.0,
                    tileSizeV: 3.0,
                    tileOffsetU: 0.25,
                    tileOffsetV: -0.5,
                }),
            }),
        ),
        (
            2,
            RenderObject::Cylinder(RenderCylinder {
                transform: transform.clone(),
                visualSettings: visual_settings.clone(),
                primitive: CylinderPrimitiveSettings { radius: -1.5, length: -7.0 },
                material: Some(CylinderMaterialSettings {
                    materialInfoId: 10.into(),
                    tileSizeU: 4.0,
                    tileSizeV: 5.0,
                    tileOffsetU: 0.5,
                    tileOffsetV: 0.75,
                }),
            }),
        ),
        (
            3,
            RenderObject::Ball(RenderBall {
                transform: transform.clone(),
                visualSettings: visual_settings.clone(),
                primitive: BallPrimitiveSettings { radius: -2.5 },
                material: None,
            }),
        ),
        (
            4,
            RenderObject::Mesh(RenderMesh {
                physicalObjectId: 5,
                visualSettings: visual_settings.clone(),
                meshDataId: 20,
                scale: Vec3::new(0.2, 0.3, 0.4),
                material: Some(MeshMaterialSettings { materialInfoId: 10.into() }),
                skin_skeleton_id: None,
            }),
        ),
        (5, RenderObject::RigidGroup(transform.clone())),
        (
            6,
            RenderObject::Ball(RenderBall {
                transform: transform.clone(),
                visualSettings: VisualSettings {
                    opacity: 0.0,
                    ..visual_settings
                },
                primitive: BallPrimitiveSettings { radius: 10.0 },
                material: None,
            }),
        ),
        (
            10,
            RenderObject::Material(MaterialInfo {
                albedoTextureDataId: Some(21),
                metallicTextureDataId: Some(22),
                roughnessTextureDataId: Some(23),
                normalTextureDataId: Some(24),
                alphaMode: AlphaMode::Mask { cutoff: 0.4 },
                isDoubleSided: false,
            }),
        ),
        (
            11,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunIntensity: 2.5,
                ambientIntensity: 0.3,
                skyboxTextureDataId: Some(25),
                skyboxTextureOpacity: 0.6,
                ..WorldLightingInfo::default()
            }),
        ),
        (
            12,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunIntensity: 99.0,
                ..WorldLightingInfo::default()
            }),
        ),
    ]);
    let meshes = HashMap::from([(
        20,
        Arc::new(MeshData::StaticMeshData {
            vertices: Vec::new(),
            indices: Vec::new(),
        }),
    )]);
    let scene = build_render_scene(&objects, &meshes, None);
    assert_eq!(
        scene.instances.len(),
        4,
        "invisible geometry and nondrawable objects must not produce instances"
    );
    assert_eq!(
        scene
            .instances
            .iter()
            .map(|instance| instance.object_id)
            .collect::<std::collections::HashSet<_>>(),
        [1, 2, 3, 4].into_iter().collect()
    );
    assert!(scene.selected_object_ids.is_empty(), "Ordinary scene conversion must not select objects");
    assert_eq!(scene.lighting.sun_color_and_intensity[3], 2.5, "the lowest lighting object ID wins");
    assert_eq!(scene.lighting.ambient_top_color_and_intensity[3], 0.3);
    assert_eq!(scene.skybox_texture_data_id, Some(25));
    assert_eq!(scene.skybox_texture_opacity, 0.6);

    let location = &transform.physicsInfo3D.location;
    let placement = Mat4::from_rotation_translation(location.quaternion, location.position);
    for instance in &scene.instances {
        let (scale, mapping, tile) = match instance.geometry {
            RenderGeometry::Brick => (Vec3::new(2.0, 4.0, 6.0), INSTANCE_TEXTURE_MAPPING_LOCAL_BOX, [2.0, 3.0, 0.25, -0.5]),
            RenderGeometry::Cylinder => (Vec3::new(1.5, 1.5, 7.0), INSTANCE_TEXTURE_MAPPING_LOCAL_CYLINDRICAL, [4.0, 5.0, 0.5, 0.75]),
            RenderGeometry::Ball => (Vec3::splat(2.5), INSTANCE_TEXTURE_MAPPING_MESH_UV, [0.0; 4]),
            RenderGeometry::Mesh { data_id, skeleton_id } => {
                assert_eq!(data_id, 20);
                assert_eq!(skeleton_id, None);
                (Vec3::new(0.2, 0.3, 0.4), INSTANCE_TEXTURE_MAPPING_MESH_UV, [0.0; 4])
            }
        };
        assert!(Mat4::from_cols_array_2d(&instance.content.model).abs_diff_eq(placement * Mat4::from_scale(scale), 0.00001));
        assert_eq!(instance.content.local_projection_scale, scale.extend(0.0).to_array());
        assert_eq!(instance.content.texture_mapping_mode, mapping);
        assert_eq!(instance.content.texture_tile_transform, tile);
        match instance.geometry {
            RenderGeometry::Ball => assert_eq!(instance.material.basecolor_texture_data_id, None),
            _ => {
                assert_eq!(instance.material.basecolor_texture_data_id, Some(21));
                assert_eq!(instance.material.metallic_texture_data_id, Some(22));
                assert_eq!(instance.material.roughness_texture_data_id, Some(23));
                assert_eq!(instance.material.normal_texture_data_id, Some(24));
                assert_eq!(instance.material.alpha_mode, MATERIAL_ALPHA_MODE_MASK);
                assert_eq!(f32::from_bits(instance.material.alpha_cutoff_bits), 0.4);
            }
        }
    }

    let loading_scene = build_render_scene(&objects, &HashMap::new(), None);
    assert_eq!(loading_scene.instances.len(), 3, "a pending mesh must not hide loaded primitives");
}

// Colors are authored like CSS colors, which are sRGB encoded, while lighting math needs linear light. Handing
// the sRGB number straight to a shader is what used to render every flat color paler than the one picked, so a
// mid grey has to arrive decoded, while the opacity and intensity riding along with it arrive untouched.
#[test]
fn authored_colors_reach_the_shaders_decoded_from_srgb() {
    let half_grey = Color3 { r: 0.5, g: 0.5, b: 0.5 };
    // 0.5 in sRGB is this much linear light.
    let decoded_half_grey = 0.21404;
    let objects = HashMap::from([
        (
            1,
            RenderObject::Brick(RenderBrick {
                transform: RenderTransform {
                    physicsInfo3D: PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                    teleport_count: 0,
                    rigid_group_to_follow: None,
                    sk_joint_to_follow_arc: None,
                },
                visualSettings: VisualSettings {
                    opacity: 0.25,
                    color: half_grey,
                    glow: 0.0,
                    isAlwaysOnTop: false,
                },
                primitive: BrickPrimitiveSettings { size: Vec3::ONE },
                material: None,
            }),
        ),
        (
            2,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunColor: half_grey,
                sunIntensity: 3.0,
                ambientTopColor: half_grey,
                ambientIntensity: 0.4,
                skyHorizonColor: half_grey,
                ..WorldLightingInfo::default()
            }),
        ),
    ]);
    let scene = build_render_scene(&objects, &HashMap::new(), None);
    let assert_decoded = |actual: [f32; 4], expected_fourth: f32, what: &str| {
        for channel in &actual[..3] {
            assert!((channel - decoded_half_grey).abs() < 0.0001, "{what} was not decoded from sRGB: {actual:?}");
        }
        assert_eq!(actual[3], expected_fourth, "{what} changed the value riding along with it: {actual:?}");
    };
    assert_decoded(scene.instances[0].content.color, 0.25, "the brick color");
    assert_decoded(scene.lighting.sun_color_and_intensity, 3.0, "the sun color");
    assert_decoded(scene.lighting.ambient_top_color_and_intensity, 0.4, "the ambient color");
    assert_decoded(scene.lighting.sky_horizon_color_and_padding, 0.0, "the sky color");
}
