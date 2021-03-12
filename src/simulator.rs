use anyhow::{Error, anyhow};
use chrono::{DateTime, Duration, Utc};
use chrono_tz::{Tz, US::Pacific};
use serde::{Deserialize, Serialize};
use sgip_signal::SgipSignal;

use crate::histogram::{charging_times, emissions_over_charge_window};
use crate::{config::Vehicle, Config};

#[derive(Serialize, Clone, Debug)]
pub struct Record {
    pub time: DateTime<Utc>,
    pub time_str: String,
    pub s10_soc: f64,
    pub s30_soc: f64,
    pub s50_soc: f64,
    pub s70_soc: f64,
    pub emissions: u64,
    pub s10_emissions_limit: u64,
    pub s30_emissions_limit: u64,
    pub s50_emissions_limit: u64,
    pub s70_emissions_limit: u64,
}

#[derive(Clone, Debug)]
pub struct Simulator {
    config: Config,
    vehicle: Vehicle,
    records: Vec<Record>,
}

impl Simulator {
    pub fn new(config: Config, vehicle: Vehicle, start: DateTime<Utc>) -> Self {
        Self {
            config,
            vehicle,
            records: vec![Record {
                time: start,
                time_str: start.with_timezone(&Pacific).to_rfc3339(),
                s10_soc: 0.1,
                s30_soc: 0.3,
                s50_soc: 0.5,
                s70_soc: 0.7,
                emissions: 0,
                s10_emissions_limit: 0,
                s30_emissions_limit: 0,
                s50_emissions_limit: 0,
                s70_emissions_limit: 0,
            }],
        }
    }

    pub fn take_records(self) -> Vec<Record> {
        self.records
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        let mut sgip = SgipSignal::login(
            &self.config.sgip_credentials.sgip_username,
            &self.config.sgip_credentials.sgip_password,
        )
        .await?;
        let region = self.config.charging.region;
        let target_charge = self.config.charging.target_charge;

        let mut now = self.records.last().expect("records is nonempty").time;
        let charging_times = charging_times(now, &self.config.charging);

        let end = charging_times
            .last()
            .expect("charging_times is nonempty and sorted")
            .end;

        let can_charge =
            |time: DateTime<Utc>| charging_times.iter().any(|range| range.contains(&time));

        let step = Duration::minutes(5);
        while now <= end {
            now = now + step;
            if !can_charge(now) {
                tracing::info!(?now, "charging unavailable, waiting");
                continue;
            }

            tracing::debug!(?now, "charging allowed, checking emissions");

            let forecast = sgip
                .historic_forecasts(region, now, now + step)
                .await?
                .get(0)
                .ok_or_else(|| anyhow!("failed to get historic forecast for {}, {}", now, now + step))?
                .clone();
            let moer = sgip
                .historic_moers(region, now, Some(now + step))
                .await?
                .get(0)
                .ok_or_else(|| anyhow!("failed to get historic moer for {}, {}", now, now + step))?
                .clone();
            let emissions = (moer.rate * 1000.) as u64;

            let emissions_rates = emissions_over_charge_window(&forecast, now, &self.config.charging);

            let mut s10_soc = self.records.last().unwrap().s10_soc;
            let mut s30_soc = self.records.last().unwrap().s30_soc;
            let mut s50_soc = self.records.last().unwrap().s50_soc;
            let mut s70_soc = self.records.last().unwrap().s70_soc;

            let s10_emissions_limit = emissions_rates.value_at_quantile(1.0 - s10_soc * target_charge);
            let s30_emissions_limit = emissions_rates.value_at_quantile(1.0 - s30_soc * target_charge);
            let s50_emissions_limit = emissions_rates.value_at_quantile(1.0 - s50_soc * target_charge);
            let s70_emissions_limit = emissions_rates.value_at_quantile(1.0 - s70_soc * target_charge);

            let s10_charge_now = emissions < s10_emissions_limit && s10_soc < target_charge;
            let s30_charge_now = emissions < s30_emissions_limit && s30_soc < target_charge;
            let s50_charge_now = emissions < s50_emissions_limit && s50_soc < target_charge;
            let s70_charge_now = emissions < s70_emissions_limit && s70_soc < target_charge;

            tracing::info!(
                ?now,
                emissions,
                ?s10_soc,
                s10_emissions_limit,
                //s10_charge_now,
                ?s30_soc,
                s30_emissions_limit,
                //s30_charge_now,
                ?s50_soc,
                s50_emissions_limit,
                //s50_charge_now,
                ?s70_soc,
                s70_emissions_limit,
                //s70_charge_now,
            );

            let delta = self.vehicle.charge_rate * (step.num_minutes() as f64 / 60.0);
            let delta_pct = delta / self.vehicle.capacity;

            s10_soc += if s10_charge_now { delta_pct } else { 0. };
            s30_soc += if s30_charge_now { delta_pct } else { 0. };
            s50_soc += if s50_charge_now { delta_pct } else { 0. };
            s70_soc += if s70_charge_now { delta_pct } else { 0. };

            self.records.push(Record {
                time: now,
                time_str: now.with_timezone(&Pacific).to_rfc3339(),
                s10_soc,
                s30_soc,
                s50_soc,
                s70_soc,
                emissions,
                s10_emissions_limit,
                s30_emissions_limit,
                s50_emissions_limit,
                s70_emissions_limit,
            })
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
    async fn simulate_charging_yesterday() {
        let _ = tracing_subscriber::fmt::try_init();

        use crate::config::Validate;

        let (sgip_username, sgip_password) = env_creds();
        let sgip_credentials = crate::config::SgipCredentials {
            sgip_username,
            sgip_password,
        }
        .validate()
        .unwrap();

        let config = Config {
            sgip_credentials,
            ..Default::default()
        };

        // start 2 days ago to be sure data is available
        let start_day = (Utc::now() - Duration::days(2 + 3)).with_timezone(&Pacific).date();
        let start = start_day.and_hms(23, 0, 0).with_timezone(&Utc);

        let mut sim = Simulator::new(config, Vehicle::default(), start);

        sim.run().await.unwrap();

        let mut writer = csv::Writer::from_writer(std::io::stdout());
        for r in sim.take_records().into_iter() {
            writer.serialize(r).unwrap();
        }
    }
}
