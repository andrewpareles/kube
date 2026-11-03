#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test ambient_occlusion_tests -- --ignored

use game_types::{
    object_types::{Orientation3D, PhysicsInfo3D, VisualSettings, WorldLightingInfo, shared::Color3},
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings},
};
use glam::{Quat, Vec3};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    render_scene::{RenderScene, build_render_scene},
    renderer::Renderer,
    renderer_types::{RenderBall, RenderBrick, RenderObject, RenderTransform},
};
use std::collections::HashMap;

const SIZE: u32 = 512;
// A wall along the back of the floor, with its front face at z = -2. One point sits just in front of its foot, the
// other far out on the floor.
const WALL: (Vec3, Vec3) = (Vec3::new(0.0, 2.0, -2.5), Vec3::new(12.0, 4.0, 1.0));
const WALL_CAMERA_POSITION: Vec3 = Vec3::new(0.0, 4.0, 6.0);
const FLOOR_POINT_AT_WALL: Vec3 = Vec3::new(0.0, 0.0, -1.8);
const FLOOR_POINT_IN_THE_OPEN: Vec3 = Vec3::new(0.0, 0.0, 3.0);

fn transform(position: Vec3) -> RenderTransform {
    RenderTransform {
        physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        teleport_count: 0,
        rigid_group_to_follow: None,
        sk_joint_to_follow_arc: None,
    }
}

fn brick(position: Vec3, size: Vec3) -> RenderObject {
    RenderObject::Brick(RenderBrick {
        transform: transform(position),
        visualSettings: VisualSettings {
            opacity: 1.0,
            color: Color3::WHITE,
            glow: 0.0,
            isAlwaysOnTop: false,
        },
        primitive: BrickPrimitiveSettings { size },
        material: None,
    })
}

// Lit only by white sky light, so a wall's sun shadow cannot be mistaken for occlusion.
fn sky_lit(world_lighting_info: WorldLightingInfo) -> WorldLightingInfo {
    WorldLightingInfo {
        sunIntensity: 0.0,
        ambientTopColor: Color3::WHITE,
        ambientBottomColor: Color3::WHITE,
        ambientIntensity: 0.5,
        ..world_lighting_info
    }
}

// Lit only by a sun straight overhead, which casts no shadow beside a wall.
fn sun_lit(world_lighting_info: WorldLightingInfo) -> WorldLightingInfo {
    WorldLightingInfo {
        sunOrientation: Orientation3D {
            theta: 0.0,
            phi: -std::f32::consts::FRAC_PI_2,
        },
        sunColor: Color3::WHITE,
        sunIntensity: 3.0,
        ambientIntensity: 0.0,
        ..world_lighting_info
    }
}

fn build_floor_scene(walls: &[(Vec3, Vec3)], world_lighting_info: WorldLightingInfo) -> RenderScene {
    let mut objects = HashMap::from([
        (1, brick(Vec3::new(0.0, -0.5, 0.0), Vec3::new(12.0, 1.0, 12.0))),
        (2, RenderObject::WorldLighting(world_lighting_info)),
    ]);
    for (index, (position, size)) in walls.iter().enumerate() {
        objects.insert(10 + index, brick(*position, *size));
    }
    build_render_scene(&objects, &HashMap::new(), None)
}

// The whole picture, as rows of RGBA pixels.
async fn capture_image(renderer: &mut Renderer, scene: &RenderScene, camera_position: Vec3, camera_target: Vec3) -> Vec<u8> {
    let view_proj = build_camera_view_proj_matrix(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, scene, None, &camera, camera_target, camera_position, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Ambient occlusion readback"),
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
    pixels
}

// The pixels the world points land on, seen from the camera.
async fn capture_pixels<const N: usize>(
    renderer: &mut Renderer,
    scene: &RenderScene,
    camera_position: Vec3,
    camera_target: Vec3,
    world_points: [Vec3; N],
) -> [[u8; 4]; N] {
    let pixels = capture_image(renderer, scene, camera_position, camera_target).await;
    let view_proj = build_camera_view_proj_matrix(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    world_points.map(|world_point| {
        let ndc = view_proj.project_point3(world_point);
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as usize;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as usize;
        let index = (y * SIZE as usize + x) * 4;
        pixels[index..index + 4].try_into().unwrap()
    })
}

async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)
}

// The floor at the foot of a wall, with and without the wall there, under the given lighting.
async fn capture_floor_at_wall(renderer: &mut Renderer, world_lighting_info: WorldLightingInfo) -> ([u8; 4], [u8; 4]) {
    let points = [FLOOR_POINT_AT_WALL, FLOOR_POINT_IN_THE_OPEN];
    let [without_wall, _] = capture_pixels(renderer, &build_floor_scene(&[], world_lighting_info), WALL_CAMERA_POSITION, Vec3::ZERO, points).await;
    let [with_wall, _] = capture_pixels(
        renderer,
        &build_floor_scene(&[WALL], world_lighting_info),
        WALL_CAMERA_POSITION,
        Vec3::ZERO,
        points,
    )
    .await;
    (without_wall, with_wall)
}

fn is_close(left: [u8; 4], right: [u8; 4]) -> bool {
    (0..3).all(|channel| left[channel].abs_diff(right[channel]) <= 2)
}

fn is_clearly_darker(darker: [u8; 4], brighter: [u8; 4]) -> bool {
    (0..3).all(|channel| darker[channel].saturating_add(8) < brighter[channel])
}

fn linear(value: u8) -> f32 {
    (((value as f32 / 255.0) + 0.055) / 1.055).powf(2.4)
}

// The share of its light a spot keeps once something is put near it.
fn darkening(with_occluder: [u8; 4], without_occluder: [u8; 4]) -> f32 {
    linear(with_occluder[0]) / linear(without_occluder[0])
}

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn ambient_occlusion_darkens_the_floor_at_a_wall_but_not_open_floor() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let world_lighting_info = sky_lit(WorldLightingInfo::default());
    let points = [FLOOR_POINT_AT_WALL, FLOOR_POINT_IN_THE_OPEN];
    let [at_wall_without_wall, open_without_wall] = capture_pixels(
        &mut renderer,
        &build_floor_scene(&[], world_lighting_info),
        WALL_CAMERA_POSITION,
        Vec3::ZERO,
        points,
    )
    .await;
    let [at_wall_with_wall, open_with_wall] = capture_pixels(
        &mut renderer,
        &build_floor_scene(&[WALL], world_lighting_info),
        WALL_CAMERA_POSITION,
        Vec3::ZERO,
        points,
    )
    .await;
    assert!(
        is_close(open_with_wall, open_without_wall),
        "the wall darkened floor well outside its reach: {open_with_wall:?} vs {open_without_wall:?}"
    );
    assert!(
        is_clearly_darker(at_wall_with_wall, at_wall_without_wall),
        "the floor at the foot of the wall {at_wall_with_wall:?} should be darker than without the wall {at_wall_without_wall:?}"
    );
    Ok(())
}

// Intensity 0 turns ambient occlusion off, and the sunlight strength decides whether sunlit creases darken at all.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn ambient_occlusion_follows_the_world_lighting_settings() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let (without_wall, with_wall) = capture_floor_at_wall(
        &mut renderer,
        sky_lit(WorldLightingInfo {
            ambientOcclusionIntensity: 0.0,
            ..WorldLightingInfo::default()
        }),
    )
    .await;
    assert!(
        is_close(with_wall, without_wall),
        "intensity 0 still darkened the floor: {with_wall:?} vs {without_wall:?}"
    );

    let (without_wall, with_wall) = capture_floor_at_wall(
        &mut renderer,
        sun_lit(WorldLightingInfo {
            ambientOcclusionSunlightStrength: 0.0,
            ..WorldLightingInfo::default()
        }),
    )
    .await;
    assert!(
        is_close(with_wall, without_wall),
        "sunlight strength 0 still darkened sunlight: {with_wall:?} vs {without_wall:?}"
    );

    let (without_wall, with_wall) = capture_floor_at_wall(
        &mut renderer,
        sun_lit(WorldLightingInfo {
            ambientOcclusionSunlightStrength: 1.0,
            ..WorldLightingInfo::default()
        }),
    )
    .await;
    assert!(
        is_clearly_darker(with_wall, without_wall),
        "sunlight strength 1 should darken sunlight in the crease: {with_wall:?} vs {without_wall:?}"
    );
    Ok(())
}

// The inside corner of a room, where the floor meets two walls, is about as deep a crease as screen space ambient
// occlusion can see. It should come out clearly darker, but never black: the intensity is the darkest a crease is
// allowed to get.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn the_deepest_crease_never_goes_black_at_the_default_intensity() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let walls = [WALL, (Vec3::new(-2.5, 2.0, 0.0), Vec3::new(1.0, 4.0, 12.0))];
    let [corner_floor, open_floor] = capture_pixels(
        &mut renderer,
        &build_floor_scene(&walls, sky_lit(WorldLightingInfo::default())),
        Vec3::new(3.0, 5.0, 5.0),
        Vec3::ZERO,
        [Vec3::new(-1.85, 0.0, -1.85), Vec3::new(3.0, 0.0, 0.0)],
    )
    .await;
    assert!(
        is_clearly_darker(corner_floor, open_floor),
        "the corner {corner_floor:?} should be darker than open floor {open_floor:?}"
    );
    for channel in 0..3 {
        assert!(
            linear(corner_floor[channel]) >= 0.15 * linear(open_floor[channel]),
            "the corner {corner_floor:?} went nearly black next to open floor {open_floor:?}"
        );
    }
    Ok(())
}

// A spot one unit out from a wall stays just as dark whether the camera is close or far away.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn ambient_occlusion_keeps_its_size_as_the_camera_moves_closer() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let world_lighting_info = sky_lit(WorldLightingInfo {
        ambientOcclusionRadius: 2.4,
        ..WorldLightingInfo::default()
    });
    let point = Vec3::new(0.0, 0.0, -1.0);
    let mut darkenings = Vec::new();
    for distance in [3.0, 12.0] {
        let camera_position = point + Vec3::new(0.0, 0.6, 1.0).normalize() * distance;
        let [without_wall] = capture_pixels(&mut renderer, &build_floor_scene(&[], world_lighting_info), camera_position, point, [point]).await;
        let [with_wall] = capture_pixels(&mut renderer, &build_floor_scene(&[WALL], world_lighting_info), camera_position, point, [point]).await;
        darkenings.push(darkening(with_wall, without_wall));
    }
    assert!(darkenings[1] < 0.98, "the wall should darken a spot within its reach: {darkenings:?}");
    // One step of 8-bit color is about 1.2% of the light here.
    assert!(
        (darkenings[0] - darkenings[1]).abs() < 0.015,
        "up close and far away the spot kept different shares of its light: {darkenings:?}"
    );
    Ok(())
}

// The foot of a wall stays just as dark when the camera turns until it sits at the edge of the screen, where half of what
// surrounds it is off screen.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn ambient_occlusion_does_not_fade_toward_the_edge_of_the_screen() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let world_lighting_info = sky_lit(WorldLightingInfo::default());
    let edge_camera_target = FLOOR_POINT_AT_WALL + Vec3::new(5.5, 0.0, 0.0);
    let edge_ndc = build_camera_view_proj_matrix(WALL_CAMERA_POSITION, edge_camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES).project_point3(FLOOR_POINT_AT_WALL);
    assert!(edge_ndc.x < -0.85, "the spot should sit at the left edge of the screen, not at {edge_ndc}");
    let mut darkenings = Vec::new();
    for camera_target in [FLOOR_POINT_AT_WALL, edge_camera_target] {
        let points = [FLOOR_POINT_AT_WALL];
        let [without_wall] = capture_pixels(
            &mut renderer,
            &build_floor_scene(&[], world_lighting_info),
            WALL_CAMERA_POSITION,
            camera_target,
            points,
        )
        .await;
        let [with_wall] = capture_pixels(
            &mut renderer,
            &build_floor_scene(&[WALL], world_lighting_info),
            WALL_CAMERA_POSITION,
            camera_target,
            points,
        )
        .await;
        darkenings.push(darkening(with_wall, without_wall));
    }
    assert!(
        (darkenings[0] - darkenings[1]).abs() < 0.05,
        "in the middle and at the edge of the screen the spot kept different shares of its light: {darkenings:?}"
    );
    Ok(())
}

// Open ground has nothing near it to block its sky, so it gets no darkening anywhere on screen, including toward the
// edges, where the camera sees it at a slant.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn open_ground_is_not_darkened_anywhere_on_screen() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let [with_ambient_occlusion, without_ambient_occlusion] = [WorldLightingInfo::default().ambientOcclusionIntensity, 0.0].map(|intensity| {
        build_floor_scene(
            &[],
            sky_lit(WorldLightingInfo {
                ambientOcclusionIntensity: intensity,
                ..WorldLightingInfo::default()
            }),
        )
    });
    let camera_position = Vec3::new(0.0, 3.0, 5.0);
    let pixels_with = capture_image(&mut renderer, &with_ambient_occlusion, camera_position, Vec3::ZERO).await;
    let pixels_without = capture_image(&mut renderer, &without_ambient_occlusion, camera_position, Vec3::ZERO).await;
    let largest_difference = pixels_with.iter().zip(&pixels_without).map(|(with, without)| with.abs_diff(*without)).max();
    assert!(largest_difference <= Some(1), "open ground was darkened by up to {largest_difference:?} levels");
    Ok(())
}

// A post standing well in front of the wall hides part of what surrounds the foot of the wall on screen, but not the spot
// itself, and it is too far away to block that spot's sky. The spot should stay just as dark.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn ambient_occlusion_stays_when_something_in_front_hides_part_of_its_surroundings() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let world_lighting_info = sky_lit(WorldLightingInfo::default());
    let post = (Vec3::new(0.5, 2.0, 2.0), Vec3::new(0.4, 4.0, 0.4));
    let view_proj = build_camera_view_proj_matrix(WALL_CAMERA_POSITION, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let post_left_ndc_x = view_proj.project_point3(Vec3::new(0.3, 2.0, 2.2)).x;
    let spot_ndc_x = view_proj.project_point3(FLOOR_POINT_AT_WALL).x;
    let post_distance_pixels = (post_left_ndc_x - spot_ndc_x) * SIZE as f32 * 0.5;
    assert!(
        (8.0..40.0).contains(&post_distance_pixels),
        "the post should be just beside the spot on screen, not {post_distance_pixels} pixels away"
    );
    let points = [FLOOR_POINT_AT_WALL];
    let [without_wall] = capture_pixels(
        &mut renderer,
        &build_floor_scene(&[], world_lighting_info),
        WALL_CAMERA_POSITION,
        Vec3::ZERO,
        points,
    )
    .await;
    let [with_wall] = capture_pixels(
        &mut renderer,
        &build_floor_scene(&[WALL], world_lighting_info),
        WALL_CAMERA_POSITION,
        Vec3::ZERO,
        points,
    )
    .await;
    let [with_wall_and_post] = capture_pixels(
        &mut renderer,
        &build_floor_scene(&[WALL, post], world_lighting_info),
        WALL_CAMERA_POSITION,
        Vec3::ZERO,
        points,
    )
    .await;
    let darkening_without_post = darkening(with_wall, without_wall);
    let darkening_with_post = darkening(with_wall_and_post, without_wall);
    assert!(
        (darkening_without_post - darkening_with_post).abs() < 0.03,
        "the post changed the spot's share of its light from {darkening_without_post} to {darkening_with_post}"
    );
    Ok(())
}

// A ball resting on the ground darkens the ground around where it touches. That darkening should be a smooth gradient, not
// a speckle of individual pixels.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn ambient_occlusion_around_a_ball_is_smooth() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let mut scene_objects = HashMap::from([
        (1, brick(Vec3::new(0.0, -0.5, 0.0), Vec3::new(12.0, 1.0, 12.0))),
        (2, RenderObject::WorldLighting(sky_lit(WorldLightingInfo::default()))),
    ]);
    scene_objects.insert(
        3,
        RenderObject::Ball(RenderBall {
            transform: transform(Vec3::new(0.0, 1.0, 0.0)),
            visualSettings: VisualSettings {
                opacity: 1.0,
                color: Color3 { r: 0.0, g: 0.0, b: 0.0 },
                glow: 0.0,
                isAlwaysOnTop: false,
            },
            primitive: BallPrimitiveSettings { radius: 1.0 },
            material: None,
        }),
    );
    let scene = build_render_scene(&scene_objects, &HashMap::new(), None);
    let pixels = capture_image(&mut renderer, &scene, Vec3::new(0.0, 3.0, 4.0), Vec3::new(0.0, 0.5, 0.0)).await;
    let size = SIZE as usize;
    let red = |x: usize, y: usize| pixels[(y * size + x) * 4] as i32;
    // Ground pixels in the bottom half of the picture, below the horizon, at least 4 pixels from the black ball.
    let ground_brightness = red(size / 2, size - 2);
    let is_ground_all_around = |x: usize, y: usize| (y - 4..=y + 4).all(|y| (x - 4..=x + 4).all(|x| red(x, y) > ground_brightness / 2));
    let mut second_differences = Vec::new();
    let mut darkest_ground = ground_brightness;
    for y in size / 2..size - 5 {
        for x in 5..size - 5 {
            if !is_ground_all_around(x, y) {
                continue;
            }
            darkest_ground = darkest_ground.min(red(x, y));
            second_differences.push((red(x - 1, y) - 2 * red(x, y) + red(x + 1, y)).abs());
            second_differences.push((red(x, y - 1) - 2 * red(x, y) + red(x, y + 1)).abs());
        }
    }
    assert!(
        darkest_ground < ground_brightness - 20,
        "the ball should darken the ground around it: the darkest ground is {darkest_ground}, open ground is {ground_brightness}"
    );
    let speckle_count = second_differences.iter().filter(|&&difference| difference >= 4).count();
    assert!(
        speckle_count * 500 < second_differences.len(),
        "{speckle_count} of {} ground pixels stand out from their neighbors",
        second_differences.len()
    );
    Ok(())
}
