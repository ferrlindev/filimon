// ─────────────────────────────────────────────────────────────────────────────
// streaming.rs - Fluvio Streaming Integration
//
// This module provides the streaming infrastructure for the WISE pipeline,
// using Fluvio as the message broker. Records flow through topics:
//
//   raw-pages   → crawled HTML pages (producer output)
//   articles    → final processed articles (SmartModule output)
//
// Processing is handled by SmartModules in the Fluvio cluster:
//   - map-preprocess: tokenization and text extraction
//   - map-extract: article extraction and relevance scoring
//   - filter-relevance: filter low-relevance articles
// ─────────────────────────────────────────────────────────────────────────────

use fluvio::{Fluvio, Offset, RecordKey, TopicProducerConfigBuilder};
use fluvio::metadata::topic::TopicSpec;
use fluvio::spu::SpuSocketPool;
use futures_util::StreamExt;
use miette::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

// ── Topic Names ───────────────────────────────────────────────────────────────

pub const TOPIC_RAW_PAGES: &str = "filimon-raw-pages";
pub const TOPIC_ARTICLES: &str = "filimon-articles";

// ── Fluvio Client Wrapper ─────────────────────────────────────────────────────

/// Wrapper around Fluvio client providing typed produce/consume operations.
pub struct FluvioStream {
    client: Fluvio,
}

impl FluvioStream {
    /// Connect to the Fluvio cluster.
    pub async fn connect() -> Result<Self> {
        info!("Connecting to Fluvio cluster...");
        let client = Fluvio::connect().await
            .map_err(|e| miette::miette!("Failed to connect to Fluvio: {}", e))?;
        info!("Connected to Fluvio cluster");
        Ok(Self { client })
    }

    /// Get a producer for the specified topic.
    pub async fn producer(&self, topic: &str) -> Result<TypedProducer> {
        let config = TopicProducerConfigBuilder::default()
            .batch_size(16384)
            .linger(std::time::Duration::from_millis(100))
            .build()
            .map_err(|e| miette::miette!("Failed to build producer config: {}", e))?;

        let producer = self
            .client
            .topic_producer_with_config(topic, config)
            .await
            .map_err(|e| miette::miette!("Failed to create producer: {}", e))?;

        Ok(TypedProducer { producer })
    }

    /// Get a consumer stream for the specified topic.
    pub async fn consumer<T: DeserializeOwned>(
        &self,
        topic: &str,
        offset: Offset,
    ) -> Result<TypedConsumer<T>> {
        #[allow(deprecated)]
        let consumer = self
            .client
            .partition_consumer(topic, 0)
            .await
            .map_err(|e| miette::miette!("Failed to create consumer: {}", e))?;

        Ok(TypedConsumer {
            consumer,
            offset,
            _marker: std::marker::PhantomData,
        })
    }

    /// Ensure required topics exist.
    pub async fn ensure_topics(&self) -> Result<()> {
        let admin = self.client.admin().await;
        let topics = [TOPIC_RAW_PAGES, TOPIC_ARTICLES];

        for topic in topics {
            let topic_spec = TopicSpec::new_computed(1, 1, None);
            match admin.create(topic.to_string(), false, topic_spec).await {
                Ok(_) => info!("Created topic: {}", topic),
                Err(e) => warn!("Topic creation note for {}: {}", topic, e),
            }
        }

        Ok(())
    }
}

// ── Typed Producer ────────────────────────────────────────────────────────────

/// A producer that serializes records to JSON before sending.
pub struct TypedProducer {
    producer: fluvio::TopicProducer<SpuSocketPool>,
}

impl TypedProducer {
    /// Send a record with a key.
    pub async fn send<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_vec(value)
            .map_err(|e| miette::miette!("Failed to serialize: {}", e))?;
        self.producer
            .send(RecordKey::from(key.to_string()), json)
            .await
            .map_err(|e| miette::miette!("Failed to send record: {}", e))?;
        Ok(())
    }

    /// Send a record without a key.
    pub async fn send_no_key<T: Serialize>(&self, value: &T) -> Result<()> {
        let json = serde_json::to_vec(value)
            .map_err(|e| miette::miette!("Failed to serialize: {}", e))?;
        self.producer
            .send(RecordKey::NULL, json)
            .await
            .map_err(|e| miette::miette!("Failed to send record: {}", e))?;
        Ok(())
    }

    /// Flush all pending records.
    pub async fn flush(&self) -> Result<()> {
        self.producer.flush().await
            .map_err(|e| miette::miette!("Failed to flush producer: {}", e))?;
        Ok(())
    }
}

// ── Typed Consumer ────────────────────────────────────────────────────────────

/// A consumer that deserializes JSON records into typed values.
pub struct TypedConsumer<T> {
    consumer: fluvio::PartitionConsumer,
    offset: Offset,
    _marker: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned> TypedConsumer<T> {
    /// Process records from the stream with a handler function.
    pub async fn process<F, Fut>(&self, mut handler: F) -> Result<()>
    where
        F: FnMut(T) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        #[allow(deprecated)]
        let mut stream = self.consumer.stream(self.offset.clone()).await
            .map_err(|e| miette::miette!("Failed to create stream: {}", e))?;

        while let Some(result) = stream.next().await {
            match result {
                Ok(record) => {
                    let value = record.get_value().as_ref();
                    match serde_json::from_slice::<T>(value) {
                        Ok(typed_value) => {
                            if let Err(e) = handler(typed_value).await {
                                error!("Handler error: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Deserialization error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Consume a batch of records up to `max_count`.
    pub async fn consume_batch(&self, max_count: usize) -> Result<Vec<T>> {
        #[allow(deprecated)]
        let mut stream = self.consumer.stream(self.offset.clone()).await
            .map_err(|e| miette::miette!("Failed to create stream: {}", e))?;
        let mut results = Vec::with_capacity(max_count);

        while results.len() < max_count {
            match tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await {
                Ok(Some(Ok(record))) => {
                    let value = record.get_value().as_ref();
                    if let Ok(typed_value) = serde_json::from_slice::<T>(value) {
                        results.push(typed_value);
                    }
                }
                Ok(Some(Err(e))) => {
                    error!("Stream error: {}", e);
                    break;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        Ok(results)
    }
}

// ── Pipeline Coordinator ──────────────────────────────────────────────────────

/// Coordinates the streaming pipeline.
pub struct PipelineCoordinator {
    stream: Arc<FluvioStream>,
    raw_pages_producer: Arc<Mutex<TypedProducer>>,
}

impl PipelineCoordinator {
    /// Create a new pipeline coordinator.
    pub async fn new() -> Result<Self> {
        let stream = Arc::new(FluvioStream::connect().await?);
        stream.ensure_topics().await?;

        let raw_pages_producer = Arc::new(Mutex::new(
            stream.producer(TOPIC_RAW_PAGES).await?,
        ));

        Ok(Self {
            stream,
            raw_pages_producer,
        })
    }

    /// Get a producer for raw pages.
    pub fn raw_pages_producer(&self) -> Arc<Mutex<TypedProducer>> {
        Arc::clone(&self.raw_pages_producer)
    }

    /// Get a producer for a specific topic.
    pub async fn producer(&self, topic: &str) -> Result<TypedProducer> {
        self.stream.producer(topic).await
    }

    /// Get a consumer for a specific topic.
    pub async fn consumer<T: DeserializeOwned>(
        &self,
        topic: &str,
        offset: Offset,
    ) -> Result<TypedConsumer<T>> {
        self.stream.consumer(topic, offset).await
    }
}
