use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rdkafka::client::ClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, CommitMode, Consumer};
use rdkafka::error::RDKafkaErrorCode;
use rdkafka::message::{DeliveryResult, Message, ToBytes};
use rdkafka::producer::{BaseProducer, BaseRecord, Producer, ProducerContext};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{Sink, Source};

#[derive(Default)]
pub struct DeliveryTracker {
    failures: AtomicUsize,
    first_error: Mutex<Option<String>>,
}

impl ClientContext for DeliveryTracker {}

impl ProducerContext for DeliveryTracker {
    type DeliveryOpaque = ();

    fn delivery(&self, result: &DeliveryResult<'_>, _: Self::DeliveryOpaque) {
        let Err((error, _)) = result else {
            return;
        };

        self.failures.fetch_add(1, Ordering::Relaxed);
        let mut first_error = self
            .first_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if first_error.is_none() {
            *first_error = Some(error.to_string());
        }
    }
}

#[derive(Debug)]
pub enum KafkaError {
    Client(rdkafka::error::KafkaError),
    Json(serde_json::Error),
    Delivery { failed: usize, first: String },
}

impl Display for KafkaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "Kafka error: {error}"),
            Self::Json(error) => write!(formatter, "Kafka JSON error: {error}"),
            Self::Delivery { failed, first } => {
                write!(
                    formatter,
                    "{failed} Kafka messages failed delivery; first error: {first}"
                )
            }
        }
    }
}

impl Error for KafkaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Client(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Delivery { .. } => None,
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
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false");
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

    /// Commits consumed offsets after all output and DLQ sinks finish successfully.
    ///
    /// Call only after every sink (output and DLQ) has finished successfully to
    /// preserve at-least-once delivery.
    pub fn commit(&self) -> Result<(), KafkaError> {
        match self.consumer.commit_consumer_state(CommitMode::Sync) {
            Ok(())
            | Err(rdkafka::error::KafkaError::ConsumerCommit(RDKafkaErrorCode::NoOffset)) => Ok(()),
            Err(error) => Err(KafkaError::Client(error)),
        }
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
    producer: BaseProducer<DeliveryTracker>,
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
        Self::from_config(producer_config(brokers), topic, key)
    }

    pub fn from_config(
        config: ClientConfig,
        topic: impl Into<String>,
        key: fn(&T) -> Option<String>,
    ) -> Result<Self, KafkaError> {
        let producer: BaseProducer<DeliveryTracker> =
            config.create_with_context(DeliveryTracker::default())?;
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
        self.flush(timeout)?;
        let context = self.producer.client().context();
        let failed = context.failures.load(Ordering::Relaxed);
        if failed > 0 {
            let first = context
                .first_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .unwrap_or_default();
            return Err(KafkaError::Delivery { failed, first });
        }
        Ok(())
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
            Some(key) => send_with_retry(
                &self.producer,
                BaseRecord::to(&self.topic)
                    .payload(payload.as_slice())
                    .key(key),
            ),
            None => send_with_retry(
                &self.producer,
                BaseRecord::<(), [u8]>::to(&self.topic).payload(payload.as_slice()),
            ),
        }
    }
}

fn send_with_retry<K>(
    producer: &BaseProducer<DeliveryTracker>,
    mut record: BaseRecord<'_, K, [u8], ()>,
) -> Result<(), KafkaError>
where
    K: ToBytes + ?Sized,
{
    loop {
        match producer.send(record) {
            Ok(()) => {
                producer.poll(Duration::ZERO);
                return Ok(());
            }
            Err((
                rdkafka::error::KafkaError::MessageProduction(RDKafkaErrorCode::QueueFull),
                returned,
            )) => {
                producer.poll(Duration::from_millis(100));
                record = returned;
            }
            Err((error, _)) => return Err(KafkaError::Client(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{consumer_config, producer_config, KafkaError, KafkaSink};
    use crate::Sink;

    fn no_key(_: &usize) -> Option<String> {
        None
    }

    #[test]
    fn consumer_config_uses_earliest_offsets() {
        let config = consumer_config("localhost:9092", "test-group");

        assert_eq!(config.get("bootstrap.servers"), Some("localhost:9092"));
        assert_eq!(config.get("group.id"), Some("test-group"));
        assert_eq!(config.get("auto.offset.reset"), Some("earliest"));
        assert_eq!(config.get("enable.auto.commit"), Some("false"));
    }

    #[test]
    fn producer_config_enables_idempotence_and_all_acks() {
        let config = producer_config("localhost:9092");

        assert_eq!(config.get("enable.idempotence"), Some("true"));
        assert_eq!(config.get("acks"), Some("all"));
    }

    #[test]
    fn finish_reports_delivery_failures() {
        let mut config = producer_config("127.0.0.1:1");
        config.set("message.timeout.ms", "1000");
        let mut sink: KafkaSink<usize> =
            KafkaSink::from_config(config, "test-topic", no_key).expect("create Kafka sink");
        for record in 0..3 {
            sink.write_record(&record).expect("queue Kafka message");
        }

        let error = sink
            .finish(Duration::from_secs(10))
            .expect_err("report delivery failures");

        assert!(matches!(error, KafkaError::Delivery { failed: 3, .. }));
    }

    #[test]
    fn write_retries_when_the_producer_queue_is_full() {
        let mut config = producer_config("127.0.0.1:1");
        config
            .set("message.timeout.ms", "1000")
            .set("queue.buffering.max.messages", "1");
        let mut sink: KafkaSink<usize> =
            KafkaSink::from_config(config, "test-topic", no_key).expect("create Kafka sink");
        for record in 0..5 {
            sink.write_record(&record)
                .expect("retry full producer queue");
        }

        let error = sink
            .finish(Duration::from_secs(10))
            .expect_err("report delivery failures");

        assert!(matches!(error, KafkaError::Delivery { failed: 5, .. }));
    }
}
