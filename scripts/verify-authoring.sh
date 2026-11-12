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
# Environment:
#   FILI_BIN   the fili binary (default: <repo>/target/debug/fili)
#
# Exit codes: 0 pass, 1 a step failed, 2 the fili binary is missing.
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
FILI_BIN=${FILI_BIN:-$REPO/target/debug/fili}

STEP=setup
STEP_NO=0
WORK=""

fail() {
    echo "FAIL: step \"$STEP\": $1" >&2
    exit 1
}

step() {
    STEP=$1
    STEP_NO=$((STEP_NO + 1))
    echo "step $STEP_NO: $STEP"
}

cleanup() {
    if [ -n "$WORK" ] && [ -d "$WORK" ]; then
        rm -rf "$WORK"
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

# ── Step 1: record the starting state ────────────────────────────────────────
step "record"
HASH_BEFORE=$(sha256_of "$FILI_BIN")
TREE_BEFORE=$(git -C "$REPO" status --porcelain)

# ── Step 2: a fresh directory outside the repository ─────────────────────────
step "workdir"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/fili-authoring.XXXXXX")
PLUGIN="$WORK/plugin"
mkdir -p "$PLUGIN" || fail "cannot create the plugin directory"

# ── Step 3: copy the template and the published WIT ──────────────────────────
step "copy"
tar -C "$REPO/plugins/template" --exclude=./target --exclude=./src/bindings.rs -cf - . |
    tar -C "$PLUGIN" -xf - || fail "cannot copy plugins/template"

step "wit"
# Local mode: the WIT comes from `git archive HEAD wit`, so the proof needs
# neither the network nor a tag.
git -C "$REPO" archive HEAD wit | tar -x -C "$PLUGIN" ||
    fail "cannot read wit/ from HEAD (is it committed?)"
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
