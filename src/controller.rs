use chrono::{DateTime, Duration, Utc};
use chrono_tz::US::Pacific;
use sgip_signal::{Forecast, Moer};

use super::config;
use crate::{DurationExt, ForecastExt, History};

impl config::Charging {
    /// Returns whether to charge, and the emissions limit used to make that decision.
    pub fn can_charge(
        &self,
        now: DateTime<Utc>,
        soc: f64,
        history: &History,
        current: &Moer,
        forecast: &Forecast,
    ) -> (bool, u64) {
        let end = now + Duration::hours(self.flex_charge_hours);
        // Don't charge outside of allowed times.
        if !self.allowed_at(now) {
            return (false, 0);
        }

        // Don't charge if the state of charge is bigger than the maximum.
        if soc >= self.max_charge {
            return (false, 0);
        }

        let local_time = now.with_timezone(&Pacific).time();
        // Determine whether our goal is base charging or flex charging.
        let below_base_charge = soc <= self.base_charge;
        let before_base_charge_by = local_time < self.base_charge_by;

        let (target_time, target_charge) = if below_base_charge && before_base_charge_by {
            (
                now.with_timezone(&Pacific)
                    .date()
                    .and_time(self.base_charge_by)
                    .unwrap()
                    .with_timezone(&Utc),
                self.base_charge,
            )
        } else {
            (end, self.max_charge)
        };

        let available_charging_hours: f64 = self
            .allowed_times_during(now..target_time)
            .map(|range| (range.end - range.start).num_hours_f64())
            .sum();

        let charge_kwh = (target_charge - soc) * self.capacity_kwh;
        let charge_hours = charge_kwh / self.charge_rate_kw;
        let charging_time_proportion = charge_hours / available_charging_hours;

        // The SGIP forecasts often get the curve right but offset up or down,
        // which biases the forecast emissions data, so combine the forecast
        // data with actual emissions over a longer time period.
        let lookback = self
            .allowed_times_during((now - Duration::hours(2 * self.flex_charge_hours))..now)
            .collect();
        let lookahead = self.allowed_times_during(now..target_time).collect();

        let mut emissions = history.histogram_over(lookback);
        emissions += forecast.histogram_over(lookahead);

        // Ensure that the current emissions rate is included in the histogram,
        // so that the 100th-percentile value of the histogram is >= the current
        // rate.  This means that if charge_time_proportion >= 1, we're sure to
        // charge continuously until the charge target is met.
        let current_rate = (current.rate * 1000.) as u64;
        emissions += current_rate;

        let emissions_limit = emissions.value_at_quantile(charging_time_proportion);
        tracing::debug!(
            ?target_time,
            ?soc,
            ?target_charge,
            ?charge_kwh,
            ?charge_hours,
            ?available_charging_hours,
            ?charging_time_proportion,
            ?emissions_limit,
            ?current_rate,
            can_charge = (current_rate <= emissions_limit),
        );

        (current_rate <= emissions_limit, emissions_limit)
    }
}
