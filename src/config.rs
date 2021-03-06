use anyhow::{anyhow, Error};
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use sgip_signal::GridRegion;

pub trait Validate: Sized {
    fn validate(self) -> Result<Self, Error>;
}

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    pub charging: Charging,
    pub tesla_credentials: TeslaCredentials,
    pub sgip_credentials: SgipCredentials,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Charging {
    pub region: GridRegion,
    pub allowed_times: Vec<(NaiveTime, NaiveTime)>,
    pub charge_rate_kw: f64,
    pub capacity_kwh: f64,
    pub max_charge: f64,
    pub flex_charge_hours: i64,
    pub daily_goals: Vec<(NaiveTime, f64)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TeslaCredentials {
    pub tesla_username: String,
    pub tesla_password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SgipCredentials {
    pub sgip_username: String,
    pub sgip_password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Simulator {
    pub capacity: f64,
    pub charge_rate: f64,
}

impl Validate for Config {
    fn validate(self) -> Result<Self, Error> {
        Ok(Self {
            charging: self.charging.validate()?,
            tesla_credentials: self.tesla_credentials.validate()?,
            sgip_credentials: self.sgip_credentials.validate()?,
        })
    }
}

impl Validate for Charging {
    fn validate(self) -> Result<Self, Error> {
        if !(0.0..1.0).contains(&self.max_charge) {
            return Err(anyhow!(
                "max_charge {} must be in range [0.0, 1.0)",
                self.max_charge
            ));
        }
        if !(0..(7 * 24)).contains(&self.flex_charge_hours) {
            return Err(anyhow!(
                "flex_charge_hours {} must be in range [0, {})",
                self.flex_charge_hours,
                7 * 24,
            ));
        }
        for (_time, charge) in &self.daily_goals {
            if !(0.0..1.0).contains(charge) {
                return Err(anyhow!(
                    "goal charge {} must be in range [0.0, 1.0)",
                    charge
                ));
            }
        }

        if self.allowed_times.is_empty() {
            return Err(anyhow!("must specify at least one allowed charging time"));
        }

        for (start, end) in &self.allowed_times {
            if start >= end {
                return Err(anyhow!(
                    "specified charging time with start {} >= end {}",
                    start,
                    end
                ));
            }
        }

        for ((_, prev_end), (next_start, _)) in self
            .allowed_times
            .iter()
            .zip(self.allowed_times.iter().skip(1))
        {
            if prev_end >= next_start {
                return Err(anyhow!(
                    "charging times must be nonoverlapping and sorted, but prev_end {} >= next_start {}",
                    prev_end,
                    next_start,
                ));
            }
        }

        Ok(self)
    }
}

impl Validate for TeslaCredentials {
    fn validate(self) -> Result<Self, Error> {
        if self == Self::default() {
            return Err(anyhow!(
                "tesla credentials must be changed from default values"
            ));
        }
        Ok(self)
    }
}

impl Validate for SgipCredentials {
    fn validate(self) -> Result<Self, Error> {
        if self == Self::default() {
            return Err(anyhow!(
                "sgip credentials must be changed from default values"
            ));
        }
        Ok(self)
    }
}

impl Default for Charging {
    fn default() -> Charging {
        Charging {
            region: GridRegion::CAISO_PGE,
            allowed_times: vec![(
                NaiveTime::from_hms(00, 00, 00),
                NaiveTime::from_hms(15, 00, 00),
            )],
            max_charge: 0.85,
            capacity_kwh: 75.,
            charge_rate_kw: 8.,
            flex_charge_hours: 24,
            daily_goals: vec![
                (NaiveTime::from_hms(8, 0, 0), 0.33),
                (NaiveTime::from_hms(15, 0, 0), 0.66),
            ],
        }
    }
}

impl Default for TeslaCredentials {
    fn default() -> Self {
        TeslaCredentials {
            tesla_username: "your_tesla_username".to_string(),
            tesla_password: "your_tesla_password".to_string(),
        }
    }
}

impl Default for SgipCredentials {
    fn default() -> Self {
        SgipCredentials {
            sgip_username: "your_sgip_username".to_string(),
            sgip_password: "your_sgip_password".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_round_trip() {
        let config = Config::default();

        let tomled = toml::to_string_pretty(&config).unwrap();

        println!("{}", tomled);

        let config2: Config = toml::from_str(&tomled).unwrap();

        assert_eq!(config, config2);
    }
}
