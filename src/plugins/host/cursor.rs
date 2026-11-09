// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/cursor.rs - Per-plugin cursor persistence (D-05, D-08)
//
// One JSON file per plugin alias: `<state_dir>/<alias>.json` holding
// `{"version":1,"plugin":"<alias>","cursor":"<base64>"|null}`. The cursor is
// the plugin's opaque bytes. Writes are atomic: a same-directory temp file is
// synced and renamed over the target. A missing file is a first poll; an
// unreadable or corrupt file is an error the scheduler turns into a disabled
// plugin, and it is never reset automatically.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use super::error::HostError;

/// Version written into every state file.
pub const STATE_VERSION: u32 = 1;

/// The state file's JSON shape. Unknown fields are tolerated on read so later
/// phases can add fields without breaking restarts.
#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    plugin: String,
    cursor: Option<String>,
}

/// Reads and writes the cursor state files in one directory.
#[derive(Debug)]
pub struct CursorStore {
    dir: PathBuf,
}

impl CursorStore {
    /// Opens the store, creating the directory when needed.
    ///
    /// # Errors
    /// Returns `HostError::StateDir` when the directory cannot be created.
    pub fn open(dir: &Path) -> Result<Self, HostError> {
        std::fs::create_dir_all(dir).map_err(|e| HostError::StateDir {
            path: dir.display().to_string(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// The state file for `alias`. Aliases come only from the loaded registry,
    /// which limits them to `[a-z0-9_-]`.
    #[must_use]
    pub fn path_for(&self, alias: &str) -> PathBuf {
        self.dir.join(format!("{alias}.json"))
    }

    /// Reads the stored cursor. `Ok(None)` means a first poll: no file, or a
    /// file whose cursor is null.
    ///
    /// # Errors
    /// `CursorUnreadable` for an io failure other than a missing file,
    /// `CursorCorrupt` for invalid JSON or base64.
    pub fn read(&self, alias: &str) -> Result<Option<Vec<u8>>, HostError> {
        let path = self.path_for(alias);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(HostError::CursorUnreadable {
                    plugin: alias.to_string(),
                    path: path.display().to_string(),
                    reason: e.to_string(),
                });
            }
        };
        let corrupt = |reason: &str| HostError::CursorCorrupt {
            plugin: alias.to_string(),
            path: path.display().to_string(),
            reason: reason.to_string(),
        };
        let state: StateFile =
            serde_json::from_slice(&bytes).map_err(|_| corrupt("not a valid state file"))?;
        match state.cursor {
            None => Ok(None),
            Some(encoded) => STANDARD
                .decode(encoded)
                .map(Some)
                .map_err(|_| corrupt("cursor is not valid base64")),
        }
    }

    /// Atomically stores `cursor` (`None` makes the next poll a first poll).
    ///
    /// # Errors
    /// Returns `HostError::CursorWrite` when the file cannot be written.
    pub fn write(&self, alias: &str, cursor: Option<&[u8]>) -> Result<(), HostError> {
        let path = self.path_for(alias);
        let fail = |reason: String| HostError::CursorWrite {
            plugin: alias.to_string(),
            path: path.display().to_string(),
            reason,
        };
        let state = StateFile {
            version: STATE_VERSION,
            plugin: alias.to_string(),
            cursor: cursor.map(|bytes| STANDARD.encode(bytes)),
        };
        let mut temp =
            tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| fail(e.to_string()))?;
        serde_json::to_writer(&mut temp, &state).map_err(|e| fail(e.to_string()))?;
        temp.as_file().sync_all().map_err(|e| fail(e.to_string()))?;
        temp.persist(&path).map_err(|e| fail(e.error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_then_read_round_trips() {
        let dir = TempDir::new().expect("tempdir");
        let store = CursorStore::open(dir.path()).expect("open");

        store.write("stub", Some(&[1])).expect("write");
        assert_eq!(store.read("stub").expect("read"), Some(vec![1]));

        let text = std::fs::read_to_string(store.path_for("stub")).expect("file");
        let json: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(json["version"], 1);
        assert_eq!(json["plugin"], "stub");
        assert_eq!(json["cursor"], "AQ==");

        // A null cursor reads back as a first poll.
        store.write("stub", None).expect("write none");
        assert_eq!(store.read("stub").expect("read"), None);
    }

    #[test]
    fn missing_file_is_a_first_poll() {
        let dir = TempDir::new().expect("tempdir");
        let store = CursorStore::open(&dir.path().join("nested")).expect("open creates dir");
        assert_eq!(store.read("nobody").expect("read"), None);
    }

    #[test]
    fn corrupt_json_names_plugin_and_path() {
        let dir = TempDir::new().expect("tempdir");
        let store = CursorStore::open(dir.path()).expect("open");
        std::fs::write(store.path_for("stub"), "{not json").expect("write");

        let error = store.read("stub").expect_err("corrupt");
        assert!(matches!(error, HostError::CursorCorrupt { .. }));
        let text = error.to_string();
        assert!(text.contains("stub"), "{text}");
        assert!(
            text.contains(&store.path_for("stub").display().to_string()),
            "{text}"
        );
    }
}
