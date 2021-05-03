use std::ops::Range;

use chrono::{DateTime, Utc};
use hdrhistogram::Histogram;
use sgip_signal::Forecast;

pub trait ForecastExt {
    fn histogram_over(&self, intervals: Vec<Range<DateTime<Utc>>>) -> Histogram<u64>;
}

impl ForecastExt for Forecast {
    fn histogram_over(&self, intervals: Vec<Range<DateTime<Utc>>>) -> Histogram<u64> {
        let mut emissions = Histogram::<u64>::new(3).unwrap();

        for m in self.moers() {
            if intervals.iter().any(|range| range.contains(&m.start)) {
                let rate = (m.rate * 1000.) as u64;
                emissions.record(rate).unwrap();
            }
        }

        emissions
    }
}
