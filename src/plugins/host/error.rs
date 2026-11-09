// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/error.rs - Plugin host diagnostics
//
// Every text here is host-generated: plugin aliases, file paths and io or
// wasmtime reasons. Guest strings, secret values, settings and record content
// never appear in a `HostError`, and no variant carries a source-code block.
// ─────────────────────────────────────────────────────────────────────────────

use miette::Diagnostic;

/// A failure in the plugin host.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum HostError {
    /// The wasmtime engine or its linker could not be built.
    #[error("cannot start the plugin engine: {reason}")]
    #[diagnostic(code(fili::host::engine))]
    Engine { reason: String },

    /// The cursor state directory could not be created.
    #[error("cannot create plugin state directory `{path}`: {reason}")]
    #[diagnostic(
        code(fili::host::state_dir),
        help("pass a writable directory as the state directory")
    )]
    StateDir { path: String, reason: String },

    /// Another process holds the lock on the cursor state directory.
    #[error("state directory `{path}` is in use by another fili process")]
    #[diagnostic(
        code(fili::host::state_dir_locked),
        help("stop the other fili process or pass --state-dir to use a different directory")
    )]
    StateDirLocked { path: String },

    /// A plugin component could not be opened, compiled or linked.
    #[error("plugin `{plugin}`: cannot load component `{path}`: {reason}")]
    #[diagnostic(code(fili::host::component_load))]
    ComponentLoad {
        plugin: String,
        path: String,
        reason: String,
    },

    /// A component imports something outside the `filimon:plugin/` package.
    #[error("plugin `{plugin}` (`{path}`) imports {imports:?}, which fili does not provide")]
    #[diagnostic(
        code(fili::host::foreign_import),
        help(
            "build the component for wasm32-unknown-unknown so it has no WASI imports; see plugins/README.md"
        )
    )]
    ForeignImports {
        plugin: String,
        path: String,
        imports: Vec<String>,
    },

    /// A component does not fit the WIT world it was loaded as.
    #[error("plugin `{plugin}` (`{path}`) does not fit the `{kind}` world: {reason}")]
    #[diagnostic(code(fili::host::wrong_world))]
    WrongWorld {
        plugin: String,
        kind: &'static str,
        path: String,
        reason: String,
    },

    /// A cursor state file exists but could not be read.
    #[error("plugin `{plugin}`: cannot read cursor file `{path}`: {reason}")]
    #[diagnostic(
        code(fili::host::cursor_unreadable),
        help("fix the file's permissions or delete it to reset the cursor")
    )]
    CursorUnreadable {
        plugin: String,
        path: String,
        reason: String,
    },

    /// A cursor state file was read but its content is not valid.
    #[error("plugin `{plugin}`: cursor file `{path}` is corrupt: {reason}")]
    #[diagnostic(
        code(fili::host::cursor_corrupt),
        help("delete the file to reset the cursor; fili never resets it on its own")
    )]
    CursorCorrupt {
        plugin: String,
        path: String,
        reason: String,
    },

    /// A cursor state file could not be written.
    #[error("plugin `{plugin}`: cannot write cursor file `{path}`: {reason}")]
    #[diagnostic(code(fili::host::cursor_write))]
    CursorWrite {
        plugin: String,
        path: String,
        reason: String,
    },
}
