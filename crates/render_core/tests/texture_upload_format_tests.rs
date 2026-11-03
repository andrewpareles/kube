use render_core::create_gpu_texture::{choose_texture_upload_format, mip_level_byte_count};

const ASTC_4X4: wgpu::TextureFormat = wgpu::TextureFormat::Astc {
    block: wgpu::AstcBlock::B4x4,
    channel: wgpu::AstcChannel::Unorm,
};

#[test]
fn textures_upload_as_astc_then_bc1_or_bc7_and_stay_rgba_without_compression_or_whole_blocks() {
    let astc_and_bc = wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC;
    let bc = wgpu::Features::TEXTURE_COMPRESSION_BC;
    for (is_srgb, has_alpha) in [(true, false), (true, true), (false, false)] {
        assert_eq!(choose_texture_upload_format(astc_and_bc, 16, 8, is_srgb, has_alpha), ASTC_4X4);
        assert_eq!(
            choose_texture_upload_format(wgpu::Features::TEXTURE_COMPRESSION_ASTC, 16, 8, is_srgb, has_alpha),
            ASTC_4X4
        );
        assert_eq!(
            choose_texture_upload_format(wgpu::Features::empty(), 16, 8, is_srgb, has_alpha),
            wgpu::TextureFormat::Rgba8Unorm
        );
        for (width, height) in [(1000, 750), (6, 8), (1, 1)] {
            assert_eq!(
                choose_texture_upload_format(astc_and_bc, width, height, is_srgb, has_alpha),
                wgpu::TextureFormat::Rgba8Unorm
            );
        }
    }
    // Opaque color maps take BC1. Color with alpha and data maps keep BC7.
    assert_eq!(choose_texture_upload_format(bc, 16, 8, true, false), wgpu::TextureFormat::Bc1RgbaUnorm);
    assert_eq!(choose_texture_upload_format(bc, 16, 8, true, true), wgpu::TextureFormat::Bc7RgbaUnorm);
    assert_eq!(choose_texture_upload_format(bc, 16, 8, false, false), wgpu::TextureFormat::Bc7RgbaUnorm);
}

#[test]
fn compressed_mips_smaller_than_a_block_still_take_a_whole_block() {
    let mip_byte_counts = |upload_format| {
        (0..5)
            .map(|mip_level| mip_level_byte_count(upload_format, 16, 8, mip_level))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        mip_byte_counts(wgpu::TextureFormat::Bc7RgbaUnorm),
        [Some(128), Some(32), Some(16), Some(16), Some(16)]
    );
    assert_eq!(mip_byte_counts(ASTC_4X4), [Some(128), Some(32), Some(16), Some(16), Some(16)]);
    assert_eq!(
        mip_byte_counts(wgpu::TextureFormat::Bc1RgbaUnorm),
        [Some(64), Some(16), Some(8), Some(8), Some(8)]
    );
    assert_eq!(
        mip_byte_counts(wgpu::TextureFormat::Rgba8Unorm),
        [Some(512), Some(128), Some(32), Some(8), Some(4)]
    );
}
