#[path = "helpers/render_capture.rs"]
mod render_capture;

// The GPU tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test culling_tests -- --ignored

use game_types::{
    object_types::{Orientation3D, PhysicsInfo3D, VisualSettings, WorldLightingInfo, shared::Color3},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    culling::{
        BoundingSphere, SHADOW_CASTER_BUCKET_COUNT, SHADOW_CASTER_MIN_TEXELS, ShadowCascadeSquares, ShadowCasterBucketStarts, ViewEdges, largest_stretch,
        shadow_caster_ranges,
    },
    render_scene::{RenderScene, SHADOW_CASCADE_COUNT, build_render_scene},
    renderer::Renderer,
    renderer_types::{RenderBrick, RenderObject, RenderTransform},
};
use std::collections::{HashMap, HashSet};

const HALF_SIZES: [f32; SHADOW_CASCADE_COUNT] = [16.0, 32.0, 64.0, 256.0, 2048.0];
const SHADOW_MAP_SIZE: u32 = 2048;

fn sphere(center: Vec3, radius: f32) -> BoundingSphere {
    BoundingSphere { center, radius }
}

#[test]
fn the_view_keeps_what_reaches_into_the_picture_and_nothing_behind_the_camera() {
    let view_edges = ViewEdges::of_view_proj(build_camera_view_proj_matrix(Vec3::ZERO, Vec3::NEG_Z, 1.0, DEFAULT_CAMERA_FOV_DEGREES));
    // 10 units ahead, the picture's left edge is this far to the left, and it leans out from the camera at half the field of view.
    let half_field_of_view = (DEFAULT_CAMERA_FOV_DEGREES / 2.0).to_radians();
    let left_edge_x = -10.0 * half_field_of_view.tan();
    let distance_past_left_edge = 2.0 * half_field_of_view.cos();
    assert!(view_edges.contains(sphere(Vec3::new(0.0, 0.0, -10.0), 0.1)), "straight ahead");
    assert!(!view_edges.contains(sphere(Vec3::new(0.0, 0.0, 10.0), 1.0)), "behind the camera");
    assert!(
        !view_edges.contains(sphere(Vec3::new(left_edge_x - 2.0, 0.0, -10.0), distance_past_left_edge - 0.01)),
        "past the left edge"
    );
    assert!(
        view_edges.contains(sphere(Vec3::new(left_edge_x - 2.0, 0.0, -10.0), distance_past_left_edge + 0.01)),
        "past the left edge, but wide enough to reach back in"
    );
    assert!(view_edges.contains(sphere(Vec3::new(0.0, 0.0, 50.0), 60.0)), "around the camera");
}

#[test]
fn largest_stretch_is_the_longest_axis_of_a_rotated_scaled_model_and_never_less_than_any_stretch() {
    let rotated_scaled = Mat4::from_scale_rotation_translation(
        Vec3::new(1.0, 3.0, 0.5),
        Quat::from_euler(EulerRot::YXZ, 0.3, 1.1, -0.7),
        Vec3::new(5.0, 6.0, 7.0),
    );
    assert!((largest_stretch(rotated_scaled) - 3.0).abs() < 1e-4);
    // Skewed, a diagonal stretches by the golden ratio, further than either axis's length of 1 and the square root of 2.
    let skewed = Mat4::from_cols(Vec4::X, Vec4::new(1.0, 1.0, 0.0, 0.0), Vec4::Z, Vec4::W);
    assert!(largest_stretch(skewed) >= (1.0 + 5.0f32.sqrt()) / 2.0);
}

// Cascades looking straight down at the origin, so a point's place across and up each square is its x and z.
fn shadow_cascade_squares() -> ShadowCascadeSquares {
    let view = Mat4::look_at_rh(Vec3::new(0.0, 100.0, 0.0), Vec3::ZERO, Vec3::NEG_Z);
    let view_projections = HALF_SIZES.map(|half_size| Mat4::orthographic_rh(-half_size, half_size, -half_size, half_size, 0.1, 200.0) * view);
    ShadowCascadeSquares::new(&view_projections, HALF_SIZES, SHADOW_MAP_SIZE)
}

// The cascades a caster should cast into, checked one at a time: those whose square it reaches, and whose texels it is
// wide enough for.
fn cascades_by_hand(sphere: BoundingSphere) -> HashSet<usize> {
    (0..SHADOW_CASCADE_COUNT)
        .filter(|&cascade_index| {
            let reach = HALF_SIZES[cascade_index] + sphere.radius;
            let texel_size = 2.0 * HALF_SIZES[cascade_index] / SHADOW_MAP_SIZE as f32;
            sphere.center.x.abs() <= reach && sphere.center.z.abs() <= reach && 2.0 * sphere.radius >= SHADOW_CASTER_MIN_TEXELS * texel_size
        })
        .collect()
}

#[test]
fn casters_land_in_the_cascades_they_reach_and_are_wide_enough_for() {
    let squares = shadow_cascade_squares();
    let bucket = |sphere: BoundingSphere| squares.shadow_caster_bucket(sphere);
    let cascades = |sphere: BoundingSphere| {
        let mut bucket_starts: ShadowCasterBucketStarts = [0; SHADOW_CASTER_BUCKET_COUNT + 1];
        if let Some(bucket) = bucket(sphere) {
            bucket_starts[bucket + 1..].fill(1);
        }
        (0..SHADOW_CASCADE_COUNT)
            .filter(|&cascade_index| shadow_caster_ranges(&bucket_starts, cascade_index).next().is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(cascades(sphere(Vec3::ZERO, 10.0)), [0, 1, 2, 3, 4], "big and near the middle");
    assert_eq!(
        cascades(sphere(Vec3::new(100.0, 0.0, 0.0), 10.0)),
        [3, 4],
        "big but outside the three small squares"
    );
    assert_eq!(
        cascades(sphere(Vec3::new(70.0, 0.0, 0.0), 10.0)),
        [2, 3, 4],
        "outside a square, but reaching into it"
    );
    assert_eq!(
        cascades(sphere(Vec3::ZERO, 0.5)),
        [0, 1, 2, 3],
        "too narrow for the outer cascade's two-unit texels"
    );
    assert_eq!(
        cascades(sphere(Vec3::new(1000.0, 0.0, 0.0), 0.5)),
        [] as [usize; 0],
        "too narrow for the only square it reaches"
    );
    assert_eq!(cascades(sphere(Vec3::new(3000.0, 0.0, 0.0), 10.0)), [] as [usize; 0], "outside every square");
}

// Laid out bucket by bucket, as the renderer does, each cascade's ranges hold exactly the casters it should draw.
#[test]
fn each_cascade_draws_exactly_its_casters() {
    let squares = shadow_cascade_squares();
    let mut seed: u64 = 7;
    let mut random = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    let spheres: Vec<BoundingSphere> = (0..4000)
        .map(|_| {
            // Spread out over the outer cascade and a little past it, from tiny pebbles to whole buildings.
            let center = Vec3::new(random() * 4400.0 - 2200.0, random() * 20.0, random() * 4400.0 - 2200.0) * random().powi(4);
            sphere(center, 0.01 * 3000.0f32.powf(random()))
        })
        .collect();
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); SHADOW_CASTER_BUCKET_COUNT];
    for (caster_index, sphere) in spheres.iter().enumerate() {
        if let Some(bucket) = squares.shadow_caster_bucket(*sphere) {
            buckets[bucket].push(caster_index);
        }
    }
    let laid_out: Vec<usize> = buckets.concat();
    let mut bucket_starts: ShadowCasterBucketStarts = [0; SHADOW_CASTER_BUCKET_COUNT + 1];
    for (bucket_index, bucket) in buckets.iter().enumerate() {
        bucket_starts[bucket_index + 1] = bucket_starts[bucket_index] + bucket.len() as u32;
    }
    for cascade_index in 0..SHADOW_CASCADE_COUNT {
        let mut drawn: Vec<usize> = Vec::new();
        for instance_range in shadow_caster_ranges(&bucket_starts, cascade_index) {
            drawn.extend(&laid_out[instance_range.start as usize..instance_range.end as usize]);
        }
        let drawn_set: HashSet<usize> = drawn.iter().copied().collect();
        assert_eq!(drawn.len(), drawn_set.len(), "cascade {cascade_index} draws a caster twice");
        let expected: HashSet<usize> = (0..spheres.len())
            .filter(|&caster_index| cascades_by_hand(spheres[caster_index]).contains(&cascade_index))
            .collect();
        assert!(!expected.is_empty(), "the spread should give cascade {cascade_index} casters");
        assert_eq!(drawn_set, expected, "cascade {cascade_index}");
    }
}

#[test]
fn runs_with_nothing_between_them_draw_as_one_range() {
    // Two casters that reach the first square: one wide enough for every cascade, one too narrow for the outer cascade.
    // Nothing sits between them, so the first cascade draws both at once.
    let squares = shadow_cascade_squares();
    let buckets = [
        squares.shadow_caster_bucket(sphere(Vec3::ZERO, 10.0)),
        squares.shadow_caster_bucket(sphere(Vec3::ZERO, 0.5)),
    ]
    .map(Option::unwrap);
    let mut bucket_starts: ShadowCasterBucketStarts = [0; SHADOW_CASTER_BUCKET_COUNT + 1];
    for bucket in buckets {
        for start in &mut bucket_starts[bucket + 1..] {
            *start += 1;
        }
    }
    assert_eq!(shadow_caster_ranges(&bucket_starts, 0).collect::<Vec<_>>(), [0..2]);
    assert_eq!(shadow_caster_ranges(&bucket_starts, 4).collect::<Vec<_>>(), [0..1]);
}

const SIZE: u32 = 256;

fn brick(position: Vec3, size: Vec3) -> RenderObject {
    RenderObject::Brick(RenderBrick {
        transform: RenderTransform {
            physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            teleport_count: 0,
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        },
        visualSettings: VisualSettings {
            color: Color3::WHITE,
            ..VisualSettings::TYPICAL_OPAQUE
        },
        primitive: BrickPrimitiveSettings { size },
        material: None,
    })
}

async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)
}

// The red channel and the coverage of the pixels each world point lands on, as 0 to 1.
async fn capture<const N: usize>(
    renderer: &mut Renderer,
    scene: &RenderScene,
    camera_position: Vec3,
    camera_target: Vec3,
    world_points: [Vec3; N],
) -> [(f32, f32); N] {
    let view_proj = build_camera_view_proj_matrix(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, scene, None, &camera, camera_target, camera_position, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Culling readback"),
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
    renderer.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.await.unwrap().unwrap();
    let pixels = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    world_points.map(|world_point| {
        let ndc = view_proj.project_point3(world_point);
        assert!(ndc.x.abs() < 0.9 && ndc.y.abs() < 0.9, "{world_point} should be on screen");
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as usize;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as usize;
        let pixel = &pixels[(y * SIZE as usize + x) * 4..][..4];
        (pixel[0] as f32 / 255.0, pixel[3] as f32 / 255.0)
    })
}

// A wall off to the side of the picture, lit by a low sun 20 degrees up from +X, still throws its long shadow across the
// ground the camera sees.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_caster_outside_the_picture_still_shadows_the_ground_in_it() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let world_lighting_info = WorldLightingInfo {
        sunOrientation: Orientation3D {
            theta: std::f32::consts::FRAC_PI_2,
            phi: -20.0f32.to_radians(),
        },
        sunColor: Color3::WHITE,
        sunIntensity: 3.0,
        ambientIntensity: 0.0,
        ambientOcclusionIntensity: 0.0,
        ..WorldLightingInfo::default()
    };
    let objects = HashMap::from([
        (1, brick(Vec3::new(0.0, -0.5, 0.0), Vec3::new(40.0, 1.0, 40.0))),
        (2, RenderObject::WorldLighting(world_lighting_info)),
        // 4 high, so its shadow runs 11 units toward -X, out to where the camera looks.
        (3, brick(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.2, 4.0, 3.0))),
    ]);
    let scene = build_render_scene(&objects, &HashMap::new(), None);
    let (camera_position, camera_target) = (Vec3::new(-9.0, 3.0, 3.0), Vec3::new(-9.0, 0.0, 0.0));
    let view_proj = build_camera_view_proj_matrix(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let wall_corners = [-0.1, 0.1].map(|x| [0.0, 4.0].map(|y| [-1.5, 1.5].map(|z| view_proj * Vec4::new(x, y, z, 1.0))));
    assert!(
        wall_corners.as_flattened().as_flattened().iter().all(|clip| clip.x > clip.w),
        "the whole wall should be past the right edge of the picture"
    );
    let [(shadowed, _), (lit, _)] = capture(
        &mut renderer,
        &scene,
        camera_position,
        camera_target,
        [Vec3::new(-9.0, 0.0, 0.0), Vec3::new(-9.0, 0.0, -3.0)],
    )
    .await;
    assert!(
        shadowed < 0.3 * lit,
        "the wall's shadow should darken the ground: {shadowed} in its shadow, {lit} in the sun"
    );
    Ok(())
}

// A floor whose middle is far behind the camera still covers the ground ahead of it.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn something_whose_middle_is_behind_the_camera_still_draws_where_it_reaches_into_the_picture() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let objects = HashMap::from([(1, brick(Vec3::new(0.0, -0.5, 90.0), Vec3::new(200.0, 1.0, 200.0)))]);
    let mut scene = build_render_scene(&objects, &HashMap::new(), None);
    scene.should_draw_sky = false;
    let [(_, coverage)] = capture(
        &mut renderer,
        &scene,
        Vec3::new(0.0, 2.0, 0.0),
        Vec3::new(0.0, 0.0, -6.0),
        [Vec3::new(0.0, 0.0, -6.0)],
    )
    .await;
    assert_eq!(coverage, 1.0, "the floor should cover the ground ahead of the camera");
    Ok(())
}
