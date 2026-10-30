use serde::de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

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

impl<'de> Deserialize<'de> for ChargingEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ChargingEventVisitor;

        impl<'de> Visitor<'de> for ChargingEventVisitor {
            type Value = ChargingEvent;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a charging event object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut event_id = None;
                let mut vehicle_id = None;
                let mut session_id = None;
                let mut event_ts = None;
                let mut ingest_ts = None;
                let mut seq = None;
                let mut charging_state = None;
                let mut soc_pct = None;
                let mut power_kw = None;
                let mut location = None;
                let mut schema_version = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "event_id" => event_id = Some(map.next_value::<serde_json::Value>()?),
                        "vehicle_id" => vehicle_id = Some(map.next_value::<serde_json::Value>()?),
                        "session_id" => session_id = Some(map.next_value::<serde_json::Value>()?),
                        "event_ts" => event_ts = Some(map.next_value::<serde_json::Value>()?),
                        "ingest_ts" => ingest_ts = Some(map.next_value::<serde_json::Value>()?),
                        "seq" => seq = Some(map.next_value::<serde_json::Value>()?),
                        "charging_state" => {
                            charging_state = Some(map.next_value::<serde_json::Value>()?)
                        }
                        "soc_pct" => soc_pct = Some(map.next_value::<serde_json::Value>()?),
                        "power_kw" => power_kw = Some(map.next_value::<serde_json::Value>()?),
                        "location" => location = Some(map.next_value::<serde_json::Value>()?),
                        "schema_version" => {
                            schema_version = Some(map.next_value::<serde_json::Value>()?)
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }

                Ok(ChargingEvent {
                    event_id: decode(event_id),
                    vehicle_id: decode(vehicle_id),
                    session_id: decode(session_id),
                    event_ts: decode(event_ts),
                    ingest_ts: decode(ingest_ts),
                    seq: decode(seq),
                    charging_state: decode(charging_state),
                    soc_pct: decode(soc_pct),
                    power_kw: decode(power_kw),
                    location: decode(location),
                    schema_version: decode(schema_version),
                })
            }
        }

        deserializer.deserialize_map(ChargingEventVisitor)
    }
}

fn decode<T: DeserializeOwned>(value: Option<serde_json::Value>) -> Option<T> {
    value.and_then(|value| serde_json::from_value(value).ok())
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
    fn charging_event_requires_an_object() {
        for input in ["[]", r#""text""#, "42", "null"] {
            assert!(
                serde_json::from_str::<ChargingEvent>(input).is_err(),
                "{input} should not deserialize as an object"
            );
        }
    }

    #[test]
    fn duplicate_keys_keep_the_last_raw_value() {
        let earlier_wrong_later_valid: ChargingEvent =
            serde_json::from_str(r#"{"event_id":7,"event_id":"last","seq":1.0,"seq":2}"#)
                .expect("duplicate object keys");
        assert_eq!(earlier_wrong_later_valid.event_id.as_deref(), Some("last"));
        assert_eq!(earlier_wrong_later_valid.seq, Some(2));

        let earlier_valid_later_wrong: ChargingEvent = serde_json::from_str(
            r#"{"event_id":"first","event_id":7,"soc_pct":25,"soc_pct":"wrong"}"#,
        )
        .expect("duplicate object keys");
        assert_eq!(earlier_valid_later_wrong.event_id, None);
        assert_eq!(earlier_valid_later_wrong.soc_pct, None);
    }

    #[test]
    fn wrong_types_are_absent_and_known_overflow_is_corrupt() {
        let event: ChargingEvent =
            serde_json::from_str(r#"{"soc_pct":"25","seq":1.0}"#).expect("wrong-typed fields");
        assert_eq!(event.soc_pct, None);
        assert_eq!(event.seq, None);

        assert!(
            serde_json::from_str::<ChargingEvent>(r#"{"soc_pct":1e400}"#).is_err(),
            "overflow in a known field should reject the object"
        );
        assert!(
            serde_json::from_str::<ChargingEvent>(r#"{"location":{"lat":1e400,"lon":2}}"#).is_err(),
            "overflow in a nested known field should reject the object"
        );
    }

    #[test]
    fn malformed_optional_location_is_absent() {
        let event: ChargingEvent =
            serde_json::from_str(r#"{"location":{"lat":"40.0","lon":-74.0}}"#)
                .expect("wrong-typed nested field");

        assert_eq!(event.location, None);
    }

    #[test]
    fn overflow_under_an_unknown_key_is_ignored() {
        let event: ChargingEvent = serde_json::from_str(r#"{"event_id":"e","extra":1e400}"#)
            .expect("unknown value is ignored");

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
