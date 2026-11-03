#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test sky_reflection_tests -- --ignored

use game_types::{
    object_types::{BrickMaterialSettings, MaterialInfo, PhysicsInfo3D, VisualSettings, WorldLightingInfo, material::AlphaMode, shared::Color3},
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
const CAMERA_POSITION: Vec3 = Vec3::new(0.0, 3.0, 3.0);
const METAL_TEXTURE: i64 = 31;
const SMOOTH_TEXTURE: i64 = 32;
const SKYBOX_TEXTURE: i64 = 33;
const ROUGH_TEXTURE: i64 = 34;
const RED_SKY: Color3 = Color3 { r: 0.9, g: 0.3, b: 0.2 };
const BLUE_SKY: Color3 = Color3 { r: 0.2, g: 0.4, b: 0.9 };

async fn create_renderer() -> anyhow::Result<Renderer> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, SIZE, SIZE)?;
    renderer.load_texture_into_gpu(METAL_TEXTURE, &[vec![0, 0, 255, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, false)?;
    renderer.load_texture_into_gpu(SMOOTH_TEXTURE, &[vec![0, 0, 0, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, false)?;
    renderer.load_texture_into_gpu(SKYBOX_TEXTURE, &[vec![40, 200, 60, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, true)?;
    renderer.load_texture_into_gpu(ROUGH_TEXTURE, &[vec![0, 204, 0, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, false)?;
    Ok(renderer)
}

// What the floor is made of. A plain floor is a brick with no material, which renders middling rough and not metal. The
// others are white: smooth metal, rough and not metal, or rough metal.
#[derive(Clone, Copy)]
enum Floor {
    Plain,
    Mirror,
    RoughPlain,
    RoughMetal,
}

type RenderObjects = HashMap<usize, RenderObject>;

// A floor lit by nothing but the sky it reflects, under a sky of one color.
fn build_floor_scene(floor: Floor, sky_color: Color3, skybox_opacity: Option<f32>, sky_reflection_strength: f32) -> RenderObjects {
    let world_lighting_info = WorldLightingInfo {
        sunIntensity: 0.0,
        ambientIntensity: 0.0,
        ambientOcclusionIntensity: 0.0,
        skyTopColor: sky_color,
        skyHorizonColor: sky_color,
        skyBottomColor: sky_color,
        skyboxTextureDataId: skybox_opacity.map(|_| SKYBOX_TEXTURE),
        skyboxTextureOpacity: skybox_opacity.unwrap_or(1.0),
        skyReflectionStrength: sky_reflection_strength,
        ..WorldLightingInfo::default()
    };
    let floor_textures = match floor {
        Floor::Plain => None,
        Floor::Mirror => Some((Some(METAL_TEXTURE), SMOOTH_TEXTURE)),
        Floor::RoughPlain => Some((None, ROUGH_TEXTURE)),
        Floor::RoughMetal => Some((Some(METAL_TEXTURE), ROUGH_TEXTURE)),
    };
    let floor_material = floor_textures.map(|_| BrickMaterialSettings {
        materialInfoId: 2.into(),
        tileSizeU: 1.0,
        tileSizeV: 1.0,
        tileOffsetU: 0.0,
        tileOffsetV: 0.0,
    });
    let mut objects = HashMap::from([
        (
            1,
            RenderObject::Brick(RenderBrick {
                transform: RenderTransform {
                    physicsInfo3D: PhysicsInfo3D::new(Vec3::new(0.0, -0.5, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                    teleport_count: 0,
                    rigid_group_to_follow: None,
                    sk_joint_to_follow_arc: None,
                },
                visualSettings: VisualSettings {
                    color: Color3::WHITE,
                    ..VisualSettings::TYPICAL_OPAQUE
                },
                primitive: BrickPrimitiveSettings {
                    size: Vec3::new(10.0, 1.0, 10.0),
                },
                material: floor_material,
            }),
        ),
        (3, RenderObject::WorldLighting(world_lighting_info)),
    ]);
    if let Some((metallic_texture_data_id, roughness_texture_data_id)) = floor_textures {
        objects.insert(
            2,
            RenderObject::Material(MaterialInfo {
                albedoTextureDataId: None,
                metallicTextureDataId: metallic_texture_data_id,
                roughnessTextureDataId: Some(roughness_texture_data_id),
                normalTextureDataId: None,
                alphaMode: AlphaMode::Opaque,
                isDoubleSided: false,
            }),
        );
    }
    objects
}

// The floor right under the camera's gaze, and the sky along the top edge of the picture, in linear light.
async fn capture_floor_and_sky(renderer: &mut Renderer, objects: &RenderObjects) -> ([f32; 3], [f32; 3]) {
    let camera_target = Vec3::ZERO;
    let scene = build_render_scene(objects, &HashMap::new(), None);
    let view_proj = build_camera_view_proj_matrix(CAMERA_POSITION, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera) = renderer.create_camera_matrix_buffer_pair(CAMERA_POSITION, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(renderer, SIZE, SIZE);
    renderer.render_scene(&mut frame, &scene, None, &camera, camera_target, CAMERA_POSITION, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Sky reflection readback"),
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
    let linear_pixel = |x: u32, y: u32| {
        let index = ((y * SIZE + x) * 4) as usize;
        std::array::from_fn(|channel| ((pixels[index + channel] as f32 / 255.0 + 0.055) / 1.055).powf(2.4))
    };
    (linear_pixel(SIZE / 2, SIZE / 2), linear_pixel(SIZE / 2, 1))
}

fn is_near(left: [f32; 3], right: [f32; 3], tolerance: f32) -> bool {
    (0..3).all(|channel| (left[channel] - right[channel]).abs() <= tolerance * right[channel].max(0.02))
}

fn brightness(color: [f32; 3]) -> f32 {
    color.iter().sum()
}

// With no other light, smooth metal shows the sky it faces, and changes with it.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn smooth_metal_reflects_the_sky() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    for sky_color in [RED_SKY, BLUE_SKY] {
        let (floor, sky) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::Mirror, sky_color, None, 1.0)).await;
        assert!(is_near(floor, sky, 0.1), "the mirror floor {floor:?} should show the sky {sky:?}");
    }
    Ok(())
}

// A rough surface that is not metal still catches a little of the sky, but far less than a mirror does.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn rough_surfaces_reflect_only_a_little_sky() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let (mirror_floor, _) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::Mirror, BLUE_SKY, None, 1.0)).await;
    let (rough_floor, _) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::Plain, BLUE_SKY, None, 1.0)).await;
    assert!(brightness(rough_floor) > 0.0, "a rough floor should still catch some sky: {rough_floor:?}");
    assert!(
        brightness(mirror_floor) > 5.0 * brightness(rough_floor),
        "the rough floor {rough_floor:?} should reflect far less than the mirror {mirror_floor:?}"
    );
    Ok(())
}

// Only smooth surfaces show the sky. A rough surface that is not metal reflects none of it, while rough metal, which gets
// all its color from what it reflects, still does.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn rough_plain_surfaces_reflect_no_sky_but_rough_metal_does() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let (rough_plain_floor, _) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::RoughPlain, BLUE_SKY, None, 1.0)).await;
    let (rough_metal_floor, _) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::RoughMetal, BLUE_SKY, None, 1.0)).await;
    assert!(
        brightness(rough_plain_floor) < 0.01,
        "a rough floor that is not metal should reflect no sky: {rough_plain_floor:?}"
    );
    assert!(
        brightness(rough_metal_floor) > 0.05,
        "a rough metal floor should still reflect the sky: {rough_metal_floor:?}"
    );
    Ok(())
}

// A skybox image covers the gradient in reflections just as it does in the sky, as far as its opacity lets it. The sky
// shows the image exactly as drawn, while a reflection is light like any other and goes through the tone curve, which
// darkens its darkest channel a little, so only the main color is compared.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn smooth_metal_reflects_the_skybox_image_over_the_gradient() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let (image_floor, image_sky) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::Mirror, RED_SKY, Some(1.0), 1.0)).await;
    assert!(
        image_floor[1] > 2.0 * image_floor[0],
        "the mirror floor {image_floor:?} should show the green skybox image, not the red gradient"
    );
    assert!(
        (image_floor[1] - image_sky[1]).abs() < 0.1 * image_sky[1],
        "the mirror floor {image_floor:?} should be about as bright as the sky {image_sky:?}"
    );
    let (hidden_image_floor, _) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::Mirror, RED_SKY, Some(0.0), 1.0)).await;
    assert!(
        hidden_image_floor[0] > 2.0 * hidden_image_floor[1],
        "with the image fully transparent the mirror floor {hidden_image_floor:?} should show the red gradient"
    );
    Ok(())
}

// The world's sky reflection strength scales every reflection of the sky, and 0 turns them off.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn sky_reflection_strength_scales_reflections() -> anyhow::Result<()> {
    let mut renderer = create_renderer().await?;
    let mut floor_brightnesses = Vec::new();
    for sky_reflection_strength in [1.0, 0.5, 0.0] {
        let (floor, _) = capture_floor_and_sky(&mut renderer, &build_floor_scene(Floor::Mirror, BLUE_SKY, None, sky_reflection_strength)).await;
        floor_brightnesses.push(brightness(floor));
    }
    let [full, half, off] = floor_brightnesses[..] else { unreachable!() };
    assert!(
        off < 0.01,
        "strength 0 should turn the mirror floor's sky reflection off: {floor_brightnesses:?}"
    );
    assert!(
        (0.3 * full..0.7 * full).contains(&half),
        "strength 0.5 should roughly halve the mirror floor's sky reflection: {floor_brightnesses:?}"
    );
    Ok(())
}
