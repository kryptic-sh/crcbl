#!/usr/bin/env bash
# Run the `crcbl` CLI's scaffold end-to-end suite.
#
#   crates/crcbl-cli/tests/run-cli-e2e.sh [extra nextest args…]
#
# The suite scaffolds a project with `crcbl new`, compiles it, lints it, and
# runs it headless. That means a full engine compile into a throwaway target
# directory, so — exactly like `crates/crcbl-shell/tests/run-wayland-e2e.sh` —
# the tests are feature-gated *and* `#[ignore]`d, this script is the only thing
# that turns them on, and CI runs this script.
#
# `docs/plan/12-testing.md` calls a silently-skipped e2e job a known trap, so
# the script fails when the suite reports zero tests run.
#
# It needs no display, no GPU and no compositor: the scaffolded game runs
# against `HeadlessShell` and, by default, the null GPU backend — which is the
# entire point of the CLI/headless pillar. Set `CRCBL_CLI_E2E_BACKEND=vk` (with
# a driver installed) to put the template's render graph in front of a real one
# instead; CI does exactly that against lavapipe.
#
# It does need `rustfmt` and `clippy`, which `rust-toolchain.toml` installs.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# The suite starts a nested `cargo` for the scaffolded project. Inheriting an
# outer `CARGO_TARGET_DIR` would point that build at the directory this very
# test run holds a lock on, which deadlocks rather than fails. The test sets the
# variable per child too; this is the belt to that pair of braces.
unset CARGO_TARGET_DIR

for tool in rustfmt cargo-clippy; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "crcbl e2e: $tool is not installed; the scaffold's lint checks need it" >&2
        exit 1
    fi
done

RUNTIME_DIR="$(mktemp -d -t crcbl-cli-e2e.XXXXXX)"
cleanup() {
    local status=$?
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

cd "$REPO_ROOT"
OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-cli \
    --features cli-e2e \
    --test cli_e2e \
    --run-ignored all \
    --test-threads 1 \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl e2e: the CLI suite failed" >&2
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job.
#
# Parsed from a colour-stripped copy: CI sets `CARGO_TERM_COLOR: always`, so
# nextest emits the count as `\e[1m1\e[0m tests run` and a plain-text match sees
# no digits next to "tests run".
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"$PLAIN"
RAN="$(grep -Eo '[0-9]+ tests? run' "$PLAIN" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl e2e: the suite reported no tests run — the gate is not gating" >&2
    exit 1
fi
echo "crcbl e2e: $RAN CLI scaffold tests ran on the ${CRCBL_CLI_E2E_BACKEND:-null} GPU backend"
