#!/usr/bin/env bash
# Run `crcbl-mtl`'s hardware suite — the tests that make the GPU execute a
# shader program.
#
#   crates/crcbl-mtl/tests/run-mtl-e2e.sh [extra nextest args…]
#
# # Who runs this
#
# **CI runs it, on `macos-latest`.** The device that image exposes is an
# `Apple Paravirtual device`, and a paravirtual device was long assumed unable
# to execute a shader — but that was generalised from macos-14, the one hosted
# image whose `MTLCreateSystemDefaultDevice()` returns nil. macos-15 and
# macos-26 both run a compute dispatch and a triangle draw correctly, and
# `macos-latest` resolves to macos-26. `docs/backlog.md` has the measurements.
#
# **A person on a real Mac runs it too, and that is still the only thing that
# covers a non-virtual GPU.** The CI job says the suite passes on Apple's
# paravirtual device; it says nothing about a discrete or an unvirtualised
# Apple GPU, and Metal has no software rasteriser to cross-check against the
# way Vulkan has lavapipe.
#
# The CI job holds one test out — the layer swapchain's drawable acquisition,
# which is gated on a headless container vending a `CAMetalLayer` drawable and
# not on shader execution. `.github/workflows/ci.yml` explains why there and
# passes the filter; this script excludes nothing on its own.
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
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

set +e
cargo nextest run --locked -p crcbl-mtl --features mtl-e2e \
    --run-ignored all --no-tests fail "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl mtl e2e: the suite failed" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in
# `crates/crcbl-vk/tests/run-vk-e2e.sh`: CI sets `CARGO_TERM_COLOR: always`, so
# nextest wraps its counts in escapes and the match below sees no digits next to
# "tests run". Without this the check fires on a suite that ran everything and
# passed — which is what run 31045734181 did, reporting the zero-tests trap at
# `102 tests run: 102 passed`.
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$LOG" >"${LOG}.plain"

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! grep -qE "Summary \[[^]]*\] +[1-9][0-9]* tests? run" "${LOG}.plain"; then
    echo "crcbl mtl e2e: the suite reported zero tests run, which is the trap" >&2
    echo "  this script exists to catch — the feature gate or the ignore" >&2
    echo "  attribute stopped matching the tests." >&2
    exit 1
fi

echo "crcbl mtl e2e: the hardware suite ran against a Metal device"
