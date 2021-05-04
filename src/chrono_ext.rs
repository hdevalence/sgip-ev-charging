use chrono::Duration;

const SECS_PER_HOUR: f64 = 60. * 60.;
const SECS_PER_MIN: f64 = 60.;

pub trait DurationExt {
    fn num_hours_f64(&self) -> f64;
    fn num_minutes_f64(&self) -> f64;
}

impl DurationExt for Duration {
    fn num_hours_f64(&self) -> f64 {
        self.num_seconds() as f64 / SECS_PER_HOUR
    }

    fn num_minutes_f64(&self) -> f64 {
        self.num_seconds() as f64 / SECS_PER_MIN
    }
}
