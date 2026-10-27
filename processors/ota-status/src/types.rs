use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct OtaEvent {
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub event_id: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub vehicle_id: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub campaign_id: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub progress_pct: Option<f64>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub event_ts: Option<i64>,
    #[serde(default, deserialize_with = "telemetry_core::lenient::lenient")]
    pub schema_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OtaStatus {
    Downloading,
    Installing,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OtaStatusRecord {
    pub event_id: String,
    pub vehicle_id: String,
    pub campaign_id: String,
    pub status: OtaStatus,
    pub progress_pct: f64,
    pub event_ts: i64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DlqReason {
    MissingField,
    UnknownStatus,
}

impl DlqReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingField => "MISSING_FIELD",
            Self::UnknownStatus => "UNKNOWN_STATUS",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DlqRecord {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub reason: DlqReason,
}
