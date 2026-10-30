use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ota-status")
}

#[test]
fn processes_all_ota_fixtures_with_private_logs() {
    let tempdir = tempfile::tempdir().expect("temporary directory");
    let mut input_count = 0;
    let mut output_count = 0;
    let mut dlq_lines = Vec::new();
    let mut malformed_reasons = Vec::new();

    for fixture in [
        "01-normal.jsonl",
        "02-variant-status-strings.jsonl",
        "03-malformed.jsonl",
    ] {
        let input_path = fixture_dir().join(fixture);
        let input = fs::read_to_string(&input_path).expect("read fixture");
        input_count += input.lines().filter(|line| !line.trim().is_empty()).count();

        let output_path = tempdir.path().join(format!("{fixture}.output"));
        let dlq_path = tempdir.path().join(format!("{fixture}.dlq"));
        let log_path = tempdir.path().join(format!("{fixture}.log"));
        let result = Command::new(env!("CARGO_BIN_EXE_ota-status"))
            .arg("--input")
            .arg(&input_path)
            .arg("--output")
            .arg(&output_path)
            .arg("--dlq")
            .arg(&dlq_path)
            .arg("--log")
            .arg(&log_path)
            .output()
            .expect("run ota-status binary");

        assert!(
            result.status.success(),
            "binary failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );

        let output = fs::read_to_string(output_path).expect("read output");
        let dlq = fs::read_to_string(dlq_path).expect("read DLQ");
        output_count += output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        let current_dlq = dlq
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if fixture == "03-malformed.jsonl" {
            malformed_reasons.extend(current_dlq.iter().map(|line| {
                let record: Value = serde_json::from_str(line).expect("DLQ JSON");
                record["reason"].as_str().expect("DLQ reason").to_owned()
            }));
        }
        dlq_lines.extend(current_dlq);

        for line in output.lines().filter(|line| !line.trim().is_empty()) {
            let record: Value = serde_json::from_str(line).expect("output JSON");
            assert!(matches!(
                record["status"].as_str(),
                Some("DOWNLOADING" | "INSTALLING" | "SUCCEEDED" | "FAILED")
            ));
            let progress = record["progress_pct"].as_f64().expect("numeric progress");
            assert!((0.0..=100.0).contains(&progress));
        }

        let log = fs::read_to_string(log_path).expect("read JSON log");
        assert!(!log.contains("VH-"));
    }

    assert_eq!(output_count + dlq_lines.len(), input_count);

    assert_eq!(
        malformed_reasons,
        vec![
            "UNKNOWN_STATUS".to_owned(),
            "MISSING_FIELD".to_owned(),
            "MISSING_FIELD".to_owned(),
            "MISSING_FIELD".to_owned()
        ]
    );
}

fn run_jsonl(input: &str) -> (Vec<Value>, Vec<Value>, String) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input_path = directory.path().join("input.jsonl");
    let output_path = directory.path().join("output.jsonl");
    let dlq_path = directory.path().join("dlq.jsonl");
    let log_path = directory.path().join("log.jsonl");
    fs::write(&input_path, input).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ota-status"))
        .arg("--input")
        .arg(&input_path)
        .arg("--output")
        .arg(&output_path)
        .arg("--dlq")
        .arg(&dlq_path)
        .arg("--log")
        .arg(&log_path)
        .output()
        .expect("run binary");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let records = |path| {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    };
    (
        records(output_path),
        records(dlq_path),
        fs::read_to_string(log_path).unwrap(),
    )
}

fn valid_input() -> Value {
    serde_json::json!({
        "event_id": "synthetic-event", "vehicle_id": "synthetic-private-vehicle",
        "campaign_id": "synthetic-campaign", "status": "dl",
        "progress_pct": 25.125, "event_ts": 10, "schema_version": "future"
    })
}

#[test]
fn rejects_wrong_types_without_coercion_and_retains_only_text_identifiers() {
    let mut lines = Vec::new();
    let mut expected = Vec::new();
    for field in [
        "event_id",
        "vehicle_id",
        "campaign_id",
        "status",
        "progress_pct",
        "event_ts",
    ] {
        let wrong_values = match field {
            "progress_pct" => vec![serde_json::json!("25"), serde_json::json!(true)],
            "event_ts" => vec![
                serde_json::json!("10"),
                serde_json::json!(10.0),
                serde_json::json!(1.5),
                serde_json::json!(9223372036854775808_u64),
                serde_json::json!(true),
            ],
            _ => vec![serde_json::json!(123), serde_json::json!(true)],
        };
        for wrong in wrong_values {
            let mut input = valid_input();
            input[field] = wrong;
            lines.push(input.to_string());
            expected.push(serde_json::json!({
                "event_id": if field == "event_id" { Value::Null } else { input["event_id"].clone() },
                "vehicle_id": if field == "vehicle_id" { Value::Null } else { input["vehicle_id"].clone() },
                "reason": "MISSING_FIELD"
            }));
        }
    }
    let (output, dlq, _) = run_jsonl(&lines.join("\n"));
    assert!(output.is_empty());
    assert_eq!(dlq, expected);
}

#[test]
fn malformed_and_nonfinite_declared_fields_reject_with_null_identifiers() {
    let mut lines = vec![
        "{".to_owned(),
        "[]".to_owned(),
        "null".to_owned(),
        "42".to_owned(),
    ];
    for field in [
        "event_id",
        "vehicle_id",
        "campaign_id",
        "status",
        "progress_pct",
        "event_ts",
        "schema_version",
    ] {
        for overflow in ["1e400", "-1e400", "{\"nested\":[1e400]}"] {
            let mut input = valid_input();
            input[field] = serde_json::json!("placeholder");
            lines.push(input.to_string().replace("\"placeholder\"", overflow));
        }
    }
    let (output, dlq, _) = run_jsonl(&lines.join("\n"));
    assert!(output.is_empty());
    assert_eq!(
        dlq,
        vec![
            serde_json::json!({"event_id": null, "vehicle_id": null, "reason": "MISSING_FIELD"});
            lines.len()
        ]
    );
}

#[test]
fn ignores_extra_fields_and_input_version_and_logs_only_hashes() {
    let mut lines = Vec::new();
    for version in [
        Value::Null,
        serde_json::json!(9),
        serde_json::json!({"future": true}),
    ] {
        let mut input = valid_input();
        input["schema_version"] = version;
        input["location"] = serde_json::json!("synthetic-private-location");
        input["extra"] = serde_json::json!("overflow-placeholder");
        lines.push(
            input
                .to_string()
                .replace("\"overflow-placeholder\"", "1e400"),
        );
    }
    let mut input = valid_input();
    input.as_object_mut().unwrap().remove("schema_version");
    lines.push(input.to_string());
    let (output, dlq, log) = run_jsonl(&lines.join("\n"));
    assert!(dlq.is_empty());
    assert_eq!(output.len(), lines.len());
    for record in output {
        assert_eq!(
            record,
            serde_json::json!({
                "event_id": "synthetic-event", "vehicle_id": "synthetic-private-vehicle",
                "campaign_id": "synthetic-campaign", "status": "DOWNLOADING",
                "progress_pct": 25.125, "event_ts": 10, "schema_version": "1"
            })
        );
    }
    for raw in [
        "synthetic-private-vehicle",
        "synthetic-private-location",
        "synthetic-event",
        "synthetic-campaign",
        "overflow-placeholder",
    ] {
        assert!(!log.contains(raw), "raw data logged: {raw}");
    }
    let hash = telemetry_core::logging::hash_vehicle_id("synthetic-private-vehicle");
    let logs: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for line in &logs[..logs.len() - 1] {
        assert_eq!(line["fields"]["vehicle_id_hash"], hash);
    }
    assert_eq!(logs.last().unwrap()["fields"]["read"], lines.len());
}

#[test]
fn keeps_duplicates_and_input_order_per_destination_while_skipping_blank_lines() {
    let mut lines = vec![String::new(), " \r".to_owned()];
    for timestamp in [30, 10, 10, -1] {
        let mut input = valid_input();
        input["event_ts"] = serde_json::json!(timestamp);
        lines.push(input.to_string());
    }
    for id in ["z", "a"] {
        let mut input = valid_input();
        input["event_id"] = serde_json::json!(id);
        input["status"] = serde_json::json!("unknown");
        lines.push(input.to_string());
    }
    let (output, dlq, _) = run_jsonl(&lines.join("\n"));
    assert_eq!(
        output
            .iter()
            .map(|record| record["event_ts"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![30, 10, 10, -1]
    );
    assert_eq!(
        dlq.iter()
            .map(|record| record["event_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["z", "a"]
    );
}
