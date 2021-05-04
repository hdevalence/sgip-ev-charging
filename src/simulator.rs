use anyhow::Error;
use chrono::{DateTime, Duration, Utc};
use chrono_tz::US::Pacific;
use serde::Serialize;
use sgip_signal::{Forecast, GridRegion, SgipSignal};
use std::{collections::BTreeMap, fmt, ops::Range};

use crate::Config;
use crate::{ChargeController, History};

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
    start: DateTime<Utc>,
    records: Vec<Record>,
    s10_controller: ChargeController,
    s30_controller: ChargeController,
    s50_controller: ChargeController,
    s70_controller: ChargeController,
}
struct F(pub f64, pub usize);

impl std::fmt::Debug for F {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{0:.1$}", self.0, self.1)
    }
}

impl Simulator {
    pub fn new(config: Config, start: DateTime<Utc>) -> Self {
        let charging = config.charging.clone();
        Self {
            config,
            start,
            records: vec![Record {
                time: start,
                time_str: format!("{}", start.with_timezone(&Pacific).time()),
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
            s10_controller: ChargeController::new(charging.clone(), start),
            s30_controller: ChargeController::new(charging.clone(), start),
            s50_controller: ChargeController::new(charging.clone(), start),
            s70_controller: ChargeController::new(charging.clone(), start),
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
        let end = self.start + Duration::hours(self.config.charging.flex_charge_hours);

        let forecasts = ForecastTable::crawl(&mut sgip, region, self.start..end).await?;
        let history = History::new(
            region,
            sgip.historic_moers(region, self.start, Some(end + Duration::hours(1)))
                .await?,
        );

        let step = Duration::minutes(5);
        let mut now = self.records.last().expect("records is nonempty").time;
        while now <= end {
            now = now + step;

            let moer = history.at(now).unwrap();
            let forecast = forecasts.at(now).unwrap();

            let emissions = (moer.rate * 1000.) as u64;

            let mut s10_soc = self.records.last().unwrap().s10_soc;
            let mut s30_soc = self.records.last().unwrap().s30_soc;
            let mut s50_soc = self.records.last().unwrap().s50_soc;
            let mut s70_soc = self.records.last().unwrap().s70_soc;

            let (s10_charge_now, s10_emissions_limit) = self
                .s10_controller
                .can_charge(now, s10_soc, &history, &moer, &forecast);

            let (s30_charge_now, s30_emissions_limit) = self
                .s30_controller
                .can_charge(now, s30_soc, &history, &moer, &forecast);

            let (s50_charge_now, s50_emissions_limit) = self
                .s50_controller
                .can_charge(now, s50_soc, &history, &moer, &forecast);

            let (s70_charge_now, s70_emissions_limit) = self
                .s70_controller
                .can_charge(now, s70_soc, &history, &moer, &forecast);

            tracing::info!(
                now = ?now.with_timezone(&Pacific).time(),
                emissions,
                s10_l = s10_emissions_limit,
                s30_l = s30_emissions_limit,
                s50_l = s50_emissions_limit,
                s70_l = s70_emissions_limit,
                s10_soc = ?F(s10_soc, 3),
                //s10_charge_now,
                s30_soc = ?F(s30_soc, 3),
                //s30_charge_now,
                s50_soc = ?F(s50_soc, 3),
                //s50_charge_now,
                s70_soc = ?F(s70_soc, 3),
                //s70_charge_now,
            );

            let delta = self.config.charging.charge_rate_kw * (step.num_minutes() as f64 / 60.0);
            let delta_pct = delta / self.config.charging.capacity_kwh;

            s10_soc += if s10_charge_now { delta_pct } else { 0. };
            s30_soc += if s30_charge_now { delta_pct } else { 0. };
            s50_soc += if s50_charge_now { delta_pct } else { 0. };
            s70_soc += if s70_charge_now { delta_pct } else { 0. };

            self.records.push(Record {
                time: now,
                time_str: format!("{}", now.with_timezone(&Pacific).time()),
                s10_soc,
                s30_soc,
                s50_soc,
                s70_soc,
                emissions,
                s10_emissions_limit,
                s30_emissions_limit,
                s50_emissions_limit,
                s70_emissions_limit,
            });
        }

        Ok(())
    }
}

struct ForecastTable {
    data: BTreeMap<DateTime<Utc>, Forecast>,
}

impl ForecastTable {
    pub fn at(&self, time: DateTime<Utc>) -> Option<&Forecast> {
        self.data
            .range(..=time)
            .next_back()
            .map(|(_, forecast)| forecast)
    }

    pub async fn crawl(
        sgip: &mut SgipSignal,
        region: GridRegion,
        range: Range<DateTime<Utc>>,
    ) -> Result<Self, Error> {
        let mut data = BTreeMap::default();

        let mut start = range.start;
        let step = Duration::days(1);
        while start < range.end {
            let end = std::cmp::min(start + step, range.end);
            for forecast in sgip.historic_forecasts(region, start, end).await? {
                data.insert(forecast.generated_at, forecast);
            }
            start = start + step;
        }

        Ok(Self { data })
    }
}
