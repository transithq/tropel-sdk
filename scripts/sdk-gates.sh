#!/usr/bin/env bash
# P2 · SDK enforcement gates — the CI replacement for a repo boundary.
#
# Lives in the SDK repo (transithq/tropel-sdk) and is run by BOTH CIs:
#   - the SDK repo's own CI:        bash scripts/sdk-gates.sh
#   - the Tropel monorepo's CI:     bash crates/tropel-sdk/scripts/sdk-gates.sh
#     (crates/tropel-sdk is a git submodule of the SDK repo)
#
# Three gates prove tropel-sdk is a leaf, wasm-agnostic, and buildable by a
# consumer with no SDK checkout.
#
# Local usage:  bash scripts/sdk-gates.sh
# (needs the wasm32-unknown-unknown target: rustup target add wasm32-unknown-unknown)

set -euo pipefail

# Run from anywhere: resolve the SDK repo root so the monorepo can invoke this
# script through the submodule path (crates/tropel-sdk/scripts/sdk-gates.sh).
cd "$(dirname "$0")/.."

# The dual-context trap: when this script runs from the monorepo submodule
# path, cargo walks UP and resolves the MONOREPO workspace (the SDK manifest
# has no [workspace] table), so target/ lives at the monorepo root. When run
# from the SDK repo it resolves the SDK workspace. `cargo metadata` gives the
# real workspace root either way — never assume it equals $PWD.
#
# python is needed only to parse cargo's JSON. Probe candidates that actually
# RUN: on Windows, `command -v python3` can find the Microsoft Store stub
# alias, which exists but refuses to execute ("Python was not found").
PY=""
for cand in python3 python; do
  if command -v "$cand" >/dev/null 2>&1 && "$cand" --version >/dev/null 2>&1; then
    PY="$cand"
    break
  fi
done
if [ -z "$PY" ]; then
  echo "FAIL: a working python3/python is required (to parse cargo metadata)"
  exit 1
fi
# --locked like every other cargo call here: a drifted Cargo.lock must fail
# loudly, not silently re-resolve before the gates below run.
WS_ROOT=$(cargo metadata --locked --format-version 1 --no-deps \
  | "$PY" -c 'import json,sys; print(json.load(sys.stdin)["workspace_root"])')
# cargo metadata emits NATIVE paths (Windows: D:\tropel) — tar would misread
# the drive letter as a remote host. Convert to a shell-friendly form.
if command -v cygpath >/dev/null 2>&1; then
  WS_ROOT=$(cygpath -u "$WS_ROOT")
else
  WS_ROOT=${WS_ROOT//\\/\/}
fi

# --locked everywhere we resolve against the committed lockfile (matching the
# rest of ci.yml), so a drifted Cargo.lock fails loudly instead of silently
# resolving a different graph. The outside-workspace build in Gate 3 must NOT
# be --locked — it's a fresh temp dir with no lockfile yet.
echo "── Gate 1: tropel-sdk must be a leaf ──"
if cargo tree --locked -p tropel-sdk --edges normal --prefix none | tail -n +2 | grep -q '^tropel-'; then
  echo "FAIL: tropel-sdk has internal dependencies:"
  cargo tree --locked -p tropel-sdk --edges normal --prefix none | tail -n +2 | grep '^tropel-'
  exit 1
fi
echo "PASS"

echo "── Gate 2: tropel-sdk is target-agnostic ──"
# --no-default-features first (the registration/inventory feature is the only
# one that pulls a dependency with platform-sensitive code).
cargo check --locked -p tropel-sdk --target wasm32-unknown-unknown --no-default-features
cargo check --locked -p tropel-sdk --target wasm32-unknown-unknown          # incl. inventory
echo "PASS"

echo "── Gate 3: builds from outside the workspace ──"
cargo package --locked -p tropel-sdk --allow-dirty
VER=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
tar xzf "$WS_ROOT/target/package/tropel-sdk-${VER}.crate" -C "$WORK"

mkdir -p "$WORK/sample-ext/src"
cat > "$WORK/sample-ext/Cargo.toml" <<EOF
# An empty [workspace] table stops cargo's upward workspace search at this
# temp dir — the sample must build as an independent crate, never as part of
# the SDK workspace (which a `path` dep inside the repo tree would join).
[workspace]
[package]
name = "sample-ext"
version = "0.0.0"
edition = "2021"

[dependencies]
tropel-sdk = { path = "../tropel-sdk-${VER}" }
EOF

cat > "$WORK/sample-ext/src/lib.rs" <<'EOF'
use tropel_sdk::{
    InputAdapter, InputAdapterRegistration, Scenario, ScenarioInfo, TropelError, inventory,
};

pub struct SampleAdapter;

impl InputAdapter for SampleAdapter {
    fn id(&self) -> &str { "sample" }
    fn detect(&self, bytes: &[u8]) -> bool { bytes.starts_with(b"SAMPLE\n") }
    fn parse(&self, _bytes: &[u8]) -> Result<Scenario, TropelError> {
        Ok(Scenario {
            info: ScenarioInfo { name: "sample".into(), description: None, schema: None },
            items: vec![],
            variables: Default::default(),
            auth: None,
        })
    }
}

inventory::submit!(InputAdapterRegistration::new("sample", || Box::new(SampleAdapter)));
EOF

cargo build --manifest-path "$WORK/sample-ext/Cargo.toml"
echo "PASS — SDK is usable with no SDK checkout"
