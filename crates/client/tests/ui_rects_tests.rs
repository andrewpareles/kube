use client::play_web_view::{UiRect, UiRects};
use winit::dpi::PhysicalPosition;

// The game gets presses and scrolls from the OS the moment they arrive, and checks them against where the page
// last said its UI is.

fn get_ui_rects() -> UiRects {
    UiRects {
        ui_only_rects: vec![UiRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        }],
        scrollable_rects: vec![UiRect {
            x: 200.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }],
    }
}

#[test]
fn presses_on_the_ui_stay_with_the_page() {
    let ui_rects = get_ui_rects();
    assert!(!ui_rects.should_game_get_press(PhysicalPosition::new(50.0, 25.0)));
    assert!(ui_rects.should_game_get_press(PhysicalPosition::new(150.0, 25.0)));
}

#[test]
fn scrolls_over_the_ui_or_a_panel_that_can_scroll_stay_with_the_page() {
    let ui_rects = get_ui_rects();
    assert!(!ui_rects.should_game_get_scroll(PhysicalPosition::new(50.0, 25.0)));
    assert!(!ui_rects.should_game_get_scroll(PhysicalPosition::new(250.0, 50.0)));
    assert!(ui_rects.should_game_get_scroll(PhysicalPosition::new(150.0, 25.0)));
}
