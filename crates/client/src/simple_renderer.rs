use crate::texture_lru_tracker::TextureLruTracker;
use anyhow::Result;
use game_types::{
    data_types::{DataId, DataIds, TextureData},
    object_types::{ObjectId, WorldLightingInfo},
};
use glam::{Mat4, Vec3};
use ktx2_rw::{Ktx2Texture, TranscodeFormat};
use render_core::{
    computeSkeletonPose::SkeletonPose,
    create_gpu_texture::{choose_texture_upload_format, mip_level_byte_count},
    render_scene::{build_render_scene, collect_material_texture_data_ids, collect_mesh_data_ids, world_lighting_info_of_render_object_of_id},
    renderer::{RenderFrame, Renderer, SimpleViewportRegion},
    renderer_types::RenderObject,
};
use shared::game::data_fetcher::DataFetcherArc;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
    time::Instant,
};
use winit::{dpi::PhysicalSize, window::Window};

struct DecodedTextureData {
    mip_data: Vec<Vec<u8>>,
    width: u32,
    height: u32,
    upload_format: wgpu::TextureFormat,
    is_srgb: bool,
}

pub struct SimpleRenderer {
    pub gpu: Renderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    render_scale: f32,                           // 1 draws the world at the window's size, 0.5 at half its width and height
    scaled_world_texture: Option<wgpu::Texture>, // below a render_scale of 1 the world is drawn here, then stretched onto the window
    texture_blitter: wgpu::util::TextureBlitter,
    // The transcode worker sends one result through tx; the renderer stores and polls this rx without blocking.
    texture_data_decode_result_rx: Option<(DataId, mpsc::Receiver<TextureDataDecodeResult>)>,
    failed_texture_data_ids: HashSet<DataId>,
    texture_lru_tracker: TextureLruTracker,
}
impl SimpleRenderer {
    pub async fn new(windowArc: Arc<Window>) -> Result<Self> {
        // ------------  Setup wgpu ------------
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let windowSize = windowArc.inner_size();
        let surface = instance.create_surface(windowArc.clone()).map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: adapter.features() & (wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                label: None,
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;
        let texture_lru_tracker = TextureLruTracker::new_from_device(&device, &adapter);

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|format| format.is_srgb())
            .or_else(|| surface_caps.formats.iter().find(|format| !format.is_srgb()))
            .copied()
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: windowSize.width.max(1),
            height: windowSize.height.max(1),
            // Vsync caps the redraw loop (RedrawRequested unconditionally requests the next redraw),
            // so present() blocks at the display rate instead of spinning the CPU/GPU flat out.
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // 2 lets the CPU build the next frame while the GPU draws this one. With 1, CPU and GPU time add up,
            // and any frame over the vsync budget waits a whole extra refresh (60fps drops to 30fps).
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let texture_blitter = wgpu::util::TextureBlitterBuilder::new(&device, config.format)
            .sample_type(wgpu::FilterMode::Linear)
            .build();
        let gpu = Renderer::new(device, queue, config.format, config.width, config.height)?;
        Ok(Self {
            gpu,
            surface,
            config,
            render_scale: 1.0,
            scaled_world_texture: None,
            texture_blitter,
            texture_data_decode_result_rx: None,
            failed_texture_data_ids: HashSet::new(),
            texture_lru_tracker,
        })
    }
    pub fn resize_wgpu_surface(&mut self, new_size: &PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        // let _ = self.windowArc.request_inner_size(new_size); // !!!this breaks windows builds if uncommented -mat
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(self.gpu.device(), &self.config);
        self.resize_world_targets();
    }
    pub fn set_render_scale(&mut self, render_scale: f32) {
        if render_scale == self.render_scale {
            return;
        }
        self.render_scale = render_scale;
        self.resize_world_targets();
    }
    // Everything the world is drawn into follows the window's size times render_scale.
    fn resize_world_targets(&mut self) {
        let width = ((self.config.width as f32 * self.render_scale) as u32).max(1);
        let height = ((self.config.height as f32 * self.render_scale) as u32).max(1);
        self.gpu.resize(width, height);
        self.scaled_world_texture = (self.render_scale < 1.0).then(|| {
            self.gpu.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("Scaled World Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
    }
    pub fn begin_frame(&mut self) -> Result<(RenderFrame, wgpu::SurfaceTexture)> {
        let surface_texture = match self.surface.get_current_texture() {
            Ok(surface_texture) => surface_texture,
            Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
                self.surface.configure(self.gpu.device(), &self.config);
                self.resize_world_targets();
                return Err(anyhow::anyhow!("SurfaceOutdatedOrLost performed refresh"));
            }
            Err(wgpu::SurfaceError::Timeout) => {
                return Err(anyhow::anyhow!("SurfaceTimeout"));
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err(anyhow::anyhow!("SurfaceOutOfMemory"));
            }
            Err(wgpu::SurfaceError::Other) => {
                return Err(anyhow::anyhow!("SurfaceOtherError"));
            }
        };

        let view = match &self.scaled_world_texture {
            Some(scaled_world_texture) => scaled_world_texture.create_view(&wgpu::TextureViewDescriptor::default()),
            None => surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default()), // new swapchain apparently
        };
        Ok((self.gpu.begin_frame(view), surface_texture))
    }
    pub fn finish_frame(&mut self, mut render_frame: RenderFrame, surface_texture: &wgpu::SurfaceTexture) {
        if let Some(scaled_world_texture) = &self.scaled_world_texture {
            let scaled_world_view = scaled_world_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let surface_view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.texture_blitter
                .copy(self.gpu.device(), &mut render_frame.encoder, &scaled_world_view, &surface_view);
        }
        self.gpu.finish_frame(render_frame);
    }
    fn update_texture_gpu_cache_for_frame(
        &mut self,
        render_object_of_id: &HashMap<ObjectId, RenderObject>,
        data_fetcher: &DataFetcherArc,
        world_lighting_info: WorldLightingInfo,
        starter_texture_data_ids: &mut HashSet<DataId>,
    ) {
        let mut texture_data_ids = collect_material_texture_data_ids(render_object_of_id);
        if let Some(skybox_texture_data_id) = world_lighting_info.skyboxTextureDataId {
            texture_data_ids.insert(skybox_texture_data_id);
        }

        self.texture_lru_tracker.set_last_drawn_time(&texture_data_ids, Instant::now());
        if !self.texture_lru_tracker.remove_textures_over_budget(&texture_data_ids).is_empty() {
            self.gpu.retain_textures(&self.texture_lru_tracker.get_tracked_data_ids());
        }

        // Poll the pending transcode rx before starting another worker or drawing with the texture.
        let texture_data_decode_result = self
            .texture_data_decode_result_rx
            .as_ref()
            .and_then(
                |(texture_data_id, texture_data_decode_result_rx)| match texture_data_decode_result_rx.try_recv() {
                    Ok(result) => Some((*texture_data_id, result)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some((*texture_data_id, Err("Texture data decoder disconnected".to_string()))),
                },
            );
        if let Some((texture_data_id, texture_data_decode_result)) = texture_data_decode_result {
            self.texture_data_decode_result_rx = None;
            match texture_data_decode_result {
                Ok(decoded_texture_data) => {
                    let texture_gpu = self.gpu.load_texture_into_gpu(
                        texture_data_id,
                        &decoded_texture_data.mip_data,
                        decoded_texture_data.width,
                        decoded_texture_data.height,
                        decoded_texture_data.upload_format,
                        decoded_texture_data.is_srgb,
                    );
                    match texture_gpu {
                        Ok(()) => {
                            let byte_count = decoded_texture_data.mip_data.iter().map(|mip| mip.len() as u64).sum();
                            self.texture_lru_tracker.add_texture(texture_data_id, byte_count);
                        }
                        Err(error) => {
                            log::error!("Failed to upload texture data {}: {}", texture_data_id, error);
                            self.failed_texture_data_ids.insert(texture_data_id);
                        }
                    }
                }
                Err(error) => {
                    log::error!("Failed to decode texture data {}: {}", texture_data_id, error);
                    self.failed_texture_data_ids.insert(texture_data_id);
                }
            }
        }

        // A starter texture leaves the list once it's on the GPU or has failed, so it only loads once even if the budget drops it.
        starter_texture_data_ids.retain(|texture_data_id| !self.gpu.has_texture(*texture_data_id) && !self.failed_texture_data_ids.contains(texture_data_id));
        // Starter textures go after the ones on screen, and only once they've downloaded, since the preload paces their downloads.
        let downloaded_starter_texture_data_ids = starter_texture_data_ids
            .iter()
            .copied()
            .filter(|texture_data_id| data_fetcher.getDownloadedTextureData(*texture_data_id).is_some());
        for texture_data_id in texture_data_ids.into_iter().chain(downloaded_starter_texture_data_ids) {
            if self.gpu.has_texture(texture_data_id) {
                continue;
            }
            // A texture is still pending if it hasn't been uploaded yet but hasn't permanently failed either.
            if self.failed_texture_data_ids.contains(&texture_data_id) {
                continue;
            }

            let Some(texture_data_arc) = data_fetcher.getOrFetchTextureData(texture_data_id) else {
                continue;
            };
            if self.texture_data_decode_result_rx.is_some() {
                continue;
            }

            let (texture_data_decode_result_tx, texture_data_decode_result_rx) = mpsc::sync_channel(1);
            self.texture_data_decode_result_rx = Some((texture_data_id, texture_data_decode_result_rx));
            let upload_format = choose_texture_upload_format(
                self.gpu.device().features(),
                texture_data_arc.width,
                texture_data_arc.height,
                texture_data_arc.isSrgb,
                texture_data_arc.hasAlpha,
            );
            // The texture worker performs CPU-only KTX transcoding before the renderer uploads completed mip data.
            std::thread::spawn(move || {
                let texture_data_decode_result = decode_texture_data_to_mip_data(texture_data_arc.as_ref(), upload_format)
                    .map(|mip_data| DecodedTextureData {
                        mip_data,
                        width: texture_data_arc.width,
                        height: texture_data_arc.height,
                        upload_format,
                        is_srgb: texture_data_arc.isSrgb,
                    })
                    .map_err(|error| error.to_string());
                let _ = texture_data_decode_result_tx.send(texture_data_decode_result);
            });
        }
    }
    // ------------------------------ Resolve game objects for the GPU renderer ------------------------------

    pub fn render_objects(
        &mut self,
        render_frame: &mut RenderFrame,
        render_object_of_id: &HashMap<ObjectId, RenderObject>,
        data_fetcher: &DataFetcherArc,
        starter_data_ids: &mut DataIds,         // put on the GPU as they download, without being drawn
        viewport: Option<SimpleViewportRegion>, // if this is None, the viewport is the entire screen
        skeleton_pose_of_id: Option<&HashMap<ObjectId, SkeletonPose>>,
        camera_bind_group: &wgpu::BindGroup,
        shadow_center: Vec3,
        camera_position: Vec3,
        // The same matrix camera_bind_group was built from. The shadow pass draws with the sun's
        // matrix but still has to size constant-size instances against this one.
        camera_view_proj: Mat4,
    ) {
        let world_lighting_info = world_lighting_info_of_render_object_of_id(render_object_of_id);
        self.update_texture_gpu_cache_for_frame(render_object_of_id, data_fetcher, world_lighting_info, &mut starter_data_ids.texture_data_ids);
        // Make sure we created a buffer for each mesh we're about to render!
        let mut mesh_data_of_id = HashMap::new();
        for data_id in collect_mesh_data_ids(render_object_of_id) {
            let Some(mesh_data_arc) = data_fetcher.getOrFetchMeshData(data_id) else {
                continue;
            };
            if !self.gpu.has_mesh(data_id) {
                self.gpu.load_mesh_into_gpu(data_id, &mesh_data_arc);
            }
            mesh_data_of_id.insert(data_id, mesh_data_arc);
        }
        // Starter meshes go on the GPU as they download. Meshes are never dropped from it, so each one leaves the list once it's there.
        for &data_id in starter_data_ids.mesh_data_ids.iter() {
            if self.gpu.has_mesh(data_id) {
                continue;
            }
            let Some(mesh_data_arc) = data_fetcher.getDownloadedMeshData(data_id) else {
                continue;
            };
            self.gpu.load_mesh_into_gpu(data_id, &mesh_data_arc);
        }
        starter_data_ids.mesh_data_ids.retain(|data_id| !self.gpu.has_mesh(*data_id));
        // Desktop games and browser previews resolve draw data through the same CPU conversion.
        let scene = build_render_scene(render_object_of_id, &mesh_data_of_id, skeleton_pose_of_id);
        // The viewport is in window pixels, and the world is drawn at render_scale.
        let viewport = viewport.map(|SimpleViewportRegion { x, y, width, height }| SimpleViewportRegion {
            x: (x as f32 * self.render_scale) as u32,
            y: (y as f32 * self.render_scale) as u32,
            width: (width as f32 * self.render_scale) as u32,
            height: (height as f32 * self.render_scale) as u32,
        });
        self.gpu.render_scene(
            render_frame,
            &scene,
            viewport,
            camera_bind_group,
            shadow_center,
            camera_position,
            camera_view_proj,
        );
    }
    // Render into a separate target, then deliver the PNG directly to this capture's caller.
}

// ------------------------------ helpers ------------------------------

type TextureDataDecodeResult = std::result::Result<DecodedTextureData, String>;

pub fn decode_texture_data_to_mip_data(texture_data: &TextureData, upload_format: wgpu::TextureFormat) -> Result<Vec<Vec<u8>>> {
    let transcode_format = match upload_format {
        wgpu::TextureFormat::Rgba8Unorm => TranscodeFormat::Rgba32,
        wgpu::TextureFormat::Bc1RgbaUnorm => TranscodeFormat::Bc1Rgb,
        wgpu::TextureFormat::Bc7RgbaUnorm => TranscodeFormat::Bc7Rgba,
        wgpu::TextureFormat::Astc {
            block: wgpu::AstcBlock::B4x4,
            channel: wgpu::AstcChannel::Unorm,
        } => TranscodeFormat::Astc_4x4_Rgba,
        other => return Err(anyhow::anyhow!("KTX2 textures cannot be transcoded to {:?}", other)),
    };
    let mut ktx2_texture = Ktx2Texture::from_memory(&texture_data.ktx2Data).map_err(|error| anyhow::anyhow!("Failed to parse KTX2 bytes: {}", error))?;
    ktx2_texture
        .transcode_basis(transcode_format)
        .map_err(|error| anyhow::anyhow!("Failed to transcode KTX2 texture: {}", error))?;

    let mip_level_count = ktx2_texture.levels();
    if mip_level_count == 0 {
        return Err(anyhow::anyhow!("KTX2 texture has no mip levels"));
    }

    let mut mip_data: Vec<Vec<u8>> = Vec::with_capacity(mip_level_count as usize);
    for mip_level in 0..mip_level_count {
        let mip_level_data = ktx2_texture
            .get_image_data(mip_level, 0, 0)
            .map_err(|error| anyhow::anyhow!("Failed to read KTX2 mip level {} data: {}", mip_level, error))?;
        let Some(mip_byte_count) = mip_level_byte_count(upload_format, texture_data.width.max(1), texture_data.height.max(1), mip_level) else {
            return Err(anyhow::anyhow!("Texture byte count overflow while decoding mip level {}", mip_level));
        };
        if mip_level_data.len() < mip_byte_count {
            return Err(anyhow::anyhow!(
                "Decoded KTX2 mip level {} is too small: expected at least {}, got {}",
                mip_level,
                mip_byte_count,
                mip_level_data.len(),
            ));
        }

        mip_data.push(mip_level_data[..mip_byte_count].to_vec());
    }

    if texture_data.mipLevelCount != mip_level_count {
        log::warn!(
            "TextureData mip level metadata mismatch: expected {}, decoded {}",
            texture_data.mipLevelCount,
            mip_level_count
        );
    }

    Ok(mip_data)
}
