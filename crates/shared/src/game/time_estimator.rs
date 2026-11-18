// The offset between this client's clock and the server's.
//
// The server stamps its messages with the milliseconds since it started, so a client places server time on its own
// clock. One-way latency can't be measured, so each pong is assumed to have taken half the round trip in each direction,
// and the median of the recent samples is used. Both the desktop client and a browser page keep this estimate; each
// reads its own clock and passes the readings in.

use std::collections::VecDeque;

const NUM_TIME_SAMPLES: usize = 21;

#[derive(Debug)]
pub struct TimeEstimator {
    estimated_offset_ms: f64, // added to this client's wall clock to get the server's
    samples: VecDeque<f64>,
}

impl TimeEstimator {
    // Before any pong has arrived, the server's clock in the initial world is the only sample there is.
    pub fn new(initial_live_server_wall_ms: f64, live_client_wall_ms: f64) -> Self {
        Self {
            estimated_offset_ms: initial_live_server_wall_ms - live_client_wall_ms,
            samples: VecDeque::with_capacity(NUM_TIME_SAMPLES),
        }
    }

    pub fn estimated_offset_ms(&self) -> f64 {
        self.estimated_offset_ms
    }

    pub fn process_pong(&mut self, live_client_wall_ms: f64, t0_client_send_ms: f64, t1_server_recv_ms: f64) {
        let round_trip_ms = live_client_wall_ms - t0_client_send_ms;
        let ping_ms = round_trip_ms * 0.5;
        self.samples.push_back(t1_server_recv_ms - (t0_client_send_ms + ping_ms));
        if self.samples.len() > NUM_TIME_SAMPLES {
            self.samples.pop_front();
        }
        self.estimated_offset_ms = median(&mut self.samples);
    }

    pub fn get_est_server_ms(&self, live_client_wall_ms: f64) -> f64 {
        (live_client_wall_ms + self.estimated_offset_ms).max(0.0)
    }
}

// The median in O(n), so one slow pong moves the estimate by one sample instead of all of it.
fn median(samples: &mut VecDeque<f64>) -> f64 {
    let mut values: Vec<f64> = samples.iter().copied().collect();
    let mid = values.len() / 2;
    let (_, median, _) = values.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    *median
}
