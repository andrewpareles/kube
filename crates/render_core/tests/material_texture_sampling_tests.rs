#[path = "helpers/render_capture.rs"]
mod render_capture;

// These tests require a graphics adapter. Run them explicitly with:
// cargo test -p render_core --test material_texture_sampling_tests -- --ignored

use bytemuck::Zeroable;
use render_core::{
    create_gpu_texture::create_gpu_texture_from_mip_data,
    render_scene::{LightingBufferContent, ShadowBufferContent},
};
use wgpu::util::DeviceExt;

// -------------------------- GPU sampling and readback --------------------------

// Sample real GPU views so these tests catch color conversion happening before texture filtering.
async fn read_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<f32> {
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        result_tx.send(result).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    result_rx.await.unwrap().unwrap();
    let values = bytemuck::cast_slice(&buffer.slice(..).get_mapped_range()).to_vec();
    buffer.unmap();
    values
}

async fn sample_views(device: &wgpu::Device, queue: &wgpu::Queue, color_view: &wgpu::TextureView, data_view: &wgpu::TextureView) -> Vec<f32> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Material texture sampling regression"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
            @group(0) @binding(0) var color_texture: texture_2d<f32>;
            @group(0) @binding(1) var data_texture: texture_2d<f32>;
            @group(0) @binding(2) var texture_sampler: sampler;
            @group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>, 3>;
            @compute @workgroup_size(1)
            fn main() {
                result[0] = textureSampleLevel(color_texture, texture_sampler, vec2<f32>(0.5), 0.0);
                result[1] = textureSampleLevel(data_texture, texture_sampler, vec2<f32>(0.5), 0.0);
                result[2] = textureLoad(data_texture, vec2<i32>(0), i32(textureNumLevels(data_texture)) - 1);
            }
        "#
            .into(),
        ),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(color_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(data_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: result_buffer.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&result_buffer, 0, &readback, 0, 48);
    queue.submit([encoder.finish()]);
    read_buffer(device, &readback).await
}

// -------------------------- Color, data, mipmaps, and coverage --------------------------

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn material_data_views_preserve_values_without_changing_color_or_coverage_views() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let srgb_to_linear = |value: f32| ((value + 0.055) / 1.055).powf(2.4);

    // A flat normal marked sRGB must still mean a flat normal when used as material data.
    // The same uploaded image may simultaneously be used as color in another material.
    for is_srgb in [false, true] {
        let texture = create_gpu_texture_from_mip_data(
            &device,
            &queue,
            &[vec![128, 128, 255, 128]],
            1,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            is_srgb,
            "Flat normal",
        )
        .unwrap();
        let values = sample_views(&device, &queue, &texture.texture_view, &texture.linear_texture_view).await;
        let encoded_half = 128.0 / 255.0;
        let expected_color = if is_srgb { srgb_to_linear(encoded_half) } else { encoded_half };
        assert!((values[0] - expected_color).abs() < 0.001, "color transfer changed: {values:?}");
        assert!((values[3] - encoded_half).abs() < 0.001, "color alpha changed: {values:?}");
        assert!((values[4] - encoded_half).abs() < 0.001, "normal X was color-corrected: {values:?}");
        assert!((values[5] - encoded_half).abs() < 0.001, "normal Y was color-corrected: {values:?}");
        assert_eq!(values[6], 1.0);
        assert!((values[7] - encoded_half).abs() < 0.001, "data alpha changed: {values:?}");
    }

    // Data maps interpolate their numeric values; color maps decode sRGB before interpolation.
    let texture = create_gpu_texture_from_mip_data(
        &device,
        &queue,
        &[vec![64, 64, 64, 64, 192, 192, 192, 192], vec![128; 4]],
        2,
        1,
        wgpu::TextureFormat::Rgba8Unorm,
        true,
        "Filtered scalar data",
    )
    .unwrap();
    let values = sample_views(&device, &queue, &texture.texture_view, &texture.linear_texture_view).await;
    let expected_color = (srgb_to_linear(64.0 / 255.0) + srgb_to_linear(192.0 / 255.0)) * 0.5;
    assert!((values[0] - expected_color).abs() < 0.001, "color filtering changed: {values:?}");
    assert!((values[4] - 128.0 / 255.0).abs() < 0.001, "data filtering used sRGB: {values:?}");
    assert!((values[8] - 128.0 / 255.0).abs() < 0.001, "data mip level used sRGB: {values:?}");

    let distances = create_gpu_texture_from_mip_data(&device, &queue, &[vec![128]], 1, 1, wgpu::TextureFormat::R8Unorm, false, "Glyph distances").unwrap();
    let values = sample_views(&device, &queue, &distances.texture_view, &distances.linear_texture_view).await;
    assert!((values[0] - 128.0 / 255.0).abs() < 0.001);
    assert_eq!(&values[..4], &values[4..8]);
}

// -------------------------- Sun visibility across cascades and angles --------------------------

// Run the production shadow function on the GPU with an actual clear depth texture. A surface
// facing away from the sun is occluded by its own geometry, even if a normal map tilts toward it.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn sun_visibility_respects_surface_facing_and_shadow_map_occlusion() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let shader_source = format!(
        "{}\n{}",
        include_str!("../src/scene.wgsl"),
        r#"
        @vertex fn test_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
            let positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
            return vec4<f32>(positions[index], 0.5, 1.0);
        }
        @fragment fn test_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
            let world_position = vec3<f32>(position.xy * 0.01, 0.5);
            let direction_to_light = normalize(lighting.sun_direction.xyz);
            return vec4<f32>(
                sample_sun_shadow(world_position, vec3<f32>(0.0, 0.0, 1.0), direction_to_light),
                sample_sun_shadow(world_position, vec3<f32>(0.0, 0.0, -1.0), direction_to_light),
                sample_sun_shadow(world_position, vec3<f32>(1.0, 0.0, 0.0), direction_to_light), 1.0);
        }
    "#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("test_vertex"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("test_fragment"),
            targets: &[Some(wgpu::TextureFormat::Rgba32Float.into())],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    // Each cascade's texels are twice as wide as the one before, as in the renderer, so the blur settles in the cascade
    // that holds the point instead of the coarsest one.
    let cascade_settings = std::array::from_fn(|index| [(1 << index) as f32, 0.01 * (1 << index) as f32, 0.0, 0.0]);
    let shadow_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&ShadowBufferContent {
            light_view_proj: [glam::Mat4::IDENTITY.to_cols_array_2d(); 5],
            cascade_settings,
            settings: [0.0, 0.0, 0.0, 1.0],
        }),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let lighting_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&LightingBufferContent::zeroed()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 5,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth_array_view = depth.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let depth_layer_views: Vec<_> = (0..5)
        .map(|layer| {
            depth.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let compare_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(3),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: lighting_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shadow_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&depth_array_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&compare_sampler),
            },
        ],
    });
    let empty_bind_groups: Vec<_> = (0..3)
        .map(|index| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(index),
                entries: &[],
            })
        })
        .collect();
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output.create_view(&Default::default());
    // Exercise all cascade branches, the reported sun angle, the opposite side, grazing sun, and overhead sun.
    for cascade_index in 0..5 {
        let shadow_content = ShadowBufferContent {
            light_view_proj: std::array::from_fn(|index| {
                if index < cascade_index {
                    glam::Mat4::from_translation(glam::Vec3::splat(10.0)).to_cols_array_2d()
                } else {
                    glam::Mat4::IDENTITY.to_cols_array_2d()
                }
            }),
            cascade_settings,
            settings: [0.0, 0.0, 0.0, 1.0],
        };
        queue.write_buffer(&shadow_buffer, 0, bytemuck::bytes_of(&shadow_content));
        for direction_to_light in [
            [0.413176, 0.766044, 0.492404, 0.0],
            [-0.4, 0.8, -0.45, 0.0],
            [1.0, 0.001, 0.001, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ] {
            let mut lighting_content = LightingBufferContent::zeroed();
            lighting_content.sun_direction = direction_to_light;
            queue.write_buffer(&lighting_buffer, 0, bytemuck::bytes_of(&lighting_content));
            for depth_value in [1.0, 0.0] {
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 512,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                for depth_layer_view in &depth_layer_views {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_layer_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(depth_value),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                }
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &output_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_pipeline(&pipeline);
                    for (index, empty_bind_group) in empty_bind_groups.iter().enumerate() {
                        pass.set_bind_group(index as u32, empty_bind_group, &[]);
                    }
                    pass.set_bind_group(3, &bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                encoder.copy_texture_to_buffer(
                    output.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: Some(2),
                        },
                    },
                    wgpu::Extent3d {
                        width: 2,
                        height: 2,
                        depth_or_array_layers: 1,
                    },
                );
                queue.submit([encoder.finish()]);
                let values = read_buffer(&device, &readback).await;
                assert_eq!(
                    values[0],
                    if direction_to_light[2] > 0.0 { depth_value } else { 0.0 },
                    "+Z face in cascade {cascade_index} with sun {direction_to_light:?}"
                );
                assert_eq!(
                    values[1],
                    if direction_to_light[2] < 0.0 { depth_value } else { 0.0 },
                    "-Z face in cascade {cascade_index} with sun {direction_to_light:?}"
                );
                assert_eq!(
                    values[2],
                    if direction_to_light[0] > 0.0 { depth_value } else { 0.0 },
                    "+X face in cascade {cascade_index} with sun {direction_to_light:?}"
                );
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn renderer_refreshes_material_when_texture_arrives_or_changes() -> anyhow::Result<()> {
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

    let white = Color3 { r: 1.0, g: 1.0, b: 1.0 };
    let objects = HashMap::from([
        (
            1,
            RenderObject::Brick(RenderBrick {
                transform: RenderTransform {
                    physicsInfo3D: PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                    teleport_count: 0,
                    rigid_group_to_follow: None,
                    sk_joint_to_follow_arc: None,
                },
                visualSettings: VisualSettings {
                    color: white,
                    ..VisualSettings::TYPICAL_OPAQUE
                },
                primitive: BrickPrimitiveSettings { size: Vec3::splat(2.0) },
                material: Some(BrickMaterialSettings {
                    materialInfoId: 2.into(),
                    tileSizeU: 1.0,
                    tileSizeV: 1.0,
                    tileOffsetU: 0.0,
                    tileOffsetV: 0.0,
                }),
            }),
        ),
        (
            2,
            RenderObject::Material(MaterialInfo {
                albedoTextureDataId: Some(21),
                metallicTextureDataId: None,
                roughnessTextureDataId: None,
                normalTextureDataId: None,
                alphaMode: AlphaMode::Opaque,
                isDoubleSided: false,
            }),
        ),
        (
            3,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunIntensity: 0.0,
                ambientIntensity: 1.0,
                ambientTopColor: white,
                ambientBottomColor: white,
                ..WorldLightingInfo::default()
            }),
        ),
    ]);
    let scene = build_render_scene(&objects, &HashMap::new(), None);
    let adapter = wgpu::Instance::default().request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, 64, 64)?;
    let camera_position = Vec3::Z * 5.0;
    let (_, camera_bind_group) = renderer.create_camera_matrix_buffer_pair(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let mut colors = Vec::new();
    // Draw before decoding finishes, then load the texture and replace it under the same ID.
    // The next draw must rebuild bindings in both cases, without the caller managing them.
    for rgba in [None, Some(vec![255, 0, 0, 255]), Some(vec![0, 255, 0, 255])] {
        if let Some(rgba) = rgba {
            renderer.load_texture_into_gpu(21, &[rgba], 1, 1, wgpu::TextureFormat::Rgba8Unorm, true)?;
        }
        renderer.resize(96, 96);
        let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, 64, 64);
        assert_eq!(texture.width(), 64);
        renderer.render_scene(
            &mut frame,
            &scene,
            None,
            &camera_bind_group,
            Vec3::ZERO,
            camera_position,
            build_camera_view_proj_matrix(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES),
        );
        let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Material center pixel"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut source = texture.as_image_copy();
        source.origin = wgpu::Origin3d { x: 32, y: 32, z: 0 };
        frame.encoder.copy_texture_to_buffer(
            source,
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        renderer.finish_frame(frame);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            result_tx.send(result).unwrap();
        });
        renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
        result_rx.await??;
        let pixel: [u8; 4] = buffer.slice(..).get_mapped_range()[..4].try_into()?;
        colors.push(pixel);
        buffer.unmap();
    }
    let [white_pixel, red_pixel, green_pixel] = colors.as_slice() else {
        unreachable!()
    };
    assert!(
        white_pixel[..3].iter().all(|channel| *channel > 100),
        "Expected the initial white material: {colors:?}"
    );
    assert!(
        red_pixel[0] > 100 && red_pixel[1] < 80 && red_pixel[2] < 80,
        "Texture arrival did not replace the default binding: {colors:?}"
    );
    assert!(
        green_pixel[1] > 100 && green_pixel[0] < 80 && green_pixel[2] < 80,
        "Texture replacement did not refresh the binding: {colors:?}"
    );
    assert!(renderer.device().pop_error_scope().await.is_none());
    Ok(())
}

// A normal map bends a brick face's normal by the same amount however long the brick is. The end of a long thin wall
// and the side of a cube, lit alike through the same slightly tilted normal map, should come out the same shade. Taking
// the brick's size into the normal a second time used to bend the long brick's end almost flat against the wall, so the
// map's faint noise flipped it between lit and unlit in blotches.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn normal_maps_bend_the_end_of_a_long_brick_like_any_other_face() -> anyhow::Result<()> {
    use game_types::{
        object_types::{BrickMaterialSettings, MaterialInfo, Orientation3D, PhysicsInfo3D, VisualSettings, WorldLightingInfo, material::AlphaMode},
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

    let adapter = wgpu::Instance::default().request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, 64, 64)?;
    // Tilted about ten degrees along the face.
    renderer.load_texture_into_gpu(22, &[vec![150, 128, 250, 255]], 1, 1, wgpu::TextureFormat::Rgba8Unorm, false)?;
    let camera_position = Vec3::new(4.0, 2.0, 0.0);
    let camera_target = Vec3::new(0.0, 2.0, 0.0);
    let mut center_pixels = Vec::new();
    // Both bricks put a 1 by 4 face at x = 0, facing the camera, but the first is 20 long along that face's normal.
    for (center, size) in [
        (Vec3::new(-10.0, 2.0, 0.0), Vec3::new(20.0, 4.0, 1.0)),
        (Vec3::new(-0.5, 2.0, 0.0), Vec3::new(1.0, 4.0, 1.0)),
    ] {
        let objects = HashMap::from([
            (
                1,
                RenderObject::Brick(RenderBrick {
                    transform: RenderTransform {
                        physicsInfo3D: PhysicsInfo3D::new(center, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                        teleport_count: 0,
                        rigid_group_to_follow: None,
                        sk_joint_to_follow_arc: None,
                    },
                    visualSettings: VisualSettings::TYPICAL_OPAQUE,
                    primitive: BrickPrimitiveSettings { size },
                    material: Some(BrickMaterialSettings {
                        materialInfoId: 2.into(),
                        tileSizeU: 1.0,
                        tileSizeV: 1.0,
                        tileOffsetU: 0.0,
                        tileOffsetV: 0.0,
                    }),
                }),
            ),
            (
                2,
                RenderObject::Material(MaterialInfo {
                    albedoTextureDataId: None,
                    metallicTextureDataId: None,
                    roughnessTextureDataId: None,
                    normalTextureDataId: Some(22),
                    alphaMode: AlphaMode::Opaque,
                    isDoubleSided: false,
                }),
            ),
            (
                3,
                RenderObject::WorldLighting(WorldLightingInfo {
                    // Up and out from the face, a little to one side, so bending the normal along the face changes how
                    // much sun it gets.
                    sunOrientation: Orientation3D { theta: 1.2, phi: -0.5 },
                    ambientOcclusionIntensity: 0.0,
                    ..WorldLightingInfo::default()
                }),
            ),
        ]);
        let scene = build_render_scene(&objects, &HashMap::new(), None);
        let (_, camera_bind_group) = renderer.create_camera_matrix_buffer_pair(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
        let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, 64, 64);
        renderer.render_scene(
            &mut frame,
            &scene,
            None,
            &camera_bind_group,
            camera_target,
            camera_position,
            build_camera_view_proj_matrix(camera_position, camera_target, 1.0, DEFAULT_CAMERA_FOV_DEGREES),
        );
        let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Brick face center pixel"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut source = texture.as_image_copy();
        source.origin = wgpu::Origin3d { x: 32, y: 32, z: 0 };
        frame.encoder.copy_texture_to_buffer(
            source,
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        renderer.finish_frame(frame);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            result_tx.send(result).unwrap();
        });
        renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
        result_rx.await??;
        let pixel: [u8; 4] = buffer.slice(..).get_mapped_range()[..4].try_into()?;
        buffer.unmap();
        center_pixels.push(pixel);
    }
    let [long_brick_end, cube_side] = center_pixels.as_slice() else {
        unreachable!()
    };
    assert!(
        (0..3).all(|channel| long_brick_end[channel].abs_diff(cube_side[channel]) <= 2),
        "the long brick's end {long_brick_end:?} and the cube's side {cube_side:?} should shade alike"
    );
    Ok(())
}

// With the tile as big as the face and no offset, the image starts at the face's top left corner and covers it once.
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn a_tile_the_size_of_a_brick_face_shows_the_whole_image_once() -> anyhow::Result<()> {
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

    let white = Color3 { r: 1.0, g: 1.0, b: 1.0 };
    let objects = HashMap::from([
        (
            1,
            RenderObject::Brick(RenderBrick {
                transform: RenderTransform {
                    physicsInfo3D: PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                    teleport_count: 0,
                    rigid_group_to_follow: None,
                    sk_joint_to_follow_arc: None,
                },
                visualSettings: VisualSettings {
                    color: white,
                    ..VisualSettings::TYPICAL_OPAQUE
                },
                primitive: BrickPrimitiveSettings {
                    size: Vec3::new(4.0, 4.0, 1.0),
                },
                material: Some(BrickMaterialSettings {
                    materialInfoId: 2.into(),
                    tileSizeU: 4.0,
                    tileSizeV: 4.0,
                    tileOffsetU: 0.0,
                    tileOffsetV: 0.0,
                }),
            }),
        ),
        (
            2,
            RenderObject::Material(MaterialInfo {
                albedoTextureDataId: Some(21),
                metallicTextureDataId: None,
                roughnessTextureDataId: None,
                normalTextureDataId: None,
                alphaMode: AlphaMode::Opaque,
                isDoubleSided: false,
            }),
        ),
        (
            3,
            RenderObject::WorldLighting(WorldLightingInfo {
                sunIntensity: 0.0,
                ambientIntensity: 1.0,
                ambientTopColor: white,
                ambientBottomColor: white,
                ..WorldLightingInfo::default()
            }),
        ),
    ]);
    let scene = build_render_scene(&objects, &HashMap::new(), None);
    let adapter = wgpu::Instance::default().request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, 64, 64)?;
    // Red and green on the top row, blue and white on the bottom.
    let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
    renderer.load_texture_into_gpu(21, &[rgba], 2, 2, wgpu::TextureFormat::Rgba8Unorm, true)?;
    let camera_position = Vec3::Z * 8.0;
    let view_proj = build_camera_view_proj_matrix(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (_, camera_bind_group) = renderer.create_camera_matrix_buffer_pair(camera_position, Vec3::ZERO, 1.0, DEFAULT_CAMERA_FOV_DEGREES);
    let (mut frame, texture) = render_capture::begin_frame_for_readback(&mut renderer, 64, 64);
    renderer.render_scene(&mut frame, &scene, None, &camera_bind_group, Vec3::ZERO, camera_position, view_proj);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Brick face pixels"),
        size: 256 * 64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    frame.encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(64),
            },
        },
        wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
    );
    renderer.finish_frame(frame);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        result_tx.send(result).unwrap();
    });
    renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
    result_rx.await??;
    let pixels = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    // The middle of each quarter of the face, where each of the image's four texels lands.
    let pixel_at = |x: f32, y: f32| {
        let ndc = view_proj.project_point3(Vec3::new(x, y, 0.5));
        let column = ((ndc.x * 0.5 + 0.5) * 64.0) as usize;
        let row = ((0.5 - ndc.y * 0.5) * 64.0) as usize;
        let start = row * 256 + column * 4;
        [pixels[start], pixels[start + 1], pixels[start + 2]]
    };
    let top_left = pixel_at(-1.0, 1.0);
    let top_right = pixel_at(1.0, 1.0);
    let bottom_left = pixel_at(-1.0, -1.0);
    let bottom_right = pixel_at(1.0, -1.0);
    let quarters = [top_left, top_right, bottom_left, bottom_right];
    assert!(
        top_left[0] > 100 && top_left[1] < 80 && top_left[2] < 80,
        "top left should be red: {quarters:?}"
    );
    assert!(
        top_right[1] > 100 && top_right[0] < 80 && top_right[2] < 80,
        "top right should be green: {quarters:?}"
    );
    assert!(
        bottom_left[2] > 100 && bottom_left[0] < 80 && bottom_left[1] < 80,
        "bottom left should be blue: {quarters:?}"
    );
    assert!(bottom_right.iter().all(|channel| *channel > 100), "bottom right should be white: {quarters:?}");
    assert!(renderer.device().pop_error_scope().await.is_none());
    Ok(())
}
