#[cfg(target_os = "macos")]
use super::window_input::{WindowInput, get_mouse_button};
#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventPhase, NSEventType};
#[cfg(target_os = "macos")]
use std::{cell::RefCell, collections::HashSet, ptr::NonNull, rc::Rc, sync::Arc};
#[cfg(target_os = "macos")]
use winit::{
    dpi::{LogicalPosition, PhysicalPosition, PhysicalSize},
    event::{MouseButton, MouseScrollDelta},
    keyboard::{KeyCode, PhysicalKey},
    platform::scancode::PhysicalKeyExtScancode,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

// The play webview covers the window, so AppKit hands every key and mouse event to the page instead of winit.
// This monitor sees each event first, when AppKit takes it off the queue (the moment winit used to), and turns
// it into the WindowInput winit would have reported. A key press then goes to the game or to the page, never
// both: the page only gets keys while its chat has focus. WebKit hides the pointer after every key press the
// page handles, so a held game key would otherwise keep the pointer hidden. Everything else goes on to the page.
#[cfg(target_os = "macos")]
pub struct MacosInputMonitor {
    event_monitor: Retained<AnyObject>,
    window_inputs_rc: Rc<RefCell<Vec<WindowInput>>>, // everything that arrived since the app last took it
    input_state_rc: Rc<RefCell<MacosInputState>>,
    _input_block: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>,
}

// What the monitor remembers between events.
#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct MacosInputState {
    pub held_key_codes: HashSet<KeyCode>, // every key reported down and not yet up
    pub is_chat_focused: bool,
}

// What one AppKit event means for the game, and whether the page gets it too.
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq)]
pub struct MacosEventRoute {
    pub window_inputs: Vec<WindowInput>,
    pub should_page_get_event: bool,
}

// A modifier key only reports that the modifiers changed, and these flags say which side is down.
// The values are the NX_DEVICE*KEYMASK flags from IOKit, which winit reads the same way.
#[cfg(target_os = "macos")]
fn get_device_flag_for_modifier_key(key_code: KeyCode) -> Option<usize> {
    let device_flag = match key_code {
        KeyCode::ControlLeft => 0x1,
        KeyCode::ShiftLeft => 0x2,
        KeyCode::ShiftRight => 0x4,
        KeyCode::SuperLeft => 0x8,
        KeyCode::SuperRight => 0x10,
        KeyCode::AltLeft => 0x20,
        KeyCode::AltRight => 0x40,
        KeyCode::ControlRight => 0x2000,
        _ => return None,
    };
    Some(device_flag)
}

// Turns one AppKit event into what winit would have reported for it, and decides whether the page gets it.
#[cfg(target_os = "macos")]
pub fn route_macos_event(
    event: &NSEvent,
    window_number: isize,
    window_size: PhysicalSize<u32>,
    scale_factor: f64,
    input_state: &mut MacosInputState,
) -> MacosEventRoute {
    let to_page = |window_inputs: Vec<WindowInput>| MacosEventRoute {
        window_inputs,
        should_page_get_event: true,
    };
    let event_type = event.r#type();
    // AppKit measures from the bottom left of the window in points.
    let get_position = || {
        if event.windowNumber() != window_number {
            return None;
        }
        let location = event.locationInWindow();
        let position = PhysicalPosition::new(location.x * scale_factor, window_size.height as f64 - location.y * scale_factor);
        let is_inside_window = (0.0..window_size.width as f64).contains(&position.x) && (0.0..window_size.height as f64).contains(&position.y);
        is_inside_window.then_some(position)
    };
    // Other buttons only say which one they are through their number.
    let get_mouse_button_for_event_type = || match event_type {
        NSEventType::LeftMouseDown | NSEventType::LeftMouseUp => MouseButton::Left,
        NSEventType::RightMouseDown | NSEventType::RightMouseUp => MouseButton::Right,
        _ => get_mouse_button(event.buttonNumber() as u32),
    };
    let get_key_code = || match PhysicalKey::from_scancode(event.keyCode() as u32) {
        PhysicalKey::Code(key_code) => Some(key_code),
        PhysicalKey::Unidentified(_) => None,
    };
    match event_type {
        NSEventType::KeyDown => {
            // A key pressed with Command held is a shortcut for the app's menus, and its release never reaches this monitor.
            if event.modifierFlags().contains(NSEventModifierFlags::Command) {
                return to_page(Vec::new());
            }
            // The page opens the chat on "/", so from that key on the page has the keyboard. The page's own report
            // of the new focus comes a frame later, and keys typed before it arrives still belong in the chat.
            let is_slash = event.characters().is_some_and(|characters| characters.to_string() == "/");
            if input_state.is_chat_focused || is_slash {
                input_state.is_chat_focused = true;
                return to_page(Vec::new());
            }
            // The page opens and closes the menu on Escape, and shows the player list while Tab is held.
            if let Some(KeyCode::Escape | KeyCode::Tab) = get_key_code() {
                return to_page(Vec::new());
            }
            let key_code = get_key_code().filter(|_| !event.isARepeat());
            if let Some(key_code) = key_code {
                input_state.held_key_codes.insert(key_code);
            }
            MacosEventRoute {
                window_inputs: key_code.map(|key_code| WindowInput::Key { key_code, is_pressed: true }).into_iter().collect(),
                should_page_get_event: false,
            }
        }
        NSEventType::KeyUp => {
            let Some(key_code) = get_key_code() else {
                return to_page(Vec::new());
            };
            input_state.held_key_codes.remove(&key_code);
            to_page(vec![WindowInput::Key { key_code, is_pressed: false }])
        }
        NSEventType::FlagsChanged => {
            let Some(key_code) = get_key_code() else {
                return to_page(Vec::new());
            };
            let Some(device_flag) = get_device_flag_for_modifier_key(key_code) else {
                return to_page(Vec::new());
            };
            let NSEventModifierFlags(modifier_flag_bits) = event.modifierFlags();
            let is_pressed = modifier_flag_bits & device_flag != 0;
            let mut window_inputs = vec![WindowInput::Key { key_code, is_pressed }];
            // While Command is held, winit hands key releases straight to the window, so they skip this monitor.
            // Keys held when Command goes down are let go now instead of staying stuck.
            if is_pressed && (key_code == KeyCode::SuperLeft || key_code == KeyCode::SuperRight) {
                window_inputs.extend(input_state.held_key_codes.drain().map(|held_key_code| WindowInput::Key {
                    key_code: held_key_code,
                    is_pressed: false,
                }));
            }
            to_page(window_inputs)
        }
        NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown => to_page(vec![WindowInput::MouseButton {
            button: get_mouse_button_for_event_type(),
            is_pressed: true,
            position: get_position(),
        }]),
        NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp => to_page(vec![WindowInput::MouseButton {
            button: get_mouse_button_for_event_type(),
            is_pressed: false,
            position: get_position(),
        }]),
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged | NSEventType::RightMouseDragged | NSEventType::OtherMouseDragged => {
            to_page(vec![WindowInput::CursorMoved { position: get_position() }])
        }
        NSEventType::ScrollWheel => {
            // A trackpad keeps scrolling on its own after the fingers lift. The page's panels still coast with it,
            // but the game throws it away, so the camera stops when the fingers stop.
            if event.momentumPhase() != NSEventPhase::None {
                return to_page(Vec::new());
            }
            // This is the same scroll winit reported when its own window received the events.
            let delta = match event.hasPreciseScrollingDeltas() {
                true => MouseScrollDelta::PixelDelta(LogicalPosition::new(event.scrollingDeltaX(), event.scrollingDeltaY()).to_physical(scale_factor)),
                false => MouseScrollDelta::LineDelta(event.scrollingDeltaX() as f32, event.scrollingDeltaY() as f32),
            };
            let position = get_position();
            let wheel = WindowInput::Wheel { delta, position };
            // A trackpad's two-finger drag says when it begins and ends, and the camera turns in between, with the
            // pointer locked like while the right button is held.
            let window_inputs = match event.phase() {
                NSEventPhase::Began => vec![WindowInput::TouchpadDrag { is_dragging: true, position }, wheel],
                NSEventPhase::Ended | NSEventPhase::Cancelled => vec![wheel, WindowInput::TouchpadDrag { is_dragging: false, position }],
                _ => vec![wheel],
            };
            to_page(window_inputs)
        }
        _ => to_page(Vec::new()),
    }
}

#[cfg(target_os = "macos")]
impl MacosInputMonitor {
    pub fn new(window_arc: Arc<Window>) -> Option<Self> {
        let RawWindowHandle::AppKit(app_kit_window_handle) = window_arc.window_handle().ok()?.as_raw() else {
            return None;
        };
        let ns_view = app_kit_window_handle.ns_view.as_ptr() as *mut AnyObject;
        let ns_window: *mut AnyObject = unsafe { msg_send![ns_view, window] };
        let window_number: isize = unsafe { msg_send![ns_window, windowNumber] };

        let window_inputs_rc = Rc::new(RefCell::new(Vec::new()));
        let window_inputs_rc_for_block = Rc::clone(&window_inputs_rc);
        let input_state_rc = Rc::new(RefCell::new(MacosInputState::default()));
        let input_state_rc_for_block = Rc::clone(&input_state_rc);
        let input_block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event = unsafe { event.as_ref() };
            let MacosEventRoute {
                window_inputs,
                should_page_get_event,
            } = route_macos_event(
                event,
                window_number,
                window_arc.inner_size(),
                window_arc.scale_factor(),
                &mut input_state_rc_for_block.borrow_mut(),
            );
            window_inputs_rc_for_block.borrow_mut().extend(window_inputs);
            // Returning no event keeps AppKit from handing it on.
            match should_page_get_event {
                true => event as *const NSEvent as *mut NSEvent,
                false => std::ptr::null_mut(),
            }
        });
        let event_mask = NSEventMask::KeyDown
            | NSEventMask::KeyUp
            | NSEventMask::FlagsChanged
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseUp
            | NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDragged
            | NSEventMask::ScrollWheel;
        let event_monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(event_mask, &input_block) };
        let Some(event_monitor) = event_monitor else {
            log::warn!("Failed to install macOS input monitor");
            return None;
        };
        Some(Self {
            event_monitor,
            window_inputs_rc,
            input_state_rc,
            _input_block: input_block,
        })
    }

    pub fn take_window_inputs(&self) -> Vec<WindowInput> {
        self.window_inputs_rc.take()
    }

    // The page reports when its chat gains or loses focus, and every new game or splash screen starts without it.
    pub fn set_is_chat_focused(&self, is_chat_focused: bool) {
        self.input_state_rc.borrow_mut().is_chat_focused = is_chat_focused;
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacosInputMonitor {
    fn drop(&mut self) {
        unsafe { NSEvent::removeMonitor(&self.event_monitor) };
    }
}

#[cfg(not(target_os = "macos"))]
use super::window_input::WindowInput;
#[cfg(not(target_os = "macos"))]
use std::sync::Arc;
#[cfg(not(target_os = "macos"))]
use winit::window::Window;

#[cfg(not(target_os = "macos"))]
pub struct MacosInputMonitor {}

#[cfg(not(target_os = "macos"))]
impl MacosInputMonitor {
    pub fn new(_window_arc: Arc<Window>) -> Option<Self> {
        None
    }

    pub fn take_window_inputs(&self) -> Vec<WindowInput> {
        Vec::new()
    }

    pub fn set_is_chat_focused(&self, _is_chat_focused: bool) {}
}
