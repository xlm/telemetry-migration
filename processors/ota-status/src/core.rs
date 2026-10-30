use crate::types::{DlqReason, DlqRecord, OtaEvent, OtaStatus, OtaStatusRecord};

pub fn process(event: &OtaEvent) -> Result<OtaStatusRecord, DlqRecord> {
    if event.progress_pct.is_some_and(|value| !value.is_finite()) {
        return Err(DlqRecord {
            event_id: None,
            vehicle_id: None,
            reason: DlqReason::MissingField,
        });
    }

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
        Some(progress),
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
        progress_pct: if progress <= 0.0 {
            0.0
        } else {
            progress.min(100.0)
        },
        event_ts,
        schema_version: "1".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> OtaEvent {
        OtaEvent {
            event_id: Some("synthetic-event".to_owned()),
            vehicle_id: Some("synthetic-vehicle".to_owned()),
            campaign_id: Some("synthetic-campaign".to_owned()),
            status: Some("dl".to_owned()),
            progress_pct: Some(25.125),
            event_ts: Some(10),
            schema_version: Some("old-version".to_owned()),
        }
    }

    #[test]
    fn projects_fields_and_always_emits_version_one_without_mutating_input() {
        let event = event();
        let original = event.clone();
        assert_eq!(
            process(&event),
            Ok(OtaStatusRecord {
                event_id: "synthetic-event".to_owned(),
                vehicle_id: "synthetic-vehicle".to_owned(),
                campaign_id: "synthetic-campaign".to_owned(),
                status: OtaStatus::Downloading,
                progress_pct: 25.125,
                event_ts: 10,
                schema_version: "1".to_owned(),
            })
        );
        assert_eq!(event, original);
    }

    #[test]
    fn normalizes_every_alias_without_case_sensitivity() {
        for (alias, expected) in [
            ("downloading", OtaStatus::Downloading),
            ("dl", OtaStatus::Downloading),
            ("download", OtaStatus::Downloading),
            ("installing", OtaStatus::Installing),
            ("inst", OtaStatus::Installing),
            ("done", OtaStatus::Succeeded),
            ("complete", OtaStatus::Succeeded),
            ("success", OtaStatus::Succeeded),
            ("succeeded", OtaStatus::Succeeded),
            ("failed", OtaStatus::Failed),
            ("error", OtaStatus::Failed),
        ] {
            let mixed: String = alias
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    if index % 2 == 0 {
                        ch.to_ascii_uppercase()
                    } else {
                        ch
                    }
                })
                .collect();
            for status in [alias.to_owned(), alias.to_uppercase(), mixed] {
                let event = OtaEvent {
                    status: Some(status),
                    ..event()
                };
                assert_eq!(process(&event).unwrap().status, expected);
            }
        }
    }

    #[test]
    fn rejects_unknown_status_without_trimming_or_rewriting() {
        for status in [
            "",
            " dl",
            "dl ",
            "dl\n",
            "\"dl\"",
            "d-l",
            "d_l",
            "d.l",
            "ｄｌ",
            "d\u{301}l",
            "İNSTALLING",
            "succeſs",
            "queued",
        ] {
            let event = OtaEvent {
                status: Some(status.to_owned()),
                ..event()
            };
            assert_eq!(
                process(&event),
                Err(DlqRecord {
                    event_id: event.event_id.clone(),
                    vehicle_id: event.vehicle_id.clone(),
                    reason: DlqReason::UnknownStatus,
                }),
                "status {status:?}"
            );
        }
    }

    #[test]
    fn missing_required_fields_take_precedence_and_preserve_available_ids() {
        let valid = serde_json::json!({
            "event_id": "synthetic-event", "vehicle_id": "synthetic-vehicle",
            "campaign_id": "synthetic-campaign", "status": "unknown",
            "progress_pct": 25, "event_ts": 10
        });
        for field in [
            "event_id",
            "vehicle_id",
            "campaign_id",
            "status",
            "progress_pct",
            "event_ts",
        ] {
            for wrong in [
                serde_json::Value::Null,
                serde_json::json!([]),
                serde_json::json!({}),
            ] {
                let mut input = valid.clone();
                input[field] = wrong;
                let event: OtaEvent = serde_json::from_value(input).unwrap();
                assert_eq!(
                    process(&event),
                    Err(DlqRecord {
                        event_id: event.event_id.clone(),
                        vehicle_id: event.vehicle_id.clone(),
                        reason: DlqReason::MissingField,
                    }),
                    "field {field}"
                );
            }
            let mut input = valid.clone();
            input.as_object_mut().unwrap().remove(field);
            let event: OtaEvent = serde_json::from_value(input).unwrap();
            assert_eq!(process(&event).unwrap_err().reason, DlqReason::MissingField);
        }
        assert_eq!(
            process(&OtaEvent::default()),
            Err(DlqRecord {
                event_id: None,
                vehicle_id: None,
                reason: DlqReason::MissingField,
            })
        );
    }

    #[test]
    fn accepts_empty_identifiers_and_ignores_input_version() {
        for version in [None, Some(String::new()), Some("future-version".to_owned())] {
            let event = OtaEvent {
                event_id: Some(String::new()),
                vehicle_id: Some(" ".to_owned()),
                campaign_id: Some(String::new()),
                schema_version: version,
                ..event()
            };
            let record = process(&event).unwrap();
            assert_eq!(record.event_id, "");
            assert_eq!(record.vehicle_id, " ");
            assert_eq!(record.campaign_id, "");
            assert_eq!(record.schema_version, "1");
        }
    }

    #[test]
    fn clamps_progress_at_both_bounds_without_rounding_or_status_override() {
        for (progress, expected) in [
            (-f64::MAX, 0.0),
            (-0.01, 0.0),
            (-0.0, 0.0),
            (0.0, 0.0),
            (0.01, 0.01),
            (25.125, 25.125),
            (99.99, 99.99),
            (100.0, 100.0),
            (100.01, 100.0),
            (f64::MAX, 100.0),
        ] {
            for status in ["dl", "done", "failed"] {
                let event = OtaEvent {
                    progress_pct: Some(progress),
                    status: Some(status.to_owned()),
                    ..event()
                };
                let actual = process(&event).unwrap().progress_pct;
                assert_eq!(actual, expected);
                assert!(!actual.is_sign_negative());
            }
        }
    }

    #[test]
    fn nonfinite_progress_rejects_the_entire_record() {
        for progress in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let event = OtaEvent {
                progress_pct: Some(progress),
                ..event()
            };
            assert_eq!(
                process(&event),
                Err(DlqRecord {
                    event_id: None,
                    vehicle_id: None,
                    reason: DlqReason::MissingField
                })
            );
        }
    }

    #[test]
    fn timestamps_and_duplicates_do_not_introduce_state_or_timeouts() {
        for timestamp in [i64::MAX, 10, 10, 0, -1, i64::MIN] {
            let event = OtaEvent {
                event_ts: Some(timestamp),
                ..event()
            };
            let first = process(&event).unwrap();
            assert_eq!(first.event_ts, timestamp);
            assert_eq!(process(&event), Ok(first));
        }
    }
}
