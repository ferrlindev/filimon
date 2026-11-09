# filimon plugins

This directory holds WebAssembly component plugins built against the filimon
plugin contract in [`../wit`](../wit) (package `filimon:plugin@0.1.0`).

## What lives here

- `stub-source/`: a minimal plugin for the `source` world. `init` parses its
  `[settings]` TOML, logs a greeting and the secret alias names (never the
  values), and calls the imported `fetch` when `fetch_url` is set. `poll`
  returns one hardcoded item and advances a one-byte cursor.
- `stub-fetcher/`: a minimal plugin for the `fetcher` world. `fetch` logs the
  request URL and returns a canned `200` JSON response. It never calls
  `outbound-http`.
- `fault-source/`: a fault-injection plugin for the `source` world and a
  Phase 3 test fixture, not a registry example. Its `[settings]` `mode`
  selects `ok`, `rate-limited`, `transient`, `auth-required`, `fatal`,
  `config`, `init-fatal`, `trap` or `spin`. Optional `retry_after_secs`
  (used by `rate-limited`) and `after_polls` (polls up to that count behave as
  `ok`) refine it. The `trap` and `spin` modes deliberately misbehave, so
  never reference it from a plugin registry such as `examples/plugins/`.

Both stubs and the fault source double as test fixtures for the Phase 3 host.

These crates are not members of the root filimon workspace. Each has its own
empty `[workspace]` table, its own `Cargo.lock`, and its own toolchain pin.

## Prerequisites

Install the two pinned tools once per machine:

```sh
cargo +stable install cargo-component@0.21.1 --locked
cargo +stable install wasm-tools@1.263.0 --locked
```

Both build from source and take roughly 8 minutes each. The `+stable` flag is
required: the repository root may carry a local `rustup override` for an older
Rust, and wasm-tools needs Rust 1.88 or newer.

Each stub's `rust-toolchain.toml` installs Rust 1.96.0 and the
`wasm32-unknown-unknown` target automatically on first build.

## Build

From the repository root:

```sh
(cd plugins/stub-source && cargo component build --release)
(cd plugins/stub-fetcher && cargo component build --release)
(cd plugins/fault-source && cargo component build --release)
```

The build must run from the crate directory, which the subshells handle:
rustup picks the toolchain from the current directory. Each crate's `rust-toolchain.toml` pins Rust 1.96.0, and its
`.cargo/config.toml` selects the `wasm32-unknown-unknown` build target. The
components land at:

- `plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm`
- `plugins/stub-fetcher/target/wasm32-unknown-unknown/release/stub_fetcher.wasm`
- `plugins/fault-source/target/wasm32-unknown-unknown/release/fault_source.wasm`

## Conformance check

`wasm-tools component targets` confirms that a component matches a world. From
each crate directory, after building:

```sh
# in plugins/stub-source
wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/stub_source.wasm

# in plugins/stub-fetcher
wasm-tools component targets ../../wit -w fetcher target/wasm32-unknown-unknown/release/stub_fetcher.wasm
```

Each command exits 0 on success. Checking a component against the wrong world
fails, for example the stub source against `fetcher`.

To see what a component imports and exports:

```sh
wasm-tools component wit target/wasm32-unknown-unknown/release/stub_source.wasm
```

## Why wasm32-unknown-unknown

`cargo component` builds for `wasm32-wasip1` by default. That build adds WASI
interface imports (`wasi:cli`, `wasi:io`, `wasi:filesystem`, and others) that
neither world declares, so `wasm-tools component targets` rejects it. Building
for `wasm32-unknown-unknown` gives a component that imports only what its
world declares: logging goes through `log` and HTTP through `fetch`.

Guest code on this target has no filesystem, environment, clock, or stdout.
Do not use `std::fs`, `std::env`, or `SystemTime` in a plugin.

Whether the host also accepts components that import WASI is a Phase 3
decision.

## Built from WIT alone

Each crate references only `../../wit` through
`[package.metadata.component.target]` in its `Cargo.toml`. `cargo component`
generates `src/bindings.rs` from the WIT on every build. That file is
gitignored, along with `target/`.

## Plugin secrets

Each source's plugin TOML has a `[host.secrets]` table that maps an alias to
an env var name, for example `client_id = "REDDIT_CLIENT_ID"`. The plugin's
`init` receives only its own aliases, keyed by alias, never the env var names
and never another plugin's secrets.

fili looks each env var up in two places:

1. The process environment, first. A variable set there overrides `.env`,
   even when it is set but empty. In that case fili reports an empty secret,
   names the process environment as the source, and tells you to unset the
   variable or give it a value there.
2. The `.env` file in the same directory as `filimon.toml`.
   [`examples/plugins/.env.example`](../examples/plugins/.env.example) lists
   the names the sample plugins need.

### The `.env` format

fili reads every value literally and never expands `$NAME` or `${NAME}`. It
does not process backslash escapes either. A value reaches the plugin exactly
as written between its delimiters, or its line is rejected.

Each line is blank, a comment, or `KEY=VALUE`. A `KEY` starts with a letter or
`_` and continues with letters, digits, `_` or `.`. Keys are case-sensitive.
An optional `export ` prefix is allowed, as is whitespace around `=`. A value
takes one of four forms:

- Empty: `KEY=`, or `KEY= # comment`.
- In single quotes: `KEY='pa$$word'`. Anything except `'` is kept as
  written, including `$`, backslashes, `"`, `#` and spaces. Use single quotes
  for any value that contains `$`, a backslash, whitespace or a `"`, or that
  starts with `#`.
- In double quotes: `KEY="a b # c"`. The value must not contain `$` or a
  backslash.
- Unquoted: `KEY=abc123`. The value must not contain whitespace, `$`, a
  backslash or quotes, and must not start with `#`.

A comment is a line whose first non-blank character is `#`, or text after
whitespace that follows a value (`KEY=abc # note`). LF and CRLF line endings
both work.

fili rejects these lines:

- `$` or a backslash in an unquoted or double-quoted value.
- A quote inside an unquoted value (`KEY=it's`).
- An unquoted value that starts with `#` straight after `=`.
- Text after the value that is not a comment (`KEY=a b`, `KEY='a'b`).
- A quote that is not closed on the same line. Multi-line values are not
  supported.
- A line that is not blank, not a comment and not `KEY=VALUE`.
- A duplicate key, that is a key already set on an earlier line.

Each rejected line is reported with its line number and a reason, never with
its text, because the text can be a secret. One bad line makes the whole file
unusable, and every bad line is listed. Keep fili's `.env` separate from
`.env` files written for shell-style tools, which use rules fili rejects.

A value that holds `'` together with `$` or a backslash cannot be written in
`.env`. Set it in the process environment instead, which overrides `.env`.

## Host behavior

The component host (`src/plugins/host/`) loads plugins from the registry, polls
each enabled source on its own task, turns what they return into `SourceItem`
records, produces those to Fluvio and remembers where each plugin stopped. It is
a library in this phase: `Host::load` and `Host::run`. The `fili` command that
starts it arrives in Phase 4, which also reads `--state-dir`.

### Loading

- Only components listed in the registry and enabled are opened. Nothing is
  discovered by scanning a directory, so a `.wasm` file that is not in
  `filimon.toml` is never read.
- A component that imports anything outside the `filimon:plugin/` package is
  rejected before it runs, with an error that names the imports. A WASI build
  (the `cargo component` default target) is the usual cause. The host links no
  WASI at all. Build for `wasm32-unknown-unknown` as described above.
- A component that does not fit the world it was loaded as (a fetcher loaded as
  a source, say) is rejected with a `wrong world` error.
- One bad component never stops the others. Load failures are listed in the
  host report and the remaining plugins run.

### Polling and backoff

- A plugin is polled immediately when the host starts. After that the wait is a
  fixed delay: `poll_interval_secs` after the previous cycle (poll, send and
  cursor write) finished.
- After a `transient`, `rate-limited` or `auth-required` error, or a trap, the
  plugin waits `initial_secs * multiplier^n` (`n` is the number of consecutive
  failures so far), never more than `max_secs`. `[host.backoff]` sets these
  values for each plugin; the defaults are 60 s, 3600 s and 2.0.
- A `rate-limited` error that carries a retry-after waits the larger of the
  retry-after and the current step, still capped at `max_secs`.
- One successful poll resets the backoff completely. After a backoff window the
  plugin is polled again and then returns to its normal interval.
- There is no jitter. The backoff window is not compared with the poll
  interval, so it can be shorter than `poll_interval_secs` (a locked decision,
  D-02).

### Failures

- `fatal` and `config` errors, from `init` or from `poll`, disable that plugin
  until `fili` restarts. The host logs the plugin name and the reason at error
  level (`plugin disabled until fili restarts`).
- A trap (or any wasmtime-level failure) discards the instance. After the
  backoff window the host builds a new instance and calls `init` again, so a
  plugin's in-memory state starts over. A failed `init` is handled the same
  way, except that `fatal` and `config` still disable.
- A failure of the item sink (for example the Fluvio broker is down) is the
  host's problem, not the plugin's. It uses the same backoff, keeps the
  instance and polls again from the same cursor. The sink gives one batch 60 s
  to send and flush, counted from the moment it holds the producer. On expiry
  it clears the producer's stored error and reports `send timed out`, with no
  cursor write.
- Each instance has a fixed 64 MiB memory ceiling. A guest that tries to grow
  past it traps, and the host treats that like any other trap: the instance is
  discarded and rebuilt after the backoff window. The process and the other
  plugins are not affected. The same store also caps a component at a few
  core instances, tables and memories.
- A `next-cursor` over 4096 bytes is a plugin fault: nothing from that poll is
  sent, the cursor is not written, and the plugin is disabled with a `fatal`
  error that names the size and the cap (the same instance and cursor would
  reproduce it).
- A panic inside one plugin's host task is caught and reported for that plugin
  only (`plugin task panicked`).
- Every other plugin and the process keep running. Log lines go to stderr and
  carry a `plugin` field. The default filter is `fili::plugins=info`, and
  `RUST_LOG` replaces it when it holds a valid, non-blank value.

### Cursor state

- Each plugin has one file, `<state dir>/<alias>.json`, with `version`,
  `plugin` and `cursor` (base64, or null).
- The state directory is `.fili-state/` next to the registry file. Override it
  with `--state-dir <path>` or the `FILI_STATE_DIR` environment variable.
- The file is written atomically (temporary file, then rename). The cursor is
  written only after the items of that poll were produced and flushed, so
  delivery is at-least-once: a crash between the send and the write replays the
  last batch on the next start. There is no dedupe in this phase.
- A missing file means a first poll. A corrupt or unreadable file, a file that
  names another plugin, or a file with a newer version disables only that
  plugin, with an error that names the file. The host never rewrites or deletes
  it. To reset a plugin's cursor, stop `fili` and delete its file.
- A `.lock` file in the state directory is held for the host's lifetime, so two
  `fili` processes cannot share one state directory.

### Items

- The host drops an item with an empty id, an empty or unparsable url, a
  message key (`<platform>:<id>`) over 512 bytes, or a record over 1,000,000
  bytes. The record size is the serialized item plus its key, because Fluvio
  sizes the whole record. The warning names the plugin, the item index and a
  fixed reason, never the item's content. A dropped item does not fail the poll
  or hold back the cursor.
- The host takes at most 1000 items from one poll. Later items are dropped and
  counted, with one warning per poll that carries the count and the fixed
  reason `too-many-items`. The cursor still advances, so the dropped items are
  not retried: a plugin should page its own results to stay under the cap.
- An empty `platform` falls back to the plugin's registry name.
- Items go to the `filimon-social-items` topic (created by hand, as the root
  README describes) with the key `<platform>:<id>`, in the order the plugin
  returned them. The key assumes a single-partition topic. A missing topic is
  reported with the `fluvio topic create` command.

### Interim imports

Until the fetcher and OAuth phases land, the `fetch` import returns
`not-configured` for every request, and `poll` receives no access token (`none`).
A source that needs HTTP or a token cannot get far yet. The guest `log` import
is live: guest log lines become host log events with a `plugin` field, cleaned
of control characters and clipped.

### Known limits

- Fixed safety caps only: 64 MiB of memory per instance, 1000 items per poll
  and 4096 cursor bytes. Configurable per-plugin CPU and memory limits stay
  deferred (v2 HOST-06), and there is no CPU limit at all. A guest can still
  log without a rate limit.
- A guest that loops forever is never terminated while the host runs. It does
  not block other plugins or shutdown: cancelling the host returns within
  seconds because the stuck call is dropped. But the spinning plugin itself
  stays stuck inside its poll and is never polled again.
- No dedupe across polls (v2 HOST-07). A plugin that returns the same item twice
  produces it twice, and a replay after a crash can duplicate the last batch.
- `timeout_secs` and `allowlist` are read from the registry but only enforced
  when real fetch routing lands, not yet.
- The last poll's status is not persisted. The state file is versioned and
  tolerant of unknown keys so Phase 4 can add it.

### Running the host tests

From the repository root. A group that cannot run is not a pass: report it as
not run.

```sh
# 1. No wasm, no cluster.
cargo test -p filimon plugins::host

# 2. Real components. Needs the three fixtures built with the build lines above.
cargo test -p filimon -- --ignored wasm_

# 3. Real cluster. Needs a local Fluvio cluster; uses scratch topics only.
cargo test -p filimon -- --ignored live_
```

The live group creates its own `filimon-test-...` topics and deletes them, also
when a test fails. It never touches `filimon-social-items` or the other real
topics. Group 3 also needs the `stub-source` fixture. Each group filter is a
single substring on purpose: libtest ORs several filters, so adding a path
filter beside `wasm_` or `live_` would run the other group as well.

## Contract version

The contract is `filimon:plugin@0.1.0`. It stays at 0.x until the Reddit and
YouTube plugins validate it. Every `wit/*.wit` file repeats the same line,
`package filimon:plugin@0.1.0;`, so a version bump edits all of them.

## If cargo-component 0.21.1 stops building

cargo-component is marked experimental and has not released since 2025-04.
The fallback is `cargo build --target wasm32-wasip2` with the `wit-bindgen`
macro. Switching to it changes the build and conformance requirements, so it
needs user sign-off first.
