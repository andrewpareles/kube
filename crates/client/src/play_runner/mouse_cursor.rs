use crate::simple_renderer::decode_texture_data_to_mip_data;
use game_types::data_types::TextureData;
use image::{
    DynamicImage, ImageFormat, Rgba, RgbaImage,
    imageops::{self, FilterType},
};
use shared::game::jsRuntime::{BuiltinMouseCursor, MouseCursor, MouseCursorImage};
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use winit::window::Window;

// A script picks the mouse cursor, and its picture is made here. Each OS shows it in its own file.
#[cfg(target_os = "macos")]
mod macos_mouse_cursor;
#[cfg(target_os = "macos")]
pub use self::macos_mouse_cursor::{MOUSE_CURSOR_PIXELS_PER_POINT_LIST, keep_window_mouse_cursor_shown, set_window_mouse_cursor};
#[cfg(target_os = "windows")]
mod windows_mouse_cursor;
#[cfg(target_os = "windows")]
pub use self::windows_mouse_cursor::{MOUSE_CURSOR_PIXELS_PER_POINT_LIST, keep_window_mouse_cursor_shown, set_window_mouse_cursor};

// The cursors built into the game, drawn by assets/cursors/draw_cursors.mjs.
const BUILTIN_MOUSE_CURSOR_PIXELS_PER_POINT: f32 = 8.0;
const ARROW_PNG_BYTES: &[u8] = include_bytes!("../../../../assets/cursors/arrow.png");
const POINTER_PNG_BYTES: &[u8] = include_bytes!("../../../../assets/cursors/pointer.png");
const CROSSHAIR_PNG_BYTES: &[u8] = include_bytes!("../../../../assets/cursors/crosshair.png");

// A mouse cursor's picture made for the window, once at each number of pixels per point.
pub struct MouseCursorPicture {
    pub width_points: f32,
    pub height_points: f32,
    pub hotspot_x_points: f32,
    pub hotspot_y_points: f32,
    pub rgba_images: Vec<RgbaImage>,
}

// Decodes the cursor's picture and resizes it for each number of pixels per point. Colors are weighted by their alpha
// while resizing, or see-through pixels would darken the picture's edges. A picture from a texture needs that texture.
pub fn make_mouse_cursor_picture(mouse_cursor: MouseCursor, texture_data: Option<&TextureData>) -> anyhow::Result<MouseCursorPicture> {
    let (rgba_image, width_points, height_points, hotspot_x, hotspot_y) = match mouse_cursor {
        MouseCursor::Builtin(builtin_mouse_cursor) => {
            let (png_bytes, hotspot_x, hotspot_y) = match builtin_mouse_cursor {
                BuiltinMouseCursor::Default => (ARROW_PNG_BYTES, 0.1421, 0.0119),
                BuiltinMouseCursor::Pointer => (POINTER_PNG_BYTES, 0.3934, 0.0123),
                BuiltinMouseCursor::Crosshair => (CROSSHAIR_PNG_BYTES, 0.5, 0.5),
            };
            let rgba_image = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)?.into_rgba8();
            let width_points = rgba_image.width() as f32 / BUILTIN_MOUSE_CURSOR_PIXELS_PER_POINT;
            let height_points = rgba_image.height() as f32 / BUILTIN_MOUSE_CURSOR_PIXELS_PER_POINT;
            (rgba_image, width_points, height_points, hotspot_x, hotspot_y)
        }
        MouseCursor::Image(MouseCursorImage {
            width,
            height,
            hotspotX,
            hotspotY,
            ..
        }) => {
            let texture_data = texture_data.ok_or_else(|| anyhow::anyhow!("The cursor's texture hasn't arrived"))?;
            let rgba8_pixels = decode_texture_data_to_mip_data(texture_data, wgpu::TextureFormat::Rgba8Unorm)?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Texture data has no pixels"))?;
            let rgba_image = RgbaImage::from_raw(texture_data.width, texture_data.height, rgba8_pixels)
                .ok_or_else(|| anyhow::anyhow!("Texture pixels do not fill {}x{}", texture_data.width, texture_data.height))?;
            (rgba_image, width, height, hotspotX, hotspotY)
        }
    };

    let mut weighted_image = DynamicImage::ImageRgba8(rgba_image).into_rgba32f();
    for Rgba([red, green, blue, alpha]) in weighted_image.pixels_mut() {
        *red *= *alpha;
        *green *= *alpha;
        *blue *= *alpha;
    }
    let rgba_images = MOUSE_CURSOR_PIXELS_PER_POINT_LIST
        .iter()
        .map(|pixels_per_point| {
            let width_pixels = (width_points * pixels_per_point).round().max(1.0) as u32;
            let height_pixels = (height_points * pixels_per_point).round().max(1.0) as u32;
            let mut resized_image = imageops::resize(&weighted_image, width_pixels, height_pixels, FilterType::Triangle);
            for Rgba([red, green, blue, alpha]) in resized_image.pixels_mut() {
                if *alpha > 0.0 {
                    *red /= *alpha;
                    *green /= *alpha;
                    *blue /= *alpha;
                }
            }
            DynamicImage::ImageRgba32F(resized_image).into_rgba8()
        })
        .collect();
    Ok(MouseCursorPicture {
        width_points,
        height_points,
        hotspot_x_points: hotspot_x * width_points,
        hotspot_y_points: hotspot_y * height_points,
        rgba_images,
    })
}

// ------------------------------ other platforms ------------------------------
// Other systems only build the client for headless playtest players, which have no window to show a pointer in.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const MOUSE_CURSOR_PIXELS_PER_POINT_LIST: &[f32] = &[];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn set_window_mouse_cursor(_window: &Window, _mouse_cursor_picture: Option<MouseCursorPicture>) {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn keep_window_mouse_cursor_shown() {}
