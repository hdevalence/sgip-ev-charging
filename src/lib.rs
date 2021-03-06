#![feature(iter_map_while)]

mod chrono_ext;
mod controller;
mod forecast_ext;
mod history;
mod intervals;
mod simulator;
pub mod tesla;

use chrono_ext::DurationExt;
use forecast_ext::ForecastExt;

pub mod config;

pub use config::{Config, Validate};
pub use controller::start;
pub use history::History;
pub use simulator::Simulator;
