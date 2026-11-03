use bytemuck::Zeroable;
use game_types::data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix, build_ray_from_view_proj_ndc},
    picking::pick_scene_object,
    render_scene::{
        FontAtlas, InstanceContent, JointMatrixBufferContent, LabelContent, LabelGeometry, MAX_JOINTS, MaterialTextureDataIds, RenderGeometry, RenderInstance,
        RenderLabel, RenderScene, VertexContent, build_render_scene,
    },
};
use std::{collections::HashMap, sync::Arc};

fn view() -> Mat4 {
    build_camera_view_proj_matrix(Vec3::Z * 5.0, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES)
}
fn instance(object_id: usize, geometry: RenderGeometry, model: Mat4) -> RenderInstance {
    RenderInstance {
        object_id,
        geometry,
        content: InstanceContent {
            model: model.to_cols_array_2d(),
            color: [1.0; 4],
            ..InstanceContent::zeroed()
        },
        material: MaterialTextureDataIds::default(),
        is_always_on_top: false,
    }
}
fn scene(instances: Vec<RenderInstance>) -> RenderScene {
    let mut scene = build_render_scene(&HashMap::new(), &HashMap::new(), None);
    scene.instances = instances;
    scene
}
fn mesh(positions: &[Vec3]) -> MeshData {
    MeshData::StaticMeshData {
        vertices: positions
            .iter()
            .map(|&position| MeshVertexInternal {
                position,
                normal: Vec3::Z,
                uv: Vec2::ZERO,
                tangent: Vec4::X,
                skinning: (),
            })
            .collect(),
        indices: vec![0, 1, 2],
    }
}

#[test]
fn picking_ray_projects_back_to_requested_ndc() {
    let position = Vec3::new(7.0, 3.0, 5.0);
    let target = Vec3::new(-1.0, 2.0, 0.0);
    let projection = build_camera_view_proj_matrix(position, target, 1.7, DEFAULT_CAMERA_FOV_DEGREES);
    for (x, y) in [(0.0, 0.0), (-0.7, 0.3), (0.9, -0.4)] {
        let (origin, direction) = build_ray_from_view_proj_ndc(projection, x, y).unwrap();
        let projected = projection.project_point3(origin + direction * 10.0);
        assert!(Vec2::new(projected.x, projected.y).abs_diff_eq(Vec2::new(x, y), 1e-4));
    }
}

#[test]
fn every_primitive_is_pickable_after_rotation_and_nonuniform_scale() {
    for geometry in [RenderGeometry::Brick, RenderGeometry::Ball, RenderGeometry::Cylinder] {
        let model = Mat4::from_scale_rotation_translation(Vec3::new(2.0, 0.6, 1.4), Quat::from_rotation_y(0.7), Vec3::ZERO);
        let scene = scene(vec![instance(1, geometry, model)]);
        assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.0, 0.0), Some(1));
        assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.95, 0.95), None);
    }
}

#[test]
fn nearest_hit_uses_world_distance_and_front_layer_takes_priority() {
    let mut scene = scene(vec![
        instance(
            1,
            RenderGeometry::Brick,
            Mat4::from_scale_rotation_translation(Vec3::splat(0.2), Quat::IDENTITY, Vec3::Z),
        ),
        instance(
            2,
            RenderGeometry::Brick,
            Mat4::from_scale_rotation_translation(Vec3::splat(4.0), Quat::IDENTITY, -Vec3::Z * 3.0),
        ),
    ]);
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.0, 0.0), Some(1));
    scene.instances[1].is_always_on_top = true;
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.0, 0.0), Some(2));
    scene.instances[1].content.color[3] = 0.0;
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.0, 0.0), Some(1));
    scene.instances[0].content.color[3] = 0.0;
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.0, 0.0), None);
}

#[test]
fn meshes_hit_triangles_not_their_boxes_and_respect_back_faces() {
    let meshes = HashMap::from([(
        1,
        Arc::new(mesh(&[Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(-1.0, 1.0, 0.0)])),
    )]);
    let mut scene = scene(vec![instance(3, RenderGeometry::Mesh { data_id: 1, skeleton_id: None }, Mat4::IDENTITY)]);
    let point = view().project_point3(Vec3::new(-0.5, -0.5, 0.0));
    assert_eq!(pick_scene_object(&scene, &meshes, view(), point.x, point.y), Some(3));
    let miss = view().project_point3(Vec3::new(0.75, 0.75, 0.0));
    assert_eq!(pick_scene_object(&scene, &meshes, view(), miss.x, miss.y), None);
    scene.instances[0].content.model = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)).to_cols_array_2d();
    assert_eq!(pick_scene_object(&scene, &meshes, view(), -point.x, point.y), None);
    scene.instances[0].material.is_double_sided = true;
    assert_eq!(pick_scene_object(&scene, &meshes, view(), -point.x, point.y), Some(3));
}

#[test]
fn skinned_hits_follow_each_skeleton_and_normalize_only_usable_weights() {
    let vertices = [Vec3::new(4.0, -1.0, 0.0), Vec3::new(6.0, -1.0, 0.0), Vec3::new(5.0, 1.0, 0.0)]
        .into_iter()
        .map(|position| MeshVertexInternal {
            position,
            normal: Vec3::Z,
            uv: Vec2::ZERO,
            tangent: Vec4::X,
            skinning: SkinningInfoPerVertex {
                jointIndices: [0, MAX_JOINTS as u32, 0, 0],
                jointWeights: [2.0, 10.0, 0.0, 0.0],
            },
        })
        .collect();
    let meshes = HashMap::from([(
        1,
        Arc::new(MeshData::SkinnedMeshData {
            vertices,
            indices: vec![0, 1, 2],
            jointNameOfIdx: vec!["joint".into()],
            inverseBindMatrixOfJointName: HashMap::new(),
        }),
    )]);
    let mut scene = scene(vec![
        instance(
            7,
            RenderGeometry::Mesh {
                data_id: 1,
                skeleton_id: Some(1),
            },
            Mat4::IDENTITY,
        ),
        instance(
            8,
            RenderGeometry::Mesh {
                data_id: 1,
                skeleton_id: Some(2),
            },
            Mat4::IDENTITY,
        ),
    ]);
    let mut moved = JointMatrixBufferContent {
        matrices: [Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS],
    };
    moved.matrices[0] = Mat4::from_translation(Vec3::new(-5.0, 0.0, 0.0)).to_cols_array_2d();
    scene.joint_matrices = vec![
        (
            1,
            1,
            JointMatrixBufferContent {
                matrices: [Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS],
            },
        ),
        (1, 2, moved),
    ];
    assert_eq!(pick_scene_object(&scene, &meshes, view(), 0.0, 0.0), Some(8));
    let (_, _, joints) = &mut scene.joint_matrices[1];
    joints.matrices[0] = Mat4::IDENTITY.to_cols_array_2d();
    assert_eq!(pick_scene_object(&scene, &meshes, view(), 0.0, 0.0), None);
}

#[test]
fn labels_follow_billboard_and_constant_size_transforms() {
    let position = Vec3::new(0.5, 0.0, -5.0);
    let mut content = InstanceContent {
        model: Mat4::from_rotation_translation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), position).to_cols_array_2d(),
        color: [1.0; 4],
        ..InstanceContent::zeroed()
    };
    // These bits are the existing shader's billboard and constant-size flags.
    content.instance_flags = 3;
    let geometry_arc = Arc::new(LabelGeometry {
        cache_key: 1,
        background_index_count: 0,
        vertices: [Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)]
            .into_iter()
            .map(|position| VertexContent {
                position: position.to_array(),
                normal: Vec3::Z.to_array(),
                uv: [0.0; 2],
                tangent: Vec4::X.to_array(),
            })
            .collect(),
        indices: vec![0, 1, 2],
    });
    let mut scene = scene(Vec::new());
    scene.labels.push(RenderLabel {
        object_id: 10,
        content: LabelContent::Text {
            geometry: geometry_arc,
            atlas: Arc::new(FontAtlas {
                version: 1,
                width: 1,
                height: 1,
                mip_levels: vec![vec![255]],
            }),
            instances: [content; 2],
            draws_background: false,
        },
        is_always_on_top: false,
    });
    let point = view().project_point3(position);
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), point.x, point.y), Some(10));
    let LabelContent::Text { instances, .. } = &mut scene.labels[0].content else {
        unreachable!()
    };
    instances[1].color[3] = 0.0;
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), point.x, point.y), None);
}

#[test]
fn empty_space_invalid_inputs_clipped_objects_and_singular_transforms_do_not_hit() {
    let scene = scene(vec![
        instance(1, RenderGeometry::Brick, Mat4::ZERO),
        instance(2, RenderGeometry::Brick, Mat4::from_translation(Vec3::Z * 7.0)),
    ]);
    for (x, y) in [(0.0, 0.0), (f32::NAN, 0.0), (0.0, f32::INFINITY), (2.0, 0.0)] {
        assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), x, y), None);
    }
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), Mat4::ZERO, 0.0, 0.0), None);
    assert!(build_ray_from_view_proj_ndc(Mat4::ZERO, 0.0, 0.0).is_none());
}

#[test]
fn flattened_visible_faces_are_pickable_without_an_invertible_transform() {
    let scene = scene(vec![instance(1, RenderGeometry::Brick, Mat4::from_scale(Vec3::new(2.0, 2.0, 0.0)))]);
    assert_eq!(pick_scene_object(&scene, &HashMap::new(), view(), 0.0, 0.0), Some(1));
}
