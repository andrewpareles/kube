use shared::conversions::time_conversions::{get_accurate_elapsed_time, get_accurate_elapsed_time_at};
use std::collections::VecDeque;
use tokio::sync::watch;

// ---------------------- TimeEstimator ----------------------

const NUM_TIME_SAMPLES: usize = 21;

#[derive(Debug)]
pub struct TimeSample {
    estimated_offset_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct TimeEstimate {
    pub estimated_offset_ms: f64, // offset between server and client start times - can only be estimated, since one-way latency is unknowable
}

#[derive(Debug)]
pub struct TimeEstimatorWriter {
    local_start_time: std::time::Instant, // client frame of reference
    samples: VecDeque<TimeSample>,        // previously computed offsets

    time_estimate_tx: watch::Sender<TimeEstimate>, // send the latest value when it changes
}

impl TimeEstimatorWriter {
    pub fn new(local_start_time: std::time::Instant, initial_live_server_wall_ms: f64, time_estimate_tx: watch::Sender<TimeEstimate>) -> Self {
        let estimated_offset_ms = initial_live_server_wall_ms - get_accurate_elapsed_time(local_start_time);
        let writer = Self {
            local_start_time: local_start_time,
            samples: VecDeque::new(),
            time_estimate_tx,
        };
        let _ = writer.time_estimate_tx.send(TimeEstimate { estimated_offset_ms });
        writer
    }

    pub fn process_pong(&mut self, t0: f64, t1: f64) {
        let live_client_wall_ms = get_accurate_elapsed_time(self.local_start_time);
        let rtt_ms = live_client_wall_ms - t0;
        let ping_ms = rtt_ms * 0.5;
        let estimated_offset_ms = t1 - (t0 + ping_ms);
        self.samples.push_back(TimeSample { estimated_offset_ms });
        if self.samples.len() > NUM_TIME_SAMPLES {
            self.samples.pop_front();
        }
        let time_estimate = TimeEstimate {
            estimated_offset_ms: Self::median_from_samples(&self.samples, |sample| sample.estimated_offset_ms),
        };
        let _ = self.time_estimate_tx.send(time_estimate);
    }

    // private - median in O(n)
    fn median_from_samples<F>(samples: &VecDeque<TimeSample>, mut get_value: F) -> f64
    where
        F: FnMut(&TimeSample) -> f64,
    {
        let mut values: Vec<f64> = samples.iter().map(|sample| get_value(sample)).collect();
        let mid = values.len() / 2;
        let (_, median, _) = values.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        *median
    }
}

// reader
#[derive(Clone)]
pub struct TimeEstimatorReader {
    local_start_time: std::time::Instant,
    time_estimate_rx: watch::Receiver<TimeEstimate>,
}

impl TimeEstimatorReader {
    pub fn new(local_start_time: std::time::Instant, time_estimate_rx: watch::Receiver<TimeEstimate>) -> Self {
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
    let local_start_time = std::time::Instant::now();
    let (tx, rx) = watch::channel(TimeEstimate {
        estimated_offset_ms: f64::NEG_INFINITY,
    });
    let writer = TimeEstimatorWriter::new(local_start_time, initial_world_live_server_wall_ms, tx);
    let reader = TimeEstimatorReader::new(local_start_time, rx);
    (writer, reader)
}
