use super::MouseCursorPicture;
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{CreateBitmap, CreateCompatibleBitmap, DeleteObject, GetDC, ReleaseDC, SetBitmapBits},
    UI::{
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::{
            CreateIconIndirect, DestroyCursor, GA_ROOT, GetAncestor, GetCursor, GetCursorPos, HCURSOR, HICON, HTCLIENT, ICONINFO, IDC_ARROW, LoadCursorW,
            SetCursor, USER_DEFAULT_SCREEN_DPI, WM_DPICHANGED, WM_SETCURSOR, WindowFromPoint,
        },
    },
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

// Windows draws a cursor at the size of its picture, so there is one for each display scale Windows offers.
pub const MOUSE_CURSOR_PIXELS_PER_POINT_LIST: &[f32] = &[1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 3.5];

// The game window decides the pointer, since the page draws into it without windows of its own (see windows_web_view.rs).
// winit answers the window's question of which pointer to show with its own, so the game answers it first, with a cursor
// made from the picture for the scale of the display the window is on.
struct WindowMouseCursor {
    mouse_cursor_picture: MouseCursorPicture,
    hcursor: HCURSOR,
}

thread_local! {
    static WINDOW_MOUSE_CURSOR: RefCell<Option<WindowMouseCursor>> = const { RefCell::new(None) };
}

// The picture made for the number of pixels per point closest to the display's, as a cursor with see-through edges. None if
// Windows can't make it.
fn make_hcursor(mouse_cursor_picture: &MouseCursorPicture, pixels_per_point: f32) -> Option<HCURSOR> {
    let (picture_pixels_per_point, rgba_image) =
        MOUSE_CURSOR_PIXELS_PER_POINT_LIST
            .iter()
            .zip(&mouse_cursor_picture.rgba_images)
            .min_by(|(pixels_per_point_a, _), (pixels_per_point_b, _)| {
                (*pixels_per_point_a - pixels_per_point)
                    .abs()
                    .total_cmp(&(*pixels_per_point_b - pixels_per_point).abs())
            })?;
    let mut bgra_bytes = rgba_image.as_raw().clone();
    bgra_bytes.chunks_exact_mut(4).for_each(|pixel| pixel.swap(0, 2));
    let (width, height) = (rgba_image.width() as i32, rgba_image.height() as i32);
    unsafe {
        let screen_hdc = GetDC(None);
        let color_hbitmap = CreateCompatibleBitmap(screen_hdc, width, height);
        ReleaseDC(None, screen_hdc);
        SetBitmapBits(color_hbitmap, bgra_bytes.len() as u32, bgra_bytes.as_ptr() as *const _);
        // The color picture's alpha decides what's see-through, so the mask is all ones.
        let mask_bits = vec![0xff_u8; (((width + 15) >> 4) << 1) as usize * height as usize];
        let mask_hbitmap = CreateBitmap(width, height, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let hicon = CreateIconIndirect(&ICONINFO {
            fIcon: false.into(),
            xHotspot: (mouse_cursor_picture.hotspot_x_points * picture_pixels_per_point).round() as u32,
            yHotspot: (mouse_cursor_picture.hotspot_y_points * picture_pixels_per_point).round() as u32,
            hbmMask: mask_hbitmap,
            hbmColor: color_hbitmap,
        });
        let _ = DeleteObject(color_hbitmap.into());
        let _ = DeleteObject(mask_hbitmap.into());
        hicon.ok().map(|HICON(hicon_pointer)| HCURSOR(hicon_pointer))
    }
}

unsafe extern "system" fn show_window_mouse_cursor(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _subclass_id: usize, _ref_data: usize) -> LRESULT {
    match message {
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => {
            if let Some(hcursor) =
                WINDOW_MOUSE_CURSOR.with_borrow(|window_mouse_cursor| window_mouse_cursor.as_ref().map(|window_mouse_cursor| window_mouse_cursor.hcursor))
            {
                unsafe { SetCursor(Some(hcursor)) };
                return LRESULT(1);
            }
        }
        // A display with another scale gets the picture made for it.
        WM_DPICHANGED => WINDOW_MOUSE_CURSOR.with_borrow_mut(|window_mouse_cursor| {
            let Some(WindowMouseCursor { mouse_cursor_picture, hcursor }) = window_mouse_cursor else {
                return;
            };
            let Some(new_hcursor) = make_hcursor(mouse_cursor_picture, ((wparam.0 >> 16) & 0xffff) as f32 / USER_DEFAULT_SCREEN_DPI as f32) else {
                return;
            };
            let old_hcursor = std::mem::replace(hcursor, new_hcursor);
            if unsafe { GetCursor() } == old_hcursor {
                unsafe { SetCursor(Some(new_hcursor)) };
            }
            let _ = unsafe { DestroyCursor(old_hcursor) };
        }),
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

// None puts back winit's arrow.
pub fn set_window_mouse_cursor(window: &Window, mouse_cursor_picture: Option<MouseCursorPicture>) {
    let Ok(window_handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32_window_handle.hwnd.get() as *mut std::ffi::c_void);
    let window_mouse_cursor = mouse_cursor_picture.and_then(|mouse_cursor_picture| {
        let hcursor = make_hcursor(&mouse_cursor_picture, window.scale_factor() as f32)?;
        Some(WindowMouseCursor { mouse_cursor_picture, hcursor })
    });
    let hcursor = window_mouse_cursor.as_ref().map(|window_mouse_cursor| window_mouse_cursor.hcursor);
    let old_window_mouse_cursor = WINDOW_MOUSE_CURSOR.replace(window_mouse_cursor);
    if !unsafe { SetWindowSubclass(hwnd, Some(show_window_mouse_cursor), 0, 0) }.as_bool() {
        log::warn!("Failed to let the game decide the window's pointer");
    }
    // The pointer changes now if it's over the window, instead of on the next mouse move.
    let mut cursor_point = POINT::default();
    if unsafe { GetCursorPos(&mut cursor_point) }.is_ok() && unsafe { GetAncestor(WindowFromPoint(cursor_point), GA_ROOT) } == hwnd {
        let shown_hcursor = hcursor.or_else(|| unsafe { LoadCursorW(None, IDC_ARROW) }.ok());
        unsafe { SetCursor(shown_hcursor) };
    }
    if let Some(WindowMouseCursor { hcursor, .. }) = old_window_mouse_cursor {
        let _ = unsafe { DestroyCursor(hcursor) };
    }
}

// Windows asks for the cursor on every mouse move, so it never needs setting again.
pub fn keep_window_mouse_cursor_shown() {}
