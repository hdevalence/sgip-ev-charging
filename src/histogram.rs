use std::ops::Range;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::US::Pacific;
use hdrhistogram::Histogram;
use sgip_signal::Forecast;

use super::config;

pub fn charging_times(now: DateTime<Utc>, config: &config::Charging) -> Vec<Range<DateTime<Utc>>> {
    // this function is only for californians
    let local_time = now.with_timezone(&Pacific).time();
    let today = now.with_timezone(&Pacific).date();
    let tomorrow = today.succ();

    let today_charging_times = config.allowed_times.iter().map(|(start, end)| {
        (
            today.and_time(*start).unwrap().with_timezone(&Utc),
            today.and_time(*end).unwrap().with_timezone(&Utc),
        )
    });
    let tomorrow_charging_times = config.allowed_times.iter().map(|(start, end)| {
        (
            tomorrow.and_time(*start).unwrap().with_timezone(&Utc),
            tomorrow.and_time(*end).unwrap().with_timezone(&Utc),
        )
    });

    let target_time = if local_time < config.target_time {
        today
            .and_time(config.target_time)
            .unwrap()
            .with_timezone(&Utc)
    } else {
        tomorrow
            .and_time(config.target_time)
            .unwrap()
            .with_timezone(&Utc)
    };

    today_charging_times
        .chain(tomorrow_charging_times)
        .filter_map(|(start, end)| {
            use std::cmp::{max, min};
            if start > target_time || now > end {
                None
            } else {
                Some(max(now, start)..min(target_time, end))
            }
        })
        .collect()
}

/// Compute a distribution of emissions over charging window up to the next target time.
///
/// Emissions are recorded in gCO2/kWh (integer).
pub fn emissions_over_charge_window(
    forecast: &Forecast,
    now: DateTime<Utc>,
    config: &config::Charging,
) -> Histogram<u64> {
    let times = charging_times(now, config);

    let mut emissions = Histogram::<u64>::new(3).unwrap();

    for m in forecast.moers() {
        if times.iter().any(|range| range.contains(&m.start)) {
            let rate = (m.rate * 1000.) as u64;
            emissions.record(rate).unwrap();
            tracing::trace!(?rate, ?m.start, "recording");
        } else {
            tracing::trace!(?m.start, "skipping");
        }
    }

    emissions
}

#[cfg(test)]
mod tests {
    use sgip_signal::SgipSignal;

    use super::*;

    fn env_creds() -> (String, String) {
        (
            std::env::var("SGIP_SIGNAL_TEST_USER")
                .expect("SGIP_SIGNAL_TEST_USER is unset, please register an account and set the environment variable"),
            std::env::var("SGIP_SIGNAL_TEST_PASS")
                .expect("SGIP_SIGNAL_TEST_PASS is unset, please register an account and set the environment variable"),
        )
    }

    #[tokio::test]
    async fn current_window_percentiles() {
        let _ = tracing_subscriber::fmt::try_init();

        let config = config::Charging::default();
        tracing::info!("logging in");
        let (user, pass) = env_creds();
        let mut sgip = SgipSignal::login(&user, &pass).await.unwrap();

        tracing::info!("getting forecast");
        let forecast = sgip.forecast(config.region).await.unwrap();

        let emissions = emissions_over_charge_window(&forecast, Utc::now(), &config);

        for q in &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9] {
            let val = emissions.value_at_quantile(*q);
            tracing::info!(?q, ?val);
        }

        let moer = sgip.moer(config.region).await.unwrap();
        tracing::info!(?moer.rate);
    }

    #[test]
    fn charging_calc() {
        let config = config::Charging::default();

        let pst = |r: Range<DateTime<Utc>>| {
            r.start.with_timezone(&Pacific)..r.end.with_timezone(&Pacific)
        };

        let mut start = Utc::now();
        println!(
            "{:?}, {:?}",
            start.with_timezone(&Pacific),
            charging_times(start, &config)
                .into_iter()
                .map(pst)
                .collect::<Vec<_>>()
        );
        start = start + Duration::hours(6);
        println!(
            "{:?}, {:?}",
            start.with_timezone(&Pacific),
            charging_times(start, &config)
                .into_iter()
                .map(pst)
                .collect::<Vec<_>>()
        );
        start = start + Duration::hours(6);
        println!(
            "{:?}, {:?}",
            start.with_timezone(&Pacific),
            charging_times(start, &config)
                .into_iter()
                .map(pst)
                .collect::<Vec<_>>()
        );
        start = start + Duration::hours(6);
        println!(
            "{:?}, {:?}",
            start.with_timezone(&Pacific),
            charging_times(start, &config)
                .into_iter()
                .map(pst)
                .collect::<Vec<_>>()
        );
        start = start + Duration::hours(6);
        println!(
            "{:?}, {:?}",
            start.with_timezone(&Pacific),
            charging_times(start, &config)
                .into_iter()
                .map(pst)
                .collect::<Vec<_>>()
        );
    }
}
