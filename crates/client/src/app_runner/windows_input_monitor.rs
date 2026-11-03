// The play webview covers the window, and on Windows that leaves the game three problems, which this monitor solves:
// - Windows does not count mouse input over the webview as this app's, since the webview's windows belong to the
//   browser process. So the app asks for raw input even while it is not in front (DeviceEvents::Always), and the
//   monitor keeps only what arrives while the window is in front.
// - A precision touchpad never holds the right button down for a two-finger click, and a light two-finger drag goes
//   to the webview as scrolling. So the monitor reads the touchpad's fingers from its own raw input and turns them
//   into what a Mac trackpad does: a two-finger click is a right click, two fingers moving together turn the camera,
//   and two fingers spreading apart or pinching zoom.
// - While two fingers rest on the touchpad, Windows makes up mouse clicks and movement from them. The fingers are
//   read directly instead, so that made up mouse input is dropped.
// The webview also holds keyboard focus, so winit's focus events cannot say when the window goes to the back, and
// the monitor watches for that itself.

use super::window_input::WindowInput;
#[cfg(target_os = "windows")]
use super::window_input::get_mouse_button;
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::{cell::Cell, cell::RefCell, collections::HashMap, ffi::c_void};
#[cfg(target_os = "windows")]
use windows::Win32::{
    Devices::HumanInterfaceDevice::{
        HID_USAGE_DIGITIZER_TIP_SWITCH, HID_USAGE_DIGITIZER_TOUCH_PAD, HID_USAGE_GENERIC_X, HID_USAGE_GENERIC_Y, HID_USAGE_PAGE_BUTTON,
        HID_USAGE_PAGE_DIGITIZER, HID_USAGE_PAGE_GENERIC, HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, HidP_GetButtonCaps, HidP_GetCaps,
        HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input, PHIDP_PREPARSED_DATA,
    },
    Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::{
        Input::{
            GetRawInputData, GetRawInputDeviceInfoW, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER, RID_INPUT, RIDI_PREPARSEDDATA,
            RIM_TYPEHID, RegisterRawInputDevices,
        },
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{ClipCursor, GetCursorPos, GetForegroundWindow, WM_INPUT},
    },
};
use winit::dpi::PhysicalPosition;
use winit::window::Window;
#[cfg(target_os = "windows")]
use winit::{
    event::{DeviceEvent, ElementState, RawKeyEvent},
    keyboard::PhysicalKey,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
};

// TODO: AI-PICKED-VALUE: One millimeter of finger movement turns the camera as far as 20 counts of mouse movement do. It felt right on an ASUS precision touchpad.
const TOUCHPAD_MOUSE_COUNTS_PER_MM: f32 = 20.0;
// TODO: AI-PICKED-VALUE: Two light fingers have to move 1.5 mm, together or apart, before they count as turning or zooming, so a two-finger tap does neither.
const TWO_FINGER_GESTURE_START_MM: f32 = 1.5;

// ---------------------------------------- touchpad gestures ----------------------------------------
// This part has no Windows calls, so it builds and is tested on every platform.

// One finger on the touchpad, in millimeters from the pad's top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchpadFinger {
    pub id: u32,
    pub x_mm: f32,
    pub y_mm: f32,
}

// What the touchpad reported at one moment: the fingers on it, palms left out, and whether it is clicked down.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TouchpadFrame {
    pub fingers: Vec<TouchpadFinger>,
    pub is_clicked: bool,
}

// What the fingers are doing. Only one gesture runs at a time, so clicking down during a drag keeps it a drag.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum TouchpadGesture {
    #[default]
    None,
    Click,
    // two light fingers that have not moved far enough yet to tell turning apart from zooming
    Undecided {
        moved_mm: f32,
        spread_change_mm: f32,
    },
    Drag,
    Pinch,
}

// Turns touchpad frames into window input, one frame at a time.
#[derive(Default)]
pub struct TouchpadGestures {
    previous_frame: TouchpadFrame,
    gesture: TouchpadGesture,
}

impl TouchpadGestures {
    pub fn get_finger_count(&self) -> usize {
        self.previous_frame.fingers.len()
    }

    // The position is where the pointer is, which stays put during these gestures.
    pub fn push_frame(&mut self, frame: TouchpadFrame, position: Option<PhysicalPosition<f64>>) -> Vec<WindowInput> {
        let previous_frame = std::mem::replace(&mut self.previous_frame, frame);
        let frame = &self.previous_frame;

        // The fingers that stayed down since the last frame, each with where it was then.
        let finger_pairs: Vec<(&TouchpadFinger, &TouchpadFinger)> = frame
            .fingers
            .iter()
            .filter_map(|finger| Some((finger, previous_frame.fingers.iter().find(|previous_finger| previous_finger.id == finger.id)?)))
            .collect();

        // How far those fingers moved, on average, in the units of mouse movement.
        let finger_pair_count = finger_pairs.len().max(1) as f32;
        let move_x_mm = finger_pairs
            .iter()
            .map(|(finger, previous_finger)| finger.x_mm - previous_finger.x_mm)
            .sum::<f32>()
            / finger_pair_count;
        let move_y_mm = finger_pairs
            .iter()
            .map(|(finger, previous_finger)| finger.y_mm - previous_finger.y_mm)
            .sum::<f32>()
            / finger_pair_count;
        let touchpad_move = (move_x_mm != 0.0 || move_y_mm != 0.0).then_some(WindowInput::TouchpadMove {
            delta_x: move_x_mm * TOUCHPAD_MOUSE_COUNTS_PER_MM,
            delta_y: move_y_mm * TOUCHPAD_MOUSE_COUNTS_PER_MM,
        });

        // How much further apart the same two fingers got, when exactly two stayed down.
        let get_spread_mm = |finger_a: &TouchpadFinger, finger_b: &TouchpadFinger| (finger_a.x_mm - finger_b.x_mm).hypot(finger_a.y_mm - finger_b.y_mm);
        let two_finger_spreads_mm = match finger_pairs.as_slice() {
            [(finger_a, previous_finger_a), (finger_b, previous_finger_b)] if frame.fingers.len() == 2 => {
                Some((get_spread_mm(finger_a, finger_b), get_spread_mm(previous_finger_a, previous_finger_b)))
            }
            _ => None,
        };
        let is_two_fingers_moving = two_finger_spreads_mm.is_some();
        let (spread_change_mm, previous_spread_mm) = match two_finger_spreads_mm {
            Some((spread_mm, previous_spread_mm)) => (spread_mm - previous_spread_mm, previous_spread_mm),
            None => (0.0, 0.0),
        };

        let mut window_inputs = Vec::new();
        match self.gesture {
            // A click that starts with two fingers down is a right click, held until the touchpad is let go, even if a finger lifts.
            TouchpadGesture::Click if frame.is_clicked => window_inputs.extend(touchpad_move),
            TouchpadGesture::Click => {
                self.gesture = TouchpadGesture::None;
                window_inputs.push(WindowInput::TouchpadClick { is_pressed: false, position });
            }
            TouchpadGesture::None | TouchpadGesture::Undecided { .. } if frame.is_clicked && !previous_frame.is_clicked && frame.fingers.len() == 2 => {
                self.gesture = TouchpadGesture::Click;
                window_inputs.push(WindowInput::TouchpadClick { is_pressed: true, position });
            }
            // Two light fingers turn the camera when they move together, and zoom when they spread apart or pinch.
            // Any other number of fingers ends that.
            _ if frame.fingers.len() != 2 => {
                if self.gesture == TouchpadGesture::Drag {
                    window_inputs.push(WindowInput::TouchpadDrag { is_dragging: false, position });
                }
                self.gesture = TouchpadGesture::None;
            }
            TouchpadGesture::None | TouchpadGesture::Undecided { .. } if is_two_fingers_moving && !frame.is_clicked => {
                let (moved_mm, spread_change_sum_mm) = match self.gesture {
                    TouchpadGesture::Undecided { moved_mm, spread_change_mm } => (moved_mm, spread_change_mm),
                    _ => (0.0, 0.0),
                };
                let moved_mm = moved_mm + move_x_mm.hypot(move_y_mm);
                let spread_change_sum_mm = spread_change_sum_mm + spread_change_mm.abs();
                self.gesture = if moved_mm.max(spread_change_sum_mm) < TWO_FINGER_GESTURE_START_MM {
                    TouchpadGesture::Undecided {
                        moved_mm,
                        spread_change_mm: spread_change_sum_mm,
                    }
                } else if spread_change_sum_mm > moved_mm {
                    TouchpadGesture::Pinch
                } else {
                    window_inputs.push(WindowInput::TouchpadDrag { is_dragging: true, position });
                    TouchpadGesture::Drag
                };
            }
            TouchpadGesture::Drag => window_inputs.extend(touchpad_move),
            // The fingers can never be on top of each other, but a bad report must not make the zoom infinite.
            TouchpadGesture::Pinch if is_two_fingers_moving => window_inputs.push(WindowInput::TouchpadPinch {
                delta: spread_change_mm / previous_spread_mm.max(1.0),
                position,
            }),
            _ => {}
        }
        window_inputs
    }
}

// ---------------------------------------- reading the touchpad ----------------------------------------

// HID usages a precision touchpad reports that the windows crate has no names for.
#[cfg(target_os = "windows")]
const HID_USAGE_DIGITIZER_CONFIDENCE: u16 = 0x47; // false when the touchpad thinks the contact is a palm
#[cfg(target_os = "windows")]
const HID_USAGE_DIGITIZER_CONTACT_ID: u16 = 0x51;
#[cfg(target_os = "windows")]
const HID_USAGE_DIGITIZER_CONTACT_COUNT: u16 = 0x54;
#[cfg(target_os = "windows")]
const HID_USAGE_BUTTON_1: u16 = 0x01; // the touchpad's physical click

#[cfg(target_os = "windows")]
fn get_value_cap_usages(value_cap: &HIDP_VALUE_CAPS) -> (u16, u16) {
    match value_cap.IsRange {
        true => unsafe { (value_cap.Anonymous.Range.UsageMin, value_cap.Anonymous.Range.UsageMax) },
        false => unsafe { (value_cap.Anonymous.NotRange.Usage, value_cap.Anonymous.NotRange.Usage) },
    }
}

#[cfg(target_os = "windows")]
fn get_button_cap_usages(button_cap: &HIDP_BUTTON_CAPS) -> (u16, u16) {
    match button_cap.IsRange {
        true => unsafe { (button_cap.Anonymous.Range.UsageMin, button_cap.Anonymous.Range.UsageMax) },
        false => unsafe { (button_cap.Anonymous.NotRange.Usage, button_cap.Anonymous.NotRange.Usage) },
    }
}

// How many millimeters one step of a finger position is. HID sizes are in centimeters (units 0x11) or inches (units
// 0x13), times ten to a 4 bit signed exponent.
#[cfg(target_os = "windows")]
fn get_mm_per_step(value_cap: &HIDP_VALUE_CAPS) -> Option<f32> {
    let mm_per_unit = match value_cap.Units {
        0x11 => 10.0,
        0x13 => 25.4,
        _ => return None,
    };
    let exponent = match value_cap.UnitsExp {
        exponent @ 0..=7 => exponent as i32,
        exponent @ 8..=15 => exponent as i32 - 16,
        exponent => exponent as u8 as i8 as i32,
    };
    let physical_size = (value_cap.PhysicalMax - value_cap.PhysicalMin) as f32 * 10f32.powi(exponent) * mm_per_unit;
    let step_count = (value_cap.LogicalMax - value_cap.LogicalMin) as f32;
    (physical_size > 0.0 && step_count > 0.0).then_some(physical_size / step_count)
}

// A touchpad frame can arrive split across several reports. The first one says how many contacts the frame has.
#[cfg(target_os = "windows")]
struct PendingFrame {
    unread_contact_count: usize,
    frame: TouchpadFrame,
}

// Where one touchpad keeps each finger and its click in its reports, read once from the touchpad itself.
#[cfg(target_os = "windows")]
struct TouchpadLayout {
    preparsed_data: Vec<u64>, // the layout Windows hands out, kept 8-byte aligned
    finger_link_collections: Vec<u16>,
    contact_count_link_collection: u16,
    click_link_collection: u16,
    x_min: i32,
    y_min: i32,
    mm_per_x_step: f32,
    mm_per_y_step: f32,
}

#[cfg(target_os = "windows")]
impl TouchpadLayout {
    // None for a device that is not a precision touchpad Windows knows the layout of.
    fn load(device: HANDLE) -> Option<Self> {
        let mut size = 0u32;
        unsafe { GetRawInputDeviceInfoW(Some(device), RIDI_PREPARSEDDATA, None, &mut size) };
        let mut preparsed_data = vec![0u64; (size as usize).div_ceil(8)];
        let copied_size = unsafe { GetRawInputDeviceInfoW(Some(device), RIDI_PREPARSEDDATA, Some(preparsed_data.as_mut_ptr() as *mut c_void), &mut size) };
        if copied_size == 0 || copied_size == u32::MAX {
            return None;
        }
        let preparsed = PHIDP_PREPARSED_DATA(preparsed_data.as_ptr() as isize);
        let mut caps = HIDP_CAPS::default();
        if unsafe { HidP_GetCaps(preparsed, &mut caps) } != HIDP_STATUS_SUCCESS {
            return None;
        }
        let mut value_cap_count = caps.NumberInputValueCaps;
        let mut value_caps = vec![HIDP_VALUE_CAPS::default(); value_cap_count as usize];
        let _ = unsafe { HidP_GetValueCaps(HidP_Input, value_caps.as_mut_ptr(), &mut value_cap_count, preparsed) };
        value_caps.truncate(value_cap_count as usize);
        let mut button_cap_count = caps.NumberInputButtonCaps;
        let mut button_caps = vec![HIDP_BUTTON_CAPS::default(); button_cap_count as usize];
        let _ = unsafe { HidP_GetButtonCaps(HidP_Input, button_caps.as_mut_ptr(), &mut button_cap_count, preparsed) };
        button_caps.truncate(button_cap_count as usize);

        // Each finger has its own link collection holding its position, and the contact count and click sit outside them.
        let has_usage = |page: u16, usage: u16, cap_page: u16, (usage_min, usage_max): (u16, u16)| cap_page == page && (usage_min..=usage_max).contains(&usage);
        let x_value_caps: Vec<&HIDP_VALUE_CAPS> = value_caps
            .iter()
            .filter(|value_cap| {
                has_usage(
                    HID_USAGE_PAGE_GENERIC,
                    HID_USAGE_GENERIC_X,
                    value_cap.UsagePage,
                    get_value_cap_usages(value_cap),
                )
            })
            .collect();
        let y_value_cap = value_caps.iter().find(|value_cap| {
            has_usage(
                HID_USAGE_PAGE_GENERIC,
                HID_USAGE_GENERIC_Y,
                value_cap.UsagePage,
                get_value_cap_usages(value_cap),
            )
        })?;
        let contact_count_value_cap = value_caps.iter().find(|value_cap| {
            has_usage(
                HID_USAGE_PAGE_DIGITIZER,
                HID_USAGE_DIGITIZER_CONTACT_COUNT,
                value_cap.UsagePage,
                get_value_cap_usages(value_cap),
            )
        })?;
        let click_button_cap = button_caps.iter().find(|button_cap| {
            has_usage(
                HID_USAGE_PAGE_BUTTON,
                HID_USAGE_BUTTON_1,
                button_cap.UsagePage,
                get_button_cap_usages(button_cap),
            )
        })?;
        let x_value_cap = x_value_caps.first()?;
        Some(Self {
            finger_link_collections: x_value_caps.iter().map(|value_cap| value_cap.LinkCollection).collect(),
            contact_count_link_collection: contact_count_value_cap.LinkCollection,
            click_link_collection: click_button_cap.LinkCollection,
            x_min: x_value_cap.LogicalMin,
            y_min: y_value_cap.LogicalMin,
            mm_per_x_step: get_mm_per_step(x_value_cap)?,
            mm_per_y_step: get_mm_per_step(y_value_cap)?,
            preparsed_data,
        })
    }

    fn get_value(&self, usage_page: u16, link_collection: u16, usage: u16, report: &[u8]) -> Option<u32> {
        let preparsed = PHIDP_PREPARSED_DATA(self.preparsed_data.as_ptr() as isize);
        let mut value = 0u32;
        let status = unsafe { HidP_GetUsageValue(HidP_Input, usage_page, Some(link_collection), usage, &mut value, preparsed, report) };
        (status == HIDP_STATUS_SUCCESS).then_some(value)
    }

    // The on/off usages of a page that are on, like a finger's tip switch.
    fn get_on_usages(&self, usage_page: u16, link_collection: u16, report: &mut [u8]) -> Vec<u16> {
        let preparsed = PHIDP_PREPARSED_DATA(self.preparsed_data.as_ptr() as isize);
        let mut usages = [0u16; 16];
        let mut usage_count = usages.len() as u32;
        let status = unsafe {
            HidP_GetUsages(
                HidP_Input,
                usage_page,
                Some(link_collection),
                usages.as_mut_ptr(),
                &mut usage_count,
                preparsed,
                report,
            )
        };
        if status != HIDP_STATUS_SUCCESS {
            return Vec::new();
        }
        usages[..usage_count as usize].to_vec()
    }

    // Returns a frame once all of its contacts have arrived. Contacts that are lifting or that the touchpad thinks are
    // palms are left out of its fingers.
    fn read_report(&self, report: &mut [u8], pending_frame: &mut Option<PendingFrame>) -> Option<TouchpadFrame> {
        let contact_count = self.get_value(
            HID_USAGE_PAGE_DIGITIZER,
            self.contact_count_link_collection,
            HID_USAGE_DIGITIZER_CONTACT_COUNT,
            report,
        )?;
        if contact_count > 0 {
            let is_clicked = self
                .get_on_usages(HID_USAGE_PAGE_BUTTON, self.click_link_collection, report)
                .contains(&HID_USAGE_BUTTON_1);
            *pending_frame = Some(PendingFrame {
                unread_contact_count: contact_count as usize,
                frame: TouchpadFrame {
                    fingers: Vec::new(),
                    is_clicked,
                },
            });
        }
        // Each report has room for a few contacts, and only as many as are still unread are real.
        let PendingFrame { unread_contact_count, frame } = pending_frame.as_mut()?;
        for &link_collection in &self.finger_link_collections {
            if *unread_contact_count == 0 {
                break;
            }
            let Some(x) = self.get_value(HID_USAGE_PAGE_GENERIC, link_collection, HID_USAGE_GENERIC_X, report) else {
                continue;
            };
            *unread_contact_count -= 1;
            let on_usages = self.get_on_usages(HID_USAGE_PAGE_DIGITIZER, link_collection, report);
            if !on_usages.contains(&HID_USAGE_DIGITIZER_TIP_SWITCH) || !on_usages.contains(&HID_USAGE_DIGITIZER_CONFIDENCE) {
                continue;
            }
            let y = self.get_value(HID_USAGE_PAGE_GENERIC, link_collection, HID_USAGE_GENERIC_Y, report)?;
            frame.fingers.push(TouchpadFinger {
                id: self.get_value(HID_USAGE_PAGE_DIGITIZER, link_collection, HID_USAGE_DIGITIZER_CONTACT_ID, report)?,
                x_mm: (x as i32 - self.x_min) as f32 * self.mm_per_x_step,
                y_mm: (y as i32 - self.y_min) as f32 * self.mm_per_y_step,
            });
        }
        if *unread_contact_count > 0 {
            return None;
        }
        pending_frame.take().map(|PendingFrame { frame, .. }| frame)
    }
}

// ---------------------------------------- the monitor ----------------------------------------

// Where the pointer is on the window, or None outside it.
#[cfg(target_os = "windows")]
fn get_cursor_position(window: &Window) -> Option<PhysicalPosition<f64>> {
    let mut cursor_point = POINT::default();
    unsafe { GetCursorPos(&mut cursor_point) }.ok()?;
    let window_position = window.inner_position().ok()?;
    let window_size = window.inner_size();
    let position = PhysicalPosition::new((cursor_point.x - window_position.x) as f64, (cursor_point.y - window_position.y) as f64);
    let is_inside_window = (0.0..window_size.width as f64).contains(&position.x) && (0.0..window_size.height as f64).contains(&position.y);
    is_inside_window.then_some(position)
}

// On Windows the webview holds keyboard focus, so winit's focus can't tell, and Windows is asked which window is in front.
#[cfg(target_os = "windows")]
pub fn is_window_in_front(window: &Window) -> bool {
    let Ok(window_handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32_window_handle) = window_handle.as_raw() else {
        return false;
    };
    HWND(win32_window_handle.hwnd.get() as *mut c_void) == unsafe { GetForegroundWindow() }
}

// What the window's subclass keeps between touchpad reports.
#[cfg(target_os = "windows")]
struct TouchpadState {
    window_arc: Arc<Window>,
    layout_of_device: HashMap<isize, Option<TouchpadLayout>>,
    pending_frame: Option<PendingFrame>,
    gestures: TouchpadGestures,
    window_inputs: Vec<WindowInput>, // everything that arrived since the app last took it
}

#[cfg(target_os = "windows")]
impl TouchpadState {
    fn read_raw_input(&mut self, raw_input_handle: HRAWINPUT) {
        let header_size = size_of::<RAWINPUTHEADER>() as u32;
        let mut size = 0u32;
        unsafe { GetRawInputData(raw_input_handle, RID_INPUT, None, &mut size, header_size) };
        // u64s keep the RAWINPUT header aligned.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let copied_size = unsafe { GetRawInputData(raw_input_handle, RID_INPUT, Some(buffer.as_mut_ptr() as *mut c_void), &mut size, header_size) };
        if copied_size == u32::MAX {
            return;
        }
        let raw_input = unsafe { &*(buffer.as_ptr() as *const RAWINPUT) };
        if raw_input.header.dwType != RIM_TYPEHID.0 {
            return;
        }
        let device = raw_input.header.hDevice;
        let Some(layout) = self.layout_of_device.entry(device.0 as isize).or_insert_with(|| TouchpadLayout::load(device)) else {
            return;
        };
        // RAWHID is two u32 sizes, then the reports one after another.
        let (report_size, report_count) = unsafe { (raw_input.data.hid.dwSizeHid as usize, raw_input.data.hid.dwCount as usize) };
        let buffer_bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u8, buffer.len() * size_of::<u64>()) };
        let reports_start = size_of::<RAWINPUTHEADER>() + 2 * size_of::<u32>();
        for report_index in 0..report_count {
            let report_start = reports_start + report_index * report_size;
            let Some(report) = buffer_bytes.get(report_start..report_start + report_size) else {
                return;
            };
            let Some(frame) = layout.read_report(&mut report.to_vec(), &mut self.pending_frame) else {
                continue;
            };
            let position = get_cursor_position(&self.window_arc);
            self.window_inputs.extend(self.gestures.push_frame(frame, position));
        }
    }
}

// winit never sees the touchpad's raw input, since it only asks for the keyboard and mouse, so the game window's
// messages pass through here first.
#[cfg(target_os = "windows")]
unsafe extern "system" fn read_touchpad_messages(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    touchpad_state_pointer: usize,
) -> LRESULT {
    if message == WM_INPUT {
        let touchpad_state = unsafe { &*(touchpad_state_pointer as *const RefCell<TouchpadState>) };
        touchpad_state.borrow_mut().read_raw_input(HRAWINPUT(lparam.0 as *mut c_void));
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(target_os = "windows")]
pub struct WindowsInputMonitor {
    hwnd: HWND,
    touchpad_state_box: Box<RefCell<TouchpadState>>, // the window's subclass holds a pointer to this
    is_window_in_front: Cell<bool>,
}

#[cfg(target_os = "windows")]
impl WindowsInputMonitor {
    pub fn new(window_arc: Arc<Window>) -> Option<Self> {
        let RawWindowHandle::Win32(win32_window_handle) = window_arc.window_handle().ok()?.as_raw() else {
            return None;
        };
        let hwnd = HWND(win32_window_handle.hwnd.get() as *mut c_void);
        let touchpad_state_box = Box::new(RefCell::new(TouchpadState {
            window_arc,
            layout_of_device: HashMap::new(),
            pending_frame: None,
            gestures: TouchpadGestures::default(),
            window_inputs: Vec::new(),
        }));
        // Unlike the mouse's, Windows counts the touchpad's input over the webview as this app's, so it arrives
        // whenever the window is in front, which is the only time the game wants it.
        let touchpad_device = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_DIGITIZER,
            usUsage: HID_USAGE_DIGITIZER_TOUCH_PAD,
            dwFlags: RAWINPUTDEVICE_FLAGS(0),
            hwndTarget: hwnd,
        };
        if let Err(error) = unsafe { RegisterRawInputDevices(&[touchpad_device], size_of::<RAWINPUTDEVICE>() as u32) } {
            log::warn!("Failed to ask for touchpad input: {}", error);
        }
        // Subclass ids only have to differ between subclasses that share a procedure, so 0 is enough.
        let touchpad_state_pointer = touchpad_state_box.as_ref() as *const RefCell<TouchpadState> as usize;
        if !unsafe { SetWindowSubclass(hwnd, Some(read_touchpad_messages), 0, touchpad_state_pointer) }.as_bool() {
            log::warn!("Failed to watch the window for touchpad input");
        }
        Some(Self {
            hwnd,
            touchpad_state_box,
            is_window_in_front: Cell::new(unsafe { GetForegroundWindow() } == hwnd),
        })
    }

    // The touchpad gestures since the app last took them, whether the window went to the back, and where the pointer is.
    pub fn take_window_inputs(&self) -> Vec<WindowInput> {
        let mut touchpad_state = self.touchpad_state_box.borrow_mut();
        let mut window_inputs = std::mem::take(&mut touchpad_state.window_inputs);
        let is_window_in_front = unsafe { GetForegroundWindow() } == self.hwnd;
        if self.is_window_in_front.replace(is_window_in_front) && !is_window_in_front {
            // winit only changes the pointer lock while the window is in front, so the lock is let go here.
            let _ = unsafe { ClipCursor(None) };
            // The touchpad stops reporting once the window is in the back, so its fingers would otherwise never lift.
            touchpad_state.gestures = TouchpadGestures::default();
            touchpad_state.pending_frame = None;
            window_inputs.push(WindowInput::FocusLost);
        }
        // Windows has no event for the cursor moving over the page, so the game reads where it is each frame.
        // While another app is in front, the cursor is over that app even when it is inside the window's rect.
        window_inputs.push(WindowInput::CursorMoved {
            position: get_cursor_position(&touchpad_state.window_arc).filter(|_| is_window_in_front),
        });
        window_inputs
    }

    // Raw input arrives even while another app is in front, and while two fingers rest on the touchpad Windows
    // makes up mouse input from them.
    pub fn should_game_get_device_event(&self, device_event: &DeviceEvent) -> bool {
        if unsafe { GetForegroundWindow() } != self.hwnd {
            return false;
        }
        match device_event {
            DeviceEvent::Key(_) => true,
            _ => self.touchpad_state_box.borrow().gestures.get_finger_count() < 2,
        }
    }

    // Raw input reports every key, button and wheel notch, even while WebView2 has the keyboard.
    pub fn get_window_input_for_device_event(&self, device_event: &DeviceEvent) -> Option<WindowInput> {
        match device_event {
            DeviceEvent::Key(RawKeyEvent {
                physical_key: PhysicalKey::Code(key_code),
                state,
            }) => Some(WindowInput::Key {
                key_code: *key_code,
                is_pressed: *state == ElementState::Pressed,
            }),
            DeviceEvent::Button { button, state } => Some(WindowInput::MouseButton {
                button: get_mouse_button(*button),
                is_pressed: *state == ElementState::Pressed,
                position: get_cursor_position(&self.touchpad_state_box.borrow().window_arc),
            }),
            DeviceEvent::MouseWheel { delta } => Some(WindowInput::Wheel {
                delta: *delta,
                position: get_cursor_position(&self.touchpad_state_box.borrow().window_arc),
            }),
            _ => None,
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsInputMonitor {
    fn drop(&mut self) {
        let _ = unsafe { RemoveWindowSubclass(self.hwnd, Some(read_touchpad_messages), 0) };
    }
}

// ---------------------------------------- other platforms ----------------------------------------

#[cfg(not(target_os = "windows"))]
use winit::event::DeviceEvent;

#[cfg(not(target_os = "windows"))]
pub fn is_window_in_front(window: &Window) -> bool {
    window.has_focus()
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsInputMonitor {}

#[cfg(not(target_os = "windows"))]
impl WindowsInputMonitor {
    pub fn new(_window_arc: Arc<Window>) -> Option<Self> {
        None
    }

    pub fn take_window_inputs(&self) -> Vec<WindowInput> {
        Vec::new()
    }

    pub fn should_game_get_device_event(&self, _device_event: &DeviceEvent) -> bool {
        true
    }

    pub fn get_window_input_for_device_event(&self, _device_event: &DeviceEvent) -> Option<WindowInput> {
        None
    }
}
