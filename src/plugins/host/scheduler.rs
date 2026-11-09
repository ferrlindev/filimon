// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/scheduler.rs - The per-plugin task loop
//
// One task per enabled source. A whole cycle (poll, map, send, persist) runs
// before the task waits and polls again, so `poll` is never concurrent on one
// instance. The cursor is written only after `send_batch` returned (D-07).
//
// Every non-success outcome ends this plugin's task with a `disabled` reason.
// The D-01..D-04 backoff and rebuild state machine replaces those branches in
// a later plan; nothing here retries.
//
// Nothing logged here carries settings, secret values, init arguments or item
// content: only the plugin name, counts and cleaned error text.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::cursor::CursorStore;
use super::engine::{PollError, clean_guest_text};
use super::instance::{CallOutcome, InstanceFactory, PluginInstance};
use super::map::map_items;
use super::sink::{ItemSink, SINK_TIMEOUT, SinkRecord};
use crate::plugins::config::{HostConfig, LoadedSource};
use crate::plugins::secrets::ResolvedSecrets;

/// Longest error text kept in a report or log line, in characters.
const REPORT_TEXT_CLIP: usize = 512;

/// Everything one plugin task needs. `ResolvedSecrets` redacts its values in
/// Debug, and there is no hand-written Debug here.
#[derive(Debug)]
pub struct PluginSpec {
    pub name: String,
    pub host: HostConfig,
    pub settings_toml: String,
    pub secrets: ResolvedSecrets,
}

impl PluginSpec {
    /// `Some` only for an enabled source whose plugin file loaded.
    #[must_use]
    pub fn from_loaded(source: &LoadedSource) -> Option<Self> {
        if !source.enabled {
            return None;
        }
        let plugin = source.plugin.as_ref()?;
        Some(Self {
            name: source.name.clone(),
            host: plugin.host.clone(),
            settings_toml: plugin.settings_toml.clone(),
            secrets: plugin.secrets.clone(),
        })
    }
}

/// What one plugin task did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginReport {
    pub polls: u64,
    pub items_sent: u64,
    pub dropped: u64,
    pub failures: u64,
    /// Why the task stopped on its own, when it did.
    pub disabled: Option<String>,
}

/// Describes a guest error without echoing it unsanitised.
fn describe_guest_error(error: &PollError) -> String {
    let text = match error {
        PollError::RateLimited(Some(secs)) => format!("rate-limited (retry after {secs}s)"),
        PollError::RateLimited(None) => "rate-limited".to_string(),
        PollError::AuthRequired => "auth-required".to_string(),
        PollError::Config(msg) => format!("config error: {msg}"),
        PollError::Transient(msg) => format!("transient error: {msg}"),
        PollError::Fatal(msg) => format!("fatal error: {msg}"),
    };
    clean_guest_text(&text, REPORT_TEXT_CLIP)
}

/// Logs the reason and records it as the plugin's `disabled` text.
fn disable(report: &mut PluginReport, plugin: &str, reason: String) {
    tracing::error!(plugin = %plugin, "plugin disabled: {reason}");
    report.disabled = Some(reason);
}

/// Sends one poll's records, bounded by `SINK_TIMEOUT`. Returns the number of
/// records sent, or a host-side failure reason (never blamed on the plugin).
async fn send_records<S: ItemSink>(
    sink: &S,
    plugin: &str,
    records: Vec<SinkRecord>,
) -> Result<u64, String> {
    if records.is_empty() {
        return Ok(0);
    }
    let count = records.len() as u64;
    match tokio::time::timeout(SINK_TIMEOUT, sink.send_batch(plugin, records)).await {
        Ok(Ok(())) => Ok(count),
        Ok(Err(e)) => Err(format!(
            "sink failed (host side): {}",
            clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP)
        )),
        Err(_) => Err("sink timed out (host side)".to_string()),
    }
}

/// Runs one plugin until shutdown or until it is disabled.
pub async fn run_plugin<F: InstanceFactory, S: ItemSink>(
    spec: PluginSpec,
    factory: Arc<F>,
    sink: Arc<S>,
    cursors: Arc<CursorStore>,
    shutdown: CancellationToken,
) -> PluginReport {
    let mut report = PluginReport::default();

    // a. The persisted cursor. A bad file disables only this plugin (D-08).
    let mut cursor = match cursors.read(&spec.name) {
        Ok(cursor) => cursor,
        Err(e) => {
            report.failures += 1;
            let reason = clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP);
            disable(&mut report, &spec.name, reason);
            return report;
        }
    };

    // b. A fresh instance.
    let mut instance = match factory.instantiate(&spec.name).await {
        Ok(instance) => instance,
        Err(e) => {
            report.failures += 1;
            disable(
                &mut report,
                &spec.name,
                format!("cannot build instance: {}", e.0),
            );
            return report;
        }
    };

    // c. `init`. The only place a secret value is read.
    let secrets: Vec<(String, String)> = spec
        .secrets
        .iter()
        .map(|(alias, secret)| (alias.clone(), secret.expose().to_string()))
        .collect();
    match instance.init(&spec.settings_toml, &secrets).await {
        CallOutcome::Ok(()) => {}
        CallOutcome::Guest(e) => {
            report.failures += 1;
            let reason = format!("init failed: {}", describe_guest_error(&e));
            disable(&mut report, &spec.name, reason);
            return report;
        }
        CallOutcome::InstanceFatal(text) => {
            report.failures += 1;
            disable(&mut report, &spec.name, format!("init trapped: {text}"));
            return report;
        }
    }

    // d. The poll loop.
    loop {
        let outcome = tokio::select! {
            biased;
            () = shutdown.cancelled() => return report,
            outcome = instance.poll(cursor.as_deref()) => outcome,
        };

        match outcome {
            CallOutcome::Ok(response) => {
                report.polls += 1;
                let mapped = map_items(&spec.name, response.items);
                report.dropped += mapped.dropped as u64;

                // Not raced against shutdown: a stop must not split send
                // from persist.
                match send_records(&*sink, &spec.name, mapped.records).await {
                    Ok(count) => report.items_sent += count,
                    Err(reason) => {
                        report.failures += 1;
                        disable(&mut report, &spec.name, reason);
                        return report;
                    }
                }

                // D-07: persist only after the sink returned. A failure keeps
                // the in-memory cursor and keeps running (A6).
                if let Err(e) = cursors.write(&spec.name, response.next_cursor.as_deref()) {
                    report.failures += 1;
                    tracing::error!(
                        plugin = %spec.name,
                        "{}",
                        clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP)
                    );
                }
                cursor = response.next_cursor;
            }
            CallOutcome::Guest(e) => {
                report.failures += 1;
                let reason = format!("poll failed: {}", describe_guest_error(&e));
                disable(&mut report, &spec.name, reason);
                return report;
            }
            CallOutcome::InstanceFatal(text) => {
                report.failures += 1;
                disable(&mut report, &spec.name, format!("poll trapped: {text}"));
                return report;
            }
        }

        // e. Fixed delay after the cycle; the first poll had no wait.
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return report,
            () = tokio::time::sleep(spec.host.poll_interval) => {}
        }
    }
}
