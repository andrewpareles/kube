// A Windows touchpad's fingers are read directly and turned into what a Mac trackpad does (windows_input_monitor.rs).
// These frames are the shapes a real ASUS precision touchpad sent, in millimeters.

use client::app_runner::{TouchpadFinger, TouchpadFrame, TouchpadGestures, WindowInput};
use winit::dpi::PhysicalPosition;

const POINTER_POSITION: Option<PhysicalPosition<f64>> = Some(PhysicalPosition::new(400.0, 300.0));

fn push_frame(touchpad_gestures: &mut TouchpadGestures, fingers: &[(u32, f32, f32)], is_clicked: bool) -> Vec<WindowInput> {
    let fingers = fingers.iter().map(|&(id, x_mm, y_mm)| TouchpadFinger { id, x_mm, y_mm }).collect();
    touchpad_gestures.push_frame(TouchpadFrame { fingers, is_clicked }, POINTER_POSITION)
}

#[test]
fn two_light_fingers_moving_together_turn_the_camera_until_one_lifts() {
    let mut touchpad_gestures = TouchpadGestures::default();
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0), (1, 70.0, 40.0)], false), vec![]);
    // A small move could still be a tap or the start of a pinch, so nothing turns yet.
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 51.0, 40.0), (1, 71.0, 40.0)], false), vec![]);
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 52.0, 40.0), (1, 72.0, 40.0)], false),
        vec![WindowInput::TouchpadDrag {
            is_dragging: true,
            position: POINTER_POSITION
        }]
    );
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 53.0, 41.0), (1, 73.0, 41.0)], false),
        vec![WindowInput::TouchpadMove { delta_x: 20.0, delta_y: 20.0 }]
    );
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 53.0, 41.0)], false),
        vec![WindowInput::TouchpadDrag {
            is_dragging: false,
            position: POINTER_POSITION
        }]
    );
}

#[test]
fn two_light_fingers_spreading_apart_zoom_instead_of_turning() {
    let mut touchpad_gestures = TouchpadGestures::default();
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0), (1, 70.0, 40.0)], false), vec![]);
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 49.0, 40.0), (1, 71.0, 40.0)], false), vec![]);
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 48.0, 40.0), (1, 72.0, 40.0)], false),
        vec![WindowInput::TouchpadPinch {
            delta: 2.0 / 22.0,
            position: POINTER_POSITION
        }]
    );
    assert_eq!(push_frame(&mut touchpad_gestures, &[], false), vec![]);
}

// Windows never holds the right button down for this, which is why the fingers are read directly.
#[test]
fn two_finger_click_is_a_right_click_held_until_the_touchpad_is_let_go() {
    let mut touchpad_gestures = TouchpadGestures::default();
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0), (1, 70.0, 40.0)], false), vec![]);
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0), (1, 70.0, 40.0)], true),
        vec![WindowInput::TouchpadClick {
            is_pressed: true,
            position: POINTER_POSITION
        }]
    );
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 51.0, 40.0), (1, 71.0, 40.0)], true),
        vec![WindowInput::TouchpadMove { delta_x: 20.0, delta_y: 0.0 }]
    );
    // While the touchpad is held down, the other finger can lift and the drag goes on.
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 51.0, 42.0)], true),
        vec![WindowInput::TouchpadMove { delta_x: 0.0, delta_y: 40.0 }]
    );
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 51.0, 42.0)], false),
        vec![WindowInput::TouchpadClick {
            is_pressed: false,
            position: POINTER_POSITION
        }]
    );
}

#[test]
fn one_finger_click_and_two_finger_tap_are_left_to_windows() {
    let mut touchpad_gestures = TouchpadGestures::default();
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0)], true), vec![]);
    assert_eq!(push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0)], false), vec![]);
    assert_eq!(push_frame(&mut touchpad_gestures, &[(1, 50.0, 40.0), (2, 70.0, 40.0)], false), vec![]);
    assert_eq!(push_frame(&mut touchpad_gestures, &[(1, 50.0, 40.0), (2, 70.0, 40.0)], false), vec![]);
    assert_eq!(push_frame(&mut touchpad_gestures, &[], false), vec![]);
}

#[test]
fn clicking_down_during_a_drag_keeps_it_a_drag() {
    let mut touchpad_gestures = TouchpadGestures::default();
    push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0), (1, 70.0, 40.0)], false);
    push_frame(&mut touchpad_gestures, &[(0, 52.0, 40.0), (1, 72.0, 40.0)], false);
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 53.0, 40.0), (1, 73.0, 40.0)], true),
        vec![WindowInput::TouchpadMove { delta_x: 20.0, delta_y: 0.0 }]
    );
    assert_eq!(
        push_frame(&mut touchpad_gestures, &[(0, 54.0, 40.0), (1, 74.0, 40.0)], false),
        vec![WindowInput::TouchpadMove { delta_x: 20.0, delta_y: 0.0 }]
    );
}

// While two fingers rest on the touchpad, Windows makes up mouse clicks and movement from them, and the
// monitor drops mouse input for as long as this count is two or more.
#[test]
fn finger_count_follows_the_fingers_on_the_touchpad() {
    let mut touchpad_gestures = TouchpadGestures::default();
    assert_eq!(touchpad_gestures.get_finger_count(), 0);
    push_frame(&mut touchpad_gestures, &[(0, 50.0, 40.0), (1, 70.0, 40.0)], true);
    assert_eq!(touchpad_gestures.get_finger_count(), 2);
    push_frame(&mut touchpad_gestures, &[], false);
    assert_eq!(touchpad_gestures.get_finger_count(), 0);
}
