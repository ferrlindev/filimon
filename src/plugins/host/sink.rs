// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/sink.rs - Where polled items go
//
// The scheduler hands each poll's records to an `ItemSink` and only then
// persists the cursor (D-07). `FluvioSink` produces to the social topic;
// tests use the in-memory `VecSink`.
// ─────────────────────────────────────────────────────────────────────────────

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::debug;

use crate::models::SourceItem;
use crate::streaming::{FluvioStream, TOPIC_SOCIAL_ITEMS, TypedProducer};

/// Longest `FluvioSink` spends sending and flushing one batch, counted from the
/// moment it holds the producer lock (time spent waiting for the lock is not
/// counted), before it treats the batch as failed.
pub const SINK_TIMEOUT: Duration = Duration::from_mins(1);

/// One record bound for the sink: its message key and the item.
#[derive(Debug, Clone)]
pub struct SinkRecord {
    pub key: String,
    pub item: SourceItem,
}

/// A sink failure. The text is host-generated, never record content.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SinkError(pub String);

/// Receives the records of one poll, in the plugin's list order.
pub trait ItemSink: Send + Sync + 'static {
    /// Sends and flushes `records`. `Ok` means they are delivered, so the
    /// caller may persist the cursor.
    ///
    /// An implementation that can block on I/O bounds its own wait and returns
    /// an `Err` when that bound expires: the caller does not cancel the future,
    /// because dropping it mid-flight would skip the implementation's cleanup.
    fn send_batch(
        &self,
        plugin: &str,
        records: Vec<SinkRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send;
}

/// Produces records to one Fluvio topic.
///
/// The production sink is only ever built for `filimon-social-items`: no
/// plugin or registry value selects a topic.
pub struct FluvioSink {
    producer: Arc<Mutex<TypedProducer>>,
}

impl FluvioSink {
    /// Connects and checks that `topic` exists. A missing topic is named in the
    /// error together with the `fluvio topic create` command.
    pub async fn connect(topic: &str) -> miette::Result<Self> {
        let stream = FluvioStream::connect().await?;
        stream.preflight(&[topic], &[]).await?;
        let producer = stream.producer(topic).await?;
        Ok(Self {
            producer: Arc::new(Mutex::new(producer)),
        })
    }

    /// The sink for social items (`filimon-social-items`).
    pub async fn connect_social() -> miette::Result<Self> {
        Self::connect(TOPIC_SOCIAL_ITEMS).await
    }
}

impl ItemSink for FluvioSink {
    /// Sends every record, then flushes, under one lock so a failure cannot be
    /// attributed to another plugin's batch. Fluvio 0.50.1 keeps returning a
    /// stored producer error until it is cleared, so every failure clears it
    /// before returning and a later batch can succeed.
    ///
    /// The `SINK_TIMEOUT` bound lives here, not in the caller, and starts after
    /// the lock is held: a slow flush by another plugin does not count against
    /// this batch, and an expiry still runs `clear_errors` because the future is
    /// never dropped mid-flight.
    async fn send_batch(&self, plugin: &str, records: Vec<SinkRecord>) -> Result<(), SinkError> {
        let count = records.len();
        let producer = self.producer.lock().await;

        send_bounded(
            SINK_TIMEOUT,
            async {
                for record in &records {
                    producer
                        .send(&record.key, &record.item)
                        .await
                        .map_err(|e| SinkError(e.to_string()))?;
                }
                producer
                    .flush()
                    .await
                    .map_err(|e| SinkError(e.to_string()))
            },
            producer.clear_errors(),
        )
        .await?;

        debug!(plugin = %plugin, count, "sent plugin items");
        Ok(())
    }
}

/// Runs `work` for at most `limit`. On a failure or an expiry it awaits `clear`
/// (the producer's `clear_errors`) before returning, so a stored error cannot
/// poison the next batch. An expiry is reported as `send timed out`.
async fn send_bounded<W, C>(limit: Duration, work: W, clear: C) -> Result<(), SinkError>
where
    W: Future<Output = Result<(), SinkError>>,
    C: Future<Output = ()>,
{
    match tokio::time::timeout(limit, work).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            clear.await;
            Err(error)
        }
        Err(_) => {
            clear.await;
            Err(SinkError("send timed out".to_string()))
        }
    }
}

/// In-memory sink for tests: keeps `(plugin, record)` pairs in arrival order.
#[cfg(test)]
#[derive(Default)]
pub struct VecSink {
    records: std::sync::Mutex<Vec<(String, SinkRecord)>>,
}

#[cfg(test)]
impl VecSink {
    /// Every record received so far, in order.
    pub fn records(&self) -> Vec<SinkRecord> {
        self.records
            .lock()
            .expect("sink lock")
            .iter()
            .map(|(_, record)| record.clone())
            .collect()
    }

    /// The records one plugin sent, in order.
    pub fn plugin_records(&self, plugin: &str) -> Vec<SinkRecord> {
        self.records
            .lock()
            .expect("sink lock")
            .iter()
            .filter(|(name, _)| name == plugin)
            .map(|(_, record)| record.clone())
            .collect()
    }
}

#[cfg(test)]
impl ItemSink for VecSink {
    async fn send_batch(&self, plugin: &str, records: Vec<SinkRecord>) -> Result<(), SinkError> {
        let mut guard = self.records.lock().expect("sink lock");
        for record in records {
            guard.push((plugin.to_string(), record));
        }
        Ok(())
    }
}

/// Scratch-topic helpers for cluster-backed tests. They never touch
/// `filimon-social-items`.
#[cfg(test)]
pub(crate) mod live_support {
    use std::time::Duration;

    use fluvio::metadata::topic::TopicSpec;

    use super::FluvioSink;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    }

    async fn admin() -> miette::Result<fluvio::FluvioAdmin> {
        Ok(fluvio::Fluvio::connect()
            .await
            .map_err(|e| miette::miette!("connect: {e}"))?
            .admin()
            .await)
    }

    /// Creates a scratch topic `filimon-test-<kind>-<nanos>-<pid>-<n>`.
    ///
    /// Tests run in parallel and the clock can be coarser than a nanosecond,
    /// so the process id and a counter keep names unique.
    pub async fn scratch_topic(kind: &str) -> miette::Result<String> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let topic = format!(
            "filimon-test-{kind}-{}-{}-{}",
            nanos(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        admin()
            .await?
            .create(topic.clone(), false, TopicSpec::new_computed(1, 1, None))
            .await
            .map_err(|e| miette::miette!("create {topic}: {e}"))?;
        Ok(topic)
    }

    /// Deletes a scratch topic.
    pub async fn delete_topic(topic: &str) -> miette::Result<()> {
        admin()
            .await?
            .delete::<TopicSpec>(topic.to_string())
            .await
            .map_err(|e| miette::miette!("delete {topic}: {e}"))
    }

    /// A sink for a new topic; it can take a moment to provision.
    pub async fn ready_sink(topic: &str) -> miette::Result<FluvioSink> {
        for _ in 0..20 {
            if let Ok(sink) = FluvioSink::connect(topic).await {
                return Ok(sink);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(miette::miette!("no sink for {topic}"))
    }
}

#[cfg(test)]
mod tests {
    use fluvio::Offset;
    use futures_util::{FutureExt, StreamExt};

    use super::live_support::{delete_topic, ready_sink, scratch_topic};
    use super::*;

    fn assert_item_sink<S: ItemSink + Send + Sync + 'static>() {}

    fn record(id: &str) -> SinkRecord {
        SinkRecord {
            key: format!("p:{id}"),
            item: SourceItem::social(
                id.to_string(),
                format!("https://example.invalid/{id}"),
                Some(format!("title {id}")),
                format!("text {id}"),
                None,
                Some(1_730_000_000_123),
                "p".to_string(),
                Some("{\"n\":1}".to_string()),
            ),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn send_bounded_clears_errors_when_the_timeout_expires() {
        let cleared = std::sync::atomic::AtomicBool::new(false);
        let result = send_bounded(
            Duration::from_secs(60),
            std::future::pending::<Result<(), SinkError>>(),
            async { cleared.store(true, std::sync::atomic::Ordering::SeqCst) },
        )
        .await;

        let error = result.expect_err("a stuck send times out");
        assert_eq!(error.0, "send timed out");
        assert!(cleared.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn send_bounded_clears_errors_on_a_failure_and_keeps_its_text() {
        let cleared = std::sync::atomic::AtomicBool::new(false);
        let result = send_bounded(
            Duration::from_secs(60),
            async { Err(SinkError("broker down".to_string())) },
            async { cleared.store(true, std::sync::atomic::Ordering::SeqCst) },
        )
        .await;

        assert_eq!(result.expect_err("fails").0, "broker down");
        assert!(cleared.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn send_bounded_does_not_clear_on_success_and_work_may_take_until_the_limit() {
        let cleared = std::sync::atomic::AtomicBool::new(false);
        let result = send_bounded(
            Duration::from_secs(60),
            async {
                tokio::time::sleep(Duration::from_secs(59)).await;
                Ok(())
            },
            async { cleared.store(true, std::sync::atomic::Ordering::SeqCst) },
        )
        .await;

        assert!(result.is_ok());
        assert!(!cleared.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn fluvio_sink_is_an_item_sink() {
        assert_item_sink::<FluvioSink>();
        assert_item_sink::<VecSink>();
    }

    #[tokio::test]
    async fn vec_sink_keeps_batch_order() {
        let sink = VecSink::default();
        sink.send_batch("a", vec![record("1"), record("2")])
            .await
            .expect("batch a");
        sink.send_batch("b", vec![record("3"), record("4")])
            .await
            .expect("batch b");

        let keys: Vec<_> = sink.records().into_iter().map(|r| r.key).collect();
        assert_eq!(keys, ["p:1", "p:2", "p:3", "p:4"]);
        let a: Vec<_> = sink
            .plugin_records("a")
            .into_iter()
            .map(|r| r.key)
            .collect();
        let b: Vec<_> = sink
            .plugin_records("b")
            .into_iter()
            .map(|r| r.key)
            .collect();
        assert_eq!(a, ["p:1", "p:2"]);
        assert_eq!(b, ["p:3", "p:4"]);
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with topic creation rights"]
    async fn live_fluvio_sink_produces_records_in_order_with_keys() -> miette::Result<()> {
        let topic = scratch_topic("sink").await?;
        let body = std::panic::AssertUnwindSafe(async {
            let sink = ready_sink(&topic).await?;
            let sent = vec![record("1"), record("2")];
            sink.send_batch("p", sent.clone())
                .await
                .map_err(|e| miette::miette!("send_batch: {e}"))?;

            let stream = FluvioStream::connect().await?;
            let mut records = stream
                .topic_stream(&topic, Offset::beginning(), Vec::new())
                .await?;
            for expected in &sent {
                let got = tokio::time::timeout(Duration::from_secs(10), records.next())
                    .await
                    .map_err(|_| miette::miette!("timed out waiting for a record"))?
                    .ok_or_else(|| miette::miette!("stream ended early"))?
                    .map_err(|e| miette::miette!("stream error: {e:?}"))?;
                let key = got.key().map(|k| String::from_utf8_lossy(k).into_owned());
                assert_eq!(key.as_deref(), Some(expected.key.as_str()));
                let item: SourceItem =
                    serde_json::from_slice(got.value()).expect("value is a SourceItem");
                assert_eq!(item, expected.item);
            }
            Ok::<(), miette::Report>(())
        })
        .catch_unwind()
        .await;
        let deleted = delete_topic(&topic).await;
        match body {
            Ok(outcome) => {
                outcome?;
                deleted
            }
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// A failed batch clears the producer's sticky error, so the next batch on
    /// the same sink succeeds. The failure here is a record over the broker's
    /// request limit, which is what `MAX_RECORD_BYTES` exists to prevent. The
    /// timeout expiry path (a stuck broker) needs a cluster that stops
    /// responding and is covered only by the `send_bounded` unit tests.
    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with topic creation rights"]
    async fn live_fluvio_sink_recovers_after_a_failed_batch() -> miette::Result<()> {
        let topic = scratch_topic("sink-clear").await?;
        let body = std::panic::AssertUnwindSafe(async {
            let sink = ready_sink(&topic).await?;

            let mut huge = record("huge");
            huge.item.text = "a".repeat(2_000_000);
            let failed = sink.send_batch("p", vec![huge]).await;
            assert!(failed.is_err(), "a 2 MB record must be refused");

            sink.send_batch("p", vec![record("after")])
                .await
                .map_err(|e| miette::miette!("batch after the failure: {e}"))?;
            Ok::<(), miette::Report>(())
        })
        .catch_unwind()
        .await;
        let deleted = delete_topic(&topic).await;
        match body {
            Ok(outcome) => {
                outcome?;
                deleted
            }
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Delegates to a `FluvioSink` and counts the records it delivered.
    struct CountingSink {
        inner: FluvioSink,
        delivered: std::sync::atomic::AtomicUsize,
    }

    impl CountingSink {
        fn new(inner: FluvioSink) -> Self {
            Self {
                inner,
                delivered: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn delivered(&self) -> usize {
            self.delivered.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl ItemSink for CountingSink {
        async fn send_batch(
            &self,
            plugin: &str,
            records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            let count = records.len();
            self.inner.send_batch(plugin, records).await?;
            self.delivered
                .fetch_add(count, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    /// Runs a host over `registry` into `topic` until `at_least` records were
    /// delivered (30 s limit), then cancels and joins. Returns the final count.
    async fn run_host_until(
        registry: &crate::plugins::config::LoadedRegistry,
        state_dir: &std::path::Path,
        topic: &str,
        at_least: usize,
    ) -> miette::Result<usize> {
        use tokio_util::sync::CancellationToken;

        use super::super::{Host, HostOptions};

        let sink = Arc::new(CountingSink::new(ready_sink(topic).await?));
        let host = Host::load(
            registry,
            HostOptions {
                state_dir: state_dir.to_path_buf(),
            },
        )
        .map_err(|e| miette::miette!("Host::load: {e}"))?;
        let token = CancellationToken::new();
        let handle = tokio::spawn(host.run(Arc::clone(&sink), token.clone()));

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while sink.delivered() < at_least {
            if tokio::time::Instant::now() >= deadline {
                token.cancel();
                let _ = handle.await;
                return Err(miette::miette!(
                    "only {} of {at_least} records were produced within 30 s",
                    sink.delivered()
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        token.cancel();
        let report = handle
            .await
            .map_err(|e| miette::miette!("host task: {e}"))?;
        let stub = report
            .plugins
            .get("stub")
            .ok_or_else(|| miette::miette!("no report for stub"))?;
        if let Some(reason) = &stub.disabled {
            return Err(miette::miette!("stub was disabled: {reason}"));
        }
        Ok(sink.delivered())
    }

    /// SC2 on a real cluster: the real stub component produces `SourceItem`s
    /// to a scratch topic, and a stop and restart on the same state directory
    /// continues the sequence with no gap and no repeat. Real clock: the Fluvio
    /// client's I/O and the host's sink timeout must not see an auto-advanced
    /// clock.
    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster and plugin fixtures: see plugins/README.md"]
    async fn live_host_end_to_end_stub_to_fluvio_with_restart() -> miette::Result<()> {
        use super::super::testing::{
            TestSource, fixture_wasm, load_test_registry, write_registry,
        };
        use crate::models::SourceType;

        let topic = scratch_topic("host").await?;
        let body = std::panic::AssertUnwindSafe(async {
            let tmp = tempfile::TempDir::new().map_err(|e| miette::miette!("tempdir: {e}"))?;
            let registry_path = write_registry(
                tmp.path(),
                &[TestSource {
                    name: "stub".to_string(),
                    component: fixture_wasm("stub-source", "stub_source.wasm"),
                    enabled: true,
                    interval_secs: 1,
                    backoff: None,
                    settings: "greeting = \"live end to end\"".to_string(),
                }],
            );
            let registry = load_test_registry(&registry_path);
            let state_dir = tmp.path().join(".fili-state");

            // Run 1, then a stop; run 2 on the same state directory.
            let n1 = run_host_until(&registry, &state_dir, &topic, 2).await?;
            let n2 = run_host_until(&registry, &state_dir, &topic, 1).await?;
            assert!(n1 >= 2, "run 1 produced {n1}");
            assert!(n2 >= 1, "run 2 produced {n2}");
            let total = n1 + n2;

            let stream = FluvioStream::connect().await?;
            let mut records = stream
                .topic_stream(&topic, Offset::beginning(), Vec::new())
                .await?;
            let mut ids = Vec::new();
            for _ in 0..total {
                let got = tokio::time::timeout(Duration::from_secs(15), records.next())
                    .await
                    .map_err(|_| miette::miette!("timed out reading back {total} records"))?
                    .ok_or_else(|| miette::miette!("stream ended early"))?
                    .map_err(|e| miette::miette!("stream error: {e:?}"))?;
                let item: SourceItem =
                    serde_json::from_slice(got.value()).expect("value is a SourceItem");
                assert_eq!(item.platform, "stub");
                assert_eq!(item.source_type, SourceType::Social);
                let key = got.key().map(|k| String::from_utf8_lossy(k).into_owned());
                assert_eq!(key, Some(format!("stub:{}", item.id)));
                ids.push(item.id);
            }

            // No gap and no repeat across the restart.
            let expected: Vec<String> = (0..total).map(|n| format!("stub-{n}")).collect();
            assert_eq!(ids, expected, "n1 = {n1}, n2 = {n2}");
            Ok::<(), miette::Report>(())
        })
        .catch_unwind()
        .await;
        let deleted = delete_topic(&topic).await;
        match body {
            Ok(outcome) => {
                outcome?;
                deleted
            }
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster with topic creation rights"]
    async fn live_fluvio_sink_connect_names_a_missing_topic() -> miette::Result<()> {
        // A unique name that was never created, so it cannot exist.
        let topic = format!(
            "filimon-test-missing-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos(),
            std::process::id()
        );
        let text = match FluvioSink::connect(&topic).await {
            Ok(_) => panic!("connect to a missing topic should fail"),
            Err(e) => format!("{e}"),
        };
        assert!(text.contains("fluvio topic create"), "{text}");
        assert!(text.contains(&topic), "{text}");
        Ok(())
    }
}
