# filimon plugins

This directory holds WebAssembly component plugins built against the filimon
plugin contract in [`../wit`](../wit) (package `filimon:plugin@0.1.0`).

## What lives here

- `stub-source/`: a minimal plugin for the `source` world. `init` parses its
  `[settings]` TOML, logs a greeting and the secret alias names (never the
  values), and calls the imported `fetch` when `fetch_url` is set (an optional
  `fetch_headers` table is sent with that request; it exists for host tests
  only, to steer the test fetcher through a header such as `x-stub-mode`, and
  is not meant for a registry). `poll`
  returns one hardcoded item and advances a one-byte cursor.
- `stub-fetcher/`: a minimal plugin for the `fetcher` world. `fetch` logs the
  request URL and returns a canned `200` JSON response, unless the request
  carries an `x-stub-mode` header: `spin` never returns and `forward:<url>`
  calls `outbound-http.send` for that url and returns its result. Those modes
  exist for host tests only (a fetcher that outlives its deadline, a fetcher
  that tries to reach a host the source may not); never register it for a real
  source or reference the modes from `examples/plugins/`.
- `http-fetcher/`: the default fetcher, a plugin for the `fetcher` world.
  `fetch` forwards the request to the host's `outbound-http.send` and returns
  the answer unchanged. It adds no header, follows no redirect and logs no url,
  header or body. The host, not the component, enforces TLS validation, the
  allowlist, the timeout, redirects and size limits. It is a normal registry
  entry (`[fetchers.http]` in `examples/plugins/filimon.toml`), not part of the
  `fili` binary.
- `fault-source/`: a fault-injection plugin for the `source` world and a
  Phase 3 test fixture, not a registry example. Its `[settings]` `mode`
  selects `ok`, `rate-limited`, `transient`, `auth-required`, `fatal`,
  `config`, `init-fatal`, `trap`, `spin` or `grow` (allocates `grow_mib`,
  default 192, past the host's 64 MiB memory cap). Optional `retry_after_secs`
  (used by `rate-limited`) and `after_polls` (polls up to that count behave as
  `ok`) refine it. The `trap` and `spin` modes deliberately misbehave, so
  never reference it from a plugin registry such as `examples/plugins/`.

Both stubs and the fault source double as test fixtures for the Phase 3 host.
The default fetcher is not a fixture: it is the fetcher that ships.

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
(cd plugins/http-fetcher && cargo component build --release)
```

The build must run from the crate directory, which the subshells handle:
rustup picks the toolchain from the current directory. Each crate's `rust-toolchain.toml` pins Rust 1.96.0, and its
`.cargo/config.toml` selects the `wasm32-unknown-unknown` build target. The
components land at:

- `plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm`
- `plugins/stub-fetcher/target/wasm32-unknown-unknown/release/stub_fetcher.wasm`
- `plugins/fault-source/target/wasm32-unknown-unknown/release/fault_source.wasm`
- `plugins/http-fetcher/target/wasm32-unknown-unknown/release/http_fetcher.wasm`

## Conformance check

`wasm-tools component targets` confirms that a component matches a world. From
each crate directory, after building:

```sh
# in plugins/stub-source
wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/stub_source.wasm

# in plugins/stub-fetcher
wasm-tools component targets ../../wit -w fetcher target/wasm32-unknown-unknown/release/stub_fetcher.wasm

# in plugins/http-fetcher
wasm-tools component targets ../../wit -w fetcher target/wasm32-unknown-unknown/release/http_fetcher.wasm
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
records, produces those to Fluvio and remembers where each plugin stopped.
`fili run` starts it, and `fili plugins list`, `check` and `run-once` inspect
and try plugins without running it (see [Plugin CLI](#plugin-cli)).
`--state-dir` and `--registry` apply to all of them.

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
- Stopping is bounded. After Ctrl-C or SIGTERM the host waits up to 10 seconds
  for in-flight sends to finish, then aborts the tasks still running and
  reports each as `aborted at shutdown`. A second signal during that wait
  aborts at once. An aborted send may replay its last batch on the next start,
  because the cursor is written only after the send (at-least-once). A guest
  that spins inside a call is still dropped promptly when the host stops.

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
- Each source also has `<state dir>/<alias>.status.json` with `version`,
  `plugin`, `at` (RFC 3339, UTC), `ok`, `error` and `items`. The scheduler
  writes it after every cycle, separately from the cursor file, so a status
  write can never touch the cursor. `fili plugins list` reads it without taking
  the lock, which is why it works while `fili run` is up.

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

### Imports

- `fetch` is answered by the source's assigned fetcher (see
  [Fetcher layer](#fetcher-layer)). It answers `not-configured` only for a
  source that has no `fetcher` key.
- `poll` receives the plugin's current access token, or `none` when the plugin
  has no login (see [OAuth](#oauth)).
- The guest `log` import is live: guest log lines become host log events with a
  `plugin` field, cleaned of control characters and clipped.

### Known limits

- Fixed safety caps only: 64 MiB of memory per instance, 1000 items per poll
  and 4096 cursor bytes. Configurable per-plugin CPU and memory limits stay
  deferred (v2 HOST-06), and there is no CPU limit at all. A guest can still
  log without a rate limit.
- A guest that loops forever is never terminated while the host runs. It does
  not block other plugins or shutdown: when the host stops, the stuck call is
  dropped at once. A stuck broker is bounded the same way: the host waits at
  most 10 seconds for in-flight sends, then aborts them (a second signal aborts
  at once), and an aborted send may replay its batch on the next start. But the
  spinning plugin itself stays stuck inside its poll and is never polled again.
- No dedupe across polls (v2 HOST-07). A plugin that returns the same item twice
  produces it twice, and a replay after a crash can duplicate the last batch.
- The allowlist is the operator's list of host names, not an address filter.
  The host compares the name in the url and does not look at the address the
  name resolves to, so DNS rebinding (or any private or loopback address behind
  an allowlisted name) is not blocked. Allowlist only names you control or
  trust.
- A fetcher sees every header of the requests it serves, `authorization` and
  `cookie` included, and can read the response. Register only fetchers you
  trust. Its own sends are held to the calling source's allowlist, so it cannot
  reach a host the source may not, but it can still pass what it sees to an
  allowlisted host.
- There is no per-source request rate limiter. A `429` reaches the plugin as an
  ordinary response, with its status and headers (a `Retry-After` included),
  and the plugin decides what to do with it.
- The host trusts the certificate roots bundled with its HTTP client, not the
  operating system's store. Behind a TLS-intercepting proxy whose CA is only in
  the system store, every https request fails with a certificate error. The
  host has no setting to add a root.
- The last poll's status is kept in a separate file, `<alias>.status.json`,
  which only `fili plugins list` reads (see Cursor state). It describes the
  most recent cycle only; there is no history.

### Running the host tests

From the repository root. A group that cannot run is not a pass: report it as
not run.

```sh
# 1. No wasm, no cluster.
cargo test -p filimon plugins::host

# 2. Real components. Needs the four fixtures built with the build lines above.
cargo test -p filimon -- --ignored wasm_

# 3. Real cluster. Needs a local Fluvio cluster; uses scratch topics only.
cargo test -p filimon -- --ignored live_

# 4. TLS. Needs the `openssl` binary (it makes throwaway certificates).
cargo test -p filimon -- --ignored tls_

# 5. Public internet. Needs outbound access to the network.
cargo test -p filimon -- --ignored net_
```

The live group creates its own `filimon-test-...` topics and deletes them, also
when a test fails. It never touches `filimon-social-items` or the other real
topics. Group 3 also needs the `stub-source` fixture. Group 4 needs the
`http_fetcher.wasm` fixture for one test and a local certificate authority made
with `openssl`; a machine without the binary fails the group, so report it as
not run rather than passed. Group 5 reaches real sites, so it is off by default and depends on
the network and on those sites staying up. Each group filter is a single
substring on purpose: libtest ORs several filters, so adding a path filter
beside `wasm_`, `live_`, `tls_` or `net_` would run another group as well.

## Fetcher layer

A source never opens a connection itself. Its `fetch` import goes to the
fetcher the registry assigns to it, and the fetcher goes out through the
host's `outbound-http.send` import. The host judges both calls, so the rules
below hold whichever fetcher is in charge.

### Assigning a fetcher

- Define a fetcher under `[fetchers.<name>]` in the registry and point a source
  at it with `fetcher = "<name>"` under `[sources.<name>]`. See
  [`examples/plugins/filimon.toml`](../examples/plugins/filimon.toml).
- There is no implicit default. [`http-fetcher/`](http-fetcher) is the default
  fetcher in the sense that the sample registry uses it as `[fetchers.http]`:
  it forwards every request to `outbound-http.send` and adds nothing. A source
  with no `fetcher` key gets `not-configured` for every `fetch` call.
- Changing the key changes who serves that source's requests. Nothing is
  rebuilt: restart `fili` and the next call goes to the new component.
- A source whose assigned fetcher cannot be loaded (the component is missing,
  invalid, or does not fit the `fetcher` world) is not loaded at all. The host
  report and `fili plugins run-once` name both the source and the fetcher,
  with the fetcher's own failure as the reason. A registry that names an
  undefined or disabled fetcher is rejected when the registry is read.
- Every call runs in a fresh fetcher instance, which is dropped when the call
  ends. A fetcher has no state between calls, and one component may serve
  several sources, so no instance is ever re-used and nothing a fetcher stores
  in memory can reach another call or another source.

### What the host enforces

The same checks run when a source calls `fetch` and again when the fetcher
calls `send`, with the calling source's allowlist and deadline in both cases.

| Rule | Value |
|------|-------|
| Allowlist | The url's host must equal an entry of the source's `[host] allowlist`. Matching is exact and case-insensitive, one trailing dot is ignored, any port matches, and there are no wildcards or subdomain matching. An entry that is not a bare host name never matches (a `[host]` entry like that is a registry error). |
| Schemes | `http` and `https` only. Anything else is `invalid-request`. |
| Deadline | One per call, from `[host] timeout_secs` (default 30). It covers fetcher start-up, every redirect hop and the response body read. The fetcher is dropped when it passes. |
| Redirects | At most five redirects (`MAX_REDIRECTS`). The host follows them itself, checks the allowlist on every hop before it connects, and removes `authorization`, `cookie` and `proxy-authorization` when a hop changes origin. A `303`, and a `301` or `302` after a `POST`, becomes a `GET` without body or `content-type`. A sixth redirect is a `transport` error. |
| Request headers | At most 100 headers and 64 KiB of header text (names plus values). `host`, `content-length`, `transfer-encoding`, `connection`, `proxy-connection`, `upgrade`, `te` and `trailer` are refused. |
| Request size | A url of at most 8 KiB and a body of at most 1 MiB. |
| Methods | The standard methods and `other(...)` for a valid method token. `CONNECT` and `TRACE` are refused in any letter case. |
| Response size | At most 8 MiB (`MAX_RESPONSE_BYTES`). A larger declared length, or a body that passes the cap while it is read, is a `transport` error. |
| Rate limiting | None. A `429` is returned as an ordinary response. |
| Certificates | Validated, with the roots bundled with the HTTP client. The operating system's store is not read, so a TLS-intercepting proxy CA is not trusted and its connections fail as `transport` errors. |
| Logs | Host log lines name the consumer, method, host, status and body length only: never a path, a query, a header or a body. A denial names the host and the consumer, never the rest of the url. |

Errors a plugin can see:

- `invalid-request`: the url is empty, unparsable, too long, has no host or has
  a scheme other than `http`/`https`; a header is malformed, forbidden, too
  many or too large; the body is too large; the method is not allowed.
- `denied`: the host of the url, or of a redirect target, is not in the
  allowlist. No connection is made.
- `timeout`: the call's single deadline passed.
- `transport`: a connection, DNS or certificate failure, a response over the
  cap, too many redirects, a redirect without a usable `Location`, or a
  fetcher that could not start or trapped.
- `not-configured`: the source has no fetcher.

Header order between different names is not specified. Repeated headers with
the same name keep their order.

### The crawler

`fili produce` can fetch its pages through a fetcher component. Add a
`[crawler]` table to the registry:

```toml
[crawler]
fetcher = "http"        # a [fetchers.<name>] entry that is enabled
timeout_secs = 15       # optional, default 15, one deadline per page
allowlist = []          # optional, hosts allowed besides the seed urls' hosts
```

- The crawler is a consumer of its own, labelled `crawler`. It may reach the
  hosts of the seed urls given to `fili produce` and the extra `allowlist`
  hosts, under the same rules as a source (the table above).
- A page link that redirects to another host is denied, naming the host, and
  that page is reported and skipped like any other failed page.
- The built-in HTTP client is used only when there is no registry file or the
  registry has no `[crawler]` table. `fili produce` prints which one it uses.
- A `[crawler]` table that cannot be used is an error, never a silent fall
  back: an undefined or disabled `fetcher`, or a fetcher component that
  cannot be loaded, stops `fili produce` before it crawls.
- The built-in client keeps its existing behaviour, including its own
  certificate handling. Only the fetcher path validates certificates.

## Plugin CLI

All of these read the registry (`--registry <path>`, default `filimon.toml`, or
`FILI_REGISTRY`) and, where they touch state, the state directory
(`--state-dir <path>`, default `.fili-state/` next to the registry, or
`FILI_STATE_DIR`). Both flags work before or after the subcommand. An empty or
blank `--state-dir` is a usage error. Exit codes: 0 success, 1 failure, 2 usage
error (an unknown flag, a missing name). The `fili plugins` commands and
`fili run` print no banner, so stdout holds only what the command wrote.

### `fili plugins list [--json]`

One row per registered plugin, sources first and then fetchers, each in name
order: `NAME`, `KIND`, `ENABLED`, `FETCHER` and `LAST POLL`. `LAST POLL` is the
status file's time and outcome (`ok (3 items)` or `error: <text>`), `-` for a
source that has not been polled yet, and `n/a` for a fetcher. A status file that
cannot be read shows as `unreadable (...)`. `--json` prints one compact JSON
array: `name`, `kind`, `enabled`, `fetcher` and a `last_poll` object with
`state` (`never`, `not-applicable`, `unreadable`, `ok` or `error`), `at`,
`items` and `error`. `list` loads no component and no secret, never takes the
state-directory lock and creates nothing, so it works while `fili run` is up.

### `fili plugins check <name>`

Validates one plugin without polling it: the component is compiled and checked
against its WIT world with the host's own preflight, and the plugin's TOML and
secrets go through the same loader as a run (only secret names are printed,
never values). `init` and `poll` are never called. A disabled source has its
component checked and its config skipped; a fetcher has no config file. The
result is a short summary on stdout, any diagnostics on stderr, and exit 1 when
a part failed.

### `fili plugins run-once <name> [--produce] [--json]`

Polls one enabled source once and prints the items it returned. It is a dry run
by default: it starts from the saved cursor, reads it without the lock, sends
nothing, saves nothing and does not create the state directory. `--produce`
takes the state-directory lock, polls, sends the batch to `filimon-social-items`
and only then saves the cursor, so the next `run-once` or `fili run` continues
from there. `--json` prints one `SourceItem` per line on stdout and moves every
note to stderr. The source's fetches go through its assigned fetcher, with its allowlist
and timeout, exactly as under `fili run`. A fetcher, a disabled source or an unknown name is refused
before any component is loaded.

### `fili run`

Starts the host: it loads the registry strictly (one broken enabled plugin TOML
stops the command; set `enabled = false` to skip a plugin), refuses a registry
with no enabled source, takes the state-directory lock (a second `fili run` on
the same directory fails with `StateDirLocked` and names the directory),
connects to Fluvio and loads every enabled source component. It then prints the
state directory, the loaded sources and fetchers, and one line for every
component that could not be loaded, naming the plugin and its path. If no
source component could be loaded it exits 1 instead of idling. Otherwise it
polls each source on its own interval until stopped (see Polling and backoff
for how stopping works) and prints a summary table sorted by plugin name:
polls, items sent, items dropped, failures, and a state of `stopped`,
`disabled: <reason>` or `aborted at shutdown`.

An operator stop (Ctrl-C or SIGTERM) exits 0, even when some tasks had to be
aborted at shutdown; those are named in a warning. The command exits 1 when
every plugin ended disabled by a fault, when no source is enabled, when none
could be loaded, or on any startup error. One plugin's batches reach the sink
in poll order; the order across plugins is not specified.

## OAuth

A source can ask the host to manage an OAuth 2.0 login for it. The user logs in
once with `fili auth login <plugin>` (authorization code flow with PKCE), the
host keeps the tokens, refreshes the access token before it runs out, and hands
the plugin only a current access token. The plugin never sees the refresh token
or the client secret.

### Declaring OAuth

Add a `[host.oauth]` table to the plugin's TOML, beside `[host]` and
`[host.secrets]`. It is not part of `[settings]`: `[settings]` is passed to the
plugin as is, and nothing about the login belongs there. A plugin with no
`[host.oauth]` table has no login and always polls with `access-token = none`.

| Key | Required | Meaning |
|-----|----------|---------|
| `grant` | no | `authorization_code` (the default: a user logs in with `fili auth login`) or `client_credentials` (app-only: no user, no login; see [App-only access](#app-only-access-client_credentials)). |
| `authorize_url` | for `authorization_code` | The provider's authorization endpoint. Must be `https://`. Not allowed with `client_credentials`. |
| `token_url` | yes | The provider's token endpoint. Must be `https://`. |
| `scopes` | no | A list of scope strings, each one scope with no whitespace. Default: none. Not allowed with `client_credentials`. |
| `client_id` | yes | The application's client id. It is public, so it lives in the TOML. |
| `client_secret_env` | for `client_credentials` | The name of a `.env` key that holds the client secret. Required for `client_credentials`; for `authorization_code` it is optional and only for providers that need one. |
| `revocation_url` | no | The provider's revocation endpoint, used by `fili auth logout`. Must be `https://`. Not allowed with `client_credentials`. |
| `redirect_port` | no | The loopback port of the login callback. Default: `17420`. Not allowed with `client_credentials`. |
| `authorize_params` | no | A table of extra parameters appended to the authorize URL. Not allowed with `client_credentials`. |
| `user_agent` | no | 1 to 200 printable ASCII characters, sent as the `User-Agent` of the host's token and revocation requests. The plugin's own requests are unaffected. |

```toml
[host.oauth]
authorize_url = "https://provider.example/oauth/authorize"
token_url = "https://provider.example/oauth/token"
scopes = ["read"]
client_id = "your-client-id"
client_secret_env = "EXAMPLE_CLIENT_SECRET"                # optional; the value goes in .env
revocation_url = "https://provider.example/oauth/revoke"   # optional
redirect_port = 17420                                      # optional
authorize_params = { duration = "permanent" }              # optional
```

- The three endpoints must be `https://` URLs with a host and no user name or
  password. Only the loopback callback is plain http. The host calls them with
  its own TLS-validating client, outside the plugin's fetch allowlist, so the
  allowlist does not need to list them.
- `client_secret_env` follows the rules in [Plugin secrets](#plugin-secrets):
  the name is looked up in the process environment first and then in `.env`
  beside `filimon.toml`, and the value is never written in the TOML. It is not
  one of the plugin's `[host.secrets]` and is never given to the plugin.
- `authorize_params` cannot repeat a parameter the host sets itself: `state`,
  `code_challenge`, `code_challenge_method`, `redirect_uri`, `client_id`,
  `response_type` and `scope` are a config error.
- Register the redirect URI with the provider exactly as
  `http://127.0.0.1:17420/callback`, or with the port you set in
  `redirect_port`. Providers compare it exactly, so `localhost` or another port
  fails the login at the provider.
- A config error names the plugin and the field. Like any invalid plugin TOML,
  an invalid `[host.oauth]` stops `fili run`; set `enabled = false` on the
  source to skip it.

### App-only access (client_credentials)

Some providers hand out an application token with no user involved. Set
`grant = "client_credentials"` and the host fetches that token itself: there is
no login, no browser step and no token file. This is the mode of the app-only
Reddit instance.

```toml
[host.oauth]
grant = "client_credentials"
token_url = "https://www.reddit.com/api/v1/access_token"
client_id = "your-client-id"
client_secret_env = "REDDIT_CLIENT_SECRET"                 # the value goes in .env
user_agent = "linux:com.example.filimon:v0.1.0 (by /u/your-username)"
```

- The host requests the token with HTTP Basic authentication (client id and
  secret) and the body `grant_type=client_credentials`. It keeps the token in
  memory only, and fetches a new one when under 60 seconds of life remain
  (3600 seconds are assumed when the answer has no `expires_in`). Nothing is
  written to disk, so every `fili run` start and every `fili plugins run-once`
  fetches a fresh token.
- There is no login. `fili auth login <plugin>` refuses a `client_credentials`
  plugin with exit 1, before it reads any secret or creates anything, and
  `fili auth status` shows the plugin as `app-only`.
- A `client_credentials` table must set `client_secret_env`, and must not set
  `authorize_url`, `redirect_port`, `authorize_params`, `revocation_url` or
  `scopes`: each of those is a config error naming the field, so a setting is
  never silently ignored.
- The plugin receives only the access token. The client id and the client secret
  never reach it.
- One grant applies per plugin instance. To use both grants against one
  provider, register two instances of the plugin.

### Logging in and out

- `fili auth login <plugin> [--no-browser]` (for the authorization code grant;
  a `client_credentials` plugin is refused, see
  [App-only access](#app-only-access-client_credentials)) starts a listener on
  `127.0.0.1:<redirect_port>`, prints the authorize URL and also tries to open
  your browser. `--no-browser` only prints it, which suits a remote shell: open
  the URL on any machine that can reach the loopback port (for example through
  an SSH tunnel). It waits 5 minutes for the provider to call back and checks
  the `state` it sent. A denied login, a wrong `state`, a timeout or a failed
  code exchange exits 1 with a named error and stores nothing; an existing
  login is left as it was. The code, the tokens and the PKCE verifier are never
  printed or logged. On success it prints when the access token expires, and
  warns when the provider gave no refresh token.
- `fili auth status [--json]` prints one row per plugin that declares
  `[host.oauth]` or has a stored login: `PLUGIN`, `STATE`, `EXPIRES` and
  `REFRESH` (whether a refresh token is stored). `--json` prints one JSON array
  with `plugin`, `state`, `expires_at`, `has_refresh_token` and `detail`. It
  never prints a token value. The states are:
  - `logged-in`: an access token that has not expired.
  - `expired`: the access token has expired. With a refresh token the host
    renews it before the next poll; without one, log in again.
  - `needs-login`: the host could not renew the login for good (see
    [Refresh and failures](#refresh-and-failures)). Run `fili auth login` again.
  - `not-logged-in`: the plugin declares `[host.oauth]` and nothing is stored.
  - `app-only`: the plugin declares `grant = "client_credentials"`. Nothing is
    stored or needed, and a token file of the same name, if one exists, is
    ignored. `EXPIRES` and `REFRESH` show `-` (`null` in `--json`).
  - `unreadable`: the token file cannot be used. The row names the plugin and
    the path, and gives the reason in fixed words, never the file's content.
- `fili auth logout <plugin>` deletes the token file first, and then, when the
  plugin declares `revocation_url`, tries to revoke the login at the provider.
  A failed revocation is a warning, and the logout still succeeds. A second
  logout says there is no stored login. Later polls run with
  `access-token = none`.

`fili auth` prints no banner, and `status` never takes the state-directory lock
and creates nothing, so these commands work while `fili run` is up. A running
host picks up a login, a re-login or a logout at its next poll, with no
restart. Exit codes are as in [Plugin CLI](#plugin-cli): `fili auth` with no
subcommand is a usage error (2), and an unknown plugin, a fetcher, a disabled
source, a plugin without `[host.oauth]` or a `client_credentials` plugin is
refused with exit 1 before anything is created.

### Where tokens live

Each login is one file, `<state-dir>/auth/<alias>.json`, in the state directory
described under [Cursor state](#cursor-state). The `auth` directory is created
with mode 0700 and the file with mode 0600, from its first byte. The file is
JSON with `version`, `plugin`, `access_token`, `refresh_token` (or null),
`expires_at` (Unix seconds), `token_type`, `scope`, `needs_login` and
`obtained_at`. It is written atomically (temporary file, then rename).
`<alias>.lock` beside it serialises a login, a refresh and a logout of one
plugin; it is a different file from the state directory's own `.lock`.

Do not edit the file by hand. A file with any group or other permission bit, a
file that is not valid JSON, a file that names another plugin and a file of a
newer version each disable that plugin only, with an error that names the
plugin and the path. The host never repairs, rewrites or deletes such a file:
fix the permissions (`chmod 600`) or run `fili auth logout <plugin>` and log in
again.

### What a plugin receives

Only the access token, as `poll`'s `access-token` argument (`none` when there is
no login, or the login needs a new one; an app-only plugin receives the app
token the host fetched). `init` receives the plugin's own
`[host.secrets]` and settings as before. The refresh token, the client secret
and every other host-side credential stay in the host; they are not in a
plugin's arguments, settings, logs or items. The WIT contract is unchanged.
`fili plugins run-once` reads, and when needed refreshes, the same login as
`fili run`.

### Refresh and failures

- The refresh is lazy. Just before each poll the host re-reads the token file;
  with at least 60 seconds of life left it hands that token over, otherwise it
  refreshes first. There is no background timer, so a plugin with a long poll
  interval gets a fresh token only when it is polled. One refresh runs at a
  time per plugin, and the new tokens are saved before the new access token is
  used.
- A permanent refusal (the provider answers `invalid_grant`: the refresh token
  is revoked or expired) keeps the file, sets `needs_login` and polls with
  `access-token = none`. The plugin is expected to answer `auth-required`, and
  the usual backoff applies. `fili auth status` shows `needs-login`.
- A transient failure (no network, a timeout, a 5xx answer) marks nothing. The
  host keeps using the old access token while it is still valid; once it has
  expired, the poll is skipped and retried under the backoff.
- A plugin with no refresh token whose access token has expired is treated as
  needing a new login.
- After `fili auth login` the plugin may still be waiting out its backoff, so
  the new login is used at its next poll, which can be up to `backoff.max_secs`
  away (`[host.backoff]`, 3600 s by default). The login message says the same.
- When a plugin has no usable login the host logs one hint per change of state,
  `run: fili auth login <plugin>`, rather than one line per poll.

### Provider notes

- Reddit issues a refresh token only when the authorize request carries
  `duration=permanent`, so a Reddit plugin needs
  `authorize_params = { duration = "permanent" }`. Without it the login cannot
  be renewed and ends when its access token does (the login command warns).
- Which providers need `client_secret_env` is the provider's rule: a public or
  installed application type often has none.
- Reddit: register the application as a web app. A web app has a secret and a
  redirect URI, so one registration serves both the app-only instance
  (`grant = "client_credentials"`) and the code-flow instance (with the
  redirect URI `http://127.0.0.1:17420/callback`). An installed-app registration
  has no secret and cannot use the `client_credentials` grant.
- Reddit requires a unique, descriptive User-Agent of the form
  `<platform>:<app ID>:<version> (by /u/<username>)`. Set it in
  `[host.oauth] user_agent` for the host's token calls; the plugin sends its own
  User-Agent for its API calls. Reddit may restrict new API access, so
  credentials may need Reddit's approval before they work.
- Google: use a Desktop-app OAuth client with the loopback redirect. Google's
  token endpoint normally expects the client secret for desktop clients, so set
  `client_secret_env`. A consent screen left in Testing mode makes refresh
  tokens expire after about 7 days, so log in again then, or publish the app.

### Known limits

- Some providers rotate the refresh token on every refresh. The host saves the
  new one before it uses the new access token, but a crash between the
  provider's answer and the file write loses it, and the next refresh then
  fails with `invalid_grant`. Log in again.
- Expiry is judged by the wall clock, using `expires_in` from the provider
  (3600 s when it gives none). A large clock change makes a token look fresh or
  stale until the next refresh or login.
- Tokens are plain files protected by mode 0600. There is no keychain or
  encryption, and the mode check is Unix only.
- One login per plugin: there are no multiple accounts for one source.
- The app-only token is memory-only, so it is fetched again at every start, and
  one grant applies per plugin instance.

## Contract version

The contract is `filimon:plugin@0.1.0`. It stays at 0.x until the Reddit and
YouTube plugins validate it. Every `wit/*.wit` file repeats the same line,
`package filimon:plugin@0.1.0;`, so a version bump edits all of them.

## If cargo-component 0.21.1 stops building

cargo-component is marked experimental and has not released since 2025-04.
The fallback is `cargo build --target wasm32-wasip2` with the `wit-bindgen`
macro. Switching to it changes the build and conformance requirements, so it
needs user sign-off first.
