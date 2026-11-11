// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  reddit-source (filimon:plugin `source` world)
//
// Everything except the component glue is ordinary Rust behind the `Http`
// trait, so `cargo test` replays fixtures natively with no network and no keys.
// The generated WIT bindings and the `Guest` impl exist only for wasm32.
//
// A bare `cargo test` targets wasm32 (see `.cargo/config.toml`) and cannot run;
// test with the host triple:
//
//   cargo test --target "$(rustc -vV | sed -n 's/^host: //p')"
// ─────────────────────────────────────────────────────────────────────────────

pub mod api;
pub mod cursor;
pub mod http;
pub mod poll;
pub mod settings;

#[cfg(target_arch = "wasm32")]
#[allow(warnings)]
mod bindings;

#[cfg(target_arch = "wasm32")]
mod component;
