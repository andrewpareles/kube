#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test sun_shadow_tests -- --ignored

use game_types::{
    object_types::{Orientation3D, PhysicsInfo3D, VisualSettings, WorldLightingInfo, shared::Color3},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    render_scene::{RenderScene, build_render_scene},
    renderer::Renderer,
    renderer_types::{RenderBrick, RenderObject, RenderTransform},
};
use std::collections::HashMap;

const SIZE: u32 = 512;
// A thin wall 4 high and 3 wide, lit by a sun 45 degrees up from +X, so its shadow runs 4 units toward -X. At a spot d
// units out along the shadow, the wall's side edge casts the shadow's side edge from d units up.
const WALL: (Vec3, Vec3) = (Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.2, 4.0, 3.0));
const SHADOW_SIDE_EDGE_Z: f32 = 1.5;
const CAMERA_POSITION: Vec3 = Vec3::new(-2.0, 4.0, 3.5);
const CAMERA_TARGET: Vec3 = Vec3::new(-2.0, 0.0, 1.5);

fn brick(position: Vec3, size: Vec3) -> RenderObject {
    RenderObject::Brick(RenderBrick {
        transform: RenderTransform {
            physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            teleport_count: 0,
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        },
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

// Lit by the sun alone, so a shadowed spot goes dark and the shadow edge is all that changes.
fn build_wall_scene(sun_shadow_softness: f32) -> RenderScene {
    let world_lighting_info = WorldLightingInfo {
        sunOrientation: Orientation3D {
            theta: std::f32::consts::FRAC_PI_2,
            phi: -std::f32::consts::FRAC_PI_4,
        },
        sunColor: Color3::WHITE,
        sunIntensity: 3.0,
        sunShadowSoftness: sun_shadow_softness,
        ambientIntensity: 0.0,
        ambientOcclusionIntensity: 0.0,
        ..WorldLightingInfo::default()
    };
    let objects = HashMap::from([
        (1, brick(Vec3::new(0.0, -0.5, 0.0), Vec3::new(40.0, 1.0, 40.0))),
        (2, RenderObject::WorldLighting(world_lighting_info)),
        (3, brick(WALL.0, WALL.1)),
    ]);
    build_render_scene(&objects, &HashMap::new(), None)
}

async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)
}

// The linear brightness of the pixel each world point lands on.
async fn capture_brightness<const N: usize>(renderer: &mut Renderer, scene: &RenderScene, world_points: [Vec3; N]) -> [f32; N] {
    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, CAMERA_TARGET, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(CAMERA_POSITION, CAMERA_TARGET, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, scene, None, &camera, CAMERA_TARGET, CAMERA_POSITION, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Sun shadow readback"),
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
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as usize;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as usize;
        let value = pixels[(y * SIZE as usize + x) * 4] as f32 / 255.0;
        ((value + 0.055) / 1.055).powf(2.4)
    })
}

const EDGE_SAMPLE_COUNT: usize = 121;

// Points across the shadow's side edge, from inside the shadow out into the sun, at `distance` out along the shadow.
fn points_across_side_edge(distance: f32) -> [Vec3; EDGE_SAMPLE_COUNT] {
    std::array::from_fn(|index| Vec3::new(-distance, 0.0, SHADOW_SIDE_EDGE_Z - 0.6 + index as f32 * 0.01))
}

// How far it takes the brightness to climb from 10% to 90% of the way from the shadow to full sun.
fn edge_width(brightness: &[f32; EDGE_SAMPLE_COUNT]) -> f32 {
    let dark = brightness[0];
    let lit = brightness[EDGE_SAMPLE_COUNT - 1];
    let crossing = |fraction: f32| {
        let level = dark + (lit - dark) * fraction;
        let index = brightness.iter().position(|&value| value >= level).expect("the edge should reach full sun");
        if index == 0 {
            return 0.0;
        }
        let (before, after) = (brightness[index - 1], brightness[index]);
        (index as f32 - 1.0 + (level - before) / (after - before).max(1e-6)) * 0.01
    };
    crossing(0.9) - crossing(0.1)
}

async fn measure_edge_widths(renderer: &mut Renderer, sun_shadow_softness: f32) -> (f32, f32) {
    let scene = build_wall_scene(sun_shadow_softness);
    let near = capture_brightness(renderer, &scene, points_across_side_edge(0.4)).await;
    let far = capture_brightness(renderer, &scene, points_across_side_edge(3.4)).await;
    (edge_width(&near), edge_width(&far))
}

// A shadow's edge is just as soft right where the wall meets the ground as it is far out along the shadow.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn sun_shadow_edges_are_equally_soft_near_and_far_from_what_casts_them() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let (near_width, far_width) = measure_edge_widths(&mut renderer, WorldLightingInfo::default().sunShadowSoftness).await;
    assert!(
        near_width > 0.06,
        "the default edge should be soft even at the wall, but it spans only {near_width}"
    );
    assert!(
        (far_width - near_width).abs() < 0.2 * near_width,
        "the edge should be as soft far out as at the wall: {near_width} near the wall, {far_width} far out"
    );
    let (_, wider_width) = measure_edge_widths(&mut renderer, 3.0 * WorldLightingInfo::default().sunShadowSoftness).await;
    assert!(
        wider_width > 2.0 * far_width,
        "tripling the softness should widen the edge: {far_width} at the default, {wider_width} at triple"
    );
    Ok(())
}

// No softness casts the same hard edge everywhere.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn zero_shadow_softness_keeps_every_shadow_edge_hard() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let (near_width, far_width) = measure_edge_widths(&mut renderer, 0.0).await;
    assert!(far_width < 0.05, "no softness should cast a hard edge, but it spans {far_width}");
    assert!(
        far_width < 1.5 * near_width + 0.01,
        "no softness should not soften the edge away from the wall: {near_width} near, {far_width} far"
    );
    Ok(())
}

// Along a very soft edge the blur should stay smooth, not ripple where its taps cross shadow map texels.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn soft_shadow_edges_do_not_wobble() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let scene = build_wall_scene(0.5);
    let points: [Vec3; 201] = std::array::from_fn(|index| Vec3::new(-1.5 - index as f32 * 0.01, 0.0, SHADOW_SIDE_EDGE_Z));
    let brightness = capture_brightness(&mut renderer, &scene, points).await;
    let largest_wobble = brightness
        .windows(3)
        .map(|window| (window[0] - 2.0 * window[1] + window[2]).abs())
        .fold(0.0, f32::max);
    assert!(
        largest_wobble < 0.02,
        "the brightness along the middle of the soft edge wobbles by up to {largest_wobble}: {brightness:?}"
    );
    Ok(())
}
