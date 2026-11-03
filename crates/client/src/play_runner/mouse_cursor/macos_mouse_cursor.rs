use super::MouseCursorPicture;
use objc2::{
    AnyThread, ffi,
    rc::Retained,
    runtime::{AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSCursor, NSDeviceRGBColorSpace, NSImage, NSView};
use objc2_foundation::{NSPoint, NSSize};
use std::cell::RefCell;
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

// Every Mac display draws at one or two pixels per point.
pub const MOUSE_CURSOR_PIXELS_PER_POINT_LIST: &[f32] = &[1.0, 2.0];

// macOS shows the cursor that the window's view keeps for its whole area, and WebKit never changes the cursor while a
// view keeps one, so the page can't fight it. winit makes custom cursors at one pixel per point, which is blurry on
// Retina, so the game makes its own cursor with a picture for each scale, and winit's view keeps that one instead.
thread_local! {
    static WINDOW_MOUSE_CURSOR: RefCell<Retained<NSCursor>> = RefCell::new(NSCursor::arrowCursor());
}

extern "C-unwind" fn reset_cursor_rects(view: &NSView, _selector: Sel) {
    WINDOW_MOUSE_CURSOR.with_borrow(|window_mouse_cursor| view.addCursorRect_cursor(view.bounds(), window_mouse_cursor));
}

// None puts back the system's arrow.
pub fn set_window_mouse_cursor(window: &Window, mouse_cursor_picture: Option<MouseCursorPicture>) {
    let Ok(window_handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(app_kit_window_handle) = window_handle.as_raw() else {
        return;
    };
    let view: &NSView = unsafe { app_kit_window_handle.ns_view.cast().as_ref() };
    let window_mouse_cursor = match mouse_cursor_picture {
        None => NSCursor::arrowCursor(),
        Some(MouseCursorPicture {
            width_points,
            height_points,
            hotspot_x_points,
            hotspot_y_points,
            rgba_images,
        }) => {
            // Each picture is one representation of the same image in points, and macOS draws the one that fits the display.
            let size = NSSize::new(width_points as f64, height_points as f64);
            let image = NSImage::initWithSize(NSImage::alloc(), size);
            for rgba_image in &rgba_images {
                let Some(bitmap) = (unsafe {
                    NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                        NSBitmapImageRep::alloc(),
                        std::ptr::null_mut(),
                        rgba_image.width() as isize,
                        rgba_image.height() as isize,
                        8,
                        4,
                        true,
                        false,
                        NSDeviceRGBColorSpace,
                        NSBitmapFormat::AlphaNonpremultiplied,
                        rgba_image.width() as isize * 4,
                        32,
                    )
                }) else {
                    log::warn!("Failed to make a {}x{} bitmap for the mouse cursor", rgba_image.width(), rgba_image.height());
                    return;
                };
                unsafe { std::slice::from_raw_parts_mut(bitmap.bitmapData(), rgba_image.as_raw().len()) }.copy_from_slice(rgba_image.as_raw());
                bitmap.setSize(size);
                image.addRepresentation(&bitmap);
            }
            NSCursor::initWithImage_hotSpot(NSCursor::alloc(), &image, NSPoint::new(hotspot_x_points as f64, hotspot_y_points as f64))
        }
    };
    WINDOW_MOUSE_CURSOR.set(window_mouse_cursor);
    unsafe {
        ffi::class_replaceMethod(
            ffi::object_getClass(view as *const NSView as *const AnyObject) as *mut _,
            sel!(resetCursorRects),
            std::mem::transmute::<extern "C-unwind" fn(&NSView, Sel), Imp>(reset_cursor_rects),
            c"v@:".as_ptr(),
        )
    };
    if let Some(ns_window) = view.window() {
        ns_window.invalidateCursorRectsForView(view);
    }
}

// Other apps can change the cursor while it is over the window, like a screen recorder's overlay does at its edge, and
// AppKit only sets the window's cursor when the mouse comes in. So the game sets it again whenever the mouse moves.
pub fn keep_window_mouse_cursor_shown() {
    WINDOW_MOUSE_CURSOR.with_borrow(|window_mouse_cursor| {
        if Retained::as_ptr(&NSCursor::currentCursor()) == Retained::as_ptr(window_mouse_cursor) {
            window_mouse_cursor.set();
        }
    });
}
