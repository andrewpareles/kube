use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex},
};
use glam::{Vec2, Vec3, Vec4};
use render_core::dehydrated_gltf::DehydratedGltf;
use serde_json::json;
use shared::api::protocol_for_chat::{CatalogItemEditArgs, CatalogItemPublishArgs, check_catalog_item_is_a_hat, static_copy_of_rigged_mesh_data};
use std::{collections::HashMap, sync::Arc};

fn mesh_json(version: u64, mesh_data_ids: &[i64]) -> String {
    let dehydrated_meshes = mesh_data_ids
        .iter()
        .map(|mesh_data_id| json!({"name":"mesh","mesh_data_id":mesh_data_id,"skinned_skeleton_index":null,"material":null}))
        .collect::<Vec<_>>();
    json!({"version":version,"blender_id":7,"import_scale":1.0,"dehydrated_meshes":dehydrated_meshes,"dehydrated_skeletons":[]}).to_string()
}

#[test]
fn publishing_trims_text() {
    let mut args: CatalogItemPublishArgs =
        serde_json::from_value(json!({"asset_id":156,"name":" Top Hat ","description":" Tall. ","is_listed":true})).unwrap();
    args.validate().unwrap();
    assert_eq!(
        (args.asset_id, args.name.as_str(), args.description.as_deref()),
        (156, "Top Hat", Some("Tall."))
    );
}

#[test]
fn invalid_publishing_is_rejected_before_database_access() {
    let valid = json!({"asset_id":156,"name":"Hat","description":null,"is_listed":false});
    for (field, value) in [
        ("name", json!(" \n")),
        ("name", json!("a".repeat(101))),
        ("description", json!("a".repeat(1001))),
        ("asset_id", json!(0)),
        ("asset_id", json!(-1)),
    ] {
        let mut body = valid.clone();
        body[field] = value;
        let mut args: CatalogItemPublishArgs = serde_json::from_value(body.clone()).unwrap();
        assert!(args.validate().is_err(), "{body}");
    }
    let mut args: CatalogItemPublishArgs = serde_json::from_value(valid).unwrap();
    assert!(args.validate().is_ok());
    // An item always comes from an asset, so mesh JSON alone is not enough.
    for body in [
        json!({"mesh":mesh_json(DEHYDRATED_GLTF_CURRENT_VERSION, &[3]),"name":"Hat","is_listed":true}),
        json!({"asset_id":1.5,"name":"Hat","is_listed":true}),
        json!({"asset_id":"7","name":"Hat","is_listed":true}),
    ] {
        assert!(serde_json::from_value::<CatalogItemPublishArgs>(body).is_err());
    }
}

#[test]
fn editing_needs_a_change_and_holds_the_same_limits() {
    for body in [
        json!({"catalog_item_id":1}),
        json!({"catalog_item_id":0,"name":"Hat"}),
        json!({"catalog_item_id":1,"name":""}),
        json!({"catalog_item_id":1,"description":"a".repeat(1001)}),
    ] {
        let mut args: CatalogItemEditArgs = serde_json::from_value(body.clone()).unwrap();
        assert!(args.validate().is_err(), "{body}");
    }
    // An empty description clears it, while an omitted one is left alone.
    let mut args: CatalogItemEditArgs = serde_json::from_value(json!({"catalog_item_id":1,"description":" \n","is_listed":false})).unwrap();
    args.validate().unwrap();
    assert_eq!(
        serde_json::to_value(args).unwrap(),
        json!({"catalog_item_id":1,"name":null,"description":"","is_listed":false})
    );
}

fn hat_error(min: Vec3, max: Vec3, import_scale: f32) -> Option<String> {
    let vertex = |position| MeshVertexInternal {
        position,
        normal: Vec3::Y,
        uv: Vec2::ZERO,
        tangent: Vec4::ZERO,
        skinning: (),
    };
    let mesh_data = Arc::new(MeshData::StaticMeshData {
        vertices: vec![vertex(min), vertex(max)],
        indices: Vec::new(),
    });
    let mut mesh: DehydratedGltf = serde_json::from_str(&mesh_json(DEHYDRATED_GLTF_CURRENT_VERSION, &[3])).unwrap();
    mesh.import_scale = import_scale;
    check_catalog_item_is_a_hat(&mesh, &HashMap::from([(3, mesh_data)]))
        .err()
        .map(|error| error.to_string())
}

// The default character's head is a 1-unit cube, and a hat's origin is the top middle of it.
#[test]
fn a_catalog_item_must_be_hat_sized_and_sit_on_the_head() {
    let top_hat = hat_error(Vec3::new(-0.6, 0.0, -0.6), Vec3::new(0.6, 0.8, 0.6), 1.0);
    let beanie = hat_error(Vec3::new(-0.55, -0.5, -0.55), Vec3::new(0.55, 0.2, 0.55), 1.0);
    let modeled_ten_times_bigger = hat_error(Vec3::new(-6.0, 0.0, -6.0), Vec3::new(6.0, 8.0, 6.0), 0.1);
    assert_eq!((top_hat, beanie, modeled_ten_times_bigger), (None, None, None));
    for (min, max, message) in [
        (Vec3::new(-0.1, 0.0, -0.1), Vec3::new(0.1, 0.2, 0.1), "widest side is 0.20"),
        (Vec3::new(-1.5, 0.0, -1.5), Vec3::new(1.5, 1.0, 1.5), "widest side is 3.00"),
        (Vec3::new(0.0, 0.0, -0.5), Vec3::new(1.0, 1.0, 0.5), "0.50 units across"),
        (Vec3::new(-0.5, 1.0, -0.5), Vec3::new(0.5, 1.5, 0.5), "lowest point is at 1.00"),
        (Vec3::new(-0.5, -1.5, -0.5), Vec3::new(0.5, 0.0, 0.5), "lowest point is at -1.50"),
    ] {
        let error = hat_error(min, max, 1.0).unwrap_or_default();
        assert!(error.contains(message), "{min} to {max}: {error}");
    }
    let mesh: DehydratedGltf = serde_json::from_str(&mesh_json(DEHYDRATED_GLTF_CURRENT_VERSION, &[3])).unwrap();
    let missing = check_catalog_item_is_a_hat(&mesh, &HashMap::new()).unwrap_err();
    assert!(missing.to_string().contains("could not be read"), "{missing}");
}

// Every player can wear a hat, so a hat has at most 10,000 triangles, and 3 vertices for each.
#[test]
fn a_hat_has_at_most_10000_triangles_and_30000_vertices() {
    let hat_error = |vertex_count: usize, triangle_count: usize| {
        let vertex = |position| MeshVertexInternal {
            position,
            normal: Vec3::Y,
            uv: Vec2::ZERO,
            tangent: Vec4::ZERO,
            skinning: (),
        };
        let mesh_data = Arc::new(MeshData::StaticMeshData {
            vertices: [
                vec![vertex(Vec3::new(-0.6, 0.0, -0.6))],
                vec![vertex(Vec3::new(0.6, 0.8, 0.6)); vertex_count - 1],
            ]
            .concat(),
            indices: vec![0; triangle_count * 3],
        });
        let mesh: DehydratedGltf = serde_json::from_str(&mesh_json(DEHYDRATED_GLTF_CURRENT_VERSION, &[3])).unwrap();
        check_catalog_item_is_a_hat(&mesh, &HashMap::from([(3, mesh_data)]))
            .err()
            .map(|error| error.to_string())
    };
    assert_eq!(hat_error(30_000, 10_000), None);
    for (vertex_count, triangle_count) in [(2, 10_001), (30_001, 1)] {
        let error = hat_error(vertex_count, triangle_count).unwrap_or_default();
        assert!(
            error.contains(&format!("This model has {triangle_count} triangles and {vertex_count} vertices")),
            "{error}"
        );
    }
}

// Catalog items are static, so a rigged mesh is drawn from a static copy of its geometry with the same vertices and triangles.
#[test]
fn a_rigged_mesh_gets_a_static_copy_with_the_same_geometry() {
    let rigged = MeshData::SkinnedMeshData {
        vertices: vec![MeshVertexInternal {
            position: Vec3::new(1.0, 2.0, 3.0),
            normal: Vec3::Y,
            uv: Vec2::new(0.5, 0.25),
            tangent: Vec4::X,
            skinning: SkinningInfoPerVertex {
                jointIndices: [0, 0, 0, 0],
                jointWeights: [1.0, 0.0, 0.0, 0.0],
            },
        }],
        indices: vec![0, 0, 0],
        jointNameOfIdx: vec!["Bone".to_string()],
        inverseBindMatrixOfJointName: HashMap::new(),
    };
    let Some(MeshData::StaticMeshData { vertices, indices }) = static_copy_of_rigged_mesh_data(&rigged) else {
        panic!("A rigged mesh should get a static copy");
    };
    assert_eq!(
        (vertices[0].position, vertices[0].uv, indices.clone()),
        (Vec3::new(1.0, 2.0, 3.0), Vec2::new(0.5, 0.25), vec![0, 0, 0])
    );
    assert!(static_copy_of_rigged_mesh_data(&MeshData::StaticMeshData { vertices, indices }).is_none());
}
