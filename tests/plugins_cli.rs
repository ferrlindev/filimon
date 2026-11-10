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

#[test]
fn list_json_stdout_parses_as_a_json_array() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path());
    let state = dir.path().join("state");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(&state)
        .args(["plugins", "list", "--json"])
        .output()
        .expect("run fili");

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout.first(), Some(&b'['), "{}", stdout(&output));
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON array");
    assert_eq!(rows.len(), 1, "{rows:?}");
    for key in ["name", "kind", "enabled", "fetcher", "last_poll"] {
        assert!(rows[0].get(key).is_some(), "missing key {key}: {rows:?}");
    }
    assert_eq!(rows[0]["name"], "alpha");
    assert_eq!(rows[0]["kind"], "source");
    assert_eq!(rows[0]["last_poll"]["state"], "never");
    assert!(!state.exists(), "list must not create the state directory");
}

#[test]
fn list_shows_the_sample_registry_plugins() {
    let dir = TempDir::new().expect("tempdir");
    let registry = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins/filimon.toml");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "list"])
        .output()
        .expect("run fili");

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = stdout(&output);
    let names: Vec<&str> = text
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(
        names,
        ["reddit", "stub", "youtube", "http", "stub-fetcher"],
        "{text}"
    );
}

#[test]
fn list_survives_a_broken_plugin_toml() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path());
    std::fs::write(dir.path().join("alpha.toml"), "[host\nbroken = ").expect("broken plugin toml");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "list"])
        .output()
        .expect("run fili");

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout(&output).contains("alpha"), "{}", stdout(&output));
}

#[test]
fn list_a_missing_registry_exits_one_and_names_the_path() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("nope.toml");

    let output = fili()
        .arg("--registry")
        .arg(&missing)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "list"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // miette wraps long lines (and draws a `│` gutter), so compare with the
    // wrapping removed.
    let unwrapped: String = stderr
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '│')
        .collect();
    let wanted: String = missing
        .display()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        unwrapped.contains(&wanted),
        "stderr must name the registry path: {stderr}"
    );
    assert!(output.stdout.is_empty(), "{}", stdout(&output));
}

#[test]
fn list_with_a_blank_state_dir_flag_is_a_usage_error() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path());

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .args(["--state-dir", ""])
        .args(["plugins", "list"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn plugins_without_a_subcommand_is_a_usage_error() {
    let output = fili().arg("plugins").output().expect("run fili");

    assert_eq!(output.status.code(), Some(2));
}
