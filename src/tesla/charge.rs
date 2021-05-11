use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ChargeState {
    pub battery_heater_on: bool,
    pub battery_level: u32,
    pub battery_range: f32,
    pub charge_current_request: u32,
    pub charge_current_request_max: u32,
    pub charge_enable_request: bool,
    pub charge_energy_added: f32,
    pub charge_limit_soc: u32,
    pub charge_limit_soc_max: u32,
    pub charge_limit_soc_min: u32,
    pub charge_limit_soc_std: u32,
    pub charge_miles_added_ideal: f32,
    pub charge_miles_added_rated: f32,
    pub charge_port_cold_weather_mode: bool,
    pub charge_port_door_open: bool,
    pub charge_port_latch: String,
    pub charge_rate: f32,
    pub charge_to_max_range: bool,
    pub charger_actual_current: u32,
    pub charger_phases: u32,
    pub charger_pilot_current: u32,
    pub charger_power: u32,
    pub charger_voltage: u32,
    pub charging_state: String,
    pub conn_charge_cable: String,
    pub est_battery_range: f32,
    pub fast_charger_brand: String,
    pub fast_charger_present: bool,
    pub fast_charger_type: String,
    pub ideal_battery_range: f32,
    pub managed_charging_active: bool,
    //pub managed_charging_start_time: Null,
    pub managed_charging_user_canceled: bool,
    pub max_range_charge_counter: u32,
    pub minutes_to_full_charge: u32,
    pub not_enough_power_to_heat: Option<bool>,
    pub scheduled_charging_pending: bool,
    pub scheduled_charging_start_time: i64,
    pub time_to_full_charge: f64,
    pub timestamp: i64,
    pub trip_charging: bool,
    pub usable_battery_level: u32,
    //pub user_charge_enable_request: Null,
}