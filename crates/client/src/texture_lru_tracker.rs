use anyhow::Result;
use game_types::data_types::DataId;
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

// TODO: AI-PICKED-VALUE: Textures may fill half of the GPU memory the OS says this app should use. The other half is left for
// meshes, shadow maps and render targets.
const TEXTURE_PERCENT_OF_GPU_MEMORY_BUDGET: u64 = 50;
// TODO: AI-PICKED-VALUE: When the OS won't say how much GPU memory this app should use, textures may fill 1 GiB, half of a 2 GiB GPU.
const TEXTURE_BYTE_BUDGET_WITHOUT_GPU_MEMORY_BUDGET: u64 = 1024 * 1024 * 1024;

// Which textures stay on the GPU. They stay until they pass a byte budget, so a character or hat that gets deleted and added back
// (like on respawn) doesn't have to decode its textures again. Past it, the least recently drawn textures that this frame doesn't
// draw are dropped.
pub struct TextureLruTracker {
    byte_budget: u64,
    last_drawn_instant_of_texture_data_id: HashMap<DataId, Instant>,
    byte_count_of_texture_data_id: HashMap<DataId, u64>, // only textures that are on the GPU
}
impl TextureLruTracker {
    pub fn new(byte_budget: u64) -> Self {
        Self {
            byte_budget,
            last_drawn_instant_of_texture_data_id: HashMap::new(),
            byte_count_of_texture_data_id: HashMap::new(),
        }
    }
    pub fn new_from_device(device: &wgpu::Device, adapter: &wgpu::Adapter) -> Self {
        let byte_budget = match get_gpu_memory_budget(device, adapter) {
            Ok(gpu_memory_budget) => gpu_memory_budget / 100 * TEXTURE_PERCENT_OF_GPU_MEMORY_BUDGET,
            Err(error) => {
                log::warn!("Could not read the GPU memory budget, so textures may use {TEXTURE_BYTE_BUDGET_WITHOUT_GPU_MEMORY_BUDGET} bytes: {error}");
                TEXTURE_BYTE_BUDGET_WITHOUT_GPU_MEMORY_BUDGET
            }
        };
        Self::new(byte_budget)
    }
    pub fn set_last_drawn_time(&mut self, drawn_texture_data_ids: &HashSet<DataId>, now: Instant) {
        for texture_data_id in drawn_texture_data_ids {
            self.last_drawn_instant_of_texture_data_id.insert(*texture_data_id, now);
        }
    }
    pub fn add_texture(&mut self, texture_data_id: DataId, byte_count: u64) {
        self.byte_count_of_texture_data_id.insert(texture_data_id, byte_count);
    }
    pub fn get_tracked_data_ids(&self) -> HashSet<DataId> {
        self.byte_count_of_texture_data_id.keys().copied().collect()
    }
    // Removes the least recently drawn textures that this frame doesn't draw until the rest fit in the budget, and returns their ids.
    pub fn remove_textures_over_budget(&mut self, drawn_texture_data_ids: &HashSet<DataId>) -> Vec<DataId> {
        let mut byte_count: u64 = self.byte_count_of_texture_data_id.values().sum();
        if byte_count <= self.byte_budget {
            return Vec::new();
        }
        // A preloaded texture that was never drawn has no last drawn time, and goes first.
        let mut undrawn_textures: Vec<(Option<Instant>, DataId, u64)> = self
            .byte_count_of_texture_data_id
            .iter()
            .filter(|(texture_data_id, _)| !drawn_texture_data_ids.contains(texture_data_id))
            .map(|(texture_data_id, texture_byte_count)| {
                (
                    self.last_drawn_instant_of_texture_data_id.get(texture_data_id).copied(),
                    *texture_data_id,
                    *texture_byte_count,
                )
            })
            .collect();
        undrawn_textures.sort_by_key(|(last_drawn_instant, _, _)| *last_drawn_instant);
        let mut removed_texture_data_ids = Vec::new();
        for (_, texture_data_id, texture_byte_count) in undrawn_textures {
            if byte_count <= self.byte_budget {
                break;
            }
            byte_count -= texture_byte_count;
            self.byte_count_of_texture_data_id.remove(&texture_data_id);
            self.last_drawn_instant_of_texture_data_id.remove(&texture_data_id);
            removed_texture_data_ids.push(texture_data_id);
        }
        removed_texture_data_ids
    }
}

// -------------------------- How much GPU memory the OS says this app should use --------------------------

// On macOS it's Metal's recommended working set.
#[cfg(target_os = "macos")]
fn get_gpu_memory_budget(device: &wgpu::Device, _adapter: &wgpu::Adapter) -> Result<u64> {
    let metal_device = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.ok_or_else(|| anyhow::anyhow!("The GPU device is not a Metal device."))?;
    Ok(metal_device.raw_device().lock().recommended_max_working_set_size())
}
// On Windows it's DXGI's video memory budget for the GPU wgpu picked, which DXGI lists with the same vendor and device ids.
#[cfg(target_os = "windows")]
fn get_gpu_memory_budget(_device: &wgpu::Device, adapter: &wgpu::Adapter) -> Result<u64> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIFactory1,
    };
    use windows::core::Interface;
    let wgpu::AdapterInfo { vendor, device, .. } = adapter.get_info();
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }?;
    let mut adapter_index = 0;
    loop {
        let dxgi_adapter = unsafe { factory.EnumAdapters1(adapter_index) }
            .map_err(|error| anyhow::anyhow!("DXGI has no GPU with vendor id {vendor:#x} and device id {device:#x}: {error}"))?;
        adapter_index += 1;
        let DXGI_ADAPTER_DESC1 { VendorId, DeviceId, .. } = unsafe { dxgi_adapter.GetDesc1() }?;
        if VendorId != vendor || DeviceId != device {
            continue;
        }
        let mut video_memory_info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        unsafe {
            dxgi_adapter
                .cast::<IDXGIAdapter3>()?
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut video_memory_info)
        }?;
        return Ok(video_memory_info.Budget);
    }
}
// The desktop client only ships on macOS and Windows. Other systems build the client for headless playtest players, which never open
// a window.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn get_gpu_memory_budget(_device: &wgpu::Device, _adapter: &wgpu::Adapter) -> Result<u64> {
    Err(anyhow::anyhow!("The game window only opens on macOS and Windows."))
}
