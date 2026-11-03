use render_core::renderer::{RenderFrame, Renderer};

// GPU tests read back an sRGB texture through the renderer's normal resize and frame APIs.
pub fn begin_frame_for_readback(renderer: &mut Renderer, width: u32, height: u32) -> (RenderFrame, wgpu::Texture) {
    renderer.resize(width, height);
    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("Test Readback Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (renderer.begin_frame(view), texture)
}
