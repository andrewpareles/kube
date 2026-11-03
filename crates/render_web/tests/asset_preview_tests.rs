use game_types::{consts::DEHYDRATED_GLTF_CURRENT_VERSION, object_types::ObjectInfo, stored_world_type::StoredWorld};
use render_core::{
    dehydrated_gltf::{DehydratedGltf, DehydratedMaterial, DehydratedMesh},
    dehydrated_gltf_to_stored_world::{add_dehydrated_gltf_to_stored_world, dehydrated_gltf_to_stored_world},
    render_scene::{RenderGeometry, build_render_scene},
    renderable::Renderable,
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;

#[test]
fn imported_mesh_roundtrips_through_stored_world_without_showing_its_collider() -> anyhow::Result<()> {
    let mesh = DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: Some(23),
        import_scale: 0.125,
        dehydrated_meshes: vec![DehydratedMesh {
            name: "mesh".to_string(),
            mesh_data_id: 19,
            skinned_skeleton_index: None,
            material: None,
        }],
        dehydrated_skeletons: Vec::new(),
    };
    let stored = dehydrated_gltf_to_stored_world("Test mesh", mesh)?;
    assert_eq!(stored.objectOfId[&stored.rootObjectId].name, "Test mesh");
    let ObjectInfo::Mesh(mesh) = &stored
        .objectOfId
        .values()
        .find(|object| match object.info {
            ObjectInfo::Mesh(_) => true,
            _ => false,
        })
        .unwrap()
        .info
    else {
        unreachable!()
    };
    assert_eq!(mesh.meshDataId, 19);
    assert_eq!(mesh.scale, glam::Vec3::splat(0.125));
    let object: StoredWorld = serde_json::from_str(&serde_json::to_string(&stored)?)?;
    assert_eq!(serde_json::to_value(&stored)?, serde_json::to_value(&object)?);
    let source = StoredWorldRenderable::new(stored)?;
    // Hiding the import collider must leave its mesh in the resolved render scene.
    let objects = source.getRenderSnapshot().render_object_of_id;
    let mesh_data_of_id = std::collections::HashMap::from([(
        19,
        std::sync::Arc::new(game_types::data_types::MeshData::StaticMeshData {
            vertices: [glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Y]
                .into_iter()
                .map(|position| game_types::data_types::MeshVertexInternal {
                    position,
                    normal: glam::Vec3::Z,
                    uv: glam::Vec2::ZERO,
                    tangent: glam::Vec4::X,
                    skinning: (),
                })
                .collect(),
            indices: vec![0, 1, 2],
        }),
    )]);
    let scene = build_render_scene(&objects, &mesh_data_of_id, None);
    assert_eq!(scene.instances.len(), 1);
    let RenderGeometry::Mesh { data_id, .. } = scene.instances[0].geometry else {
        panic!("Only the mesh should be visible, not its collider");
    };
    assert_eq!(data_id, 19);
    StoredWorldRenderable::new(object)?;
    Ok(())
}

// A mesh's tint multiplies its texture, so an imported mesh with a texture is tinted white to show the texture's own colors. One
// without a texture keeps the gray a new brick has.
#[test]
fn imported_meshes_show_their_textures_own_colors() -> anyhow::Result<()> {
    let textured_material = game_types::object_types::MaterialInfo {
        albedoTextureDataId: Some(31),
        metallicTextureDataId: None,
        roughnessTextureDataId: None,
        normalTextureDataId: None,
        alphaMode: game_types::object_types::material::AlphaMode::Opaque,
        isDoubleSided: false,
    };
    let mesh = DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes: vec![
            DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id: 19,
                skinned_skeleton_index: None,
                material: Some(DehydratedMaterial {
                    name: "material".to_string(),
                    material_info: textured_material,
                }),
            },
            DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id: 20,
                skinned_skeleton_index: None,
                material: None,
            },
        ],
        dehydrated_skeletons: Vec::new(),
    };
    let stored = dehydrated_gltf_to_stored_world("Test mesh", mesh)?;
    let color_of_mesh_data_id = |mesh_data_id| {
        stored.objectOfId.values().find_map(|object| match &object.info {
            ObjectInfo::Mesh(mesh) if mesh.meshDataId == mesh_data_id => {
                Some([mesh.visualSettings.color.r, mesh.visualSettings.color.g, mesh.visualSettings.color.b])
            }
            _ => None,
        })
    };
    assert_eq!(color_of_mesh_data_id(19), Some([1.0, 1.0, 1.0]));
    assert_eq!(color_of_mesh_data_id(20), Some([0.5, 0.5, 0.5]));
    Ok(())
}

// A model added to a world takes ids past the world's own, so neither one's objects are renumbered.
#[test]
fn a_model_added_to_a_world_takes_ids_past_the_worlds_own() -> anyhow::Result<()> {
    let model = |mesh_data_id| DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes: vec![DehydratedMesh {
            name: "mesh".to_string(),
            mesh_data_id,
            skinned_skeleton_index: None,
            material: None,
        }],
        dehydrated_skeletons: Vec::new(),
    };
    let mut stored = dehydrated_gltf_to_stored_world("First", model(19))?;
    let first_count = stored.objectOfId.len();
    let last_first_id = *stored.objectOfId.keys().max().unwrap();
    let root = stored.rootObjectId;
    let second = add_dehydrated_gltf_to_stored_world(&mut stored, "Second", model(20), root)?;
    assert_eq!(second, last_first_id + 1);
    assert!(stored.childrenOfId[&root].contains(&second));
    assert_eq!(stored.objectOfId.len(), 2 * first_count);
    let collider_of_mesh_data_id = |mesh_data_id| {
        stored.objectOfId.values().find_map(|object| match &object.info {
            ObjectInfo::Mesh(mesh) if mesh.meshDataId == mesh_data_id => mesh.physicalObjectId.as_ref().map(|pointer| pointer.0),
            _ => None,
        })
    };
    let (Some(first_collider), Some(second_collider)) = (collider_of_mesh_data_id(19), collider_of_mesh_data_id(20)) else {
        panic!("Both meshes should follow a collider");
    };
    assert!(first_collider <= last_first_id && second_collider > last_first_id);
    assert_eq!(stored.objectOfId[&second_collider].name, "physics_collider");
    StoredWorldRenderable::new(stored)?;
    Ok(())
}

#[test]
fn empty_gltf_is_rejected() {
    let mesh = DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes: Vec::new(),
        dehydrated_skeletons: Vec::new(),
    };
    assert!(dehydrated_gltf_to_stored_world("Empty", mesh).is_err());
}

// A material has no shape of its own, so its asset preview shows it on a ball that uses every one of its maps.
#[test]
fn material_preview_is_a_ball_wearing_the_material() -> anyhow::Result<()> {
    let material = game_types::object_types::MaterialInfo {
        albedoTextureDataId: Some(31),
        metallicTextureDataId: None,
        roughnessTextureDataId: Some(32),
        normalTextureDataId: Some(33),
        alphaMode: game_types::object_types::material::AlphaMode::Opaque,
        isDoubleSided: false,
    };
    let stored = render_core::material_to_stored_world::material_to_stored_world("Grass", material);
    assert_eq!(stored.objectOfId[&stored.rootObjectId].name, "Grass");
    let objects = StoredWorldRenderable::new(stored)?.getRenderSnapshot().render_object_of_id;
    let texture_data_ids = render_core::render_scene::collect_material_texture_data_ids(&objects);
    assert_eq!(texture_data_ids, std::collections::HashSet::from([31, 32, 33]));
    let scene = build_render_scene(&objects, &std::collections::HashMap::new(), None);
    assert_eq!(scene.instances.len(), 1);
    let RenderGeometry::Ball = scene.instances[0].geometry else {
        panic!("The preview should draw one ball");
    };
    Ok(())
}

// A skybox has no shape of its own either, so its asset preview is an empty world under the skybox, fully opaque.
#[test]
fn skybox_preview_is_an_empty_world_under_the_skybox() -> anyhow::Result<()> {
    let stored = render_core::skybox_to_stored_world::skybox_to_stored_world("Dusk", 41);
    assert_eq!(stored.objectOfId[&stored.rootObjectId].name, "Dusk");
    let objects = StoredWorldRenderable::new(stored)?.getRenderSnapshot().render_object_of_id;
    let world_lighting = render_core::render_scene::world_lighting_info_of_render_object_of_id(&objects);
    assert_eq!((world_lighting.skyboxTextureDataId, world_lighting.skyboxTextureOpacity), (Some(41), 1.0));
    assert!(build_render_scene(&objects, &std::collections::HashMap::new(), None).instances.is_empty());
    Ok(())
}

// An asset preview without lighting of its own is lit like a default world with gray sky and ambient colors, so nothing
// looks blue; a preview that brings its own lighting, like a skybox, keeps it.
#[test]
fn asset_previews_without_lighting_get_white_ambient_light_under_the_default_sky() -> anyhow::Result<()> {
    let material = game_types::object_types::MaterialInfo {
        albedoTextureDataId: Some(31),
        metallicTextureDataId: None,
        roughnessTextureDataId: None,
        normalTextureDataId: None,
        alphaMode: game_types::object_types::material::AlphaMode::Opaque,
        isDoubleSided: false,
    };
    let lighting_of = |stored: StoredWorld| -> anyhow::Result<game_types::object_types::WorldLightingInfo> {
        let objects = StoredWorldRenderable::new(stored)?.getRenderSnapshot().render_object_of_id;
        Ok(render_core::render_scene::world_lighting_info_of_render_object_of_id(&objects))
    };
    let mut material_preview = render_core::material_to_stored_world::material_to_stored_world("Grass", material);
    render_core::preview_lighting::add_neutral_preview_lighting(&mut material_preview);
    let lighting = lighting_of(material_preview)?;
    let default_lighting = game_types::object_types::WorldLightingInfo::default();
    let is_same_color = |a: game_types::object_types::shared::Color3, b: game_types::object_types::shared::Color3| a.r == b.r && a.g == b.g && a.b == b.b;
    for color in [lighting.ambientTopColor, lighting.ambientBottomColor] {
        assert!(
            is_same_color(color, game_types::object_types::shared::Color3::WHITE),
            "{color:?} should be white"
        );
    }
    assert!(is_same_color(lighting.sunColor, default_lighting.sunColor) && lighting.sunIntensity == default_lighting.sunIntensity);
    for (color, default_color) in [
        (lighting.skyTopColor, default_lighting.skyTopColor),
        (lighting.skyHorizonColor, default_lighting.skyHorizonColor),
        (lighting.skyBottomColor, default_lighting.skyBottomColor),
    ] {
        assert!(is_same_color(color, default_color), "{color:?} should be the default sky color");
    }

    let mut skybox_preview = render_core::skybox_to_stored_world::skybox_to_stored_world("Dusk", 41);
    render_core::preview_lighting::add_neutral_preview_lighting(&mut skybox_preview);
    let lighting = lighting_of(skybox_preview)?;
    assert_eq!(lighting.skyboxTextureDataId, Some(41));
    assert!(is_same_color(lighting.skyTopColor, default_lighting.skyTopColor));
    Ok(())
}

// The website shows a bare mesh file on an empty skeleton. None of an animated mesh's joints are in it, so every joint
// matrix stays the identity and the mesh is drawn in the pose it was modeled in, whatever its inverse bind matrices.
#[test]
fn a_skinned_mesh_on_an_empty_skeleton_is_drawn_in_its_modeled_pose() -> anyhow::Result<()> {
    use game_types::data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex};
    use glam::{Mat4, Vec2, Vec3, Vec4};
    let mesh = DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 1.0,
        dehydrated_meshes: vec![DehydratedMesh {
            name: "mesh".to_string(),
            mesh_data_id: 19,
            skinned_skeleton_index: Some(0),
            material: None,
        }],
        dehydrated_skeletons: vec![render_core::dehydrated_gltf::DehydratedSkeleton {
            skeleton_index: 0,
            jointOfIndex: Vec::new(),
            animations: Vec::new(),
        }],
    };
    let objects = StoredWorldRenderable::new(dehydrated_gltf_to_stored_world("Mesh 19", mesh)?)?
        .getRenderSnapshot()
        .render_object_of_id;
    let mesh_data_of_id = std::collections::HashMap::from([(
        19,
        std::sync::Arc::new(MeshData::SkinnedMeshData {
            vertices: [Vec3::ZERO, Vec3::X, Vec3::Y]
                .into_iter()
                .map(|position| MeshVertexInternal {
                    position,
                    normal: Vec3::Z,
                    uv: Vec2::ZERO,
                    tangent: Vec4::X,
                    skinning: SkinningInfoPerVertex {
                        jointIndices: [0; 4],
                        jointWeights: [1.0, 0.0, 0.0, 0.0],
                    },
                })
                .collect(),
            indices: vec![0, 1, 2],
            jointNameOfIdx: vec!["hips".to_string()],
            inverseBindMatrixOfJointName: std::collections::HashMap::from([("hips".to_string(), Mat4::from_translation(Vec3::NEG_Y).to_cols_array())]),
        }),
    )]);
    let poses = render_core::computeSkeletonPose::computeSkeletonPoseOfIdAtTime(&objects, 0.0, None);
    let scene = build_render_scene(&objects, &mesh_data_of_id, Some(&poses));
    assert_eq!(scene.instances.len(), 1);
    let [(19, _, joint_matrices)] = scene.joint_matrices.as_slice() else {
        panic!("The mesh should get one joint matrix buffer");
    };
    assert!(joint_matrices.matrices.iter().all(|matrix| *matrix == Mat4::IDENTITY.to_cols_array_2d()));
    Ok(())
}
