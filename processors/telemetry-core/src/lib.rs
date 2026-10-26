pub mod adapters;
pub mod logging;
pub mod types;

#[cfg(feature = "kafka")]
pub mod kafka;

pub trait Source<T> {
    type Error;

    fn read_next(&mut self) -> Result<Option<T>, Self::Error>;
}

pub trait Sink<T> {
    type Error;

    fn write_record(&mut self, record: &T) -> Result<(), Self::Error>;
}
