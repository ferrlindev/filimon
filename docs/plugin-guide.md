# Writing filimon plugins

<!-- wit-tag: wit-v0.1.0 -->

This guide takes you from nothing to a source plugin and a fetcher plugin that
an unmodified `fili` loads, checks and runs. Every command in it is a command
that [`scripts/verify-authoring.sh`](../scripts/verify-authoring.sh) runs, so
the guide is tested by that script (see [Prove it works](#prove-it-works)).

The WIT contract is published as the git tag `wit-v0.1.0` of the filimon
repository. Host behavior, the full plugin CLI and OAuth are described once, in
[`plugins/README.md`](../plugins/README.md); this guide links there instead of
repeating them.

## What you will build

- A **source plugin**: a WebAssembly component that polls one platform and
  returns items (`world source`).
- A **fetcher plugin**: a WebAssembly component that serves the HTTP requests a
  source makes (`world fetcher`).

Both are built against only the published WIT package `filimon:plugin@0.1.0`,
listed in a `filimon.toml` registry and loaded by an unmodified `fili`. You
never rebuild `fili` and you never touch its source.

## Prerequisites

Install the two pinned tools once per machine:

```sh
cargo +stable install cargo-component@0.21.1 --locked
cargo +stable install wasm-tools@1.263.0 --locked
```

Both build from source and take several minutes each. The crates you copy carry
a `rust-toolchain.toml` that pins Rust 1.96.0 and adds the
`wasm32-unknown-unknown` target, so rustup installs them on your first build.
You also need `git` and a built `fili` binary (`cargo build -p filimon` in a
filimon checkout, or a binary you were given).

## Get the template and the WIT

The WIT is distributed by git tag. There is no registry and no `wkg` or `warg`
step: you clone a pinned tag, check out only the directories you need, and copy
plain files.

```sh
git clone --depth 1 --branch wit-v0.1.0 --filter=blob:none --sparse https://github.com/ferrlindev/filimon.git filimon-authoring
cd filimon-authoring
git sparse-checkout set wit plugins/template plugins/stub-fetcher
cd ..
```

Make your own source crate from the template and put the `wit` directory beside
its `Cargo.toml`:

```sh
cp -R filimon-authoring/plugins/template my-source
cp -R filimon-authoring/wit my-source/wit
```

Then make three edits to `my-source/Cargo.toml`:

1. In `[package.metadata.component.target]`, change `path = "../../wit"` to
   `path = "wit"`.
2. Change the crate name `name = "template-source"` to your own, here
   `name = "my-source"`.
3. Change the component package `package = "filimon:template-source"` to
   `<namespace>:<name>`, here `package = "authors:my-source"`.

These are the same copy, rename and rewrite steps `scripts/verify-authoring.sh`
performs, and it checks that no `../..` path is left in `Cargo.toml` afterwards.

## Why the build target matters

`cargo component` builds for `wasm32-wasip1` by default. That adds WASI imports
(`wasi:cli`, `wasi:filesystem` and others) that neither world declares, and the
host rejects such a component with an error that names the imports. Components
for filimon target `wasm32-unknown-unknown`, which imports only what the world
declares: logging through `log` and HTTP through `fetch`.

Two files in the template select that target, and you keep both:

- `.cargo/config.toml` sets `[build] target = "wasm32-unknown-unknown"`.
- `rust-toolchain.toml` pins Rust 1.96.0 and installs that target.

Guest code on this target has no filesystem, environment, clock or stdout. Do
not use `std::fs`, `std::env` or `SystemTime`.

## The source plugin

Open [`plugins/template/src/lib.rs`](../plugins/template/src/lib.rs); the lines
you change are marked `EDIT:`. A source exports two functions.

- **`init(settings, secrets)`** runs once per instance, before any `poll`.
  `settings` is the raw text of the `[settings]` table of your plugin TOML; you
  parse and validate it. The template uses `serde` with `deny_unknown_fields`, so
  a typo becomes a `config` error instead of a silent default. `poll` receives no
  settings, so `init` keeps them (the template uses a `thread_local!`). The host
  keeps one long-lived instance across polls and builds a new one, calling `init`
  again, after a failure, so your state is rebuilt whenever it has to be.
- **`poll(cursor, access-token)`** returns a `poll-response`: a list of items and
  an optional `next-cursor`.

Secrets arrive in `init` as `(alias, value)` pairs for only the aliases your
plugin TOML declares. Log alias names if you must, never values.

The cursor is one opaque byte string. The host stores it after the items were
produced and hands it back unchanged on the next poll; `none` means a first
poll. Keep it small: a cursor over 4096 bytes is a plugin fault.

An `item` has `id` (stable on the platform; the message key is
`<platform>:<id>`, and the host does not de-duplicate across polls), `url`, optional `title`,
`text`, optional `author`, optional `published` (Unix epoch milliseconds, since a
`wasm32-unknown-unknown` component has no clock), `platform` (a short name) and
optional `raw` (platform metadata as JSON). The host drops an item with an empty
id or an unusable url.

When `init` or `poll` fails you return a `poll-error`, and the host reacts per
variant:

| Variant | What the host does |
|---------|--------------------|
| `transient(message)` | Backs off and retries. |
| `rate-limited(retry-after)` | Backs off, waiting at least the retry-after when you give one. |
| `auth-required` | Backs off; the plugin needs a login (see [OAuth](../plugins/README.md#oauth)). |
| `config(message)` | Disables the plugin until `fili` restarts. Use it for bad settings or secrets. |
| `fatal(message)` | Disables the plugin until `fili` restarts. |

Two imports are available. `log(level, message)` writes a host log line tagged
with your plugin name. `fetch(request)` performs an HTTP request through the
fetcher the operator assigned to your source; the host checks the url against
your source's allowlist first. A source with no `fetcher` key in the registry
gets `fetch-error::not-configured` for every `fetch` call. A template source
with no `fetch_url` setting makes no `fetch` call at all, needs no fetcher and
still runs.

The host holds each instance to fixed safety caps: 64 MiB of memory, at most
1000 items per poll and 4096 cursor bytes. See
[Known limits](../plugins/README.md#known-limits) for the rest.

## Build and check

From the crate directory (rustup picks the toolchain from the current
directory):

```sh
cd my-source
cargo component build --release
```

The component lands at
`target/wasm32-unknown-unknown/release/my_source.wasm`. Check that it fits the
`source` world:

```sh
wasm-tools component targets wit -w source target/wasm32-unknown-unknown/release/my_source.wasm
cd ..
```

The command exits 0 on success. `wasm-tools component wit <wasm>` lists what the
component imports and exports; it must show no `wasi:` import.

## Write the plugin TOML

Each source has a plugin TOML. Start from
[`plugins/template/template.toml`](../plugins/template/template.toml).

```toml
[host]
poll_interval_secs = 60
allowlist = []

[settings]
greeting = "hello from my first plugin"
```

- `[host]` is read by the host: `poll_interval_secs` (the wait between polls),
  `timeout_secs` (one deadline per fetch call, default 30), `allowlist` (the exact
  host names `fetch` may reach; empty means none) and an optional `[host.backoff]`
  table with `initial_secs`, `max_secs` and `multiplier`.
- `[host.secrets]` maps an alias to the name of an environment variable, for
  example `api_key = "MY_SERVICE_API_KEY"`. The value lives in a `.env` file next
  to `filimon.toml` or in the process environment, never in the TOML. Your `init`
  receives the value under the alias.
- `[host.oauth]` declares an OAuth login the host manages for you; see
  [OAuth](../plugins/README.md#oauth).
- `[settings]` is passed raw to your `init`. Its keys are yours.

## Register it

Create a `filimon.toml` registry. Paths resolve relative to the file's
directory:

```toml
[sources.my-source]
component = "my-source/target/wasm32-unknown-unknown/release/my_source.wasm"
config = "my-source.toml"
enabled = true
# fetcher = "<fetcher name>"   # needed only when the source calls fetch
```

Put your plugin TOML beside it as `my-source.toml`. A complete sample registry is
[`examples/plugins/filimon.toml`](../examples/plugins/filimon.toml). Then:

```sh
fili --registry filimon.toml plugins check my-source
fili --registry filimon.toml plugins run-once my-source --json
```

`plugins check` compiles the component, checks it against its world with the
host's own preflight and loads its TOML and secrets, without calling `init` or
`poll`; it prints `component: ok` when the component is sound. `plugins run-once`
polls the source once and prints each item as one JSON line. It is a dry run:
nothing is produced and no cursor is saved (add `--produce` to send the batch).
When both look right, start the host with `fili --registry filimon.toml run`,
which polls every enabled source on its own interval. The full CLI is described
in [Plugin CLI](../plugins/README.md#plugin-cli).

## Write a fetcher plugin

A fetcher is the plugin a source's `fetch` calls go to. The `fetcher` world
exports `fetch(request)` and imports the host's `outbound-http.send`, the only
way out to the network. A fetcher has no `init` and no settings.

This walkthrough starts from `plugins/stub-fetcher`, which you already have from
the sparse checkout, and replaces what `fetch` does with a fixed canned answer.
Copy the crate and the WIT:

```sh
cp -R filimon-authoring/plugins/stub-fetcher canned-fetcher
cp -R filimon-authoring/wit canned-fetcher/wit
```

Make the same three edits to `canned-fetcher/Cargo.toml` as for the source:
`path = "../../wit"` becomes `path = "wit"`, `name = "stub-fetcher"` becomes
`name = "canned-fetcher"`, and `package = "filimon:stub-fetcher"` becomes
`package = "authors:canned-fetcher"`.

The copied `src/lib.rs` belongs to the repository's test suite (it has modes that
exist only to test the host), so replace the whole file with your own `fetch`:

```rust
#[allow(warnings)]
mod bindings;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::types::{FetchError, Request, Response};

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        if request.url.contains("canned.example/hello") {
            return Ok(Response {
                status: 200,
                headers: vec![("content-type".to_string(), "application/json".to_string())],
                body: br#"{"canned":true}"#.to_vec(),
            });
        }
        Ok(Response {
            status: 404,
            headers: vec![],
            body: br#"{"canned":"no fixture"}"#.to_vec(),
        })
    }
}

bindings::export!(Component with_types_in bindings);
```

Build it and check it against the `fetcher` world:

```sh
cd canned-fetcher
cargo component build --release
wasm-tools component targets wit -w fetcher target/wasm32-unknown-unknown/release/canned_fetcher.wasm
cd ..
```

Register it under `[fetchers.<name>]` with its `component`, and assign it to a
source with `fetcher = "<name>"` under that source's `[sources.<name>]` entry.
Point the template source at a host in its allowlist:

```toml
[sources.my-source]
component = "my-source/target/wasm32-unknown-unknown/release/my_source.wasm"
config = "my-source.toml"
fetcher = "canned"

[fetchers.canned]
component = "canned-fetcher/target/wasm32-unknown-unknown/release/canned_fetcher.wasm"
```

```toml
[host]
poll_interval_secs = 60
allowlist = ["canned.example"]

[settings]
greeting = "hello from my first plugin"
fetch_url = "https://canned.example/hello"
```

Now `plugins run-once` shows the canned answer in the item's text:

```sh
fili --registry filimon.toml plugins run-once my-source --json
```

```text
"text":"status 200: {\"canned\":true}"
```

What this shows:

- A url your `fetch` does not match gets the answer you chose for it. Change
  `fetch_url` to `https://canned.example/other` and the item text becomes
  `status 404: {"canned":"no fixture"}`.
- The assignment decides who serves the request. Remove the `fetcher` key and
  the host answers `fetch-error::not-configured`; the template turns that into a
  transient poll error, so `run-once` exits 1 and prints no canned answer.
- A fetcher keeps no state. Every call runs in a fresh instance, so two
  `run-once` calls are independent and give the same answer for the same url.
- The host still enforces the source's allowlist and timeout whichever fetcher
  answers: `canned.example` has to be in the allowlist even though this fetcher
  never leaves the sandbox. A fetcher that does reach the network calls the
  imported `outbound-http.send`, and the host holds that call to the same
  allowlist (see [Fetcher layer](../plugins/README.md#fetcher-layer)).

**Trust.** A fetcher sees every header of the requests it serves, `authorization`
and `cookie` included, and can read the response. Register only fetchers you
trust. The host holds a fetcher's own sends to the calling source's allowlist, so
it cannot reach a host the source may not, but it can pass what it sees to an
allowlisted host.

The repository also holds `plugins/replay-fetcher`, a table-driven fetcher whose
only edited file is `src/fixtures.rs`, used by the host's end-to-end replay
tests. Never register the replay fetcher for a real source: it serves frozen
fixtures as if they were live data. The stub fetcher is a test fixture too; you
copied it only as scaffolding, and your renamed crate is your own.

## Prove it works

[`scripts/verify-authoring.sh`](../scripts/verify-authoring.sh) is the
acceptance test of this guide. From a filimon checkout with a built `fili`
(`cargo build -p filimon`):

```sh
scripts/verify-authoring.sh
```

It works in a temporary directory outside the repository, from the working tree
and `git archive HEAD wit`, and needs no network. To run the same proof from the
tag, the way an outside author gets it:

```sh
WIT_REF=wit-v0.1.0 scripts/verify-authoring.sh
```

It has two legs (`LEGS="source"` or `LEGS="fetcher"` runs one):

- The **source** leg copies the template and the WIT, renames the crate, builds
  it, registers it in a standalone `filimon.toml` and shows `plugins check` and
  `plugins run-once` accept it.
- The **fetcher** leg copies the stub fetcher, renames it, replaces `fetch` with
  the canned answer above, builds it, registers and assigns it, and shows the
  canned 200, the 404 for an unmatched url, and that without the `fetcher` key the
  canned answer does not appear.

It ends by checking that the `fili` binary and the repository tree are
unchanged, then prints `AUTHORING PROOF: PASS (legs: source fetcher)`. Nothing it
fetches is run or evaluated: it only copies files and builds the copied crates.

## Troubleshooting

- **The host rejects the component and names `wasi:` imports.** You built for
  `wasm32-wasip1`. Keep `.cargo/config.toml` and `rust-toolchain.toml` from the
  template, build from the crate directory, and check with
  `wasm-tools component wit <wasm>`.
- **`not-configured`.** The source calls `fetch` but has no `fetcher` key in the
  registry. Add `fetcher = "<name>"` to its `[sources.<name>]` entry.
- **A source does not load and the report names its fetcher.** The assigned
  fetcher's component is missing, invalid or does not fit the `fetcher` world.
  The host report and `plugins run-once` name both the source and the fetcher with
  the fetcher's own failure as the reason. Check the fetcher with
  `plugins check <fetcher name>`.
- **The registry is rejected when it is read.** A source names a fetcher that is
  not defined under `[fetchers.<name>]`, or that is disabled. Define it or fix the
  name.
- **`denied`.** The url's host is not in the source's `[host] allowlist`. Matching
  is exact and case-insensitive, with no wildcards.
