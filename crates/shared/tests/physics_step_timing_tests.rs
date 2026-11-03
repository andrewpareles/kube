use game_types::consts::{CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK, CLIENT_SIM_AND_SEND_SECS, SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK};
use shared::game::world::World;

#[test]
fn a_tick_less_than_half_a_step_after_the_last_takes_no_step() {
    let dt_ms = 10.0;
    let prev_physics_step_time_ms = 100.0;
    let step_physics_to_time_ms = prev_physics_step_time_ms + 0.4 * dt_ms as f64;

    let num_steps = World::get_num_physics_steps_to_take(prev_physics_step_time_ms, step_physics_to_time_ms, dt_ms, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK);

    assert_eq!(num_steps, 0);
}

#[test]
fn ticks_one_period_apart_take_exactly_one_step() {
    let dt_ms = CLIENT_SIM_AND_SEND_SECS * 1000.0;
    // One period can measure a little over or under dt_ms.
    for prev_physics_step_time_ms in [100.0_f64, 3_600_000.0] {
        for step_physics_to_time_ms in [prev_physics_step_time_ms + dt_ms as f64 - 0.25, prev_physics_step_time_ms + dt_ms as f64 + 0.25] {
            let num_steps =
                World::get_num_physics_steps_to_take(prev_physics_step_time_ms, step_physics_to_time_ms, dt_ms, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK);
            assert_eq!(num_steps, 1, "from {prev_physics_step_time_ms} to {step_physics_to_time_ms}");
        }
    }
}

#[test]
fn physics_step_timing_runs_elapsed_fixed_steps_when_under_cap() {
    let dt_ms = 10.0;
    let prev_physics_step_time_ms = 100.0;
    let step_physics_to_time_ms = prev_physics_step_time_ms + 2.0 * dt_ms as f64;

    let num_steps = World::get_num_physics_steps_to_take(prev_physics_step_time_ms, step_physics_to_time_ms, dt_ms, SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK);

    assert_eq!(num_steps, 2);
}

#[test]
fn physics_step_timing_drops_backlog_after_client_step_cap() {
    let dt_ms = 10.0;
    let prev_physics_step_time_ms = 100.0;
    let step_physics_to_time_ms = prev_physics_step_time_ms + 20.0 * dt_ms as f64;

    let num_steps = World::get_num_physics_steps_to_take(prev_physics_step_time_ms, step_physics_to_time_ms, dt_ms, CLIENT_MAX_NUM_PHYSICS_STEPS_PER_TICK);

    assert_eq!(num_steps, 4);
}

#[test]
fn physics_step_timing_drops_backlog_after_server_step_cap() {
    let dt_ms = 10.0;
    let prev_physics_step_time_ms = 100.0;
    let step_physics_to_time_ms = prev_physics_step_time_ms + 20.0 * dt_ms as f64;

    let num_steps = World::get_num_physics_steps_to_take(prev_physics_step_time_ms, step_physics_to_time_ms, dt_ms, SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK);

    assert_eq!(num_steps, 4);
}
