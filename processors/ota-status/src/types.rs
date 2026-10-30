use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OtaEvent {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub campaign_id: Option<String>,
    pub status: Option<String>,
    pub progress_pct: Option<f64>,
    pub event_ts: Option<i64>,
    pub schema_version: Option<String>,
}

fn non_finite(raw: &RawValue) -> bool {
    let text = raw.get();
    match text.as_bytes()[0] {
        b'{' => serde_json::from_str::<HashMap<String, Box<RawValue>>>(text)
            .is_ok_and(|fields| fields.values().any(|value| non_finite(value))),
        b'[' => serde_json::from_str::<Vec<Box<RawValue>>>(text)
            .is_ok_and(|values| values.iter().any(|value| non_finite(value))),
        b'-' | b'0'..=b'9' if text.contains(['.', 'e', 'E']) => {
            text.parse::<f64>().is_ok_and(|value| !value.is_finite())
        }
        _ => false,
    }
}

impl<'de> Deserialize<'de> for OtaEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields = HashMap::<String, Box<RawValue>>::deserialize(deserializer)?;
        let names = [
            "event_id",
            "vehicle_id",
            "campaign_id",
            "status",
            "progress_pct",
            "event_ts",
            "schema_version",
        ];
        if names
            .iter()
            .filter_map(|name| fields.get(*name))
            .any(|value| non_finite(value))
        {
            return Ok(Self::default());
        }
        let string = |name| {
            fields
                .get(name)
                .and_then(|value| serde_json::from_str(value.get()).ok())
        };
        let progress_pct = fields
            .get("progress_pct")
            .and_then(|value| value.get().parse::<f64>().ok());
        let event_ts = fields
            .get("event_ts")
            .and_then(|value| value.get().parse::<i64>().ok());
        Ok(Self {
            event_id: string("event_id"),
            vehicle_id: string("vehicle_id"),
            campaign_id: string("campaign_id"),
            status: string("status"),
            progress_pct,
            event_ts,
            schema_version: string("schema_version"),
        })
    }
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
