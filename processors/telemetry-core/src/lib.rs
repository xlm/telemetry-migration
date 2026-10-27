pub mod adapters;
pub mod cli;
pub mod lenient;
pub mod logging;

#[cfg(feature = "kafka")]
pub mod kafka;

/// A source of records for a processor.
pub trait Source<T> {
    type Error;

    /// Reads the next record, returning `Ok(None)` when the input is exhausted.
    fn read_next(&mut self) -> Result<Option<T>, Self::Error>;
}

/// A destination for processor records.
pub trait Sink<T> {
    type Error;

    /// Writes one record to the destination.
    fn write_record(&mut self, record: &T) -> Result<(), Self::Error>;
}
