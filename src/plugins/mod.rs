// ─────────────────────────────────────────────────────────────────────────────
// plugins/mod.rs - WebAssembly plugin configuration
//
// Loads the `filimon.toml` plugin registry, each plugin's own TOML file and the
// `.env` secrets next to the registry into a typed `LoadedRegistry`. The plugin
// host and the CLI consume it; secrets are filtered per plugin here, before
// they can ever cross into a guest's `init`.
// ─────────────────────────────────────────────────────────────────────────────

#![cfg_attr(
    not(test),
    allow(dead_code, reason = "consumed by the plugin host and CLI in Phases 3-4")
)]

pub mod config;
pub mod error;
pub mod secrets;
