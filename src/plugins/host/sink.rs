// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/sink.rs - Where polled items go
//
// The scheduler hands each poll's records to an `ItemSink` and only then
// persists the cursor (D-07). The Fluvio sink arrives in a later plan; tests
// use the in-memory `VecSink`.
// ─────────────────────────────────────────────────────────────────────────────

use std::future::Future;
use std::time::Duration;

use crate::models::SourceItem;

/// Longest the host waits for one `send_batch` before treating it as failed.
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
    fn send_batch(
        &self,
        plugin: &str,
        records: Vec<SinkRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send;
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
