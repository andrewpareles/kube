// Both the desktop client and a browser page place server time on their own clock with this estimate, so the tests here
// cover the two things that can go wrong with it: one slow pong dragging the whole estimate with it, and a client clock
// running ahead of the server's.
use shared::game::time_estimator::TimeEstimator;

#[test]
fn a_fresh_estimator_uses_the_initial_world_stamp() {
    let time_estimator = TimeEstimator::new(1_500.0, 1_000.0);
    assert_eq!(time_estimator.estimated_offset_ms(), 500.0);
    assert_eq!(time_estimator.get_est_server_ms(1_200.0), 1_700.0);
}

#[test]
fn a_pong_places_the_server_clock_half_a_round_trip_behind_its_stamp() {
    let mut time_estimator = TimeEstimator::new(0.0, 0.0);
    // Sent at 1000 on the client's clock, answered at 2030 on the server's, back at 1060 on the client's: the server is
    // 1000ms ahead.
    time_estimator.process_pong(1_060.0, 1_000.0, 2_030.0);
    assert_eq!(time_estimator.estimated_offset_ms(), 1_000.0);
    assert_eq!(time_estimator.get_est_server_ms(2_060.0), 3_060.0);
}

#[test]
fn one_slow_pong_moves_the_estimate_by_a_single_sample() {
    let mut time_estimator = TimeEstimator::new(0.0, 0.0);
    for _ in 0..5 {
        time_estimator.process_pong(1_100.0, 1_000.0, 1_100.0);
    }
    // A pong that took a second to come back claims the server is 400ms behind, and must not be believed alone.
    time_estimator.process_pong(2_000.0, 1_000.0, 1_100.0);
    assert_eq!(time_estimator.estimated_offset_ms(), 50.0);
}

#[test]
fn older_pongs_leave_the_estimate_once_enough_new_ones_arrive() {
    let mut time_estimator = TimeEstimator::new(0.0, 0.0);
    // 21 samples named the constant, and the estimate is their median, so the window fills with the first twenty-one
    // pongs. Every later pong must push an old one out, or the estimate would never follow a drifting clock.
    for _ in 0..21 {
        time_estimator.process_pong(1_100.0, 1_000.0, 1_100.0);
    }
    assert_eq!(time_estimator.estimated_offset_ms(), 50.0);
    for _ in 0..21 {
        time_estimator.process_pong(1_200.0, 1_100.0, 1_225.0);
    }
    assert_eq!(time_estimator.estimated_offset_ms(), 75.0);
}

#[test]
fn the_server_clock_never_runs_before_zero() {
    let time_estimator = TimeEstimator::new(0.0, 5_000.0);
    assert_eq!(time_estimator.get_est_server_ms(100.0), 0.0);
}
