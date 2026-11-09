// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/testing.rs - Shared test helpers (cfg(test) only)
// ─────────────────────────────────────────────────────────────────────────────

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::subscriber::DefaultGuard;

use crate::plugins::config::{LoadedRegistry, load_registry_with_env};

/// The repository root.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A prebuilt fixture component, or a panic naming how to build it.
pub fn fixture_wasm(crate_dir: &str, file: &str) -> PathBuf {
    let path = repo_root()
        .join("plugins")
        .join(crate_dir)
        .join("target/wasm32-unknown-unknown/release")
        .join(file);
    assert!(
        path.is_file(),
        "needs plugin fixtures: see plugins/README.md ({} is missing)",
        path.display()
    );
    path
}

/// One source for `write_registry`.
pub struct TestSource {
    pub name: String,
    pub component: PathBuf,
    pub enabled: bool,
    pub interval_secs: u64,
    /// `(initial_secs, max_secs, multiplier)`.
    pub backoff: Option<(u64, u64, f64)>,
    /// Text placed under `[settings]`.
    pub settings: String,
}

/// Writes `filimon.toml` and one `<name>.toml` per source into `dir`.
pub fn write_registry(dir: &Path, sources: &[TestSource]) -> PathBuf {
    let mut registry = String::new();
    for source in sources {
        registry.push_str(&format!(
            "[sources.{name}]\ncomponent = {component:?}\nconfig = \"{name}.toml\"\nenabled = {enabled}\n\n",
            name = source.name,
            component = source.component.display().to_string(),
            enabled = source.enabled,
        ));

        let mut plugin = format!("[host]\npoll_interval_secs = {}\n", source.interval_secs);
        if let Some((initial, max, multiplier)) = source.backoff {
            plugin.push_str(&format!(
                "\n[host.backoff]\ninitial_secs = {initial}\nmax_secs = {max}\nmultiplier = {multiplier:?}\n"
            ));
        }
        plugin.push_str(&format!("\n[settings]\n{}\n", source.settings));
        std::fs::write(dir.join(format!("{}.toml", source.name)), plugin)
            .expect("write plugin toml");
    }
    let path = dir.join("filimon.toml");
    std::fs::write(&path, registry).expect("write registry");
    path
}

/// Loads a test registry, panicking with the rendered error on failure.
pub fn load_test_registry(path: &Path) -> LoadedRegistry {
    load_registry_with_env(path, &|_| None).unwrap_or_else(|e| {
        panic!("test registry failed to load: {:?}", miette::Report::new(e));
    })
}

#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Captures `tracing` output into a string. The default subscriber is
/// thread-local, so tests using it stay on the current-thread runtime.
pub struct LogCapture {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl LogCapture {
    pub fn start() -> (Self, DefaultGuard) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedBuf(Arc::clone(&buf));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_target(true)
            .with_writer(move || writer.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        (Self { buf }, guard)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().expect("log buffer")).into_owned()
    }
}
