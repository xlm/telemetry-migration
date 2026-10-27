use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
use rdkafka::Message;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{Sink, Source};

#[derive(Debug)]
pub enum KafkaError {
    Client(rdkafka::error::KafkaError),
    Json(serde_json::Error),
}

impl Display for KafkaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "Kafka error: {error}"),
            Self::Json(error) => write!(formatter, "Kafka JSON error: {error}"),
        }
    }
}

impl Error for KafkaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Client(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<rdkafka::error::KafkaError> for KafkaError {
    fn from(error: rdkafka::error::KafkaError) -> Self {
        Self::Client(error)
    }
}

impl From<serde_json::Error> for KafkaError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn consumer_config(brokers: &str, group_id: &str) -> ClientConfig {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", brokers)
        .set("group.id", group_id)
        .set("auto.offset.reset", "earliest");
    config
}

pub fn producer_config(brokers: &str) -> ClientConfig {
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", brokers)
        .set("enable.idempotence", "true")
        .set("acks", "all");
    config
}

pub struct KafkaSource<T> {
    consumer: BaseConsumer,
    idle_timeout: Duration,
    marker: PhantomData<fn() -> T>,
}

impl<T> KafkaSource<T> {
    pub fn new(
        brokers: &str,
        group_id: &str,
        topic: &str,
        idle_timeout: Duration,
    ) -> Result<Self, KafkaError> {
        let consumer: BaseConsumer = consumer_config(brokers, group_id).create()?;
        consumer.subscribe(&[topic])?;

        Ok(Self {
            consumer,
            idle_timeout,
            marker: PhantomData,
        })
    }
}

impl<T> Source<T> for KafkaSource<T>
where
    T: DeserializeOwned,
{
    type Error = KafkaError;

    fn read_next(&mut self) -> Result<Option<T>, Self::Error> {
        let deadline = Instant::now() + self.idle_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }

            match self.consumer.poll(remaining) {
                Some(Ok(message)) => {
                    return serde_json::from_slice(message.payload().unwrap_or_default())
                        .map(Some)
                        .map_err(KafkaError::Json);
                }
                Some(Err(error)) => return Err(KafkaError::Client(error)),
                None if Instant::now() >= deadline => return Ok(None),
                None => {}
            }
        }
    }
}

pub struct KafkaSink<T> {
    producer: BaseProducer,
    topic: String,
    key: fn(&T) -> Option<String>,
    marker: PhantomData<fn(&T)>,
}

impl<T> KafkaSink<T> {
    pub fn new(
        brokers: &str,
        topic: impl Into<String>,
        key: fn(&T) -> Option<String>,
    ) -> Result<Self, KafkaError> {
        let producer: BaseProducer = producer_config(brokers).create()?;
        Ok(Self {
            producer,
            topic: topic.into(),
            key,
            marker: PhantomData,
        })
    }

    pub fn flush(&self, timeout: Duration) -> Result<(), KafkaError> {
        self.producer.flush(timeout).map_err(KafkaError::Client)
    }

    pub fn finish(self, timeout: Duration) -> Result<(), KafkaError> {
        self.flush(timeout)
    }
}

impl<T> Sink<T> for KafkaSink<T>
where
    T: Serialize,
{
    type Error = KafkaError;

    fn write_record(&mut self, record: &T) -> Result<(), Self::Error> {
        let payload = serde_json::to_vec(record)?;
        let key = (self.key)(record);
        match key.as_deref() {
            Some(key) => self
                .producer
                .send(
                    BaseRecord::to(&self.topic)
                        .payload(payload.as_slice())
                        .key(key),
                )
                .map_err(|(error, _)| KafkaError::Client(error)),
            None => self
                .producer
                .send(BaseRecord::<(), [u8]>::to(&self.topic).payload(payload.as_slice()))
                .map_err(|(error, _)| KafkaError::Client(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{consumer_config, producer_config};

    #[test]
    fn consumer_config_uses_earliest_offsets() {
        let config = consumer_config("localhost:9092", "test-group");

        assert_eq!(config.get("bootstrap.servers"), Some("localhost:9092"));
        assert_eq!(config.get("group.id"), Some("test-group"));
        assert_eq!(config.get("auto.offset.reset"), Some("earliest"));
    }

    #[test]
    fn producer_config_enables_idempotence_and_all_acks() {
        let config = producer_config("localhost:9092");

        assert_eq!(config.get("enable.idempotence"), Some("true"));
        assert_eq!(config.get("acks"), Some("all"));
    }
}
