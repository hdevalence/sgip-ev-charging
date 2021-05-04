#![feature(iter_map_while)]

mod simulator;
mod intervals;
mod controller;
mod chrono_ext;
mod forecast_ext;
mod history;

use chrono_ext::DurationExt;
use forecast_ext::ForecastExt;

pub mod config;

pub use config::{Config, Validate};
pub use simulator::Simulator;
pub use controller::ChargeController;
pub use history::History;
