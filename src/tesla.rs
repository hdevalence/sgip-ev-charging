mod auth;
mod charge;
mod vehicle;

static BASE_URL: &str = "https://owner-api.teslamotors.com/";

pub use auth::AccessToken;
pub use charge::ChargeState;
pub use vehicle::{Vehicle, VehicleData};
