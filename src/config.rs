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
    pub simulator: Simulator,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Charging {
    pub allowed_times: Vec<(NaiveTime, NaiveTime)>,
    pub target_charge: f64,
    pub target_time: NaiveTime,
    pub region: GridRegion,
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
            simulator: self.simulator.validate()?,
        })
    }
}

impl Validate for Charging {
    fn validate(self) -> Result<Self, Error> {
        if !(0.0..1.0).contains(&self.target_charge) {
            return Err(anyhow!(
                "target_charge {} must be in range [0.0, 1.0)",
                self.target_charge
            ));
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

impl Validate for Simulator {
    fn validate(self) -> Result<Self, Error> {
        Ok(self)
    }
}

impl Default for Simulator {
    fn default() -> Self {
        Self {
            capacity: 75.,
            charge_rate: 8.,
        }
    }
}

impl Default for Charging {
    fn default() -> Charging {
        Charging {
            allowed_times: vec![(
                NaiveTime::from_hms(00, 00, 00),
                NaiveTime::from_hms(15, 00, 00),
            )],
            target_charge: 0.85,
            target_time: NaiveTime::from_hms(14, 50, 00),
            region: GridRegion::CAISO_PGE,
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
