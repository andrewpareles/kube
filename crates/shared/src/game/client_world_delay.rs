use game_types::consts::{CLIENT_WORLD_DELAY_BASE_MS, CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC, CLIENT_WORLD_DELAY_MAX_RTT_MS};

// The client lives in the past by an adaptive world delay: a fixed base plus the measured round-trip ping.
// The play loop owns the single live instance; the renderer reads the value stamped into each render snapshot.
pub struct ClientWorldDelay {
    curr_delay_ms: f32, // current delay in ms
}

impl ClientWorldDelay {
    pub fn new() -> Self {
        Self {
            curr_delay_ms: CLIENT_WORLD_DELAY_BASE_MS,
        }
    }

    // the delay the client wants to be at, given the current ping. the server also uses this to validate input timestamps.
    pub fn desired_delay_ms(rtt_ms: f32) -> f32 {
        CLIENT_WORLD_DELAY_BASE_MS + rtt_ms.clamp(0.0, CLIENT_WORLD_DELAY_MAX_RTT_MS)
    }

    pub fn current_delay_ms(&self) -> f32 {
        self.curr_delay_ms
    }

    pub fn update(&mut self, curr_rtt_ms: f32, ms_since_prev_update: f32) {
        // Increasing the delay slows or pauses world time, but must never make world time move backward.
        // Decreasing the delay makes world time catch up, so limit it to keep that catch-up smooth.
        let max_delay_decrease_ms = CLIENT_WORLD_DELAY_MAX_DECREASE_MS_PER_SEC * ms_since_prev_update / 1000.0;

        self.curr_delay_ms = Self::desired_delay_ms(curr_rtt_ms).clamp(self.curr_delay_ms - max_delay_decrease_ms, self.curr_delay_ms + ms_since_prev_update);
    }
}
