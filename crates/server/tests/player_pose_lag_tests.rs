use game_types::consts::{CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC, MAX_POSE_LAG_MS, MAX_UNSHIFTED_POSE_LAG_MS};
use server::PlayerPoseLag;

const TICK_MS: f64 = 1000.0 / 60.0;

// Sends one pose a tick for `secs` seconds, each stamped `lag_ms` before it reaches the server, and returns how far the last stamp was moved.
fn send_poses(player_pose_lag: &mut PlayerPoseLag, recv_ms: &mut f64, lag_ms: f32, secs: f32) -> f32 {
    let mut shift_ms = 0.0;
    for _ in 0..(secs as f64 * 1000.0 / TICK_MS).round() as usize {
        *recv_ms += TICK_MS;
        let stamp_ms = *recv_ms - lag_ms as f64;
        shift_ms = (player_pose_lag.shift_stamp(stamp_ms, *recv_ms) - stamp_ms) as f32;
    }
    shift_ms
}

#[test]
fn a_player_whose_poses_arrive_in_time_keeps_their_stamps() {
    let mut recv_ms = 1000.0;
    let mut player_pose_lag = PlayerPoseLag::new(recv_ms);
    let shift_ms = send_poses(&mut player_pose_lag, &mut recv_ms, MAX_UNSHIFTED_POSE_LAG_MS - 10.0, 2.0);
    assert_eq!(shift_ms, 0.0);
}

#[test]
fn a_laggy_player_is_moved_later_by_how_far_past_the_unshifted_lag_their_poses_come() {
    let mut recv_ms = 1000.0;
    let mut player_pose_lag = PlayerPoseLag::new(recv_ms);
    let shift_ms = send_poses(&mut player_pose_lag, &mut recv_ms, MAX_UNSHIFTED_POSE_LAG_MS + 100.0, 2.0);
    assert!((shift_ms - 100.0).abs() < 1e-2, "shift {shift_ms}");
}

// However late a player's poses come, watchers get them no more than MAX_UNSHIFTED_POSE_LAG_MS before they reached the server.
#[test]
fn a_player_lagging_past_the_max_pose_lag_is_still_moved_by_all_of_it() {
    let mut recv_ms = 1000.0;
    let mut player_pose_lag = PlayerPoseLag::new(recv_ms);
    let lag_ms = MAX_POSE_LAG_MS + 200.0;
    let shift_ms = send_poses(&mut player_pose_lag, &mut recv_ms, lag_ms, 2.0);
    assert!((shift_ms - (lag_ms - MAX_UNSHIFTED_POSE_LAG_MS)).abs() < 1e-2, "shift {shift_ms}");
}

// Like a client's world delay, the shift falls slowly when the lag drops, so the player doesn't jump forward.
#[test]
fn a_laggy_players_shift_falls_slowly_when_their_lag_drops() {
    let mut recv_ms = 1000.0;
    let mut player_pose_lag = PlayerPoseLag::new(recv_ms);
    send_poses(&mut player_pose_lag, &mut recv_ms, MAX_UNSHIFTED_POSE_LAG_MS + 150.0, 2.0);

    let shift_ms = send_poses(&mut player_pose_lag, &mut recv_ms, MAX_UNSHIFTED_POSE_LAG_MS + 130.0, 0.5);
    let expected_shift_ms = 150.0 - CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC * 0.5;
    assert!((shift_ms - expected_shift_ms).abs() < 1.0, "shift {shift_ms}, expected {expected_shift_ms}");

    let shift_ms = send_poses(&mut player_pose_lag, &mut recv_ms, MAX_UNSHIFTED_POSE_LAG_MS + 130.0, 2.0);
    assert!((shift_ms - 130.0).abs() < 1e-2, "shift {shift_ms}");
}

// A stamp absurdly far in the past counts as only MAX_POSE_LAG_MS late, so it stops holding the player back once the lag has fallen again.
#[test]
fn one_absurd_stamp_holds_a_player_back_only_until_their_lag_falls_again() {
    let mut recv_ms = 1000.0;
    let mut player_pose_lag = PlayerPoseLag::new(recv_ms);
    send_poses(&mut player_pose_lag, &mut recv_ms, 20.0, 1.0);
    recv_ms += TICK_MS;
    player_pose_lag.shift_stamp(-3e38, recv_ms);

    let recovery_secs = (MAX_POSE_LAG_MS - MAX_UNSHIFTED_POSE_LAG_MS) / CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC;
    let shift_ms = send_poses(&mut player_pose_lag, &mut recv_ms, 20.0, recovery_secs + 1.0);
    assert_eq!(shift_ms, 0.0);
}
