use std::collections::VecDeque;

// A value as it changes over time: (est_live_server_sim_ms, value) entries, oldest first.
pub struct Timeline<T> {
    entries: VecDeque<(f64, T)>,
}

impl<T> Timeline<T> {
    pub fn new() -> Self {
        Self { entries: VecDeque::new() }
    }

    // entries can arrive out of order
    pub fn insert(&mut self, est_live_server_sim_ms: f64, value: T) {
        let index = self
            .entries
            .iter()
            .position(|(entry_est_live_server_sim_ms, _)| *entry_est_live_server_sim_ms > est_live_server_sim_ms)
            .unwrap_or(self.entries.len());
        self.entries.insert(index, (est_live_server_sim_ms, value));
    }

    pub fn find_last_entry_at_or_before(&self, est_server_sim_ms: f64) -> Option<&(f64, T)> {
        self.entries
            .iter()
            .rev()
            .find(|(entry_est_live_server_sim_ms, _)| *entry_est_live_server_sim_ms <= est_server_sim_ms)
    }

    pub fn find_first_entry_after(&self, est_server_sim_ms: f64) -> Option<&(f64, T)> {
        self.entries
            .iter()
            .find(|(entry_est_live_server_sim_ms, _)| *entry_est_live_server_sim_ms > est_server_sim_ms)
    }

    pub fn forget_entries_before(&mut self, est_server_sim_ms: f64) {
        self.entries
            .retain(|(entry_est_live_server_sim_ms, _)| *entry_est_live_server_sim_ms >= est_server_sim_ms);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
