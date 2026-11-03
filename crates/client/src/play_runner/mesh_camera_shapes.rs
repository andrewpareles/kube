// Meshes own no colliders, so the camera tests their triangles: the same camera ball that stops at bricks stops at the
// first triangle it would touch. A mesh's triangles go into a tree the first time the camera comes near it. An animated
// mesh is cut into one piece for each joint, and each piece moves with its joint, so nothing is rebuilt as it animates.

use game_types::{
    consts::CAMERA_SIZE,
    data_types::{DataId, MeshData},
    object_types::{Location3D, ObjectId, ObjectInfo, ObjectKind},
};
use glam::{Mat3, Mat4, Quat, Vec3};
use rapier3d::parry::{
    bounding_volume::Aabb,
    math::Pose,
    query::{Ray, RayCast, ShapeCastOptions, cast_shapes},
    shape::{Ball, TriMesh, Triangle},
};
use render_core::{
    computeSkeletonPose::{SkeletonPose, computeSkeletonPoseAtTime},
    render_scene::build_joint_matrix_buffer_content,
};
use shared::game::world::World;
use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

// TODO: AI-PICKED-VALUE: An animated mesh can reach past the box around its rest pose, so the camera tests it once it comes within a box twice that size.
const ANIMATED_MESH_REACH_SCALE: f32 = 2.0;

// Triangles that move together: all of a still mesh, or the part of an animated mesh that one joint moves the most.
struct MeshCameraPiece {
    joint_index: Option<usize>,
    triangles: TriMesh,
}

struct MeshCameraShape {
    mesh_data_arc: Arc<MeshData>,
    rest_min: Vec3,
    rest_max: Vec3,
    pieces: Option<Vec<MeshCameraPiece>>, // built the first time the camera comes near the mesh
}

pub struct MeshCameraShapes {
    shape_of_mesh_data_id: HashMap<DataId, MeshCameraShape>,
}

fn positions_of_mesh_data(mesh_data: &MeshData) -> (Vec<Vec3>, &[u32]) {
    match mesh_data {
        MeshData::StaticMeshData { vertices, indices } => (vertices.iter().map(|vertex| vertex.position).collect(), indices),
        MeshData::SkinnedMeshData { vertices, indices, .. } => (vertices.iter().map(|vertex| vertex.position).collect(), indices),
    }
}

// When the camera ball first touches the box that `model` puts this local box in, as a fraction of `translation`.
fn camera_ball_toi_of_box(local_min: Vec3, local_max: Vec3, model: Mat4, origin: Vec3, translation: Vec3) -> Option<f32> {
    let linear = Mat3::from_mat4(model);
    let abs_linear = Mat3::from_cols(linear.x_axis.abs(), linear.y_axis.abs(), linear.z_axis.abs());
    let center = model.transform_point3((local_min + local_max) * 0.5);
    let half_extents = abs_linear * ((local_max - local_min) * 0.5) + Vec3::splat(CAMERA_SIZE);
    Aabb::new(center - half_extents, center + half_extents).cast_local_ray(&Ray::new(origin, translation), 1.0, true)
}

impl MeshCameraShapes {
    pub fn new() -> Self {
        Self {
            shape_of_mesh_data_id: HashMap::new(),
        }
    }

    // How far the camera ball can move from `origin` along `translation` before it touches a mesh. Meshes block it when
    // they have downloaded and follow a solid object the camera doesn't ignore, however see-through they are.
    pub fn cast_camera_ball(
        &mut self,
        world: &World,
        get_mesh_data: &dyn Fn(DataId) -> Option<Arc<MeshData>>,
        origin: Vec3,
        translation: Vec3,
        should_ignore_object: &dyn Fn(ObjectId) -> bool,
        est_dely_server_sim_ms: f64,
        live_client_sim_ms: f64,
    ) -> Option<f32> {
        // Sort the triangles into pieces by the joint that moves each one the most, adding up its corners' weights.
        fn build_pieces(mesh_data: &MeshData) -> Vec<MeshCameraPiece> {
            let (positions, indices) = positions_of_mesh_data(mesh_data);
            let mut triangles_of_joint_index: HashMap<Option<usize>, Vec<[u32; 3]>> = HashMap::new();
            let mut weight_of_joint_index: Vec<(u32, f32)> = Vec::new();
            for triangle in indices.chunks_exact(3) {
                let is_drawable = triangle
                    .iter()
                    .all(|&vertex_index| positions.get(vertex_index as usize).is_some_and(|position| position.is_finite()));
                if !is_drawable {
                    continue;
                }
                let joint_index = match mesh_data {
                    MeshData::StaticMeshData { .. } => None,
                    MeshData::SkinnedMeshData { vertices, .. } => {
                        weight_of_joint_index.clear();
                        for &vertex_index in triangle {
                            let skinning = &vertices[vertex_index as usize].skinning;
                            for (&joint_index, &weight) in skinning.jointIndices.iter().zip(&skinning.jointWeights) {
                                if weight <= 0.0 {
                                    continue;
                                }
                                match weight_of_joint_index.iter_mut().find(|(index, _)| *index == joint_index) {
                                    Some((_, total_weight)) => *total_weight += weight,
                                    None => weight_of_joint_index.push((joint_index, weight)),
                                }
                            }
                        }
                        weight_of_joint_index
                            .iter()
                            .max_by(|(_, weight_a), (_, weight_b)| weight_a.total_cmp(weight_b))
                            .map(|&(joint_index, _)| joint_index as usize)
                    }
                };
                triangles_of_joint_index
                    .entry(joint_index)
                    .or_default()
                    .push([triangle[0], triangle[1], triangle[2]]);
            }

            // Each piece keeps only the vertices its own triangles use.
            triangles_of_joint_index
                .into_iter()
                .filter_map(|(joint_index, triangles)| {
                    let mut new_index_of_old_index = vec![u32::MAX; positions.len()];
                    let mut piece_positions = Vec::new();
                    let piece_triangles = triangles
                        .into_iter()
                        .map(|triangle| {
                            triangle.map(|old_index| {
                                if new_index_of_old_index[old_index as usize] == u32::MAX {
                                    new_index_of_old_index[old_index as usize] = piece_positions.len() as u32;
                                    piece_positions.push(positions[old_index as usize]);
                                }
                                new_index_of_old_index[old_index as usize]
                            })
                        })
                        .collect();
                    let triangles = TriMesh::new(piece_positions, piece_triangles).ok()?;
                    Some(MeshCameraPiece { joint_index, triangles })
                })
                .collect()
        }

        // The tree is in the piece's own space, and `model` places it in the world, so the ball is tested against
        // triangles in the world. That keeps the ball round however the piece is stretched.
        fn cast_against_triangles(triangles: &TriMesh, model: Mat4, origin: Vec3, translation: Vec3, max_toi: f32) -> Option<f32> {
            let ball = Ball::new(CAMERA_SIZE);
            let ball_pose = Pose::from_parts(origin, Quat::IDENTITY);
            let (_, toi) = triangles.bvh().find_best(
                max_toi,
                |node, _| camera_ball_toi_of_box(node.mins(), node.maxs(), model, origin, translation).unwrap_or(f32::MAX),
                |triangle_index, best_toi| {
                    let Triangle { a, b, c } = triangles.triangle(triangle_index);
                    let triangle = Triangle::new(model.transform_point3(a), model.transform_point3(b), model.transform_point3(c));
                    // A mesh doesn't collide, so the head can already touch its triangles. Those only stop the camera when it moves into them.
                    let options = ShapeCastOptions {
                        max_time_of_impact: best_toi,
                        stop_at_penetration: false,
                        ..ShapeCastOptions::default()
                    };
                    let hit = cast_shapes(&ball_pose, translation, &ball, &Pose::IDENTITY, Vec3::ZERO, &triangle, options).ok()??;
                    Some(hit.time_of_impact)
                },
            )?;
            Some(toi)
        }

        let mut closest_toi = 1.0;
        let mut skeleton_pose_of_id: HashMap<ObjectId, SkeletonPose> = HashMap::new();
        for &mesh_object_id in world._kindOfObjectId.get_keys(&ObjectKind::Mesh).into_iter().flatten() {
            let Some(mesh_object) = world.objectOfId.get(&mesh_object_id) else {
                continue;
            };
            let ObjectInfo::Mesh(mesh_info) = &mesh_object.info else {
                continue;
            };
            let Some(physical_object_id) = mesh_info.physicalObjectId.map(ObjectId::from) else {
                continue;
            };
            let Some(physics_info_3d) = world.getObjectPhysicsInfo3D(physical_object_id) else {
                continue;
            };
            let Location3D { position, quaternion } = physics_info_3d.location;
            let model = Mat4::from_rotation_translation(quaternion, position) * Mat4::from_scale(mesh_info.scale);

            // Skip meshes whose data hasn't downloaded, and meshes the camera doesn't come near.
            let shape = match self.shape_of_mesh_data_id.entry(mesh_info.meshDataId) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let Some(mesh_data_arc) = get_mesh_data(mesh_info.meshDataId) else {
                        continue;
                    };
                    let (positions, _) = positions_of_mesh_data(&mesh_data_arc);
                    let rest_min = positions.iter().fold(Vec3::INFINITY, |rest_min, &position| rest_min.min(position));
                    let rest_max = positions.iter().fold(Vec3::NEG_INFINITY, |rest_max, &position| rest_max.max(position));
                    entry.insert(MeshCameraShape {
                        mesh_data_arc,
                        rest_min,
                        rest_max,
                        pieces: None,
                    })
                }
            };
            let reach_scale = match shape.mesh_data_arc.as_ref() {
                MeshData::StaticMeshData { .. } => 1.0,
                MeshData::SkinnedMeshData { .. } => ANIMATED_MESH_REACH_SCALE,
            };
            let rest_center = (shape.rest_min + shape.rest_max) * 0.5;
            let reach_half_extents = (shape.rest_max - shape.rest_min) * 0.5 * reach_scale;
            let box_toi = camera_ball_toi_of_box(rest_center - reach_half_extents, rest_center + reach_half_extents, model, origin, translation);
            if box_toi.is_none_or(|box_toi| box_toi >= closest_toi) {
                continue;
            }

            // Players and walkthrough objects don't block the camera, so neither do the meshes that follow them.
            // A rigid group is solid when any of its parts is.
            let rigid_group_id = world.physics.getSubitemRigidGroupId(physical_object_id);
            if should_ignore_object(physical_object_id) || rigid_group_id.is_some_and(|rigid_group_id| should_ignore_object(rigid_group_id)) {
                continue;
            }
            let is_solid = match world.getObjectPhysicsToggles3D(physical_object_id) {
                Some(physics_toggles_3d) => !physics_toggles_3d.isWalkthrough,
                None => world.physics.getRigidGroupSubitemIds(physical_object_id).into_iter().any(|subitem_id| {
                    world
                        .getObjectPhysicsToggles3D(subitem_id)
                        .is_some_and(|physics_toggles_3d| !physics_toggles_3d.isWalkthrough)
                }),
            };
            if !is_solid {
                continue;
            }

            // An animated mesh is drawn where its skeleton's joints put it, so pose the skeleton at this tick, like the
            // physics that follows it.
            let joint_matrix_buffer_content = match shape.mesh_data_arc.as_ref() {
                MeshData::StaticMeshData { .. } => None,
                MeshData::SkinnedMeshData {
                    jointNameOfIdx,
                    inverseBindMatrixOfJointName,
                    ..
                } => {
                    let Some(skeleton_id) = mesh_info.followSkeletonId.map(ObjectId::from) else {
                        continue;
                    };
                    let Some(render_skeleton_info_arc) = world.getRenderSkeletonInfoArc(skeleton_id) else {
                        continue;
                    };
                    let pose = match skeleton_pose_of_id.entry(skeleton_id) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => {
                            let render_sk_animations = world.collectRenderSkAnimationsForSkeleton(skeleton_id);
                            let Ok(pose) = computeSkeletonPoseAtTime(
                                &render_skeleton_info_arc.jointOfIndex,
                                &render_sk_animations,
                                est_dely_server_sim_ms,
                                Some(live_client_sim_ms),
                            ) else {
                                continue;
                            };
                            entry.insert(pose)
                        }
                    };
                    Some(build_joint_matrix_buffer_content(
                        render_skeleton_info_arc,
                        pose,
                        jointNameOfIdx,
                        inverseBindMatrixOfJointName,
                    ))
                }
            };

            let pieces = shape.pieces.get_or_insert_with(|| build_pieces(&shape.mesh_data_arc));
            for MeshCameraPiece { joint_index, triangles } in pieces.iter() {
                let piece_model = match (joint_index, &joint_matrix_buffer_content) {
                    (Some(joint_index), Some(joint_matrix_buffer_content)) => {
                        let Some(skin_matrix) = joint_matrix_buffer_content.matrices.get(*joint_index) else {
                            continue;
                        };
                        model * Mat4::from_cols_array_2d(skin_matrix)
                    }
                    _ => model,
                };
                if let Some(toi) = cast_against_triangles(triangles, piece_model, origin, translation, closest_toi) {
                    closest_toi = toi;
                }
            }
        }
        (closest_toi < 1.0).then(|| closest_toi * translation.length())
    }
}
