#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test display_color_tests -- --ignored

use game_types::{
    object_types::{BrickMaterialSettings, MaterialInfo, PhysicsInfo3D, VisualSettings, material::AlphaMode, shared::Color3},
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

const SIZE: u32 = 64;

// -------------------------- One brick filling the view --------------------------

// A brick wide enough to cover the whole image, with its front face looking straight at the camera.
fn build_brick_scene(color: Color3, material: Option<MaterialInfo>) -> RenderScene {
    let mut objects = HashMap::from([(
        1,
        RenderObject::Brick(RenderBrick {
            transform: RenderTransform {
                physicsInfo3D: PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                teleport_count: 0,
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings {
                opacity: 1.0,
                color,
                glow: 0.0,
                isAlwaysOnTop: false,
            },
            primitive: BrickPrimitiveSettings {
                size: Vec3::new(4.0, 4.0, 1.0),
            },
            material: material.is_some().then(|| BrickMaterialSettings {
                materialInfoId: 2.into(),
                tileSizeU: 1.0,
                tileSizeV: 1.0,
                tileOffsetU: 0.0,
                tileOffsetV: 0.0,
            }),
        }),
    )]);
    if let Some(material) = material {
        objects.insert(2, RenderObject::Material(material));
    }
    build_render_scene(&objects, &HashMap::new(), None)
}

async fn capture_center_pixel(renderer: &mut Renderer, scene: &RenderScene) -> [u8; 4] {
    let camera_position = Vec3::new(0.0, 0.0, 3.0);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(
        &mut frame,
        scene,
        None,
        &camera,
        Vec3::ZERO,
        camera_position,
        build_camera_view_proj_matrix(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES),
    );
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Display color readback"),
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
    let center = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let pixel = buffer.slice(..).get_mapped_range()[center..center + 4].try_into().unwrap();
    buffer.unmap();
    pixel
}

async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)
}

// -------------------------- Colors and brightness --------------------------

// A color picked as a hex and a texture holding that same hex must look identical. Flat colors used to reach the
// shader without their sRGB decode, which rendered them far paler than a texture of the same color.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn flat_color_renders_the_same_as_a_texture_of_the_same_hex() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    renderer.load_texture_into_gpu(7, &[vec![128, 128, 128, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, true)?;
    let hex_128 = 128.0 / 255.0;
    let grey_128 = Color3 {
        r: hex_128,
        g: hex_128,
        b: hex_128,
    };
    let flat_pixel = capture_center_pixel(&mut renderer, &build_brick_scene(grey_128, None)).await;
    let textured_pixel = capture_center_pixel(
        &mut renderer,
        &build_brick_scene(
            Color3::WHITE,
            Some(MaterialInfo {
                albedoTextureDataId: Some(7),
                metallicTextureDataId: None,
                roughnessTextureDataId: None,
                normalTextureDataId: None,
                alphaMode: AlphaMode::Opaque,
                isDoubleSided: false,
            }),
        ),
    )
    .await;
    for channel in 0..3 {
        assert!(
            flat_pixel[channel].abs_diff(textured_pixel[channel]) <= 1,
            "flat color {flat_pixel:?} does not match the texture of the same hex {textured_pixel:?}"
        );
    }
    Ok(())
}

// Light brighter than the screen can show has to keep reading brighter, not stop at white. Before the tone
// curve, both of these suns clipped a white surface to exactly 255.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn brighter_sun_keeps_reading_brighter_instead_of_clipping_to_white() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let mut scene = build_brick_scene(Color3::WHITE, None);
    scene.lighting.sun_direction = [0.0, 0.0, 1.0, 0.0];
    scene.lighting.ambient_top_color_and_intensity = [0.0; 4];
    scene.lighting.ambient_bottom_color_and_padding = [0.0; 4];
    scene.lighting.sun_color_and_intensity = [1.0, 1.0, 1.0, 3.0];
    let bright_pixel = capture_center_pixel(&mut renderer, &scene).await;
    scene.lighting.sun_color_and_intensity = [1.0, 1.0, 1.0, 12.0];
    let brighter_pixel = capture_center_pixel(&mut renderer, &scene).await;
    for channel in 0..3 {
        assert!(
            brighter_pixel[channel].saturating_sub(bright_pixel[channel]) > 3,
            "a four times brighter sun {brighter_pixel:?} should read clearly brighter than {bright_pixel:?}"
        );
    }
    Ok(())
}

// Vibrance pushes a dull color further from gray, leaves gray alone, and barely changes a color that is already vivid.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn vibrance_livens_dull_colors_and_barely_changes_vivid_ones() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let linear = |value: u8| ((value as f32 / 255.0 + 0.055) / 1.055).powf(2.4);
    let colorfulness = |pixel: [u8; 4]| {
        let channels = [linear(pixel[0]), linear(pixel[1]), linear(pixel[2])];
        channels.iter().copied().fold(f32::MIN, f32::max) - channels.iter().copied().fold(f32::MAX, f32::min)
    };
    let dull = Color3 { r: 0.55, g: 0.57, b: 0.62 };
    let vivid = Color3 { r: 0.9, g: 0.1, b: 0.1 };
    let gray = Color3 { r: 0.5, g: 0.5, b: 0.5 };
    let mut pixels_of_color = Vec::new();
    for color in [dull, vivid, gray] {
        let mut scene = build_brick_scene(color, None);
        scene.lighting.display_settings = [0.0; 4];
        let without_vibrance = capture_center_pixel(&mut renderer, &scene).await;
        scene.lighting.display_settings = [0.5, 0.0, 0.0, 0.0];
        let with_vibrance = capture_center_pixel(&mut renderer, &scene).await;
        pixels_of_color.push((without_vibrance, with_vibrance));
    }
    let [(dull_without, dull_with), (vivid_without, vivid_with), (gray_without, gray_with)] = pixels_of_color[..] else {
        unreachable!()
    };
    assert!(
        colorfulness(dull_with) > 1.25 * colorfulness(dull_without),
        "vibrance should push a dull color further from gray: {dull_without:?} -> {dull_with:?}"
    );
    assert!(
        colorfulness(vivid_with) < 1.15 * colorfulness(vivid_without),
        "vibrance should barely change a vivid color: {vivid_without:?} -> {vivid_with:?}"
    );
    for channel in 0..3 {
        assert!(
            gray_with[channel].abs_diff(gray_without[channel]) <= 1,
            "vibrance should leave gray alone: {gray_without:?} -> {gray_with:?}"
        );
    }
    Ok(())
}
