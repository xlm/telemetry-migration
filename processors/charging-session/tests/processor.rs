use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use telemetry_core::logging::hash_vehicle_id;

struct Run(PathBuf);

impl Run {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("charging-session-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn execute(&self, input: &Path) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
        let output = self.0.join("sessions");
        let dlq = self.0.join("dlq");
        let log = self.0.join("log");
        let result = Command::new(env!("CARGO_BIN_EXE_charging-session"))
            .arg("--input")
            .arg(input)
            .arg("--output")
            .arg(&output)
            .arg("--dlq")
            .arg(&dlq)
            .arg("--log")
            .arg(&log)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
        (records(&output), records(&dlq), records(&log))
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn canonical(mut records: Vec<Value>) -> Vec<Value> {
    records.sort_by_key(Value::to_string);
    records
}

#[test]
fn all_fixtures_match_goldens_and_logs_never_include_raw_identifiers_or_location() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let run = Run::new();
    let mut count = 0;
    for fixture in fs::read_dir(root.join("fixtures/charging-session")).unwrap() {
        let path = fixture.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        count += 1;
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let (sessions, dlq, logs) = run.execute(&path);
        for (actual, suffix) in [(sessions, "sessions"), (dlq, "dlq")] {
            let golden = root.join(format!("goldens/charging-session/{stem}.{suffix}.jsonl"));
            assert_eq!(
                canonical(actual),
                canonical(records(&golden)),
                "{stem}.{suffix}"
            );
        }
        let raw = fs::read_to_string(&path).unwrap();
        let log_text = logs.iter().map(Value::to_string).collect::<String>();
        for event in raw
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            if let Some(id) = event["vehicle_id"].as_str() {
                assert!(!log_text.contains(id), "raw vehicle ID in {stem}");
            }
        }
        for record in logs {
            for key in record["fields"].as_object().unwrap().keys() {
                assert!([
                    "processor",
                    "event",
                    "vehicle_id_hash",
                    "reason",
                    "read",
                    "emitted",
                    "dlq"
                ]
                .contains(&key.as_str()));
            }
        }
        assert!(!log_text.contains("location"));
    }
    assert_eq!(count, 11);
}

#[test]
fn file_adapter_sorts_batch_recovers_from_corruption_and_logs_only_hashes_and_counts() {
    let run = Run::new();
    let input = run.0.join("input.jsonl");
    let mut start = serde_json::json!({
        "event_id":"start", "vehicle_id":"VH-private", "session_id":"s",
        "event_ts":0, "ingest_ts":1, "seq":1, "charging_state":"CHARGING",
        "power_kw":74.07, "soc_pct":20, "location":{"lat":12.3456,"lon":23.4567},
    });
    let mut end = start.clone();
    end["event_id"] = "end".into();
    end["charging_state"] = "COMPLETE".into();
    end["event_ts"] = 60_000.into();
    end["ingest_ts"] = 2.into();
    end["seq"] = 2.into();
    start["soc_pct"] = "not-a-number".into();
    fs::write(&input, format!("{end}\n\n{{\n[]\n{start}\n")).unwrap();
    let (sessions, dlq, logs) = run.execute(&input);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["event_count"], 2);
    assert_eq!(sessions[0]["energy_kwh"], 1.234);
    assert_eq!(sessions[0]["start_soc_pct"], 20.0);
    assert_eq!(
        dlq,
        [
            serde_json::json!({"event_id":null,"vehicle_id":null,"reason":"MISSING_FIELD"}),
            serde_json::json!({"event_id":null,"vehicle_id":null,"reason":"MISSING_FIELD"}),
        ]
    );
    let summary = logs
        .iter()
        .find(|log| log["fields"]["event"] == "summary_emitted")
        .unwrap();
    assert_eq!(
        summary["fields"]["vehicle_id_hash"],
        hash_vehicle_id("VH-private")
    );
    let counts = &logs.last().unwrap()["fields"];
    assert_eq!(counts["read"], 4);
    assert_eq!(counts["emitted"], 1);
    assert_eq!(counts["dlq"], 2);
    let log_text = logs.iter().map(Value::to_string).collect::<String>();
    for private in [
        "VH-private",
        "location",
        "12.3456",
        "23.4567",
        "start",
        "not-a-number",
    ] {
        assert!(!log_text.contains(private), "{private}");
    }
}
