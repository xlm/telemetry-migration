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
        loop {
            self.line.clear();
            if self.reader.read_line(&mut self.line)? == 0 {
                return Ok(None);
            }

            let line = self.line.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() {
                continue;
            }

            return serde_json::from_str(line)
                .map(Some)
                .map_err(JsonlError::Json);
        }
    }
}

/// Callers must call `finish` (or `flush`) before reporting success because
/// dropping the sink cannot report write errors.
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

    pub fn finish(mut self) -> Result<W, JsonlError> {
        self.flush()?;
        Ok(self.writer)
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

    use serde::{Deserialize, Serialize};

    use crate::{Sink, Source};

    use super::{JsonlError, JsonlSink, JsonlSource};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct JsonlTestRecord {
        value: String,
    }

    const JSONL_RECORD: &str = r#"{"value":"record-1"}"#;

    #[test]
    fn reads_jsonl_records_in_order() {
        let input = format!("{JSONL_RECORD}\n{JSONL_RECORD}\n");
        let mut source = JsonlSource::from_reader(BufReader::new(Cursor::new(input)));
        let first: Option<JsonlTestRecord> = source.read_next().expect("first record");
        let second: Option<JsonlTestRecord> = source.read_next().expect("second record");
        let end: Option<JsonlTestRecord> = source.read_next().expect("end of input");

        assert_eq!(
            first.as_ref().map(|record| record.value.as_str()),
            Some("record-1")
        );
        assert_eq!(
            second.as_ref().map(|record| record.value.as_str()),
            Some("record-1")
        );
        assert!(end.is_none());
    }

    #[test]
    fn propagates_malformed_jsonl_records() {
        let mut source = JsonlSource::from_reader(Cursor::new("{invalid}\n"));
        let result: Result<Option<JsonlTestRecord>, JsonlError> = source.read_next();

        assert!(matches!(result, Err(JsonlError::Json(_))));
    }

    #[test]
    fn reads_next_record_after_a_malformed_line() {
        let input = format!("{{invalid}}\n{JSONL_RECORD}\n");
        let mut source = JsonlSource::from_reader(Cursor::new(input));
        let malformed: Result<Option<JsonlTestRecord>, JsonlError> = source.read_next();
        let next: Option<JsonlTestRecord> = source.read_next().expect("next record");

        assert!(matches!(malformed, Err(JsonlError::Json(_))));
        assert_eq!(
            next,
            Some(JsonlTestRecord {
                value: "record-1".to_owned()
            })
        );
    }

    #[test]
    fn skips_blank_lines() {
        let input = format!("\n  \r\n{JSONL_RECORD}\n");
        let mut source = JsonlSource::from_reader(Cursor::new(input));
        let record: Option<JsonlTestRecord> = source.read_next().expect("record after blanks");

        assert_eq!(
            record,
            Some(JsonlTestRecord {
                value: "record-1".to_owned()
            })
        );
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
        let result: Result<Option<JsonlTestRecord>, JsonlError> = source.read_next();

        assert!(matches!(result, Err(JsonlError::Io(_))));
    }

    #[test]
    fn writes_jsonl_records_with_a_trailing_newline() {
        let event: JsonlTestRecord =
            serde_json::from_str(JSONL_RECORD).expect("valid JSONL record");
        let mut sink = JsonlSink::from_writer(Vec::new());

        sink.write_record(&event).expect("write record");
        let output = sink.writer;

        assert_eq!(output.last(), Some(&b'\n'));
        let decoded: JsonlTestRecord =
            serde_json::from_slice(&output[..output.len() - 1]).expect("written record");
        assert_eq!(decoded, event);
    }

    #[test]
    fn finish_returns_the_writer_with_written_records() {
        let event: JsonlTestRecord =
            serde_json::from_str(JSONL_RECORD).expect("valid JSONL record");
        let mut sink = JsonlSink::from_writer(Vec::new());
        sink.write_record(&event).expect("write record");

        let output = sink.finish().expect("finish sink");

        assert_eq!(output.last(), Some(&b'\n'));
        let decoded: JsonlTestRecord =
            serde_json::from_slice(&output[..output.len() - 1]).expect("written record");
        assert_eq!(decoded, event);
    }

    #[test]
    fn finish_propagates_flush_errors() {
        struct FailingFlushWriter;

        impl std::io::Write for FailingFlushWriter {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                Ok(buffer.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("injected flush failure"))
            }
        }

        let event: JsonlTestRecord =
            serde_json::from_str(JSONL_RECORD).expect("valid JSONL record");
        let mut sink = JsonlSink::from_writer(FailingFlushWriter);
        sink.write_record(&event).expect("write record");

        let result = sink.finish();

        assert!(matches!(result, Err(JsonlError::Io(_))));
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

        let event: JsonlTestRecord =
            serde_json::from_str(JSONL_RECORD).expect("valid JSONL record");
        let mut sink = JsonlSink::from_writer(FailingWriter);
        let result = sink.write_record(&event);

        assert!(matches!(
            result,
            Err(JsonlError::Json(error)) if error.is_io()
        ));
    }
}
