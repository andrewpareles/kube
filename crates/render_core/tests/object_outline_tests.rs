#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test object_outline_tests -- --ignored

use game_types::{
    object_types::{
        PhysicsInfo3D, VisualSettings, WorldLightingInfo,
        shared::{Color3, OutlineSettings},
    },
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    render_scene::build_render_scene,
    renderer::Renderer,
    renderer_types::{RenderBrick, RenderObject, RenderTransform},
};
use std::collections::HashMap;

const SIZE: u32 = 256;
const CAMERA_POSITION: Vec3 = Vec3::new(0.0, 0.0, 0.0);
const GREEN: Color3 = Color3 { r: 0.0, g: 1.0, b: 0.0 };
const RED: Color3 = Color3 { r: 1.0, g: 0.0, b: 0.0 };
async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)
}

fn brick(position: Vec3, size: Vec3, visual_settings: VisualSettings) -> RenderObject {
    RenderObject::Brick(RenderBrick {
        transform: RenderTransform {
            physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            teleport_count: 0,
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        },
        visualSettings: visual_settings,
        primitive: BrickPrimitiveSettings { size },
        material: None,
    })
}

fn outline(color: Color3, opacity: f32, thickness: f32) -> Option<OutlineSettings> {
    Some(OutlineSettings { color, opacity, thickness })
}

// A white brick in the middle of the picture, ten units ahead, with nothing behind it but the sky.
fn outlined_brick(opacity: f32, outline: Option<OutlineSettings>) -> RenderObject {
    brick(
        Vec3::new(0.0, 0.0, -10.0),
        Vec3::ONE,
        VisualSettings {
            opacity,
            color: Color3::WHITE,
            outline,
            ..VisualSettings::TYPICAL_OPAQUE
        },
    )
}

// Flat lighting, so colors read back about as they were authored.
fn lighting() -> RenderObject {
    RenderObject::WorldLighting(WorldLightingInfo {
        sunIntensity: 0.0,
        ambientTopColor: Color3::WHITE,
        ambientBottomColor: Color3::WHITE,
        ambientIntensity: 1.0,
        ambientOcclusionIntensity: 0.0,
        skyReflectionStrength: 0.0,
        ..WorldLightingInfo::default()
    })
}

// Every pixel of the picture, in linear light, row by row.
async fn capture(renderer: &mut Renderer, objects: &HashMap<usize, RenderObject>) -> Vec<[f32; 3]> {
    let camera_target = CAMERA_POSITION + Vec3::NEG_Z;
    let mut scene = build_render_scene(objects, &HashMap::new(), None);
    scene.should_draw_sky = false;
    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(CAMERA_POSITION, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, &scene, None, &camera, camera_target, CAMERA_POSITION, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Outline readback"),
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

// The pixels along the row through the middle of the picture, from the middle out to the right.
fn middle_row_to_the_right(pixels: &[[f32; 3]]) -> Vec<[f32; 3]> {
    (SIZE / 2..SIZE).map(|x| pixels[((SIZE / 2) * SIZE + x) as usize]).collect()
}

// How many pixels along the middle row are mostly green. The brick is white and the background black, so only outline counts.
fn green_pixels_beside_brick(pixels: &[[f32; 3]]) -> usize {
    middle_row_to_the_right(pixels)
        .iter()
        .filter(|pixel| pixel[1] > 0.3 && pixel[1] > 2.0 * pixel[0])
        .count()
}

// Two pixels past the right edge of the brick, inside any outline at least three pixels thick.
fn pixel_index_beside_brick() -> usize {
    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, CAMERA_POSITION + Vec3::NEG_Z, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let edge_ndc = view_proj.project_point3(Vec3::new(0.5, 0.0, -10.0)).x;
    (edge_ndc * SIZE as f32 / 2.0).ceil() as usize + 1
}

// An outline draws a band of its color just outside the object, as wide as its thickness.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn an_outline_draws_a_band_of_its_color_around_the_object() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let plain = capture(&mut renderer, &HashMap::from([(1, outlined_brick(1.0, None)), (2, lighting())])).await;
    let thin = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 2.0))), (2, lighting())]),
    )
    .await;
    let thick = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 6.0))), (2, lighting())]),
    )
    .await;
    assert_eq!(
        green_pixels_beside_brick(&plain),
        0,
        "a brick with no outline should have nothing green beside it"
    );
    let thin_width = green_pixels_beside_brick(&thin);
    let thick_width = green_pixels_beside_brick(&thick);
    assert!((2..=4).contains(&thin_width), "a 2 pixel outline should be about 2 pixels wide: {thin_width}");
    assert!((6..=8).contains(&thick_width), "a 6 pixel outline should be about 6 pixels wide: {thick_width}");
    let middle = middle_row_to_the_right(&thick)[0];
    assert!(middle[0] > 0.5 && middle[2] > 0.5, "the outline should not cover the brick itself: {middle:?}");
    Ok(())
}

// The outer edge of an outline fades over a pixel instead of stopping dead, so it is smooth.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn an_outline_with_a_fractional_thickness_has_a_soft_outer_edge() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let pixels = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 2.5))), (2, lighting())]),
    )
    .await;
    let row = middle_row_to_the_right(&pixels);
    let has_partial_pixel = row.iter().any(|pixel| pixel[1] > 0.05 && pixel[1] < 0.8 && pixel[0] < 0.05);
    assert!(has_partial_pixel, "a 2.5 pixel outline should end in a partly green pixel: {row:?}");
    Ok(())
}

// A fully see-through object still draws its outline, which makes a ghost outline.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn an_invisible_object_still_draws_its_outline() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let pixels = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(0.0, outline(GREEN, 1.0, 3.0))), (2, lighting())]),
    )
    .await;
    let width = green_pixels_beside_brick(&pixels);
    assert!((3..=5).contains(&width), "an invisible brick's 3 pixel outline should still show: {width}");
    let middle = middle_row_to_the_right(&pixels)[0];
    assert!(
        middle.iter().all(|channel| *channel < 0.05),
        "the invisible brick itself should not show: {middle:?}"
    );
    Ok(())
}

// A semi-transparent outline blends with what is behind it, and a semi-transparent object's outline still shows.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn see_through_outlines_and_outlines_of_see_through_objects_blend() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let red_wall = brick(
        Vec3::new(0.0, 0.0, -20.0),
        Vec3::new(40.0, 40.0, 1.0),
        VisualSettings {
            color: RED,
            ..VisualSettings::TYPICAL_OPAQUE
        },
    );
    let half_outline = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 0.5, 4.0))), (2, lighting()), (3, red_wall.clone())]),
    )
    .await;
    let beside = middle_row_to_the_right(&half_outline)[pixel_index_beside_brick()];
    assert!(
        beside[0] > 0.15 && beside[1] > 0.15,
        "a half see-through green outline over a red wall should show both: {beside:?}"
    );
    let glass_brick = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(0.5, outline(GREEN, 1.0, 4.0))), (2, lighting()), (3, red_wall)]),
    )
    .await;
    let beside = middle_row_to_the_right(&glass_brick)[pixel_index_beside_brick()];
    assert!(
        beside[1] > 0.5 && beside[0] < 0.2,
        "a see-through brick's solid outline should cover the wall: {beside:?}"
    );
    Ok(())
}

// A wall in front of an outlined object hides its outline, and glass in front tints it.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn what_is_in_front_of_an_outline_hides_or_tints_it() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let front = |opacity: f32| {
        brick(
            Vec3::new(0.0, 0.0, -5.0),
            Vec3::new(10.0, 10.0, 0.1),
            VisualSettings {
                opacity,
                color: RED,
                ..VisualSettings::TYPICAL_OPAQUE
            },
        )
    };
    let behind_wall = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 4.0))), (2, lighting()), (3, front(1.0))]),
    )
    .await;
    assert!(
        middle_row_to_the_right(&behind_wall).iter().all(|pixel| pixel[1] < 0.05),
        "a wall should hide the outline behind it"
    );
    let behind_glass = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 4.0))), (2, lighting()), (3, front(0.5))]),
    )
    .await;
    let beside = middle_row_to_the_right(&behind_glass)[pixel_index_beside_brick()];
    assert!(
        beside[1] > 0.1 && beside[0] > 0.1,
        "red glass in front should tint the green outline: {beside:?}"
    );
    Ok(())
}

// An outlined object in front of another outlined object draws its outline across the one behind it, but an object
// never outlines itself.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn the_nearer_of_two_outlined_objects_outlines_itself_over_the_farther() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let big_back = brick(
        Vec3::new(0.0, 0.0, -20.0),
        Vec3::new(40.0, 40.0, 1.0),
        VisualSettings {
            color: RED,
            outline: outline(Color3 { r: 0.0, g: 0.0, b: 1.0 }, 1.0, 4.0),
            ..VisualSettings::TYPICAL_OPAQUE
        },
    );
    let pixels = capture(
        &mut renderer,
        &HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 4.0))), (2, lighting()), (3, big_back)]),
    )
    .await;
    let width = green_pixels_beside_brick(&pixels);
    assert!((4..=6).contains(&width), "the near brick's outline should show over the far one: {width}");
    let row = middle_row_to_the_right(&pixels);
    let far = row[row.len() / 2 + 20];
    assert!(far[0] > 0.3 && far[2] < 0.05, "the far brick's own face should have no outline on it: {far:?}");
    Ok(())
}

// Thickness is in screen pixels, so on a screen with two render pixels per screen pixel an outline covers twice as many.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn outlines_keep_their_screen_thickness_on_high_density_screens() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let objects = HashMap::from([(1, outlined_brick(1.0, outline(GREEN, 1.0, 3.0))), (2, lighting())]);
    let normal_width = green_pixels_beside_brick(&capture(&mut renderer, &objects).await);
    renderer.set_device_pixel_ratio(2.0);
    let dense_width = green_pixels_beside_brick(&capture(&mut renderer, &objects).await);
    assert!(
        (5..=8).contains(&dense_width) && dense_width > normal_width,
        "a 3 pixel outline at twice the pixel density should be about 6 render pixels wide: {dense_width} vs {normal_width}"
    );
    Ok(())
}
