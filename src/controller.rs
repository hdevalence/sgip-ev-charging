use anyhow::Error;
use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::US::Pacific;
use sgip_signal::{Forecast, Moer, SgipSignal};

use super::config;
use crate::{tesla::Vehicle, DurationExt, ForecastExt, History};

struct Goal<'c> {
    time: DateTime<Utc>,
    charge: f64,
    config: &'c config::Charging,
}

impl<'c> std::fmt::Debug for Goal<'c> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Goal")
            .field("time", &self.time.with_timezone(&Pacific))
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
    #[tracing::instrument(skip(self, current, history, forecast))]
    pub fn can_charge(
        &self,
        now: DateTime<Utc>,
        soc: f64,
        history: &History,
        current: &Moer,
        forecast: &Forecast,
    ) -> (bool, i64) {
        // Don't charge outside of allowed times.
        if !self.allowed_at(now) {
            return (false, -1);
        }

        // Don't charge if the state of charge is bigger than the maximum.
        if soc >= self.max_charge {
            return (false, -1);
        }

        // The config specifies recurring daily goals.  The next recurrence is
        // either today or tomorrow, so generate both as candidates.
        let today = now.with_timezone(&Pacific).date();
        let tomorrow = today.succ();

        let today_goals = self.daily_goals.iter().map(|(time, charge)| Goal {
            time: today.and_time(*time).unwrap().with_timezone(&Utc),
            charge: *charge,
            config: &self,
        });
        let tomorrow_goals = self.daily_goals.iter().map(|(time, charge)| Goal {
            time: tomorrow.and_time(*time).unwrap().with_timezone(&Utc),
            charge: *charge,
            config: &self,
        });

        // Flex charging: aim to complete the rest of the charging a fixed
        // time from now, whatever now is.
        let flex_goal = Goal {
            time: now + Duration::hours(self.flex_charge_hours),
            // Setting this to 1.0 instead of max_charge avoids asymptotic
            // behavior near the top of the charge state.
            charge: 1.0,
            config: &self,
        };

        let mut goals = std::iter::once(flex_goal)
            .chain(today_goals)
            .chain(tomorrow_goals)
            // Only retain future goals
            .filter(|goal| goal.time > now)
            .collect::<Vec<Goal>>();

        // Choose the goal with the largest required charging proportion.
        goals.sort_by(|a, b| {
            let a_req = a.required_charging_proportion(now, soc);
            let b_req = b.required_charging_proportion(now, soc);

            a_req.partial_cmp(&b_req).unwrap()
        });
        tracing::info!(?goals);
        let goal = goals.pop().expect("must have at least one goal");
        tracing::info!(?goal, "selected goal");

        let available_charging_hours = goal.available_charging_hours(now);
        let required_charging_proportion = goal.required_charging_proportion(now, soc);

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
        let can_charge = current_rate <= emissions_limit;

        // Finally, do tracing and logging of the factors for the decision.

        tracing::info!(
            now = ?now.with_timezone(&Pacific),
            goal.time = ?now.with_timezone(&Pacific),
            ?soc,
            ?goal.charge,
            ?required_charging_proportion,
            ?emissions_limit,
            ?current_rate,
            ?can_charge,
        );

        metrics::gauge!("soc", soc);
        metrics::gauge!("available_charging_hours", available_charging_hours);
        metrics::gauge!("required_charging_proportion", required_charging_proportion);
        metrics::gauge!("target_charge", goal.charge);

        let g_to_kg = |g: u64| (g as f64) / 1000.;
        let emissions_quantile = |q: f64| g_to_kg(emissions.value_at_quantile(q));
        metrics::gauge!("emissions_min", emissions_quantile(0.00));
        metrics::gauge!("emissions_q10", emissions_quantile(0.10));
        metrics::gauge!("emissions_q25", emissions_quantile(0.25));
        metrics::gauge!("emissions_q50", emissions_quantile(0.50));
        metrics::gauge!("emissions_q75", emissions_quantile(0.75));
        metrics::gauge!("emissions_q90", emissions_quantile(0.90));
        metrics::gauge!("emissions_max", emissions_quantile(1.00));
        metrics::gauge!("emissions_current", g_to_kg(current_rate));
        metrics::gauge!("emissions_limit", g_to_kg(emissions_limit));

        (can_charge, emissions_limit as i64)
    }
}

pub async fn start(
    charging: config::Charging,
    mut sgip: SgipSignal,
    vehicle: Vehicle,
) -> Result<(), Error> {
    let next_window = || {
        let now = Utc::now().timestamp();
        // Next 5-minute interval.
        let next = now + (300 - now.rem_euclid(300));
        Utc.timestamp(next, 0)
    };

    loop {
        tracing::info!("Fetching current MOER");
        let current = sgip.moer(charging.region).await?;
        if charging.allowed_at(Utc::now()) {
            // This can be slow, so start in now in another task
            // and come back to it.
            let vehicle2 = vehicle.clone();
            let charge_state = tokio::spawn(async move {
                tracing::info!("waking vehicle");
                vehicle2.wake().await?;
                vehicle2.charge_state().await
            });

            // TODO: don't download these every time
            tracing::info!("Fetching SGIP data");
            let forecast = sgip.forecast(charging.region).await?;
            let history = History::new(
                charging.region,
                sgip.historic_moers(
                    charging.region,
                    Utc::now() - Duration::hours(2 * charging.flex_charge_hours),
                    None,
                )
                .await?,
            );

            // Ensure the car is online
            let charge_state = charge_state.await??;
            tracing::debug!(?charge_state);

            let soc = charge_state.battery_level as f64 / 100.;
            tracing::info!(?soc);

            if charging
                .can_charge(Utc::now(), soc, &history, &current, &forecast)
                .0
            {
                let rsp = vehicle.charge_start().await;
                tracing::info!(?rsp, "charge start");
            } else {
                let rsp = vehicle.charge_stop().await;
                tracing::info!(?rsp, "charge stop");
            }
        } else {
            // Log the current MOER anyways, for metrics dashboards.
            metrics::gauge!("emissions_current", current.rate);
            tracing::info!("Not allowed to charge, sleeping");
        }

        let next = next_window();
        tokio::time::sleep((next - Utc::now()).to_std().unwrap()).await;
    }
}
