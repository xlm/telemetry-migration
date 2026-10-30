use crate::types::{DlqReason, DlqRecord, OtaEvent, OtaStatus, OtaStatusRecord};

pub fn process(event: &OtaEvent) -> Result<OtaStatusRecord, DlqRecord> {
    let reject = |reason| DlqRecord {
        event_id: event.event_id.clone(),
        vehicle_id: event.vehicle_id.clone(),
        reason,
    };
    let (
        Some(event_id),
        Some(vehicle_id),
        Some(campaign_id),
        Some(status),
        Some(progress_pct),
        Some(event_ts),
    ) = (
        &event.event_id,
        &event.vehicle_id,
        &event.campaign_id,
        &event.status,
        event.progress_pct,
        event.event_ts,
    )
    else {
        return Err(reject(DlqReason::MissingField));
    };
    let status = match status.to_lowercase().as_str() {
        "downloading" | "dl" | "download" => OtaStatus::Downloading,
        "installing" | "inst" => OtaStatus::Installing,
        "done" | "complete" | "success" | "succeeded" => OtaStatus::Succeeded,
        "failed" | "error" => OtaStatus::Failed,
        _ => return Err(reject(DlqReason::UnknownStatus)),
    };
    Ok(OtaStatusRecord {
        event_id: event_id.clone(),
        vehicle_id: vehicle_id.clone(),
        campaign_id: campaign_id.clone(),
        status,
        progress_pct: if progress_pct <= 0.0 {
            0.0
        } else {
            progress_pct.min(100.0)
        },
        event_ts,
        schema_version: "1".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use telemetry_core::adapters::jsonl::JsonlSource;
    use telemetry_core::Source;

    use super::*;

    fn classify(line: &str) -> Result<OtaStatusRecord, DlqRecord> {
        let mut source = JsonlSource::from_reader(Cursor::new(line));
        let event: OtaEvent = source
            .read_next()
            .unwrap_or(Some(OtaEvent::default()))
            .unwrap();
        process(&event)
    }

    fn line(status: &str, progress: &str, ts: &str) -> String {
        format!(
            r#"{{"event_id":"e","vehicle_id":"v","campaign_id":"c","status":{status},"progress_pct":{progress},"event_ts":{ts}}}"#
        )
    }

    #[test]
    fn aliases_projection_and_clamping() {
        for (alias, expected) in [
            ("downloading", OtaStatus::Downloading),
            ("DL", OtaStatus::Downloading),
            ("download", OtaStatus::Downloading),
            ("installing", OtaStatus::Installing),
            ("INST", OtaStatus::Installing),
            ("done", OtaStatus::Succeeded),
            ("complete", OtaStatus::Succeeded),
            ("success", OtaStatus::Succeeded),
            ("succeeded", OtaStatus::Succeeded),
            ("failed", OtaStatus::Failed),
            ("error", OtaStatus::Failed),
        ] {
            let record = classify(&line(&format!("\"{alias}\""), "25.75", "-4")).unwrap();
            assert_eq!(
                (
                    record.event_id.as_str(),
                    record.vehicle_id.as_str(),
                    record.campaign_id.as_str()
                ),
                ("e", "v", "c")
            );
            assert_eq!(
                (
                    record.status,
                    record.progress_pct,
                    record.event_ts,
                    record.schema_version.as_str()
                ),
                (expected, 25.75, -4, "1")
            );
        }
        for (input, expected) in [("-1", 0.0), ("0", 0.0), ("100", 100.0), ("101", 100.0)] {
            assert_eq!(
                classify(&line("\"dl\"", input, "0")).unwrap().progress_pct,
                expected
            );
        }
        for input in ["-0.0", "-0"] {
            assert!(classify(&line("\"dl\"", input, "0"))
                .unwrap()
                .progress_pct
                .is_sign_positive());
        }
        assert_eq!(
            classify(&line("\" dl\"", "0", "0")).unwrap_err().reason,
            DlqReason::UnknownStatus
        );
        assert_eq!(
            classify(&line("\"İNST\"", "0", "0")).unwrap_err().reason,
            DlqReason::UnknownStatus
        );
    }

    #[test]
    fn validation_and_precedence() {
        for value in ["null", "true", "\"1\"", "1.0", "9223372036854775808"] {
            let rejected = classify(&line("\"unknown\"", "0", value)).unwrap_err();
            assert_eq!(rejected.reason, DlqReason::MissingField);
            assert_eq!(
                (rejected.event_id.as_deref(), rejected.vehicle_id.as_deref()),
                (Some("e"), Some("v"))
            );
        }
        for value in ["null", "true", "\"25\"", "[]", "{}"] {
            assert_eq!(
                classify(&line("\"dl\"", value, "1")).unwrap_err().reason,
                DlqReason::MissingField
            );
        }
        for field in ["event_id", "vehicle_id", "campaign_id", "status"] {
            let input = line("\"dl\"", "1", "1").replace(
                &format!(
                    "\"{field}\":\"{}\"",
                    match field {
                        "event_id" => "e",
                        "vehicle_id" => "v",
                        "campaign_id" => "c",
                        _ => "dl",
                    }
                ),
                &format!("\"{field}\":42"),
            );
            assert_eq!(
                classify(&input).unwrap_err().reason,
                DlqReason::MissingField
            );
        }
        assert!(classify(r#"{"event_id":"","vehicle_id":"","campaign_id":"","status":"dl","progress_pct":0,"event_ts":0,"schema_version":false}"#).is_ok());
        assert_eq!(
            classify(&line("\"bogus\"", "1", "1")).unwrap_err().reason,
            DlqReason::UnknownStatus
        );
        for input in ["{", "[]", "null"] {
            let rejected = classify(input).unwrap_err();
            assert_eq!(
                (rejected.reason, rejected.event_id, rejected.vehicle_id),
                (DlqReason::MissingField, None, None)
            );
        }
    }

    #[test]
    fn jsonl_duplicate_keys_and_non_finite_values() {
        let base =
            r#""event_id":"e","vehicle_id":"v","campaign_id":"c","status":"dl","event_ts":1"#;
        for fields in [
            "\"progress_pct\":1e400,\"progress_pct\":25",
            "\"progress_pct\":\"wrong\",\"progress_pct\":25",
            "\"progress_pct\":25,\"extra\":1e400",
            "\"progress_pct\":25,\"schema_version\":{\"x\":1e400,\"x\":0}",
        ] {
            assert_eq!(
                classify(&format!("{{{base},{fields}}}"))
                    .unwrap()
                    .progress_pct,
                25.0
            );
        }
        for fields in [
            "\"progress_pct\":25,\"progress_pct\":1e400",
            "\"progress_pct\":25,\"schema_version\":[{\"x\":1e400}]",
            "\"progress_pct\":25,\"schema_version\":{\"x\":0,\"x\":1e400}",
        ] {
            let rejected = classify(&format!("{{{base},{fields}}}")).unwrap_err();
            assert_eq!(
                (rejected.reason, rejected.event_id, rejected.vehicle_id),
                (DlqReason::MissingField, None, None)
            );
        }
        let rejected = classify(&format!(
            "{{{base},\"progress_pct\":25,\"progress_pct\":\"wrong\"}}"
        ))
        .unwrap_err();
        assert_eq!(
            (rejected.reason, rejected.event_id.as_deref()),
            (DlqReason::MissingField, Some("e"))
        );
    }
}
