// ─────────────────────────────────────────────────────────────────────────────
// tests/plugins_cli.rs - `fili plugins` through the real binary
//
// These tests drive the compiled `fili` executable. They only ever run the
// commands that read the registry and the state directory; nothing here
// exercises a command that writes to the production topic or needs a Fluvio
// cluster.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// A `fili` invocation with the environment that could redirect it removed.
fn fili() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fili"));
    command
        .env_remove("FILI_REGISTRY")
        .env_remove("FILI_STATE_DIR")
        .env_remove("RUST_LOG");
    command
}

/// Writes a registry with one source, `alpha`, and returns its path. The
/// plugin file and the component are never opened by `list`, so neither is
/// written.
fn write_registry(dir: &Path) -> PathBuf {
    let path = dir.join("filimon.toml");
    std::fs::write(
        &path,
        "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfig = \"alpha.toml\"\n",
    )
    .expect("write registry");
    path
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn list_prints_the_registered_plugins_and_no_banner() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path());
    let state = dir.path().join("state");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(&state)
        .args(["plugins", "list"])
        .output()
        .expect("run fili");

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    let mut lines = text.lines();
    let header = lines.next().expect("header line");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        ["NAME", "KIND", "ENABLED", "FETCHER", "LAST", "POLL"],
        "{text}"
    );
    let row = lines.next().expect("one row per plugin");
    assert!(row.starts_with("alpha"), "{row:?}");
    assert!(row.contains("source"), "{row:?}");
    assert!(!text.contains('╔'), "the banner leaked into stdout: {text}");
    assert!(
        !state.exists(),
        "list must not create the state directory"
    );
}
