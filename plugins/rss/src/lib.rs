// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  rss-source (filimon:plugin `source` world)
//
// Polls RSS 2.0 and Atom 1.0 feeds and returns one item per entry. Everything
// except the component glue is ordinary Rust behind the `Http` trait, so
// `cargo test` replays feeds natively with no network.
//
// A bare `cargo test` targets wasm32 (see `.cargo/config.toml`) and cannot run;
// test with the host triple:
//
//   cargo test --target "$(rustc -vV | sed -n 's/^host: //p')"
// ─────────────────────────────────────────────────────────────────────────────

pub mod cursor;
pub mod feed;
pub mod http;
pub mod poll;
pub mod settings;
pub mod text;

#[cfg(target_arch = "wasm32")]
#[allow(warnings)]
mod bindings;

#[cfg(target_arch = "wasm32")]
mod component;
