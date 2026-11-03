// Turns window key/mouse input into JsCallbackInputs events for scripts, tracking what scripts currently see as held.
// Extracted from PlayRunner so the event semantics (once per physical press, releases on focus loss) are unit-testable.

use shared::game::world::JsCallbackInputs;
use std::collections::HashSet;
use tokio::sync::mpsc;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

// These names must match the KeyName union in kube-ts-api clientInputService.ts. Unsupported keys return None and are never sent to scripts.
fn scriptKeyNameOfKeyCode(key: KeyCode) -> Option<&'static str> {
    let key_name = match key {
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyD => "D",
        KeyCode::KeyE => "E",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyI => "I",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyM => "M",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyP => "P",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyR => "R",
        KeyCode::KeyS => "S",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyW => "W",
        KeyCode::KeyX => "X",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Space => "Space",
        KeyCode::ShiftLeft => "ShiftLeft",
        KeyCode::ShiftRight => "ShiftRight",
        KeyCode::ArrowUp => "ArrowUp",
        KeyCode::ArrowDown => "ArrowDown",
        KeyCode::ArrowLeft => "ArrowLeft",
        KeyCode::ArrowRight => "ArrowRight",
        _ => return None,
    };
    Some(key_name)
}

// These names must match the MouseButtonName union in kube-ts-api clientInputService.ts.
// TODO: expose the camera zoom (as a readable constant) to scripts in the future, instead of raw scroll wheel events.
fn scriptMouseButtonName(button: MouseButton) -> Option<&'static str> {
    match button {
        MouseButton::Left => Some("left"),
        MouseButton::Right => Some("right"),
        _ => None,
    }
}

pub struct ScriptInputTracker {
    js_callback_inputs_tx: mpsc::UnboundedSender<JsCallbackInputs>,
    held_key_names: HashSet<&'static str>,
    held_mouse_button_names: HashSet<&'static str>,
}

impl ScriptInputTracker {
    pub fn new(js_callback_inputs_tx: mpsc::UnboundedSender<JsCallbackInputs>) -> Self {
        Self {
            js_callback_inputs_tx,
            held_key_names: HashSet::new(),
            held_mouse_button_names: HashSet::new(),
        }
    }

    // The held sets make presses fire once per physical press (they also swallow OS auto-repeat when the repeat flag is missing).
    pub fn pushKeyEvent(&mut self, key: KeyCode, pressed: bool) {
        let Some(key_name) = scriptKeyNameOfKeyCode(key) else {
            return;
        };
        if pressed {
            if !self.held_key_names.insert(key_name) {
                return;
            }
            let _ = self.js_callback_inputs_tx.send(JsCallbackInputs::KeyDown { key: key_name.to_string() });
        } else {
            if !self.held_key_names.remove(key_name) {
                return;
            }
            let _ = self.js_callback_inputs_tx.send(JsCallbackInputs::KeyUp { key: key_name.to_string() });
        }
    }

    pub fn pushMouseEvent(&mut self, button: MouseButton, pressed: bool) {
        let Some(button_name) = scriptMouseButtonName(button) else {
            return;
        };
        if pressed {
            if !self.held_mouse_button_names.insert(button_name) {
                return;
            }
            let _ = self.js_callback_inputs_tx.send(JsCallbackInputs::MouseDown {
                button: button_name.to_string(),
            });
        } else {
            if !self.held_mouse_button_names.remove(button_name) {
                return;
            }
            let _ = self.js_callback_inputs_tx.send(JsCallbackInputs::MouseUp {
                button: button_name.to_string(),
            });
        }
    }

    // Releases everything the window can no longer report: swallowed keyups (like after Cmd+Tab) would otherwise stay held forever.
    pub fn releaseAll(&mut self) {
        for key_name in std::mem::take(&mut self.held_key_names) {
            let _ = self.js_callback_inputs_tx.send(JsCallbackInputs::KeyUp { key: key_name.to_string() });
        }
        for button_name in std::mem::take(&mut self.held_mouse_button_names) {
            let _ = self.js_callback_inputs_tx.send(JsCallbackInputs::MouseUp {
                button: button_name.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tracker() -> (ScriptInputTracker, mpsc::UnboundedReceiver<JsCallbackInputs>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (ScriptInputTracker::new(tx), rx)
    }

    // JsCallbackInputs has no PartialEq, so events are compared through their serde shape
    fn drain_as_json(rx: &mut mpsc::UnboundedReceiver<JsCallbackInputs>) -> Vec<serde_json::Value> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(serde_json::to_value(&event).unwrap());
        }
        events
    }

    #[test]
    fn key_press_and_release_send_mapped_events() {
        let (mut tracker, mut rx) = make_tracker();
        tracker.pushKeyEvent(KeyCode::KeyE, true);
        tracker.pushKeyEvent(KeyCode::KeyE, false);
        assert_eq!(
            drain_as_json(&mut rx),
            vec![serde_json::json!({ "KeyDown": { "key": "E" } }), serde_json::json!({ "KeyUp": { "key": "E" } }),]
        );
    }

    #[test]
    fn key_names_match_the_inputservice_union() {
        let (mut tracker, mut rx) = make_tracker();
        for (key, expected_name) in [
            (KeyCode::KeyA, "A"),
            (KeyCode::KeyZ, "Z"),
            (KeyCode::Digit0, "0"),
            (KeyCode::Digit9, "9"),
            (KeyCode::Space, "Space"),
            (KeyCode::ShiftLeft, "ShiftLeft"),
            (KeyCode::ShiftRight, "ShiftRight"),
            (KeyCode::ArrowUp, "ArrowUp"),
            (KeyCode::ArrowDown, "ArrowDown"),
            (KeyCode::ArrowLeft, "ArrowLeft"),
            (KeyCode::ArrowRight, "ArrowRight"),
        ] {
            tracker.pushKeyEvent(key, true);
            assert_eq!(drain_as_json(&mut rx), vec![serde_json::json!({ "KeyDown": { "key": expected_name } })]);
        }
    }

    #[test]
    fn repeated_presses_fire_once_until_released() {
        let (mut tracker, mut rx) = make_tracker();
        // a held key produces repeated pressed events at the OS level; scripts must see exactly one KeyDown
        tracker.pushKeyEvent(KeyCode::KeyW, true);
        tracker.pushKeyEvent(KeyCode::KeyW, true);
        tracker.pushKeyEvent(KeyCode::KeyW, true);
        tracker.pushKeyEvent(KeyCode::KeyW, false);
        tracker.pushKeyEvent(KeyCode::KeyW, true);
        assert_eq!(
            drain_as_json(&mut rx),
            vec![
                serde_json::json!({ "KeyDown": { "key": "W" } }),
                serde_json::json!({ "KeyUp": { "key": "W" } }),
                serde_json::json!({ "KeyDown": { "key": "W" } }),
            ]
        );
    }

    #[test]
    fn release_without_press_sends_nothing() {
        // this is the GUI-consumed-press case: the press never reached scripts, so its release must not either
        let (mut tracker, mut rx) = make_tracker();
        tracker.pushKeyEvent(KeyCode::KeyE, false);
        tracker.pushMouseEvent(MouseButton::Left, false);
        assert_eq!(drain_as_json(&mut rx), Vec::<serde_json::Value>::new());
    }

    #[test]
    fn unsupported_keys_and_buttons_send_nothing() {
        let (mut tracker, mut rx) = make_tracker();
        tracker.pushKeyEvent(KeyCode::Escape, true);
        tracker.pushKeyEvent(KeyCode::F1, true);
        tracker.pushMouseEvent(MouseButton::Middle, true);
        assert_eq!(drain_as_json(&mut rx), Vec::<serde_json::Value>::new());
    }

    #[test]
    fn mouse_buttons_map_to_left_and_right() {
        let (mut tracker, mut rx) = make_tracker();
        tracker.pushMouseEvent(MouseButton::Left, true);
        tracker.pushMouseEvent(MouseButton::Left, true); // duplicate press swallowed
        tracker.pushMouseEvent(MouseButton::Right, true);
        tracker.pushMouseEvent(MouseButton::Left, false);
        tracker.pushMouseEvent(MouseButton::Right, false);
        assert_eq!(
            drain_as_json(&mut rx),
            vec![
                serde_json::json!({ "MouseDown": { "button": "left" } }),
                serde_json::json!({ "MouseDown": { "button": "right" } }),
                serde_json::json!({ "MouseUp": { "button": "left" } }),
                serde_json::json!({ "MouseUp": { "button": "right" } }),
            ]
        );
    }

    #[test]
    fn release_all_releases_everything_held_exactly_once() {
        let (mut tracker, mut rx) = make_tracker();
        tracker.pushKeyEvent(KeyCode::KeyE, true);
        tracker.pushMouseEvent(MouseButton::Left, true);
        drain_as_json(&mut rx); // ignore the presses

        tracker.releaseAll();
        let released = drain_as_json(&mut rx);
        assert!(released.contains(&serde_json::json!({ "KeyUp": { "key": "E" } })), "released: {released:?}");
        assert!(
            released.contains(&serde_json::json!({ "MouseUp": { "button": "left" } })),
            "released: {released:?}"
        );
        assert_eq!(released.len(), 2);

        // nothing is held anymore, so a second releaseAll and stray releases send nothing
        tracker.releaseAll();
        tracker.pushKeyEvent(KeyCode::KeyE, false);
        assert_eq!(drain_as_json(&mut rx), Vec::<serde_json::Value>::new());

        // and pressing again after the release works like a fresh press
        tracker.pushKeyEvent(KeyCode::KeyE, true);
        assert_eq!(drain_as_json(&mut rx), vec![serde_json::json!({ "KeyDown": { "key": "E" } })]);
    }
}
