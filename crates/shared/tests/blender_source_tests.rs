use game_types::{consts::DEHYDRATED_GLTF_CURRENT_VERSION, object_types::ObjectInfo};
use render_core::{
    dehydrated_gltf::{BlenderId, DehydratedGltf, DehydratedMesh},
    dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world,
};
use shared::game::assets::AssetData;

fn model(blender_id: Option<BlenderId>) -> DehydratedGltf {
    DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id,
        import_scale: 1.0,
        dehydrated_meshes: [11, 12]
            .into_iter()
            .map(|mesh_data_id| DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id,
                skinned_skeleton_index: None,
                material: None,
            })
            .collect(),
        dehydrated_skeletons: Vec::new(),
    }
}

// Asset serialization preserves the source independently from every runtime mesh id.
#[test]
fn asset_mesh_keeps_its_optional_blender_source() -> anyhow::Result<()> {
    for blender_id in [None, Some(35)] {
        let item = AssetData::Mesh(model(blender_id));
        let json = serde_json::to_string(&item)?;
        let restored: AssetData = serde_json::from_str(&json)?;
        restored.validate()?;
        let AssetData::Mesh(gltf) = restored else {
            panic!("Expected a mesh asset");
        };
        assert_eq!(gltf.blender_id, blender_id);
        assert_eq!(gltf.dehydrated_meshes.iter().map(|mesh| mesh.mesh_data_id).collect::<Vec<_>>(), [11, 12]);

        // Rebuilding runtime objects still uses mesh data ids and needs no Blender download.
        let world = dehydrated_gltf_to_stored_world("Model", gltf)?;
        let mut ids = world
            .objectOfId
            .values()
            .filter_map(|object| match &object.info {
                ObjectInfo::Mesh(mesh) => Some(mesh.meshDataId),
                _ => None,
            })
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, [11, 12]);
    }
    Ok(())
}

#[test]
fn invalid_blender_ids_are_rejected_before_import() {
    for id in [0, -1, i64::MIN] {
        let gltf = model(Some(id));
        assert!(gltf.validate().unwrap_err().to_string().contains("Blender id"));
        assert!(dehydrated_gltf_to_stored_world("Invalid source", gltf).is_err());
    }
}
