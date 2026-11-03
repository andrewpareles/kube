use anyhow::Result;
use client::simple_renderer::decode_texture_data_to_mip_data;
use render_core::create_gpu_texture::create_gpu_texture_from_mip_data;
use shared::game::texture_converter::convert_rgba8_pixels_to_texture_data;

const ASTC_4X4: wgpu::TextureFormat = wgpu::TextureFormat::Astc {
    block: wgpu::AstcBlock::B4x4,
    channel: wgpu::AstcChannel::Unorm,
};

// ETC1S keeps one color per 4x4 block and can only brighten or darken it per pixel, so normals whose X and Y move in
// opposite directions inside a block came back flat. That is what made the sides of objects look pixelated.
#[test]
fn normal_maps_keep_opposite_x_and_y_detail_inside_each_4x4_block() -> Result<()> {
    let tilted_right = [166, 89, 243, 255];
    let tilted_left = [89, 166, 243, 255];
    let pixels: Vec<u8> = (0..16 * 16)
        .flat_map(|index| if (index % 16 + index / 16) % 2 == 0 { tilted_right } else { tilted_left })
        .collect();
    let texture = convert_rgba8_pixels_to_texture_data(pixels.clone(), 16, 16, false)?;
    let decoded = decode_texture_data_to_mip_data(&texture, wgpu::TextureFormat::Rgba8Unorm)?;
    for (index, (actual, expected)) in decoded[0].iter().zip(&pixels).enumerate() {
        assert!(
            actual.abs_diff(*expected) <= 4,
            "texel {} channel {} decoded as {actual}, stored {expected}",
            index / 4,
            index % 4
        );
    }
    Ok(())
}

#[test]
fn compressed_decodes_fill_whole_4x4_blocks_in_every_mip() -> Result<()> {
    for is_srgb in [true, false] {
        // Every pixel differs, so a data map keeps its full size instead of being stored as one flat 4x4 block.
        let pixels = (0..16 * 8).flat_map(|index| [200, 30, index as u8, 255]).collect();
        let texture = convert_rgba8_pixels_to_texture_data(pixels, 16, 8, is_srgb)?;
        for (upload_format, expected_byte_counts) in [
            (wgpu::TextureFormat::Bc1RgbaUnorm, [64, 16, 8, 8, 8]),
            (wgpu::TextureFormat::Bc7RgbaUnorm, [128, 32, 16, 16, 16]),
            (ASTC_4X4, [128, 32, 16, 16, 16]),
        ] {
            let mip_byte_counts: Vec<usize> = decode_texture_data_to_mip_data(&texture, upload_format)?.iter().map(Vec::len).collect();
            assert_eq!(mip_byte_counts, expected_byte_counts, "{upload_format:?}, is_srgb {is_srgb}");
        }
        assert!(decode_texture_data_to_mip_data(&texture, wgpu::TextureFormat::R8Unorm).is_err());
    }
    Ok(())
}

// Requires a graphics adapter. Run explicitly with:
// cargo test -p client --test texture_encoding_tests -- --ignored
#[tokio::test]
#[ignore = "requires a graphics adapter; run with --ignored"]
async fn compressed_textures_read_back_their_source_colors_on_the_gpu() -> Result<()> {
    let adapter = wgpu::Instance::default().request_adapter(&Default::default()).await?;
    let compression_features = adapter.features() & (wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC);
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: compression_features,
            ..Default::default()
        })
        .await?;
    let mut upload_formats = vec![wgpu::TextureFormat::Rgba8Unorm];
    if compression_features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
        upload_formats.extend([wgpu::TextureFormat::Bc1RgbaUnorm, wgpu::TextureFormat::Bc7RgbaUnorm]);
    }
    if compression_features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC) {
        upload_formats.push(ASTC_4X4);
    }

    // One solid color per 8x8 quadrant, read back from the middle of each quadrant.
    let quadrant_colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]];
    let pixels: Vec<u8> = (0..16 * 16).flat_map(|index| quadrant_colors[(index / 16 / 8) * 2 + index % 16 / 8]).collect();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            r#"
            @group(0) @binding(0) var source_texture: texture_2d<f32>;
            @group(0) @binding(1) var<storage, read_write> result: array<vec4<f32>, 4>;
            @compute @workgroup_size(1)
            fn main() {
                result[0] = textureLoad(source_texture, vec2<i32>(4, 4), 0);
                result[1] = textureLoad(source_texture, vec2<i32>(12, 4), 0);
                result[2] = textureLoad(source_texture, vec2<i32>(4, 12), 0);
                result[3] = textureLoad(source_texture, vec2<i32>(12, 12), 0);
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

    for is_srgb in [false, true] {
        let texture_data = convert_rgba8_pixels_to_texture_data(pixels.clone(), 16, 16, is_srgb)?;
        for &upload_format in &upload_formats {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mip_data = decode_texture_data_to_mip_data(&texture_data, upload_format)?;
            let texture = create_gpu_texture_from_mip_data(&device, &queue, &mip_data, 16, 16, upload_format, is_srgb, "Quadrants")?;
            let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            // The linear view reads the stored values, so sRGB color textures compare against their bytes too.
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.linear_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
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
            encoder.copy_buffer_to_buffer(&result_buffer, 0, &readback_buffer, 0, 64);
            queue.submit([encoder.finish()]);
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            readback_buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                result_tx.send(result).unwrap();
            });
            device.poll(wgpu::PollType::wait_indefinitely())?;
            result_rx.await??;
            let values: Vec<f32> = readback_buffer
                .slice(..)
                .get_mapped_range()
                .chunks_exact(4)
                .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
            readback_buffer.unmap();
            assert!(device.pop_error_scope().await.is_none(), "{upload_format:?} upload failed validation");
            for (quadrant_index, quadrant_color) in quadrant_colors.iter().enumerate() {
                for (channel, expected) in quadrant_color.iter().enumerate() {
                    let actual = values[quadrant_index * 4 + channel] * 255.0;
                    assert!(
                        (actual - *expected as f32).abs() <= 6.0,
                        "{upload_format:?}, is_srgb {is_srgb}: quadrant {quadrant_index} channel {channel} read {actual}, stored {expected}"
                    );
                }
            }
        }
    }
    Ok(())
}

// A one-row image is stored as a 1D texture, whose header records a height of 0 as KTX2 requires. Nothing
// reads that header: the renderer uploads from the width and height stored beside the bytes.
#[test]
fn single_row_textures_round_trip_as_one_dimensional_images() -> Result<()> {
    for width in [1, 8] {
        let texture = convert_rgba8_pixels_to_texture_data([0, 55, 230, 255].repeat(width as usize), width, 1, false)?;
        assert_eq!(u32::from_le_bytes(texture.ktx2Data[20..24].try_into()?), width);
        assert_eq!(u32::from_le_bytes(texture.ktx2Data[24..28].try_into()?), 0);
        let decoded = decode_texture_data_to_mip_data(&texture, wgpu::TextureFormat::Rgba8Unorm)?;
        assert_eq!(decoded.len(), texture.mipLevelCount as usize);
        assert_eq!(decoded[0].len(), width as usize * 4);
        for pixel in decoded[0].chunks_exact(4) {
            for (actual, expected) in pixel.iter().zip([0u8, 55, 230, 255]) {
                assert!(actual.abs_diff(expected) <= 2);
            }
        }
    }
    Ok(())
}

// UASTC costs a byte per pixel whatever the image holds, so data maps are stored small. Color maps keep their size.
#[test]
fn data_maps_are_scaled_down_and_flat_ones_are_stored_as_one_block() -> Result<()> {
    let varied_pixels = |width: u32, height: u32| {
        (0..width * height)
            .flat_map(|index| [(index % 251) as u8, (index % 241) as u8, 128, 255])
            .collect()
    };
    let data_map = convert_rgba8_pixels_to_texture_data(varied_pixels(600, 300), 600, 300, false)?;
    assert_eq!((data_map.width, data_map.height), (256, 128));
    let color_map = convert_rgba8_pixels_to_texture_data(varied_pixels(600, 4), 600, 4, true)?;
    assert_eq!((color_map.width, color_map.height), (600, 4));

    let flat_map = convert_rgba8_pixels_to_texture_data([0, 0, 0, 255].repeat(300 * 300), 300, 300, false)?;
    assert_eq!((flat_map.width, flat_map.height), (4, 4));
    let decoded = decode_texture_data_to_mip_data(&flat_map, wgpu::TextureFormat::Rgba8Unorm)?;
    assert!(
        decoded[0]
            .chunks_exact(4)
            .all(|pixel| pixel.iter().zip([0u8, 0, 0, 255]).all(|(actual, expected)| actual.abs_diff(expected) <= 1))
    );
    Ok(())
}
