use game_types::{consts::DEHYDRATED_GLTF_CURRENT_VERSION, object_types::ObjectInfo};
use render_core::{
    dehydrated_gltf::{DehydratedGltf, DehydratedMesh},
    dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world,
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;
use std::collections::HashSet;

// React supplies one selected ID. The preview resolves only that object's authored subtree.
#[test]
fn selection_includes_descendants_without_parents_or_siblings() -> anyhow::Result<()> {
    let stored = dehydrated_gltf_to_stored_world(
        "Group",
        DehydratedGltf {
            version: DEHYDRATED_GLTF_CURRENT_VERSION,
            blender_id: None,
            import_scale: 1.0,
            dehydrated_meshes: vec![
                DehydratedMesh {
                    name: "mesh".to_string(),
                    mesh_data_id: 1,
                    skinned_skeleton_index: None,
                    material: None,
                },
                DehydratedMesh {
                    name: "mesh".to_string(),
                    mesh_data_id: 2,
                    skinned_skeleton_index: None,
                    material: None,
                },
            ],
            dehydrated_skeletons: Vec::new(),
        },
    )?;
    let root_id = stored.rootObjectId;
    let all_ids: HashSet<_> = stored.objectOfId.keys().copied().collect();
    let mesh_ids: Vec<_> = stored
        .objectOfId
        .values()
        .filter_map(|object| match object.info {
            ObjectInfo::Mesh(_) => Some(object.id),
            _ => None,
        })
        .collect();
    assert_eq!(mesh_ids.len(), 2);
    let source = StoredWorldRenderable::new(stored)?;
    assert_eq!(source.object_and_descendant_ids(root_id), all_ids);
    for mesh_id in mesh_ids {
        assert_eq!(source.object_and_descendant_ids(mesh_id), HashSet::from([mesh_id]));
    }
    assert!(source.object_and_descendant_ids(usize::MAX).is_empty());
    Ok(())
}
