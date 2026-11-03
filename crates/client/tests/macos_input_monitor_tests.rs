#![cfg(target_os = "macos")]

use client::app_runner::{MacosEventRoute, MacosInputState, WindowInput, route_macos_event};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::MouseButton,
    keyboard::KeyCode,
};

// The game reads input from AppKit events before the play webview gets them, so these check that each event
// turns into what winit would have reported, and that a key press goes to the game or to the page, never both.

const WINDOW_NUMBER: isize = 42;
const WINDOW_SIZE: PhysicalSize<u32> = PhysicalSize::new(800, 600);
const SCALE_FACTOR: f64 = 2.0;
const MAC_KEY_CODE_W: u16 = 0x0D;
const MAC_KEY_CODE_SLASH: u16 = 0x2C;
const MAC_KEY_CODE_ESCAPE: u16 = 0x35;
const MAC_KEY_CODE_TAB: u16 = 0x30;
const MAC_KEY_CODE_SHIFT_LEFT: u16 = 0x38;
const MAC_KEY_CODE_SHIFT_RIGHT: u16 = 0x3C;
const MAC_KEY_CODE_COMMAND_LEFT: u16 = 0x37;
// NX_DEVICELSHIFTKEYMASK, NX_DEVICERSHIFTKEYMASK and NX_DEVICELCMDKEYMASK
const DEVICE_FLAG_SHIFT_LEFT: usize = 0x2;
const DEVICE_FLAG_SHIFT_RIGHT: usize = 0x4;
const DEVICE_FLAG_COMMAND_LEFT: usize = 0x8;

fn create_key_event(event_type: NSEventType, mac_key_code: u16, characters: &str, modifier_flags: NSEventModifierFlags, is_repeat: bool) -> Retained<NSEvent> {
    let characters = NSString::from_str(characters);
    let key_event: Option<Retained<NSEvent>> = unsafe {
        msg_send![class!(NSEvent), keyEventWithType: event_type, location: NSPoint::new(0.0, 0.0), modifierFlags: modifier_flags, timestamp: 0.0f64, windowNumber: WINDOW_NUMBER, context: std::ptr::null_mut::<AnyObject>(), characters: &*characters, charactersIgnoringModifiers: &*characters, isARepeat: is_repeat, keyCode: mac_key_code]
    };
    key_event.unwrap()
}

fn create_mouse_event(event_type: NSEventType, location: NSPoint, window_number: isize) -> Retained<NSEvent> {
    let mouse_event: Option<Retained<NSEvent>> = unsafe {
        msg_send![class!(NSEvent), mouseEventWithType: event_type, location: location, modifierFlags: NSEventModifierFlags(0), timestamp: 0.0f64, windowNumber: window_number, context: std::ptr::null_mut::<AnyObject>(), eventNumber: 0isize, clickCount: 1isize, pressure: 1.0f32]
    };
    mouse_event.unwrap()
}

fn route(event: &NSEvent, input_state: &mut MacosInputState) -> MacosEventRoute {
    route_macos_event(event, WINDOW_NUMBER, WINDOW_SIZE, SCALE_FACTOR, input_state)
}

fn to_game_only(window_inputs: Vec<WindowInput>) -> MacosEventRoute {
    MacosEventRoute {
        window_inputs,
        should_page_get_event: false,
    }
}

fn to_page(window_inputs: Vec<WindowInput>) -> MacosEventRoute {
    MacosEventRoute {
        window_inputs,
        should_page_get_event: true,
    }
}

fn key(key_code: KeyCode, is_pressed: bool) -> WindowInput {
    WindowInput::Key { key_code, is_pressed }
}

// WebKit hides the pointer after every key press the page handles, so game key presses never reach the page.
#[test]
fn game_key_presses_reach_only_the_game_once_per_press() {
    let mut input_state = MacosInputState::default();
    let no_flags = NSEventModifierFlags(0);
    assert_eq!(
        route(&create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_W, "w", no_flags, false), &mut input_state),
        to_game_only(vec![key(KeyCode::KeyW, true)])
    );
    assert_eq!(
        route(&create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_W, "w", no_flags, true), &mut input_state),
        to_game_only(vec![])
    );
    assert_eq!(
        route(&create_key_event(NSEventType::KeyUp, MAC_KEY_CODE_W, "w", no_flags, false), &mut input_state),
        to_page(vec![key(KeyCode::KeyW, false)])
    );
    assert!(input_state.held_key_codes.is_empty());
}

// The page opens the chat on "/", and its report of the new focus comes a frame later, so the keys typed right
// after "/" have to reach the chat already. Releases still reach the game, so a held key can always come back up.
#[test]
fn slash_hands_the_page_the_keyboard_until_the_chat_loses_focus() {
    let mut input_state = MacosInputState::default();
    let no_flags = NSEventModifierFlags(0);
    assert_eq!(
        route(
            &create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_SLASH, "/", no_flags, false),
            &mut input_state
        ),
        to_page(vec![])
    );
    assert!(input_state.is_chat_focused);
    assert_eq!(
        route(&create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_W, "w", no_flags, false), &mut input_state),
        to_page(vec![])
    );
    assert_eq!(
        route(&create_key_event(NSEventType::KeyUp, MAC_KEY_CODE_W, "w", no_flags, false), &mut input_state),
        to_page(vec![key(KeyCode::KeyW, false)])
    );

    input_state.is_chat_focused = false;
    assert_eq!(
        route(&create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_W, "w", no_flags, false), &mut input_state),
        to_game_only(vec![key(KeyCode::KeyW, true)])
    );
}

#[test]
fn escape_and_tab_go_to_the_page() {
    let mut input_state = MacosInputState::default();
    let no_flags = NSEventModifierFlags(0);
    assert_eq!(
        route(
            &create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_ESCAPE, "\u{1b}", no_flags, false),
            &mut input_state
        ),
        to_page(vec![])
    );
    assert_eq!(
        route(
            &create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_TAB, "\t", no_flags, false),
            &mut input_state
        ),
        to_page(vec![])
    );
    assert!(!input_state.is_chat_focused);
}

// winit hands key releases made while Command is held straight to the window, so the monitor never sees them.
#[test]
fn command_shortcuts_go_to_the_app_and_command_lets_go_of_held_keys() {
    let mut input_state = MacosInputState::default();
    let no_flags = NSEventModifierFlags(0);
    let command_flags = NSEventModifierFlags::Command | NSEventModifierFlags(DEVICE_FLAG_COMMAND_LEFT);
    assert_eq!(
        route(
            &create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_W, "w", command_flags, false),
            &mut input_state
        ),
        to_page(vec![])
    );

    route(&create_key_event(NSEventType::KeyDown, MAC_KEY_CODE_W, "w", no_flags, false), &mut input_state);
    assert_eq!(
        route(
            &create_key_event(NSEventType::FlagsChanged, MAC_KEY_CODE_COMMAND_LEFT, "", command_flags, false),
            &mut input_state
        ),
        to_page(vec![key(KeyCode::SuperLeft, true), key(KeyCode::KeyW, false)])
    );
    assert_eq!(
        route(
            &create_key_event(NSEventType::FlagsChanged, MAC_KEY_CODE_COMMAND_LEFT, "", no_flags, false),
            &mut input_state
        ),
        to_page(vec![key(KeyCode::SuperLeft, false)])
    );
}

// A modifier key only says the modifiers changed, so each side's own flag says whether it went down.
#[test]
fn each_shift_key_is_reported_by_its_own_flag() {
    let mut input_state = MacosInputState::default();
    let mut route_shift = |mac_key_code: u16, device_flags: usize| {
        let modifier_flags = NSEventModifierFlags::Shift | NSEventModifierFlags(device_flags);
        route(
            &create_key_event(NSEventType::FlagsChanged, mac_key_code, "", modifier_flags, false),
            &mut input_state,
        )
    };
    assert_eq!(
        route_shift(MAC_KEY_CODE_SHIFT_LEFT, DEVICE_FLAG_SHIFT_LEFT),
        to_page(vec![key(KeyCode::ShiftLeft, true)])
    );
    assert_eq!(
        route_shift(MAC_KEY_CODE_SHIFT_RIGHT, DEVICE_FLAG_SHIFT_LEFT | DEVICE_FLAG_SHIFT_RIGHT),
        to_page(vec![key(KeyCode::ShiftRight, true)])
    );
    assert_eq!(
        route_shift(MAC_KEY_CODE_SHIFT_LEFT, DEVICE_FLAG_SHIFT_RIGHT),
        to_page(vec![key(KeyCode::ShiftLeft, false)])
    );
}

// AppKit measures from the bottom left of the window in points, and winit from the top left in physical pixels.
#[test]
fn mouse_positions_are_flipped_into_physical_pixels() {
    let mut input_state = MacosInputState::default();
    assert_eq!(
        route(
            &create_mouse_event(NSEventType::RightMouseDown, NSPoint::new(100.0, 50.0), WINDOW_NUMBER),
            &mut input_state
        ),
        to_page(vec![WindowInput::MouseButton {
            button: MouseButton::Right,
            is_pressed: true,
            position: Some(PhysicalPosition::new(200.0, 500.0)),
        }])
    );
    assert_eq!(
        route(
            &create_mouse_event(NSEventType::MouseMoved, NSPoint::new(500.0, 50.0), WINDOW_NUMBER),
            &mut input_state
        ),
        to_page(vec![WindowInput::CursorMoved { position: None }])
    );
    assert_eq!(
        route(
            &create_mouse_event(NSEventType::LeftMouseUp, NSPoint::new(100.0, 50.0), WINDOW_NUMBER + 1),
            &mut input_state
        ),
        to_page(vec![WindowInput::MouseButton {
            button: MouseButton::Left,
            is_pressed: false,
            position: None,
        }])
    );
}
