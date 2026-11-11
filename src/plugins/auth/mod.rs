// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/mod.rs - Host-managed OAuth for plugins
//
// A plugin whose TOML declares `[host.oauth]` never holds a credential of its
// own: the host stores the user's login in `<state-dir>/auth/<alias>.json`,
// re-reads it before every poll and hands the guest only the current access
// token, as `poll`'s `access-token` argument.
//
// - `store`: the token file and the redacting token types.
// - `provider`: the pre-poll decision (token, not logged in, needs login,
//   refresh due, or a file problem).
// - `error`: the diagnostics of both.
//
// Nothing in this module takes the state directory lock: `fili run` holds it
// for its whole life, and the auth commands must work beside it.
// ─────────────────────────────────────────────────────────────────────────────

// The login flow, the token client and the CLI commands that read the items
// still unused here land in later plans of this phase; plan 06-08 removes this.
#![allow(
    dead_code,
    reason = "readers for the remaining items land in later plans of this phase"
)]

pub mod error;
pub mod pkce;
pub mod provider;
pub mod store;

#[cfg(test)]
pub(crate) mod testing;
