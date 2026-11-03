use render_core::{
    render_scene::collect_material_texture_data_ids,
    renderer_types::{RenderBall, RenderBrick, RenderCylinder, RenderMesh, RenderObject, RenderTransform},
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use client::simple_renderer::decode_texture_data_to_mip_data;
use client::texture_lru_tracker::TextureLruTracker;
use game_types::data_types::TextureData;
use game_types::object_types::{
    BallMaterialSettings, BrickMaterialSettings, CylinderMaterialSettings, MaterialInfo, MeshMaterialSettings, ObjectIdPointer, PhysicsInfo3D, VisualSettings,
    material::AlphaMode,
};
use game_types::physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings};

use glam::{Quat, Vec3};

fn default_physics_info_3d() -> PhysicsInfo3D {
    PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO)
}

#[test]
fn collect_material_texture_data_ids_includes_referenced_material_texture_ids() {
    let material_object_id = 100;
    let mesh_object_id = 101;
    let ball_object_id = 102;
    let brick_object_id = 103;
    let cylinder_object_id = 104;

    let mut render_object_of_id = HashMap::new();
    render_object_of_id.insert(
        material_object_id,
        RenderObject::Material(MaterialInfo {
            albedoTextureDataId: Some(9001),
            metallicTextureDataId: Some(9002),
            roughnessTextureDataId: Some(9003),
            normalTextureDataId: Some(9004),
            alphaMode: AlphaMode::Opaque,
            isDoubleSided: false,
        }),
    );
    render_object_of_id.insert(
        mesh_object_id,
        RenderObject::Mesh(RenderMesh {
            physicalObjectId: 102,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: Some(MeshMaterialSettings {
                materialInfoId: ObjectIdPointer(material_object_id),
            }),
            meshDataId: 456,
            scale: Vec3::ONE,
            skin_skeleton_id: None,
        }),
    );
    render_object_of_id.insert(
        ball_object_id,
        RenderObject::Ball(RenderBall {
            transform: RenderTransform {
                teleport_count: 0,
                physicsInfo3D: default_physics_info_3d(),
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: Some(BallMaterialSettings {
                materialInfoId: ObjectIdPointer(material_object_id),
                tileSizeU: 1.0,
                tileSizeV: 1.0,
                tileOffsetU: 0.0,
                tileOffsetV: 0.0,
            }),
            primitive: BallPrimitiveSettings { radius: 1.0 },
        }),
    );
    render_object_of_id.insert(
        brick_object_id,
        RenderObject::Brick(RenderBrick {
            transform: RenderTransform {
                teleport_count: 0,
                physicsInfo3D: default_physics_info_3d(),
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: Some(BrickMaterialSettings {
                materialInfoId: ObjectIdPointer(material_object_id),
                tileSizeU: 1.0,
                tileSizeV: 1.0,
                tileOffsetU: 0.0,
                tileOffsetV: 0.0,
            }),
            primitive: BrickPrimitiveSettings {
                size: Vec3::new(2.0, 2.0, 2.0),
            },
        }),
    );
    render_object_of_id.insert(
        cylinder_object_id,
        RenderObject::Cylinder(RenderCylinder {
            transform: RenderTransform {
                teleport_count: 0,
                physicsInfo3D: default_physics_info_3d(),
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: Some(CylinderMaterialSettings {
                materialInfoId: ObjectIdPointer(material_object_id),
                tileSizeU: 1.0,
                tileSizeV: 1.0,
                tileOffsetU: 0.0,
                tileOffsetV: 0.0,
            }),
            primitive: CylinderPrimitiveSettings { radius: 1.0, length: 2.0 },
        }),
    );

    let texture_data_ids = collect_material_texture_data_ids(&render_object_of_id);
    let expected_texture_data_ids: HashSet<i64> = [9001, 9002, 9003, 9004].into_iter().collect();
    assert_eq!(texture_data_ids, expected_texture_data_ids);
}

#[test]
fn collect_material_texture_data_ids_ignores_unreferenced_materials() {
    let referenced_material_object_id = 200;
    let unreferenced_material_object_id = 201;
    let mesh_object_id = 202;

    let mut render_object_of_id = HashMap::new();
    render_object_of_id.insert(
        referenced_material_object_id,
        RenderObject::Material(MaterialInfo {
            albedoTextureDataId: Some(9001),
            metallicTextureDataId: Some(9002),
            roughnessTextureDataId: Some(9003),
            normalTextureDataId: Some(9004),
            alphaMode: AlphaMode::Opaque,
            isDoubleSided: false,
        }),
    );
    render_object_of_id.insert(
        unreferenced_material_object_id,
        RenderObject::Material(MaterialInfo {
            albedoTextureDataId: Some(9101),
            metallicTextureDataId: Some(9102),
            roughnessTextureDataId: Some(9103),
            normalTextureDataId: Some(9104),
            alphaMode: AlphaMode::Opaque,
            isDoubleSided: false,
        }),
    );
    render_object_of_id.insert(
        mesh_object_id,
        RenderObject::Mesh(RenderMesh {
            physicalObjectId: 203,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: Some(MeshMaterialSettings {
                materialInfoId: ObjectIdPointer(referenced_material_object_id),
            }),
            meshDataId: 456,
            scale: Vec3::ONE,
            skin_skeleton_id: None,
        }),
    );

    let texture_data_ids = collect_material_texture_data_ids(&render_object_of_id);
    let expected_texture_data_ids: HashSet<i64> = [9001, 9002, 9003, 9004].into_iter().collect();
    assert_eq!(texture_data_ids, expected_texture_data_ids);
}

#[test]
fn decode_texture_data_to_mip_data_returns_error_for_invalid_ktx2_bytes() {
    let texture_data = TextureData {
        ktx2Data: vec![1, 2, 3, 4, 5],
        width: 1,
        height: 1,
        mipLevelCount: 1,
        isSrgb: false,
        hasAlpha: false,
    };

    let decode_result = decode_texture_data_to_mip_data(&texture_data, wgpu::TextureFormat::Rgba8Unorm);
    assert!(decode_result.is_err());
}

// -------------------------- dropping textures from the GPU once they pass the budget --------------------------

// Four 100-byte textures on the GPU, last drawn 1 to 4 seconds after the start, in id order.
fn four_textures_last_drawn_in_id_order(byte_budget: u64) -> TextureLruTracker {
    let start = Instant::now();
    let mut texture_lru_tracker = TextureLruTracker::new(byte_budget);
    for texture_data_id in [1, 2, 3, 4] {
        texture_lru_tracker.set_last_drawn_time(&HashSet::from([texture_data_id]), start + Duration::from_secs(texture_data_id as u64));
        texture_lru_tracker.add_texture(texture_data_id, 100);
    }
    texture_lru_tracker
}

#[test]
fn textures_under_the_budget_all_stay_on_the_gpu() {
    let mut texture_lru_tracker = four_textures_last_drawn_in_id_order(400);
    let removed = texture_lru_tracker.remove_textures_over_budget(&HashSet::new());
    assert!(removed.is_empty(), "400 bytes fit a 400-byte budget, so nothing should be dropped: {removed:?}");
    assert_eq!(texture_lru_tracker.get_tracked_data_ids(), HashSet::from([1, 2, 3, 4]));
}

#[test]
fn over_the_budget_the_least_recently_drawn_textures_go_until_the_rest_fit() {
    let mut texture_lru_tracker = four_textures_last_drawn_in_id_order(250);
    let removed = texture_lru_tracker.remove_textures_over_budget(&HashSet::new());
    assert_eq!(
        removed,
        vec![1, 2],
        "dropping the two oldest leaves 200 bytes, which fits; a third would be dropped for nothing"
    );
    assert_eq!(texture_lru_tracker.get_tracked_data_ids(), HashSet::from([3, 4]));
    assert!(
        texture_lru_tracker.remove_textures_over_budget(&HashSet::new()).is_empty(),
        "what's left already fits"
    );
}

#[test]
fn textures_this_frame_draws_stay_even_when_they_are_the_oldest_or_over_the_budget() {
    let mut texture_lru_tracker = four_textures_last_drawn_in_id_order(250);
    let removed = texture_lru_tracker.remove_textures_over_budget(&HashSet::from([1]));
    assert_eq!(removed, vec![2, 3], "texture 1 is drawn this frame, so the next two oldest go instead");

    let mut texture_lru_tracker = four_textures_last_drawn_in_id_order(0);
    let removed = texture_lru_tracker.remove_textures_over_budget(&HashSet::from([1, 2, 3, 4]));
    assert!(
        removed.is_empty(),
        "a texture the frame draws is never dropped, even past the budget: {removed:?}"
    );
}

#[test]
fn a_preloaded_texture_that_was_never_drawn_goes_before_any_drawn_one() {
    let mut texture_lru_tracker = four_textures_last_drawn_in_id_order(400);
    texture_lru_tracker.add_texture(5, 100);
    let removed = texture_lru_tracker.remove_textures_over_budget(&HashSet::new());
    assert_eq!(
        removed,
        vec![5],
        "texture 5 was preloaded and never drawn, so it goes before texture 1, the oldest drawn one"
    );
}

#[test]
fn drawing_a_texture_again_makes_it_the_last_to_go() {
    let mut texture_lru_tracker = four_textures_last_drawn_in_id_order(300);
    texture_lru_tracker.set_last_drawn_time(&HashSet::from([1]), Instant::now() + Duration::from_secs(10));
    let removed = texture_lru_tracker.remove_textures_over_budget(&HashSet::new());
    assert_eq!(removed, vec![2], "texture 1 was drawn most recently, so texture 2 is now the oldest");
}
