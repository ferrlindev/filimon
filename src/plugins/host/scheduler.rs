// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/scheduler.rs - The per-plugin task loop
//
// One task per enabled source. A whole cycle (poll, map, send, persist) runs
// before the task waits and polls again, so `poll` is never concurrent on one
// instance. The cursor is written only after `send_batch` returned (D-07).
//
// Every cycle ends in a `CycleOutcome`. The pure `next_action` turns it into an
// `Action` (D-01..D-04): wait and retry, wait then rebuild the instance, or
// disable the plugin until `fili` restarts. `run_plugin` only drives that
// state machine, so the rules are tested without a clock or wasm.
//
// Nothing logged here carries settings, secret values, init arguments or item
// content: only the plugin name, counts, seconds and cleaned error text.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::backoff;
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
    /// Polls the guest answered successfully.
    pub polls: u64,
    pub items_sent: u64,
    pub dropped: u64,
    pub failures: u64,
    /// Why the task stopped on its own, when it did.
    pub disabled: Option<String>,
}

// ── State Machine ─────────────────────────────────────────────────────────────

/// Which guest call produced an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Init,
    Poll,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Poll => "poll",
        }
    }
}

/// How one cycle ended.
#[derive(Debug)]
pub enum CycleOutcome {
    /// The guest answered, the batch was sent and the cursor persisted.
    PollOk,
    /// The guest returned an error; the instance is still usable.
    Guest { phase: Phase, error: PollError },
    /// A trap or host failure inside a wasmtime call (cleaned text).
    InstanceFatal(String),
    /// A fresh instance could not be built (cleaned text).
    BuildFailed(String),
    /// The sink failed or timed out: a host-side failure, not the plugin's.
    SinkFailed(String),
}

/// What the loop does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Wait, then continue. With `discard_instance` the next attempt builds a
    /// fresh instance and calls `init` again.
    Wait {
        wait: Duration,
        discard_instance: bool,
    },
    /// Stop this plugin until `fili` restarts.
    Disable { reason: String },
}

/// The failure count of one plugin. Each plugin task owns its own.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginState {
    /// Consecutive failures since the last successful poll.
    pub failures: u32,
}

/// Turns a cycle outcome into the next action. Pure: no clock, no IO, no
/// logging.
///
/// - D-01: `rate-limited(Some(n))` waits `max(n s, step)` capped at the
///   plugin's `max_secs`; `rate-limited(None)` waits the step alone.
/// - D-02: the step is `initial * multiplier^failures` capped at `max`; each
///   consecutive transient, rate-limited or auth-required failure advances it
///   and one successful poll resets it. After the wait the plugin is polled
///   again, then returns to `poll_interval`. No jitter, no decay.
/// - D-03: `fatal` and `config` disable the plugin; `auth-required` backs off
///   exactly like `transient`.
/// - D-04: a trap, host error or build failure discards the instance and
///   counts as a transient failure; a failed `init` also discards it, because
///   `init` runs once per instance. `SinkFailed` reuses the plugin's backoff
///   (RESEARCH A9) and keeps the instance.
///
/// Literal D-02 note (RESEARCH A10): a backoff window can be shorter than
/// `poll_interval_secs`. That is the locked decision and is not adjusted.
pub fn next_action(state: &mut PluginState, cfg: &HostConfig, outcome: &CycleOutcome) -> Action {
    let step = backoff::step(&cfg.backoff, state.failures);
    let (wait, discard_instance) = match outcome {
        CycleOutcome::PollOk => {
            state.failures = 0;
            return Action::Wait {
                wait: cfg.poll_interval,
                discard_instance: false,
            };
        }
        CycleOutcome::Guest { phase, error } => {
            let discard = *phase == Phase::Init;
            match error {
                PollError::Config(_) | PollError::Fatal(_) => {
                    return Action::Disable {
                        reason: describe(outcome),
                    };
                }
                PollError::Transient(_) | PollError::AuthRequired => (step, discard),
                PollError::RateLimited(retry_after) => {
                    (backoff::rate_limited_wait(*retry_after, step, &cfg.backoff), discard)
                }
            }
        }
        CycleOutcome::InstanceFatal(_) | CycleOutcome::BuildFailed(_) => (step, true),
        CycleOutcome::SinkFailed(_) => (step, false),
    };
    state.failures = state.failures.saturating_add(1);
    Action::Wait {
        wait,
        discard_instance,
    }
}

/// A short label for an outcome. Every guest- or wasmtime-supplied string is
/// cleaned and clipped first.
#[must_use]
pub fn describe(outcome: &CycleOutcome) -> String {
    let clean = |text: &str| clean_guest_text(text, REPORT_TEXT_CLIP);
    match outcome {
        CycleOutcome::PollOk => "poll ok".to_string(),
        CycleOutcome::Guest { phase, error } => {
            let phase = phase.label();
            match error {
                PollError::Transient(text) => format!("{phase} transient: {}", clean(text)),
                PollError::Config(text) => format!("{phase} config error: {}", clean(text)),
                PollError::Fatal(text) => format!("{phase} fatal error: {}", clean(text)),
                PollError::RateLimited(retry_after) => {
                    format!("{phase} rate-limited (retry-after {retry_after:?})")
                }
                PollError::AuthRequired => format!("{phase} auth-required"),
            }
        }
        CycleOutcome::InstanceFatal(text) => format!("instance fault: {}", clean(text)),
        CycleOutcome::BuildFailed(text) => format!("instance build failed: {}", clean(text)),
        CycleOutcome::SinkFailed(text) => format!("item sink failed: {}", clean(text)),
    }
}

// ── The Loop ──────────────────────────────────────────────────────────────────

/// Logs the reason and records it as the plugin's `disabled` text.
fn disable(report: &mut PluginReport, plugin: &str, reason: String) {
    tracing::error!(plugin = %plugin, reason = %reason, "plugin disabled until fili restarts");
    report.disabled = Some(reason);
}

/// Sends one poll's records, bounded by `SINK_TIMEOUT`. Returns the number of
/// records sent, or a host-side failure text (never blamed on the plugin).
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
        Ok(Err(e)) => Err(clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP)),
        Err(_) => Err("send timed out".to_string()),
    }
}

/// What the loop does after `settle`.
enum Next {
    /// Shutdown or disabled: return the report.
    Stop,
    /// Build a fresh instance and call `init` again.
    Rebuild,
    /// Keep polling the same instance.
    Same,
}

/// Applies `next_action` to `outcome`: logs, waits (cancellable) and says what
/// the loop does next.
async fn settle(
    spec: &PluginSpec,
    state: &mut PluginState,
    report: &mut PluginReport,
    shutdown: &CancellationToken,
    outcome: &CycleOutcome,
) -> Next {
    match next_action(state, &spec.host, outcome) {
        Action::Disable { reason } => {
            report.failures += 1;
            disable(report, &spec.name, reason);
            Next::Stop
        }
        Action::Wait {
            wait,
            discard_instance,
        } => {
            if !matches!(outcome, CycleOutcome::PollOk) {
                report.failures += 1;
                if matches!(outcome, CycleOutcome::SinkFailed(_)) {
                    tracing::warn!(
                        plugin = %spec.name,
                        failures = state.failures,
                        wait_secs = wait.as_secs(),
                        reason = %describe(outcome),
                        "item sink failed, the plugin is not at fault; backing off"
                    );
                } else {
                    tracing::warn!(
                        plugin = %spec.name,
                        failures = state.failures,
                        wait_secs = wait.as_secs(),
                        reason = %describe(outcome),
                        "plugin failed; backing off"
                    );
                }
            }
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return Next::Stop,
                () = tokio::time::sleep(wait) => {}
            }
            if discard_instance {
                Next::Rebuild
            } else {
                Next::Same
            }
        }
    }
}

/// Runs one plugin until shutdown or until it is disabled.
#[allow(clippy::too_many_lines, reason = "one linear instance lifecycle loop")]
pub async fn run_plugin<F: InstanceFactory, S: ItemSink>(
    spec: PluginSpec,
    factory: Arc<F>,
    sink: Arc<S>,
    cursors: Arc<CursorStore>,
    shutdown: CancellationToken,
) -> PluginReport {
    let mut report = PluginReport::default();
    let mut state = PluginState::default();

    // The persisted cursor. A bad file disables only this plugin (D-08).
    let mut cursor = match cursors.read(&spec.name) {
        Ok(cursor) => cursor,
        Err(e) => {
            report.failures += 1;
            let reason = clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP);
            disable(&mut report, &spec.name, reason);
            return report;
        }
    };

    // The only place a secret value is read.
    let secrets: Vec<(String, String)> = spec
        .secrets
        .iter()
        .map(|(alias, secret)| (alias.clone(), secret.expose().to_string()))
        .collect();

    'instances: loop {
        // A fresh instance.
        let mut instance = match factory.instantiate(&spec.name).await {
            Ok(instance) => instance,
            Err(e) => {
                let outcome = CycleOutcome::BuildFailed(e.0);
                match settle(&spec, &mut state, &mut report, &shutdown, &outcome).await {
                    Next::Stop => return report,
                    Next::Rebuild | Next::Same => continue 'instances,
                }
            }
        };

        // `init`, once per instance.
        let init = tokio::select! {
            biased;
            () = shutdown.cancelled() => return report,
            init = instance.init(&spec.settings_toml, &secrets) => init,
        };
        let init_failure = match init {
            CallOutcome::Ok(()) => None,
            CallOutcome::Guest(error) => Some(CycleOutcome::Guest {
                phase: Phase::Init,
                error,
            }),
            CallOutcome::InstanceFatal(text) => Some(CycleOutcome::InstanceFatal(text)),
        };
        if let Some(outcome) = init_failure {
            match settle(&spec, &mut state, &mut report, &shutdown, &outcome).await {
                Next::Stop => return report,
                Next::Rebuild | Next::Same => continue 'instances,
            }
        }

        // The poll loop on this instance.
        loop {
            let polled = tokio::select! {
                biased;
                () = shutdown.cancelled() => return report,
                polled = instance.poll(cursor.as_deref()) => polled,
            };

            let outcome = match polled {
                CallOutcome::Ok(response) => {
                    report.polls += 1;
                    let mapped = map_items(&spec.name, response.items);
                    let dropped = mapped.dropped as u64;

                    // Not raced against shutdown: a stop must not split send
                    // from persist.
                    match send_records(&*sink, &spec.name, mapped.records).await {
                        Ok(count) => {
                            report.items_sent += count;
                            report.dropped += dropped;

                            // D-07: persist only after the sink returned. A
                            // failure keeps the in-memory cursor and keeps
                            // running (A6).
                            if let Err(e) =
                                cursors.write(&spec.name, response.next_cursor.as_deref())
                            {
                                report.failures += 1;
                                tracing::error!(
                                    plugin = %spec.name,
                                    "{}",
                                    clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP)
                                );
                            }
                            cursor = response.next_cursor;
                            CycleOutcome::PollOk
                        }
                        // The cursor stays unwritten and unchanged.
                        Err(text) => CycleOutcome::SinkFailed(text),
                    }
                }
                CallOutcome::Guest(error) => CycleOutcome::Guest {
                    phase: Phase::Poll,
                    error,
                },
                CallOutcome::InstanceFatal(text) => CycleOutcome::InstanceFatal(text),
            };

            match settle(&spec, &mut state, &mut report, &shutdown, &outcome).await {
                Next::Stop => return report,
                Next::Rebuild => continue 'instances,
                Next::Same => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::config::BackoffConfig;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// Interval 300 s, backoff initial 60 s, max 3600 s, multiplier 2.0.
    fn cfg() -> HostConfig {
        HostConfig {
            poll_interval: secs(300),
            timeout: secs(30),
            allowlist: vec![],
            backoff: BackoffConfig {
                initial: secs(60),
                max: secs(3600),
                multiplier: 2.0,
            },
        }
    }

    fn poll(error: PollError) -> CycleOutcome {
        CycleOutcome::Guest {
            phase: Phase::Poll,
            error,
        }
    }

    fn init(error: PollError) -> CycleOutcome {
        CycleOutcome::Guest {
            phase: Phase::Init,
            error,
        }
    }

    fn wait(secs_: u64, discard_instance: bool) -> Action {
        Action::Wait {
            wait: secs(secs_),
            discard_instance,
        }
    }

    fn transient() -> PollError {
        PollError::Transient("later".to_string())
    }

    #[test]
    fn poll_ok_resets_failures_and_waits_the_interval() {
        let mut state = PluginState { failures: 2 };
        let action = next_action(&mut state, &cfg(), &CycleOutcome::PollOk);
        assert_eq!(action, wait(300, false));
        assert_eq!(state.failures, 0);
    }

    #[test]
    fn transient_waits_step_and_increments_failures() {
        let mut state = PluginState::default();
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(transient())),
            wait(60, false)
        );
        assert_eq!(state.failures, 1);
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(transient())),
            wait(120, false)
        );
        assert_eq!(state.failures, 2);
    }

    #[test]
    fn rate_limited_some_waits_max_of_retry_after_and_step() {
        let mut state = PluginState::default();
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(PollError::RateLimited(Some(200)))),
            wait(200, false)
        );
        let mut state = PluginState::default();
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(PollError::RateLimited(Some(30)))),
            wait(60, false)
        );
    }

    #[test]
    fn rate_limited_none_waits_the_step() {
        let mut state = PluginState { failures: 1 };
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(PollError::RateLimited(None))),
            wait(120, false)
        );
        assert_eq!(state.failures, 2);
    }

    #[test]
    fn rate_limited_wait_is_capped_at_backoff_max() {
        let mut state = PluginState::default();
        assert_eq!(
            next_action(
                &mut state,
                &cfg(),
                &poll(PollError::RateLimited(Some(100_000)))
            ),
            wait(3600, false)
        );
    }

    #[test]
    fn auth_required_backs_off_like_transient() {
        let mut a = PluginState { failures: 1 };
        let mut b = PluginState { failures: 1 };
        assert_eq!(
            next_action(&mut a, &cfg(), &poll(PollError::AuthRequired)),
            next_action(&mut b, &cfg(), &poll(transient()))
        );
        assert_eq!(a, b);
    }

    #[test]
    fn fatal_disables_with_cleaned_reason() {
        let mut state = PluginState::default();
        let action = next_action(
            &mut state,
            &cfg(),
            &poll(PollError::Fatal("boom\u{1b}[31m red".to_string())),
        );
        match action {
            Action::Disable { reason } => {
                assert!(reason.contains("boom"), "{reason}");
                assert!(!reason.contains('\u{1b}'), "{reason:?}");
            }
            other @ Action::Wait { .. } => panic!("expected Disable, got {other:?}"),
        }
    }

    #[test]
    fn config_disables() {
        for outcome in [
            poll(PollError::Config("bad setting".to_string())),
            init(PollError::Config("bad setting".to_string())),
        ] {
            let mut state = PluginState::default();
            assert!(matches!(
                next_action(&mut state, &cfg(), &outcome),
                Action::Disable { .. }
            ));
        }
    }

    #[test]
    fn instance_fatal_discards_instance_and_backs_off() {
        let mut state = PluginState::default();
        let action = next_action(
            &mut state,
            &cfg(),
            &CycleOutcome::InstanceFatal("trap".to_string()),
        );
        assert_eq!(action, wait(60, true));
        assert_eq!(state.failures, 1);
    }

    #[test]
    fn build_failure_backs_off_and_discards() {
        let mut state = PluginState::default();
        let action = next_action(
            &mut state,
            &cfg(),
            &CycleOutcome::BuildFailed("no memory".to_string()),
        );
        assert_eq!(action, wait(60, true));
        assert_eq!(state.failures, 1);
    }

    #[test]
    fn sink_failure_backs_off_without_discarding() {
        let mut state = PluginState::default();
        let action = next_action(
            &mut state,
            &cfg(),
            &CycleOutcome::SinkFailed("broker down".to_string()),
        );
        assert_eq!(action, wait(60, false));
        assert_eq!(state.failures, 1);
    }

    #[test]
    fn init_transient_discards_the_instance() {
        for outcome in [init(transient()), init(PollError::RateLimited(None))] {
            let mut state = PluginState::default();
            assert!(matches!(
                next_action(&mut state, &cfg(), &outcome),
                Action::Wait {
                    discard_instance: true,
                    ..
                }
            ));
        }
        for outcome in [poll(transient()), poll(PollError::RateLimited(None))] {
            let mut state = PluginState::default();
            assert!(matches!(
                next_action(&mut state, &cfg(), &outcome),
                Action::Wait {
                    discard_instance: false,
                    ..
                }
            ));
        }
    }

    #[test]
    fn consecutive_failures_grow_then_one_success_resets_fully() {
        let mut state = PluginState::default();
        for expected in [60, 120, 240] {
            assert_eq!(
                next_action(&mut state, &cfg(), &poll(transient())),
                wait(expected, false)
            );
        }
        let _ = next_action(&mut state, &cfg(), &CycleOutcome::PollOk);
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(transient())),
            wait(60, false)
        );
    }

    #[test]
    fn failure_counter_saturates() {
        let mut state = PluginState {
            failures: u32::MAX,
        };
        assert_eq!(
            next_action(&mut state, &cfg(), &poll(transient())),
            wait(3600, false)
        );
        assert_eq!(state.failures, u32::MAX);
    }

    #[test]
    fn failure_waits_are_never_zero() {
        let outcomes = [
            poll(transient()),
            poll(PollError::RateLimited(None)),
            poll(PollError::RateLimited(Some(0))),
            poll(PollError::AuthRequired),
            CycleOutcome::InstanceFatal("trap".to_string()),
            CycleOutcome::BuildFailed("nope".to_string()),
            CycleOutcome::SinkFailed("down".to_string()),
        ];
        for outcome in &outcomes {
            let mut state = PluginState::default();
            match next_action(&mut state, &cfg(), outcome) {
                Action::Wait { wait, .. } => {
                    assert!(wait >= secs(60), "{outcome:?} waited {wait:?}");
                }
                other @ Action::Disable { .. } => panic!("{outcome:?} gave {other:?}"),
            }
        }
    }

    #[test]
    fn describe_cleans_control_characters() {
        let text = describe(&poll(PollError::Transient("bad\u{1b}[31m\nline".to_string())));
        assert!(text.contains("bad"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text:?}");
        assert!(!text.contains('\n'), "{text:?}");
    }
}
