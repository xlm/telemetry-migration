use serde::de::{DeserializeOwned, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BatteryEvent {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub battery_id: Option<String>,
    pub soh_pct: Option<f64>,
    pub event_ts: Option<i64>,
    pub schema_version: Option<String>,
}

struct OptionalField<T>(Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for OptionalField<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        telemetry_core::lenient::lenient(deserializer).map(Self)
    }
}

impl<'de> Deserialize<'de> for BatteryEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EventVisitor;

        impl<'de> Visitor<'de> for EventVisitor {
            type Value = BatteryEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a battery health event object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<BatteryEvent, M::Error> {
                let mut event = BatteryEvent::default();
                // Jackson's tree parser keeps the final value of a duplicate key.
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "event_id" => event.event_id = map.next_value::<OptionalField<String>>()?.0,
                        "vehicle_id" => {
                            event.vehicle_id = map.next_value::<OptionalField<String>>()?.0
                        }
                        "battery_id" => {
                            event.battery_id = map.next_value::<OptionalField<String>>()?.0
                        }
                        "soh_pct" => event.soh_pct = map.next_value::<OptionalField<f64>>()?.0,
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
