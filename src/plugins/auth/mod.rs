// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/mod.rs - Host-managed OAuth for plugins
//
// A plugin whose TOML declares `[host.oauth]` never holds a credential of its
// own: the host stores the user's login in `<state-dir>/auth/<alias>.json`,
// re-reads it before every poll and hands the guest only the current access
// token, as `poll`'s `access-token` argument.
//
// - `client`: the token endpoint client (code exchange, refresh, revoke).
// - `store`: the token file and the redacting token types.
// - `provider`: the pre-poll decision (token, not logged in, needs login,
//   refresh due, or a file problem).
// - `login`: the interactive PKCE login flow behind a small hooks trait.
// - `error`: the diagnostics of all of them.
//
// Nothing in this module takes the state directory lock: `fili run` holds it
// for its whole life, and the auth commands must work beside it.
// ─────────────────────────────────────────────────────────────────────────────

pub mod callback;
pub mod client;
pub mod error;
pub mod login;
pub mod pkce;
pub mod provider;
pub mod store;

#[cfg(test)]
pub(crate) mod testing;
