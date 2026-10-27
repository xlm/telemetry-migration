// Throwaway: proves the Gate check goes green for a Processor-only change.
mod core;
mod types;

use std::error::Error;
use std::process;

use clap::Parser;
use telemetry_core::adapters::jsonl::{JsonlError, JsonlSink, JsonlSource};
use telemetry_core::cli::FileArgs;
use telemetry_core::logging::{
    init_json_logging_to_file, log_record, log_run_summary, LogEvent, LogRecord,
};
use telemetry_core::{Sink, Source};

use crate::types::OtaEvent;

fn main() {
    if let Err(error) = run() {
        eprintln!("ota-status: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = FileArgs::try_parse()?;
    init_json_logging_to_file(&args.log)?;
    let mut source = JsonlSource::<std::io::BufReader<std::fs::File>>::open(&args.input)?;
    let mut output = JsonlSink::<std::io::BufWriter<std::fs::File>>::create(&args.output)?;
    let mut dlq = JsonlSink::<std::io::BufWriter<std::fs::File>>::create(&args.dlq)?;
    let mut read = 0;
    let mut emitted = 0;
    let mut rejected = 0;

    loop {
        let event = match source.read_next() {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(JsonlError::Json(_)) => OtaEvent::default(),
            Err(JsonlError::Io(error)) => return Err(error.into()),
        };
        read += 1;

        match core::process(&event) {
            Ok(record) => {
                output.write_record(&record)?;
                log_record(&LogRecord::new(
                    "ota-status",
                    LogEvent::EventAccepted,
                    Some(&record.vehicle_id),
                    None,
                ));
                log_record(&LogRecord::new(
                    "ota-status",
                    LogEvent::StatusEmitted,
                    Some(&record.vehicle_id),
                    None,
                ));
                emitted += 1;
            }
            Err(record) => {
                log_record(&LogRecord::new(
                    "ota-status",
                    LogEvent::EventRejected,
                    record.vehicle_id.as_deref(),
                    Some(record.reason.as_str()),
                ));
                dlq.write_record(&record)?;
                rejected += 1;
            }
        }
    }

    output.finish()?;
    dlq.finish()?;
    log_run_summary("ota-status", read, emitted, rejected);
    Ok(())
}
