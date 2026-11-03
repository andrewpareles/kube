// Run with: cargo test -p render_web --lib --target wasm32-unknown-unknown --release --config 'target.wasm32-unknown-unknown.runner="wasm-bindgen-test-runner"'
use super::{camera_position_for_bounds, scene_bounds};
use game_types::data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use render_core::render_scene::{
    InstanceContent, JointMatrixBufferContent, MAX_JOINTS, MaterialTextureDataIds, RenderGeometry, RenderInstance, RenderScene, build_render_scene,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use wasm_bindgen_test::wasm_bindgen_test;

// ------------------------------ Preview Bounds ------------------------------

fn scene_with_geometry(geometry: RenderGeometry, model: Mat4) -> RenderScene {
    let mut scene = build_render_scene(&HashMap::new(), &HashMap::new(), None);
    scene.instances.push(RenderInstance {
        object_id: usize::MAX,
        geometry,
        content: InstanceContent {
            model: model.to_cols_array_2d(),
            color: [1.0; 4],
            texture_mapping_mode: 0,
            instance_flags: 0,
            glow: 0.0,
            _texture_mapping_padding: 0,
            texture_tile_transform: [0.0; 4],
            local_projection_scale: [0.0; 4],
        },
        material: MaterialTextureDataIds::default(),
        is_always_on_top: false,
    });
    scene
}

// Framing must use each primitive's dimensions after both rotation and nonuniform scaling.
#[wasm_bindgen_test]
fn preview_bounds_include_transformed_primitive_extents() {
    let center = Vec3::new(10.0, 20.0, 30.0);
    let model = Mat4::from_scale_rotation_translation(Vec3::new(2.0, 3.0, 4.0), Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), center);
    for (geometry, extent) in [
        (RenderGeometry::Ball, Vec3::new(3.0, 2.0, 4.0)),
        (RenderGeometry::Brick, Vec3::new(1.5, 1.0, 2.0)),
        (RenderGeometry::Cylinder, Vec3::new(3.0, 2.0, 2.0)),
    ] {
        let scene = scene_with_geometry(geometry, model);
        let (min, max) = scene_bounds(&scene, &HashMap::new(), None).unwrap();
        assert!(min.abs_diff_eq(center - extent, 0.00001));
        assert!(max.abs_diff_eq(center + extent, 0.00001));
    }
}

// Use actual mesh vertices and joint matrices: an import's placeholder primitive cannot frame its animated shape.
#[wasm_bindgen_test]
fn preview_mesh_bounds_follow_vertices_and_weighted_skinning() {
    let positions = [Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 0.0), Vec3::new(0.0, 1.0, 1.0)];
    let meshes = HashMap::from([
        (
            1,
            Arc::new(MeshData::StaticMeshData {
                vertices: positions
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
            }),
        ),
        (
            2,
            Arc::new(MeshData::SkinnedMeshData {
                vertices: positions
                    .into_iter()
                    .map(|position| MeshVertexInternal {
                        position,
                        normal: Vec3::Z,
                        uv: Vec2::ZERO,
                        tangent: Vec4::X,
                        skinning: SkinningInfoPerVertex {
                            jointIndices: [0, 1, 0, 0],
                            jointWeights: [0.25, 0.75, 0.0, 0.0],
                        },
                    })
                    .collect(),
                indices: vec![0, 1, 2],
                jointNameOfIdx: vec!["first".into(), "second".into()],
                inverseBindMatrixOfJointName: HashMap::new(),
            }),
        ),
    ]);
    let model = Mat4::from_scale_rotation_translation(Vec3::new(2.0, 3.0, 4.0), Quat::IDENTITY, Vec3::new(10.0, 20.0, 30.0));
    let static_scene = scene_with_geometry(RenderGeometry::Mesh { data_id: 1, skeleton_id: None }, model);
    assert_eq!(
        scene_bounds(&static_scene, &meshes, None),
        Some((Vec3::new(8.0, 20.0, 30.0), Vec3::new(12.0, 26.0, 34.0)))
    );

    let mut skinned_scene = scene_with_geometry(
        RenderGeometry::Mesh {
            data_id: 2,
            skeleton_id: Some(3),
        },
        model,
    );
    assert_eq!(scene_bounds(&skinned_scene, &meshes, None), scene_bounds(&static_scene, &meshes, None));
    let mut joints = JointMatrixBufferContent {
        matrices: [Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS],
    };
    joints.matrices[0] = Mat4::from_translation(Vec3::new(4.0, 0.0, 0.0)).to_cols_array_2d();
    joints.matrices[1] = Mat4::from_translation(Vec3::new(0.0, 8.0, 0.0)).to_cols_array_2d();
    skinned_scene.joint_matrices.push((2, 3, joints));
    assert_eq!(
        scene_bounds(&skinned_scene, &meshes, None),
        Some((Vec3::new(10.0, 38.0, 30.0), Vec3::new(14.0, 44.0, 34.0)))
    );

    assert_eq!(scene_bounds(&static_scene, &HashMap::new(), None), None, "unloaded meshes have no known bounds");
    let empty_scene = build_render_scene(&HashMap::new(), &HashMap::new(), None);
    assert_eq!(scene_bounds(&empty_scene, &meshes, None), None);
}

// Framing a group must include its descendants and exclude unrelated geometry elsewhere in the world.
#[wasm_bindgen_test]
fn object_bounds_include_only_requested_ids() {
    let mut scene = scene_with_geometry(RenderGeometry::Brick, Mat4::IDENTITY);
    scene.instances[0].object_id = 1;
    let mut child = scene_with_geometry(RenderGeometry::Brick, Mat4::from_translation(Vec3::new(4.0, 0.0, 0.0)));
    child.instances[0].object_id = 2;
    scene.instances.extend(child.instances);
    let mut unrelated = scene_with_geometry(RenderGeometry::Ball, Mat4::from_translation(Vec3::splat(1000.0)));
    unrelated.instances[0].object_id = 3;
    scene.instances.extend(unrelated.instances);
    let meshes = HashMap::new();
    assert_eq!(
        scene_bounds(&scene, &meshes, Some(&HashSet::from([1, 2]))),
        Some((Vec3::splat(-0.5), Vec3::new(4.5, 0.5, 0.5)))
    );
    assert_eq!(
        scene_bounds(&scene, &meshes, Some(&HashSet::from([2]))),
        Some((Vec3::new(3.5, -0.5, -0.5), Vec3::new(4.5, 0.5, 0.5)))
    );
    assert_eq!(scene_bounds(&scene, &meshes, Some(&HashSet::from([4]))), None);
}

// Every corner must be inside both screen axes and beyond the near plane, including narrow panels and tiny objects.
#[wasm_bindgen_test]
fn framed_bounds_fit_the_camera_at_different_angles_and_aspects() {
    let center = Vec3::new(8.0, -2.0, 12.0);
    for extent in [Vec3::new(0.1, 10.0, 2.0), Vec3::new(20.0, 1.0, 0.0), Vec3::splat(0.0001), Vec3::ZERO] {
        for aspect in [0.2, 1.0, 4.0] {
            for (yaw, pitch) in [(0.0, 0.0), (1.2, -0.8), (-2.4, 1.4)] {
                let position = camera_position_for_bounds((center - extent, center + extent), aspect, yaw, pitch, 1.0);
                let direction = Vec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), -yaw.cos() * pitch.cos());
                assert!((center - position).normalize().abs_diff_eq(direction, 0.0001));
                let view_proj = render_core::camera::build_camera_view_proj_matrix(position, center, aspect, render_core::camera::DEFAULT_CAMERA_FOV_DEGREES);
                for x in [-extent.x, extent.x] {
                    for y in [-extent.y, extent.y] {
                        for z in [-extent.z, extent.z] {
                            let point = view_proj.project_point3(center + Vec3::new(x, y, z));
                            assert!(
                                point.is_finite() && point.x.abs() <= 1.0 && point.y.abs() <= 1.0 && (0.0..=1.0).contains(&point.z),
                                "Projected corner {point:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
