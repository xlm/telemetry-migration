use crate::types::{DlqReason, DlqRecord, OtaEvent, OtaStatus, OtaStatusRecord};

pub fn process(event: &OtaEvent) -> Result<OtaStatusRecord, DlqRecord> {
    let Some(event_id) = event.event_id.as_ref() else {
        return Err(dlq(event, DlqReason::MissingField));
    };
    let Some(vehicle_id) = event.vehicle_id.as_ref() else {
        return Err(dlq(event, DlqReason::MissingField));
    };
    let Some(campaign_id) = event.campaign_id.as_ref() else {
        return Err(dlq(event, DlqReason::MissingField));
    };
    let Some(status) = event.status.as_deref() else {
        return Err(dlq(event, DlqReason::MissingField));
    };
    let Some(progress_pct) = event.progress_pct else {
        return Err(dlq(event, DlqReason::MissingField));
    };
    let Some(event_ts) = event.event_ts else {
        return Err(dlq(event, DlqReason::MissingField));
    };

    let Some(status) = normalize_status(status) else {
        return Err(dlq(event, DlqReason::UnknownStatus));
    };

    Ok(OtaStatusRecord {
        event_id: event_id.clone(),
        vehicle_id: vehicle_id.clone(),
        campaign_id: campaign_id.clone(),
        status,
        progress_pct: progress_pct.clamp(0.0, 100.0),
        event_ts,
        schema_version: "1".to_owned(),
    })
}

fn dlq(event: &OtaEvent, reason: DlqReason) -> DlqRecord {
    DlqRecord {
        event_id: event.event_id.clone(),
        vehicle_id: event.vehicle_id.clone(),
        reason,
    }
}

fn normalize_status(status: &str) -> Option<OtaStatus> {
    match status.to_ascii_lowercase().as_str() {
        "downloading" | "dl" | "download" => Some(OtaStatus::Downloading),
        "installing" | "inst" => Some(OtaStatus::Installing),
        "done" | "complete" | "success" | "succeeded" => Some(OtaStatus::Succeeded),
        "failed" | "error" => Some(OtaStatus::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::process;
    use crate::types::{DlqReason, OtaEvent, OtaStatus, OtaStatusRecord};

    fn valid_event(status: &str) -> OtaEvent {
        OtaEvent {
            event_id: Some("event-1".to_owned()),
            vehicle_id: Some("VH-12345678".to_owned()),
            campaign_id: Some("campaign-1".to_owned()),
            status: Some(status.to_owned()),
            progress_pct: Some(50.0),
            event_ts: Some(1_700_000_000_000),
            schema_version: Some("99".to_owned()),
        }
    }

    fn expected_status(status: OtaStatus) -> OtaStatusRecord {
        OtaStatusRecord {
            event_id: "event-1".to_owned(),
            vehicle_id: "VH-12345678".to_owned(),
            campaign_id: "campaign-1".to_owned(),
            status,
            progress_pct: 50.0,
            event_ts: 1_700_000_000_000,
            schema_version: "1".to_owned(),
        }
    }

    #[test]
    fn normalizes_status_aliases_case_insensitively() {
        let aliases = [
            ("downloading", OtaStatus::Downloading),
            ("dl", OtaStatus::Downloading),
            ("download", OtaStatus::Downloading),
            ("installing", OtaStatus::Installing),
            ("inst", OtaStatus::Installing),
            ("done", OtaStatus::Succeeded),
            ("complete", OtaStatus::Succeeded),
            ("success", OtaStatus::Succeeded),
            ("failed", OtaStatus::Failed),
            ("error", OtaStatus::Failed),
        ];

        for (alias, expected) in aliases {
            let variants = [
                alias.to_owned(),
                alias.to_ascii_uppercase(),
                alias
                    .chars()
                    .enumerate()
                    .map(|(index, character)| {
                        if index % 2 == 0 {
                            character.to_ascii_uppercase()
                        } else {
                            character
                        }
                    })
                    .collect(),
            ];

            for variant in variants {
                assert_eq!(
                    process(&valid_event(&variant)).expect("recognized status"),
                    expected_status(expected),
                    "status alias {variant}"
                );
            }
        }
    }

    #[test]
    fn accepts_canonical_uppercase_status_names() {
        for (status, expected) in [
            ("DOWNLOADING", OtaStatus::Downloading),
            ("INSTALLING", OtaStatus::Installing),
            ("SUCCEEDED", OtaStatus::Succeeded),
            ("FAILED", OtaStatus::Failed),
        ] {
            assert_eq!(
                process(&valid_event(status)).expect("canonical status"),
                expected_status(expected)
            );
        }
    }

    #[test]
    fn rejects_unknown_and_whitespace_padded_statuses() {
        for status in ["unknown", " downloading "] {
            let error = process(&valid_event(status)).expect_err("unknown status");
            assert_eq!(error.reason, DlqReason::UnknownStatus);
        }
    }

    #[test]
    fn clamps_progress_and_keeps_boundaries() {
        for (input, expected) in [(-1.0, 0.0), (101.0, 100.0), (0.0, 0.0), (100.0, 100.0)] {
            let mut event = valid_event("done");
            event.progress_pct = Some(input);

            assert_eq!(
                process(&event).expect("valid progress").progress_pct,
                expected
            );
        }
    }

    #[test]
    fn each_required_field_missing_is_missing_field_and_preserves_available_ids() {
        let mut event = valid_event("done");
        event.event_id = None;
        assert_missing_field_with_ids(&event, None, Some("VH-12345678"));

        let mut event = valid_event("done");
        event.vehicle_id = None;
        assert_missing_field_with_ids(&event, Some("event-1"), None);

        let mut event = valid_event("done");
        event.campaign_id = None;
        assert_missing_field_with_ids(&event, Some("event-1"), Some("VH-12345678"));

        let mut event = valid_event("done");
        event.status = None;
        assert_missing_field_with_ids(&event, Some("event-1"), Some("VH-12345678"));

        let mut event = valid_event("done");
        event.progress_pct = None;
        assert_missing_field_with_ids(&event, Some("event-1"), Some("VH-12345678"));

        let mut event = valid_event("done");
        event.event_ts = None;
        assert_missing_field_with_ids(&event, Some("event-1"), Some("VH-12345678"));
    }

    fn assert_missing_field_with_ids(
        event: &OtaEvent,
        event_id: Option<&str>,
        vehicle_id: Option<&str>,
    ) {
        let error = process(event).expect_err("missing required field");

        assert_eq!(error.reason, DlqReason::MissingField);
        assert_eq!(error.event_id.as_deref(), event_id);
        assert_eq!(error.vehicle_id.as_deref(), vehicle_id);
    }

    #[test]
    fn missing_required_field_wins_over_unknown_status() {
        let mut event = valid_event("not-known");
        event.event_id = None;

        assert_eq!(
            process(&event).expect_err("missing field").reason,
            DlqReason::MissingField
        );
    }

    #[test]
    fn missing_schema_version_emits_version_one() {
        let mut event = valid_event("complete");
        event.schema_version = None;

        assert_eq!(
            process(&event)
                .expect("schema version is optional")
                .schema_version,
            "1"
        );
    }
}
