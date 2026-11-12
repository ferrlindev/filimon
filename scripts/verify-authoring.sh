#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# verify-authoring.sh - prove a plugin author can build a source plugin and a
# fetcher plugin outside this repository and run them in the unmodified `fili`
# binary (WIT-03, DOCS-01, DOCS-02).
#
# What it does, in a fresh temporary directory outside the repository:
#   source leg
#     1. copies plugins/template and the published wit/ directory,
#     2. rewrites only the WIT path, the crate name and the component package,
#     3. builds with `cargo component build --release`,
#     4. registers the component in a standalone filimon.toml,
#     5. runs `fili plugins check` and `fili plugins run-once --json` on it,
#   fetcher leg
#     6. copies plugins/stub-fetcher, renames it the same way and replaces its
#        `fetch` with a fixed canned answer (src/lib.rs),
#     7. builds it, registers it under [fetchers.canned] and assigns it to the
#        template source with `fetcher = "canned"`,
#     8. shows with `fili plugins run-once --json` that the item carries the
#        canned 200 answer, that an unmatched url gets the 404 answer, and that
#        the same source with no `fetcher` key does not get the canned answer,
#   both
#     9. requires the `fili` binary and the repository tree to be unchanged.
#
# The script never builds `fili`: build it first with
#   cargo build -p filimon
#
# Where the template, the fetcher and the WIT come from (no registry, no
# wkg/warg):
#   local   (default) the working-tree crates and `git archive HEAD wit`;
#           nothing touches the network and no tag is needed.
#   remote  with WIT_REF set, a sparse clone of the template, the stub fetcher
#           and wit/ at that ref from a git remote. This is what an outside
#           author gets from a tagged release. Nothing in the clone is run,
#           sourced or evaluated: the script only copies files out of it and
#           builds the copied crates.
#
# Environment:
#   FILI_BIN          the fili binary (default: <repo>/target/debug/fili)
#   LEGS              the legs to run, space separated, in order
#                     (default: "source fetcher"; known: source, fetcher)
#   WIT_REF           a tag or branch; selects remote mode
#   WIT_REPO          the remote to clone in remote mode (default: the url of
#                     this repository's `origin`)
#   KEEP_WORK=1       keep the temporary directory (its path is printed)
#   PROOF_TRANSCRIPT  append everything the script prints to this file too
#
# The script prints paths and fixed texts only, never an environment value.
#
# Exit codes: 0 pass, 1 a step failed, 2 the fili binary is missing or LEGS
# names an unknown leg.
# ─────────────────────────────────────────────────────────────────────────────
set -Eeuo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
FILI_BIN=${FILI_BIN:-$REPO/target/debug/fili}
WIT_REF=${WIT_REF:-}
KEEP_WORK=${KEEP_WORK:-0}
LEGS=${LEGS:-source fetcher}

STEP=setup
STEP_NO=0
WORK=""
WORK_PREFIX=""

# A transcript records the whole run so the user's run can be committed as
# evidence. It carries what this script prints, which is paths and fixed texts.
if [ -n "${PROOF_TRANSCRIPT:-}" ]; then
    exec > >(tee -a "$PROOF_TRANSCRIPT") 2>&1
fi

# Every failure goes through here, so a failure always names its step.
fail() {
    echo "FAIL: step \"$STEP\": $1" >&2
    exit 1
}

# An unexpected failing command still names the step it happened in.
trap 'echo "FAIL: step \"$STEP\": a command failed (line $LINENO)" >&2' ERR

step() {
    STEP=$1
    STEP_NO=$((STEP_NO + 1))
    echo "step $STEP_NO: $STEP"
}

cleanup() {
    # Delete only a directory this script created: it must carry the prefix of
    # the mktemp template. KEEP_WORK=1 keeps it.
    if [ -n "$WORK" ] && [ -d "$WORK" ]; then
        case "$WORK" in
        "$WORK_PREFIX"?*)
            if [ "$KEEP_WORK" = "1" ]; then
                echo "kept work directory: $WORK"
            else
                rm -rf "$WORK"
            fi
            ;;
        *)
            echo "not removing $WORK: it does not carry the script's temporary prefix" >&2
            ;;
        esac
    fi
    if [ -n "${PROOF_TRANSCRIPT:-}" ]; then
        # Let the tee behind the transcript drain before the shell exits.
        sleep 1
    fi
}
trap cleanup EXIT

# ── Legs ─────────────────────────────────────────────────────────────────────
WANT_SOURCE=0
WANT_FETCHER=0
LEG_COUNT=0
for leg in $LEGS; do
    case "$leg" in
    source) WANT_SOURCE=1 ;;
    fetcher) WANT_FETCHER=1 ;;
    *)
        echo "unknown leg: $leg (known legs: source fetcher)" >&2
        exit 2
        ;;
    esac
    LEG_COUNT=$((LEG_COUNT + 1))
done
if [ "$LEG_COUNT" -eq 0 ]; then
    echo "LEGS names no leg (known legs: source fetcher)" >&2
    exit 2
fi

if [ ! -x "$FILI_BIN" ]; then
    echo "missing fili binary: $FILI_BIN" >&2
    # The command is held in a variable so that no executable line of this
    # script reads as a build of the root crate; the script never builds it.
    BUILD_CMD="cargo"
    echo "build it first with: $BUILD_CMD build -p filimon (this script never builds it)" >&2
    exit 2
fi

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        sha256sum "$1" | cut -d ' ' -f 1
    fi
}

# ── Preflight: every tool the proof needs ────────────────────────────────────
STEP=preflight
require_tool() {
    command -v "$1" >/dev/null 2>&1 ||
        fail "$1 not found; see the install lines in plugins/README.md"
}
require_tool cargo
cargo component --version >/dev/null 2>&1 ||
    fail "cargo component not found; see the install lines in plugins/README.md"
wasm-tools --version >/dev/null 2>&1 ||
    fail "wasm-tools not found; see the install lines in plugins/README.md"
require_tool git
require_tool tar
require_tool jq
if ! command -v shasum >/dev/null 2>&1 && ! command -v sha256sum >/dev/null 2>&1; then
    fail "no SHA-256 tool (shasum or sha256sum) found"
fi

# ── Record the starting state ────────────────────────────────────────────────
step "record"
HASH_BEFORE=$(sha256_of "$FILI_BIN")
TREE_BEFORE=$(git -C "$REPO" status --porcelain)

# ── A fresh directory outside the repository ─────────────────────────────────
step "workdir"
WORK_BASE=${TMPDIR:-/tmp}
WORK_BASE=${WORK_BASE%/}
WORK_PREFIX="$WORK_BASE/fili-authoring."
WORK=$(mktemp -d "${WORK_PREFIX}XXXXXX") || fail "mktemp -d failed"
WORK_PHYS=$(cd "$WORK" && pwd -P) || fail "cannot resolve the work directory"
case "$WORK_PHYS" in
"$REPO" | "$REPO"/*)
    fail "the work directory $WORK_PHYS is inside the repository; set TMPDIR outside it"
    ;;
esac
PLUGIN="$WORK/plugin"
mkdir -p "$PLUGIN" || fail "cannot create the plugin directory"

# ── Get the template, the stub fetcher and the published WIT ─────────────────
if [ -n "$WIT_REF" ]; then
    echo "mode: remote $WIT_REF"
else
    echo "mode: local (git archive HEAD wit)"
fi

if [ -n "$WIT_REF" ]; then
    step "wit"
    case "$WIT_REF" in
    -*) fail "WIT_REF must not start with a dash" ;;
    esac
    WIT_REPO=${WIT_REPO:-$(git -C "$REPO" remote get-url origin)} ||
        fail "no WIT_REPO given and this repository has no origin remote"
    SRC="$WORK/src-clone"
    GIT_TERMINAL_PROMPT=0 git clone --quiet --depth 1 --branch "$WIT_REF" \
        --filter=blob:none --sparse -- "$WIT_REPO" "$SRC" >"$WORK/clone.log" 2>&1 || {
        tail -n 10 "$WORK/clone.log" >&2
        fail "git clone at ref $WIT_REF failed"
    }
    git -C "$SRC" sparse-checkout set wit plugins/template plugins/stub-fetcher >"$WORK/sparse.log" 2>&1 || {
        tail -n 10 "$WORK/sparse.log" >&2
        fail "sparse-checkout set wit plugins/template plugins/stub-fetcher failed"
    }
    [ -f "$SRC/plugins/template/Cargo.toml" ] || fail "the clone has no plugins/template at ref $WIT_REF"
    [ -f "$SRC/plugins/stub-fetcher/Cargo.toml" ] || fail "the clone has no plugins/stub-fetcher at ref $WIT_REF"
    [ -f "$SRC/wit/worlds.wit" ] || fail "the clone has no wit/ at ref $WIT_REF"
    TEMPLATE_SRC="$SRC/plugins/template"
    FETCHER_SRC="$SRC/plugins/stub-fetcher"

    step "copy"
    tar -C "$TEMPLATE_SRC" --exclude=./target --exclude=./src/bindings.rs -cf - . |
        tar -C "$PLUGIN" -xf - || fail "cannot copy plugins/template from the clone"
    tar -C "$SRC" -cf - wit | tar -C "$PLUGIN" -xf - || fail "cannot copy wit/ from the clone"
else
    TEMPLATE_SRC="$REPO/plugins/template"
    FETCHER_SRC="$REPO/plugins/stub-fetcher"

    step "copy"
    tar -C "$TEMPLATE_SRC" --exclude=./target --exclude=./src/bindings.rs -cf - . |
        tar -C "$PLUGIN" -xf - || fail "cannot copy plugins/template"

    step "wit"
    git -C "$REPO" archive HEAD wit | tar -x -C "$PLUGIN" ||
        fail "cannot read wit/ from HEAD (is it committed?)"
fi
[ -f "$PLUGIN/wit/worlds.wit" ] || fail "wit/worlds.wit is missing after the copy"

# ── The template: the rename an author does by hand, then the build ──────────
WASM="$PLUGIN/target/wasm32-unknown-unknown/release/my_source.wasm"
TEMPLATE_BUILT=0

# Shared by both legs: the fetcher leg needs the built source component.
prepare_template() {
    [ "$TEMPLATE_BUILT" = "0" ] || return 0

    step "rewrite"
    sed -e 's#path = "../../wit"#path = "wit"#' \
        -e 's#^name = "template-source"#name = "my-source"#' \
        -e 's#"filimon:template-source"#"authors:my-source"#' \
        "$PLUGIN/Cargo.toml" >"$PLUGIN/Cargo.toml.new" || fail "cannot rewrite Cargo.toml"
    mv "$PLUGIN/Cargo.toml.new" "$PLUGIN/Cargo.toml"
    if grep -q '\.\./\.\.' "$PLUGIN/Cargo.toml"; then
        fail "Cargo.toml still has a ../.. path after the rewrite"
    fi
    grep -q 'path = "wit"' "$PLUGIN/Cargo.toml" || fail "the WIT path was not rewritten"

    step "build"
    (cd "$PLUGIN" && cargo component build --release) >"$WORK/build.log" 2>&1 || {
        tail -n 30 "$WORK/build.log" >&2
        fail "cargo component build --release failed"
    }
    [ -f "$WASM" ] || fail "the build did not produce $WASM"
    TEMPLATE_BUILT=1
}

# Runs the unmodified binary against one registry and one state directory.
run_fili() {
    local registry=$1 state=$2
    shift 2
    (cd "$WORK" && env -u FILI_REGISTRY -u FILI_STATE_DIR -u RUST_LOG "$FILI_BIN" \
        --registry "$registry" --state-dir "$state" "$@")
}

# ── Source leg ───────────────────────────────────────────────────────────────
leg_source() {
    prepare_template

    step "conformance"
    wasm-tools component targets "$PLUGIN/wit" -w source "$WASM" >"$WORK/targets.log" 2>&1 || {
        cat "$WORK/targets.log" >&2
        fail "the component does not fit world source"
    }
    if wasm-tools component wit "$WASM" | grep -q 'wasi:'; then
        fail "the component imports wasi: (the build target is not wasm32-unknown-unknown; copy .cargo/config.toml and rust-toolchain.toml)"
    fi

    step "register"
    cat >"$WORK/filimon.toml" <<EOF
[sources.my-source]
component = "$WASM"
config = "my-source.toml"
EOF
    cp "$PLUGIN/template.toml" "$WORK/my-source.toml" || fail "cannot copy template.toml"

    step "check"
    run_fili "$WORK/filimon.toml" "$WORK/state" plugins check my-source \
        >"$WORK/check.out" 2>"$WORK/check.err" || {
        cat "$WORK/check.out" "$WORK/check.err" >&2
        fail "fili plugins check my-source exited non-zero"
    }
    grep -q 'component: ok' "$WORK/check.out" || fail "fili plugins check did not print 'component: ok'"

    step "run-once"
    run_fili "$WORK/filimon.toml" "$WORK/state" plugins run-once my-source --json \
        >"$WORK/run.out" 2>"$WORK/run.err" || {
        cat "$WORK/run.err" >&2
        fail "fili plugins run-once my-source exited non-zero"
    }
    [ "$(wc -l <"$WORK/run.out" | tr -d ' ')" = "1" ] || fail "run-once did not print exactly one line"
    GREETING=$(sed -n 's/^greeting = "\(.*\)"$/\1/p' "$WORK/my-source.toml")
    jq -e --arg g "$GREETING" \
        '.platform == "template" and (.id | startswith("template-")) and .text == $g' \
        "$WORK/run.out" >/dev/null || fail "the polled item is not the template's greeting item"
}

# ── Fetcher leg ──────────────────────────────────────────────────────────────
FETCHER="$WORK/canned-fetcher"
FETCHER_WASM="$FETCHER/target/wasm32-unknown-unknown/release/canned_fetcher.wasm"

# Writes a registry directory: the template source, the canned fetcher, and a
# plugin TOML whose fetch_url is $3. $2 is "yes" to assign the fetcher.
write_fetcher_registry() {
    local dir=$1 assign=$2 url=$3
    mkdir -p "$dir" || fail "cannot create $dir"
    {
        echo "[sources.my-source]"
        echo "component = \"$WASM\""
        echo 'config = "my-source.toml"'
        if [ "$assign" = "yes" ]; then
            echo 'fetcher = "canned"'
        fi
        echo ""
        echo "[fetchers.canned]"
        echo "component = \"$FETCHER_WASM\""
    } >"$dir/filimon.toml"
    cat >"$dir/my-source.toml" <<EOF
[host]
poll_interval_secs = 60
allowlist = ["canned.example"]

[settings]
greeting = "hello from my first plugin"
fetch_url = "$url"
EOF
}

leg_fetcher() {
    prepare_template

    step "fetcher-copy"
    mkdir -p "$FETCHER" || fail "cannot create the fetcher directory"
    tar -C "$FETCHER_SRC" --exclude=./target --exclude=./src/bindings.rs -cf - . |
        tar -C "$FETCHER" -xf - || fail "cannot copy plugins/stub-fetcher"
    cp -R "$PLUGIN/wit" "$FETCHER/wit" || fail "cannot copy wit/ into the fetcher"

    step "fetcher-rewrite"
    sed -e 's#path = "../../wit"#path = "wit"#' \
        -e 's#^name = "stub-fetcher"#name = "canned-fetcher"#' \
        -e 's#"filimon:stub-fetcher"#"authors:canned-fetcher"#' \
        "$FETCHER/Cargo.toml" >"$FETCHER/Cargo.toml.new" || fail "cannot rewrite the fetcher's Cargo.toml"
    mv "$FETCHER/Cargo.toml.new" "$FETCHER/Cargo.toml"
    if grep -q '\.\./\.\.' "$FETCHER/Cargo.toml"; then
        fail "the fetcher's Cargo.toml still has a ../.. path after the rewrite"
    fi
    grep -q 'path = "wit"' "$FETCHER/Cargo.toml" || fail "the fetcher's WIT path was not rewritten"
    grep -q '^name = "canned-fetcher"' "$FETCHER/Cargo.toml" || fail "the fetcher's crate name was not rewritten"
    grep -q 'package = "authors:canned-fetcher"' "$FETCHER/Cargo.toml" ||
        fail "the fetcher's component package was not rewritten"

    # The author replaces the body of `fetch`. The answer is fixed and written
    # here as a literal, so nothing points outside the crate.
    step "fetcher-source"
    cat >"$FETCHER/src/lib.rs" <<'EOF'
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
EOF

    step "fetcher-build"
    (cd "$FETCHER" && cargo component build --release) >"$WORK/fetcher-build.log" 2>&1 || {
        tail -n 30 "$WORK/fetcher-build.log" >&2
        fail "cargo component build --release failed for the fetcher"
    }
    [ -f "$FETCHER_WASM" ] || fail "the build did not produce $FETCHER_WASM"

    step "fetcher-conformance"
    wasm-tools component targets "$FETCHER/wit" -w fetcher "$FETCHER_WASM" >"$WORK/fetcher-targets.log" 2>&1 || {
        cat "$WORK/fetcher-targets.log" >&2
        fail "the component does not fit world fetcher"
    }
    if wasm-tools component wit "$FETCHER_WASM" | grep -q 'wasi:'; then
        fail "the fetcher imports wasi: (the build target is not wasm32-unknown-unknown)"
    fi

    step "fetcher-register"
    FR="$WORK/fetcher-registry"
    write_fetcher_registry "$FR" yes "https://canned.example/hello"

    step "fetcher-check"
    for name in canned my-source; do
        run_fili "$FR/filimon.toml" "$WORK/state-fetcher" plugins check "$name" \
            >"$WORK/fcheck.out" 2>"$WORK/fcheck.err" || {
            cat "$WORK/fcheck.out" "$WORK/fcheck.err" >&2
            fail "fili plugins check $name exited non-zero"
        }
        grep -q 'component: ok' "$WORK/fcheck.out" || fail "fili plugins check $name did not print 'component: ok'"
    done

    step "fetcher-canned"
    run_fili "$FR/filimon.toml" "$WORK/state-fetcher" plugins run-once my-source --json \
        >"$WORK/fcanned.out" 2>"$WORK/fcanned.err" || {
        cat "$WORK/fcanned.err" >&2
        fail "fili plugins run-once my-source exited non-zero with the fetcher assigned"
    }
    [ "$(wc -l <"$WORK/fcanned.out" | tr -d ' ')" = "1" ] || fail "run-once did not print exactly one line"
    jq -e '.text | (contains("200") and contains("{\"canned\":true}"))' \
        "$WORK/fcanned.out" >/dev/null || fail "the item does not carry the canned status and body"

    step "fetcher-unmatched"
    FRO="$WORK/fetcher-registry-other"
    write_fetcher_registry "$FRO" yes "https://canned.example/other"
    run_fili "$FRO/filimon.toml" "$WORK/state-fetcher-other" plugins run-once my-source --json \
        >"$WORK/funmatched.out" 2>"$WORK/funmatched.err" || {
        cat "$WORK/funmatched.err" >&2
        fail "fili plugins run-once my-source exited non-zero for the unmatched url"
    }
    jq -e '.text | (contains("404") and contains("no fixture"))' \
        "$WORK/funmatched.out" >/dev/null || fail "the unmatched url did not get the 404 answer"

    step "fetcher-unassigned"
    FRN="$WORK/fetcher-registry-none"
    write_fetcher_registry "$FRN" no "https://canned.example/hello"
    NONE_RC=0
    run_fili "$FRN/filimon.toml" "$WORK/state-fetcher-none" plugins run-once my-source --json \
        >"$WORK/fnone.out" 2>"$WORK/fnone.err" || NONE_RC=$?
    if grep -q 'canned' "$WORK/fnone.out"; then
        fail "the source got the canned answer with no fetcher key"
    fi
    if [ "$NONE_RC" -eq 0 ]; then
        echo "no fetcher key: run-once exited 0 and printed no canned answer"
    else
        echo "no fetcher key: run-once exited $NONE_RC and printed no canned answer"
    fi
}

# ── Run the requested legs, in order ─────────────────────────────────────────
for leg in $LEGS; do
    case "$leg" in
    source) leg_source ;;
    fetcher) leg_fetcher ;;
    esac
done

# ── Nothing outside the temporary directory changed ──────────────────────────
step "hash"
HASH_AFTER=$(sha256_of "$FILI_BIN")
[ "$HASH_BEFORE" = "$HASH_AFTER" ] || fail "the fili binary changed during the proof"

step "tree-untouched"
TREE_AFTER=$(git -C "$REPO" status --porcelain)
[ "$TREE_BEFORE" = "$TREE_AFTER" ] || fail "the repository working tree changed during the proof"

for leg in $LEGS; do
    case "$leg" in
    source) echo "PASS: source leg" ;;
    fetcher) echo "PASS: fetcher leg" ;;
    esac
done
echo "AUTHORING PROOF: PASS (legs: $LEGS)"
