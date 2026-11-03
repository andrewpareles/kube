use std::borrow::Cow;

use crate::detail_levels::{bounding_radius_around_origin, build_simpler_detail_levels};
use crate::render_scene::VertexContent;
use crate::renderer::GpuGeometryBuffer;
use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

const DUMMY_TANGENT_PLACEHOLDER: [f32; 4] = [f32::MAX, f32::MAX, f32::MAX, f32::MAX];
pub const BALL_NUM_SLICES: u32 = 24;
pub const CYLINDER_NUM_SLICES: u32 = 32;

pub struct PrimitiveGeometryData<'a> {
    pub vertices: Cow<'a, [VertexContent]>,
    pub indices: Cow<'a, [u16]>,
}

// -------------------------- Primitive mesh generation --------------------------

pub fn brick_geometry() -> PrimitiveGeometryData<'static> {
    // Bricks render from one shared unit cube and get their actual size from the instance transform.
    let half_extent = Vec3::splat(0.5);
    // let inner_extent = (half_extent - Vec3::splat(BRICK_FACE_BEVEL_DISTANCE)).max(Vec3::ZERO);
    let inner_extent = half_extent;
    let half_x = half_extent.x;
    let half_y = half_extent.y;
    let half_z = half_extent.z;
    let inner_x = inner_extent.x;
    let inner_y = inner_extent.y;
    let inner_z = inner_extent.z;
    let mut vertices: Vec<VertexContent> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();

    // This helper keeps the brick face setup readable while still building one shared brick mesh here.
    let push_quad = |vertices: &mut Vec<VertexContent>, indices: &mut Vec<u16>, positions: [Vec3; 4], normal: Vec3| {
        let mut ordered_vertices = [
            (positions[0], Vec2::new(0.0, 0.0)),
            (positions[1], Vec2::new(1.0, 0.0)),
            (positions[2], Vec2::new(1.0, 1.0)),
            (positions[3], Vec2::new(0.0, 1.0)),
        ];
        let triangle_normal = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
        if triangle_normal.dot(normal) < 0.0 {
            ordered_vertices = [ordered_vertices[0], ordered_vertices[3], ordered_vertices[2], ordered_vertices[1]];
        }

        let base_index = vertices.len() as u16;
        for (position, uv) in ordered_vertices {
            vertices.push(VertexContent {
                position: position.to_array(),
                normal: normal.to_array(),
                uv: uv.to_array(),
                tangent: DUMMY_TANGENT_PLACEHOLDER,
            });
        }
        indices.extend_from_slice(&[base_index, base_index + 1, base_index + 2, base_index, base_index + 2, base_index + 3]);
    };

    /*
    let push_triangle = |vertices: &mut Vec<VertexContent>, indices: &mut Vec<u16>, positions: [Vec3; 3], normal: Vec3| {
        let mut ordered_vertices = [
            (positions[0], Vec2::new(0.0, 0.0)),
            (positions[1], Vec2::new(1.0, 0.0)),
            (positions[2], Vec2::new(0.0, 1.0)),
        ];
        let triangle_normal = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
        if triangle_normal.dot(normal) < 0.0 {
            ordered_vertices = [ordered_vertices[0], ordered_vertices[2], ordered_vertices[1]];
        }

        let base_index = vertices.len() as u16;
        for (position, uv) in ordered_vertices {
            vertices.push(VertexContent {
                position: position.to_array(),
                normal: normal.to_array(),
                uv: uv.to_array(),
                tangent: DUMMY_TANGENT_PLACEHOLDER,
            });
        }
        indices.extend_from_slice(&[base_index, base_index + 1, base_index + 2]);
    };
    */

    // These six quads stay on the original brick planes and become the flat center of each face.
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(-inner_x, -inner_y, half_z),
            Vec3::new(inner_x, -inner_y, half_z),
            Vec3::new(inner_x, inner_y, half_z),
            Vec3::new(-inner_x, inner_y, half_z),
        ],
        Vec3::Z,
    );
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(inner_x, -inner_y, -half_z),
            Vec3::new(-inner_x, -inner_y, -half_z),
            Vec3::new(-inner_x, inner_y, -half_z),
            Vec3::new(inner_x, inner_y, -half_z),
        ],
        -Vec3::Z,
    );
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(half_x, -inner_y, inner_z),
            Vec3::new(half_x, -inner_y, -inner_z),
            Vec3::new(half_x, inner_y, -inner_z),
            Vec3::new(half_x, inner_y, inner_z),
        ],
        Vec3::X,
    );
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(-half_x, -inner_y, -inner_z),
            Vec3::new(-half_x, -inner_y, inner_z),
            Vec3::new(-half_x, inner_y, inner_z),
            Vec3::new(-half_x, inner_y, -inner_z),
        ],
        -Vec3::X,
    );
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(-inner_x, half_y, inner_z),
            Vec3::new(inner_x, half_y, inner_z),
            Vec3::new(inner_x, half_y, -inner_z),
            Vec3::new(-inner_x, half_y, -inner_z),
        ],
        Vec3::Y,
    );
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(-inner_x, -half_y, -inner_z),
            Vec3::new(inner_x, -half_y, -inner_z),
            Vec3::new(inner_x, -half_y, inner_z),
            Vec3::new(-inner_x, -half_y, inner_z),
        ],
        -Vec3::Y,
    );

    /*
    // These edge strips connect adjacent faces and create the beveled edge band.
    for sign_x in [-1.0, 1.0] {
        for sign_y in [-1.0, 1.0] {
            push_quad(
                &mut vertices,
                &mut indices,
                [
                    Vec3::new(sign_x * half_x, sign_y * inner_y, -inner_z),
                    Vec3::new(sign_x * half_x, sign_y * inner_y, inner_z),
                    Vec3::new(sign_x * inner_x, sign_y * half_y, inner_z),
                    Vec3::new(sign_x * inner_x, sign_y * half_y, -inner_z),
                ],
                Vec3::new(sign_x, sign_y, 0.0).normalize(),
            );
        }
    }
    for sign_x in [-1.0, 1.0] {
        for sign_z in [-1.0, 1.0] {
            push_quad(
                &mut vertices,
                &mut indices,
                [
                    Vec3::new(sign_x * half_x, -inner_y, sign_z * inner_z),
                    Vec3::new(sign_x * half_x, inner_y, sign_z * inner_z),
                    Vec3::new(sign_x * inner_x, inner_y, sign_z * half_z),
                    Vec3::new(sign_x * inner_x, -inner_y, sign_z * half_z),
                ],
                Vec3::new(sign_x, 0.0, sign_z).normalize(),
            );
        }
    }
    for sign_y in [-1.0, 1.0] {
        for sign_z in [-1.0, 1.0] {
            push_quad(
                &mut vertices,
                &mut indices,
                [
                    Vec3::new(-inner_x, sign_y * half_y, sign_z * inner_z),
                    Vec3::new(inner_x, sign_y * half_y, sign_z * inner_z),
                    Vec3::new(inner_x, sign_y * inner_y, sign_z * half_z),
                    Vec3::new(-inner_x, sign_y * inner_y, sign_z * half_z),
                ],
                Vec3::new(0.0, sign_y, sign_z).normalize(),
            );
        }
    }

    // These corner triangles close the bevel and keep the box outline intact.
    for sign_x in [-1.0, 1.0] {
        for sign_y in [-1.0, 1.0] {
            for sign_z in [-1.0, 1.0] {
                push_triangle(
                    &mut vertices,
                    &mut indices,
                    [
                        Vec3::new(sign_x * half_x, sign_y * inner_y, sign_z * inner_z),
                        Vec3::new(sign_x * inner_x, sign_y * half_y, sign_z * inner_z),
                        Vec3::new(sign_x * inner_x, sign_y * inner_y, sign_z * half_z),
                    ],
                    Vec3::new(sign_x, sign_y, sign_z).normalize(),
                );
            }
        }
    }
    */

    compute_tangents(&mut vertices, &indices);

    PrimitiveGeometryData {
        vertices: Cow::Owned(vertices),
        indices: Cow::Owned(indices),
    }
}

pub fn cylinder_geometry() -> PrimitiveGeometryData<'static> {
    let slices = CYLINDER_NUM_SLICES;
    let mut vertices: Vec<VertexContent> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();

    for slice in 0..=slices {
        let u = slice as f32 / slices as f32;
        let angle = u * std::f32::consts::TAU;
        let (sin_angle, cos_angle) = angle.sin_cos();
        let radial = Vec3::new(cos_angle, sin_angle, 0.0);
        for (z, v) in [(-0.5, 0.0), (0.5, 1.0)] {
            vertices.push(VertexContent {
                position: [radial.x, radial.y, z],
                normal: radial.to_array(),
                uv: [u, v],
                tangent: DUMMY_TANGENT_PLACEHOLDER,
            });
        }
    }

    for slice in 0..slices {
        let current = (slice * 2) as u16;
        let next = current + 2;
        indices.extend_from_slice(&[current, next, current + 1, current + 1, next, next + 1]);
    }

    let top_center_index = vertices.len() as u16;
    vertices.push(VertexContent {
        position: [0.0, 0.0, 0.5],
        normal: Vec3::Z.to_array(),
        uv: [0.5, 0.5],
        tangent: DUMMY_TANGENT_PLACEHOLDER,
    });
    let top_ring_start = vertices.len() as u16;
    for slice in 0..slices {
        let u = slice as f32 / slices as f32;
        let angle = u * std::f32::consts::TAU;
        let (sin_angle, cos_angle) = angle.sin_cos();
        vertices.push(VertexContent {
            position: [cos_angle, sin_angle, 0.5],
            normal: Vec3::Z.to_array(),
            uv: [cos_angle * 0.5 + 0.5, sin_angle * 0.5 + 0.5],
            tangent: DUMMY_TANGENT_PLACEHOLDER,
        });
    }
    for slice in 0..slices {
        let current = top_ring_start + slice as u16;
        let next = top_ring_start + ((slice + 1) % slices) as u16;
        indices.extend_from_slice(&[top_center_index, current, next]);
    }

    let bottom_center_index = vertices.len() as u16;
    vertices.push(VertexContent {
        position: [0.0, 0.0, -0.5],
        normal: (-Vec3::Z).to_array(),
        uv: [0.5, 0.5],
        tangent: DUMMY_TANGENT_PLACEHOLDER,
    });
    let bottom_ring_start = vertices.len() as u16;
    for slice in 0..slices {
        let u = slice as f32 / slices as f32;
        let angle = u * std::f32::consts::TAU;
        let (sin_angle, cos_angle) = angle.sin_cos();
        vertices.push(VertexContent {
            position: [cos_angle, sin_angle, -0.5],
            normal: (-Vec3::Z).to_array(),
            uv: [cos_angle * 0.5 + 0.5, sin_angle * 0.5 + 0.5],
            tangent: DUMMY_TANGENT_PLACEHOLDER,
        });
    }
    for slice in 0..slices {
        let current = bottom_ring_start + slice as u16;
        let next = bottom_ring_start + ((slice + 1) % slices) as u16;
        indices.extend_from_slice(&[bottom_center_index, next, current]);
    }

    compute_tangents(&mut vertices, &indices);

    PrimitiveGeometryData {
        vertices: Cow::Owned(vertices),
        indices: Cow::Owned(indices),
    }
}

// -------------------------- Sphere mesh generation --------------------------

pub fn ball_geometry() -> PrimitiveGeometryData<'static> {
    let stacks = BALL_NUM_SLICES / 2; // make this half of # slices to get roughly square faces at the equator
    let slices = BALL_NUM_SLICES;
    let mut vertices: Vec<VertexContent> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let ring = slices + 1;

    for stack in 0..=stacks {
        let v = stack as f32 / stacks as f32;
        let theta = v * std::f32::consts::PI;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();

        for slice in 0..=slices {
            let u = slice as f32 / slices as f32;
            let phi = u * std::f32::consts::TAU;
            let sin_phi = phi.sin();
            let cos_phi = phi.cos();

            let normal = [sin_theta * cos_phi, cos_theta, sin_theta * sin_phi];
            vertices.push(VertexContent {
                position: normal,
                normal,
                uv: [u, v],
                // Tangent is a placeholder and is overwritten by compute_tangents below.
                tangent: DUMMY_TANGENT_PLACEHOLDER,
            });
        }
    }

    for stack in 0..stacks {
        for slice in 0..slices {
            let first = stack * ring + slice;
            let second = first + ring;
            indices.extend_from_slice(&[
                first as u16,
                (first + 1) as u16,
                second as u16,
                (first + 1) as u16,
                (second + 1) as u16,
                second as u16,
            ]);
        }
    }

    compute_tangents(&mut vertices, &indices);

    PrimitiveGeometryData {
        vertices: Cow::Owned(vertices),
        indices: Cow::Owned(indices),
    }
}

// -------------------------- Tangent generation --------------------------

// Every label image draws on this one quad, sized by its instance. It faces +Z, and v = 0 is the top of the picture.
pub fn label_image_geometry() -> PrimitiveGeometryData<'static> {
    let vertices = [(-0.5, -0.5, 0.0, 1.0), (0.5, -0.5, 1.0, 1.0), (0.5, 0.5, 1.0, 0.0), (-0.5, 0.5, 0.0, 0.0)].map(|(x, y, u, v)| VertexContent {
        position: [x, y, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [u, v],
        tangent: [1.0, 0.0, 0.0, 1.0],
    });
    PrimitiveGeometryData {
        vertices: Cow::Owned(vertices.to_vec()),
        indices: Cow::Owned(vec![0, 1, 2, 0, 2, 3]),
    }
}

fn compute_tangents(vertices: &mut [VertexContent], indices: &[u16]) {
    let mut tangent_sum_of_vertex_index: Vec<Vec3> = vec![Vec3::ZERO; vertices.len()];
    let mut bitangent_sum_of_vertex_index: Vec<Vec3> = vec![Vec3::ZERO; vertices.len()];

    // Accumulate triangle tangents and bitangents from positions and UV gradients.
    for triangle_indices in indices.chunks_exact(3) {
        let i0 = triangle_indices[0] as usize;
        let i1 = triangle_indices[1] as usize;
        let i2 = triangle_indices[2] as usize;

        let p0 = Vec3::from_array(vertices[i0].position);
        let p1 = Vec3::from_array(vertices[i1].position);
        let p2 = Vec3::from_array(vertices[i2].position);

        let uv0 = Vec2::from_array(vertices[i0].uv);
        let uv1 = Vec2::from_array(vertices[i1].uv);
        let uv2 = Vec2::from_array(vertices[i2].uv);

        let edge1 = p1 - p0;
        let edge2 = p2 - p0;
        let delta_uv1 = uv1 - uv0;
        let delta_uv2 = uv2 - uv0;

        let determinant = delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x;
        if determinant.abs() <= 1e-8 {
            continue;
        }
        let inverse_determinant = 1.0 / determinant;
        let triangle_tangent = (edge1 * delta_uv2.y - edge2 * delta_uv1.y) * inverse_determinant;
        let triangle_bitangent = (edge2 * delta_uv1.x - edge1 * delta_uv2.x) * inverse_determinant;

        for vertex_index in [i0, i1, i2] {
            tangent_sum_of_vertex_index[vertex_index] += triangle_tangent;
            bitangent_sum_of_vertex_index[vertex_index] += triangle_bitangent;
        }
    }

    // Orthonormalize against normal, then derive handedness for tangent.w.
    for (vertex_index, vertex) in vertices.iter_mut().enumerate() {
        let normal = Vec3::from_array(vertex.normal).normalize_or_zero();
        let tangent_sum = tangent_sum_of_vertex_index[vertex_index];

        let mut tangent = tangent_sum - normal * tangent_sum.dot(normal);
        if tangent.length_squared() <= 1e-12 {
            let fallback_axis = if normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            tangent = fallback_axis.cross(normal);
        }
        if tangent.length_squared() <= 1e-12 {
            tangent = Vec3::X;
        }
        tangent = tangent.normalize();

        let bitangent_sum = bitangent_sum_of_vertex_index[vertex_index];
        let handedness = if normal.cross(tangent).dot(bitangent_sum) < 0.0 { -1.0 } else { 1.0 };
        vertex.tangent = [tangent.x, tangent.y, tangent.z, handedness];
    }
}

// -------------------------- GPU buffer upload --------------------------

pub fn create_primitive_buffers(device: &wgpu::Device, geometry: &PrimitiveGeometryData) -> GpuGeometryBuffer {
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Primitive VB"),
        contents: bytemuck::cast_slice(geometry.vertices.as_ref()),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Primitive IB"),
        contents: bytemuck::cast_slice(geometry.indices.as_ref()),
        usage: wgpu::BufferUsages::INDEX,
    });
    GpuGeometryBuffer {
        vertex_buffer,
        index_buffer,
        num_indices: geometry.indices.len() as u32,
        index_format: wgpu::IndexFormat::Uint16,
        bounding_radius: bounding_radius_around_origin(geometry.vertices.iter().map(|vertex| vertex.position)),
        simpler_detail_levels: Vec::new(),
    }
}

// Balls are the one primitive drawn with simpler copies when they look small (see detail_levels.rs).
pub fn create_ball_buffers(device: &wgpu::Device) -> GpuGeometryBuffer {
    let geometry = ball_geometry();
    let positions: Vec<[f32; 3]> = geometry.vertices.iter().map(|vertex| vertex.position).collect();
    let uvs: Vec<[f32; 2]> = geometry.vertices.iter().map(|vertex| vertex.uv).collect();
    let indices: Vec<u32> = geometry.indices.iter().map(|&index| u32::from(index)).collect();
    let (indices_with_simpler_detail_levels, simpler_detail_levels) = build_simpler_detail_levels(&positions, &uvs, &indices);
    let geometry_with_simpler_detail_levels = PrimitiveGeometryData {
        vertices: geometry.vertices,
        indices: indices_with_simpler_detail_levels.into_iter().map(|index| index as u16).collect(),
    };
    GpuGeometryBuffer {
        num_indices: indices.len() as u32,
        simpler_detail_levels,
        ..create_primitive_buffers(device, &geometry_with_simpler_detail_levels)
    }
}
