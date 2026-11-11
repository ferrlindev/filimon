// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/store.rs - The stored login of one plugin
//
// A login lives in `<state-dir>/auth/<alias>.json`, one file per plugin,
// written atomically: the new content goes to a temporary file in the same
// directory, is synced, and replaces the old file by a rename, so a reader sees
// the old login or the new one and never half of either (D-01).
//
// `read` takes no lock and creates nothing, so it works while `fili run` holds
// the state directory lock and on a directory that does not exist yet. It
// refuses anything but a small regular file with no group or other permission
// bit. A file that cannot be used is an error naming the plugin and the path;
// it is never treated as "logged out" and never repaired, chmod'd, rewritten or
// deleted (D-02). The directory is created 0700 and files 0600 from the first
// byte (D-01).
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
pub const MAX_TOKEN_FILE_BYTES: u64 = 64 * 1024;

/// Any group or other permission bit makes a token file too loose.
#[cfg(unix)]
const LOOSE_MODE_BITS: u32 = 0o077;

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

    /// The path of this alias's file with `extension`, after the alias passed
    /// the name rule, so no path is ever built from an unchecked alias.
    fn path_with(&self, extension: &str) -> Result<PathBuf, AuthError> {
        if !is_valid_name(&self.alias) {
            return Err(AuthError::InvalidName {
                name: self.alias.clone(),
            });
        }
        Ok(self.dir.join(format!("{}.{extension}", self.alias)))
    }

    /// The token file's path.
    fn file_path(&self) -> Result<PathBuf, AuthError> {
        self.path_with("json")
    }

    /// Reads the stored login without taking any lock and without creating
    /// anything. `Ok(None)` means there is no login: no file, or no directory.
    ///
    /// The checks run in a fixed order and each refusal has a fixed reason that
    /// never echoes file content: not a regular file (a symlink or a
    /// directory), permissions with a group or other bit (Unix), too large, not
    /// valid JSON, an unknown `version`, or a file that belongs to another
    /// plugin.
    ///
    /// # Errors
    /// `InvalidName` for an alias that fails the name rule; `TokenFile` for any
    /// refusal above or an io failure other than a missing file. The file is
    /// left exactly as it was found: never chmod'd, rewritten or deleted.
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

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_problem(&e)),
        };
        if !meta.file_type().is_file() {
            return Err(bad("is not a regular file".to_string()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode();
            if mode & LOOSE_MODE_BITS != 0 {
                return Err(bad(format!(
                    "has permissions {:03o}, looser than 600; fix it with chmod 600",
                    mode & 0o777
                )));
            }
        }
        if meta.len() > MAX_TOKEN_FILE_BYTES {
            return Err(bad(format!("is larger than {MAX_TOKEN_FILE_BYTES} bytes")));
        }

        let file = File::open(&path).map_err(|e| io_problem(&e))?;
        // The file can grow between the check above and this read.
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

    /// Creates the `auth` directory when it is missing: 0700 on Unix. The
    /// permissions of a directory that already exists are never changed.
    fn ensure_dir(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.dir)
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(&self.dir)
        }
    }

    /// Atomically stores `tokens`, creating the `auth` directory (0700) when
    /// needed: a same-directory temporary file that is 0600 before its first
    /// byte, synced, then renamed over the target.
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
        self.ensure_dir().map_err(|e| fail(e.to_string()))?;

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
        // The tempfile crate already creates 0600; stating it here keeps the
        // guarantee if that ever changes. No byte is written before this.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| fail(e.to_string()))?;
        }
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

    /// Sets a file's mode on Unix; a no-op elsewhere.
    fn set_mode(path: &Path, mode: u32) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .expect("set permissions");
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
        }
    }

    /// The permission bits of a path.
    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    /// Writes `<state>/auth/<alias>.json` with `content` and `mode`.
    fn put_file(state: &Path, alias: &str, content: &[u8], mode: u32) -> PathBuf {
        let auth = state.join("auth");
        std::fs::create_dir_all(&auth).expect("auth dir");
        let path = auth.join(format!("{alias}.json"));
        std::fs::write(&path, content).expect("write the fixture");
        set_mode(&path, mode);
        path
    }

    /// A valid version-1 token file body for `plugin`.
    fn valid_json(plugin: &str) -> String {
        format!(
            r#"{{"version":1,"plugin":"{plugin}","access_token":"AT-1","refresh_token":"RT-1","expires_at":2000000000,"obtained_at":1}}"#
        )
    }

    /// The text of a refusal, asserting it is a `TokenFile` error and that
    /// neither its display nor its debug form contains `must_not_contain`.
    fn refusal(result: Result<Option<StoredTokens>, AuthError>, must_not_contain: &str) -> String {
        let err = result.expect_err("a refusal");
        assert!(matches!(err, AuthError::TokenFile { .. }), "{err:?}");
        let text = err.to_string();
        assert!(!text.contains(must_not_contain), "{text}");
        assert!(!format!("{err:?}").contains(must_not_contain), "{err:?}");
        text
    }

    #[cfg(unix)]
    #[test]
    fn write_creates_a_0600_file_in_a_0700_directory() {
        let dir = TempDir::new().expect("tempdir");
        let store = TokenStore::new(dir.path(), "stub");
        store.write(&tokens("AT-1", 2_000_000_000)).expect("write");
        assert_eq!(mode_of(&dir.path().join("auth")), 0o700);
        assert_eq!(mode_of(&dir.path().join("auth").join("stub.json")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_looser_than_0600_is_refused_naming_plugin_and_path_and_left_alone() {
        for mode in [0o644, 0o640, 0o604, 0o660] {
            let dir = TempDir::new().expect("tempdir");
            let body = valid_json("stub");
            let path = put_file(dir.path(), "stub", body.as_bytes(), mode);
            let store = TokenStore::new(dir.path(), "stub");

            let text = refusal(store.read(), "AT-1");
            assert!(text.contains("`stub`"), "{mode:o}: {text}");
            assert!(text.contains(&path.display().to_string()), "{mode:o}: {text}");
            assert!(text.contains("permissions"), "{mode:o}: {text}");
            assert!(text.contains(&format!("{mode:03o}")), "{mode:o}: {text}");
            assert_eq!(std::fs::read(&path).expect("bytes"), body.as_bytes());
            assert_eq!(mode_of(&path), mode, "never chmod'd");
        }
        for mode in [0o600, 0o400] {
            let dir = TempDir::new().expect("tempdir");
            put_file(dir.path(), "stub", valid_json("stub").as_bytes(), mode);
            let store = TokenStore::new(dir.path(), "stub");
            assert!(store.read().expect("read").is_some(), "{mode:o}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_token_file_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let real = put_file(dir.path(), "real", valid_json("stub").as_bytes(), 0o600);
        let link = dir.path().join("auth").join("stub.json");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let store = TokenStore::new(dir.path(), "stub");

        let text = refusal(store.read(), "AT-1");
        assert!(text.contains("is not a regular file"), "{text}");
        assert!(link.is_symlink(), "left alone");
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("auth").join("stub.json")).expect("dir");
        let store = TokenStore::new(dir.path(), "stub");

        let text = refusal(store.read(), "AT-1");
        assert!(text.contains("is not a regular file"), "{text}");
    }

    #[test]
    fn an_oversized_token_file_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let big = vec![b' '; usize::try_from(MAX_TOKEN_FILE_BYTES).expect("fits") + 1];
        put_file(dir.path(), "stub", &big, 0o600);
        let store = TokenStore::new(dir.path(), "stub");

        let text = refusal(store.read(), "AT-1");
        assert!(text.contains("65536"), "{text}");
    }

    #[test]
    fn a_future_version_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let body = valid_json("stub").replace("\"version\":1", "\"version\":2");
        put_file(dir.path(), "stub", body.as_bytes(), 0o600);
        let store = TokenStore::new(dir.path(), "stub");

        let text = refusal(store.read(), "AT-1");
        assert!(text.contains("unsupported version"), "{text}");
    }

    #[test]
    fn a_file_for_another_plugin_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        put_file(dir.path(), "mine", valid_json("other").as_bytes(), 0o600);
        let store = TokenStore::new(dir.path(), "mine");

        let text = refusal(store.read(), "AT-1");
        assert!(text.contains("other"), "{text}");
    }

    #[test]
    fn invalid_json_is_refused_without_echoing_content() {
        let dir = TempDir::new().expect("tempdir");
        put_file(dir.path(), "stub", b"SECRET-CONTENT-SENTINEL", 0o600);
        let store = TokenStore::new(dir.path(), "stub");

        let text = refusal(store.read(), "SECRET-CONTENT-SENTINEL");
        assert!(text.contains("is not a valid token file"), "{text}");
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let dir = TempDir::new().expect("tempdir");
        let body = valid_json("stub").replace("\"version\":1,", "\"version\":1,\"future_field\":1,");
        put_file(dir.path(), "stub", body.as_bytes(), 0o600);
        let store = TokenStore::new(dir.path(), "stub");

        assert!(store.read().expect("read").is_some());
    }

    #[test]
    fn read_never_creates_or_modifies_anything() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("not-there");
        assert_eq!(TokenStore::new(&missing, "stub").read().expect("read"), None);
        assert!(!missing.exists(), "the state directory is still absent");

        let path = put_file(dir.path(), "stub", valid_json("stub").as_bytes(), 0o600);
        let before = (
            std::fs::read(&path).expect("bytes"),
            std::fs::metadata(&path).expect("metadata").modified().expect("mtime"),
        );
        let store = TokenStore::new(dir.path(), "stub");
        assert!(store.read().expect("read").is_some());
        let after = (
            std::fs::read(&path).expect("bytes"),
            std::fs::metadata(&path).expect("metadata").modified().expect("mtime"),
        );
        assert_eq!(before, after);
        let names: Vec<_> = std::fs::read_dir(dir.path().join("auth"))
            .expect("auth dir")
            .collect();
        assert_eq!(names.len(), 1, "nothing was created beside the file");
    }

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
        assert!(!debug.contains("RT-SECRET"), "{debug}");
        assert!(debug.contains("AccessToken(***)"), "{debug}");
        assert!(debug.contains("RefreshToken(***)"), "{debug}");
        assert!(debug.contains("***"), "{debug}");
    }

    #[test]
    fn a_missing_file_or_directory_reads_as_no_login_and_creates_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let store = TokenStore::new(dir.path(), "stub");
        assert_eq!(store.read().expect("read"), None);
        assert!(!dir.path().join("auth").exists());
    }

    #[test]
    fn an_invalid_alias_is_refused_before_any_path_is_built() {
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
