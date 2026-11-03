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

Both stubs double as test fixtures for the Phase 3 host.

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
```

The build must run from the crate directory, which the subshells handle:
rustup picks the toolchain from the current directory. Each crate's `rust-toolchain.toml` pins Rust 1.96.0, and its
`.cargo/config.toml` selects the `wasm32-unknown-unknown` build target. The
components land at:

- `plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm`
- `plugins/stub-fetcher/target/wasm32-unknown-unknown/release/stub_fetcher.wasm`

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

## Contract version

The contract is `filimon:plugin@0.1.0`. It stays at 0.x until the Reddit and
YouTube plugins validate it. Every `wit/*.wit` file repeats the same line,
`package filimon:plugin@0.1.0;`, so a version bump edits all of them.

## If cargo-component 0.21.1 stops building

cargo-component is marked experimental and has not released since 2025-04.
The fallback is `cargo build --target wasm32-wasip2` with the `wit-bindgen`
macro. Switching to it changes the build and conformance requirements, so it
needs user sign-off first.
