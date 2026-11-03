#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test glow_tests -- --ignored

use game_types::{
    object_types::{BillboardMode, PhysicsInfo3D, VisualSettings, WorldLightingInfo, shared::Color3},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    render_scene::{FontAtlas, LabelGeometry, VertexContent, build_render_scene},
    renderer::Renderer,
    renderer_types::{RenderBrick, RenderLabel, RenderLabelContent, RenderObject, RenderTransform},
};
use std::{collections::HashMap, sync::Arc};

const SIZE: u32 = 256;
const CAMERA_POSITION: Vec3 = Vec3::new(0.0, 2.0, 0.0);
const RED: Color3 = Color3 { r: 1.0, g: 0.1, b: 0.1 };
const BLUE: Color3 = Color3 { r: 0.1, g: 0.2, b: 1.0 };
// Just outside the edge of the glowing things these tests put in the middle of the picture.
const PIXEL_BESIDE: (u32, u32) = (SIZE / 2 + 24, SIZE / 2);

async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)
}

fn brick(position: Vec3, size: Vec3, color: Color3, glow: f32) -> RenderObject {
    RenderObject::Brick(RenderBrick {
        transform: RenderTransform {
            physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            teleport_count: 0,
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        },
        visualSettings: VisualSettings {
            color,
            glow,
            ..VisualSettings::TYPICAL_OPAQUE
        },
        primitive: BrickPrimitiveSettings { size },
        material: None,
    })
}

// A big dim gray wall 12 units straight ahead of the camera, under light too faint to show much of anything.
fn build_dark_scene() -> HashMap<usize, RenderObject> {
    HashMap::from([
        (1, brick(Vec3::new(0.0, 2.0, -12.0), Vec3::new(60.0, 60.0, 1.0), Color3::WHITE, 0.0)),
        (
            2,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunIntensity: 0.0,
                ambientTopColor: Color3::WHITE,
                ambientBottomColor: Color3::WHITE,
                ambientIntensity: 0.05,
                ambientOcclusionIntensity: 0.0,
                skyReflectionStrength: 0.0,
                vibrance: 0.0,
                ..WorldLightingInfo::default()
            }),
        ),
    ])
}

// A label with a solid square of text, two units wide, floating in front of the middle of the wall.
fn label(color: Color3, glow: f32) -> RenderObject {
    let vertex = |x: f32, y: f32, u: f32, v: f32| VertexContent {
        position: [x, y, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [u, v],
        tangent: [1.0, 0.0, 0.0, 1.0],
    };
    RenderObject::RenderLabel(RenderLabel {
        physicalObjectId: 1,
        offsetFromObject: Vec3::new(0.0, 0.0, 2.0),
        rotationFromObject: Quat::IDENTITY,
        billboardMode: BillboardMode::Full,
        isConstantSize: false,
        visualSettings: VisualSettings {
            color,
            glow,
            ..VisualSettings::TYPICAL_OPAQUE
        },
        content: RenderLabelContent::Text {
            scale: 0.1,
            backgroundColor: None,
            backgroundOpacity: 1.0,
            geometry: Arc::new(LabelGeometry {
                cache_key: 1,
                vertices: vec![
                    vertex(-10.0, -10.0, 0.0, 1.0),
                    vertex(10.0, -10.0, 1.0, 1.0),
                    vertex(10.0, 10.0, 1.0, 0.0),
                    vertex(-10.0, 10.0, 0.0, 0.0),
                ],
                indices: vec![0, 1, 2, 0, 2, 3],
                background_index_count: 0,
            }),
            atlas: Arc::new(FontAtlas {
                version: 1,
                width: 1,
                height: 1,
                mip_levels: vec![vec![255]],
            }),
        },
    })
}

// Every pixel of the picture, in linear light, row by row.
async fn capture(renderer: &mut Renderer, objects: &HashMap<usize, RenderObject>) -> Vec<[f32; 3]> {
    let camera_target = CAMERA_POSITION + Vec3::NEG_Z;
    let scene = build_render_scene(objects, &HashMap::new(), None);
    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(CAMERA_POSITION, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, &scene, None, &camera, camera_target, CAMERA_POSITION, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Glow readback"),
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
        .chunks_exact(4)
        .map(|pixel| std::array::from_fn(|channel| ((pixel[channel] as f32 / 255.0 + 0.055) / 1.055).powf(2.4)))
        .collect()
}

fn pixel_at(pixels: &[[f32; 3]], (x, y): (u32, u32)) -> [f32; 3] {
    pixels[(y * SIZE + x) as usize]
}

// Glow makes an object shine in its own color with no light on it at all, where the same object without glow is dark.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_glowing_object_shines_in_its_own_color_in_the_dark() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let middle = (SIZE / 2, SIZE / 2);
    let mut dark_objects = build_dark_scene();
    dark_objects.insert(3, brick(Vec3::new(0.0, 2.0, -10.0), Vec3::ONE, RED, 0.0));
    let dark = pixel_at(&capture(&mut renderer, &dark_objects).await, middle);
    let mut glowing_objects = build_dark_scene();
    glowing_objects.insert(3, brick(Vec3::new(0.0, 2.0, -10.0), Vec3::ONE, RED, 1.0));
    let glowing = pixel_at(&capture(&mut renderer, &glowing_objects).await, middle);
    assert!(dark[0] < 0.1, "a red brick with no light on it should be dark: {dark:?}");
    assert!(
        glowing[0] > 0.7 && glowing[0] > 3.0 * glowing[1],
        "a glowing red brick should shine red in the dark: {glowing:?}"
    );
    Ok(())
}

// Glow spills past the edge of a glowing object onto what is around it.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn glow_spreads_onto_the_surroundings() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let mut plain_objects = build_dark_scene();
    plain_objects.insert(3, brick(Vec3::new(0.0, 2.0, -10.0), Vec3::ONE, RED, 0.0));
    let plain_beside = pixel_at(&capture(&mut renderer, &plain_objects).await, PIXEL_BESIDE);
    let mut glowing_objects = build_dark_scene();
    glowing_objects.insert(3, brick(Vec3::new(0.0, 2.0, -10.0), Vec3::ONE, RED, 1.0));
    let glowing_beside = pixel_at(&capture(&mut renderer, &glowing_objects).await, PIXEL_BESIDE);
    assert!(
        glowing_beside[0] > plain_beside[0] + 0.01 && glowing_beside[0] > 2.0 * glowing_beside[2],
        "the wall beside a glowing red brick {glowing_beside:?} should catch its red glow, unlike beside a plain one {plain_beside:?}"
    );
    Ok(())
}

// A wall in front of a glowing object hides its glow too.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_wall_hides_the_glow_of_what_is_behind_it() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let middle = (SIZE / 2, SIZE / 2);
    let mut hidden_objects = build_dark_scene();
    hidden_objects.insert(3, brick(Vec3::new(0.0, 2.0, -14.0), Vec3::ONE, RED, 1.0));
    let hidden = capture(&mut renderer, &hidden_objects).await;
    let empty = capture(&mut renderer, &build_dark_scene()).await;
    for pixel in [middle, PIXEL_BESIDE] {
        let (with_hidden_glow, without) = (pixel_at(&hidden, pixel), pixel_at(&empty, pixel));
        assert!(
            (0..3).all(|channel| (with_hidden_glow[channel] - without[channel]).abs() < 0.005),
            "a glowing brick behind the wall should not show through it: {with_hidden_glow:?} vs {without:?}"
        );
    }
    Ok(())
}

// Labels glow too, which is how a neon sign's text glows.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn glowing_labels_spread_glow_too() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let mut plain_objects = build_dark_scene();
    plain_objects.insert(3, label(BLUE, 0.0));
    let plain_beside = pixel_at(&capture(&mut renderer, &plain_objects).await, PIXEL_BESIDE);
    let mut glowing_objects = build_dark_scene();
    glowing_objects.insert(3, label(BLUE, 1.0));
    let glowing_beside = pixel_at(&capture(&mut renderer, &glowing_objects).await, PIXEL_BESIDE);
    assert!(
        glowing_beside[2] > plain_beside[2] + 0.01 && glowing_beside[2] > 2.0 * glowing_beside[0],
        "the wall beside a glowing blue label {glowing_beside:?} should catch its blue glow, unlike beside a plain one {plain_beside:?}"
    );
    Ok(())
}

// The sun's glow comes from bloom too, so it spills over the edge of whatever hides part of the sun, the way the sun peeks
// around a roof. The sky used to paint a ring around the sun that stopped dead at that edge.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn the_suns_glow_spills_over_the_edge_of_what_hides_it() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    // The sun sits just above the middle of the picture, and the top of a dark wall runs just below it. The wall's face
    // looks away from the sun, so no sunlight reaches it either way.
    let sun_elevation_degrees: f32 = 6.0;
    let wall_top_elevation_degrees: f32 = 2.0;
    let wall_top = CAMERA_POSITION.y + 9.5 * wall_top_elevation_degrees.to_radians().tan();
    let build_scene = |sun_intensity: f32| {
        HashMap::from([
            (1, brick(Vec3::new(0.0, wall_top - 20.0, -10.0), Vec3::new(60.0, 40.0, 1.0), Color3::WHITE, 0.0)),
            (
                2,
                RenderObject::WorldLighting(WorldLightingInfo {
                    sunOrientation: game_types::object_types::Orientation3D {
                        theta: std::f32::consts::PI,
                        phi: -sun_elevation_degrees.to_radians(),
                    },
                    sunIntensity: sun_intensity,
                    ambientTopColor: Color3::WHITE,
                    ambientBottomColor: Color3::WHITE,
                    ambientIntensity: 0.05,
                    ambientOcclusionIntensity: 0.0,
                    skyReflectionStrength: 0.0,
                    vibrance: 0.0,
                    ..WorldLightingInfo::default()
                }),
            ),
        ])
    };
    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, CAMERA_POSITION + Vec3::NEG_Z, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let wall_just_below_the_sun = view_proj.project_point3(Vec3::new(0.0, wall_top - 0.15, -9.5));
    let pixel = (
        ((wall_just_below_the_sun.x * 0.5 + 0.5) * SIZE as f32) as u32,
        ((0.5 - wall_just_below_the_sun.y * 0.5) * SIZE as f32) as u32,
    );
    let without_sun = pixel_at(&capture(&mut renderer, &build_scene(0.0)).await, pixel);
    let with_sun = pixel_at(&capture(&mut renderer, &build_scene(1.0)).await, pixel);
    assert!(
        with_sun[0] > without_sun[0] + 0.01,
        "the wall just below the sun {with_sun:?} should catch its glow, unlike with no sun {without_sun:?}"
    );
    Ok(())
}
