// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/error.rs - Host-managed OAuth diagnostics
//
// Every text here is a plugin alias, a file path or a fixed phrase, plus at
// most a cleaned io error. A token, a secret or any file content never appears
// in an `AuthError` (D-06).
// ─────────────────────────────────────────────────────────────────────────────

use miette::Diagnostic;

/// A failure reading or writing a plugin's stored login.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum AuthError {
    /// The token file exists but cannot be used. It is never repaired or
    /// deleted automatically (D-02).
    #[error("plugin `{plugin}`: token file `{path}` {reason}")]
    #[diagnostic(
        code(fili::auth::token_file),
        help(
            "fili never repairs or deletes this file on its own; fix the problem or run `fili auth logout {plugin}` and log in again"
        )
    )]
    TokenFile {
        plugin: String,
        path: String,
        reason: String,
    },

    /// The token file could not be written.
    #[error("plugin `{plugin}`: cannot write token file `{path}`: {reason}")]
    #[diagnostic(code(fili::auth::token_write))]
    TokenWrite {
        plugin: String,
        path: String,
        reason: String,
    },

    /// A plugin name that could become an unsafe path.
    #[error("plugin name `{name}` may only contain a-z, 0-9, '_' and '-'")]
    #[diagnostic(code(fili::auth::invalid_name))]
    InvalidName { name: String },

    /// The operating system could not supply random bytes for the PKCE
    /// verifier or the `state` value.
    #[error("cannot read secure random bytes: {reason}")]
    #[diagnostic(code(fili::auth::random))]
    Random { reason: String },

    /// The loopback port for the login callback cannot be bound.
    #[error("cannot listen on 127.0.0.1:{port} for the login callback: {reason}")]
    #[diagnostic(
        code(fili::auth::redirect_port_in_use),
        help(
            "another login or program is using the port; stop it, or set `host.oauth.redirect_port` in the plugin TOML to a free port and register the same redirect URI with the provider"
        )
    )]
    RedirectPortInUse { port: u16, reason: String },

    /// The provider answered the login with `error=`. Only the cleaned error
    /// code is carried, never `error_description`.
    #[error("the provider denied the login: {error}")]
    #[diagnostic(code(fili::auth::callback_denied))]
    CallbackDenied { error: String },

    /// The callback's `state` was missing or different from the one sent.
    /// Neither value is carried.
    #[error("the login callback did not carry the expected state, so it was rejected")]
    #[diagnostic(code(fili::auth::state_mismatch))]
    StateMismatch,

    /// Nothing reached the callback listener in time.
    #[error("no login callback arrived within {secs} seconds")]
    #[diagnostic(code(fili::auth::callback_timeout))]
    CallbackTimeout { secs: u64 },

    /// A callback with the right state that is not usable.
    #[error("the login callback is not usable: {reason}")]
    #[diagnostic(code(fili::auth::callback_invalid))]
    CallbackInvalid { reason: String },

    /// The token or revocation endpoint could not be reached, or its answer
    /// could not be read. The text is a cleaned transport description with no
    /// url, never a body.
    #[error("the token endpoint could not be reached: {reason}")]
    #[diagnostic(code(fili::auth::token_request))]
    TokenRequest { reason: String },

    /// The token endpoint answered with a non-success status. Only the cleaned
    /// OAuth error code and description (or the HTTP status) are carried.
    #[error("the token endpoint rejected the request: {error}")]
    #[diagnostic(code(fili::auth::token_rejected))]
    TokenRejected { error: String },

    /// The token endpoint answered with success but the answer is not usable.
    /// The body is never carried.
    #[error("the token endpoint's answer is not usable: {reason}")]
    #[diagnostic(code(fili::auth::token_response))]
    TokenResponse { reason: String },

    /// Revoking the login at the provider failed. The caller decides whether
    /// to warn (D-03).
    #[error("revoking the login at the provider failed: {reason}")]
    #[diagnostic(code(fili::auth::revoke_failed))]
    RevokeFailed { reason: String },
}
