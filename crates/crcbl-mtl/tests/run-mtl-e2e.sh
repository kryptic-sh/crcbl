#!/usr/bin/env bash
# Run `crcbl-mtl`'s hardware suite — the tests that make the GPU execute a
# shader program.
#
#   crates/crcbl-mtl/tests/run-mtl-e2e.sh [extra nextest args…]
#
# # Why this is not a CI job
#
# It needs a real Metal GPU, and no CI runner this project has access to
# provides one. GitHub's `macos-latest` exposes an `Apple Paravirtual device`
# that hangs the command buffer on any draw — measured, with both encoders
# reporting `completed` rather than faulted, so the fault is the device and not
# the command stream. Vulkan has lavapipe to fall back on; Metal has no software
# rasteriser at all, so there is nothing to substitute.
#
# **So this script is run by a person on a Mac, and its results are not
# automated.** `docs/backlog.md` records that as a coverage gap rather than
# pretending the `build + test (macos-latest)` job covers it.
#
# # The zero-tests check is the point
#
# `docs/plan/12-testing.md` calls a silently-skipped e2e suite a known trap, and
# a suite that is both feature-gated and `#[ignore]`d has two ways to run
# nothing. Parsing the summary is what closes that.

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
    echo "crcbl mtl e2e: Metal only exists on macOS; this is $(uname -s)" >&2
    exit 1
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO"

LOG="$(mktemp -t crcbl-mtl-e2e.XXXXXX.log)"
trap 'rm -f "$LOG"' EXIT INT TERM

set +e
cargo nextest run --locked -p crcbl-mtl --features mtl-e2e \
    --run-ignored all --no-tests fail "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl mtl e2e: the suite failed" >&2
    exit "$STATUS"
fi

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! grep -qE "Summary \[[^]]*\] +[1-9][0-9]* tests? run" "$LOG"; then
    echo "crcbl mtl e2e: the suite reported zero tests run, which is the trap" >&2
    echo "  this script exists to catch — the feature gate or the ignore" >&2
    echo "  attribute stopped matching the tests." >&2
    exit 1
fi

echo "crcbl mtl e2e: the hardware suite ran against a real Metal GPU"
