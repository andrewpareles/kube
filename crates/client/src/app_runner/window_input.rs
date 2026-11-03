use winit::{
    dpi::PhysicalPosition,
    event::{MouseButton, MouseScrollDelta},
    keyboard::KeyCode,
};

// The play webview covers the whole window, so winit's own window never hears about keys or the mouse. The
// game reads them from the OS instead, the moment they arrive, so it never waits on the page: on macOS from an
// event monitor (macos_input_monitor.rs), and on Windows from raw input and the cursor position (windows_input_monitor.rs).
// Positions are in physical pixels from the top left of the window, and are None outside the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowInput {
    Key {
        key_code: KeyCode,
        is_pressed: bool,
    },
    MouseButton {
        button: MouseButton,
        is_pressed: bool,
        position: Option<PhysicalPosition<f64>>,
    },
    CursorMoved {
        position: Option<PhysicalPosition<f64>>,
    },
    // A mouse wheel turned, in lines, or on macOS a trackpad scrolled, in pixels. On Windows the touchpad's
    // scrolling goes to the webview, so its fingers are read instead (the Touchpad inputs below).
    Wheel {
        delta: MouseScrollDelta,
        position: Option<PhysicalPosition<f64>>,
    },
    // A Windows touchpad clicked down with two fingers, which is a right click, like on a Mac.
    TouchpadClick {
        is_pressed: bool,
        position: Option<PhysicalPosition<f64>>,
    },
    // Two fingers started or stopped dragging on a touchpad without clicking it down, which turns the camera.
    TouchpadDrag {
        is_dragging: bool,
        position: Option<PhysicalPosition<f64>>,
    },
    // How far the fingers moved during a two-finger click or drag on a Windows touchpad, in the same units as mouse movement.
    TouchpadMove {
        delta_x: f32,
        delta_y: f32,
    },
    // Two light fingers on a Windows touchpad spread apart or pinched together, as a change in scale like a Mac pinch.
    TouchpadPinch {
        delta: f32,
        position: Option<PhysicalPosition<f64>>,
    },
    // The window went to the back. Only Windows reports this here, since on macOS winit's focus events say so.
    FocusLost,
}

// macOS and Windows number mouse buttons the same way.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn get_mouse_button(button_number: u32) -> MouseButton {
    match button_number {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        _ => MouseButton::Other(button_number as u16),
    }
}
