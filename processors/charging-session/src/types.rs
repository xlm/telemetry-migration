use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChargingState {
    PluggedIn,
    Charging,
    Paused,
    Complete,
    Unplugged,
    Heartbeat,
}

impl ChargingState {
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "PLUGGED_IN" => Some(Self::PluggedIn),
            "CHARGING" => Some(Self::Charging),
            "PAUSED" => Some(Self::Paused),
            "COMPLETE" => Some(Self::Complete),
            "UNPLUGGED" => Some(Self::Unplugged),
            "HEARTBEAT" => Some(Self::Heartbeat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ChargingEvent {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub session_id: Option<String>,
    pub event_ts: Option<i64>,
    pub ingest_ts: Option<i64>,
    pub seq: Option<i64>,
    pub charging_state: Option<String>,
    pub soc_pct: Option<f64>,
    pub power_kw: Option<f64>,
    pub location: Option<Location>,
    pub schema_version: Option<String>,
}

struct Optional<T>(Option<T>);

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Optional<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        telemetry_core::lenient::lenient(deserializer).map(Self)
    }
}

impl<'de> Deserialize<'de> for ChargingEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;

        impl<'de> serde::de::Visitor<'de> for ObjectVisitor {
            type Value = ChargingEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a charging event object")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut event = ChargingEvent::default();
                // Jackson keeps the last value for repeated object keys.
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "event_id" => event.event_id = map.next_value::<Optional<String>>()?.0,
                        "vehicle_id" => event.vehicle_id = map.next_value::<Optional<String>>()?.0,
                        "session_id" => event.session_id = map.next_value::<Optional<String>>()?.0,
                        "event_ts" => event.event_ts = map.next_value::<Optional<i64>>()?.0,
                        "ingest_ts" => event.ingest_ts = map.next_value::<Optional<i64>>()?.0,
                        "seq" => event.seq = map.next_value::<Optional<i64>>()?.0,
                        "charging_state" => {
                            event.charging_state = map.next_value::<Optional<String>>()?.0
                        }
                        "soc_pct" => event.soc_pct = map.next_value::<Optional<f64>>()?.0,
                        "power_kw" => event.power_kw = map.next_value::<Optional<f64>>()?.0,
                        "location" => event.location = map.next_value::<Optional<Location>>()?.0,
                        "schema_version" => {
                            event.schema_version = map.next_value::<Optional<String>>()?.0
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(event)
            }
        }

        // Derived struct deserializers also accept positional arrays, unlike the Job.
        deserializer.deserialize_map(ObjectVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CloseReason {
    Complete,
    Unplugged,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChargingSummary {
    pub session_id: String,
    pub vehicle_id: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub duration_s: i64,
    pub start_soc_pct: Option<f64>,
    pub end_soc_pct: Option<f64>,
    pub energy_kwh: f64,
    pub peak_power_kw: f64,
    pub event_count: u64,
    pub close_reason: CloseReason,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DlqReason {
    MissingField,
    UnknownState,
    SocOutOfRange,
    NegativePower,
    Duplicate,
    Late,
}

impl DlqReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingField => "MISSING_FIELD",
            Self::UnknownState => "UNKNOWN_STATE",
            Self::SocOutOfRange => "SOC_OUT_OF_RANGE",
            Self::NegativePower => "NEGATIVE_POWER",
            Self::Duplicate => "DUPLICATE",
            Self::Late => "LATE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DlqRecord {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub reason: DlqReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Emitted {
    Summary(ChargingSummary),
    Dlq(DlqRecord),
}

#[cfg(test)]
mod tests {
    use super::{ChargingEvent, ChargingState, ChargingSummary, CloseReason, Location};

    #[test]
    fn charging_event_and_location_round_trip_through_serde() {
        let event = ChargingEvent {
            event_id: Some("event-1".to_owned()),
            vehicle_id: Some("VH-12345678".to_owned()),
            session_id: None,
            event_ts: Some(1_700_000_000_000),
            ingest_ts: Some(1_700_000_000_100),
            seq: Some(1),
            charging_state: Some("UNKNOWN".to_owned()),
            soc_pct: None,
            power_kw: Some(42.5),
            location: Some(Location {
                lat: 40.7128,
                lon: -74.006,
            }),
            schema_version: Some("1".to_owned()),
        };

        let json = serde_json::to_string(&event).expect("serialize charging event");
        let decoded: ChargingEvent =
            serde_json::from_str(&json).expect("deserialize charging event");

        assert_eq!(decoded, event);
        assert_eq!(decoded.charging_state.as_deref(), Some("UNKNOWN"));
    }

    #[test]
    fn missing_event_fields_deserialize_as_none() {
        let event: ChargingEvent = serde_json::from_str("{}").expect("missing fields");

        assert_eq!(event, ChargingEvent::default());
    }

    #[test]
    fn repeated_fields_keep_last_value_like_jackson() {
        let event: ChargingEvent = serde_json::from_str(
            r#"{"event_id":"first","event_id":"last","seq":1,"seq":2,"soc_pct":10,"soc_pct":"bad"}"#,
        ).unwrap();
        assert_eq!(event.event_id.as_deref(), Some("last"));
        assert_eq!(event.seq, Some(2));
        assert_eq!(event.soc_pct, None);
    }

    #[test]
    fn wrong_typed_fields_are_absent_without_coercion() {
        let event: ChargingEvent = serde_json::from_str(
            r#"{"event_id":1,"vehicle_id":true,"session_id":[],"event_ts":1.5,
                "ingest_ts":"1","seq":9223372036854775808,"charging_state":{},
                "soc_pct":"25","power_kw":false,"location":{"lat":"1","lon":2},
                "schema_version":1}"#,
        )
        .unwrap();
        assert_eq!(event, ChargingEvent::default());
        for number in [
            "-9223372036854775809",
            "1.0",
            "\"１\"",
            "\"1e3\"",
            "true",
            "null",
        ] {
            let event: ChargingEvent =
                serde_json::from_str(&format!(r#"{{"seq":{number}}}"#)).unwrap();
            assert_eq!(event.seq, None, "{number}");
        }
    }

    #[test]
    fn parser_rejects_corrupt_roots_and_nonfinite_declared_fields_but_ignores_unknown_fields() {
        for json in [
            "{",
            "[]",
            "null",
            "\"text\"",
            "1",
            r#"{"power_kw":NaN}"#,
            r#"{"soc_pct":1e400}"#,
            r#"{"location":{"lat":1e400,"lon":2}}"#,
            r#"{"event_id":{"nested":1e400}}"#,
        ] {
            assert!(
                serde_json::from_str::<ChargingEvent>(json).is_err(),
                "{json}"
            );
        }
        let event: ChargingEvent =
            serde_json::from_str(r#"{"extra":1e400,"event_id":"e"}"#).unwrap();
        assert_eq!(event.event_id.as_deref(), Some("e"));
    }

    #[test]
    fn from_wire_matches_only_exact_state_names() {
        assert_eq!(
            ChargingState::from_wire("PLUGGED_IN"),
            Some(ChargingState::PluggedIn)
        );
        assert_eq!(
            ChargingState::from_wire("HEARTBEAT"),
            Some(ChargingState::Heartbeat)
        );
        assert_eq!(ChargingState::from_wire("charging"), None);
        assert_eq!(ChargingState::from_wire("UNKNOWN"), None);
    }

    #[test]
    fn charging_summary_has_only_the_contract_properties() {
        let summary = ChargingSummary {
            session_id: "CS-12345678".to_owned(),
            vehicle_id: "VH-12345678".to_owned(),
            start_ts: 1,
            end_ts: 2,
            duration_s: 0,
            start_soc_pct: None,
            end_soc_pct: None,
            energy_kwh: 0.0,
            peak_power_kw: 0.0,
            event_count: 1,
            close_reason: CloseReason::Complete,
            schema_version: "1".to_owned(),
        };
        let serialized = serde_json::to_value(summary).expect("serialize summary");
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/charging-session.v1.schema.json"
        ))
        .expect("charging session contract");
        let properties = contract["properties"]
            .as_object()
            .expect("contract properties");
        let actual = serialized
            .as_object()
            .expect("summary object")
            .keys()
            .collect::<Vec<_>>();
        let expected = properties.keys().collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert!(!serialized.as_object().unwrap().contains_key("location"));
    }
}
