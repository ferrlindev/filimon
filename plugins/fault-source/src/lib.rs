// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  fault-source (filimon:plugin `source` world)
//
// Fault-injection source plugin: a TEST FIXTURE for the Phase 3 host, never a
// registry example. Its `[settings]` table selects how it misbehaves:
//
//   mode = "ok"            one item per poll, echoes the incoming cursor
//   mode = "rate-limited"  poll -> PollError::RateLimited(retry_after_secs)
//   mode = "transient"     poll -> PollError::Transient
//   mode = "auth-required" poll -> PollError::AuthRequired
//   mode = "fatal"         poll -> PollError::Fatal
//   mode = "config"        poll -> PollError::Config
//   mode = "init-fatal"    init -> PollError::Fatal
//   mode = "trap"          poll panics (a wasm `unreachable` trap)
//   mode = "spin"          poll never returns
//
// Optional settings: `retry_after_secs` (for `rate-limited`) and `after_polls`
// (default 0): a poll whose per-instance 1-based count is <= `after_polls`
// behaves as `ok`, so a test can express "ok once, then trap" and observe the
// counter restart on a fresh instance.
//
// Built for wasm32-unknown-unknown: one instance is one linear memory, so the
// thread-local state below persists across calls on one instance and restarts
// on a fresh one. Log lines carry only the mode name and counters, never
// settings text, secret values or cursor bytes.
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;

use std::cell::RefCell;

use bindings::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{Item, PollError, PollResponse};
use serde::Deserialize;

// ── Settings ─────────────────────────────────────────────────────────────────

/// The fixture's `[settings]` table, passed raw to `init`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// One of the nine mode names, see `Mode::parse`.
    mode: String,
    /// Retry-after seconds reported by `rate-limited`.
    retry_after_secs: Option<u32>,
    /// Polls (1-based count <= this value) that behave as `ok` before the
    /// configured mode takes effect. Defaults to 0.
    after_polls: Option<u32>,
}

// ── Modes ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ok,
    RateLimited,
    Transient,
    AuthRequired,
    Fatal,
    Config,
    InitFatal,
    Trap,
    Spin,
}

impl Mode {
    fn parse(name: &str) -> Option<Mode> {
        match name {
            "ok" => Some(Mode::Ok),
            "rate-limited" => Some(Mode::RateLimited),
            "transient" => Some(Mode::Transient),
            "auth-required" => Some(Mode::AuthRequired),
            "fatal" => Some(Mode::Fatal),
            "config" => Some(Mode::Config),
            "init-fatal" => Some(Mode::InitFatal),
            "trap" => Some(Mode::Trap),
            "spin" => Some(Mode::Spin),
            _ => None,
        }
    }
}

// ── Per-instance state ───────────────────────────────────────────────────────

struct State {
    mode: Mode,
    mode_name: String,
    retry_after_secs: Option<u32>,
    after_polls: u32,
    polls: u32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn init(settings: String, _secrets: Vec<(String, String)>) -> Result<(), PollError> {
        let settings: Settings =
            toml::from_str(&settings).map_err(|e| PollError::Config(e.to_string()))?;

        let Some(mode) = Mode::parse(&settings.mode) else {
            return Err(PollError::Config(format!(
                "fault-source: unknown mode {:?}",
                settings.mode
            )));
        };

        log(Level::Info, &format!("init: mode={}", settings.mode));

        if mode == Mode::InitFatal {
            return Err(PollError::Fatal("fault-source: init-fatal mode".to_string()));
        }

        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                mode,
                mode_name: settings.mode,
                retry_after_secs: settings.retry_after_secs,
                after_polls: settings.after_polls.unwrap_or(0),
                polls: 0,
            });
        });

        Ok(())
    }

    fn poll(
        cursor: Option<Vec<u8>>,
        _access_token: Option<String>,
    ) -> Result<PollResponse, PollError> {
        let (mode, mode_name, retry_after_secs, after_polls, n) = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return Err(PollError::Fatal(
                    "fault-source: poll before init".to_string(),
                ));
            };
            state.polls = state.polls.saturating_add(1);
            Ok((
                state.mode,
                state.mode_name.clone(),
                state.retry_after_secs,
                state.after_polls,
                state.polls,
            ))
        })?;

        if mode == Mode::Ok || n <= after_polls {
            let item = Item {
                id: format!("f{n}"),
                url: format!("https://example.invalid/fault/{n}"),
                title: Some("fault item".to_string()),
                text: "hello from fault-source".to_string(),
                author: Some("fault".to_string()),
                // 2026-01-01T00:00:00Z in Unix epoch milliseconds.
                published: Some(1_767_225_600_000),
                platform: "fault".to_string(),
                raw: Some(format!("{{\"n\":{n}}}")),
            };
            return Ok(PollResponse {
                items: vec![item],
                // Echo of the incoming cursor.
                next_cursor: Some(cursor.unwrap_or_else(|| vec![0])),
            });
        }

        log(
            Level::Warn,
            &format!("poll: mode={mode_name} n={n} failing"),
        );

        match mode {
            Mode::RateLimited => Err(PollError::RateLimited(retry_after_secs)),
            Mode::Transient => Err(PollError::Transient(
                "fault-source: transient mode".to_string(),
            )),
            Mode::AuthRequired => Err(PollError::AuthRequired),
            Mode::Fatal => Err(PollError::Fatal("fault-source: fatal mode".to_string())),
            Mode::Config => Err(PollError::Config("fault-source: config mode".to_string())),
            Mode::Trap => panic!("fault-source: trap mode"),
            Mode::Spin => {
                let mut counter: u64 = 0;
                loop {
                    counter = std::hint::black_box(counter.wrapping_add(1));
                }
            }
            // `ok` returned above; `init-fatal` never reaches `poll` (init
            // fails before state is stored).
            Mode::Ok | Mode::InitFatal => Err(PollError::Fatal(
                "fault-source: unreachable mode".to_string(),
            )),
        }
    }
}

bindings::export!(Component with_types_in bindings);
