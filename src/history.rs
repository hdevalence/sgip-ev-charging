use chrono::{DateTime, Utc};
use hdrhistogram::Histogram;
use sgip_signal::{GridRegion, Moer};
use std::{collections::BTreeMap, ops::Range};

pub struct History {
    region: GridRegion,
    data: BTreeMap<DateTime<Utc>, f64>,
}

impl History {
    pub fn new(region: GridRegion, moers: Vec<Moer>) -> Self {
        let mut data = BTreeMap::default();
        for moer in moers {
            data.insert(moer.start, moer.rate);
        }
        Self { region, data }
    }

    pub fn at(&self, time: DateTime<Utc>) -> Option<Moer> {
        use std::ops::Bound::{Excluded, Included, Unbounded};
        let before = self.data.range((Unbounded, Included(time))).rev().next();
        let after = self.data.range((Excluded(time), Unbounded)).next();

        match (before, after) {
            (Some((start, rate)), Some((end, _next_rate))) => Some(Moer {
                region: self.region,
                rate: *rate,
                start: *start,
                duration: *end - *start,
            }),
            _ => None,
        }
    }

    pub fn histogram_over(&self, intervals: Vec<Range<DateTime<Utc>>>) -> Histogram<u64> {
        let mut emissions = Histogram::<u64>::new(3).unwrap();

        for (start, rate) in self.data.iter() {
            if intervals.iter().any(|range| range.contains(start)) {
                let rate = (rate * 1000.) as u64;
                emissions.record(rate).unwrap();
            }
        }

        emissions
    }
}
