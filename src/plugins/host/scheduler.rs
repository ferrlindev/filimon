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

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::backoff;
use super::cursor::{CursorStore, MAX_CURSOR_BYTES, PollStatus};
use super::engine::{PollError, PollResponse, clean_guest_text};
use super::instance::{CallOutcome, InstanceFactory, PluginInstance};
use super::map::map_items;
use super::sink::{ItemSink, SinkRecord};
use crate::plugins::auth::provider::{TokenOutcome, TokenProvider};
use crate::plugins::auth::store::AccessToken;
use crate::plugins::config::{HostConfig, LoadedSource};
use crate::plugins::secrets::ResolvedSecrets;

/// Longest error text kept in a report or log line, in characters.
const REPORT_TEXT_CLIP: usize = 512;

/// Everything one plugin task needs. `ResolvedSecrets` redacts its values in
/// Debug, and so does `TokenProvider`; there is no hand-written Debug here.
#[derive(Debug)]
pub struct PluginSpec {
    pub name: String,
    pub host: HostConfig,
    pub settings_toml: String,
    pub secrets: ResolvedSecrets,
    /// The plugin's login, when its TOML declares `[host.oauth]`.
    pub auth: Option<Arc<TokenProvider>>,
}

impl PluginSpec {
    /// `Some` only for an enabled source whose plugin file loaded. A plugin
    /// with `[host.oauth]` gets a token provider over `<state_dir>/auth`.
    #[must_use]
    pub fn from_loaded(source: &LoadedSource, state_dir: &Path) -> Option<Self> {
        if !source.enabled {
            return None;
        }
        let plugin = source.plugin.as_ref()?;
        let auth = plugin
            .oauth
            .as_ref()
            .map(|oauth| Arc::new(TokenProvider::new(&source.name, oauth.clone(), state_dir)));
        Some(Self {
            name: source.name.clone(),
            host: plugin.host.clone(),
            settings_toml: plugin.settings_toml.clone(),
            secrets: plugin.secrets.clone(),
            auth,
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
    /// The access token could not be refreshed for now (cleaned text): a
    /// host-side, transient failure. The instance is kept.
    TokenRefresh(String),
    /// The plugin's token file cannot be used (cleaned text). It disables only
    /// this plugin and the file is left as it was found (D-02).
    TokenFile(String),
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
/// - Phase 6: `TokenRefresh` backs off like `SinkFailed` and keeps the
///   instance; `TokenFile` disables the plugin (D-02).
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
        CycleOutcome::SinkFailed(_) | CycleOutcome::TokenRefresh(_) => (step, false),
        CycleOutcome::TokenFile(_) => {
            return Action::Disable {
                reason: describe(outcome),
            };
        }
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
        CycleOutcome::TokenRefresh(text) => format!("token refresh failed: {}", clean(text)),
        CycleOutcome::TokenFile(text) => format!("token file problem: {}", clean(text)),
    }
}

// ── The Loop ──────────────────────────────────────────────────────────────────

/// Logs the reason and records it as the plugin's `disabled` text.
fn disable(report: &mut PluginReport, plugin: &str, reason: String) {
    tracing::error!(plugin = %plugin, reason = %reason, "plugin disabled until fili restarts");
    report.disabled = Some(reason);
}

/// Records the outcome of one cycle in the plugin's `<alias>.status.json` so
/// `fili plugins list` can show it. `items` is the number of records the sink
/// accepted this cycle, when it accepted any.
///
/// A failed write is one warning and nothing else: it is not a plugin failure,
/// never changes the cycle's control flow and never touches a cursor file.
pub(crate) fn record_status(
    cursors: &CursorStore,
    plugin: &str,
    outcome: &CycleOutcome,
    items: Option<u64>,
) {
    let ok = matches!(outcome, CycleOutcome::PollOk);
    let error = (!ok).then(|| describe(outcome));
    record_status_text(cursors, plugin, ok, error, items);
}

/// The text-level form of `record_status`, for a cycle that has no
/// `CycleOutcome` (a cursor file that could not be read).
pub(crate) fn record_status_text(
    cursors: &CursorStore,
    plugin: &str,
    ok: bool,
    error: Option<String>,
    items: Option<u64>,
) {
    let status = PollStatus::now(plugin, ok, error, items);
    if let Err(e) = cursors.write_status(plugin, &status) {
        tracing::warn!(
            plugin = %plugin,
            "{}",
            clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP)
        );
    }
}

/// The only place a secret value is read: the `(alias, value)` pairs handed to
/// the guest's `init`. Shared by the scheduler loop and `run-once`, so there is
/// one exposure site.
pub(crate) fn expose_secrets(spec: &PluginSpec) -> Vec<(String, String)> {
    spec.secrets
        .iter()
        .map(|(alias, secret)| (alias.clone(), secret.expose().to_string()))
        .collect()
}

/// Why a poll goes out without a token, for the one-time login hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginHint {
    /// The plugin has an OAuth config but no login on disk.
    NotLoggedIn,
    /// The stored login is flagged: the user must log in again.
    NeedsLogin,
}

/// What a poll is handed: the access token, when there is one, and the reason
/// there is none, when that is worth telling the user about.
#[derive(Debug)]
pub(crate) struct PollAuth {
    pub(crate) token: Option<AccessToken>,
    pub(crate) hint: Option<LoginHint>,
}

/// The one decision table between a plugin's stored login and its next `poll`,
/// shared by the loop and `run-once`. Call it immediately before every poll:
/// it re-reads the token file each time (D-08). A plugin without a token
/// provider gets no token and no hint and never touches the auth directory.
///
/// An `Err` is the cycle's outcome: a refresh problem is transient, an unusable
/// token file disables the plugin (D-02).
pub(crate) async fn auth_for_poll(spec: &PluginSpec) -> Result<PollAuth, CycleOutcome> {
    let Some(provider) = &spec.auth else {
        return Ok(PollAuth {
            token: None,
            hint: None,
        });
    };
    match provider.access_token().await {
        TokenOutcome::Token(token) => Ok(PollAuth {
            token: Some(token),
            hint: None,
        }),
        TokenOutcome::NotLoggedIn => Ok(PollAuth {
            token: None,
            hint: Some(LoginHint::NotLoggedIn),
        }),
        TokenOutcome::NeedsLogin => Ok(PollAuth {
            token: None,
            hint: Some(LoginHint::NeedsLogin),
        }),
        TokenOutcome::RefreshDue => Err(CycleOutcome::TokenRefresh(
            "access token is due for a refresh".to_string(),
        )),
        TokenOutcome::FileProblem(error) => Err(CycleOutcome::TokenFile(error.to_string())),
    }
}

/// Logs the one line that tells the user how to log in. Only the plugin alias
/// and fixed text: never a token or a path.
pub(crate) fn warn_login_hint(plugin: &str, hint: LoginHint) {
    match hint {
        LoginHint::NotLoggedIn => tracing::warn!(
            plugin = %plugin,
            "not logged in; run: fili auth login {plugin}"
        ),
        LoginHint::NeedsLogin => tracing::warn!(
            plugin = %plugin,
            "the stored login needs to be renewed; run: fili auth login {plugin}"
        ),
    }
}

/// The plugin fault for a `next-cursor` over `MAX_CURSOR_BYTES`, as a poll
/// error. `None` when the cursor is absent or within the cap.
fn oversize_cursor_error(response: &PollResponse) -> Option<PollError> {
    let len = response.next_cursor.as_ref().map_or(0, Vec::len);
    (len > MAX_CURSOR_BYTES).then(|| {
        PollError::Fatal(format!(
            "next-cursor is {len} bytes, over the {MAX_CURSOR_BYTES} byte cap"
        ))
    })
}

/// An oversize cursor is a plugin fault: nothing from the poll is sent and the
/// cursor is never persisted. It is `fatal` because the same instance and
/// cursor reproduce it. The scheduler and `run-once` both classify through the
/// same rule.
pub(crate) fn oversize_cursor_outcome(response: &PollResponse) -> Option<CycleOutcome> {
    oversize_cursor_error(response).map(|error| CycleOutcome::Guest {
        phase: Phase::Poll,
        error,
    })
}

/// Sends one poll's records. Returns the number of records sent, or a host-side
/// failure text (never blamed on the plugin).
///
/// There is no timeout here: the sink bounds its own wait (`SINK_TIMEOUT` in
/// `FluvioSink`) so that a slow send cleans up after itself. Dropping the future
/// from outside would skip that cleanup.
pub(crate) async fn send_records<S: ItemSink>(
    sink: &S,
    plugin: &str,
    records: Vec<SinkRecord>,
) -> Result<u64, String> {
    if records.is_empty() {
        return Ok(0);
    }
    let count = records.len() as u64;
    match sink.send_batch(plugin, records).await {
        Ok(()) => Ok(count),
        Err(e) => Err(clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP)),
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
            // Leave the disable cause where `list` can show it (OQ-7). The bad
            // cursor file itself is never touched (D-08).
            record_status_text(&cursors, &spec.name, false, Some(reason.clone()), None);
            disable(&mut report, &spec.name, reason);
            return report;
        }
    };

    // The only place a secret value is read (shared with `run-once`).
    let secrets = expose_secrets(&spec);

    // Whether the last poll already printed the "run: fili auth login" hint, so
    // a plugin that stays logged out logs it once, not once per poll.
    let mut hinted = false;

    'instances: loop {
        // A fresh instance.
        let mut instance = match factory.instantiate(&spec.name).await {
            Ok(instance) => instance,
            Err(e) => {
                let outcome = CycleOutcome::BuildFailed(e.0);
                record_status(&cursors, &spec.name, &outcome, None);
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
            record_status(&cursors, &spec.name, &outcome, None);
            match settle(&spec, &mut state, &mut report, &shutdown, &outcome).await {
                Next::Stop => return report,
                Next::Rebuild | Next::Same => continue 'instances,
            }
        }

        // The poll loop on this instance.
        loop {
            // The token is decided immediately before every poll. Not raced
            // against shutdown: a refresh in flight must not be cut between the
            // provider's network answer and its file write.
            let auth = match auth_for_poll(&spec).await {
                Ok(auth) => auth,
                Err(outcome) => {
                    record_status(&cursors, &spec.name, &outcome, None);
                    match settle(&spec, &mut state, &mut report, &shutdown, &outcome).await {
                        Next::Stop => return report,
                        Next::Rebuild => continue 'instances,
                        Next::Same => continue,
                    }
                }
            };
            match auth.hint {
                Some(hint) => {
                    if !hinted {
                        warn_login_hint(&spec.name, hint);
                    }
                    hinted = true;
                }
                None => hinted = false,
            }

            let polled = tokio::select! {
                biased;
                () = shutdown.cancelled() => return report,
                polled = instance.poll(
                    cursor.as_deref(),
                    auth.token.as_ref().map(AccessToken::expose),
                ) => polled,
            };

            // An oversize cursor is a plugin fault (the rule `run-once` shares):
            // nothing from this poll is sent and the cursor is never persisted.
            let polled = match polled {
                CallOutcome::Ok(response) => match oversize_cursor_error(&response) {
                    Some(error) => CallOutcome::Guest(error),
                    None => CallOutcome::Ok(response),
                },
                other => other,
            };

            // Records the sink accepted this cycle, for the status file.
            let mut sent: Option<u64> = None;

            let outcome = match polled {
                CallOutcome::Ok(response) => {
                    report.polls += 1;
                    let mapped = map_items(&spec.name, response.items);
                    let dropped = mapped.dropped as u64;

                    // Not raced against shutdown: a stop must not split send
                    // from persist.
                    match send_records(&*sink, &spec.name, mapped.records).await {
                        Ok(count) => {
                            sent = Some(count);
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

            record_status(&cursors, &spec.name, &outcome, sent);
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
            CycleOutcome::TokenRefresh("later".to_string()),
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
    fn token_file_problem_disables_the_plugin() {
        let mut state = PluginState::default();
        let outcome = CycleOutcome::TokenFile("plugin `p`: token file `/x` is corrupt".to_string());
        match next_action(&mut state, &cfg(), &outcome) {
            Action::Disable { reason } => {
                assert!(reason.starts_with("token file problem:"), "{reason}");
                assert!(reason.contains("`/x`"), "{reason}");
            }
            other @ Action::Wait { .. } => panic!("expected Disable, got {other:?}"),
        }
    }

    #[test]
    fn describe_cleans_control_characters() {
        let text = describe(&poll(PollError::Transient("bad\u{1b}[31m\nline".to_string())));
        assert!(text.contains("bad"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text:?}");
        assert!(!text.contains('\n'), "{text:?}");
    }

    // ── Loop behaviour under paused time ──────────────────────────────────────

    use tempfile::TempDir;
    use tokio::time::Instant;

    use crate::plugins::host::testing::{
        FakeEvent, FakeFactory, FakeStep, FlakySink, LogCapture, run_for, spec, wit_item,
    };
    use crate::plugins::host::sink::VecSink;

    fn store() -> (TempDir, Arc<CursorStore>) {
        let dir = TempDir::new().expect("tempdir");
        let cursors = Arc::new(CursorStore::open(dir.path()).expect("cursor store"));
        (dir, cursors)
    }

    fn err_transient() -> FakeStep {
        FakeStep::PollErr(PollError::Transient("later".to_string()))
    }

    /// `(alias, failing step, expected log substring)`.
    type LogCase = (&'static str, fn() -> FakeStep, &'static str);

    /// Interval 300 s, backoff 60 s / 3600 s / x2.
    fn default_spec(name: &str) -> PluginSpec {
        spec(name, 300, 60, 3600, 2.0)
    }

    #[tokio::test(start_paused = true)]
    async fn sched_transient_backs_off_then_returns_to_interval() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            err_transient(),
            err_transient(),
            FakeStep::poll_ok(None),
            FakeStep::poll_ok(None),
        ]));
        let start = Instant::now();
        let report = run_for(
            default_spec("t"),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(500),
        )
        .await;
        assert_eq!(factory.poll_offsets(start), vec![0, 60, 180, 480]);
        assert_eq!(report.failures, 2);
        assert_eq!(report.polls, 2);
        assert_eq!(report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_rate_limited_waits_the_larger_of_retry_after_and_step() {
        for (retry_after, duration, expected) in [(200, 250, 200), (100_000, 3700, 3600)] {
            let (_dir, cursors) = store();
            let factory = Arc::new(FakeFactory::new(vec![
                FakeStep::InitOk,
                FakeStep::PollErr(PollError::RateLimited(Some(retry_after))),
                FakeStep::poll_ok(None),
            ]));
            let start = Instant::now();
            run_for(
                default_spec("rl"),
                Arc::clone(&factory),
                Arc::new(VecSink::default()),
                cursors,
                secs(duration),
            )
            .await;
            assert_eq!(factory.poll_offsets(start), vec![0, expected]);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sched_success_resets_backoff_fully() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            err_transient(),
            FakeStep::poll_ok(None),
            err_transient(),
            FakeStep::poll_ok(None),
        ]));
        let start = Instant::now();
        run_for(
            default_spec("r"),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(450),
        )
        .await;
        assert_eq!(factory.poll_offsets(start), vec![0, 60, 360, 420]);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_auth_required_is_backed_off_not_disabled() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollErr(PollError::AuthRequired),
            FakeStep::poll_ok(None),
        ]));
        let start = Instant::now();
        let report = run_for(
            default_spec("a"),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(100),
        )
        .await;
        assert_eq!(factory.poll_offsets(start), vec![0, 60]);
        assert_eq!(report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_trap_rebuilds_instance_and_reinits_after_backoff() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::poll_ok(Some(vec![1])),
            FakeStep::Trap("wasm trap: unreachable".to_string()),
            FakeStep::InitOk,
            FakeStep::poll_ok(None),
        ]));
        let start = Instant::now();
        let report = run_for(
            spec("t", 60, 10, 3600, 2.0),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(100),
        )
        .await;
        assert_eq!(factory.builds(), 2);
        assert_eq!(factory.inits(), 2);
        assert_eq!(factory.poll_offsets(start), vec![0, 60, 70]);
        assert_eq!(factory.poll_cursors()[2], Some(vec![1]));
        assert_eq!(report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_init_transient_rebuilds_after_backoff() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitErr(PollError::Transient("not ready".to_string())),
            FakeStep::InitOk,
            FakeStep::poll_ok(None),
        ]));
        let start = Instant::now();
        let report = run_for(
            default_spec("i"),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(100),
        )
        .await;
        assert_eq!(factory.builds(), 2);
        assert_eq!(factory.inits(), 2);
        assert_eq!(factory.poll_offsets(start), vec![60]);
        assert_eq!(report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_config_error_in_init_disables() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitErr(PollError::Config(
            "bad setting".to_string(),
        ))]));
        let report = run_for(
            spec("c", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(100),
        )
        .await;
        let reason = report.disabled.expect("disabled");
        assert!(reason.contains("bad setting"), "{reason}");
        assert_eq!(report.polls, 0);
        assert_eq!(factory.builds(), 1);
        assert!(factory.poll_cursors().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn sched_fatal_poll_disables_only_that_plugin() {
        let (_dir, cursors) = store();
        let sink = Arc::new(VecSink::default());
        let bad = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollErr(PollError::Fatal("dead".to_string())),
        ]));
        let good = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let start = Instant::now();
        let (bad_report, good_report) = tokio::join!(
            run_for(
                spec("bad", 60, 60, 3600, 2.0),
                Arc::clone(&bad),
                Arc::clone(&sink),
                Arc::clone(&cursors),
                secs(200),
            ),
            run_for(
                spec("good", 60, 60, 3600, 2.0),
                Arc::clone(&good),
                Arc::clone(&sink),
                Arc::clone(&cursors),
                secs(200),
            ),
        );
        assert!(bad_report.disabled.is_some());
        assert_eq!(bad.poll_offsets(start), vec![0]);
        assert_eq!(good.poll_offsets(start), vec![0, 60, 120, 180]);
        assert_eq!(good_report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_oversize_next_cursor_is_a_plugin_fault_and_never_persisted() {
        let (_dir, cursors) = store();
        let sink = Arc::new(VecSink::default());
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![7; MAX_CURSOR_BYTES + 1]),
                delay: Duration::ZERO,
            },
        ]));
        let report = run_for(
            default_spec("big"),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;

        let reason = report.disabled.as_deref().expect("disabled");
        assert!(reason.contains("next-cursor"), "{reason}");
        assert!(reason.contains("4096"), "{reason}");
        assert_eq!(report.items_sent, 0);
        assert!(sink.records().is_empty());
        assert_eq!(cursors.read("big").expect("read"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_next_cursor_exactly_at_the_cap_is_persisted() {
        let (_dir, cursors) = store();
        let sink = Arc::new(VecSink::default());
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![7; MAX_CURSOR_BYTES]),
                delay: Duration::ZERO,
            },
        ]));
        let report = run_for(
            default_spec("ok"),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;

        assert_eq!(report.disabled, None);
        assert_eq!(report.items_sent, 1);
        assert_eq!(
            cursors.read("ok").expect("read"),
            Some(vec![7; MAX_CURSOR_BYTES])
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sched_poll_over_the_item_cap_sends_the_cap_and_counts_the_rest_dropped() {
        let (_dir, cursors) = store();
        let sink = Arc::new(VecSink::default());
        let items: Vec<_> = (0..super::super::map::MAX_ITEMS_PER_POLL + 3)
            .map(|n| wit_item(&format!("i{n}")))
            .collect();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items,
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
        ]));
        let report = run_for(
            default_spec("many"),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;

        assert_eq!(report.items_sent, super::super::map::MAX_ITEMS_PER_POLL as u64);
        assert_eq!(report.dropped, 3);
        assert_eq!(report.disabled, None);
        assert_eq!(cursors.read("many").expect("read"), Some(vec![1]));
    }

    #[tokio::test(start_paused = true)]
    async fn sched_two_plugins_back_off_independently() {
        let (_dir, cursors) = store();
        let sink = Arc::new(VecSink::default());
        let mut script = vec![FakeStep::InitOk];
        script.extend((0..10).map(|_| err_transient()));
        let a = Arc::new(FakeFactory::new(script));
        let b = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let start = Instant::now();
        let _ = tokio::join!(
            run_for(
                spec("a", 60, 60, 3600, 2.0),
                Arc::clone(&a),
                Arc::clone(&sink),
                Arc::clone(&cursors),
                secs(450),
            ),
            run_for(
                spec("b", 60, 60, 3600, 2.0),
                Arc::clone(&b),
                Arc::clone(&sink),
                Arc::clone(&cursors),
                secs(450),
            ),
        );
        assert_eq!(a.poll_offsets(start), vec![0, 60, 180, 420]);
        assert_eq!(
            b.poll_offsets(start),
            vec![0, 60, 120, 180, 240, 300, 360, 420]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sched_sink_failure_backs_off_and_repolls_same_cursor() {
        let (logs, _guard) = LogCapture::start();
        let (_dir, cursors) = store();
        let sink = Arc::new(FlakySink::new(1));
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
            FakeStep::PollOk {
                items: vec![wit_item("b")],
                next_cursor: Some(vec![2]),
                delay: Duration::ZERO,
            },
        ]));
        let start = Instant::now();
        let report = run_for(
            default_spec("s"),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;
        assert_eq!(factory.poll_offsets(start), vec![0, 60]);
        assert_eq!(factory.poll_cursors(), vec![None, None]);
        assert_eq!(sink.calls(), 2);
        assert_eq!(sink.records().len(), 1);
        assert_eq!(report.failures, 1);
        assert_eq!(report.items_sent, 1);
        assert_eq!(cursors.read("s").expect("read"), Some(vec![2]));
        assert!(logs.text().contains("item sink failed"), "{}", logs.text());

        // A sink that never recovers never moves the persisted cursor.
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
        ]));
        run_for(
            default_spec("s2"),
            Arc::clone(&factory),
            Arc::new(FlakySink::new(usize::MAX)),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;
        assert_eq!(cursors.read("s2").expect("read"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_polls_are_fixed_delay_and_never_overlap() {
        let (_dir, cursors) = store();
        let slow = || FakeStep::PollOk {
            items: vec![],
            next_cursor: None,
            delay: secs(10),
        };
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            slow(),
            slow(),
            slow(),
        ]));
        let start = Instant::now();
        run_for(
            spec("f", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(155),
        )
        .await;
        assert_eq!(factory.poll_offsets(start), vec![0, 70, 140]);

        let polls: Vec<(Instant, Instant)> = factory
            .events()
            .into_iter()
            .filter_map(|e| match e {
                FakeEvent::Poll { started, ended, .. } => Some((started, ended)),
                _ => None,
            })
            .collect();
        for pair in polls.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "polls overlapped");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sched_shutdown_during_backoff_returns_promptly() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk, err_transient()]));
        let token = CancellationToken::new();
        let start = Instant::now();
        let handle = tokio::spawn(run_plugin(
            default_spec("p"),
            factory,
            Arc::new(VecSink::default()),
            cursors,
            token.clone(),
        ));
        tokio::time::sleep(secs(5)).await;
        token.cancel();
        let report = handle.await.expect("plugin task");
        assert!(start.elapsed() < secs(10), "waited out the backoff");
        assert_eq!(report.disabled, None);
        assert_eq!(report.failures, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_logs_name_the_plugin_on_backoff_trap_and_disable() {
        let (logs, _guard) = LogCapture::start();
        let cases: [LogCase; 3] = [
            (
                "lt",
                || FakeStep::PollErr(PollError::Transient("flaky\u{1b}[31m text".to_string())),
                "backing off",
            ),
            (
                "lp",
                || FakeStep::Trap("trap\u{1b}[2J message".to_string()),
                "backing off",
            ),
            (
                "lf",
                || FakeStep::PollErr(PollError::Fatal("dead\u{1b}[0m".to_string())),
                "disabled until fili restarts",
            ),
        ];
        for (alias, make_step, _) in &cases {
            let (_dir, cursors) = store();
            run_for(
                default_spec(alias),
                Arc::new(FakeFactory::new(vec![FakeStep::InitOk, make_step()])),
                Arc::new(VecSink::default()),
                cursors,
                secs(10),
            )
            .await;
        }

        let text = logs.text();
        assert!(!text.contains('\u{1b}'), "{text:?}");
        for (alias, _, needle) in &cases {
            let line = text
                .lines()
                .find(|l| l.contains(&format!("plugin={alias}")) && l.contains(needle))
                .unwrap_or_else(|| panic!("no `{needle}` line for {alias} in:\n{text}"));
            assert!(
                line.contains("fili::plugins::host::scheduler"),
                "wrong target: {line}"
            );
        }
    }

    // ── Cursor policies (D-07, D-08, A6, empty and ordering edges) ────────────

    use crate::plugins::host::sink::SinkError;

    /// A `VecSink` that records what the plugin's cursor file held at the
    /// moment each batch arrived, to prove commit-after-flush.
    struct InspectingSink {
        inner: VecSink,
        cursors: Arc<CursorStore>,
        seen: std::sync::Mutex<Vec<Option<Vec<u8>>>>,
    }

    impl InspectingSink {
        fn new(cursors: Arc<CursorStore>) -> Self {
            Self {
                inner: VecSink::default(),
                cursors,
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<Option<Vec<u8>>> {
            self.seen.lock().expect("seen").clone()
        }
    }

    impl ItemSink for InspectingSink {
        async fn send_batch(
            &self,
            plugin: &str,
            records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            let on_disk = self.cursors.read(plugin).expect("cursor file readable");
            self.seen.lock().expect("seen").push(on_disk);
            self.inner.send_batch(plugin, records).await
        }
    }

    /// A poll that returns one item and the given next cursor.
    fn poll_item(id: &str, next_cursor: Option<Vec<u8>>) -> FakeStep {
        FakeStep::PollOk {
            items: vec![wit_item(id)],
            next_cursor,
            delay: Duration::ZERO,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sched_commit_happens_only_after_the_sink_returned() {
        let (_dir, cursors) = store();
        let sink = Arc::new(InspectingSink::new(Arc::clone(&cursors)));
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_item("a", Some(vec![1])),
            poll_item("b", Some(vec![2])),
        ]));
        let report = run_for(
            spec("c", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;
        assert_eq!(report.items_sent, 2);
        // Inside batch 1 the file is absent; inside batch 2 it holds batch 1's
        // cursor, never the cursor of the batch being sent.
        assert_eq!(sink.seen(), vec![None, Some(vec![1])]);
        assert_eq!(cursors.read("c").expect("read"), Some(vec![2]));
    }

    #[tokio::test(start_paused = true)]
    async fn sched_persistent_sink_failure_never_commits_a_cursor() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_item("a", Some(vec![1])),
            poll_item("b", Some(vec![2])),
            poll_item("c", Some(vec![3])),
        ]));
        let sink = Arc::new(FlakySink::new(usize::MAX));
        let report = run_for(
            default_spec("pf"),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(200),
        )
        .await;
        assert!(report.failures >= 3, "failures: {}", report.failures);
        assert_eq!(report.disabled, None);
        assert_eq!(sink.calls(), 3);
        assert!(
            !cursors.path_for("pf").exists(),
            "no cursor may be committed for undelivered items"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sched_persist_failure_keeps_in_memory_cursor_and_continues() {
        let (logs, _guard) = LogCapture::start();
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let cursors = Arc::new(CursorStore::open(&state).expect("cursor store"));
        std::fs::remove_dir_all(&state).expect("remove the state directory");
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::poll_ok(Some(vec![1])),
            FakeStep::poll_ok(Some(vec![2])),
        ]));
        let report = run_for(
            spec("pw", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;
        assert_eq!(report.disabled, None);
        assert_eq!(report.polls, 2);
        assert_eq!(
            factory.poll_cursors(),
            vec![None, Some(vec![1])],
            "the in-memory cursor survives a failed write"
        );
        let text = logs.text();
        let path = cursors.path_for("pw").display().to_string();
        let line = text
            .lines()
            .find(|l| l.contains("plugin=pw") && l.contains(&path))
            .unwrap_or_else(|| panic!("no error line naming plugin and {path} in:\n{text}"));
        assert!(line.contains("ERROR"), "{line}");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_corrupt_cursor_file_disables_only_that_plugin_and_never_resets_it() {
        let (logs, _guard) = LogCapture::start();
        let (_dir, cursors) = store();
        let garbage: &[u8] = b"{not json \xff\xfe";
        let path = cursors.path_for("a");
        std::fs::write(&path, garbage).expect("write garbage");
        let sink = Arc::new(VecSink::default());
        let bad = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let good = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let start = Instant::now();
        let (bad_report, good_report) = tokio::join!(
            run_for(
                default_spec("a"),
                Arc::clone(&bad),
                Arc::clone(&sink),
                Arc::clone(&cursors),
                secs(200),
            ),
            run_for(
                spec("b", 60, 60, 3600, 2.0),
                Arc::clone(&good),
                Arc::clone(&sink),
                Arc::clone(&cursors),
                secs(200),
            ),
        );

        let reason = bad_report.disabled.expect("a is disabled");
        assert!(reason.contains("a.json"), "{reason}");
        assert_eq!(bad_report.polls, 0);
        assert!(bad.events().is_empty(), "a was never instantiated");
        assert_eq!(bad.builds(), 0);
        assert_eq!(std::fs::read(&path).expect("file"), garbage, "bytes changed");

        assert_eq!(good_report.disabled, None);
        assert_eq!(good.poll_offsets(start), vec![0, 60, 120, 180]);

        let text = logs.text();
        let shown = path.display().to_string();
        assert!(
            text.lines()
                .any(|l| l.contains("plugin=a") && l.contains(&shown)),
            "no log line naming plugin a and {shown} in:\n{text}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sched_unreadable_cursor_disables_plugin() {
        let (_dir, cursors) = store();
        let path = cursors.path_for("a");
        std::fs::create_dir(&path).expect("directory named a.json");
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let report = run_for(
            default_spec("a"),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;
        let reason = report.disabled.expect("disabled");
        assert!(reason.contains(&path.display().to_string()), "{reason}");
        assert!(reason.contains("cannot read"), "{reason}");
        assert!(factory.events().is_empty(), "never instantiated");
        assert!(path.is_dir(), "the path is left as it was");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_empty_items_still_persist_the_cursor() {
        let (_dir, cursors) = store();
        let sink = Arc::new(FlakySink::new(0));
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::poll_ok(Some(vec![9])),
        ]));
        let report = run_for(
            default_spec("e"),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(10),
        )
        .await;
        assert_eq!(report.polls, 1);
        assert_eq!(sink.calls(), 0, "an empty list makes no sink call");
        assert_eq!(cursors.read("e").expect("read"), Some(vec![9]));
    }

    #[tokio::test(start_paused = true)]
    async fn sched_null_next_cursor_persists_null_and_next_poll_is_a_first_poll() {
        let (_dir, cursors) = store();
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::poll_ok(Some(vec![5])),
            FakeStep::poll_ok(None),
            FakeStep::poll_ok(None),
        ]));
        run_for(
            spec("n", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(150),
        )
        .await;
        assert_eq!(
            factory.poll_cursors(),
            vec![None, Some(vec![5]), None],
            "a null next cursor makes the next poll a first poll"
        );
        let text = std::fs::read_to_string(cursors.path_for("n")).expect("file");
        assert!(text.contains(r#""cursor":null"#), "{text}");

        // A restart reads that file as a first poll too.
        let again = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        run_for(
            default_spec("n"),
            Arc::clone(&again),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(10),
        )
        .await;
        assert_eq!(again.poll_cursors(), vec![None]);
    }

    #[tokio::test(start_paused = true)]
    async fn sched_equal_cursors_are_committed_in_poll_order() {
        let (_dir, cursors) = store();
        let sink = Arc::new(InspectingSink::new(Arc::clone(&cursors)));
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_item("a", Some(vec![1])),
            poll_item("b", Some(vec![1])),
            poll_item("c", Some(vec![2])),
        ]));
        run_for(
            spec("q", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(150),
        )
        .await;
        assert_eq!(
            sink.seen(),
            vec![None, Some(vec![1]), Some(vec![1])],
            "each batch sees only the previously committed cursor"
        );
        assert_eq!(
            factory.poll_cursors(),
            vec![None, Some(vec![1]), Some(vec![1])]
        );
        assert_eq!(cursors.read("q").expect("read"), Some(vec![2]));
    }

    // ── Status records (OQ-6, OQ-7) ───────────────────────────────────────────

    use crate::plugins::host::cursor::status_path;
    use crate::plugins::host::instance::InstanceBuildError;
    use crate::plugins::host::testing::FakeInstance;

    /// `<alias>.status.json` parsed, never asserting on `at`.
    fn status_json(dir: &TempDir, alias: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(status_path(dir.path(), alias)).expect("status file");
        serde_json::from_str(&text).expect("status json")
    }

    /// Runs one scripted plugin for `duration` seconds on the default spec.
    async fn run_scripted(
        alias: &str,
        steps: Vec<FakeStep>,
        cursors: &Arc<CursorStore>,
        duration: u64,
    ) -> PluginReport {
        run_for(
            default_spec(alias),
            Arc::new(FakeFactory::new(steps)),
            Arc::new(VecSink::default()),
            Arc::clone(cursors),
            secs(duration),
        )
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_ok_with_the_item_count() {
        let (dir, cursors) = store();
        let steps = vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a"), wit_item("b")],
                next_cursor: None,
                delay: Duration::ZERO,
            },
        ];
        run_scripted("ok", steps, &cursors, 10).await;
        let status = status_json(&dir, "ok");
        assert_eq!(status["plugin"], "ok");
        assert_eq!(status["ok"], true);
        assert_eq!(status["items"], 2);
        assert!(status["error"].is_null(), "{status}");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_a_guest_error_and_leaves_the_cursor_file_untouched() {
        let (dir, cursors) = store();
        run_scripted("g", vec![FakeStep::InitOk, err_transient()], &cursors, 10).await;
        let status = status_json(&dir, "g");
        assert_eq!(status["ok"], false);
        assert!(status["items"].is_null(), "{status}");
        let error = status["error"].as_str().expect("error text");
        assert!(error.starts_with("poll transient:"), "{error}");
        assert!(
            !cursors.path_for("g").exists(),
            "a run that only failed must not create a cursor file"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_a_trap() {
        let (dir, cursors) = store();
        let steps = vec![FakeStep::InitOk, FakeStep::Trap("wasm trap".to_string())];
        run_scripted("t", steps, &cursors, 10).await;
        let status = status_json(&dir, "t");
        assert_eq!(status["ok"], false);
        let error = status["error"].as_str().expect("error text");
        assert!(error.starts_with("instance fault:"), "{error}");
    }

    /// Accepts the first batch, then fails every later one, noting what the
    /// plugin's cursor file held at the first failure.
    struct FailAfterFirst {
        inner: VecSink,
        cursors: Arc<CursorStore>,
        calls: std::sync::atomic::AtomicUsize,
        bytes_at_failure: std::sync::Mutex<Option<Vec<u8>>>,
    }

    impl ItemSink for FailAfterFirst {
        async fn send_batch(
            &self,
            plugin: &str,
            records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if call == 0 {
                return self.inner.send_batch(plugin, records).await;
            }
            let bytes = std::fs::read(self.cursors.path_for(plugin)).expect("cursor bytes");
            *self.bytes_at_failure.lock().expect("bytes") = Some(bytes);
            Err(SinkError("broker unavailable".to_string()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_a_sink_failure_and_the_cursor_file_stays_byte_identical() {
        let (dir, cursors) = store();
        let sink = Arc::new(FailAfterFirst {
            inner: VecSink::default(),
            cursors: Arc::clone(&cursors),
            calls: std::sync::atomic::AtomicUsize::new(0),
            bytes_at_failure: std::sync::Mutex::new(None),
        });
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_item("a", Some(vec![1])),
            poll_item("b", Some(vec![2])),
        ]));
        run_for(
            default_spec("sf"),
            factory,
            Arc::clone(&sink),
            Arc::clone(&cursors),
            secs(310),
        )
        .await;

        let status = status_json(&dir, "sf");
        assert_eq!(status["ok"], false);
        let error = status["error"].as_str().expect("error text");
        assert!(error.starts_with("item sink failed:"), "{error}");

        let before = sink
            .bytes_at_failure
            .lock()
            .expect("bytes")
            .clone()
            .expect("the sink failed once");
        assert_eq!(
            std::fs::read(cursors.path_for("sf")).expect("cursor bytes"),
            before,
            "a status write changed the cursor file"
        );
        assert_eq!(cursors.read("sf").expect("read"), Some(vec![1]));
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_the_disable_reason() {
        let (dir, cursors) = store();
        let steps = vec![
            FakeStep::InitOk,
            FakeStep::PollErr(PollError::Fatal("dead".to_string())),
        ];
        let report = run_scripted("d", steps, &cursors, 10).await;
        let reason = report.disabled.expect("disabled");
        let status = status_json(&dir, "d");
        assert_eq!(status["ok"], false);
        assert_eq!(status["error"], reason.as_str());
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_an_oversize_cursor() {
        let (dir, cursors) = store();
        let steps = vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![7; MAX_CURSOR_BYTES + 1]),
                delay: Duration::ZERO,
            },
        ];
        run_scripted("big", steps, &cursors, 10).await;
        let status = status_json(&dir, "big");
        assert_eq!(status["ok"], false);
        let error = status["error"].as_str().expect("error text");
        assert!(error.contains("4096 byte cap"), "{error}");
        assert!(!cursors.path_for("big").exists(), "cursor must not be written");
    }

    /// A factory whose every build fails.
    struct FailingFactory;

    impl InstanceFactory for FailingFactory {
        type Instance = FakeInstance;

        async fn instantiate(&self, _plugin: &str) -> Result<FakeInstance, InstanceBuildError> {
            Err(InstanceBuildError("no memory".to_string()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_a_build_failure() {
        let (dir, cursors) = store();
        run_for(
            default_spec("bf"),
            Arc::new(FailingFactory),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(10),
        )
        .await;
        let status = status_json(&dir, "bf");
        assert_eq!(status["ok"], false);
        assert!(status["items"].is_null(), "{status}");
        let error = status["error"].as_str().expect("error text");
        assert!(error.starts_with("instance build failed:"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_an_init_failure() {
        let (dir, cursors) = store();
        let steps = vec![FakeStep::InitErr(PollError::Transient(
            "not ready".to_string(),
        ))];
        run_scripted("if", steps, &cursors, 10).await;
        let status = status_json(&dir, "if");
        assert_eq!(status["ok"], false);
        let error = status["error"].as_str().expect("error text");
        assert!(error.starts_with("init "), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_records_a_corrupt_cursor_disable_and_keeps_the_bad_file() {
        let (dir, cursors) = store();
        let garbage: &[u8] = b"{not json \xff\xfe";
        let path = cursors.path_for("a");
        std::fs::write(&path, garbage).expect("write garbage");
        let report = run_scripted("a", vec![FakeStep::InitOk], &cursors, 10).await;

        let reason = report.disabled.expect("a is disabled");
        let status = status_json(&dir, "a");
        assert_eq!(status["ok"], false);
        assert_eq!(status["error"], reason.as_str());
        assert!(reason.contains("a.json"), "{reason}");
        assert_eq!(std::fs::read(&path).expect("file"), garbage, "bytes changed");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_text_is_cleaned() {
        let (dir, cursors) = store();
        let steps = vec![
            FakeStep::InitOk,
            FakeStep::PollErr(PollError::Fatal("boom\u{1b}[31m red".to_string())),
        ];
        run_scripted("c", steps, &cursors, 10).await;
        let status = status_json(&dir, "c");
        let error = status["error"].as_str().expect("error text");
        assert!(error.contains("boom"), "{error}");
        assert!(!error.contains('\u{1b}'), "{error:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn sched_status_write_failure_does_not_fail_the_cycle() {
        let (logs, _guard) = LogCapture::start();
        let (dir, cursors) = store();
        // A directory where the status file belongs makes every status write fail.
        std::fs::create_dir(status_path(dir.path(), "w")).expect("directory named w.status.json");
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::poll_ok(Some(vec![1])),
            FakeStep::poll_ok(Some(vec![2])),
        ]));
        let report = run_for(
            spec("w", 60, 60, 3600, 2.0),
            factory,
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;

        assert_eq!(report.polls, 2);
        assert_eq!(report.failures, 0, "a status failure is not a plugin failure");
        assert_eq!(report.disabled, None);
        assert_eq!(cursors.read("w").expect("read"), Some(vec![2]));
        let text = logs.text();
        assert!(
            text.lines()
                .any(|l| l.contains("WARN") && l.contains("plugin=w")),
            "no warn line naming the plugin in:\n{text}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sched_cursor_round_trips_across_two_runs_with_fakes() {
        let (_dir, cursors) = store();
        let first = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::poll_ok(Some(vec![1])),
            FakeStep::poll_ok(Some(vec![2])),
        ]));
        run_for(
            spec("rt", 60, 60, 3600, 2.0),
            Arc::clone(&first),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(100),
        )
        .await;
        assert_eq!(first.poll_cursors(), vec![None, Some(vec![1])]);

        let second = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        run_for(
            spec("rt", 60, 60, 3600, 2.0),
            Arc::clone(&second),
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            secs(10),
        )
        .await;
        assert_eq!(
            second.poll_cursors(),
            vec![Some(vec![2])],
            "a restart resumes from the last committed cursor"
        );
    }
}

// ── Host-managed OAuth: the token reaches `poll` ─────────────────────────────

#[cfg(test)]
mod sched_auth {
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;
    use crate::plugins::auth::store::{AccessToken, StoredTokens, TokenStore};
    use crate::plugins::auth::testing::{provider_for, unix_now, write_tokens};
    use crate::plugins::host::cursor::status_path;
    use crate::plugins::host::sink::VecSink;
    use crate::plugins::host::testing::{
        FakeFactory, FakeStep, LogCapture, load_test_registry, run_for, spec,
    };

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// Writes `filimon.toml` and `stub.toml` (a source `stub` with
    /// `[host.oauth]`) into `dir`. The component file need not exist because
    /// loading never opens it.
    fn oauth_registry(dir: &Path) -> PathBuf {
        std::fs::write(
            dir.join("filimon.toml"),
            "[sources.stub]\ncomponent = \"stub.wasm\"\nconfig = \"stub.toml\"\n",
        )
        .expect("write registry");
        std::fs::write(
            dir.join("stub.toml"),
            "[host]\npoll_interval_secs = 60\n\n[host.oauth]\n\
             authorize_url = \"https://auth.example/authorize\"\n\
             token_url = \"https://auth.example/token\"\n\
             scopes = [\"read\"]\nclient_id = \"client-1\"\n",
        )
        .expect("write plugin toml");
        dir.join("filimon.toml")
    }

    /// A state directory under `dir` and a cursor store over it.
    fn state(dir: &TempDir) -> (PathBuf, Arc<CursorStore>) {
        let state_dir = dir.path().join("state");
        let cursors = Arc::new(CursorStore::open(&state_dir).expect("cursor store"));
        (state_dir, cursors)
    }

    /// A spec polling every 60 s with a provider over `state_dir`.
    fn oauth_spec(alias: &str, state_dir: &Path) -> PluginSpec {
        let mut spec = spec(alias, 60, 60, 3600, 2.0);
        spec.auth = Some(provider_for(state_dir, alias));
        spec
    }

    #[tokio::test(start_paused = true)]
    async fn tracer_a_logged_in_plugin_gets_its_access_token_from_toml_to_poll() {
        let dir = TempDir::new().expect("tempdir");
        let registry = load_test_registry(&oauth_registry(dir.path()));
        let (state_dir, cursors) = state(&dir);
        let source = &registry.sources["stub"];
        let spec = PluginSpec::from_loaded(source, &state_dir).expect("an enabled source");
        // The wall clock, which paused tokio time does not move.
        write_tokens(&state_dir, "stub", "AT-1", unix_now() + 3600);
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));

        let report = run_for(
            spec,
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(1),
        )
        .await;

        assert_eq!(factory.poll_tokens(), vec![Some("AT-1".to_string())]);
        assert_eq!(report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_token_file_replaced_between_polls_takes_effect() {
        let dir = TempDir::new().expect("tempdir");
        let (state_dir, cursors) = state(&dir);
        write_tokens(&state_dir, "swap", "AT-A", unix_now() + 3600);
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let shutdown = CancellationToken::new();
        let handle = tokio::spawn(run_plugin(
            oauth_spec("swap", &state_dir),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            shutdown.clone(),
        ));

        // Polls at 0, 60 and 120 s; the file is replaced at 30 s.
        tokio::time::sleep(secs(30)).await;
        write_tokens(&state_dir, "swap", "AT-B", unix_now() + 3600);
        tokio::time::sleep(secs(120)).await;
        shutdown.cancel();
        let report = handle.await.expect("plugin task");

        let tokens: Vec<Option<String>> = ["AT-A", "AT-B", "AT-B"]
            .iter()
            .map(|t| Some((*t).to_string()))
            .collect();
        assert_eq!(factory.poll_tokens(), tokens);
        assert_eq!(report.disabled, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_missing_token_file_polls_with_none_and_hints_once() {
        let (logs, _guard) = LogCapture::start();
        let dir = TempDir::new().expect("tempdir");
        let (state_dir, cursors) = state(&dir);
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));

        run_for(
            oauth_spec("nolog", &state_dir),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(150),
        )
        .await;

        assert_eq!(factory.poll_tokens(), vec![None, None, None]);
        let text = logs.text();
        let hints = text
            .lines()
            .filter(|line| line.contains("run: fili auth login"))
            .count();
        assert_eq!(hints, 1, "one hint per transition, not one per poll:\n{text}");
        assert!(text.contains("plugin=nolog"), "{text}");
        assert!(!state_dir.join("auth").exists(), "nothing was created");
    }

    #[tokio::test(start_paused = true)]
    async fn needs_login_polls_with_none_and_keeps_the_file() {
        let dir = TempDir::new().expect("tempdir");
        let (state_dir, cursors) = state(&dir);
        TokenStore::new(&state_dir, "flagged")
            .write(&StoredTokens {
                access_token: AccessToken::new("AT-OLD".to_string()),
                refresh_token: None,
                expires_at: unix_now() + 3600,
                token_type: None,
                scope: None,
                needs_login: true,
                obtained_at: unix_now(),
            })
            .expect("write the flagged file");
        let path = state_dir.join("auth").join("flagged.json");
        let before = std::fs::read(&path).expect("bytes");
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));

        let report = run_for(
            oauth_spec("flagged", &state_dir),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(100),
        )
        .await;

        assert_eq!(factory.poll_tokens(), vec![None, None]);
        assert_eq!(report.disabled, None);
        assert_eq!(std::fs::read(&path).expect("bytes"), before);
    }

    #[tokio::test(start_paused = true)]
    async fn a_corrupt_token_file_disables_only_that_plugin() {
        let dir = TempDir::new().expect("tempdir");
        let (state_dir, cursors) = state(&dir);
        let bad_path = state_dir.join("auth").join("bad.json");
        std::fs::create_dir_all(state_dir.join("auth")).expect("auth dir");
        std::fs::write(&bad_path, "NOT-JSON-SENTINEL").expect("write the corrupt file");
        let before = std::fs::read(&bad_path).expect("bytes");
        write_tokens(&state_dir, "good", "AT-good", unix_now() + 3600);
        let bad_factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));
        let good_factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));

        let (bad, good) = tokio::join!(
            run_for(
                oauth_spec("bad", &state_dir),
                Arc::clone(&bad_factory),
                Arc::new(VecSink::default()),
                Arc::clone(&cursors),
                secs(150),
            ),
            run_for(
                oauth_spec("good", &state_dir),
                Arc::clone(&good_factory),
                Arc::new(VecSink::default()),
                Arc::clone(&cursors),
                secs(150),
            ),
        );

        let reason = bad.disabled.expect("the bad plugin is disabled");
        assert!(reason.contains("`bad`"), "{reason}");
        assert!(reason.contains(&bad_path.display().to_string()), "{reason}");
        assert!(!reason.contains("NOT-JSON-SENTINEL"), "{reason}");
        assert!(bad_factory.poll_tokens().is_empty(), "never polled");
        assert_eq!(std::fs::read(&bad_path).expect("bytes"), before);
        let status = std::fs::read_to_string(status_path(&state_dir, "bad")).expect("status");
        assert!(status.contains("token file problem"), "{status}");

        assert_eq!(good.disabled, None);
        let good_tokens: Vec<Option<String>> = vec![Some("AT-good".to_string()); 3];
        assert_eq!(good_factory.poll_tokens(), good_tokens);
    }

    #[tokio::test(start_paused = true)]
    async fn a_plugin_without_oauth_polls_with_none() {
        let dir = TempDir::new().expect("tempdir");
        let (state_dir, cursors) = state(&dir);
        let factory = Arc::new(FakeFactory::new(vec![FakeStep::InitOk]));

        run_for(
            spec("plain", 60, 60, 3600, 2.0),
            Arc::clone(&factory),
            Arc::new(VecSink::default()),
            cursors,
            secs(100),
        )
        .await;

        assert_eq!(factory.poll_tokens(), vec![None, None]);
        assert!(!state_dir.join("auth").exists());
    }
}
