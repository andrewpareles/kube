// Object outlines: a colored border around the outside of each outlined object's silhouette on screen.
//
// renderer.rs draws each layer's outlined objects into ObjectOutlineTextures::mask_texture_view, then calls
// draw_nearest_edges, which finds the nearest object edge for every pixel and writes the solid part of the outlines into
// the layer's depth. The layer's opaque pass then draws the solid part of the outlines before its objects, and the layer's
// transparent pass ends with draw_transparent_outline_pass. See object_outline.wgsl for what each pass does.
use crate::render_scene::decode_srgb_color;
use game_types::object_types::shared::OutlineSettings;
use wgpu::util::DeviceExt;

pub const OUTLINE_MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const OUTLINE_NEAREST_EDGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
// Keep this synced with OUTLINE_SETTINGS_TEXTURE_WIDTH in object_outline.wgsl.
const OUTLINE_SETTINGS_TEXTURE_WIDTH: u32 = 1024;
// TODO: AI-PICKED-VALUE: Jump flood steps from 16 pixels down to 1 reach 31 pixels, past the thickest outline
// (OutlineSettings::MAX_THICKNESS plus its one pixel of fade), with room to spare.
const JUMP_FLOOD_STEP_PIXELS: [i32; 5] = [16, 8, 4, 2, 1];

// ---------------------------------- Textures ----------------------------------

// The textures the outline passes share. The layers draw one after the other, so they take turns with one set.
pub struct ObjectOutlineTextures {
    pub mask_texture_view: wgpu::TextureView,
    pub mask_depth_texture_view: wgpu::TextureView,
    // The jump flood passes read one and write the other, swapping each pass.
    nearest_edge_texture_views: [wgpu::TextureView; 2],
}

pub fn create_object_outline_textures(device: &wgpu::Device, width: u32, height: u32, depth_format: wgpu::TextureFormat) -> ObjectOutlineTextures {
    let create_texture_view = |format: wgpu::TextureFormat, label: &str| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    ObjectOutlineTextures {
        mask_texture_view: create_texture_view(OUTLINE_MASK_FORMAT, "Outline Mask Texture"),
        mask_depth_texture_view: create_texture_view(depth_format, "Outline Mask Depth Texture"),
        nearest_edge_texture_views: [
            create_texture_view(OUTLINE_NEAREST_EDGE_FORMAT, "Outline Nearest Edge Texture A"),
            create_texture_view(OUTLINE_NEAREST_EDGE_FORMAT, "Outline Nearest Edge Texture B"),
        ],
    }
}

// ---------------------------------- Pipelines ----------------------------------

// The full screen passes of object_outline.wgsl. The mask pass draws objects with scene.wgsl, so renderer.rs builds it.
pub struct ObjectOutlinePipelines {
    bind_group_layout: wgpu::BindGroupLayout,
    seed_render_pipeline: wgpu::RenderPipeline,
    jump_flood_render_pipeline: wgpu::RenderPipeline,
    // One for each of JUMP_FLOOD_STEP_PIXELS, holding that step's size.
    jump_flood_step_bind_groups: Vec<wgpu::BindGroup>,
    opaque_outline_depth_render_pipeline: wgpu::RenderPipeline,
    opaque_outline_render_pipeline: wgpu::RenderPipeline,
    transparent_outline_render_pipeline: wgpu::RenderPipeline,
}

// The formats and depth rules of the layer passes the outlines draw into, so they match the scene's pipelines.
pub struct ObjectOutlineTargetFormats {
    pub color_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub depth_compare: wgpu::CompareFunction,
    pub opaque_multisample_count: u32,
    pub oit_accum_format: wgpu::TextureFormat,
    pub oit_weight_format: wgpu::TextureFormat,
    pub oit_blending: wgpu::BlendState,
}

impl ObjectOutlinePipelines {
    pub fn new(device: &wgpu::Device, target_formats: &ObjectOutlineTargetFormats) -> Self {
        let unfilterable_texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Outline Bind Group Layout"),
            entries: &[unfilterable_texture_entry(0), unfilterable_texture_entry(1), unfilterable_texture_entry(2)],
        });
        let jump_flood_step_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Outline Jump Flood Step Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let jump_flood_step_bind_groups = JUMP_FLOOD_STEP_PIXELS
            .iter()
            .map(|step_pixels| {
                // Uniform buffers are padded to 16 bytes on WebGL2.
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Outline Jump Flood Step Buffer"),
                    contents: bytemuck::cast_slice(&[*step_pixels, 0, 0, 0]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Outline Jump Flood Step Bind Group"),
                    layout: &jump_flood_step_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            })
            .collect();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Object Outline Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("object_outline.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Outline Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let jump_flood_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Outline Jump Flood Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout, &jump_flood_step_bind_group_layout],
            push_constant_ranges: &[],
        });
        // Every outline pass is one full screen triangle. Only what it writes and how it tests depth differ.
        let create_fullscreen_pipeline = |label: &str,
                                          layout: &wgpu::PipelineLayout,
                                          fragment_entry_point: &str,
                                          targets: &[Option<wgpu::ColorTargetState>],
                                          depth_stencil: Option<wgpu::DepthStencilState>,
                                          sample_count: u32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment_entry_point),
                    targets,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                multiview: None,
                cache: None,
            })
        };
        let nearest_edge_target = [Some(wgpu::ColorTargetState {
            format: OUTLINE_NEAREST_EDGE_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let depth_stencil = |depth_write_enabled: bool| {
            Some(wgpu::DepthStencilState {
                format: target_formats.depth_format,
                depth_write_enabled,
                depth_compare: target_formats.depth_compare,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        Self {
            seed_render_pipeline: create_fullscreen_pipeline("Outline Seed Pipeline", &pipeline_layout, "fs_seed", &nearest_edge_target, None, 1),
            jump_flood_render_pipeline: create_fullscreen_pipeline(
                "Outline Jump Flood Pipeline",
                &jump_flood_pipeline_layout,
                "fs_jump_flood",
                &nearest_edge_target,
                None,
                1,
            ),
            opaque_outline_depth_render_pipeline: create_fullscreen_pipeline(
                "Opaque Outline Depth Pipeline",
                &pipeline_layout,
                "fs_opaque_outline_depth",
                &[],
                depth_stencil(true),
                1,
            ),
            opaque_outline_render_pipeline: create_fullscreen_pipeline(
                "Opaque Outline Pipeline",
                &pipeline_layout,
                "fs_opaque_outline",
                &[Some(wgpu::ColorTargetState {
                    format: target_formats.color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                depth_stencil(true),
                target_formats.opaque_multisample_count,
            ),
            transparent_outline_render_pipeline: create_fullscreen_pipeline(
                "Transparent Outline Pipeline",
                &pipeline_layout,
                "fs_transparent_outline",
                &[
                    Some(wgpu::ColorTargetState {
                        format: target_formats.oit_accum_format,
                        blend: Some(target_formats.oit_blending),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: target_formats.oit_weight_format,
                        blend: Some(target_formats.oit_blending),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                depth_stencil(false),
                1,
            ),
            bind_group_layout,
            jump_flood_step_bind_groups,
        }
    }
}

// ---------------------------------- Each frame's outline settings ----------------------------------

// Every outline drawn this frame, numbered in the order they were added. The mask pass writes each object's number plus one,
// and the outline passes look its settings up in a texture by that number.
pub struct FrameOutlineSettings {
    device_pixel_ratio: f32,
    // Two texels per outline, laid out as object_outline.wgsl reads them.
    texels: Vec<[f32; 4]>,
}

impl FrameOutlineSettings {
    pub fn new(device_pixel_ratio: f32) -> Self {
        Self {
            device_pixel_ratio,
            texels: Vec::new(),
        }
    }

    // Returns the number plus one the mask pass writes for this outline.
    pub fn add(&mut self, outline: &OutlineSettings, is_object_transparent: bool) -> f32 {
        let outline_number = self.texels.len() / 2;
        self.texels.push(decode_srgb_color(&outline.color, outline.opacity.clamp(0.0, 1.0)));
        self.texels.push([
            // Thickness is in screen pixels. The jump flood passes reach no farther than the sum of their steps, and the
            // outline fades over one more pixel past its thickness.
            (outline.thickness.clamp(OutlineSettings::MIN_THICKNESS, OutlineSettings::MAX_THICKNESS) * self.device_pixel_ratio)
                .min((JUMP_FLOOD_STEP_PIXELS.iter().sum::<i32>() - 1) as f32),
            if is_object_transparent { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ]);
        (outline_number + 1) as f32
    }

    pub fn is_empty(&self) -> bool {
        self.texels.is_empty()
    }
}

// What the outline passes read this frame. reads_nearest_edge_bind_groups[i] reads nearest edge texture i.
pub struct FrameOutlineBindGroups {
    reads_nearest_edge_bind_groups: [wgpu::BindGroup; 2],
}

pub fn create_frame_outline_bind_groups(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &ObjectOutlinePipelines,
    textures: &ObjectOutlineTextures,
    frame_outline_settings: &FrameOutlineSettings,
) -> FrameOutlineBindGroups {
    let FrameOutlineSettings { texels, .. } = frame_outline_settings;
    let row_count = (texels.len() as u32).div_ceil(OUTLINE_SETTINGS_TEXTURE_WIDTH).max(1);
    let mut padded_texels = texels.clone();
    padded_texels.resize((row_count * OUTLINE_SETTINGS_TEXTURE_WIDTH) as usize, [0.0; 4]);
    let settings_texture_view = device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Outline Settings Texture"),
                size: wgpu::Extent3d {
                    width: OUTLINE_SETTINGS_TEXTURE_WIDTH,
                    height: row_count,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&padded_texels),
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
    let create_bind_group = |nearest_edge_texture_view: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Outline Bind Group"),
            layout: &pipelines.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&textures.mask_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&settings_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(nearest_edge_texture_view),
                },
            ],
        })
    };
    FrameOutlineBindGroups {
        reads_nearest_edge_bind_groups: [
            create_bind_group(&textures.nearest_edge_texture_views[0]),
            create_bind_group(&textures.nearest_edge_texture_views[1]),
        ],
    }
}

// ---------------------------------- Drawing ----------------------------------

fn set_pass_viewport(pass: &mut wgpu::RenderPass<'_>, clamped_viewport: Option<(u32, u32, u32, u32)>) {
    if let Some((x, y, width, height)) = clamped_viewport {
        pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, width, height);
    }
}

// The jump flood passes swap which nearest edge texture they write, so the last one written depends on how many there are.
fn final_nearest_edge_index() -> usize {
    JUMP_FLOOD_STEP_PIXELS.len() % 2
}

// After the mask pass: finds every pixel's nearest edge, then writes the solid part of the outlines into the layer's
// single-sample depth, so the layer's transparent surfaces behind an outline are hidden by it.
pub fn draw_nearest_edges(
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &ObjectOutlinePipelines,
    textures: &ObjectOutlineTextures,
    frame_outline_bind_groups: &FrameOutlineBindGroups,
    clamped_viewport: Option<(u32, u32, u32, u32)>,
    depth_view: &wgpu::TextureView,
) {
    // The seed pass writes texture 0, so its bind group is the one that reads texture 1.
    let mut written_index = 0;
    let passes = std::iter::once((&pipelines.seed_render_pipeline, None)).chain(
        pipelines
            .jump_flood_step_bind_groups
            .iter()
            .map(|jump_flood_step_bind_group| (&pipelines.jump_flood_render_pipeline, Some(jump_flood_step_bind_group))),
    );
    for (pass_index, (render_pipeline, jump_flood_step_bind_group)) in passes.enumerate() {
        written_index = pass_index % 2;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Outline Nearest Edge Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &textures.nearest_edge_texture_views[written_index],
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        set_pass_viewport(&mut pass, clamped_viewport);
        pass.set_pipeline(render_pipeline);
        pass.set_bind_group(0, &frame_outline_bind_groups.reads_nearest_edge_bind_groups[1 - written_index], &[]);
        if let Some(jump_flood_step_bind_group) = jump_flood_step_bind_group {
            pass.set_bind_group(1, jump_flood_step_bind_group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
    debug_assert_eq!(written_index, final_nearest_edge_index());

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Opaque Outline Depth Pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    set_pass_viewport(&mut pass, clamped_viewport);
    pass.set_pipeline(&pipelines.opaque_outline_depth_render_pipeline);
    pass.set_bind_group(0, &frame_outline_bind_groups.reads_nearest_edge_bind_groups[final_nearest_edge_index()], &[]);
    pass.draw(0..3, 0..1);
}

// Inside the layer's opaque pass, before its objects, so each object's own edge draws over the inside of its outline.
pub fn draw_opaque_outline(pass: &mut wgpu::RenderPass<'_>, pipelines: &ObjectOutlinePipelines, frame_outline_bind_groups: &FrameOutlineBindGroups) {
    pass.set_pipeline(&pipelines.opaque_outline_render_pipeline);
    pass.set_bind_group(0, &frame_outline_bind_groups.reads_nearest_edge_bind_groups[final_nearest_edge_index()], &[]);
    pass.draw(0..3, 0..1);
}

// After the layer's transparent pass, adding the rest of the outlines to its OIT targets.
pub fn draw_transparent_outline_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &ObjectOutlinePipelines,
    frame_outline_bind_groups: &FrameOutlineBindGroups,
    clamped_viewport: Option<(u32, u32, u32, u32)>,
    oit_accum_view: &wgpu::TextureView,
    oit_weight_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
) {
    let load_color_attachment = |view| {
        Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Transparent Outline Pass"),
        color_attachments: &[load_color_attachment(oit_accum_view), load_color_attachment(oit_weight_view)],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    set_pass_viewport(&mut pass, clamped_viewport);
    pass.set_pipeline(&pipelines.transparent_outline_render_pipeline);
    pass.set_bind_group(0, &frame_outline_bind_groups.reads_nearest_edge_bind_groups[final_nearest_edge_index()], &[]);
    pass.draw(0..3, 0..1);
}
