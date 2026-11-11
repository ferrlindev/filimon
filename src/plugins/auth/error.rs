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
}
