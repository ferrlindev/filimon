#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# verify-authoring.sh - prove a plugin author can build a source plugin outside
# this repository and run it in the unmodified `fili` binary (WIT-03, DOCS-01).
#
# What it does, in a fresh temporary directory outside the repository:
#   1. copies plugins/template and the published wit/ directory,
#   2. rewrites only the WIT path, the crate name and the component package,
#   3. builds with `cargo component build --release`,
#   4. registers the component in a standalone filimon.toml,
#   5. runs `fili plugins check` and `fili plugins run-once --json` on it,
#   6. requires the `fili` binary and the repository tree to be unchanged.
#
# The script never builds `fili`: build it first with
#   cargo build -p filimon
#
# Where the template and the WIT come from (no registry, no wkg/warg):
#   local   (default) the working-tree template and `git archive HEAD wit`;
#           nothing touches the network and no tag is needed.
#   remote  with WIT_REF set, a sparse clone of the template and wit/ at that
#           ref from a git remote. This is what an outside author gets from a
#           tagged release. Nothing in the clone is run, sourced or evaluated:
#           the script only copies files out of it and builds the copied crate.
#
# Environment:
#   FILI_BIN          the fili binary (default: <repo>/target/debug/fili)
#   WIT_REF           a tag or branch; selects remote mode
#   WIT_REPO          the remote to clone in remote mode (default: the url of
#                     this repository's `origin`)
#   KEEP_WORK=1       keep the temporary directory (its path is printed)
#   PROOF_TRANSCRIPT  append everything the script prints to this file too
#
# The script prints paths and fixed texts only, never an environment value.
#
# Exit codes: 0 pass, 1 a step failed, 2 the fili binary is missing.
# ─────────────────────────────────────────────────────────────────────────────
set -Eeuo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
FILI_BIN=${FILI_BIN:-$REPO/target/debug/fili}
WIT_REF=${WIT_REF:-}
KEEP_WORK=${KEEP_WORK:-0}

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

# ── Step 1: record the starting state ────────────────────────────────────────
step "record"
HASH_BEFORE=$(sha256_of "$FILI_BIN")
TREE_BEFORE=$(git -C "$REPO" status --porcelain)

# ── Step 2: a fresh directory outside the repository ─────────────────────────
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

# ── Step 3: get the template and the published WIT ───────────────────────────
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
    git -C "$SRC" sparse-checkout set wit plugins/template >"$WORK/sparse.log" 2>&1 || {
        tail -n 10 "$WORK/sparse.log" >&2
        fail "sparse-checkout set wit plugins/template failed"
    }
    [ -f "$SRC/plugins/template/Cargo.toml" ] || fail "the clone has no plugins/template at ref $WIT_REF"
    [ -f "$SRC/wit/worlds.wit" ] || fail "the clone has no wit/ at ref $WIT_REF"
    TEMPLATE_SRC="$SRC/plugins/template"

    step "copy"
    tar -C "$TEMPLATE_SRC" --exclude=./target --exclude=./src/bindings.rs -cf - . |
        tar -C "$PLUGIN" -xf - || fail "cannot copy plugins/template from the clone"
    tar -C "$SRC" -cf - wit | tar -C "$PLUGIN" -xf - || fail "cannot copy wit/ from the clone"
else
    step "copy"
    tar -C "$REPO/plugins/template" --exclude=./target --exclude=./src/bindings.rs -cf - . |
        tar -C "$PLUGIN" -xf - || fail "cannot copy plugins/template"

    step "wit"
    git -C "$REPO" archive HEAD wit | tar -x -C "$PLUGIN" ||
        fail "cannot read wit/ from HEAD (is it committed?)"
fi
[ -f "$PLUGIN/wit/worlds.wit" ] || fail "wit/worlds.wit is missing after the copy"

# ── Step 4: the rename an author does by hand ────────────────────────────────
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

# ── Step 5: build outside the repository ─────────────────────────────────────
step "build"
(cd "$PLUGIN" && cargo component build --release) >"$WORK/build.log" 2>&1 || {
    tail -n 30 "$WORK/build.log" >&2
    fail "cargo component build --release failed"
}
WASM="$PLUGIN/target/wasm32-unknown-unknown/release/my_source.wasm"
[ -f "$WASM" ] || fail "the build did not produce $WASM"

step "conformance"
wasm-tools component targets "$PLUGIN/wit" -w source "$WASM" >"$WORK/targets.log" 2>&1 || {
    cat "$WORK/targets.log" >&2
    fail "the component does not fit world source"
}
if wasm-tools component wit "$WASM" | grep -q 'wasi:'; then
    fail "the component imports wasi: (the build target is not wasm32-unknown-unknown; copy .cargo/config.toml and rust-toolchain.toml)"
fi

# ── Step 6: register it in a standalone filimon.toml ─────────────────────────
step "register"
cat >"$WORK/filimon.toml" <<EOF
[sources.my-source]
component = "$WASM"
config = "my-source.toml"
EOF
cp "$PLUGIN/template.toml" "$WORK/my-source.toml" || fail "cannot copy template.toml"

# ── Step 7: the unmodified binary checks and polls it ────────────────────────
fili() {
    (cd "$WORK" && env -u FILI_REGISTRY -u FILI_STATE_DIR -u RUST_LOG "$FILI_BIN" \
        --registry "$WORK/filimon.toml" --state-dir "$WORK/state" "$@")
}

step "check"
fili plugins check my-source >"$WORK/check.out" 2>"$WORK/check.err" || {
    cat "$WORK/check.out" "$WORK/check.err" >&2
    fail "fili plugins check my-source exited non-zero"
}
grep -q 'component: ok' "$WORK/check.out" || fail "fili plugins check did not print 'component: ok'"

step "run-once"
fili plugins run-once my-source --json >"$WORK/run.out" 2>"$WORK/run.err" || {
    cat "$WORK/run.err" >&2
    fail "fili plugins run-once my-source exited non-zero"
}
[ "$(wc -l <"$WORK/run.out" | tr -d ' ')" = "1" ] || fail "run-once did not print exactly one line"
GREETING=$(sed -n 's/^greeting = "\(.*\)"$/\1/p' "$WORK/my-source.toml")
jq -e --arg g "$GREETING" \
    '.platform == "template" and (.id | startswith("template-")) and .text == $g' \
    "$WORK/run.out" >/dev/null || fail "the polled item is not the template's greeting item"

# ── Step 8: nothing outside the temporary directory changed ──────────────────
step "hash"
HASH_AFTER=$(sha256_of "$FILI_BIN")
[ "$HASH_BEFORE" = "$HASH_AFTER" ] || fail "the fili binary changed during the proof"

step "tree-untouched"
TREE_AFTER=$(git -C "$REPO" status --porcelain)
[ "$TREE_BEFORE" = "$TREE_AFTER" ] || fail "the repository working tree changed during the proof"

echo "PASS: source leg"
echo "AUTHORING PROOF: PASS (legs: source)"
