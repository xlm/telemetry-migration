use serde::de::{DeserializeOwned, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Serialize};

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

struct OptionalField<T>(Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for OptionalField<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        telemetry_core::lenient::lenient(deserializer).map(Self)
    }
}

impl<'de> Deserialize<'de> for OtaEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EventVisitor;

        impl<'de> Visitor<'de> for EventVisitor {
            type Value = OtaEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an OTA event object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<OtaEvent, M::Error> {
                let mut event = OtaEvent::default();
                // Jackson's tree parser keeps the final value of a duplicate key.
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "event_id" => event.event_id = map.next_value::<OptionalField<String>>()?.0,
                        "vehicle_id" => {
                            event.vehicle_id = map.next_value::<OptionalField<String>>()?.0
                        }
                        "campaign_id" => {
                            event.campaign_id = map.next_value::<OptionalField<String>>()?.0
                        }
                        "status" => event.status = map.next_value::<OptionalField<String>>()?.0,
                        "progress_pct" => {
                            event.progress_pct = map.next_value::<OptionalField<f64>>()?.0
                        }
                        "event_ts" => event.event_ts = map.next_value::<OptionalField<i64>>()?.0,
                        "schema_version" => {
                            event.schema_version = map.next_value::<OptionalField<String>>()?.0
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(event)
            }
        }

        deserializer.deserialize_map(EventVisitor)
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
