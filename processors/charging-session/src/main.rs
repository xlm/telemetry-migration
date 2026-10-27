pub mod core;
pub mod types;

use std::error::Error;
use std::process;

use clap::Parser;
use telemetry_core::adapters::jsonl::{JsonlError, JsonlSink, JsonlSource};
use telemetry_core::cli::FileArgs;
use telemetry_core::logging::{
    init_json_logging_to_file, log_record, log_run_summary, LogEvent, LogRecord,
};
use telemetry_core::{Sink, Source};

use crate::types::{ChargingEvent, Emitted};

fn main() {
    if let Err(error) = run() {
        eprintln!("charging-session: {error}");
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
    let mut processor = core::Processor::new();

    loop {
        let event = match source.read_next() {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(JsonlError::Json(_)) => ChargingEvent::default(),
            Err(JsonlError::Io(error)) => return Err(error.into()),
        };
        read += 1;

        for record in processor.on_event(event) {
            match record {
                Emitted::Summary(record) => {
                    output.write_record(&record)?;
                    log_record(&LogRecord::new(
                        "charging-session",
                        LogEvent::SummaryEmitted,
                        Some(&record.vehicle_id),
                        None,
                    ));
                    emitted += 1;
                }
                Emitted::Dlq(record) => {
                    log_record(&LogRecord::new(
                        "charging-session",
                        LogEvent::EventRejected,
                        record.vehicle_id.as_deref(),
                        Some(record.reason.as_str()),
                    ));
                    dlq.write_record(&record)?;
                    rejected += 1;
                }
            }
        }
    }

    output.finish()?;
    dlq.finish()?;
    log_run_summary("charging-session", read, emitted, rejected);
    Ok(())
}
