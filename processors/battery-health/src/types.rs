use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatteryEvent {
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub event_id: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub vehicle_id: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub battery_id: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub soh_pct: Option<f64>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub event_ts: Option<i64>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub schema_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthRecord {
    pub event_id: String,
    pub vehicle_id: String,
    pub battery_id: String,
    pub soh_pct: f64,
    pub health: String,
    pub event_ts: i64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DlqRecord {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub reason: &'static str,
}
