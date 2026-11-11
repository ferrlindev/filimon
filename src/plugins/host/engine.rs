// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/engine.rs - wasmtime engine, bindings and guest imports
//
// One `HostEngine` owns the wasmtime `Engine`, the `Linker` holding only the
// `filimon:plugin/*` interfaces (no WASI is linked), and a 10 ms epoch ticker
// thread. Epoch interruption is an anti-wedge measure: every guest call yields
// to tokio about every tick, so a spinning guest cannot starve a worker and a
// shutdown can drop its future. It is not a CPU limit (HOST-06 is deferred).
// Every store also carries a small fixed memory ceiling (`store_limits`) so one
// guest cannot take the whole process down; configurable per-plugin limits stay
// deferred (HOST-06).
//
// Guest text is untrusted: every string that reaches `tracing` passes through
// `clean_guest_text` first.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use wasmtime::component::{HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

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

use super::fetch::FetchRoute;
use crate::output::sanitize_for_terminal;

/// How often the epoch ticker advances the engine epoch.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);

/// Fixed ceiling on one instance's linear memory, in bytes. A guest that tries
/// to grow past it traps, which the scheduler handles like any instance fault
/// (backoff, then a fresh instance). Not configurable until HOST-06.
pub const GUEST_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// Most core instances, tables and memories one component may create. A
/// component built by `cargo component` is a few core modules (the guest plus
/// adapter shims), so these are small but not 1.
const MAX_CORE_INSTANCES: usize = 16;
const MAX_TABLES: usize = 16;
const MAX_MEMORIES: usize = 8;
/// Most elements in one table.
const MAX_TABLE_ELEMENTS: usize = 100_000;

/// The fixed per-instance resource limits installed on every store.
#[must_use]
pub fn store_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(GUEST_MEMORY_LIMIT_BYTES)
        .table_elements(MAX_TABLE_ELEMENTS)
        .instances(MAX_CORE_INSTANCES)
        .tables(MAX_TABLES)
        .memories(MAX_MEMORIES)
        // A refused `memory.grow` traps instead of returning -1, so the breach
        // is an instance fault and never a guest that limps on or aborts the
        // host.
        .trap_on_grow_failure(true)
        .build()
}

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

/// Per-instance host state: the plugin alias used to tag every guest log line,
/// the instance's resource limits and, for a source, the route that answers
/// its `fetch` calls.
pub struct HostState {
    plugin: Arc<str>,
    limits: StoreLimits,
    route: Option<Arc<FetchRoute>>,
}

impl HostState {
    #[must_use]
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: Arc::from(plugin),
            limits: store_limits(),
            route: None,
        }
    }

    /// Sets the route that answers this instance's `fetch` calls. With none,
    /// every `fetch` answers `not-configured`.
    #[must_use]
    pub fn with_route(mut self, route: Option<Arc<FetchRoute>>) -> Self {
        self.route = route;
        self
    }

    /// The limits `Store::limiter` points at.
    pub fn limits_mut(&mut self) -> &mut dyn wasmtime::ResourceLimiter {
        &mut self.limits
    }
}

/// Builds the store every guest instance runs in, source or fetcher: the fixed
/// per-instance limits and the anti-wedge epoch yield. A limit breach traps,
/// which the caller treats as an instance fault.
#[must_use]
pub fn new_guest_store(engine: &Engine, state: HostState) -> Store<HostState> {
    let mut store = Store::new(engine, state);
    // Fixed per-instance memory and table caps.
    store.limiter(HostState::limits_mut);
    // Anti-wedge: yield to tokio each epoch tick instead of trapping.
    store.set_epoch_deadline(1);
    store.epoch_deadline_async_yield_and_update(1);
    store
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
    /// Answers a source's `fetch` through the fetcher the registry assigned
    /// to it, or `not-configured` when no fetcher is assigned.
    async fn fetch(
        &mut self,
        request: types::Request,
    ) -> wasmtime::Result<Result<types::Response, types::FetchError>> {
        // Clone the route out of the state so the await below does not hold a
        // borrow of the calling store.
        let Some(route) = self.route.clone() else {
            return Ok(Err(types::FetchError::NotConfigured));
        };
        Ok(route.fetch(request).await)
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
        assert!(!cleaned.chars().any(char::is_control));

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
    fn store_limits_cap_guest_memory_at_64_mib_and_trap_on_breach() {
        use wasmtime::ResourceLimiter;

        let mut limits = store_limits();
        // Growing up to the cap is allowed.
        assert!(
            limits
                .memory_growing(0, GUEST_MEMORY_LIMIT_BYTES, None)
                .expect("allowed")
        );
        // One byte over is a trap, not a quiet `false`, so the scheduler sees
        // an instance fault.
        let denied = limits.memory_growing(0, GUEST_MEMORY_LIMIT_BYTES + 1, None);
        assert!(denied.is_err(), "growth past the cap must trap: {denied:?}");
    }

    #[test]
    fn store_limits_allow_a_small_component_and_refuse_a_table_flood() {
        use wasmtime::ResourceLimiter;

        let mut limits = store_limits();
        assert!(limits.instances() >= 3, "adapter shims need a few instances");
        assert!(limits.table_growing(0, 1000, None).expect("allowed"));
        let denied = limits.table_growing(0, MAX_TABLE_ELEMENTS + 1, None);
        assert!(denied.is_err(), "{denied:?}");
    }

    #[test]
    fn host_state_carries_the_fixed_limits() {
        let mut state = HostState::new("p");
        let limits = state.limits_mut();
        assert!(
            wasmtime::ResourceLimiter::memory_growing(limits, 0, GUEST_MEMORY_LIMIT_BYTES + 1, None)
                .is_err()
        );
    }

    #[test]
    fn host_engine_builds_with_epoch_interruption() {
        let host_engine = HostEngine::new().expect("engine builds");
        // The engine accepts epoch calls only when interruption is configured;
        // incrementing it must not panic.
        host_engine.engine().increment_epoch();
    }
}
