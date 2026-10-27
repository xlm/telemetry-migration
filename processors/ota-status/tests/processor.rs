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
