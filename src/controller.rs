use chrono::{DateTime, Duration, Utc};
use chrono_tz::US::Pacific;
use sgip_signal::{Forecast, Moer};

use super::config;
use crate::{DurationExt, ForecastExt, History};

struct Goal<'c> {
    time: DateTime<Utc>,
    charge: f64,
    config: &'c config::Charging,
}

impl<'c> std::fmt::Debug for Goal<'c> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Goal")
            .field("time", &self.time)
            .field("charge", &self.charge)
            .finish()
    }
}

impl<'c> Goal<'c> {
    fn available_charging_hours(&self, now: DateTime<Utc>) -> f64 {
        self.config
            .allowed_times_during(now..self.time)
            .map(|range| (range.end - range.start).num_hours_f64())
            .sum()
    }

    fn required_charging_proportion(&self, now: DateTime<Utc>, present_soc: f64) -> f64 {
        let charge_kwh = (self.charge - present_soc) * self.config.capacity_kwh;
        let charge_hours = charge_kwh / self.config.charge_rate_kw;
        charge_hours / self.available_charging_hours(now)
    }
}

impl config::Charging {
    /// Returns whether to charge, and the emissions limit used to make that decision.
    #[tracing::instrument(skip(self,current,history,forecast))]
    pub fn can_charge(
        &self,
        now: DateTime<Utc>,
        soc: f64,
        history: &History,
        current: &Moer,
        forecast: &Forecast,
    ) -> (bool, i64) {
        let end = now + Duration::hours(self.flex_charge_hours);
        // Don't charge outside of allowed times.
        if !self.allowed_at(now) {
            return (false, -1);
        }

        // Don't charge if the state of charge is bigger than the maximum.
        if soc >= self.max_charge {
            return (false, -1);
        }

        let goals = vec![
            // Base charging: hit a base charge level at a fixed time each day.
            Goal {
                time: now
                    .with_timezone(&Pacific)
                    .date()
                    .and_time(self.base_charge_by)
                    .unwrap()
                    .with_timezone(&Utc),
                charge: self.base_charge,
                config: &self,
            },
            // Flex charging: aim to complete the rest of the charging a fixed
            // time from now, whatever now is.
            Goal {
                time: end,
                charge: self.max_charge,
                config: &self,
            },
        ];
        tracing::info!(?goals);

        // Only retain future goals
        let mut goals = goals
            .into_iter()
            .filter(|goal| goal.time > now)
            .collect::<Vec<Goal>>();

        // Choose the goal with the largest required charging proportion.
        goals.sort_by(|a, b| {
            let a_req = a.required_charging_proportion(now, soc);
            let b_req = b.required_charging_proportion(now, soc);

            a_req.partial_cmp(&b_req).unwrap()
        });
        let goal = goals.pop().expect("must have at least one goal");
        tracing::info!(?goal);

        let available_charging_hours = goal.available_charging_hours(now);
        let required_charging_proportion = goal.required_charging_proportion(now, soc);

        metrics::gauge!("soc", soc);
        metrics::gauge!("available_charging_hours", available_charging_hours);
        metrics::gauge!("required_charging_proportion", required_charging_proportion);
        metrics::gauge!("target_charge", goal.charge);

        // The SGIP forecasts often get the curve right but offset up or down,
        // which biases the forecast emissions data, so combine the forecast
        // data with actual emissions over a longer time period.
        let lookback = self
            .allowed_times_during((now - Duration::hours(2 * self.flex_charge_hours))..now)
            .collect();
        let lookahead = self.allowed_times_during(now..goal.time).collect();

        let mut emissions = history.histogram_over(lookback) + forecast.histogram_over(lookahead);

        // Ensure that the current emissions rate is included in the histogram,
        // so that the 100th-percentile value of the histogram is >= the current
        // rate.  This means that if charge_time_proportion >= 1, we're sure to
        // charge continuously until the charge target is met.
        let current_rate = (current.rate * 1000.) as u64;
        emissions += current_rate;

        let emissions_limit = emissions.value_at_quantile(required_charging_proportion);
        tracing::debug!(
            ?goal.time,
            ?goal.charge,
            ?soc,
            ?required_charging_proportion,
            ?emissions_limit,
            ?current_rate,
            can_charge = (current_rate <= emissions_limit),
        );

        let emissions_quantile = |q: f64| (emissions.value_at_quantile(q) as f64) / 1000.;
        metrics::gauge!("emissions_min", emissions_quantile(0.00));
        metrics::gauge!("emissions_q10", emissions_quantile(0.10));
        metrics::gauge!("emissions_q25", emissions_quantile(0.25));
        metrics::gauge!("emissions_q50", emissions_quantile(0.50));
        metrics::gauge!("emissions_q75", emissions_quantile(0.75));
        metrics::gauge!("emissions_q90", emissions_quantile(0.90));
        metrics::gauge!("emissions_max", emissions_quantile(1.00));
        metrics::gauge!(
            "emissions_limit",
            emissions_quantile(required_charging_proportion)
        );

        (current_rate <= emissions_limit, emissions_limit as i64)
    }
}
