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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `text` with whitespace and miette's gutter character removed, so a long
/// path or sentence that miette wrapped can still be searched for.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '│')
        .collect()
}

/// Writes a registry with one enabled source `stub` whose plugin file is valid
/// and whose component file holds `component_bytes`; returns the registry path
/// and the component path.
fn write_check_registry(dir: &Path, component_bytes: &[u8]) -> (PathBuf, PathBuf) {
    let registry = dir.join("filimon.toml");
    std::fs::write(
        &registry,
        "[sources.stub]\ncomponent = \"stub.wasm\"\nconfig = \"stub.toml\"\n",
    )
    .expect("write registry");
    std::fs::write(dir.join("stub.toml"), "[host]\npoll_interval_secs = 60\n")
        .expect("write plugin toml");
    let component = dir.join("stub.wasm");
    std::fs::write(&component, component_bytes).expect("write component");
    (registry, component)
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
    assert!(!state.exists(), "list must not create the state directory");
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

// ── check ─────────────────────────────────────────────────────────────────────

/// The tracer: the real prebuilt stub source passes `check`, which neither
/// initialises the guest nor creates the state directory.
#[test]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
fn wasm_check_passes_the_real_stub_source() {
    let dir = TempDir::new().expect("tempdir");
    let registry = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins/filimon.toml");
    let state = dir.path().join("state");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(&state)
        .args(["plugins", "check", "stub"])
        .output()
        .expect("run fili");

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(text.contains("plugin `stub` (source)"), "{text}");
    let component_line = text
        .lines()
        .find(|line| line.contains("component: ok"))
        .unwrap_or_else(|| panic!("no component line: {text}"));
    assert!(component_line.contains("`source` world"), "{text}");
    assert!(text.contains("config: ok"), "{text}");
    assert!(text.contains("check passed"), "{text}");
    assert!(
        !stderr(&output).contains("hello from the stub source"),
        "the guest's init ran: {}",
        stderr(&output)
    );
    assert!(!state.exists(), "check must not create the state directory");
}

#[test]
fn check_garbage_component_exits_nonzero_naming_plugin_and_path() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, component) = write_check_registry(dir.path(), b"this is not a wasm component");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "check", "stub"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let err = squash(&stderr(&output));
    assert!(err.contains("stub"), "{}", stderr(&output));
    assert!(
        err.contains(&squash(&component.display().to_string())),
        "stderr must name the component path: {}",
        stderr(&output)
    );
    assert!(
        err.contains(&squash("cannot load component")),
        "{}",
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("check failed"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn check_unknown_plugin_exits_nonzero_and_names_the_registered_ones() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, _) = write_check_registry(dir.path(), b"unused");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "check", "nope"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let err = squash(&stderr(&output));
    assert!(
        err.contains(&squash("no plugin named `nope`")),
        "{}",
        stderr(&output)
    );
    assert!(
        err.contains(&squash("sources: stub")),
        "{}",
        stderr(&output)
    );
    assert!(output.stdout.is_empty(), "{}", stdout(&output));
}

#[test]
fn check_a_broken_config_exits_nonzero_naming_plugin_and_field() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, _) = write_check_registry(dir.path(), b"unused");
    std::fs::write(
        dir.path().join("stub.toml"),
        "[host]\npoll_interval_secs = 0\n",
    )
    .expect("broken plugin toml");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "check", "stub"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let err = squash(&stderr(&output));
    assert!(err.contains("stub"), "{}", stderr(&output));
    assert!(err.contains("poll_interval_secs"), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("config: FAILED"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn check_never_prints_a_secret_value() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, _) = write_check_registry(dir.path(), b"this is not a wasm component");
    std::fs::write(
        dir.path().join("stub.toml"),
        "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"FILI_CHECK_TOKEN\"\nmissing = \"FILI_CHECK_MISSING\"\n",
    )
    .expect("plugin toml with secrets");
    std::fs::write(
        dir.path().join(".env"),
        "FILI_CHECK_TOKEN=hunter2-distinctive\n",
    )
    .expect("write .env");

    let output = fili()
        .env_remove("FILI_CHECK_TOKEN")
        .env_remove("FILI_CHECK_MISSING")
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "check", "stub"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let err = squash(&stderr(&output));
    assert!(err.contains("FILI_CHECK_MISSING"), "{}", stderr(&output));
    for stream in [squash(&stdout(&output)), err] {
        for leaked in ["hunter2-distinctive", "hunter2", "distinctive"] {
            assert!(!stream.contains(leaked), "leaked {leaked:?} in: {stream}");
        }
    }
}

#[test]
fn check_does_not_create_the_state_dir() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, _) = write_check_registry(dir.path(), b"this is not a wasm component");
    let state = dir.path().join("state");
    let before = std::fs::read_dir(dir.path()).expect("read dir").count();

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(&state)
        .args(["plugins", "check", "stub"])
        .output()
        .expect("run fili");

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(!state.exists(), "check must not create the state directory");
    let after = std::fs::read_dir(dir.path()).expect("read dir").count();
    assert_eq!(before, after, "check must not create anything");
}

#[test]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
fn wasm_check_passes_the_real_stub_fetcher() {
    let dir = TempDir::new().expect("tempdir");
    let registry = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins/filimon.toml");

    let output = fili()
        .arg("--registry")
        .arg(&registry)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(["plugins", "check", "stub-fetcher"])
        .output()
        .expect("run fili");

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(&output)
    );
    let text = stdout(&output);
    assert!(text.contains("plugin `stub-fetcher` (fetcher)"), "{text}");
    assert!(text.contains("`fetcher` world"), "{text}");
    assert!(text.contains("config: none"), "{text}");
    assert!(text.contains("check passed"), "{text}");
}
