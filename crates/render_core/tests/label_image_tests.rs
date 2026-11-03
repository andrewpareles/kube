#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test label_image_tests -- --ignored

use game_types::{
    data_types::DataId,
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
const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 2.0, -1.0);
const QUARTERS_TEXTURE_DATA_ID: DataId = 31;
const DARK_BLUE_TEXTURE_DATA_ID: DataId = 32;

// Red and green on top, blue and white below.
fn quarters_texture() -> (DataId, Vec<u8>, u32, u32) {
    let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
    (QUARTERS_TEXTURE_DATA_ID, rgba, 2, 2)
}

fn dark_blue_texture() -> (DataId, Vec<u8>, u32, u32) {
    (DARK_BLUE_TEXTURE_DATA_ID, vec![20, 30, 120, 255], 1, 1)
}

// Flat against the black wall, facing the camera 10 units away, its middle level with the camera.
fn picture_label(texture_data_id: DataId, width: f32, height: f32) -> RenderObject {
    RenderObject::RenderLabel(RenderLabel {
        physicalObjectId: 1,
        offsetFromObject: Vec3::new(0.0, 0.0, 2.0),
        rotationFromObject: Quat::IDENTITY,
        billboardMode: BillboardMode::None,
        isConstantSize: false,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
        content: RenderLabelContent::Image {
            textureDataId: texture_data_id,
            width,
            height,
        },
    })
}

fn white_text_label(text: &str, line_height: f32, offset_from_object: Vec3) -> RenderObject {
    let label_info = LabelInfo {
        text: text.to_string(),
        lineHeight: line_height,
        maxWidth: None,
        horizontalAlign: LabelHorizontalAlign::Center,
        backgroundColor: None,
        backgroundOpacity: 0.0,
        offsetFromObject: offset_from_object,
        rotationFromObject: Quat::IDENTITY,
        physicalObjectId: None,
        billboardMode: BillboardMode::None,
        isConstantSize: false,
        visualSettings: VisualSettings {
            color: Color3::WHITE,
            ..VisualSettings::TYPICAL_OPAQUE
        },
    };
    let [label_mesh] = label_meshes_of_label_infos(std::slice::from_ref(&label_info)).try_into().ok().unwrap();
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
    })
}

// The labels are drawn in front of a black wall 12 units away with no light on it.
async fn capture(labels: Vec<RenderObject>, textures: &[(DataId, Vec<u8>, u32, u32)]) -> anyhow::Result<Vec<u8>> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)?;
    for (data_id, rgba, width, height) in textures {
        renderer.load_texture_into_gpu(*data_id, &[rgba.clone()], *width, *height, wgpu::TextureFormat::Rgba8Unorm, true)?;
    }

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
    for (index, label) in labels.into_iter().enumerate() {
        objects.insert(3 + index, label);
    }
    let scene = build_render_scene(&objects, &HashMap::new(), None);

    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, CAMERA_TARGET, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(CAMERA_POSITION, CAMERA_TARGET, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, &scene, None, &camera, CAMERA_TARGET, CAMERA_POSITION, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Label image readback"),
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

fn pixel_at(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let start = (y * SIZE as usize + x) * 4;
    [pixels[start], pixels[start + 1], pixels[start + 2]]
}

// At 10 units away, one unit is about 36 pixels.
const PIXELS_PER_UNIT: usize = 36;
const MIDDLE: usize = SIZE as usize / 2;

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_label_image_shows_its_picture_upright_at_its_size() -> anyhow::Result<()> {
    let pixels = capture(vec![picture_label(QUARTERS_TEXTURE_DATA_ID, 4.0, 4.0)], &[quarters_texture()]).await?;
    let quarters = [
        pixel_at(&pixels, MIDDLE - PIXELS_PER_UNIT, MIDDLE - PIXELS_PER_UNIT),
        pixel_at(&pixels, MIDDLE + PIXELS_PER_UNIT, MIDDLE - PIXELS_PER_UNIT),
        pixel_at(&pixels, MIDDLE - PIXELS_PER_UNIT, MIDDLE + PIXELS_PER_UNIT),
        pixel_at(&pixels, MIDDLE + PIXELS_PER_UNIT, MIDDLE + PIXELS_PER_UNIT),
    ];
    let [top_left, top_right, bottom_left, bottom_right] = quarters;
    assert!(
        top_left[0] > 200 && top_left[1] < 50 && top_left[2] < 50,
        "top left should be red: {quarters:?}"
    );
    assert!(
        top_right[1] > 200 && top_right[0] < 50 && top_right[2] < 50,
        "top right should be green: {quarters:?}"
    );
    assert!(
        bottom_left[2] > 200 && bottom_left[0] < 50 && bottom_left[1] < 50,
        "bottom left should be blue: {quarters:?}"
    );
    assert!(bottom_right.iter().all(|channel| *channel > 200), "bottom right should be white: {quarters:?}");
    // Just inside the top edge is still red, not blended with the blue row at the bottom.
    let near_top_edge = pixel_at(&pixels, MIDDLE - PIXELS_PER_UNIT, MIDDLE - 2 * PIXELS_PER_UNIT + 2);
    assert!(
        near_top_edge[0] > 200 && near_top_edge[2] < 50,
        "the top edge should not pick up the bottom row: {near_top_edge:?}"
    );
    // Three units out from the middle is past the picture's 2 unit half width, on the dark wall.
    let outside = pixel_at(&pixels, MIDDLE - 3 * PIXELS_PER_UNIT, MIDDLE);
    assert!(
        outside.iter().all(|channel| *channel < 30),
        "outside the picture should be the wall: {outside:?}"
    );
    Ok(())
}

// Until its picture has downloaded, an image draws nothing rather than a blank rectangle.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_label_image_draws_nothing_before_its_picture_loads() -> anyhow::Result<()> {
    let pixels = capture(vec![picture_label(QUARTERS_TEXTURE_DATA_ID, 4.0, 4.0)], &[]).await?;
    let center = pixel_at(&pixels, MIDDLE, MIDDLE);
    assert!(
        center.iter().all(|channel| *channel < 30),
        "the wall should show where the picture will be: {center:?}"
    );
    Ok(())
}

// Text a hair in front of a picture but below its middle has its middle a little farther from the camera than the
// picture's, so it is drawn first. The depth it writes keeps the picture from covering it.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn text_just_in_front_of_a_label_image_stays_on_top() -> anyhow::Result<()> {
    let picture = picture_label(DARK_BLUE_TEXTURE_DATA_ID, 6.0, 4.0);
    let text = white_text_label("HHHH", 1.0, Vec3::new(0.0, -1.0, 2.02));
    let pixels = capture(vec![picture, text], &[dark_blue_texture()]).await?;
    let text_row = MIDDLE + PIXELS_PER_UNIT;
    let white_pixel_count = (MIDDLE - 2 * PIXELS_PER_UNIT..MIDDLE + 2 * PIXELS_PER_UNIT)
        .filter(|x| pixel_at(&pixels, *x, text_row).iter().all(|channel| *channel > 200))
        .count();
    assert!(
        white_pixel_count > 10,
        "the text should show over the picture, but {white_pixel_count} pixels are white"
    );
    let picture_above_text = pixel_at(&pixels, MIDDLE, MIDDLE - PIXELS_PER_UNIT);
    assert!(
        picture_above_text[2] > 60 && picture_above_text[0] < 40,
        "the picture should still show above the text: {picture_above_text:?}"
    );
    Ok(())
}
