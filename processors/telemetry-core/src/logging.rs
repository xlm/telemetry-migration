use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::{info, Level};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEvent {
    EventAccepted,
    EventRejected,
    SummaryEmitted,
    StatusEmitted,
}

impl LogEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::EventAccepted => "event_accepted",
            Self::EventRejected => "event_rejected",
            Self::SummaryEmitted => "summary_emitted",
            Self::StatusEmitted => "status_emitted",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogRecord {
    processor: &'static str,
    event: LogEvent,
    vehicle_id_hash: Option<String>,
    reason: Option<&'static str>,
}

impl LogRecord {
    pub fn new(
        processor: &'static str,
        event: LogEvent,
        vehicle_id: Option<&str>,
        reason: Option<&'static str>,
    ) -> Self {
        Self {
            processor,
            event,
            vehicle_id_hash: vehicle_id.map(hash_vehicle_id),
            reason,
        }
    }
}

pub fn hash_vehicle_id(vehicle_id: &str) -> String {
    let digest = Sha256::digest(vehicle_id.as_bytes());
    format!("{digest:x}")[..12].to_owned()
}

pub fn init_json_logging_to_file(path: impl AsRef<Path>) -> io::Result<()> {
    let file = File::create(path)?;
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| io::Error::other(error.to_string()))
}

pub fn log_record(record: &LogRecord) {
    info!(
        processor = record.processor,
        event = record.event.as_str(),
        vehicle_id_hash = record.vehicle_id_hash.as_deref().unwrap_or(""),
        reason = record.reason.unwrap_or(""),
    );
}

pub fn log_run_summary(processor: &'static str, read: u64, emitted: u64, dlq: u64) {
    info!(processor, event = "run_summary", read, emitted, dlq,);
}

#[cfg(test)]
mod tests {
    use crate::logging::{hash_vehicle_id, log_record, LogEvent, LogRecord};
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for MemoryWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MemoryWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn hashes_vehicle_ids_to_twelve_lowercase_hex_characters() {
        let first = hash_vehicle_id("VH-12345678");
        let second = hash_vehicle_id("VH-12345678");

        assert_eq!(first, second);
        assert_eq!(first, "1ecaa57f433a");
        assert_eq!(first.len(), 12);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }

    #[test]
    fn serialized_log_has_only_closed_fields_and_the_vehicle_hash() {
        let raw_vehicle_id = "VH-12345678";
        let record = LogRecord::new(
            "charging-session",
            LogEvent::EventRejected,
            Some(raw_vehicle_id),
            Some("LATE"),
        );
        let output = serde_json::to_string(&record).expect("serialize log record");

        assert!(output.contains(&hash_vehicle_id(raw_vehicle_id)));
        assert!(!output.contains(raw_vehicle_id));
        assert!(!output.contains("location"));
        assert!(!output.contains("payload"));
        assert!(!output.contains("message"));
    }

    #[test]
    fn emitted_json_log_contains_only_the_vehicle_hash_not_raw_id_or_location() {
        let raw_vehicle_id = "VH-12345678";
        let record = LogRecord::new(
            "charging-session",
            LogEvent::EventAccepted,
            Some(raw_vehicle_id),
            None,
        );
        let writer = MemoryWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_writer(writer.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || log_record(&record));

        let output = String::from_utf8(
            writer
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        )
        .expect("tracing output is UTF-8");
        let line: serde_json::Value =
            serde_json::from_str(output.trim()).expect("tracing emits one JSON line");

        assert_eq!(line["fields"]["vehicle_id_hash"], "1ecaa57f433a");
        assert!(!output.contains(raw_vehicle_id));
        assert!(!output.contains("location"));
    }

    #[test]
    fn summary_logs_counts_without_event_data() {
        let writer = MemoryWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_writer(writer.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            super::log_run_summary("ota-status", 4, 3, 1)
        });

        let output = String::from_utf8(
            writer
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        )
        .expect("tracing output is UTF-8");
        let line: serde_json::Value =
            serde_json::from_str(output.trim()).expect("tracing emits one JSON line");

        assert_eq!(line["fields"]["read"], 4);
        assert_eq!(line["fields"]["emitted"], 3);
        assert_eq!(line["fields"]["dlq"], 1);
        assert!(!output.contains("vehicle_id"));
    }
}
