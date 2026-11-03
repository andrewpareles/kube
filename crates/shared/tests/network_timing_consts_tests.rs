use game_types::consts::{
    CLIENT_INTERPOLATION_WINDOW_MS, CLIENT_SIM_AND_SEND_SECS, CLIENT_TOTAL_SNAPSHOT_BUFFER_MS, CLIENT_WORLD_DELAY_BASE_MS, CLIENT_WORLD_DELAY_EXTRA_LAG_MS,
    CLIENT_WORLD_DELAY_MAX_RTT_MS, JITTER_MARGIN_MS, SERVER_MAX_IDLE_TIMEOUT_SECS, SERVER_SIM_AND_SEND_SECS,
};
use shared::game::client_world_delay::ClientWorldDelay;

const EPSILON: f32 = 1e-5;

fn assert_close(actual: f32, expected: f32, label: &str) {
    let diff = (actual - expected).abs();
    assert!(diff <= EPSILON, "{} mismatch: actual={}, expected={}, diff={}", label, actual, expected, diff);
}

fn assert_positive_finite(value: f32, label: &str) {
    assert!(value.is_finite(), "{label} must be finite, got {value}");
    assert!(value > 0.0, "{label} must be > 0, got {value}");
}

macro_rules! generate_positive_finite_tests {
    ($($test_name:ident => ($value:expr, $label:literal)),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_positive_finite($value, $label);
            }
        )+
    };
}

macro_rules! generate_expected_period_tests {
    ($($test_name:ident => ($period_secs:expr, $expected_period_secs:expr, $expected_hz:expr, $label:literal)),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_close($period_secs, $expected_period_secs, $label);
                let hz = 1.0 / $period_secs;
                assert_close(hz, $expected_hz, concat!($label, " hz"));
            }
        )+
    };
}

macro_rules! generate_pairwise_equality_tests {
    ($($test_name:ident => ($lhs:expr, $rhs:expr, $label:literal)),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_close($lhs, $rhs, $label);
            }
        )+
    };
}

generate_positive_finite_tests! {
    server_sim_and_send_secs_is_positive_and_finite => (SERVER_SIM_AND_SEND_SECS, "SERVER_SIM_AND_SEND_SECS"),
    client_sim_and_send_secs_is_positive_and_finite => (CLIENT_SIM_AND_SEND_SECS, "CLIENT_SIM_AND_SEND_SECS"),
    jitter_margin_ms_is_positive_and_finite => (JITTER_MARGIN_MS, "JITTER_MARGIN_MS"),
    client_world_delay_base_ms_is_positive_and_finite => (CLIENT_WORLD_DELAY_BASE_MS, "CLIENT_WORLD_DELAY_BASE_MS"),
    client_world_delay_max_rtt_ms_is_positive_and_finite => (CLIENT_WORLD_DELAY_MAX_RTT_MS, "CLIENT_WORLD_DELAY_MAX_RTT_MS"),
    client_interpolation_window_ms_is_positive_and_finite => (CLIENT_INTERPOLATION_WINDOW_MS, "CLIENT_INTERPOLATION_WINDOW_MS"),
    client_total_snapshot_buffer_ms_is_positive_and_finite => (CLIENT_TOTAL_SNAPSHOT_BUFFER_MS, "CLIENT_TOTAL_SNAPSHOT_BUFFER_MS")
}

generate_expected_period_tests! {
    server_sim_and_send_interval_is_sixty_hz => (SERVER_SIM_AND_SEND_SECS, 1.0 / 60.0, 60.0, "SERVER_SIM_AND_SEND_SECS"),
    client_sim_and_send_interval_is_sixty_hz => (CLIENT_SIM_AND_SEND_SECS, 1.0 / 60.0, 60.0, "CLIENT_SIM_AND_SEND_SECS")
}

generate_pairwise_equality_tests! {
    client_sim_and_send_matches_server_sim_and_send_interval => (
        CLIENT_SIM_AND_SEND_SECS,
        SERVER_SIM_AND_SEND_SECS,
        "CLIENT_SIM_AND_SEND_SECS vs SERVER_SIM_AND_SEND_SECS"
    )
}

#[test]
fn base_client_world_delay_formula_is_synced() {
    let expected = f32::max(SERVER_SIM_AND_SEND_SECS, CLIENT_SIM_AND_SEND_SECS) * 1000.0 + JITTER_MARGIN_MS + CLIENT_WORLD_DELAY_EXTRA_LAG_MS;
    assert_close(CLIENT_WORLD_DELAY_BASE_MS, expected, "CLIENT_WORLD_DELAY_BASE_MS formula");
}

#[test]
fn client_world_delay_desired_adds_clamped_rtt_to_base() {
    assert_close(ClientWorldDelay::desired_delay_ms(0.0), CLIENT_WORLD_DELAY_BASE_MS, "delay with zero rtt");
    assert_close(
        ClientWorldDelay::desired_delay_ms(-100.0),
        CLIENT_WORLD_DELAY_BASE_MS,
        "delay with negative rtt",
    );
    assert_close(
        ClientWorldDelay::desired_delay_ms(80.0),
        CLIENT_WORLD_DELAY_BASE_MS + 80.0,
        "delay with typical rtt",
    );
    assert_close(
        ClientWorldDelay::desired_delay_ms(CLIENT_WORLD_DELAY_MAX_RTT_MS + 1000.0),
        CLIENT_WORLD_DELAY_BASE_MS + CLIENT_WORLD_DELAY_MAX_RTT_MS,
        "delay with huge rtt clamps to max",
    );
}

#[test]
fn client_world_delay_growth_does_not_move_the_world_backward() {
    let mut client_world_delay = ClientWorldDelay::new();
    client_world_delay.update(100.0, 10.0);
    assert_close(
        client_world_delay.current_delay_ms(),
        CLIENT_WORLD_DELAY_BASE_MS + 10.0,
        "delay after first growth",
    );

    client_world_delay.update(100.0, 15.0);
    assert_close(
        client_world_delay.current_delay_ms(),
        CLIENT_WORLD_DELAY_BASE_MS + 25.0,
        "delay after second growth",
    );
}

#[test]
fn client_world_delay_shrinks_at_thirty_ms_per_second() {
    let mut client_world_delay = ClientWorldDelay::new();
    client_world_delay.update(100.0, 1000.0);
    client_world_delay.update(0.0, 1000.0);
    assert_close(
        client_world_delay.current_delay_ms(),
        CLIENT_WORLD_DELAY_BASE_MS + 70.0,
        "delay after one second of shrinking",
    );
}

#[test]
fn client_interpolation_window_is_currently_eighty_three_ms() {
    assert_close(CLIENT_INTERPOLATION_WINDOW_MS, 83.0, "CLIENT_INTERPOLATION_WINDOW_MS");
}

#[test]
fn client_total_snapshot_buffer_formula_is_synced() {
    let expected = CLIENT_WORLD_DELAY_BASE_MS + CLIENT_WORLD_DELAY_MAX_RTT_MS + CLIENT_INTERPOLATION_WINDOW_MS + JITTER_MARGIN_MS;
    assert_close(CLIENT_TOTAL_SNAPSHOT_BUFFER_MS, expected, "CLIENT_TOTAL_SNAPSHOT_BUFFER_MS formula");
}

#[test]
fn interpolation_window_covers_three_send_intervals() {
    let server_snapshot_spacing_ms = SERVER_SIM_AND_SEND_SECS * 1000.0;
    let snapshots_in_window = CLIENT_INTERPOLATION_WINDOW_MS / server_snapshot_spacing_ms;
    assert!(
        snapshots_in_window >= 3.0 - EPSILON,
        "CLIENT_INTERPOLATION_WINDOW_MS ({CLIENT_INTERPOLATION_WINDOW_MS}) should cover roughly 3 snapshots, got {snapshots_in_window}",
    );
}

#[test]
fn idle_timeout_spans_multiple_step_and_send_ticks() {
    let idle_timeout_secs = SERVER_MAX_IDLE_TIMEOUT_SECS as f32;
    let physics_ticks = idle_timeout_secs / SERVER_SIM_AND_SEND_SECS;
    let send_ticks = idle_timeout_secs / SERVER_SIM_AND_SEND_SECS;

    assert!(
        physics_ticks >= 2.0,
        "SERVER_MAX_IDLE_TIMEOUT_SECS should cover at least 2 physics ticks, got {physics_ticks}"
    );
    assert!(
        send_ticks >= 2.0,
        "SERVER_MAX_IDLE_TIMEOUT_SECS should cover at least 2 send ticks, got {send_ticks}"
    );
}
