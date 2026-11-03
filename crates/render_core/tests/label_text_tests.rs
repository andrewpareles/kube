#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test label_text_tests -- --ignored

use game_types::{
    object_types::{BillboardMode, LabelHorizontalAlign, LabelInfo, PhysicsInfo3D, VisualSettings, WorldLightingInfo, shared::Color3},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    label_text_mesh::label_meshes_of_label_infos,
    render_scene::build_render_scene,
    renderer::Renderer,
    renderer_types::{RenderBrick, RenderLabel, RenderLabelContent, RenderObject, RenderTransform},
};
use std::collections::HashMap;

const SIZE: u32 = 512;
const CAMERA_POSITION: Vec3 = Vec3::new(0.0, 2.0, 0.0);

fn white_label(text: &str, line_height: f32) -> LabelInfo {
    LabelInfo {
        text: text.to_string(),
        lineHeight: line_height,
        maxWidth: None,
        horizontalAlign: LabelHorizontalAlign::Center,
        backgroundColor: None,
        backgroundOpacity: 0.0,
        offsetFromObject: Vec3::new(0.0, 0.0, 2.0),
        rotationFromObject: Quat::IDENTITY,
        physicalObjectId: None,
        billboardMode: BillboardMode::None,
        isConstantSize: false,
        visualSettings: VisualSettings {
            color: Color3::WHITE,
            ..VisualSettings::TYPICAL_OPAQUE
        },
    }
}

// White text standing 10 units in front of the camera, against a black wall with no light on it. Every pixel is text,
// wall, or the edge between them.
async fn capture_text(text: &str, line_height: f32) -> anyhow::Result<Vec<u8>> {
    capture_labels(&[white_label(text, line_height)], CAMERA_POSITION, Vec3::new(0.0, 2.0, -1.0)).await
}

async fn capture_labels(label_infos: &[LabelInfo], camera_position: Vec3, camera_target: Vec3) -> anyhow::Result<Vec<u8>> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)?;

    let label_meshes = label_meshes_of_label_infos(label_infos);
    let mut objects = HashMap::from([
        (
            1,
            RenderObject::Brick(RenderBrick {
                transform: RenderTransform {
                    physicsInfo3D: PhysicsInfo3D::new(Vec3::new(0.0, 2.0, -12.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                    teleport_count: 0,
                    rigid_group_to_follow: None,
                    sk_joint_to_follow_arc: None,
                },
                visualSettings: VisualSettings {
                    color: Color3 { r: 0.0, g: 0.0, b: 0.0 },
                    ..VisualSettings::TYPICAL_OPAQUE
                },
                primitive: BrickPrimitiveSettings {
                    size: Vec3::new(60.0, 60.0, 1.0),
                },
                material: None,
            }),
        ),
        (
            2,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunIntensity: 0.0,
                ambientIntensity: 0.0,
                ambientOcclusionIntensity: 0.0,
                skyReflectionStrength: 0.0,
                ..WorldLightingInfo::default()
            }),
        ),
    ]);
    for (index, (label_info, label_mesh)) in label_infos.iter().zip(label_meshes).enumerate() {
        objects.insert(
            3 + index,
            RenderObject::RenderLabel(RenderLabel {
                physicalObjectId: 1,
                offsetFromObject: label_info.offsetFromObject,
                rotationFromObject: label_info.rotationFromObject,
                billboardMode: label_info.billboardMode,
                isConstantSize: label_info.isConstantSize,
                visualSettings: label_info.visualSettings.clone(),
                content: RenderLabelContent::Text {
                    scale: label_mesh.scale_per_point,
                    backgroundColor: label_info.backgroundColor,
                    backgroundOpacity: label_info.backgroundOpacity,
                    geometry: label_mesh.geometry,
                    atlas: label_mesh.atlas,
                },
            }),
        );
    }
    let scene = build_render_scene(&objects, &HashMap::new(), None);

    let view_proj = build_camera_view_proj_matrix(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, &scene, None, &camera, camera_target, camera_position, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Label text readback"),
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
    Ok(pixels)
}

// The red channel of each pixel across the middle row of the picture.
fn middle_row(pixels: &[u8]) -> Vec<u8> {
    let row_start = (SIZE as usize / 2) * SIZE as usize * 4;
    (0..SIZE as usize).map(|x| pixels[row_start + x * 4]).collect()
}

// Letters drawn many times bigger than the atlas holds them still have an edge one pixel wide, the same as small ones.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_letter_drawn_huge_keeps_a_sharp_edge() -> anyhow::Result<()> {
    // At 10 units away, a line 8 units tall is about 290 pixels, several times the atlas's letter size.
    let row = middle_row(&capture_text("I", 8.0).await?);
    let text_pixel_count = row.iter().filter(|&&value| value > 230).count();
    let edge_pixel_count = row.iter().filter(|&&value| (25..=230).contains(&value)).count();
    assert!(text_pixel_count >= 10, "the letter should cross the middle of the picture: {row:?}");
    assert!(
        edge_pixel_count <= 4,
        "the letter's two edges should each be about a pixel wide, but {edge_pixel_count} pixels are partly lit: {row:?}"
    );
    Ok(())
}

// Text a few pixels tall still shows up, softened rather than lost between texels.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn small_text_stays_visible() -> anyhow::Result<()> {
    // At 10 units away, a line 0.25 units tall is about 9 pixels.
    let pixels = capture_text("III", 0.25).await?;
    let brightest = pixels.chunks(4).map(|pixel| pixel[0]).max().unwrap_or(0);
    assert!(brightest > 150, "small text should still show, but the brightest pixel is {brightest}");
    Ok(())
}

// A label just behind another one stays behind it, even when the camera is closer to its center than to the front one's.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_label_just_behind_another_stays_behind_it() -> anyhow::Result<()> {
    let behind = LabelInfo {
        offsetFromObject: Vec3::new(0.15, 0.0, 1.98),
        visualSettings: VisualSettings {
            color: Color3 { r: 1.0, g: 0.0, b: 0.0 },
            ..VisualSettings::TYPICAL_OPAQUE
        },
        ..white_label("I", 8.0)
    };
    let pixels = capture_labels(&[white_label("I", 8.0), behind], Vec3::new(9.0, 2.0, 0.0), Vec3::new(0.0, 2.0, -10.0)).await?;
    let center = ((SIZE as usize / 2) * SIZE as usize + SIZE as usize / 2) * 4;
    let center_pixel = &pixels[center..center + 4];
    assert!(
        center_pixel[1] > 230,
        "the white label in front should cover the red one behind it, but the middle pixel is {center_pixel:?}"
    );
    Ok(())
}
