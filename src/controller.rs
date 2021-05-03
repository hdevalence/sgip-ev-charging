use chrono::{DateTime, Duration, Utc};
use chrono_tz::US::Pacific;
use hdrhistogram::Histogram;
use sgip_signal::{Forecast, Moer};

use super::config;
use crate::{forecast_ext::ForecastExt, DurationExt};

#[derive(Clone, Debug)]
pub struct ChargeController {
    config: config::Charging,
    start: DateTime<Utc>,
    actual_rates: Histogram<u64>,
}

impl ChargeController {
    pub fn new(config: config::Charging, start: DateTime<Utc>) -> Self {
        Self {
            config,
            start,
            actual_rates: Histogram::<u64>::new(3).unwrap(),
        }
    }

    fn record_rate(&mut self, moer: &Moer) {
        self.actual_rates
            .record((moer.rate * 1000.) as u64)
            .unwrap();
    }

    /// Should be called every 5 minutes.  Returns whether to charge, and the
    /// emissions limit used to make that decision.
    pub fn can_charge(
        &mut self,
        now: DateTime<Utc>,
        soc: f64,
        moer: &Moer,
        forecast: &Forecast,
    ) -> (bool, u64) {
        let current_rate = (moer.rate * 1000.) as u64;
        let end = self.start + Duration::hours(self.config.flex_charge_hours);

        // Don't charge outside of allowed times, and don't record emissions
        // statistics outside of allowed times.
        if !self
            .config
            .allowed_times_during(self.start..end)
            .any(|range| range.contains(&now))
        {
            return (false, 0);
        } else {
            self.record_rate(moer);
        }

        // Don't charge if the state of charge is bigger than the maximum.
        if soc >= self.config.max_charge {
            return (false, 0);
        }

        let local_time = now.with_timezone(&Pacific).time();
        let local_date = now.with_timezone(&Pacific).date();

        let below_base_charge = soc <= self.config.base_charge;
        let before_base_charge_by = local_time < self.config.base_charge_by;

        let emissions_limit = if below_base_charge && before_base_charge_by {
            let target = local_date
                .and_time(self.config.base_charge_by)
                .unwrap()
                .with_timezone(&Utc);

            let available_charging_hours: f64 = self
                .config
                .allowed_times_during(now..target)
                .map(|range| (range.end - range.start).num_hours_f64())
                .sum();

            let base_charge_kwh = self.config.base_charge * self.config.capacity_kwh;
            let base_charge_hours = base_charge_kwh / self.config.charge_rate_kw;

            let charge_time_proportion = base_charge_hours / available_charging_hours;

            // Combine recorded and forecast data to create a histogram of
            // emissions rates over the entire charging session.
            let mut emissions =
                forecast.histogram_over(self.config.allowed_times_during(now..target).collect());
            emissions += &self.actual_rates;

            // Since we recorded the current rate in the histogram at the
            // beginning of the function, we know that the 100th-percentile
            // value of the histogram is >= the current rate.  This means that
            // if charge_time_proportion >= 1, we're sure to charge continuously
            // until the base charge target is met.
            emissions.value_at_quantile(charge_time_proportion)
        } else {
            let target = self.start + Duration::hours(self.config.flex_charge_hours);

            let mut emissions =
                forecast.histogram_over(self.config.allowed_times_during(now..target).collect());
            emissions += &self.actual_rates;

            emissions.value_at_quantile(1.0 - soc * self.config.max_charge)
        };

        (current_rate <= emissions_limit, emissions_limit)
    }
}
