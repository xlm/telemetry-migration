use crate::types::{BatteryEvent, DlqRecord, HealthRecord};

pub fn process(event: &BatteryEvent) -> Result<HealthRecord, DlqRecord> {
    let (Some(event_id), Some(vehicle_id), Some(battery_id), Some(soh_pct), Some(event_ts)) = (
        event.event_id.as_ref(),
        event.vehicle_id.as_ref(),
        event.battery_id.as_ref(),
        event.soh_pct,
        event.event_ts,
    ) else {
        return Err(DlqRecord {
            event_id: event.event_id.clone(),
            vehicle_id: event.vehicle_id.clone(),
            reason: "MISSING_FIELD",
        });
    };

    let soh_pct = if soh_pct == 0.0 {
        0.0
    } else {
        soh_pct.clamp(0.0, 100.0)
    };
    let health = if soh_pct >= 80.0 {
        "HEALTHY"
    } else if soh_pct >= 60.0 {
        "DEGRADED"
    } else {
        "REPLACE"
    };

    Ok(HealthRecord {
        event_id: event_id.clone(),
        vehicle_id: vehicle_id.clone(),
        battery_id: battery_id.clone(),
        soh_pct,
        health: health.to_owned(),
        event_ts,
        schema_version: "1".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use telemetry_core::adapters::jsonl::{JsonlError, JsonlSource};
    use telemetry_core::Source;

    use crate::types::{BatteryEvent, HealthRecord};

    use super::process;

    fn parse_line(line: &str) -> BatteryEvent {
        let mut source = JsonlSource::from_reader(Cursor::new(line));
        match source.read_next() {
            Ok(Some(event)) => event,
            Ok(None) => BatteryEvent::default(),
            Err(JsonlError::Json(_)) => BatteryEvent::default(),
            Err(JsonlError::Io(error)) => panic!("JSONL read failed: {error}"),
        }
    }

    fn assert_health(
        event: &BatteryEvent,
        expected_soh: f64,
        expected_health: &str,
    ) -> HealthRecord {
        let record = process(event).expect("health record");
        assert_eq!(record.soh_pct, expected_soh);
        assert_eq!(record.health, expected_health);
        record
    }

    fn assert_dlq(
        event: &BatteryEvent,
        expected_event_id: Option<&str>,
        expected_vehicle_id: Option<&str>,
    ) {
        let record = process(event).expect_err("DLQ record");
        assert_eq!(record.event_id.as_deref(), expected_event_id);
        assert_eq!(record.vehicle_id.as_deref(), expected_vehicle_id);
        assert_eq!(record.reason, "MISSING_FIELD");
    }

    fn valid_event() -> BatteryEvent {
        BatteryEvent {
            event_id: Some("event-1".to_owned()),
            vehicle_id: Some("vehicle-1".to_owned()),
            battery_id: Some("battery-1".to_owned()),
            soh_pct: Some(80.0),
            event_ts: Some(10),
            schema_version: Some("99".to_owned()),
        }
    }

    #[test]
    fn clamps_health_thresholds_and_classifies_the_clamped_value() {
        let cases = [
            (-1.0, 0.0, "REPLACE"),
            (0.0, 0.0, "REPLACE"),
            (59.999, 59.999, "REPLACE"),
            (60.0, 60.0, "DEGRADED"),
            (79.999, 79.999, "DEGRADED"),
            (80.0, 80.0, "HEALTHY"),
            (100.0, 100.0, "HEALTHY"),
            (101.0, 100.0, "HEALTHY"),
        ];

        for (input, expected_soh, expected_health) in cases {
            let mut event = valid_event();
            event.soh_pct = Some(input);

            assert_health(&event, expected_soh, expected_health);
        }
    }

    #[test]
    fn negative_zero_is_clamped_to_positive_zero() {
        let mut event = valid_event();
        event.soh_pct = Some(-0.0);

        let record = assert_health(&event, 0.0, "REPLACE");
        assert!(record.soh_pct.is_sign_positive());
    }

    #[test]
    fn each_missing_required_field_routes_to_dlq_and_retains_identifiers() {
        let cases = [
            (
                r#"{"vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":80,"event_ts":10}"#,
                None,
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","battery_id":"battery-1","soh_pct":80,"event_ts":10}"#,
                Some("event-1"),
                None,
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","soh_pct":80,"event_ts":10}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","event_ts":10}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":80}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
        ];

        for (line, event_id, vehicle_id) in cases {
            assert_dlq(&parse_line(line), event_id, vehicle_id);
        }
    }

    #[test]
    fn schema_version_is_always_one_and_health_precision_is_preserved() {
        let mut event = valid_event();
        event.soh_pct = Some(80.12345);

        let record = assert_health(&event, 80.12345, "HEALTHY");
        assert_eq!(record.schema_version, "1");

        event.schema_version = None;
        assert_eq!(
            assert_health(&event, 80.12345, "HEALTHY").schema_version,
            "1"
        );
    }

    #[test]
    fn empty_identifiers_are_valid() {
        let event = BatteryEvent {
            event_id: Some(String::new()),
            vehicle_id: Some(String::new()),
            battery_id: Some(String::new()),
            soh_pct: Some(80.0),
            event_ts: Some(0),
            schema_version: None,
        };

        let record = process(&event).expect("health record");
        assert_eq!(record.event_id, "");
        assert_eq!(record.vehicle_id, "");
        assert_eq!(record.battery_id, "");
    }

    #[test]
    fn malformed_jsonl_and_nonobjects_route_to_dlq_with_null_identifiers() {
        for line in [
            "{",
            "[]",
            "null",
            r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":1e400,"event_ts":10}"#,
        ] {
            assert_dlq(&parse_line(line), None, None);
        }
    }

    #[test]
    fn wrong_typed_or_null_fields_route_to_dlq_with_available_identifiers() {
        let cases = [
            (
                r#"{"event_id":null,"vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":90,"event_ts":10}"#,
                None,
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":null,"battery_id":"battery-1","soh_pct":90,"event_ts":10}"#,
                Some("event-1"),
                None,
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":null,"soh_pct":90,"event_ts":10}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":null,"event_ts":10}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":90,"event_ts":null}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":"90","event_ts":10}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":123,"soh_pct":90,"event_ts":10}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":90,"event_ts":1.5}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":90,"event_ts":10.0}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
            (
                r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":90,"event_ts":9223372036854775808}"#,
                Some("event-1"),
                Some("vehicle-1"),
            ),
        ];

        for (line, event_id, vehicle_id) in cases {
            assert_dlq(&parse_line(line), event_id, vehicle_id);
        }
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let base = r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":80,"event_ts":10}"#;
        let with_location = r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":80,"event_ts":10,"location":{"lat":12}}"#;
        let with_extra_float = r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":80,"event_ts":10,"extra":1e400}"#;

        let expected = process(&parse_line(base)).expect("base health record");
        let location_record = process(&parse_line(with_location)).expect("location health record");
        let extra_record = process(&parse_line(with_extra_float)).expect("extra health record");
        assert_eq!(location_record.event_id, expected.event_id);
        assert_eq!(location_record.vehicle_id, expected.vehicle_id);
        assert_eq!(location_record.battery_id, expected.battery_id);
        assert_eq!(location_record.soh_pct, expected.soh_pct);
        assert_eq!(location_record.health, expected.health);
        assert_eq!(location_record.event_ts, expected.event_ts);
        assert_eq!(location_record.schema_version, expected.schema_version);
        assert_eq!(extra_record.event_id, expected.event_id);
        assert_eq!(extra_record.vehicle_id, expected.vehicle_id);
        assert_eq!(extra_record.battery_id, expected.battery_id);
        assert_eq!(extra_record.soh_pct, expected.soh_pct);
        assert_eq!(extra_record.health, expected.health);
        assert_eq!(extra_record.event_ts, expected.event_ts);
        assert_eq!(extra_record.schema_version, expected.schema_version);
    }

    #[test]
    fn duplicate_keys_keep_the_final_typed_value() {
        let earlier_wrong = parse_line(
            r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":123,"battery_id":"battery-2","soh_pct":80,"event_ts":10}"#,
        );
        let record = process(&earlier_wrong).expect("later valid duplicate");
        assert_eq!(record.battery_id, "battery-2");

        let earlier_valid = parse_line(
            r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","battery_id":null,"soh_pct":80,"event_ts":10}"#,
        );
        assert_dlq(&earlier_valid, Some("event-1"), Some("vehicle-1"));
    }

    #[test]
    fn integer_soh_values_are_accepted() {
        let event = parse_line(
            r#"{"event_id":"event-1","vehicle_id":"vehicle-1","battery_id":"battery-1","soh_pct":-5,"event_ts":10}"#,
        );

        assert_health(&event, 0.0, "REPLACE");
    }
}
