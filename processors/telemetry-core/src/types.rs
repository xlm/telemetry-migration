use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChargingState {
    PluggedIn,
    Charging,
    Paused,
    Complete,
    Unplugged,
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChargingEvent {
    pub event_id: String,
    pub vehicle_id: String,
    pub session_id: Option<String>,
    pub event_ts: i64,
    pub ingest_ts: i64,
    pub seq: i64,
    pub charging_state: ChargingState,
    pub soc_pct: Option<f64>,
    pub power_kw: Option<f64>,
    pub location: Option<Location>,
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtaEvent {
    pub event_id: String,
    pub vehicle_id: String,
    pub campaign_id: String,
    pub status: String,
    pub progress_pct: f64,
    pub event_ts: i64,
    pub schema_version: String,
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
pub enum OtaStatus {
    Downloading,
    Installing,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    UnknownState,
    SocOutOfRange,
    NegativePower,
    Duplicate,
    Late,
    UnknownStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DlqRecord {
    pub event_id: Option<String>,
    pub vehicle_id: Option<String>,
    pub reason: DlqReason,
}

pub enum Emitted {
    Summary(ChargingSummary),
    Dlq(DlqRecord),
}

#[cfg(test)]
mod tests {
    use super::{ChargingEvent, ChargingState, CloseReason, OtaStatus};

    #[test]
    fn nullable_charging_event_round_trips_through_serde() {
        let event = ChargingEvent {
            event_id: "event-1".to_owned(),
            vehicle_id: "VH-12345678".to_owned(),
            session_id: None,
            event_ts: 1_700_000_000_000,
            ingest_ts: 1_700_000_000_100,
            seq: 1,
            charging_state: ChargingState::Charging,
            soc_pct: None,
            power_kw: Some(42.5),
            location: None,
            schema_version: "1".to_owned(),
        };

        let json = serde_json::to_string(&event).expect("serialize charging event");
        let decoded: ChargingEvent =
            serde_json::from_str(&json).expect("deserialize charging event");

        assert!(json.contains(r#""session_id":null"#));
        assert!(json.contains(r#""soc_pct":null"#));
        assert!(json.contains(r#""location":null"#));
        assert_eq!(decoded, event);
    }

    #[test]
    fn output_enums_round_trip_through_serde() {
        let close_reason = CloseReason::Timeout;
        let close_json = serde_json::to_string(&close_reason).expect("serialize close reason");
        assert_eq!(close_json, r#""TIMEOUT""#);
        assert_eq!(
            serde_json::from_str::<CloseReason>(&close_json).expect("deserialize close reason"),
            close_reason
        );

        let ota_status = OtaStatus::Succeeded;
        let ota_json = serde_json::to_string(&ota_status).expect("serialize OTA status");
        assert_eq!(ota_json, r#""SUCCEEDED""#);
        assert_eq!(
            serde_json::from_str::<OtaStatus>(&ota_json).expect("deserialize OTA status"),
            ota_status
        );
    }

    #[test]
    fn unknown_charging_state_is_a_deserialization_error() {
        let json = r#"{"event_id":"event-1","vehicle_id":"VH-12345678","session_id":null,"event_ts":1700000000000,"ingest_ts":1700000000100,"seq":1,"charging_state":"UNKNOWN","soc_pct":null,"power_kw":null,"location":null,"schema_version":"1"}"#;

        assert!(serde_json::from_str::<ChargingEvent>(json).is_err());
    }
}
