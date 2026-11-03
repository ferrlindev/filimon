---
phase: 01-plugin-contract-configuration
reviewed: 2026-11-03T10:32:33Z
depth: standard
files_reviewed: 19
files_reviewed_list:
  - Cargo.toml
  - src/main.rs
  - src/cli.rs
  - src/plugins/mod.rs
  - src/plugins/config.rs
  - src/plugins/secrets.rs
  - src/plugins/error.rs
  - wit/types.wit
  - wit/host.wit
  - wit/worlds.wit
  - plugins/stub-source/Cargo.toml
  - plugins/stub-source/src/lib.rs
  - plugins/stub-fetcher/Cargo.toml
  - plugins/stub-fetcher/src/lib.rs
  - examples/plugins/filimon.toml
  - examples/plugins/reddit.toml
  - examples/plugins/youtube.toml
  - examples/plugins/stub.toml
  - examples/plugins/.env.example
findings:
  critical: 1
  warning: 6
  info: 8
  total: 15
  necessity: 0
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-11-03T10:32:33Z
**Depth:** standard
**Files Reviewed:** 19
**Status:** issues_found

## Summary

Scope: the plugin configuration loader (`src/plugins/*`), the WIT contract, the two stub guests, and the sample configs. For `src/main.rs`, `src/cli.rs` and `Cargo.toml`, only the hunks changed since baseline `12c57b9` were reviewed.

The loader is well structured. It collects problems instead of failing on the first one, and its Debug output redacts secrets. There are still real defects:

- **Secret corruption.** The `.env` reader passes values through dotenvy's `$VAR` substitution. A secret that contains `$` is silently cut short or changed, and it still passes the non-empty check. Substitution also reads the real process environment, which bypasses the injected `env` lookup.
- **Allowlist accepted but stored un-normalized.** The check accepts entries that `url` normalizes (case, percent-encoding, IDN, IPv4 shorthand, wildcards), but the raw text is stored. The loader's own comment says such entries "would never match as written".
- **Misleading diagnostics.** A malformed `.env` produces extra MissingSecret errors. An empty or non-UTF-8 process variable shadows `.env`, or is silently ignored, and the help text points the operator at the wrong fix.
- **No upper bounds** on durations or the backoff multiplier.
- **Egress gap in the WIT contract.** It does not say where the source allowlist applies to a fetcher's `outbound-http.send`.

Empirical checks: dotenvy 0.15.7 and url 2.5.8 (the locked versions) were run in a throwaway crate outside the repo, since deleted. `PW=pa$$word` parses to `pa`. `Q="x$HOME/y"` expands to the real `$HOME`. `url::Host::parse` accepts `API.Example.COM` → `api.example.com`, `*.reddit.com`, `ex%61mple.com` → `example.com`, `bücher.de` → `xn--bcher-kva.de`, and `0x7f.1` → `127.0.0.1`.

The necessity rubric was read and applied. No necessity findings.

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: `.env` secrets are silently mangled by dotenvy `$` substitution, which also reads the real process env

**File:** `src/plugins/secrets.rs:64` (consumed at `src/plugins/secrets.rs:140`)
**Issue:** `read_dotenv` parses with `dotenvy::from_read_iter`. That parser does shell-style variable substitution on unquoted and double-quoted values (`dotenvy-0.15.7/src/parse.rs` `parse_value` / `apply_substitution`).
- `REDDIT_CLIENT_SECRET=pa$$word` resolves to `pa`. That value is non-empty, so it passes the `EmptySecret` check at `secrets.rs:141` and is handed to the guest. The plugin then fails authentication with no host diagnostic.
- Any `$NAME` inside a secret is replaced by the real process variable `NAME`, or by nothing. `apply_substitution` calls `std::env::var` directly. This bypasses the injected `env` closure that `load_registry_with_env` documents as the only env source, so tests and the "process env first, `.env` second" contract (`secrets.rs:114-117`) are not hermetic.
- A double-quoted value that ends in `$VAR"` instead fails with "malformed line". That error is at least visible.

Generated client secrets and passwords often contain `$`. The doc comment at `secrets.rs:45-46` ("parsed by dotenvy from that in-memory text… values stay in the returned map") hides this transformation.
**Fix:** Stop using a substituting parser for secret material. dotenvy 0.15 has no switch to turn substitution off. Two options:
- Parse `KEY=VALUE` lines yourself, with no substitution and only literal quote stripping.
- Reject any value that contains an unescaped `$` outside single quotes, with a diagnostic that carries no content:
```rust
// after obtaining (key, raw_value_text) for a line, before accepting it:
if value_uses_substitution(raw_value_text) {
    return Err(dotenv_invalid(path, Some(line_no),
        "value contains `$`; wrap it in single quotes or escape it as `\\$`".to_string()));
}
```
Add regression tests: `PW=pa$$word` must either round-trip exactly or be rejected, and `PW='pa$$word'` must resolve to `pa$$word`. Also update `.env.example` to tell operators to single-quote secrets.

## Warnings

### WR-01: Allowlist entries are checked with `url::Host::parse` but stored un-normalized; wildcards pass

**File:** `src/plugins/config.rs:569-580` and `src/plugins/config.rs:532-535`
**Issue:** The comment at line 567 says entries are checked so that nothing "would never match as written". However, `url::Host::parse` lowercases, percent-decodes, IDNA-encodes and reparses IPv4 shorthand, and the code then stores the original string (`toml::Spanned::into_inner`), not the parsed host. All of these are accepted and kept verbatim: `API.Example.COM`, `ex%61mple.com`, `bücher.de`, `0x7f.1`, `example.com.`. A later comparison against `Url::host_str()` (which is normalized) will never match them. `*.reddit.com` is accepted because `*` is not a forbidden domain code point, yet exact-host matching never treats it as a wildcard. The failure is fail-closed (deny), so this is not a bypass. But the operator gets a config that "validates" and then silently blocks every request.
**Fix:** Store the canonical form, and reject entries whose canonical form differs from the input or that contain `*`:
```rust
match url::Host::parse(raw) {
    Ok(host) if host.to_string() == *raw && !raw.contains('*') => { /* keep */ }
    Ok(host) => problems.push(invalid(field, format!("`{raw}` must be written as `{host}`"), span)),
    Err(_) => problems.push(/* existing message */),
}
```
Alternatively, keep `host.to_string()` in `HostConfig.allowlist`. Add test cases for uppercase, IDN and wildcard entries.

### WR-02: A malformed `.env` also produces one false MissingSecret per alias

**File:** `src/plugins/config.rs:258-266`, `src/plugins/secrets.rs:151-158`
**Issue:** When `read_dotenv` fails, the problem is recorded and then `HashMap::new()` is used as the `.env` map. Every alias that is not in the process env then gets `MissingSecret`, whose message says the variable "is not set in the process environment or <.env>" and whose help says "add `X=...` to <.env>". The variable *is* in `.env`; the file is just malformed. The existing test `malformed_dotenv_reports_line_not_content` (`config.rs:884`) only checks `.any(DotenvInvalid)`, so it hides the extra errors. Operators get N wrong instructions for one real problem.
**Fix:** If `.env` is unusable, skip the `.env` fallback and suppress the MissingSecret checks for secrets that depend on it. For example, carry `dotenv: Option<HashMap<..>>` and in `resolve_secrets` skip `MissingSecret` when it is `None`, so only the `DotenvInvalid` problem is reported. Make the test assert that `errors.problems.len() == 1`.

### WR-03: A set-but-empty process variable overrides a valid `.env` value, and the help text suggests a fix that cannot work

**File:** `src/plugins/secrets.rs:140-147`, `src/plugins/error.rs:96-100`
**Issue:** `env(name).or_else(|| dotenv.get(name).cloned())` treats `Some("")` from the process env as authoritative. An exported empty variable then shadows a correct `.env` entry and causes `EmptySecret`. CI systems commonly export a missing secret as an empty string. The help text says "give `X` a value in the process environment or the .env file", but putting the value in `.env` cannot fix it while the empty process variable exists.
**Fix:** Pick one rule and make the diagnostic match it. Either treat an empty process value as unset and fall through to `.env`:
```rust
match env(name).filter(|v| !v.is_empty()).or_else(|| dotenv.get(name).cloned()) { ... }
```
or keep the current precedence and change the help text to "unset `X` in the process environment or give it a value". Record which source supplied the empty value in the `EmptySecret` variant.

### WR-04: A non-UTF-8 process variable is silently treated as absent and the `.env` value is used instead

**File:** `src/plugins/config.rs:195`
**Issue:** `std::env::var(key).ok()` turns `VarError::NotUnicode` into `None`. That breaks the documented rule that the process env wins: the loader silently uses the `.env` value, which may be stale. If `.env` has no entry, `MissingSecret` says the variable "is not set in the process environment", which is false.
**Fix:** Tell the two cases apart. Map `NotPresent` to `None`, and turn `NotUnicode` into a dedicated `ConfigError` (name only, no value). Alternatively, change the closure type to `Fn(&str) -> Result<Option<String>, VarError>` and report it.

### WR-05: No upper bounds on durations or the backoff multiplier in a config that claims to be "validated"

**File:** `src/plugins/config.rs:84-104`, `src/plugins/config.rs:582-606`
**Issue:** `poll_interval_secs`, `timeout_secs`, `initial_secs` and `max_secs` accept values up to `u64::MAX`. `multiplier` accepts any finite value ≥ 1.0, for example `1e300`. `LoadedRegistry` is documented as "validated and typed" (`config.rs:128`). A host that computes backoff with `Duration::mul_f64` or `Duration::from_secs_f64` panics when the result overflows ("value is either too big or NaN"). For example, 60s × 1e20 exceeds `u64::MAX` seconds. Adding a far-future duration to `Instant` can also overflow. So the config layer accepts values that are certain to crash the planned host.
**Fix:** Add range checks next to the existing ones, using named constants. For example: `poll_interval_secs <= 7 days`, `timeout_secs <= 1 hour`, `max_secs <= 1 day`, `1.0 <= multiplier <= 10.0`. Each limit should produce an `InvalidValue` labelled at its span. Document the limits in `plugins/README.md`.

### WR-06: The WIT contract does not apply the source allowlist to the fetcher's `outbound-http.send`

**File:** `wit/host.wit:22-27`, `wit/worlds.wit:30-39`, `wit/types.wit:53-54`
**Issue:** `fetch` says the host "routes the request to the source's assigned fetcher and enforces its allowlist and timeout". `outbound-http.send` says only "Send one HTTP request over the host's network stack". It does not say whose allowlist applies, and it carries no source identity. One fetcher (`http`) serves several sources (`examples/plugins/filimon.toml:13,19`). If the host checks only the URL of the `fetch` request, a fetcher can call `send` with a different URL, for example by following a redirect or rewriting the request. That bypasses the source allowlist, which is a FETCH-02 security property. This phase delivers the contract, so the enforcement point must be fixed in the contract, not left for Phase 5 to guess.
**Fix:** Say in `host.wit` that `send` is checked against the allowlist and timeout of the source whose `fetch` call is in progress. Say the host never follows redirects to non-allowlisted hosts. Say a `send` outside an active `fetch` is `denied`. Also state that fetcher instances are scoped per call or per source so the host can attribute every `send`.

## Info

### IN-01: The WIT doc calls `settings` "raw" TOML, but the host sends re-serialised text

**File:** `wit/worlds.wit:15-16`, `src/plugins/config.rs:513-518`
**Issue:** The guest receives `toml::to_string(&file.settings)`. Comments are dropped, keys are re-sorted, and `[settings.x]` subtables become root-level `[x]`. "Raw `[settings]` TOML text" suggests the original bytes, possibly including the header.
**Fix:** Reword it as "the `[settings]` table re-serialised as a standalone TOML document (no `[settings]` header; comments and key order not preserved)".

### IN-02: `line_containing` can report the wrong `.env` line

**File:** `src/plugins/secrets.rs:100-108`
**Issue:** It returns the first line that *contains* the offending text. dotenvy often reports only the value part (for example `x`), and that text can also appear in an earlier comment or line, so the error points at the wrong line.
**Fix:** Count lines while iterating. For example, split `text` yourself and parse line by line, or compare against `line.trim_start().ends_with(needle)` with the key prefix.

### IN-03: The CLI default-registry test passes without checking anything when `FILI_REGISTRY` is set

**File:** `src/cli.rs:102-109`
**Issue:** It returns early and is reported as passed, so the default is never checked on a machine where `FILI_REGISTRY` is set.
**Fix:** Assert the default through `Cli::command().get_arguments()` (`get_default_values()`), which does not depend on the environment.

### IN-04: The stub cursor wraps at 255 and repeats item ids

**File:** `plugins/stub-source/src/lib.rs:81,97`
**Issue:** `n.wrapping_add(1)` on a `u8` returns to `stub-0` after 256 polls. A host-dedup test that uses this fixture for a long time will see duplicate ids.
**Fix:** Use a little-endian `u64` cursor (`u64::from_le_bytes`), or document the wrap.

### IN-05: The sample registry enables sources whose components do not exist and requires their secrets

**File:** `examples/plugins/filimon.toml:9-19`
**Issue:** `reddit` and `youtube` are `enabled = true` and point at `components/*.wasm`, which does not exist. Loading is all-or-nothing, so the sample cannot be used for the stub alone unless all three secrets are set.
**Fix:** Ship `reddit` and `youtube` with `enabled = false`, or document that the sample needs every secret.

### IN-06: A blanket `allow(dead_code)` on the whole plugins module hides real dead code

**File:** `src/plugins/mod.rs:10-16`
**Issue:** Every item in `config`, `error` and `secrets` is exempt until Phases 3-4, so genuinely unused code added now will not be flagged.
**Fix:** Narrow the allow to the specific public items that are waiting for consumers, or use `#[expect(dead_code)]` so the exemption fails once they are used.

### IN-07: With the default `--registry filimon.toml`, all resolved paths are relative to the current directory

**File:** `src/plugins/config.rs:226-227`, `src/cli.rs:26-27`
**Issue:** `base_dir` becomes `""`, so `component`, `config_path` and `.env` stay relative paths that depend on the current directory. The comment "every relative path resolves against [the registry dir]" only holds while the current directory does not change. A long-running watcher that changes directory would break.
**Fix:** Canonicalize `base_dir` (`std::path::absolute` or `fs::canonicalize`) at load time.

### IN-08: `[host.secrets]` alias names and env var names are not validated

**File:** `src/plugins/config.rs:91-93`, `src/plugins/secrets.rs:138-140`
**Issue:** `"" = "X"` or `token = ""` (an empty env var name) gets through. The first passes an empty alias to the guest. The second produces a confusing ``needs env var `` `` message. Plugin names are validated (`is_valid_name`), but aliases, which also reach the guest, are not.
**Fix:** Apply `is_valid_name` to aliases. Require env var names to match `[A-Za-z_][A-Za-z0-9_]*`, and report `InvalidValue` at the span.

---

_Reviewed: 2026-11-03T10:32:33Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
