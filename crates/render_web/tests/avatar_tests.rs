use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    data_types::{MeshData, MeshVertexInternal},
    object_types::{BrickInfo, MeshInfo, ObjectId, ObjectInfo, shared::Color3},
    stored_world_type::StoredWorld,
};
use glam::{Mat4, Vec2, Vec3, Vec4};
use render_core::{
    dehydrated_gltf::{DehydratedGltf, DehydratedMesh},
    render_scene::{RenderGeometry, build_render_scene},
    renderable::Renderable,
};
use render_web::{
    avatar::{AvatarData, AvatarFraming, face_framed_bounds, paint_avatar},
    stored_world_renderable_impl::StoredWorldRenderable,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

const DEFAULT_CHARACTER: &str = include_str!("../../shared/tests/fixtures/default_character.kube");
const ORANGE: [f32; 3] = [1.0, 128.0 / 255.0, 0.0];

fn hat() -> Value {
    let gltf = DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 0.5,
        dehydrated_meshes: vec![DehydratedMesh {
            name: "mesh".to_string(),
            mesh_data_id: 19,
            skinned_skeleton_index: None,
            material: None,
        }],
        dehydrated_skeletons: Vec::new(),
    };
    json!({ "name": "Sailor Hat", "dataJson": { "Mesh": gltf } })
}

fn avatar(character: Value, hat: Option<Value>, color: &str) -> anyhow::Result<StoredWorld> {
    let avatar: AvatarData = serde_json::from_value(json!({ "character": { "Object": character }, "hat": hat, "color": color, "isInRestPose": false }))?;
    avatar.into_stored_world()
}

// The skeleton's joints share the Body's part names, so parts are found among the bricks.
fn brick_ids_named(stored: &StoredWorld, name: &str) -> Vec<ObjectId> {
    stored
        .objectOfId
        .values()
        .filter(|object| object.name == name && matches!(object.info, ObjectInfo::Brick(_)))
        .map(|object| object.id)
        .collect()
}

fn brick_color(stored: &StoredWorld, name: &str) -> [f32; 3] {
    let [id] = brick_ids_named(stored, name)[..] else {
        panic!("There should be one {name} brick");
    };
    let ObjectInfo::Brick(BrickInfo { visualSettings, .. }) = &stored.objectOfId[&id].info else {
        unreachable!()
    };
    let Color3 { r, g, b } = visualSettings.color;
    [r, g, b]
}

fn meshes(stored: &StoredWorld) -> Vec<&MeshInfo> {
    stored
        .objectOfId
        .values()
        .filter_map(|object| match &object.info {
            ObjectInfo::Mesh(mesh) => Some(mesh),
            _ => None,
        })
        .collect()
}

// The color paints the Torso and the Cap, the rest of the character keeps its own colors, and the nametag is left off.
#[test]
fn an_avatar_wears_its_color() -> anyhow::Result<()> {
    let dressed = avatar(serde_json::from_str(DEFAULT_CHARACTER)?, None, "#ff8000")?;
    let green = avatar(serde_json::from_str(DEFAULT_CHARACTER)?, None, "#00ff00")?;
    assert_eq!(brick_color(&dressed, "Torso"), ORANGE);
    assert_eq!(brick_color(&dressed, "Cap"), ORANGE);
    assert_eq!(brick_color(&dressed, "LeftLeg"), brick_color(&green, "LeftLeg"));
    assert!(!dressed.objectOfId.values().any(|object| object.name == "Nametag"));
    Ok(())
}

// The hat goes on the HatSpot in place of the Cap. Its meshes follow the HatSpot, scaled to the width of the Head, and its own collider
// is left out.
#[test]
fn a_hat_goes_on_the_hat_spot_in_place_of_the_cap() -> anyhow::Result<()> {
    let mut character: Value = serde_json::from_str(DEFAULT_CHARACTER)?;
    let head_id = character["objectOfId"]
        .as_object()
        .and_then(|objects| {
            objects
                .iter()
                .find(|(_, object)| object["name"] == "Head" && object["info"].get("Brick").is_some())
        })
        .map(|(id, _)| id.clone())
        .expect("The default character should have a Head brick");
    character["primitiveSettingsOfId"][&head_id]["Brick"]["size"] = json!([2.0, 1.0, 2.0]);
    let stored = avatar(character, Some(hat()), "#ff8000")?;

    assert!(brick_ids_named(&stored, "Cap").is_empty());
    let [hat_spot] = brick_ids_named(&stored, "HatSpot")[..] else {
        panic!("There should be one HatSpot");
    };
    let hat_ids = &stored.childrenOfId[&hat_spot];
    assert!(hat_ids.iter().any(|id| stored.objectOfId[id].name == "Sailor Hat"));
    let [hat_mesh] = meshes(&stored)[..] else {
        panic!("The hat should bring one mesh");
    };
    assert_eq!(hat_mesh.physicalObjectId.as_ref().map(|pointer| pointer.0), Some(hat_spot));
    assert_eq!(hat_mesh.scale, Vec3::splat(1.0));
    assert!(!stored.objectOfId.values().any(|object| object.name.starts_with("physics_collider")));

    let objects = StoredWorldRenderable::new(stored)?.getRenderSnapshot().render_object_of_id;
    let triangle = MeshData::StaticMeshData {
        vertices: [Vec3::ZERO, Vec3::X, Vec3::Y]
            .into_iter()
            .map(|position| MeshVertexInternal {
                position,
                normal: Vec3::Z,
                uv: Vec2::ZERO,
                tangent: Vec4::X,
                skinning: (),
            })
            .collect(),
        indices: vec![0, 1, 2],
    };
    let scene = build_render_scene(&objects, &HashMap::from([(19, Arc::new(triangle))]), None);
    let hat_instance = scene
        .instances
        .iter()
        .find(|instance| matches!(instance.geometry, RenderGeometry::Mesh { data_id: 19, .. }))
        .expect("The hat should be drawn");
    let hat_position = Mat4::from_cols_array_2d(&hat_instance.content.model).w_axis.truncate();
    assert!(
        (hat_position - Vec3::new(0.0, 3.8, 0.0)).length() < 1e-4,
        "The hat should sit on the HatSpot, not at {hat_position}"
    );
    Ok(())
}

// On your own profile the avatar is repainted in place as you pick a color, hat and all.
#[test]
fn repainting_a_dressed_avatar_changes_its_torso() -> anyhow::Result<()> {
    let mut stored = avatar(serde_json::from_str(DEFAULT_CHARACTER)?, Some(hat()), "#ff8000")?;
    paint_avatar(&mut stored, Color3 { r: 0.0, g: 1.0, b: 0.0 })?;
    assert_eq!(brick_color(&stored, "Torso"), [0.0, 1.0, 0.0]);
    assert!(brick_ids_named(&stored, "Cap").is_empty());
    assert_eq!(meshes(&stored).len(), 1);
    Ok(())
}

#[test]
fn a_color_that_is_not_rrggbb_is_refused() {
    for color in ["ff8000", "#ff80", "#ff800000", "#gg8000"] {
        assert!(avatar(serde_json::from_str(DEFAULT_CHARACTER).unwrap(), None, color).is_err(), "{color}");
    }
}

// The preview frames the Head and the Torso.
#[test]
fn an_avatar_frames_its_head_and_torso() -> anyhow::Result<()> {
    let stored = avatar(serde_json::from_str(DEFAULT_CHARACTER)?, Some(hat()), "#ff8000")?;
    let framing = AvatarFraming::of(&stored).expect("the default character should have a Head");
    assert_eq!(framing.head_ids, brick_ids_named(&stored, "Head").into_iter().collect());
    assert_eq!(framing.torso_ids, brick_ids_named(&stored, "Torso").into_iter().collect());
    Ok(())
}

// The preview reaches from a tenth of the Torso below its bottom up over the tallest hat in the catalog, 1.875 Heads above the Head,
// so it is the same whatever hat the avatar wears. Only the height is framed.
#[test]
fn an_avatar_is_framed_from_the_bottom_of_its_torso_to_the_tallest_hat() {
    let head = (Vec3::new(-0.5, 2.8, -0.5), Vec3::new(0.5, 3.8, 0.5));
    let torso = (Vec3::new(-0.6, 1.4, -0.35), Vec3::new(0.6, 2.8, 0.35));
    let framed = face_framed_bounds(head, Some(torso));
    assert!(
        framed.0.abs_diff_eq(Vec3::new(0.0, 1.26, 0.0), 1e-5) && framed.1.abs_diff_eq(Vec3::new(0.0, 5.675, 0.0), 1e-5),
        "{framed:?}"
    );
}
