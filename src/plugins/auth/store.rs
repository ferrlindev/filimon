// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/store.rs - The stored login of one plugin
//
// A login lives in `<state-dir>/auth/<alias>.json`, one file per plugin,
// written atomically: the new content goes to a temporary file in the same
// directory, is synced, and replaces the old file by a rename, so a reader sees
// the old login or the new one and never half of either (D-01).
//
// `read` takes no lock and creates nothing, so it works while `fili run` holds
// the state directory lock and on a directory that does not exist yet. A file
// that cannot be used is an error naming the plugin and the path; it is never
// treated as "logged out" and never repaired, rewritten or deleted (D-02).
//
// Token values live in redacting newtypes: `Debug` prints `***` and there is no
// `Display`, so a token cannot reach a log line by formatting (D-06).
// ─────────────────────────────────────────────────────────────────────────────

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::AuthError;
use crate::plugins::config::is_valid_name;
use crate::plugins::host::engine::clean_guest_text;

/// The directory under the state directory that holds the token files.
pub const AUTH_DIR_NAME: &str = "auth";

/// Version written into every token file.
pub const TOKEN_FILE_VERSION: u32 = 1;

/// Largest token file `read` looks at, in bytes.
const MAX_TOKEN_FILE_BYTES: u64 = 64 * 1024;

/// Longest io error text kept in a diagnostic, in characters.
const IO_TEXT_CLIP: usize = 160;

// ── Token Types ───────────────────────────────────────────────────────────────

/// An OAuth access token. `Debug` is redacted and there is no `Display`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccessToken(String);

impl AccessToken {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// The token text. The only way to read it: call it where the value is
    /// handed on, never where it could be formatted.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccessToken(***)")
    }
}

/// An OAuth refresh token. `Debug` is redacted and there is no `Display`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RefreshToken(String);

impl RefreshToken {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// The token text; see `AccessToken::expose`.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RefreshToken(***)")
    }
}

/// What one plugin's login holds.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTokens {
    pub access_token: AccessToken,
    pub refresh_token: Option<RefreshToken>,
    /// Unix seconds, absolute.
    pub expires_at: i64,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    /// The login was revoked or its refresh token was rejected: the user must
    /// log in again. The file is kept as it is (D-09).
    pub needs_login: bool,
    /// Unix seconds when the access token was obtained.
    pub obtained_at: i64,
}

/// The file's JSON shape. Unknown fields are tolerated on read so later plans
/// can add fields.
#[derive(Serialize, Deserialize)]
struct TokenFile {
    version: u32,
    plugin: String,
    access_token: AccessToken,
    #[serde(default)]
    refresh_token: Option<RefreshToken>,
    expires_at: i64,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    needs_login: bool,
    #[serde(default)]
    obtained_at: i64,
}

// ── The Store ─────────────────────────────────────────────────────────────────

/// Reads and writes one plugin's token file.
#[derive(Debug, Clone)]
pub struct TokenStore {
    dir: PathBuf,
    alias: String,
}

impl TokenStore {
    /// A store for `alias` under `<state_dir>/auth`. Nothing is created.
    #[must_use]
    pub fn new(state_dir: &Path, alias: &str) -> Self {
        Self {
            dir: state_dir.join(AUTH_DIR_NAME),
            alias: alias.to_string(),
        }
    }

    /// The token file's path, after the alias passed the name rule, so no path
    /// is ever built from an unchecked alias.
    fn file_path(&self) -> Result<PathBuf, AuthError> {
        if !is_valid_name(&self.alias) {
            return Err(AuthError::InvalidName {
                name: self.alias.clone(),
            });
        }
        Ok(self.dir.join(format!("{}.json", self.alias)))
    }

    /// Reads the stored login without taking any lock and without creating
    /// anything. `Ok(None)` means there is no login: no file, or no directory.
    ///
    /// # Errors
    /// `InvalidName` for an alias that fails the name rule; `TokenFile` for an
    /// io failure other than a missing file, a file that is not a valid token
    /// file, a `version` this host does not know, or a file that belongs to
    /// another plugin. The file is left as it was found.
    pub fn read(&self) -> Result<Option<StoredTokens>, AuthError> {
        let path = self.file_path()?;
        let bad = |reason: String| AuthError::TokenFile {
            plugin: self.alias.clone(),
            path: path.display().to_string(),
            reason,
        };
        let io_problem = |e: &std::io::Error| {
            bad(format!(
                "cannot be read: {}",
                clean_guest_text(&e.to_string(), IO_TEXT_CLIP)
            ))
        };

        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_problem(&e)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_TOKEN_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_problem(&e))?;
        if bytes.len() as u64 > MAX_TOKEN_FILE_BYTES {
            return Err(bad(format!("is larger than {MAX_TOKEN_FILE_BYTES} bytes")));
        }

        let file: TokenFile = serde_json::from_slice(&bytes)
            .map_err(|_| bad("is not a valid token file".to_string()))?;
        if file.version > TOKEN_FILE_VERSION {
            return Err(bad(format!("has unsupported version {}", file.version)));
        }
        if file.plugin != self.alias {
            return Err(bad(format!(
                "belongs to plugin `{}`",
                clean_guest_text(&file.plugin, 64)
            )));
        }
        Ok(Some(StoredTokens {
            access_token: file.access_token,
            refresh_token: file.refresh_token,
            expires_at: file.expires_at,
            token_type: file.token_type,
            scope: file.scope,
            needs_login: file.needs_login,
            obtained_at: file.obtained_at,
        }))
    }

    /// Atomically stores `tokens`, creating the `auth` directory when needed:
    /// a same-directory temporary file, synced, then renamed over the target.
    ///
    /// # Errors
    /// `InvalidName` for an alias that fails the name rule; `TokenWrite` when
    /// the directory or the file cannot be written.
    pub fn write(&self, tokens: &StoredTokens) -> Result<(), AuthError> {
        let path = self.file_path()?;
        let fail = |reason: String| AuthError::TokenWrite {
            plugin: self.alias.clone(),
            path: path.display().to_string(),
            reason: clean_guest_text(&reason, IO_TEXT_CLIP),
        };
        std::fs::create_dir_all(&self.dir).map_err(|e| fail(e.to_string()))?;

        let file = TokenFile {
            version: TOKEN_FILE_VERSION,
            plugin: self.alias.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            token_type: tokens.token_type.clone(),
            scope: tokens.scope.clone(),
            needs_login: tokens.needs_login,
            obtained_at: tokens.obtained_at,
        };
        let mut temp =
            tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| fail(e.to_string()))?;
        serde_json::to_writer(&mut temp, &file).map_err(|e| fail(e.to_string()))?;
        temp.as_file().sync_all().map_err(|e| fail(e.to_string()))?;
        temp.persist(&path).map_err(|e| fail(e.error.to_string()))?;
        // Best effort: make the rename itself durable. Some platforms cannot
        // open a directory, and the data is already safe in the file.
        if let Ok(dir) = File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn tokens(access: &str, expires_at: i64) -> StoredTokens {
        StoredTokens {
            access_token: AccessToken::new(access.to_string()),
            refresh_token: Some(RefreshToken::new("RT-1".to_string())),
            expires_at,
            token_type: Some("Bearer".to_string()),
            scope: Some("read".to_string()),
            needs_login: false,
            obtained_at: expires_at - 3600,
        }
    }

    #[test]
    fn write_then_read_round_trips_the_tokens() {
        let dir = TempDir::new().expect("tempdir");
        let store = TokenStore::new(dir.path(), "stub");
        let first = tokens("AT-1", 2_000_000_000);

        store.write(&first).expect("write");
        let path = dir.path().join("auth").join("stub.json");
        assert!(path.is_file(), "{}", path.display());
        assert_eq!(store.read().expect("read"), Some(first));

        // A second write replaces the file atomically; no temp file is left.
        let second = tokens("AT-2", 2_000_003_600);
        store.write(&second).expect("second write");
        assert_eq!(store.read().expect("read"), Some(second));
        let names: Vec<String> = std::fs::read_dir(dir.path().join("auth"))
            .expect("auth dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["stub.json"]);
    }

    #[test]
    fn token_types_never_print_their_value() {
        let access = AccessToken::new("AT-SECRET".to_string());
        let refresh = RefreshToken::new("RT-SECRET".to_string());
        let debug = format!("{access:?} {refresh:?} {:?}", tokens("AT-SECRET", 1));
        assert!(!debug.contains("AT-SECRET"), "{debug}");
        assert!(!debug.contains("RT-1"), "{debug}");
        assert!(debug.contains("AccessToken(***)"), "{debug}");
    }

    #[test]
    fn a_missing_file_or_directory_reads_as_no_login_and_creates_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let store = TokenStore::new(dir.path(), "stub");
        assert_eq!(store.read().expect("read"), None);
        assert!(!dir.path().join("auth").exists());
    }

    #[test]
    fn an_alias_that_fails_the_name_rule_never_builds_a_path() {
        let dir = TempDir::new().expect("tempdir");
        for alias in ["", "../x", "a/b", "A", "a.b"] {
            let store = TokenStore::new(dir.path(), alias);
            assert!(
                matches!(store.read(), Err(AuthError::InvalidName { .. })),
                "{alias:?}"
            );
            assert!(
                matches!(
                    store.write(&tokens("AT", 1)),
                    Err(AuthError::InvalidName { .. })
                ),
                "{alias:?}"
            );
        }
        assert!(!dir.path().join("auth").exists());
    }
}
