use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::types::DlqReason;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorName {
    ChargingSession,
    OtaStatus,
}

impl ProcessorName {
    fn as_str(self) -> &'static str {
        match self {
            Self::ChargingSession => "charging_session",
            Self::OtaStatus => "ota_status",
        }
    }
}

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

impl DlqReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::MissingField => "MISSING_FIELD",
            Self::UnknownState => "UNKNOWN_STATE",
            Self::SocOutOfRange => "SOC_OUT_OF_RANGE",
            Self::NegativePower => "NEGATIVE_POWER",
            Self::Duplicate => "DUPLICATE",
            Self::Late => "LATE",
            Self::UnknownStatus => "UNKNOWN_STATUS",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogRecord {
    processor: ProcessorName,
    event: LogEvent,
    vehicle_id_hash: Option<String>,
    reason: Option<DlqReason>,
}

impl LogRecord {
    pub fn new(
        processor: ProcessorName,
        event: LogEvent,
        vehicle_id: Option<&str>,
        reason: Option<DlqReason>,
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

pub fn init_json_logging() {
    tracing_subscriber::fmt().json().init();
}

pub fn log_record(record: &LogRecord) {
    info!(
        processor = record.processor.as_str(),
        event = record.event.as_str(),
        vehicle_id_hash = record.vehicle_id_hash.as_deref().unwrap_or(""),
        reason = record.reason.map(DlqReason::as_str).unwrap_or(""),
    );
}

#[cfg(test)]
mod tests {
    use crate::logging::{hash_vehicle_id, log_record, LogEvent, LogRecord, ProcessorName};
    use crate::types::DlqReason;
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
            ProcessorName::ChargingSession,
            LogEvent::EventRejected,
            Some(raw_vehicle_id),
            Some(DlqReason::Late),
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
            ProcessorName::ChargingSession,
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
}
