pub fn get_accurate_elapsed_time(time: std::time::Instant) -> f64 {
    time.elapsed().as_secs_f64() * 1000.0
}

pub fn get_accurate_elapsed_time_at(time: std::time::Instant, at: std::time::Instant) -> f64 {
    at.saturating_duration_since(time).as_secs_f64() * 1000.0
}
