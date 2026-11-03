use anyhow::{Context, Result};
use client::{
    play_web_view::{HostToWebViewMessage, PLAY_WEB_VIEW_FILES, get_script_gui_roots},
    simple_renderer::decode_texture_data_to_mip_data,
};
use game_types::{
    data_types::{DataId, MeshData, TextureData},
    object_types::ObjectId,
};
use glam::Vec3;
use indexmap::IndexMap;
use render_core::{
    camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix},
    computePropertyValue::{applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime},
    computeSkeletonPose::{SkeletonPose, computeSkeletonPoseOfIdAtTime},
    create_gpu_texture::choose_texture_upload_format,
    render_scene::{build_render_scene, collect_material_texture_data_ids, collect_mesh_data_ids, world_lighting_info_of_render_object_of_id},
    renderable::{Renderable, preview_timeline_start_ms},
    renderer::Renderer,
    renderer_types::RenderObject,
};
use serde::{Deserialize, Serialize};
use shared::env::env_vars::CHROMIUM_PATH;
use shared::game::{
    data_fetcher::DataFetcherArc,
    gui_types::{GuiRenderedRoot, ScriptGuiKey},
    world::World,
};
use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    process::Stdio,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt, process::Command, sync::RwLock};

// Started with this argument, the editor_server binary draws one screenshot and exits.
pub const DRAW_SCREENSHOT_ARG: &str = "draw-screenshot";

const SCREENSHOT_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
// TODO: AI-PICKED-VALUE: A screenshot waits at most one minute for its meshes and textures to download.
const SCREENSHOT_ASSET_TIMEOUT: Duration = Duration::from_secs(60);
// TODO: AI-PICKED-VALUE: A screenshot still drawing after one minute is stuck, so its process is killed.
const SCREENSHOT_DRAW_TIMEOUT: Duration = Duration::from_secs(60);
// TODO: AI-PICKED-VALUE: Chromium takes the picture once the page has had this long, counted in the page's own time,
// which only moves while the page is idle. The page loads nothing but its own files, so React and Tailwind are done well before.
const GUI_VIRTUAL_TIME_BUDGET_MS: u32 = 300;
// TODO: AI-PICKED-VALUE: Chromium still drawing the GUI after half a minute is stuck. This is well under
// SCREENSHOT_DRAW_TIMEOUT, so Chromium is always stopped before the process that started it is.
const GUI_DRAW_TIMEOUT: Duration = Duration::from_secs(30);
// The nobody user.
#[cfg(target_os = "linux")]
const CHROMIUM_UID: u32 = 65534;

// Everything one screenshot draws, sent to the process that draws it.
#[derive(Serialize, Deserialize)]
pub struct ScreenshotRequest {
    render_object_of_id: HashMap<ObjectId, RenderObject>,
    skeleton_pose_of_id: HashMap<ObjectId, SkeletonPose>,
    mesh_data_arc_of_id: HashMap<DataId, Arc<MeshData>>,
    texture_data_arc_of_id: HashMap<DataId, Arc<TextureData>>,
    gui_root_of_script_gui_key: Option<IndexMap<ScriptGuiKey, GuiRenderedRoot>>,
    camera_position: Vec3,
    target: Vec3,
    fov_degrees: f32,
    width: u32,
    height: u32,
}

impl ScreenshotRequest {
    // Posed objects and every mesh and texture they use, with a player's GUI drawn over them when given.
    // Downloads that fail are left out.
    pub async fn new(
        data_fetcher: &DataFetcherArc,
        render_object_of_id: HashMap<ObjectId, RenderObject>,
        skeleton_pose_of_id: HashMap<ObjectId, SkeletonPose>,
        gui_root_of_script_gui_key: Option<IndexMap<ScriptGuiKey, GuiRenderedRoot>>,
        camera_position: Vec3,
        target: Vec3,
        fov_degrees: f32,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let mut texture_data_ids = collect_material_texture_data_ids(&render_object_of_id);
        texture_data_ids.extend(world_lighting_info_of_render_object_of_id(&render_object_of_id).skyboxTextureDataId);
        let (mesh_data_arc_of_id, texture_data_arc_of_id) = tokio::time::timeout(
            SCREENSHOT_ASSET_TIMEOUT,
            data_fetcher.waitForMeshAndTextureData(collect_mesh_data_ids(&render_object_of_id), texture_data_ids),
        )
        .await
        .context("The world's meshes and textures did not finish downloading.")?;
        Ok(Self {
            render_object_of_id,
            skeleton_pose_of_id,
            mesh_data_arc_of_id,
            texture_data_arc_of_id,
            camera_position,
            target,
            fov_degrees,
            width,
            height,
            gui_root_of_script_gui_key,
        })
    }

    // The editor world at the moment its animations start, which is what the browser shows.
    pub async fn of_editor_world(
        world_arc: &RwLock<World>,
        data_fetcher: &DataFetcherArc,
        camera_position: Vec3,
        target: Vec3,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let world_lock = world_arc.read().await;
        let mut render_snapshot = world_lock.getRenderSnapshot();
        let (server_start_ms, client_start_ms) = preview_timeline_start_ms(world_lock.objectOfId.values())?;
        drop(world_lock);
        let property_override_of_path_of_object_id = computePropertyOverridesOfObjectIdAtTime(
            &render_snapshot.property_overrides_of_id,
            &render_snapshot.rotation_base_degrees_of_id,
            server_start_ms,
            Some(client_start_ms),
        );
        applyPropertyOverridesToRenderObjectOfId(
            &mut render_snapshot.render_object_of_id,
            &property_override_of_path_of_object_id,
            &render_snapshot.pivot_offset_of_id,
        );
        let skeleton_pose_of_id = computeSkeletonPoseOfIdAtTime(&render_snapshot.render_object_of_id, server_start_ms, Some(client_start_ms));
        Self::new(
            data_fetcher,
            render_snapshot.render_object_of_id,
            skeleton_pose_of_id,
            None,
            camera_position,
            target,
            DEFAULT_CAMERA_FOV_DEGREES,
            width,
            height,
        )
        .await
    }
}

// Each screenshot draws in its own short-lived process, so running out of memory or a driver crash
// fails the screenshot instead of stopping the editor, and the memory it used comes back when it exits.
pub async fn draw_png_in_child_process(program: &Path, request: &ScreenshotRequest) -> Result<Vec<u8>> {
    let request_bytes = bincode::serialize(request)?;
    let mut child = Command::new(program)
        .arg(DRAW_SCREENSHOT_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().context("The screenshot process has no stdin.")?;
    stdin.write_all(&request_bytes).await?;
    drop(stdin);
    let output = tokio::time::timeout(SCREENSHOT_DRAW_TIMEOUT, child.wait_with_output())
        .await
        .context("The screenshot took too long to draw. Try again.")??;
    anyhow::ensure!(output.status.success(), "The screenshot could not be drawn ({}). Try again.", output.status);
    Ok(output.stdout)
}

// The screenshot process: read one request from stdin, draw it, and write the PNG to stdout.
pub fn draw_png_for_editor() -> Result<()> {
    // Linux kills this process first when memory runs out.
    let _ = std::fs::write("/proc/self/oom_score_adj", "1000");
    let ScreenshotRequest {
        render_object_of_id,
        skeleton_pose_of_id,
        mesh_data_arc_of_id,
        texture_data_arc_of_id,
        gui_root_of_script_gui_key,
        camera_position,
        target,
        fov_degrees,
        width,
        height,
    } = bincode::deserialize_from(std::io::stdin().lock())?;

    let mut renderer = pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await?;
        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await?;
        Renderer::new(device, queue, SCREENSHOT_TEXTURE_FORMAT, width, height)
    })?;
    for (&data_id, texture_data_arc) in &texture_data_arc_of_id {
        let upload_format = choose_texture_upload_format(
            renderer.device().features(),
            texture_data_arc.width,
            texture_data_arc.height,
            texture_data_arc.isSrgb,
            texture_data_arc.hasAlpha,
        );
        let loaded = decode_texture_data_to_mip_data(texture_data_arc, upload_format).and_then(|mip_data| {
            renderer.load_texture_into_gpu(
                data_id,
                &mip_data,
                texture_data_arc.width,
                texture_data_arc.height,
                upload_format,
                texture_data_arc.isSrgb,
            )
        });
        if let Err(error) = loaded {
            eprintln!("Screenshot is drawing without texture {data_id}: {error}");
        }
    }
    for (&data_id, mesh_data_arc) in &mesh_data_arc_of_id {
        renderer.load_mesh_into_gpu(data_id, mesh_data_arc);
    }
    let scene = build_render_scene(&render_object_of_id, &mesh_data_arc_of_id, Some(&skeleton_pose_of_id));

    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("Screenshot Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SCREENSHOT_TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let aspect = width as f32 / height as f32;
    let (_camera_buffer, camera_bind_group) = renderer.create_camera_matrix_buffer_pair(camera_position, target, aspect, fov_degrees);
    let mut frame = renderer.begin_frame(view);
    renderer.render_scene(
        &mut frame,
        &scene,
        None,
        &camera_bind_group,
        target,
        camera_position,
        build_camera_view_proj_matrix(camera_position, target, aspect, fov_degrees),
    );

    // Copied rows are padded to 256 bytes, so the padding is cut off below.
    let row_byte_count = width * 4;
    let padded_row_byte_count = row_byte_count.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("Screenshot Readback Buffer"),
        size: u64::from(padded_row_byte_count) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    frame.encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_byte_count),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    renderer.finish_frame(frame);
    let (map_result_tx, map_result_rx) = mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |map_result| {
        let _ = map_result_tx.send(map_result);
    });
    renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
    map_result_rx.recv()??;
    let pixels: Vec<u8> = buffer
        .slice(..)
        .get_mapped_range()
        .chunks_exact(padded_row_byte_count as usize)
        .flat_map(|padded_row| &padded_row[..row_byte_count as usize])
        .copied()
        .collect();

    let mut image = image::RgbaImage::from_raw(width, height, pixels).context("The screenshot did not fill its image.")?;
    if let Some(gui_root_of_script_gui_key) = &gui_root_of_script_gui_key
        && !gui_root_of_script_gui_key.is_empty()
    {
        image::imageops::overlay(&mut image, &draw_gui_with_chromium(gui_root_of_script_gui_key, width, height)?, 0, 0);
    }
    let mut png_bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;
    std::io::stdout().lock().write_all(&png_bytes)?;
    Ok(())
}

// Players see their GUI through the play webview, so the screenshot draws it with the same page in headless
// Chromium, on a see-through background, from the page's files written to a folder of their own.
fn draw_gui_with_chromium(gui_root_of_script_gui_key: &IndexMap<ScriptGuiKey, GuiRenderedRoot>, width: u32, height: u32) -> Result<image::RgbaImage> {
    let page_directory = tempfile::tempdir()?;
    // The page is told what to show the way the client tells it, from a script file since the page's policy refuses inline scripts.
    let show_gui_script = format!(
        "receiveFromHost({}); receiveFromHost({});",
        serde_json::to_string(&HostToWebViewMessage::ShowPlayScreen)?,
        serde_json::to_string(&HostToWebViewMessage::SetScriptGuiRoots(get_script_gui_roots(gui_root_of_script_gui_key)))?,
    );
    std::fs::write(page_directory.path().join("show-gui.js"), show_gui_script)?;
    for (path, _content_type, bytes) in PLAY_WEB_VIEW_FILES {
        let file_path = page_directory.path().join(path);
        if let Some(parent_directory) = file_path.parent() {
            std::fs::create_dir_all(parent_directory)?;
        }
        match path {
            "index.html" => std::fs::write(
                &file_path,
                std::str::from_utf8(bytes)?.replace("</body>", "<script type=\"module\" src=\"show-gui.js\"></script></body>"),
            )?,
            _ => std::fs::write(&file_path, bytes)?,
        }
    }

    let gui_png_path = page_directory.path().join("gui.png");
    let page_url = reqwest::Url::from_file_path(page_directory.path().join("index.html")).map_err(|()| anyhow::anyhow!("The GUI page has no file URL."))?;
    let mut chromium_command = std::process::Command::new(CHROMIUM_PATH.as_str());
    // The editor server runs as root and holds secrets, and Chromium reads creators' class names, so it runs as the nobody
    // user with none of the editor's environment. It writes its profile and the picture in the page's folder.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::{fs::chown, process::CommandExt};
        chown(page_directory.path(), Some(CHROMIUM_UID), Some(CHROMIUM_UID))?;
        chromium_command.uid(CHROMIUM_UID).gid(CHROMIUM_UID);
    }
    let mut chromium = chromium_command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", page_directory.path())
        .args([
            "--headless".to_string(),
            // Chromium's sandbox needs user namespaces, which we can't count on in the hosted container, and the container has a tiny /dev/shm.
            "--no-sandbox".to_string(),
            "--disable-dev-shm-usage".to_string(),
            "--disable-gpu".to_string(),
            "--no-first-run".to_string(),
            "--hide-scrollbars".to_string(),
            // The page loads its script and fonts from the folder beside it.
            "--allow-file-access-from-files".to_string(),
            format!("--user-data-dir={}", page_directory.path().join("profile").display()),
            "--force-device-scale-factor=1".to_string(),
            "--default-background-color=00000000".to_string(),
            format!("--virtual-time-budget={GUI_VIRTUAL_TIME_BUDGET_MS}"),
            format!("--window-size={width},{height}"),
            format!("--screenshot={}", gui_png_path.display()),
            page_url.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Could not start Chromium at {}.", CHROMIUM_PATH.as_str()))?;
    let deadline = Instant::now() + GUI_DRAW_TIMEOUT;
    let status = loop {
        if let Some(status) = chromium.try_wait()? {
            break status;
        }
        if Instant::now() > deadline {
            let _ = chromium.kill();
            anyhow::bail!("Chromium took too long to draw the GUI.");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    anyhow::ensure!(status.success(), "Chromium could not draw the GUI ({status}).");
    Ok(image::open(&gui_png_path)?.to_rgba8())
}
