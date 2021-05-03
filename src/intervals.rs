use std::ops::Range;

use chrono::{Date, DateTime, TimeZone, Utc};
use chrono_tz::US::Pacific;

use super::config;

pub trait RangeExt: Sized {
    fn intersect(&self, other: &Self) -> Option<Self>;
}

impl<R: Ord + Clone> RangeExt for Range<R> {
    fn intersect(&self, other: &Self) -> Option<Self> {
        use std::cmp::{max, min};
        let a = self;
        let b = other;
        if b.start > a.end || a.start > b.end {
            None
        } else {
            Some(max(a.start.clone(), b.start.clone())..min(a.end.clone(), b.end.clone()))
        }
    }
}

type Interval = Range<DateTime<Utc>>;

struct DateIterator<Tz: TimeZone>(pub Date<Tz>);

impl<Tz: TimeZone> Iterator for DateIterator<Tz> {
    type Item = Date<Tz>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(date) = self.0.succ_opt() {
            self.0 = date.clone();
            Some(date)
        } else {
            None
        }
    }
}

impl config::Charging {
    pub fn allowed_times_during(&self, range: Interval) -> impl Iterator<Item = Interval> {
        let allowed_times = self.allowed_times.clone();
        // this function is only for californians
        let start_date = range.start.with_timezone(&Pacific).date();

        let charging_times_for_day = move |date: Date<chrono_tz::Tz>| {
            allowed_times.clone().into_iter().map(move |(start, end)| {
                date.and_time(start).unwrap().with_timezone(&Utc)
                    ..date.and_time(end).unwrap().with_timezone(&Utc)
            })
        };

        DateIterator(start_date)
            .flat_map(charging_times_for_day)
            .map_while(move |interval| interval.intersect(&range))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charging_intervals() {
        use chrono::{Duration, Utc};
        let charging = config::Charging::default();

        let times = charging
            .allowed_times_during(Utc::now()..(Utc::now() + Duration::days(4)))
            .collect::<Vec<_>>();

        println!("{:?}", times);
    }
}
