---
phase: 01-plugin-contract-configuration
reviewed: 2026-11-03T12:17:24Z
depth: standard
files_reviewed: 8
files_reviewed_list:
  - Cargo.lock
  - Cargo.toml
  - examples/plugins/.env.example
  - plugins/README.md
  - src/cli.rs
  - src/plugins/config.rs
  - src/plugins/error.rs
  - src/plugins/secrets.rs
findings:
  critical: 0
  warning: 6
  info: 5
  total: 11
  necessity: 0
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-11-03T12:17:24Z
**Depth:** standard
**Files Reviewed:** 8
**Status:** issues_found

## Summary

This is an incremental re-review. It covers the gap-closure plans 01-05 (a hand-written literal `.env` parser, plus the WR-02 and WR-03 fixes) and 01-06 (the CLI default-registry test), on top of base `4821ae5`. The new code was read in full: `git diff 4821ae5 HEAD` for each file in scope, and all of `src/plugins/secrets.rs` and `src/plugins/error.rs`. The parts of `src/plugins/config.rs` that earlier findings cite were re-read to check whether those findings are still open.

**How the claims were checked:**
- `cargo test -- plugins:: cli::` passes 53 tests, 0 failed.
- `cargo clippy --all-targets` reports nothing new in the changed hunks.
- `parse_line` and its helpers (`secrets.rs:113-236`) were copied into a throwaway binary under `/tmp/envprobe`, outside the repo, to probe edge cases. Results:
  - `K=abc\u{a0}#tail` gives `abc`.
  - `K=\u{a0}abc` and `K=abc\u{a0}` both give `abc`.
  - `K='it's'` is rejected with "unexpected text after the value; quote the whole value".
  - A bare line `dGhpc2lzYXNlY3JldA==` is accepted as key `dGhpc2lzYXNlY3JldA` with value `=`.
  - On Rust 1.85, `str::lines` keeps a trailing `\r` on a final line that has no `\n`. The parser still accepts it as trailing whitespace.

The parser is careful overall. Rejection reasons are fixed phrases. Line numbers are counted, not searched for. The process environment is never read during parsing. No path was found that leaks a value into Display or Debug, apart from the narrow duplicate-key case in IN-10. No critical issue remains.

The new warnings concern how the strict parser interacts with the rest of the loader:
- **WR-07.** Any bad line, even for a key fili never reads, makes the whole `.env` unusable. The load then fails even when every secret comes from the process environment. The file's location cannot be changed.
- **WR-08.** `resolve_secrets` now returns `Ok` with missing secrets whenever it is handed `None`. Only the caller's discipline keeps that safe.
- **WR-09.** `MissingSecret` can tell the operator to add a `.env` line that the new parser rejects.

The necessity rubric (`gsd-core/references/necessity-review.md`) was read and applied. No necessity findings.

### Status of earlier findings (previous review, commit 8ab8a99)

Each claim was checked against the current source.

| ID | Claimed by | Status | Evidence |
|----|-----------|--------|----------|
| CR-01 | 01-05 | **Resolved** | `dotenvy` is gone from `Cargo.toml` and `Cargo.lock`, and nothing references it (grep). The parser in `secrets.rs:132-236` never substitutes. `$` outside single quotes is rejected (`DOLLAR`, `secrets.rs:205,218`). Tests: `dotenv_dollar_outside_single_quotes_is_rejected`, `dotenv_single_quoted_dollar_round_trips`, `dotenv_never_expands_process_env`. |
| WR-02 | 01-05 | **Resolved** | `config.rs:261-272` keeps `None` when `.env` cannot be used. `secrets.rs:295` then skips `MissingSecret`. Tests assert exactly one problem (`config.rs` `malformed_dotenv_reports_line_not_content`, `unusable_dotenv_suppresses_missing_secret`). The fix leaves a fail-open contract behind (see WR-08). |
| WR-03 | 01-05 | **Resolved** (by correcting the diagnostic; D-12 precedence kept) | `EmptySecret` now carries `origin` and a `#[help] fix` (`error.rs:101-116`). `empty_secret_source` (`secrets.rs:316-331`) tells the operator to unset the process variable. |
| IN-02 | 01-05 | **Resolved** | Lines are counted with `enumerate` (`secrets.rs:80-81`). `line_containing` is gone. Test: `dotenv_line_number_is_counted`. |
| IN-03 | 01-06 | **Resolved** | `cli.rs:107-118` asserts the default through `Cli::command()` metadata and no longer returns early. |
| WR-01 | - | **Open**, carried below | `config.rs:574-586` still only calls `url::Host::parse(..).is_err()` and stores the raw entry (`config.rs:538-541`). |
| WR-04 | - | **Open**, carried below | `config.rs:195` still uses `std::env::var(key).ok()`. |
| WR-05 | - | **Open**, carried below | `config.rs:589-596` has no upper bounds. |
| IN-07 | - | **Open**, carried below | `config.rs:227` is unchanged. |
| IN-08 | - | **Open, escalated** to WR-09 | Aliases and env var names are still not validated. The new parser makes this worse (see WR-09). |
| IN-01 | - | Open, fix belongs in `wit/worlds.wit` (outside this scope) | The `config.rs:519-524` re-serialisation is unchanged. |
| WR-06, IN-04, IN-05, IN-06 | - | Open, outside this scope | `wit/`, `plugins/stub-*`, `examples/plugins/filimon.toml` and `src/plugins/mod.rs` are unchanged since `4821ae5` (`git diff --stat` is empty). Not re-reviewed. |

## Narrative Findings (AI reviewer)

## Warnings

### WR-07: One bad line for an unrelated key makes `.env` unusable and fails the load, even when every secret comes from the process environment

**File:** `src/plugins/config.rs:256-272`, `src/plugins/secrets.rs:80-102`, `plugins/README.md:150-154`
**Issue:**
- **Every key is checked.** `read_dotenv` applies rejections R1-R8 to every line, including keys that no `[host.secrets]` alias references. Any problem makes the result `Err`. The loader appends those problems to `registry_problems` and the whole load fails.
- **The process environment does not help.** This still happens when every alias is satisfied by the process environment. `unusable_dotenv_suppresses_missing_secret` pins it: `envtok` resolves from env, and the load still fails.
- **Common shell-style lines are rejected.** dotenvy accepted these: `DATABASE_URL=postgres://${DB_USER}@db/app`, `PATH_EXTRA=$HOME/bin`, `WIN_PATH=C:\tools`, and a repeated key. The new parser rejects all of them, and its reasons never name the key, so the operator cannot tell that the bad line has nothing to do with fili.
- **The documented workaround has no knob.** The README says to "Keep fili's `.env` separate from `.env` files written for shell-style tools" (`README.md:152-154`). But the path is fixed at `base_dir.join(".env")` (`config.rs:260`). The default registry is `filimon.toml` in the current directory (`cli.rs:8`), which is the same place docker-compose and other dotenv loaders read `.env`. The only way to follow the advice is to move the registry.
- **Consequence:** a container that injects every secret through the environment, but has a developer `.env` in its working directory, refuses to start.
- **The code already knows the risk.** The comment at `config.rs:256-257` acknowledges that a shared `.env` is a hazard, but guards only the registries that declare no secrets.

**Fix:** Pick one, and update the README to match:
1. Fail only on lines that matter. Collect rejected lines per key, and report a problem only when an alias actually falls through to `.env` for that key, or when a line has no parseable key (`NOT_ASSIGNMENT`, or `UNTERMINATED` on a referenced key). For example, have `read_dotenv` return `(HashMap<String, String>, Vec<(Option<String>, ConfigError)>)`. Then let `resolve_secrets` surface only the problems whose key is `None` or is looked up in `.env`.
2. Make the file fili-specific: read `.fili.env` (or honour `FILI_DOTENV` / `--dotenv`), so it is never shared with shell tools.

### WR-08: `resolve_secrets` returns `Ok` with missing secrets whenever `dotenv` is `None`, and the loader logs "resolved plugin secrets" for them

**File:** `src/plugins/secrets.rs:247-250,294-295`, `src/plugins/config.rs:333-348`
**Issue:**
- **Silent omission.** `None if dotenv.is_none() => {}` drops an unresolved alias without an error. If every alias of a plugin falls into this arm, `resolve_secrets` returns `Ok(ResolvedSecrets)` that lacks the aliases the plugin declared.
- **Safety rests on prose.** The only thing preventing a plugin from being initialised without its required secrets is the doc comment "the caller … must fail the load". Today the single caller (`load_registry_with_env`) does fail, because it already pushed the `DotenvInvalid` problems. But `resolve_secrets` is `pub`, and Phases 3-4 plan new consumers (host, hot reload). A future caller that passes `None` to mean "no `.env`" gets a `PluginConfig` with silently missing secrets: fail-open.
- **Misleading log.** On this path `finish_plugin` still emits `tracing::info!(…, "resolved plugin secrets")` (`config.rs:348`) for a plugin whose secrets were not resolved, inside a load that is about to fail.

**Fix:** Make the invariant structural, not documentary:
- Either return the skipped aliases so the caller cannot ignore them, for example `Err(Vec<ConfigError>)` plus an `Unresolved` marker, or a `Result<ResolvedSecrets, SecretsOutcome>` where the `.env`-unusable case is its own variant. Or skip `resolve_secrets` entirely (and the info log) when `dotenv` is `None`: the load fails anyway, so only `EmptySecret` from the process environment is worth collecting.
- Also narrow `resolve_secrets` and `read_dotenv` to `pub(super)`.

### WR-09: `MissingSecret` can prescribe a `.env` line the new parser rejects (escalates IN-08)

**File:** `src/plugins/error.rs:81-84`, `src/plugins/secrets.rs:162-177`, `src/plugins/config.rs:554` (aliases stored unvalidated)
**Issue:**
- **Two grammars.** Env var names in `[host.secrets]` are still never validated (previous IN-08). The new parser accepts only keys matching `[A-Za-z_][A-Za-z0-9_.]*` (`is_key_start` / `is_key_char`).
- **The help sends the operator into a broken file.** Take a plugin that declares `token = "REDDIT-TOKEN"` or `token = "9TOKEN"`, with the variable unset. `MissingSecret` says ``add `REDDIT-TOKEN=...` to <.env>``. If the operator follows it, the line is rejected as `expected KEY=VALUE` (the probe: `split_key` stops at `-`, and `strip_prefix('=')` then fails). Because of WR-07, that one rejected line makes the whole `.env` unusable, and now every alias that relied on `.env` fails too. The diagnostic leads the operator into a file the loader cannot read, and the new error names neither the key nor the cause.
- **Earlier issues remain.** The original IN-08 problems still apply: an empty alias `"" = "X"`, and an empty env var name.

**Fix:** Validate `[host.secrets]` in `host_problems`. Apply `is_valid_name` to each alias. Require each env var name to match the same grammar the `.env` parser accepts. Share one predicate, for example `secrets::is_env_key(&str) -> bool` built from `is_key_start`/`is_key_char`, so the two cannot drift. Report `InvalidValue` at the span of the env var name with the message "env var names must match [A-Za-z_][A-Za-z0-9_.]*".

### WR-01: Allowlist entries are checked with `url::Host::parse` but stored un-normalized; wildcards pass (carried, still open)

**File:** `src/plugins/config.rs:574-586`, `src/plugins/config.rs:538-541`
**Issue:** Unchanged since the previous review. `url::Host::parse` normalizes its input: it lowercases, percent-decodes, IDNA-encodes and reparses IPv4 shorthand. The loader then stores the raw text with `toml::Spanned::into_inner`. So `API.Example.COM`, `ex%61mple.com`, `bücher.de`, `0x7f.1` and `*.reddit.com` all validate, but none of them will ever match a normalized `Url::host_str()`. This fails closed: every request is silently denied, even though the config "validated".
**Fix:** Reject any entry whose canonical form differs from its input, and any entry that contains `*`, or store `host.to_string()`:
```rust
match url::Host::parse(raw) {
    Ok(host) if host.to_string() == *raw && !raw.contains('*') => {}
    Ok(host) => problems.push(invalid(field, format!("`{raw}` must be written as `{host}`"), span)),
    Err(_) => problems.push(/* existing message */),
}
```

### WR-04: A non-UTF-8 process variable is silently treated as absent, and `.env` is used instead (carried, still open)

**File:** `src/plugins/config.rs:195`
**Issue:** `std::env::var(key).ok()` collapses `VarError::NotUnicode` into `None`. That silently breaks the D-12 precedence rule, which the 01-05 changes reaffirm (`secrets.rs:273`): the `.env` value wins, and it may be stale. If `.env` has no entry, `MissingSecret` falsely claims the variable "is not set in the process environment".
**Fix:** Map `NotPresent` to `None`, and report `NotUnicode` as a dedicated `ConfigError` that carries the name only. For example, change the lookup type to `Fn(&str) -> Result<Option<String>, VarError>`.

### WR-05: No upper bounds on durations or the backoff multiplier (carried, still open)

**File:** `src/plugins/config.rs:84-104`, `src/plugins/config.rs:589-596`
**Issue:** Unchanged. `poll_interval_secs`, `timeout_secs`, `initial_secs` and `max_secs` accept values up to `u64::MAX`. `multiplier` accepts any finite value ≥ 1.0. Host backoff arithmetic (`Duration::mul_f64` / `from_secs_f64`) panics on overflow, so this "validated" config can crash the planned host.
**Fix:** Add range checks with named constants, for example `poll_interval_secs <= 7d`, `timeout_secs <= 1h`, `max_secs <= 1d`, `1.0 <= multiplier <= 10.0`. Report each as `InvalidValue` at its span, and document the limits in `plugins/README.md`.

## Info

### IN-09: Unicode whitespace delimits unquoted values and starts comments, so `KEY=abc<NBSP>#x` silently becomes `abc`

**File:** `src/plugins/secrets.rs:133,181,213-214,229-231`, `plugins/README.md:132-136`
**Issue:**
- **Every Unicode space counts.** The parser uses `str::trim_start` and `char::is_whitespace`, which match all Unicode White_Space, including U+00A0, U+0085 and U+3000. The probe confirmed three cases. `K=abc\u{a0}#tail` gives `abc`: a silent truncation, the same class of defect as CR-01, though far rarer. `K=\u{a0}abc` gives `abc`. `K=abc\u{a0}` gives `abc`.
- **The docs suggest ASCII.** The README says only "whitespace", and operators read that as space and tab.
- **Zero-width characters are kept.** Non-whitespace zero-width characters such as U+FEFF and U+200B inside an unquoted value stay in the value, invisibly.
- **Severity.** This is deliberate per the 01-05 plan ("Whitespace below means `char::is_whitespace`"), and NBSP rarely appears in real secrets.

**Fix:** Treat only ASCII space and tab as delimiters. Reject any other Unicode whitespace in an unquoted value with a fixed reason such as "non-ASCII whitespace outside quotes; quote the value". Alternatively, document in the README that every Unicode space separates values.

### IN-10: The duplicate-key reason echoes line text, contradicting the module's own invariant; bare base64 lines parse as keys

**File:** `src/plugins/secrets.rs:87,115`, `plugins/README.md:150-151`
**Issue:**
- **The invariant is broken.** The comment at line 115 says "None may ever include line text", and the README says lines are reported "never with its text". The R8 reason at line 87 interpolates the key.
- **Why it usually doesn't matter.** Keys are normally env var names. But `split_key` accepts any `[A-Za-z0-9_.]` run, so a stray pasted secret line such as `c2VjcmV0dG9rZW4=` (base64 with padding) is silently accepted as key `c2VjcmV0dG9rZW4` with value `=`, with no diagnostic. If that line appears twice, the duplicate reason prints the secret's body.
- **Severity.** The leak is improbable. The silent acceptance of a non-assignment line is the more practical problem.

**Fix:** Name the key in the duplicate reason only when an alias references it; otherwise use "duplicate key (first set on line N)". Fix the line-115 comment and the README so they describe what the code actually does.

### IN-11: The guidance for values containing `'` is circular

**File:** `src/plugins/secrets.rs:121,124`, `examples/plugins/.env.example:3-5`
**Issue:**
- **The error points the wrong way.** `KEY=it's` is rejected with "quote inside an unquoted value; quote the whole value".
- **The example file points the wrong way too.** `.env.example` says to use single quotes for special characters. It mentions `'` only together with `$` or a backslash.
- **Following the advice fails.** `KEY='it's'` is then rejected with "unexpected text after the value; quote the whole value" (confirmed by the probe), which tells the operator to do what they just did.
- **The answer exists but is hard to find.** The working form is `KEY="it's"`, and it appears only in the README.

**Fix:** Change `QUOTE_IN_UNQUOTED` to "quote inside an unquoted value; wrap a value containing `'` in double quotes, otherwise in single quotes". Add a line to `.env.example`: "A value containing ' (and no $ or backslash) goes in double quotes: KEY=\"it's\"".

### IN-12: `read_dotenv` returns every `.env` value, including unrelated keys, as a plain `HashMap<String, String>` whose Debug prints them

**File:** `src/plugins/secrets.rs:68`, `src/plugins/config.rs:261`
**Issue:** The project invariant is that secret values never appear in Debug, logs or diagnostics, and that is enforced by the `Secret` newtype. `read_dotenv` is `pub` and returns raw `String` values for every key in the file, so any `{:?}` of the map (for example, a future `tracing::debug!(?dotenv)`) leaks all of them. Today no caller formats the map, so this is latent only.
**Fix:** Return `HashMap<String, Secret>`, constructing `Secret` in `read_dotenv`, so redaction holds from the moment of parsing. Narrow the function to `pub(super)`.

### IN-07: With the default `--registry filimon.toml`, every resolved path is relative to the current directory (carried, still open)

**File:** `src/plugins/config.rs:227`, `src/cli.rs:8,26-27`
**Issue:** Unchanged. `base_dir` becomes `""`, so `component`, `config_path` and the `.env` path stay relative to the current directory. This also makes the location of `.env` in WR-07 depend on the current directory.
**Fix:** Make `base_dir` absolute at load time with `std::path::absolute` or `fs::canonicalize`.

---

_Reviewed: 2026-11-03T12:17:24Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
