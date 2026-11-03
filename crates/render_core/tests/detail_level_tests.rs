#[path = "helpers/render_capture.rs"]
mod render_capture;

// The GPU test requires a graphics adapter. Run it explicitly with:
// cargo test -p render_core --test detail_level_tests -- --ignored

use game_types::{
    data_types::{MeshData, MeshVertexInternal},
    object_types::{PhysicsInfo3D, VisualSettings, shared::Color3},
    physics_primitive_types::BallPrimitiveSettings,
};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    create_gpu_primitive_shape::create_ball_buffers,
    detail_levels::{DetailLevel, build_simpler_detail_levels, detail_level_for_instance},
    render_scene::build_render_scene,
    renderer::Renderer,
    renderer_types::{RenderBall, RenderMesh, RenderObject, RenderTransform},
};
use std::{collections::HashMap, sync::Arc};

// A unit sphere of rings and slices, with its texture wrapping around once, so the first and last slice meet at a seam.
fn sphere(slices: u32, rings: u32) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    for ring in 0..=rings {
        let v = ring as f32 / rings as f32;
        for slice in 0..=slices {
            let u = slice as f32 / slices as f32;
            let (sin_theta, cos_theta) = (v * std::f32::consts::PI).sin_cos();
            let (sin_phi, cos_phi) = (u * std::f32::consts::TAU).sin_cos();
            positions.push([sin_theta * cos_phi, cos_theta, sin_theta * sin_phi]);
            uvs.push([u, v]);
        }
    }
    let mut indices = Vec::new();
    for ring in 0..rings {
        for slice in 0..slices {
            let first = ring * (slices + 1) + slice;
            let second = first + slices + 1;
            indices.extend_from_slice(&[first, first + 1, second, first + 1, second + 1, second]);
        }
    }
    (positions, uvs, indices)
}

#[test]
fn simpler_copies_follow_the_full_triangles_with_fewer_triangles_and_more_error() {
    let (positions, uvs, indices) = sphere(64, 32);
    let (indices_with_simpler_detail_levels, simpler_detail_levels) = build_simpler_detail_levels(&positions, &uvs, &indices);
    assert_eq!(
        &indices_with_simpler_detail_levels[..indices.len()],
        &indices[..],
        "the full triangles must come first, unchanged"
    );
    assert!(!simpler_detail_levels.is_empty(), "a dense sphere should get simpler copies");
    let mut previous_end = indices.len() as u32;
    let mut previous_triangle_count = indices.len() / 3;
    let mut previous_error = 0.0;
    for detail_level in &simpler_detail_levels {
        assert_eq!(detail_level.index_range.start, previous_end, "each copy starts where the one before it ends");
        let copy = &indices_with_simpler_detail_levels[detail_level.index_range.start as usize..detail_level.index_range.end as usize];
        assert!(!copy.is_empty() && copy.len() % 3 == 0);
        assert!(
            copy.len() / 3 <= previous_triangle_count / 2,
            "each copy keeps at most half the triangles of the one before it"
        );
        assert!(
            copy.iter().all(|&index| (index as usize) < positions.len()),
            "a copy only reuses the full geometry's vertices"
        );
        assert!(detail_level.error > 0.0 && detail_level.error >= previous_error);
        previous_end = detail_level.index_range.end;
        previous_triangle_count = copy.len() / 3;
        previous_error = detail_level.error;
    }
    assert_eq!(previous_end as usize, indices_with_simpler_detail_levels.len());
}

// Every triangle of a flat shaded mesh has its own three vertices, so every edge sits on a split between vertices. Those splits
// only differ in normals, which a simpler copy may merge across.
#[test]
fn flat_shaded_meshes_get_simpler_copies() {
    let (positions, uvs, indices) = sphere(64, 32);
    let flat_positions: Vec<[f32; 3]> = indices.iter().map(|&index| positions[index as usize]).collect();
    let flat_uvs: Vec<[f32; 2]> = indices.iter().map(|&index| uvs[index as usize]).collect();
    let flat_indices: Vec<u32> = (0..indices.len() as u32).collect();
    let (_, simpler_detail_levels) = build_simpler_detail_levels(&flat_positions, &flat_uvs, &flat_indices);
    assert!(!simpler_detail_levels.is_empty(), "a flat shaded sphere should get simpler copies");
}

// A cube pushed out onto a sphere, where every face is its own grid of vertices showing its own sixth of the texture, like a
// model whose texture is laid out in separate pieces. A triangle that joins two faces would stretch the texture across the
// gap between their pieces.
#[test]
fn simpler_copies_never_join_triangles_across_a_texture_seam() {
    let cells = 8;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut face_of_vertex = Vec::new();
    let mut indices = Vec::new();
    let axes_of_face = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::Y, Vec3::X),
    ];
    for (face, (normal, right, up)) in axes_of_face.into_iter().enumerate() {
        let first_vertex = positions.len() as u32;
        for row in 0..=cells {
            for column in 0..=cells {
                let (across, along) = (column as f32 / cells as f32, row as f32 / cells as f32);
                positions.push((normal + right * (across * 2.0 - 1.0) + up * (along * 2.0 - 1.0)).normalize().to_array());
                uvs.push([(face as f32 + across) / 6.0, along]);
                face_of_vertex.push(face);
            }
        }
        for row in 0..cells {
            for column in 0..cells {
                let first = first_vertex + row * (cells + 1) + column;
                let second = first + cells + 1;
                indices.extend_from_slice(&[first, first + 1, second, first + 1, second + 1, second]);
            }
        }
    }
    let (indices_with_simpler_detail_levels, simpler_detail_levels) = build_simpler_detail_levels(&positions, &uvs, &indices);
    assert!(!simpler_detail_levels.is_empty(), "a cube sphere should get simpler copies");
    for detail_level in &simpler_detail_levels {
        let copy = &indices_with_simpler_detail_levels[detail_level.index_range.start as usize..detail_level.index_range.end as usize];
        for triangle in copy.chunks_exact(3) {
            let face = face_of_vertex[triangle[0] as usize];
            assert!(
                triangle.iter().all(|&index| face_of_vertex[index as usize] == face),
                "a simpler copy joined two pieces of the texture: {triangle:?}"
            );
        }
    }
}

// The rule doesn't depend on how many pixels of error are allowed: far enough away the simplest copy is drawn, up close or
// from inside the full geometry is, and a bigger instance never gets a simpler copy than a smaller one in the same spot.
#[test]
fn instances_draw_simpler_copies_only_when_they_look_small() {
    let simpler_detail_levels = vec![
        DetailLevel {
            index_range: 3..6,
            error: 0.01,
        },
        DetailLevel { index_range: 6..9, error: 0.1 },
    ];
    let bounding_radius = 1.0;
    let pixels_per_unit_at_unit_distance = 1000.0;
    let detail_level_at = |distance: f32, scale: f32| {
        let model = Mat4::from_scale_rotation_translation(Vec3::splat(scale), Quat::IDENTITY, Vec3::new(0.0, 0.0, -distance));
        detail_level_for_instance(
            &simpler_detail_levels,
            bounding_radius,
            &model.to_cols_array_2d(),
            Vec3::ZERO,
            pixels_per_unit_at_unit_distance,
        )
    };
    assert_eq!(detail_level_at(2.0, 1.0), 0, "close up draws the full geometry");
    assert_eq!(detail_level_at(0.5, 1.0), 0, "from inside the bounding sphere draws the full geometry");
    assert_eq!(detail_level_at(1e6, 1.0), 2, "far away draws the simplest copy");
    let distances = [2.0, 10.0, 1e2, 1e3, 1e4, 1e5, 1e6];
    assert!(
        distances.windows(2).all(|pair| detail_level_at(pair[0], 1.0) <= detail_level_at(pair[1], 1.0)),
        "moving away never adds detail"
    );
    assert!(
        detail_level_at(1e6, 1e5) < detail_level_at(1e6, 1.0),
        "a huge instance far away still looks big, so it keeps more detail"
    );
}

// Far away balls and meshes draw a simpler copy, and it must still cover the shape it stands for: the right number of pixels,
// all of them around where the object is.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn faraway_balls_and_meshes_still_draw_their_whole_shape() -> anyhow::Result<()> {
    const SIZE: u32 = 512;
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let ball_buffer = create_ball_buffers(&device);
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)?;

    let (positions, uvs, indices) = sphere(64, 32);
    let (_, mesh_simpler_detail_levels) = build_simpler_detail_levels(&positions, &uvs, &indices);
    let mesh = MeshData::StaticMeshData {
        vertices: positions
            .iter()
            .zip(&uvs)
            .map(|(position, uv)| MeshVertexInternal {
                position: Vec3::from(*position),
                normal: Vec3::from(*position),
                uv: Vec2::from(*uv),
                tangent: Vec4::X,
                skinning: (),
            })
            .collect(),
        indices,
    };
    renderer.load_mesh_into_gpu(1, &mesh);

    // Each shape goes just far enough away that it draws its first simpler copy.
    let view_proj = build_camera_view_proj_matrix(Vec3::ZERO, Vec3::NEG_Z, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let pixels_per_unit_at_unit_distance = 0.5 * SIZE as f32 * view_proj.row(1).truncate().length();
    let distance_for_first_copy = |simpler_detail_levels: &[DetailLevel]| {
        let mut distance = 2.0;
        while detail_level_for_instance(
            simpler_detail_levels,
            1.0,
            &Mat4::from_translation(Vec3::new(0.0, 0.0, -distance)).to_cols_array_2d(),
            Vec3::ZERO,
            pixels_per_unit_at_unit_distance,
        ) == 0
        {
            distance *= 1.05;
        }
        distance
    };
    let ball_distance = distance_for_first_copy(&ball_buffer.simpler_detail_levels);
    let mesh_distance = distance_for_first_copy(&mesh_simpler_detail_levels);
    let transform = |position: Vec3| RenderTransform {
        physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        teleport_count: 0,
        rigid_group_to_follow: None,
        sk_joint_to_follow_arc: None,
    };
    let white = || VisualSettings {
        color: Color3::WHITE,
        ..VisualSettings::TYPICAL_OPAQUE
    };
    // Each shape sits a quarter of the picture off to its side, wherever its distance puts it.
    let ball_position = Vec3::new(-0.25 * ball_distance, 0.0, -ball_distance);
    let mesh_position = Vec3::new(0.25 * mesh_distance, 0.0, -mesh_distance);
    let objects = HashMap::from([
        (
            1,
            RenderObject::Ball(RenderBall {
                transform: transform(ball_position),
                visualSettings: white(),
                material: None,
                primitive: BallPrimitiveSettings { radius: 1.0 },
            }),
        ),
        (2, RenderObject::RigidGroup(transform(mesh_position))),
        (
            3,
            RenderObject::Mesh(RenderMesh {
                physicalObjectId: 2,
                visualSettings: white(),
                material: None,
                meshDataId: 1,
                scale: Vec3::ONE,
                skin_skeleton_id: None,
            }),
        ),
    ]);
    let mut scene = build_render_scene(&objects, &HashMap::from([(1, Arc::new(mesh))]), None);
    scene.should_draw_sky = false;
    scene.lighting.sun_color_and_intensity = [0.0; 4];
    scene.lighting.ambient_top_color_and_intensity = [1.0, 1.0, 1.0, 1.0];
    scene.lighting.ambient_bottom_color_and_padding = [1.0, 1.0, 1.0, 0.0];

    let (_, camera) = renderer.create_camera_matrix_buffer_pair(Vec3::ZERO, Vec3::NEG_Z, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, &scene, None, &camera, Vec3::NEG_Z, Vec3::ZERO, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Detail level readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    frame.encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        texture.size(),
    );
    renderer.finish_frame(frame);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).unwrap();
    });
    renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
    receiver.await??;
    let pixels = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();

    // With no sky the picture is see-through, so its alpha is how much of each pixel the shapes cover.
    for (name, position, distance) in [("ball", ball_position, ball_distance), ("mesh", mesh_position, mesh_distance)] {
        let ndc = view_proj.project_point3(position);
        let center = Vec2::new((ndc.x * 0.5 + 0.5) * SIZE as f32, (0.5 - ndc.y * 0.5) * SIZE as f32);
        let radius_in_pixels = pixels_per_unit_at_unit_distance / distance;
        let mut covered_near = 0.0;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let coverage = pixels[((y * SIZE + x) * 4 + 3) as usize] as f32 / 255.0;
                if Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(center) < radius_in_pixels + 2.0 {
                    covered_near += coverage;
                }
            }
        }
        let expected = std::f32::consts::PI * radius_in_pixels * radius_in_pixels;
        assert!(
            (covered_near - expected).abs() <= 0.2 * expected,
            "the faraway {name} covers {covered_near} pixels instead of about {expected}"
        );
    }
    let total_coverage: f32 = pixels.chunks_exact(4).map(|pixel| pixel[3] as f32 / 255.0).sum();
    let near_ball_and_mesh = [(ball_position, ball_distance), (mesh_position, mesh_distance)]
        .map(|(_, distance)| std::f32::consts::PI * (pixels_per_unit_at_unit_distance / distance).powi(2))
        .iter()
        .sum::<f32>();
    assert!(
        total_coverage <= 1.3 * near_ball_and_mesh,
        "something drew away from the two shapes: {total_coverage} pixels covered"
    );
    assert!(renderer.device().pop_error_scope().await.is_none());
    Ok(())
}
