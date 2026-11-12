#!/usr/bin/env bash
# live-e2e.sh - scripted runbook for the human-run live end-to-end checkpoint (TEST-04).
#
# This is NOT an automated test and is never run by `cargo test`. The operator runs it
# with real keys, a Fluvio cluster and the GLiNER model files:
#
#   scripts/live-e2e.sh preflight                  check every prerequisite, name each failure
#   scripts/live-e2e.sh run                        preflight, news crawl, social run, enriched consume,
#                                                  sanitise, evidence check
#   scripts/live-e2e.sh sanitise <in.json> <out>   trim and redact a consumer output into a sample
#   scripts/live-e2e.sh check-evidence <dir>       verify the evidence directory
#   scripts/live-e2e.sh selftest                   hermetic proof of the safety logic (no keys)
#
# Exit codes: 0 success, 1 failed check, 2 usage, 3 the D-09 stop (Reddit credentials missing).
#
# Safety rules of this script: shell tracing is never enabled, and no message, file or
# command line carries an environment or .env value. The only credential-derived value
# written to a file is the public Reddit client_id, in the gitignored work directory.
# Messages name keys only.
#
# Environment (all optional):
#   ENV_FILE        credentials file (default <repo>/.env)
#   FILI_BIN        the fili binary (default <repo>/target/debug/fili)
#   WORK            gitignored work directory (default <repo>/.live-e2e)
#   EVIDENCE_DIR    committed evidence directory (default the phase 08 evidence/ directory)
#   LIMIT           records the consumer reads, news and social together (default 20)
#   SAMPLE_PER_TYPE records kept per source type and platform in the sample (default 3)
#   NEWS_SEED_URL   news crawl seed (default https://blog.rust-lang.org/)
#   LIVE_E2E_WAIVE_REDDIT   the user's reason for running without Reddit (D-09)
#   TOKENIZER_PATH, MODEL_PATH   GLiNER files (defaults of the fili flags)
#   REDDIT_USER_AGENT_CONTACT    contact in the Reddit User-Agent (default u/filimon-live-e2e)
#
# Re-running `run` reuses the work directory and overwrites the evidence files. The
# production topics keep earlier records for 7 days, so a rerun can read older records.

set -euo pipefail

SELF="${BASH_SOURCE[0]}"
SCRIPT_DIR="$(cd "$(dirname "$SELF")" && pwd)"
SELF="$SCRIPT_DIR/$(basename "$SELF")"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

abs_path() {
  case "$1" in
    /*) printf '%s' "$1" ;;
    *) printf '%s/%s' "$PWD" "$1" ;;
  esac
}

# Paths given by the caller are made absolute before the script moves to the repository root.
ENV_FILE="$(abs_path "${ENV_FILE:-$REPO_ROOT/.env}")"
FILI_BIN="$(abs_path "${FILI_BIN:-$REPO_ROOT/target/debug/fili}")"
WORK="$(abs_path "${WORK:-$REPO_ROOT/.live-e2e}")"
EVIDENCE_DIR="$(abs_path "${EVIDENCE_DIR:-$REPO_ROOT/.planning/phases/08-integration-live-e2e-authoring/evidence}")"
LIMIT="${LIMIT:-20}"
SAMPLE_PER_TYPE="${SAMPLE_PER_TYPE:-3}"
NEWS_SEED_URL="${NEWS_SEED_URL:-https://blog.rust-lang.org/}"
TOKENIZER_PATH="${TOKENIZER_PATH:-models/gliner_small-v2.1/tokenizer.json}"
MODEL_PATH="${MODEL_PATH:-models/gliner_small-v2.1/onnx/model.onnx}"
REDDIT_USER_AGENT_CONTACT="${REDDIT_USER_AGENT_CONTACT:-u/filimon-live-e2e}"

cd "$REPO_ROOT"

FAILS=0

say_ok() { printf 'ok: %s\n' "$1"; }

# say_fail <what> [<how to fix>]
say_fail() {
  if [ -n "${2:-}" ]; then
    printf 'FAIL: %s: %s\n' "$1" "$2"
  else
    printf 'FAIL: %s\n' "$1"
  fi
  FAILS=$((FAILS + 1))
}

usage() {
  cat >&2 <<'USAGE'
usage: scripts/live-e2e.sh <subcommand>
  preflight                  check every prerequisite and name each failure
  run                        run the live end-to-end checkpoint and write the evidence
  sanitise <in.json> <out>   write a trimmed, redacted sample of a consumer output
  check-evidence <dir>       verify the evidence directory
  selftest                   hermetic proof of the sanitiser, secret scan and D-09 gate
USAGE
}

# ── .env reading (literal rules of the repository's parser; never expands, never prints) ──

# Parses one line into PL_KEY and PL_VAL. Returns 1 for a blank, comment or non-assignment line.
dotenv_parse_line() {
  local line=${1%$'\r'} rest key val
  rest=${line#"${line%%[![:space:]]*}"}
  case "$rest" in
    '' | '#'*) return 1 ;;
  esac
  case "$rest" in
    export[[:space:]]*)
      rest=${rest#export}
      rest=${rest#"${rest%%[![:space:]]*}"}
      ;;
  esac
  key=${rest%%=*}
  [ "$key" != "$rest" ] || return 1
  key=${key%"${key##*[![:space:]]}"}
  case "$key" in
    '' | [0-9]* | *[!A-Za-z0-9_]*) return 1 ;;
  esac
  val=${rest#*=}
  val=${val#"${val%%[![:space:]]*}"}
  case "$val" in
    \'*)
      val=${val#\'}
      val=${val%%\'*}
      ;;
    \"*)
      val=${val#\"}
      val=${val%%\"*}
      ;;
    *)
      case "$val" in
        *' #'*) val=${val%%' #'*} ;;
      esac
      val=${val%"${val##*[![:space:]]}"}
      ;;
  esac
  PL_KEY=$key
  PL_VAL=$val
  return 0
}

# Prints the value of KEY from ENV_FILE (first occurrence). Callers capture it; it is never displayed.
dotenv_lookup() {
  local want=$1 line
  [ -f "$ENV_FILE" ] || return 0
  while IFS= read -r line || [ -n "$line" ]; do
    if dotenv_parse_line "$line" && [ "$PL_KEY" = "$want" ]; then
      printf '%s' "$PL_VAL"
      return 0
    fi
  done <"$ENV_FILE"
  return 0
}

# The process environment value when non-empty, else the ENV_FILE value. Callers capture it.
env_value() {
  local key=$1 v=""
  v="${!key:-}"
  if [ -z "$v" ]; then
    v="$(dotenv_lookup "$key")"
  fi
  printf '%s' "$v"
}

has_value() {
  [ -n "$(env_value "$1")" ]
}

# ── preflight ────────────────────────────────────────────────────────────────

cmd_preflight() {
  FAILS=0
  local waiver="${LIVE_E2E_WAIVE_REDDIT:-}" reddit_missing=0 key topics modules sm comp rel fix

  if command -v jq >/dev/null 2>&1; then
    say_ok "jq on PATH"
  else
    say_fail "jq" "not on PATH; install jq (for example: brew install jq)"
  fi

  if [ -x "$FILI_BIN" ]; then
    say_ok "fili binary is executable"
  else
    say_fail "fili binary" "not built at the expected path; run: cargo build"
  fi

  if command -v fluvio >/dev/null 2>&1 && fluvio cluster status >/dev/null 2>&1; then
    say_ok "Fluvio cluster is up"
  else
    say_fail "Fluvio cluster" "not reachable; start it (fluvio cluster start) as described in README.md"
  fi

  topics="$(fluvio topic list 2>/dev/null || true)"
  for key in filimon-raw-pages filimon-social-items; do
    if printf '%s\n' "$topics" | grep -Eq "(^|[[:space:]])${key}([[:space:]]|\$)"; then
      say_ok "topic $key exists"
    else
      say_fail "topic $key" "missing; run: fluvio topic create $key"
    fi
  done

  modules="$(fluvio smartmodule list 2>/dev/null || true)"
  for sm in filimon/map-extract@0.1.0 filimon/map-preprocess@0.1.0 filimon/filter-relevance@0.1.0; do
    if printf '%s\n' "$modules" | grep -Fq "$sm"; then
      say_ok "SmartModule $sm is loaded"
    else
      say_fail "SmartModule $sm" "not loaded; build and load it with smdk as described in README.md"
    fi
  done

  if [ -n "$waiver" ]; then
    printf 'warn: Reddit waived by the user: %s\n' "$waiver"
  else
    for key in REDDIT_CLIENT_ID REDDIT_CLIENT_SECRET; do
      if has_value "$key"; then
        say_ok "$key is set"
      else
        say_fail "$key" "not set"
        reddit_missing=1
      fi
    done
  fi

  if has_value YOUTUBE_API_KEY; then
    say_ok "YOUTUBE_API_KEY is set"
  else
    say_fail "YOUTUBE_API_KEY" "not set; add YOUTUBE_API_KEY=... to the .env file or the environment"
  fi

  for comp in reddit/reddit_source http-fetcher/http_fetcher youtube/youtube_source; do
    rel="plugins/${comp%%/*}/target/wasm32-unknown-unknown/release/${comp##*/}.wasm"
    fix="build it: (cd plugins/${comp%%/*} && cargo component build --release)"
    if [ -f "$rel" ]; then
      say_ok "component $rel exists"
    else
      say_fail "component $rel" "$fix"
    fi
  done

  if [ -f "$TOKENIZER_PATH" ]; then
    say_ok "GLiNER tokenizer.json exists"
  else
    say_fail "GLiNER tokenizer.json" "not found at $TOKENIZER_PATH; see README.md 'GLiNER NER Setup'"
  fi
  if [ -f "$MODEL_PATH" ]; then
    say_ok "GLiNER model.onnx exists"
  else
    say_fail "GLiNER model.onnx" "not found at $MODEL_PATH; see README.md 'GLiNER NER Setup'"
  fi

  if [ "$reddit_missing" -eq 1 ]; then
    echo "STOP (D-09): Reddit credentials are missing. Do not drop Reddit silently: ask the user whether to waive it, then rerun with LIVE_E2E_WAIVE_REDDIT set to the user's reason."
    return 3
  fi
  if [ "$FAILS" -ne 0 ]; then
    return 1
  fi
  return 0
}

# ── sanitise ─────────────────────────────────────────────────────────────────

# sanitise <in.json> <out.json>
cmd_sanitise() {
  local in="${1:-}" out="${2:-}" tmp
  if [ -z "$in" ] || [ -z "$out" ]; then
    usage
    return 2
  fi
  if ! jq -e 'type == "array"' "$in" >/dev/null 2>&1; then
    say_fail "sanitise input" "$in is not a JSON array"
    return 1
  fi
  tmp="$(mktemp "${out}.XXXXXX")"
  if jq --argjson n "$SAMPLE_PER_TYPE" '
      def mail: if type == "string"
                then gsub("[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}"; "[email]")
                else . end;
      group_by([.source_type, .platform])
      | map(.[0:$n])
      | add // []
      | map(
          .raw = null
          | .author = (if .author == null then null else "redacted" end)
          | .text = (((.text // "") | mail)[0:280])
          | (if .title == null then . else .title = (.title | mail) end)
          | .entities = ((.entities // [])
              | map(if .kind == "PERSON" then .text = "[redacted]" else .text = (.text | mail) end))
        )
    ' "$in" >"$tmp"; then
    mv "$tmp" "$out"
    say_ok "sanitised sample written to $out"
  else
    rm -f "$tmp"
    say_fail "sanitise" "jq failed on $in"
    return 1
  fi
}

# ── check-evidence ───────────────────────────────────────────────────────────

# Tests one evidence file and one value, which is passed through a pattern file, never echoed.
scan_value() {
  local key="$1" val="$2" pf="$3" file
  [ "${#val}" -ge 6 ] || return 0
  printf '%s\n' "$val" >"$pf"
  for file in "$EV_DIR"/*; do
    [ -f "$file" ] || continue
    if grep -F -q -f "$pf" "$file"; then
      say_fail "evidence contains the value of $key" "found in $(basename "$file"); remove it and rerun"
      return 0
    fi
  done
  return 0
}

# check-evidence <dir>
cmd_check_evidence() {
  local dir="${1:-}" sample transcript pf line key file jqfilter what
  if [ -z "$dir" ]; then
    usage
    return 2
  fi
  EV_DIR="$dir"
  FAILS=0
  sample="$dir/enriched-sample.json"
  transcript="$dir/live-e2e-transcript.txt"

  [ -f "$sample" ] || say_fail "enriched-sample.json" "missing in $dir; run: scripts/live-e2e.sh run"
  [ -f "$transcript" ] || say_fail "live-e2e-transcript.txt" "missing in $dir; run: scripts/live-e2e.sh run"

  if [ -f "$sample" ]; then
    if jq -e 'type == "array"' "$sample" >/dev/null 2>&1; then
      check_sample() { # <what> <fix> <jq filter>
        if ! jq -e "$3" "$sample" >/dev/null 2>&1; then
          say_fail "$1" "$2"
        fi
      }
      check_sample "sample has a news record" "raise LIMIT or change NEWS_SEED_URL and rerun" \
        'map(select(.source_type == "news")) | length > 0'
      check_sample "sample has a social record" "raise LIMIT and rerun" \
        'map(select(.source_type == "social")) | length > 0'
      check_sample "sample has a youtube record" "raise LIMIT and rerun" \
        'map(select(.platform == "youtube")) | length > 0'
      if [ ! -f "$dir/WAIVER.md" ]; then
        check_sample "sample has a reddit record" "raise LIMIT, or waive Reddit with the user's reason (D-09)" \
          'map(select(.platform == "reddit")) | length > 0'
      fi
      check_sample "a social record has entities" "run the consumer with --enrich-ner and the GLiNER model" \
        'map(select(.source_type == "social" and ((.entities // []) | length > 0))) | length > 0'
      check_sample "raw is null on every record" "sanitise the output again" \
        'all(.[]; .raw == null)'
      check_sample "author is null or redacted on every record" "sanitise the output again" \
        'all(.[]; .author == null or .author == "redacted")'
      check_sample "every PERSON entity is redacted" "sanitise the output again" \
        'all(.[]; all((.entities // [])[]; .kind != "PERSON" or .text == "[redacted]"))'
    else
      say_fail "enriched-sample.json" "is not a JSON array"
    fi
  fi

  # Secret scan: every non-empty .env value of length 6 or more, plus the three credential
  # variables from the process environment. A hit names the key, never the value.
  pf="$(mktemp)"
  chmod 600 "$pf"
  if [ -f "$ENV_FILE" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
      if dotenv_parse_line "$line"; then
        scan_value "$PL_KEY" "$PL_VAL" "$pf"
      fi
    done <"$ENV_FILE"
  fi
  for key in REDDIT_CLIENT_ID REDDIT_CLIENT_SECRET YOUTUBE_API_KEY; do
    scan_value "$key" "$(env_value "$key")" "$pf"
  done
  rm -f "$pf"

  for file in "$dir"/*; do
    [ -f "$file" ] || continue
    if grep -i -q -e 'bearer' -e 'access_token' "$file"; then
      say_fail "evidence has a token marker" "found in $(basename "$file"); remove it and rerun"
    fi
  done

  if [ "$FAILS" -ne 0 ]; then
    return 1
  fi
  say_ok "evidence is complete and clean"
  return 0
}

# ── run ──────────────────────────────────────────────────────────────────────

# A fili invocation with the environment that could redirect it removed.
fili_run() {
  env -u FILI_REGISTRY -u FILI_STATE_DIR -u RUST_LOG "$FILI_BIN" "$@"
}

FILI_RUN_PID=""

stop_fili_run() {
  local i
  [ -n "$FILI_RUN_PID" ] || return 0
  if kill -0 "$FILI_RUN_PID" 2>/dev/null; then
    kill -INT "$FILI_RUN_PID" 2>/dev/null || true
    i=0
    while kill -0 "$FILI_RUN_PID" 2>/dev/null && [ "$i" -lt 30 ]; do
      sleep 1
      i=$((i + 1))
    done
    if kill -0 "$FILI_RUN_PID" 2>/dev/null; then
      echo "warn: fili run did not stop on SIGINT within 30 seconds; sending SIGTERM"
      kill -TERM "$FILI_RUN_PID" 2>/dev/null || true
      i=0
      while kill -0 "$FILI_RUN_PID" 2>/dev/null && [ "$i" -lt 10 ]; do
        sleep 1
        i=$((i + 1))
      done
      kill -KILL "$FILI_RUN_PID" 2>/dev/null || true
    fi
  fi
  wait "$FILI_RUN_PID" 2>/dev/null || true
  FILI_RUN_PID=""
}

# Writes the live registry and plugin configs into the work directory.
write_live_registry() {
  local waived="$1" client_id="" contact ua_contact
  mkdir -p "$WORK" "$WORK/state"
  # The registry loader reads .env next to the registry; it is linked, never copied.
  ln -sf "$ENV_FILE" "$WORK/.env"

  if [ -z "$waived" ]; then
    client_id="$(env_value REDDIT_CLIENT_ID)"
    case "$client_id" in
      *[!A-Za-z0-9._-]*)
        say_fail "REDDIT_CLIENT_ID" "has characters outside letters, digits, dot, dash and underscore"
        return 1
        ;;
    esac
    contact="$REDDIT_USER_AGENT_CONTACT"
    case "$contact" in
      *[!A-Za-z0-9._/@-]*)
        say_fail "REDDIT_USER_AGENT_CONTACT" "has characters outside letters, digits and ._/@-"
        return 1
        ;;
    esac
    ua_contact="${contact#/}"
    {
      printf '%s\n' '# Live E2E Reddit config (generated by scripts/live-e2e.sh; gitignored).'
      printf '%s\n' '[host]'
      printf '%s\n' 'poll_interval_secs = 300'
      printf '%s\n' 'timeout_secs = 30'
      printf '%s\n' 'allowlist = ["oauth.reddit.com"]'
      printf '\n%s\n' '[host.backoff]'
      printf '%s\n' 'initial_secs = 60'
      printf '%s\n' 'max_secs = 3600'
      printf '%s\n' 'multiplier = 2.0'
      printf '\n%s\n' '[host.oauth]'
      printf '%s\n' 'grant = "client_credentials"'
      printf '%s\n' 'token_url = "https://www.reddit.com/api/v1/access_token"'
      printf 'client_id = "%s"\n' "$client_id"
      printf '%s\n' 'client_secret_env = "REDDIT_CLIENT_SECRET"'
      printf 'user_agent = "filimon:reddit:0.1.0 (by /%s)"\n' "$ua_contact"
      printf '\n%s\n' '[settings]'
      printf '%s\n' 'subreddits = ["rust", "programming"]'
      printf '%s\n' 'listings = ["new"]'
      printf 'user_agent_contact = "%s"\n' "$ua_contact"
    } >"$WORK/reddit.toml"
  fi

  cp "$REPO_ROOT/examples/plugins/youtube.toml" "$WORK/youtube.toml"

  {
    printf '%s\n' '# Live E2E registry (generated by scripts/live-e2e.sh; gitignored).'
    if [ -z "$waived" ]; then
      printf '\n%s\n' '[sources.reddit]'
      printf "component = '%s'\n" "$REPO_ROOT/plugins/reddit/target/wasm32-unknown-unknown/release/reddit_source.wasm"
      printf '%s\n' 'config = "reddit.toml"'
      printf '%s\n' 'enabled = true'
      printf '%s\n' 'fetcher = "http"'
    fi
    printf '\n%s\n' '[sources.youtube]'
    printf "component = '%s'\n" "$REPO_ROOT/plugins/youtube/target/wasm32-unknown-unknown/release/youtube_source.wasm"
    printf '%s\n' 'config = "youtube.toml"'
    printf '%s\n' 'enabled = true'
    printf '%s\n' 'fetcher = "http"'
    printf '\n%s\n' '[fetchers.http]'
    printf "component = '%s'\n" "$REPO_ROOT/plugins/http-fetcher/target/wasm32-unknown-unknown/release/http_fetcher.wasm"
  } >"$WORK/filimon.toml"
}

# Everything `run` does up to and including the sample. Its output is teed into the transcript.
run_steps() {
  local waiver="${LIVE_E2E_WAIVE_REDDIT:-}" rc deadline list i

  echo "live E2E run (TEST-04)"
  date -u
  if [ -x "$FILI_BIN" ]; then
    fili_run --version || true
  fi
  echo "work directory: $WORK"
  echo "note: the production topics keep earlier records for 7 days; a rerun can read older records."

  case "$LIMIT" in
    '' | *[!0-9]* | 0)
      say_fail "LIMIT" "must be a positive number"
      return 1
      ;;
  esac
  case "$SAMPLE_PER_TYPE" in
    '' | *[!0-9]* | 0)
      say_fail "SAMPLE_PER_TYPE" "must be a positive number"
      return 1
      ;;
  esac

  rc=0
  cmd_preflight || rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "preflight failed (exit $rc); nothing was run"
    return "$rc"
  fi

  write_live_registry "$waiver" || return 1
  rm -f "$EVIDENCE_DIR/WAIVER.md"
  if [ -n "$waiver" ]; then
    {
      printf 'Reddit waived (D-09): %s\n' "$waiver"
      printf '%s\n' "$(date -u +%Y-%m-%d)"
    } >"$EVIDENCE_DIR/WAIVER.md"
  fi

  trap 'stop_fili_run' EXIT
  trap 'exit 130' INT TERM

  echo "step 1: news crawl of $NEWS_SEED_URL (fili produce)"
  if ! fili_run produce "$NEWS_SEED_URL"; then
    say_fail "fili produce" "the news crawl failed"
    return 1
  fi

  echo "step 2: social run (fili run) until every enabled source reports a successful poll"
  : >"$WORK/run.log"
  set -m
  # Not through fili_run: a function would be forked into a subshell and $! would name that
  # subshell, not fili. env execs fili, so $! is fili's own pid.
  env -u FILI_REGISTRY -u FILI_STATE_DIR -u RUST_LOG "$FILI_BIN" \
    --registry "$WORK/filimon.toml" --state-dir "$WORK/state" run >"$WORK/run.log" 2>&1 </dev/null &
  FILI_RUN_PID=$!
  set +m
  deadline=$((SECONDS + 180))
  while :; do
    if ! kill -0 "$FILI_RUN_PID" 2>/dev/null; then
      say_fail "fili run" "exited before every source polled; see $WORK/run.log"
      FILI_RUN_PID=""
      return 1
    fi
    list="$(fili_run --registry "$WORK/filimon.toml" --state-dir "$WORK/state" plugins list --json 2>/dev/null || true)"
    if printf '%s' "$list" | jq -e '[.[] | select(.kind == "source" and .enabled == true)] | (length > 0) and (map(.last_poll.state == "ok") | all)' >/dev/null 2>&1; then
      break
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
      say_fail "fili run" "not every enabled source reported a successful poll within 180 seconds; see $WORK/run.log"
      return 1
    fi
    sleep 3
  done
  echo "all enabled sources polled successfully; stopping fili run"
  stop_fili_run
  printf '%s' "$list" | jq -r '.[] | select(.kind == "source") | "source \(.name): \(.last_poll.state)"'

  echo "step 3: consume with NER (fili consume --enrich-ner)"
  rm -f "$WORK/enriched.json"
  if ! fili_run --tokenizer "$TOKENIZER_PATH" --model "$MODEL_PATH" consume \
    --from-beginning --limit "$LIMIT" --enrich-ner --source both --output "$WORK/enriched.json"; then
    say_fail "fili consume" "the enriched consume failed"
    return 1
  fi
  if ! jq -e 'type == "array"
      and (map(select(.source_type == "news")) | length > 0)
      and (map(select(.source_type == "social")) | length > 0)' "$WORK/enriched.json" >/dev/null 2>&1; then
    say_fail "consumer output lacks news or social records" "raise LIMIT (now $LIMIT) and rerun; the topics may hold older records of one type"
    return 1
  fi
  echo "raw output ready: $WORK/enriched.json"
  echo "reminder: the production topics keep earlier records for 7 days; reruns accumulate."

  echo "step 4: sanitise the sample"
  cmd_sanitise "$WORK/enriched.json" "$EVIDENCE_DIR/enriched-sample.json" || return 1
  return 0
}

cmd_run() {
  local rc=0 transcript
  mkdir -p "$EVIDENCE_DIR"
  transcript="$EVIDENCE_DIR/live-e2e-transcript.txt"
  rm -f "$EVIDENCE_DIR/enriched-sample.json"

  set +e
  run_steps 2>&1 | tee "$transcript"
  rc=${PIPESTATUS[0]}
  set -e
  if [ "$rc" -ne 0 ]; then
    echo "run failed (exit $rc); the evidence is incomplete"
    return "$rc"
  fi

  # The transcript is closed, so the scan sees the finished file.
  rc=0
  cmd_check_evidence "$EVIDENCE_DIR" || rc=$?
  if [ "$rc" -ne 0 ]; then
    return "$rc"
  fi
  echo "live E2E run complete: evidence is in $EVIDENCE_DIR"
  return 0
}

# ── selftest ─────────────────────────────────────────────────────────────────

selftest_fail() {
  echo "SELFTEST: FAIL $1"
  exit 1
}

cmd_selftest() {
  local t out rc long ev ev2
  t="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$t'" EXIT
  mkdir -p "$t/bin"
  # A fake fluvio keeps the preflight runs independent of any real cluster.
  printf '#!/bin/sh\nexit 1\n' >"$t/bin/fluvio"
  chmod +x "$t/bin/fluvio"

  # Everything below is synthetic: fake credentials, fake records.
  printf '%s\n' \
    'REDDIT_CLIENT_ID=selftest-client-id-0001' \
    "REDDIT_CLIENT_SECRET='selftest-client-secret-0002'" \
    'YOUTUBE_API_KEY=selftest-api-key-0003' >"$t/fake.env"
  : >"$t/empty.env"

  long="$(printf 'x%.0s' $(seq 1 600))"
  jq -n --arg long "$long" '
    def rec($type; $platform; $i):
      { source_type: $type, platform: $platform, id: "\($platform)-\($i)",
        url: "https://example.org/\($platform)/\($i)",
        title: (if $i == 1 then "Mail to jane@example.org" else "Title \($i)" end),
        text: (if $i == 0 then "Contact john.doe@example.com now " + $long else $long end),
        author: "Some Author",
        published: null,
        raw: (if $type == "news" then null else { payload: "private" } end),
        entities: (if $type == "social"
                   then [ { text: "Alice Example", kind: "PERSON" }, { text: "Mozilla", kind: "ORGANIZATION" } ]
                   else [] end),
        article: (if $type == "news"
                  then { relevance_score: 0.5, top_keywords: ["rust"], inferred_category: "tech", word_count: 100, published_raw: null }
                  else null end) };
    [ range(0; 4) as $i | rec("news"; "blog.example.org"; $i) ]
    + [ range(0; 4) as $i | rec("social"; "reddit"; $i) ]
    + [ range(0; 4) as $i | rec("social"; "youtube"; $i) ]
  ' >"$t/in.json"

  # 1. sanitise
  SAMPLE_PER_TYPE=3 "$SELF" sanitise "$t/in.json" "$t/sample.json" >"$t/out.txt" 2>&1 \
    || selftest_fail "sanitise exited non-zero"
  jq -e 'group_by([.source_type, .platform]) | (length == 3) and (map(length) | all(. <= 3)) and (map(length) | add == 9)' \
    "$t/sample.json" >/dev/null || selftest_fail "sanitise did not keep at most 3 records per group"
  jq -e 'all(.[]; .raw == null)' "$t/sample.json" >/dev/null || selftest_fail "raw is not null everywhere"
  jq -e 'all(.[]; .author == "redacted")' "$t/sample.json" >/dev/null || selftest_fail "authors are not redacted"
  jq -e 'all(.[]; (.text | length) <= 280)' "$t/sample.json" >/dev/null || selftest_fail "a text is longer than 280 characters"
  jq -e 'all(.[]; all(.entities[]; .kind != "PERSON" or .text == "[redacted]"))' "$t/sample.json" >/dev/null \
    || selftest_fail "a PERSON entity is not redacted"
  jq -e 'any(.[]; any(.entities[]; .kind == "ORGANIZATION" and .text == "Mozilla"))' "$t/sample.json" >/dev/null \
    || selftest_fail "a non-PERSON entity was dropped"
  jq -e 'any(.[]; .article != null)' "$t/sample.json" >/dev/null || selftest_fail "the article object was dropped"
  jq -e 'tojson | contains("@") | not' "$t/sample.json" >/dev/null || selftest_fail "an e-mail address remains"

  # 2. check-evidence on the clean sample
  printf 'ok: synthetic transcript\nstep 1: news\n' >"$t/transcript.txt"
  mk_evidence() { # <dir> [<jq filter applied to the sample>]
    mkdir -p "$1"
    jq "${2:-.}" "$t/sample.json" >"$1/enriched-sample.json"
    cp "$t/transcript.txt" "$1/live-e2e-transcript.txt"
  }
  ev="$t/ev-clean"
  mk_evidence "$ev"
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY ENV_FILE="$t/fake.env" \
    "$SELF" check-evidence "$ev" >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 0 ] || selftest_fail "check-evidence rejected a clean sample"
  grep -q 'ok: evidence is complete and clean' "$t/out.txt" || selftest_fail "check-evidence printed no success line"

  # 3. a planted secret value fails, names the key and stays out of the output
  ev2="$t/ev-secret"
  mk_evidence "$ev2" '.[0].text = "leaked selftest-client-secret-0002 here"'
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY ENV_FILE="$t/fake.env" \
    "$SELF" check-evidence "$ev2" >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 1 ] || selftest_fail "a planted secret did not fail check-evidence"
  grep -q 'evidence contains the value of REDDIT_CLIENT_SECRET' "$t/out.txt" || selftest_fail "the planted secret key was not named"
  if grep -q 'selftest-client-secret-0002' "$t/out.txt"; then
    selftest_fail "the planted secret value reached the output"
  fi

  # 3b. a token marker fails
  ev2="$t/ev-marker"
  mk_evidence "$ev2"
  printf 'Authorization: Bearer something\n' >>"$ev2/live-e2e-transcript.txt"
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY ENV_FILE="$t/fake.env" \
    "$SELF" check-evidence "$ev2" >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 1 ] || selftest_fail "a token marker did not fail check-evidence"

  # 4. no social entities fails
  ev2="$t/ev-noent"
  mk_evidence "$ev2" 'map(if .source_type == "social" then .entities = [] else . end)'
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY ENV_FILE="$t/fake.env" \
    "$SELF" check-evidence "$ev2" >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 1 ] || selftest_fail "a sample without social entities did not fail"
  grep -q 'FAIL: a social record has entities' "$t/out.txt" || selftest_fail "the missing entities were not named"

  # 5. the waiver rule: no reddit records fail without WAIVER.md and pass with it
  ev2="$t/ev-noreddit"
  mk_evidence "$ev2" 'map(select(.platform != "reddit"))'
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY ENV_FILE="$t/fake.env" \
    "$SELF" check-evidence "$ev2" >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 1 ] || selftest_fail "a sample without reddit records and without a waiver did not fail"
  printf 'Reddit waived (D-09): selftest reason\n2000-01-01\n' >"$ev2/WAIVER.md"
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY ENV_FILE="$t/fake.env" \
    "$SELF" check-evidence "$ev2" >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 0 ] || selftest_fail "a waived sample without reddit records did not pass"

  # 6. the D-09 gate (the fake fluvio first on PATH keeps this independent of a real cluster)
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY -u LIVE_E2E_WAIVE_REDDIT \
    PATH="$t/bin:$PATH" ENV_FILE="$t/empty.env" "$SELF" preflight >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -eq 3 ] || selftest_fail "preflight without Reddit credentials did not exit 3"
  grep -q 'STOP (D-09)' "$t/out.txt" || selftest_fail "preflight printed no STOP (D-09) line"
  grep -q 'FAIL: REDDIT_CLIENT_ID' "$t/out.txt" || selftest_fail "preflight did not name REDDIT_CLIENT_ID"

  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY \
    PATH="$t/bin:$PATH" LIVE_E2E_WAIVE_REDDIT="selftest waiver" ENV_FILE="$t/empty.env" \
    "$SELF" preflight >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -ne 3 ] || selftest_fail "a waived preflight still exited 3"
  grep -q 'warn: Reddit waived by the user' "$t/out.txt" || selftest_fail "a waived preflight printed no waiver warning"

  # 7. preflight with fake credentials never prints a value
  rc=0
  env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY -u LIVE_E2E_WAIVE_REDDIT \
    PATH="$t/bin:$PATH" ENV_FILE="$t/fake.env" "$SELF" preflight >"$t/out.txt" 2>&1 || rc=$?
  [ "$rc" -ne 3 ] || selftest_fail "preflight with all credentials exited 3"
  grep -q 'ok: REDDIT_CLIENT_ID is set' "$t/out.txt" || selftest_fail "preflight did not report REDDIT_CLIENT_ID as set"
  if grep -q 'selftest-' "$t/out.txt"; then
    selftest_fail "a credential value reached the preflight output"
  fi

  echo "SELFTEST: PASS"
}

# ── dispatcher ───────────────────────────────────────────────────────────────

main() {
  local sub="${1:-}" rc=0
  [ "$#" -gt 0 ] && shift
  case "$sub" in
    preflight) cmd_preflight || rc=$? ;;
    run) cmd_run || rc=$? ;;
    sanitise) cmd_sanitise "$@" || rc=$? ;;
    check-evidence) cmd_check_evidence "$@" || rc=$? ;;
    selftest) cmd_selftest || rc=$? ;;
    *)
      usage
      rc=2
      ;;
  esac
  exit "$rc"
}

main "$@"
