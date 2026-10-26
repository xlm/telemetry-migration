use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{Sink, Source};

#[derive(Debug)]
pub enum JsonlError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl Display for JsonlError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "JSONL I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSONL record error: {error}"),
        }
    }
}

impl Error for JsonlError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for JsonlError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for JsonlError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub struct JsonlSource<R> {
    reader: R,
    line: String,
}

impl<R> JsonlSource<R> {
    pub fn from_reader(reader: R) -> Self {
        Self {
            reader,
            line: String::new(),
        }
    }
}

impl JsonlSource<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JsonlError> {
        Ok(Self::from_reader(BufReader::new(File::open(path)?)))
    }
}

impl<R, T> Source<T> for JsonlSource<R>
where
    R: BufRead,
    T: DeserializeOwned,
{
    type Error = JsonlError;

    fn read_next(&mut self) -> Result<Option<T>, Self::Error> {
        self.line.clear();
        if self.reader.read_line(&mut self.line)? == 0 {
            return Ok(None);
        }

        let line = self.line.trim_end_matches(['\r', '\n']);
        Ok(Some(serde_json::from_str(line)?))
    }
}

pub struct JsonlSink<W> {
    writer: W,
}

impl<W> JsonlSink<W> {
    pub fn from_writer(writer: W) -> Self {
        Self { writer }
    }
}

impl JsonlSink<BufWriter<File>> {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, JsonlError> {
        Ok(Self::from_writer(BufWriter::new(File::create(path)?)))
    }
}

impl<W: Write> JsonlSink<W> {
    pub fn flush(&mut self) -> Result<(), JsonlError> {
        self.writer.flush().map_err(JsonlError::from)
    }
}

impl<W, T> Sink<T> for JsonlSink<W>
where
    W: Write,
    T: Serialize,
{
    type Error = JsonlError;

    fn write_record(&mut self, record: &T) -> Result<(), Self::Error> {
        serde_json::to_writer(&mut self.writer, record)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, BufReader, Cursor, Read};

    use crate::types::OtaEvent;
    use crate::{Sink, Source};

    use super::{JsonlError, JsonlSink, JsonlSource};

    const OTA_EVENT: &str = r#"{"event_id":"event-1","vehicle_id":"VH-12345678","campaign_id":"OTA-1","status":"downloading","progress_pct":25.0,"event_ts":1700000000000,"schema_version":"1"}"#;

    #[test]
    fn reads_jsonl_records_in_order() {
        let input = format!("{OTA_EVENT}\n{OTA_EVENT}\n");
        let mut source = JsonlSource::from_reader(BufReader::new(Cursor::new(input)));
        let first: Option<OtaEvent> = source.read_next().expect("first record");
        let second: Option<OtaEvent> = source.read_next().expect("second record");
        let end: Option<OtaEvent> = source.read_next().expect("end of input");

        assert_eq!(
            first.as_ref().map(|event| event.event_id.as_str()),
            Some("event-1")
        );
        assert_eq!(
            second.as_ref().map(|event| event.event_id.as_str()),
            Some("event-1")
        );
        assert!(end.is_none());
    }

    #[test]
    fn propagates_malformed_jsonl_records() {
        let mut source = JsonlSource::from_reader(Cursor::new("{invalid}\n"));
        let result: Result<Option<OtaEvent>, JsonlError> = source.read_next();

        assert!(matches!(result, Err(JsonlError::Json(_))));
    }

    #[test]
    fn propagates_jsonl_io_errors() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("injected read failure"))
            }
        }

        let reader = BufReader::new(FailingReader);
        let mut source = JsonlSource::from_reader(reader);
        let result: Result<Option<OtaEvent>, JsonlError> = source.read_next();

        assert!(matches!(result, Err(JsonlError::Io(_))));
    }

    #[test]
    fn writes_jsonl_records_with_a_trailing_newline() {
        let event: OtaEvent = serde_json::from_str(OTA_EVENT).expect("valid event");
        let mut sink = JsonlSink::from_writer(Vec::new());

        sink.write_record(&event).expect("write record");
        let output = sink.writer;

        assert_eq!(output.last(), Some(&b'\n'));
        let decoded: OtaEvent =
            serde_json::from_slice(&output[..output.len() - 1]).expect("written record");
        assert_eq!(decoded, event);
    }

    #[test]
    fn propagates_jsonl_write_errors() {
        struct FailingWriter;

        impl std::io::Write for FailingWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("injected write failure"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let event: OtaEvent = serde_json::from_str(OTA_EVENT).expect("valid event");
        let mut sink = JsonlSink::from_writer(FailingWriter);
        let result = sink.write_record(&event);

        assert!(matches!(
            result,
            Err(JsonlError::Json(error)) if error.is_io()
        ));
    }
}
