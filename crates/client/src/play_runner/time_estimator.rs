// The play thread reads this client's clock and the estimate moves it onto the server's, so the reading is handed to
// `shared`'s estimator rather than kept here.

use shared::conversions::time_conversions::{get_accurate_elapsed_time, get_accurate_elapsed_time_at};
use shared::game::time_estimator::TimeEstimator;
use std::time::Instant;
use tokio::sync::watch;

// ---------------------- TimeEstimator ----------------------

#[derive(Clone, Copy, Debug)]
pub struct TimeEstimate {
    pub estimated_offset_ms: f64, // offset between server and client start times - can only be estimated, since one-way latency is unknowable
}

#[derive(Debug)]
pub struct TimeEstimatorWriter {
    local_start_time: Instant, // client frame of reference
    estimator: TimeEstimator,

    time_estimate_tx: watch::Sender<TimeEstimate>, // send the latest value when it changes
}

impl TimeEstimatorWriter {
    pub fn new(local_start_time: Instant, initial_live_server_wall_ms: f64, time_estimate_tx: watch::Sender<TimeEstimate>) -> Self {
        let mut writer = Self {
            local_start_time,
            estimator: TimeEstimator::new(initial_live_server_wall_ms, get_accurate_elapsed_time(local_start_time)),
            time_estimate_tx,
        };
        writer.publish_estimate();
        writer
    }

    pub fn process_pong(&mut self, t0: f64, t1: f64) {
        let live_client_wall_ms = get_accurate_elapsed_time(self.local_start_time);
        self.estimator.process_pong(live_client_wall_ms, t0, t1);
        self.publish_estimate();
    }

    fn publish_estimate(&mut self) {
        let time_estimate = TimeEstimate {
            estimated_offset_ms: self.estimator.estimated_offset_ms(),
        };
        let _ = self.time_estimate_tx.send(time_estimate);
    }
}

// reader
#[derive(Clone)]
pub struct TimeEstimatorReader {
    local_start_time: Instant,
    time_estimate_rx: watch::Receiver<TimeEstimate>,
}

impl TimeEstimatorReader {
    pub fn new(local_start_time: Instant, time_estimate_rx: watch::Receiver<TimeEstimate>) -> Self {
        Self {
            local_start_time,
            time_estimate_rx,
        }
    }
    pub fn get_live_client_wall_ms(&self) -> f64 {
        get_accurate_elapsed_time(self.local_start_time)
    }
    pub fn get_live_client_sim_ms_at(&self, tick_instant: std::time::Instant) -> f64 {
        get_accurate_elapsed_time_at(self.local_start_time, tick_instant)
    }
    pub fn get_est_server_ms_at(&self, client_ms: f64) -> f64 {
        (client_ms + self.time_estimate_rx.borrow().estimated_offset_ms).max(0.0)
    }
}

pub fn createTimeEstimator(initial_world_live_server_wall_ms: f64) -> (TimeEstimatorWriter, TimeEstimatorReader) {
    let local_start_time = Instant::now();
    let (tx, rx) = watch::channel(TimeEstimate {
        estimated_offset_ms: f64::NEG_INFINITY,
    });
    let writer = TimeEstimatorWriter::new(local_start_time, initial_world_live_server_wall_ms, tx);
    let reader = TimeEstimatorReader::new(local_start_time, rx);
    (writer, reader)
}
