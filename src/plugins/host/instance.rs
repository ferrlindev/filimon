// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/instance.rs - Live plugin instances and call classification
//
// `PluginInstance` / `InstanceFactory` are the seam the scheduler drives, so
// scheduling behaviour can be tested without wasm. `classify` separates a
// guest-reported error (`Ok(Err(PollError))`, instance still usable) from any
// wasmtime error (`Err`, a trap or host failure: the instance is poisoned and
// must be discarded).
// ─────────────────────────────────────────────────────────────────────────────

use std::future::Future;
use std::sync::Arc;

use wasmtime::Store;

use super::engine::{
    HostState, PollError, PollResponse, Source, SourcePre, clean_guest_text, new_guest_store,
};
use super::fetch::FetchRoute;

/// Longest error text kept from a trap or build failure, in characters.
const ERROR_TEXT_CLIP: usize = 512;

/// The result of one guest call.
#[derive(Debug)]
pub enum CallOutcome<T> {
    Ok(T),
    /// The guest returned an error; the instance is still usable.
    Guest(PollError),
    /// A trap or host failure; the instance must be discarded. The text is
    /// already cleaned.
    InstanceFatal(String),
}

/// Sorts a guest call's result into a `CallOutcome`.
#[must_use]
pub fn classify<T>(result: wasmtime::Result<Result<T, PollError>>) -> CallOutcome<T> {
    match result {
        Ok(Ok(value)) => CallOutcome::Ok(value),
        Ok(Err(error)) => CallOutcome::Guest(error),
        Err(error) => {
            let text = error.downcast_ref::<wasmtime::Trap>().map_or_else(
                || {
                    let full = format!("{error:#}");
                    full.lines().next().unwrap_or_default().to_string()
                },
                |trap| format!("{trap:?}"),
            );
            CallOutcome::InstanceFatal(clean_guest_text(&text, ERROR_TEXT_CLIP))
        }
    }
}

/// A new instance could not be built. The text is cleaned.
#[derive(Debug)]
pub struct InstanceBuildError(pub String);

/// One live plugin instance. `init` runs once, before any `poll`; `poll` is
/// never called concurrently on one instance.
pub trait PluginInstance: Send + 'static {
    fn init(
        &mut self,
        settings: &str,
        secrets: &[(String, String)],
    ) -> impl Future<Output = CallOutcome<()>> + Send;

    fn poll(
        &mut self,
        cursor: Option<&[u8]>,
    ) -> impl Future<Output = CallOutcome<PollResponse>> + Send;
}

/// Builds fresh instances of one plugin.
pub trait InstanceFactory: Send + Sync + 'static {
    type Instance: PluginInstance;

    fn instantiate(
        &self,
        plugin: &str,
    ) -> impl Future<Output = Result<Self::Instance, InstanceBuildError>> + Send;
}

/// Builds wasmtime-backed instances from one linked component.
pub struct WasmtimeFactory {
    engine: wasmtime::Engine,
    pre: SourcePre<HostState>,
    route: Option<Arc<FetchRoute>>,
}

impl WasmtimeFactory {
    #[must_use]
    pub fn new(engine: wasmtime::Engine, pre: SourcePre<HostState>) -> Self {
        Self {
            engine,
            pre,
            route: None,
        }
    }

    /// Answers the `fetch` calls of every instance this factory builds through
    /// `route`. With `None` they answer `not-configured`.
    #[must_use]
    pub fn with_route(mut self, route: Option<Arc<FetchRoute>>) -> Self {
        self.route = route;
        self
    }
}

impl InstanceFactory for WasmtimeFactory {
    type Instance = WasmtimeInstance;

    async fn instantiate(&self, plugin: &str) -> Result<WasmtimeInstance, InstanceBuildError> {
        // A limit breach traps, which the scheduler handles as an instance
        // fault (backoff, fresh instance).
        let mut store = new_guest_store(
            &self.engine,
            HostState::new(plugin).with_route(self.route.clone()),
        );
        let source = self
            .pre
            .instantiate_async(&mut store)
            .await
            .map_err(|e| InstanceBuildError(clean_guest_text(&format!("{e:#}"), ERROR_TEXT_CLIP)))?;
        Ok(WasmtimeInstance { store, source })
    }
}

/// A live wasmtime instance of a source component.
pub struct WasmtimeInstance {
    store: Store<HostState>,
    source: Source,
}

impl PluginInstance for WasmtimeInstance {
    async fn init(&mut self, settings: &str, secrets: &[(String, String)]) -> CallOutcome<()> {
        classify(self.source.call_init(&mut self.store, settings, secrets).await)
    }

    async fn poll(&mut self, cursor: Option<&[u8]>) -> CallOutcome<PollResponse> {
        // The access token is always none until Phase 6.
        classify(self.source.call_poll(&mut self.store, cursor, None).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_maps_all_three_outcomes() {
        let ok: wasmtime::Result<Result<u32, PollError>> = Ok(Ok(7));
        assert!(matches!(classify(ok), CallOutcome::Ok(7)));

        let guest: wasmtime::Result<Result<u32, PollError>> =
            Ok(Err(PollError::Transient("later".to_string())));
        assert!(matches!(
            classify(guest),
            CallOutcome::Guest(PollError::Transient(_))
        ));

        let fatal: wasmtime::Result<Result<u32, PollError>> =
            Err(wasmtime::format_err!("host exploded\nsecond line"));
        match classify(fatal) {
            CallOutcome::InstanceFatal(text) => assert_eq!(text, "host exploded"),
            other => panic!("expected InstanceFatal, got {other:?}"),
        }
    }

    #[test]
    fn classify_labels_a_trap() {
        let trapped: wasmtime::Result<Result<u32, PollError>> =
            Err(wasmtime::Error::new(wasmtime::Trap::UnreachableCodeReached));
        match classify(trapped) {
            CallOutcome::InstanceFatal(text) => {
                assert!(text.contains("UnreachableCodeReached"), "{text}");
            }
            other => panic!("expected InstanceFatal, got {other:?}"),
        }
    }
}
