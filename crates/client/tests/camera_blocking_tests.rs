use client::play_runner::play_renderer::computeNextRenderedZoomFromBlocking;

const REQUESTED_CAMERA_ZOOM: f32 = 50.0;
const BLOCKED_CAMERA_ZOOM: f32 = 10.0;
const TEST_EPSILON: f32 = 0.0001;

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() <= TEST_EPSILON, "expected {expected}, got {actual}");
}

#[test]
fn short_camera_block_does_not_zoom_in() {
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) =
        computeNextRenderedZoomFromBlocking(REQUESTED_CAMERA_ZOOM, REQUESTED_CAMERA_ZOOM, false, BLOCKED_CAMERA_ZOOM, 0.0, 0.02);

    assert_close(rendered_camera_zoom, REQUESTED_CAMERA_ZOOM);
    assert!(!is_zoom_recovering_from_blocked);
    assert_close(blocked_zoom_elapsed_secs, 0.02);
}

#[test]
fn persistent_camera_block_zooms_in_after_delay() {
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) =
        computeNextRenderedZoomFromBlocking(REQUESTED_CAMERA_ZOOM, REQUESTED_CAMERA_ZOOM, false, BLOCKED_CAMERA_ZOOM, 0.0, 0.02);
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) = computeNextRenderedZoomFromBlocking(
        REQUESTED_CAMERA_ZOOM,
        rendered_camera_zoom,
        is_zoom_recovering_from_blocked,
        BLOCKED_CAMERA_ZOOM,
        blocked_zoom_elapsed_secs,
        0.02,
    );

    assert_close(rendered_camera_zoom, BLOCKED_CAMERA_ZOOM);
    assert!(is_zoom_recovering_from_blocked);
    assert_close(blocked_zoom_elapsed_secs, 0.04);
}

#[test]
fn camera_block_timer_resets_when_unblocked() {
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) =
        computeNextRenderedZoomFromBlocking(REQUESTED_CAMERA_ZOOM, REQUESTED_CAMERA_ZOOM, false, BLOCKED_CAMERA_ZOOM, 0.0, 0.02);
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) = computeNextRenderedZoomFromBlocking(
        REQUESTED_CAMERA_ZOOM,
        rendered_camera_zoom,
        is_zoom_recovering_from_blocked,
        REQUESTED_CAMERA_ZOOM,
        blocked_zoom_elapsed_secs,
        0.02,
    );
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) = computeNextRenderedZoomFromBlocking(
        REQUESTED_CAMERA_ZOOM,
        rendered_camera_zoom,
        is_zoom_recovering_from_blocked,
        BLOCKED_CAMERA_ZOOM,
        blocked_zoom_elapsed_secs,
        0.02,
    );

    assert_close(rendered_camera_zoom, REQUESTED_CAMERA_ZOOM);
    assert!(!is_zoom_recovering_from_blocked);
    assert_close(blocked_zoom_elapsed_secs, 0.02);
}

#[test]
fn camera_zoom_out_still_smooths_after_delayed_block() {
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) =
        computeNextRenderedZoomFromBlocking(REQUESTED_CAMERA_ZOOM, REQUESTED_CAMERA_ZOOM, false, BLOCKED_CAMERA_ZOOM, 0.0, 0.03);
    let (rendered_camera_zoom, is_zoom_recovering_from_blocked, blocked_zoom_elapsed_secs) = computeNextRenderedZoomFromBlocking(
        REQUESTED_CAMERA_ZOOM,
        rendered_camera_zoom,
        is_zoom_recovering_from_blocked,
        REQUESTED_CAMERA_ZOOM,
        blocked_zoom_elapsed_secs,
        0.03,
    );

    assert!(rendered_camera_zoom > BLOCKED_CAMERA_ZOOM);
    assert!(rendered_camera_zoom < REQUESTED_CAMERA_ZOOM);
    assert!(is_zoom_recovering_from_blocked);
    assert_close(blocked_zoom_elapsed_secs, 0.0);
}
