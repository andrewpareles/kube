//! Editor picking intersects the displayed triangles without creating a physics world.
//! Queries run only on clicks. Bounds reject misses before triangle tests, and repeated mesh instances
//! share their decoded/posed vertices for the duration of the query. Material texture alpha is not sampled.
use crate::{
    camera::build_ray_from_view_proj_ndc,
    create_gpu_primitive_shape::{PrimitiveGeometryData, ball_geometry, brick_geometry, cylinder_geometry, label_image_geometry},
    render_scene::{LabelContent, RenderGeometry, RenderScene, instance_model_for_view},
};
use game_types::{
    data_types::{DataId, MeshData},
    object_types::ObjectId,
};
use glam::{Mat4, Vec3};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, LazyLock},
};

struct PickingGeometry<'a> {
    positions: Vec<Vec3>,
    indices: Cow<'a, [u32]>,
    min: Vec3,
    max: Vec3,
}

impl<'a> PickingGeometry<'a> {
    fn new(positions: Vec<Vec3>, indices: Cow<'a, [u32]>) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &position in &positions {
            min = min.min(position);
            max = max.max(position);
        }
        Self { positions, indices, min, max }
    }

    fn from_primitive(geometry: PrimitiveGeometryData<'_>) -> Self {
        Self::new(
            geometry.vertices.iter().map(|vertex| Vec3::from_array(vertex.position)).collect(),
            Cow::Owned(geometry.indices.iter().map(|&index| u32::from(index)).collect()),
        )
    }

    // Transform the ray instead of every triangle. Keeping its length preserves world-space hit distances,
    // even under nonuniform scale. Mirrored instances reverse the front-face test just as the GPU does.
    fn intersect(&self, model: Mat4, origin: Vec3, direction: Vec3, is_double_sided: bool) -> Option<f64> {
        let determinant = model.determinant();
        if !determinant.is_finite() {
            return None;
        }
        // A zero thickness brick can still have visible faces. Its transform cannot invert, so test
        // those flattened triangles in world space; triangles collapsed to lines will simply miss.
        if determinant == 0.0 {
            let positions = self.positions.iter().map(|&position| model.transform_point3(position)).collect();
            let flattened = PickingGeometry::new(positions, Cow::Borrowed(&self.indices));
            return flattened.intersect(Mat4::IDENTITY, origin, direction, is_double_sided);
        }
        let inverse = model.inverse();
        let origin = inverse.transform_point3(origin).as_dvec3();
        let direction = inverse.transform_vector3(direction).as_dvec3();
        if !origin.is_finite() || !direction.is_finite() || self.positions.is_empty() {
            return None;
        }
        let mut near = 0.0_f64;
        let mut far = f64::INFINITY;
        for axis in 0..3 {
            let min = f64::from(self.min[axis]);
            let max = f64::from(self.max[axis]);
            if direction[axis] == 0.0 {
                if origin[axis] < min || origin[axis] > max {
                    return None;
                }
                continue;
            }
            let a = (min - origin[axis]) / direction[axis];
            let b = (max - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if far < near {
                return None;
            }
        }
        let mut closest = None;
        for triangle in self.indices.chunks_exact(3) {
            let [Some(a), Some(b), Some(c)] = [
                self.positions.get(triangle[0] as usize),
                self.positions.get(triangle[1] as usize),
                self.positions.get(triangle[2] as usize),
            ] else {
                continue;
            };
            let a = a.as_dvec3();
            let edge1 = b.as_dvec3() - a;
            let edge2 = c.as_dvec3() - a;
            let cross = direction.cross(edge2);
            let det = edge1.dot(cross);
            if det == 0.0 || (!is_double_sided && det * f64::from(determinant.signum()) <= 0.0) {
                continue;
            }
            let offset = origin - a;
            let u = offset.dot(cross) / det;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let cross = offset.cross(edge1);
            let v = direction.dot(cross) / det;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let distance = edge2.dot(cross) / det;
            if distance.is_finite() && distance >= 0.0 && closest.is_none_or(|previous| distance < previous) {
                closest = Some(distance);
            }
        }
        closest
    }
}

static BALL: LazyLock<PickingGeometry<'static>> = LazyLock::new(|| PickingGeometry::from_primitive(ball_geometry()));
static BRICK: LazyLock<PickingGeometry<'static>> = LazyLock::new(|| PickingGeometry::from_primitive(brick_geometry()));
static LABEL_IMAGE: LazyLock<PickingGeometry<'static>> = LazyLock::new(|| PickingGeometry::from_primitive(label_image_geometry()));
static CYLINDER: LazyLock<PickingGeometry<'static>> = LazyLock::new(|| PickingGeometry::from_primitive(cylinder_geometry()));

// The scene and matrix must be from the same displayed frame. Always-on-top geometry wins over the
// normal layer; otherwise the nearest front-facing triangle wins, with stable IDs breaking exact ties.
pub fn pick_scene_object(
    scene: &RenderScene,
    mesh_data_arc_of_id: &HashMap<DataId, Arc<MeshData>>,
    view_proj: Mat4,
    ndc_x: f32,
    ndc_y: f32,
) -> Option<ObjectId> {
    if !(-1.0..=1.0).contains(&ndc_x) || !(-1.0..=1.0).contains(&ndc_y) {
        return None;
    }
    let (origin, direction) = build_ray_from_view_proj_ndc(view_proj, ndc_x, ndc_y)?;
    let mut closest: Option<(bool, f64, ObjectId)> = None;
    let mut consider = |distance: Option<f64>, is_always_on_top: bool, object_id: ObjectId| {
        let Some(distance) = distance else {
            return;
        };
        if closest.is_none_or(|(previous_top, previous_distance, previous_id)| {
            is_always_on_top && !previous_top
                || is_always_on_top == previous_top && (distance < previous_distance || distance == previous_distance && object_id < previous_id)
        }) {
            closest = Some((is_always_on_top, distance, object_id));
        }
    };
    let mut geometry_of_mesh_and_skeleton = HashMap::new();
    for instance in &scene.instances {
        if instance.content.color[3] <= 0.0 {
            continue;
        }
        let geometry = match instance.geometry {
            RenderGeometry::Ball => &*BALL,
            RenderGeometry::Brick => &*BRICK,
            RenderGeometry::Cylinder => &*CYLINDER,
            RenderGeometry::Mesh { data_id, skeleton_id } => {
                let Some(mesh) = mesh_data_arc_of_id.get(&data_id) else {
                    continue;
                };
                geometry_of_mesh_and_skeleton.entry((data_id, skeleton_id)).or_insert_with(|| {
                    let (positions, indices) = match mesh.as_ref() {
                        MeshData::StaticMeshData { vertices, indices } => (vertices.iter().map(|vertex| vertex.position).collect(), indices),
                        MeshData::SkinnedMeshData { vertices, indices, .. } => {
                            let joints = scene
                                .joint_matrices
                                .iter()
                                .find(|(mesh_id, id, _)| *mesh_id == data_id && Some(*id) == skeleton_id);
                            let positions = vertices
                                .iter()
                                .map(|vertex| {
                                    let Some((_, _, joints)) = joints else {
                                        return vertex.position;
                                    };
                                    let mut position = Vec3::ZERO;
                                    let mut used_weight = 0.0;
                                    for (&index, &weight) in vertex.skinning.jointIndices.iter().zip(&vertex.skinning.jointWeights) {
                                        if weight <= 0.0 {
                                            continue;
                                        }
                                        let Some(matrix) = joints.matrices.get(index as usize) else {
                                            continue;
                                        };
                                        position += Mat4::from_cols_array_2d(matrix).transform_point3(vertex.position) * weight;
                                        used_weight += weight;
                                    }
                                    // Match scene.wgsl get_skin_matrix, including its usable-weight threshold.
                                    if used_weight < 0.001 { vertex.position } else { position / used_weight }
                                })
                                .collect();
                            (positions, indices)
                        }
                    };
                    PickingGeometry::new(positions, Cow::Borrowed(indices))
                })
            }
        };
        let model = instance_model_for_view(&instance.content, view_proj);
        consider(
            geometry.intersect(model, origin, direction, instance.material.is_double_sided),
            instance.is_always_on_top,
            instance.object_id,
        );
    }
    // Labels use the same billboard and constant-size transform as their shader. Their panel and text
    // have separate visibility, so a hidden background must not intercept a click between glyphs.
    for label in &scene.labels {
        let (label_geometry, instances, draws_background) = match &label.content {
            LabelContent::Text {
                geometry,
                instances,
                draws_background,
                ..
            } => (geometry, instances, *draws_background),
            LabelContent::Image { instance, .. } => {
                if instance.color[3] > 0.0 {
                    consider(
                        LABEL_IMAGE.intersect(instance_model_for_view(instance, view_proj), origin, direction, true),
                        label.is_always_on_top,
                        label.object_id,
                    );
                }
                continue;
            }
        };
        let background_end = label_geometry.background_index_count as usize;
        for (part, content) in instances.iter().enumerate() {
            if content.color[3] <= 0.0 || part == 0 && !draws_background {
                continue;
            }
            let indices = if part == 0 {
                label_geometry.indices.get(..background_end)
            } else {
                label_geometry.indices.get(background_end..)
            };
            let Some(indices) = indices else {
                continue;
            };
            let geometry = PickingGeometry::new(
                label_geometry.vertices.iter().map(|vertex| Vec3::from_array(vertex.position)).collect(),
                Cow::Borrowed(indices),
            );
            consider(
                geometry.intersect(instance_model_for_view(content, view_proj), origin, direction, true),
                label.is_always_on_top,
                label.object_id,
            );
        }
    }
    closest.map(|(_, _, object_id)| object_id)
}
