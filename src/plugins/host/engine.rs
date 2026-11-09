// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/engine.rs - wasmtime engine, bindings and guest imports
//
// One `HostEngine` owns the wasmtime `Engine`, the `Linker` holding only the
// `filimon:plugin/*` interfaces (no WASI is linked), and a 10 ms epoch ticker
// thread. Epoch interruption is an anti-wedge measure: every guest call yields
// to tokio about every tick, so a spinning guest cannot starve a worker and a
// shutdown can drop its future. It is not a CPU limit (HOST-06 is deferred).
//
// Guest text is untrusted: every string that reaches `tracing` passes through
// `clean_guest_text` first.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use wasmtime::component::{HasSelf, Linker};
use wasmtime::{Config, Engine};

/// Generated bindings for the `source` world.
#[allow(clippy::all, clippy::pedantic)]
pub mod source_world {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "source",
        imports: { default: async | trappable },
        exports: { default: async },
    });
}

/// Generated bindings for the `fetcher` world. The `types` and `log`
/// interfaces are shared with the `source` world, so their host impls on
/// `HostState` are written once.
#[allow(clippy::all, clippy::pedantic)]
pub mod fetcher_world {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "fetcher",
        imports: { default: async | trappable },
        exports: { default: async },
        with: {
            "filimon:plugin/types": super::source_world::filimon::plugin::types,
            "filimon:plugin/log": super::source_world::filimon::plugin::log,
        },
    });
}

pub use fetcher_world::{Fetcher, FetcherPre};
pub use source_world::filimon::plugin::types::{Item as WitItem, PollError, PollResponse};
pub use source_world::{Source, SourcePre};

use source_world::filimon::plugin::{fetch, log, types};

use crate::output::sanitize_for_terminal;

/// How often the epoch ticker advances the engine epoch.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);

/// Longest guest log line passed to `tracing`, in characters.
const GUEST_LOG_CLIP: usize = 4096;

// ── Guest Text ────────────────────────────────────────────────────────────────

/// Makes guest-supplied text safe to print: terminal control characters are
/// removed (newline, carriage return and tab become a space) and the result is
/// clipped to `max_chars` characters, never mid-character, with a single `…`
/// appended when something was cut.
#[must_use]
pub fn clean_guest_text(text: &str, max_chars: usize) -> String {
    let cleaned = sanitize_for_terminal(text);
    match cleaned.char_indices().nth(max_chars) {
        None => cleaned,
        Some((cut, _)) => {
            let mut clipped = cleaned[..cut].to_string();
            clipped.push('…');
            clipped
        }
    }
}

// ── Host State And Guest Imports ──────────────────────────────────────────────

/// Per-instance host state: the plugin alias used to tag every guest log line.
pub struct HostState {
    plugin: Arc<str>,
}

impl HostState {
    #[must_use]
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: Arc::from(plugin),
        }
    }

    #[must_use]
    pub fn plugin(&self) -> &str {
        &self.plugin
    }
}

impl types::Host for HostState {}

impl log::Host for HostState {
    async fn log(&mut self, level: log::Level, message: String) -> wasmtime::Result<()> {
        let text = clean_guest_text(&message, GUEST_LOG_CLIP);
        let plugin = &*self.plugin;
        match level {
            log::Level::Trace => tracing::trace!(plugin = %plugin, "{text}"),
            log::Level::Debug => tracing::debug!(plugin = %plugin, "{text}"),
            log::Level::Info => tracing::info!(plugin = %plugin, "{text}"),
            log::Level::Warn => tracing::warn!(plugin = %plugin, "{text}"),
            log::Level::Error => tracing::error!(plugin = %plugin, "{text}"),
        }
        Ok(())
    }
}

impl fetch::Host for HostState {
    /// Interim answer until the Phase 5 fetcher layer exists.
    async fn fetch(
        &mut self,
        _request: types::Request,
    ) -> wasmtime::Result<Result<types::Response, types::FetchError>> {
        Ok(Err(types::FetchError::NotConfigured))
    }
}

impl fetcher_world::filimon::plugin::outbound_http::Host for HostState {
    /// Interim answer until the Phase 5 fetcher layer exists: no real HTTP.
    async fn send(
        &mut self,
        _request: types::Request,
    ) -> wasmtime::Result<Result<types::Response, types::FetchError>> {
        Ok(Err(types::FetchError::NotConfigured))
    }
}

// ── Epoch Ticker ──────────────────────────────────────────────────────────────

/// Advances the engine epoch every `EPOCH_TICK` on a dedicated OS thread. It
/// is not a tokio task, so tokio's paused clock never auto-advances in tick
/// sized hops. Dropping it stops and joins the thread.
struct EpochTicker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl EpochTicker {
    fn start(engine: Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("fili-epoch-ticker".to_string())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(EPOCH_TICK);
                    engine.increment_epoch();
                }
            })
            .ok();
        Self { stop, handle }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// The shared wasmtime engine, the per-world linkers and the epoch ticker.
pub struct HostEngine {
    engine: Engine,
    source_linker: Linker<HostState>,
    fetcher_linker: Linker<HostState>,
    _ticker: EpochTicker,
}

impl HostEngine {
    /// Builds the engine with epoch interruption on and a linker holding only
    /// the `filimon:plugin/*` imports. No WASI interface is linked.
    ///
    /// # Errors
    /// Returns the wasmtime error when the engine or the linker cannot be built.
    pub fn new() -> wasmtime::Result<Self> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        let engine = Engine::new(&config)?;

        let mut source_linker: Linker<HostState> = Linker::new(&engine);
        Source::add_to_linker::<HostState, HasSelf<HostState>>(&mut source_linker, |state| state)?;

        let mut fetcher_linker: Linker<HostState> = Linker::new(&engine);
        Fetcher::add_to_linker::<HostState, HasSelf<HostState>>(&mut fetcher_linker, |state| {
            state
        })?;

        let ticker = EpochTicker::start(engine.clone());
        Ok(Self {
            engine,
            source_linker,
            fetcher_linker,
            _ticker: ticker,
        })
    }

    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    #[must_use]
    pub fn source_linker(&self) -> &Linker<HostState> {
        &self.source_linker
    }

    #[must_use]
    pub fn fetcher_linker(&self) -> &Linker<HostState> {
        &self.fetcher_linker
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_guest_text_strips_control_chars_and_clips() {
        let cleaned = clean_guest_text("a\u{1b}[31mred\nline\ttab\r", 4096);
        assert_eq!(cleaned, "a[31mred line tab ");
        assert!(!cleaned.chars().any(|c| c.is_control()));

        let long = "x".repeat(5000);
        let clipped = clean_guest_text(&long, 4096);
        assert_eq!(clipped.chars().count(), 4097);
        assert!(clipped.ends_with('…'));

        // Clipping counts characters, never bytes: a multibyte string is not
        // split mid-character.
        let multibyte = "東".repeat(10);
        let clipped = clean_guest_text(&multibyte, 4);
        assert_eq!(clipped, "東東東東…");

        // Exactly at the limit is not clipped.
        assert_eq!(clean_guest_text("abcd", 4), "abcd");
    }

    #[test]
    fn host_engine_builds_with_epoch_interruption() {
        let host_engine = HostEngine::new().expect("engine builds");
        // The engine accepts epoch calls only when interruption is configured;
        // incrementing it must not panic.
        host_engine.engine().increment_epoch();
    }
}
