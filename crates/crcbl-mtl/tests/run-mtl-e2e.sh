#!/usr/bin/env bash
# Run `crcbl-mtl`'s hardware suite — every test that opens a real Metal device,
# up to and including the ones that make the GPU execute a shader program.
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
# # Running one test at a time
#
# Extra arguments go straight to nextest, so a person on a real Mac runs any
# one test by name:
#
#   crates/crcbl-mtl/tests/run-mtl-e2e.sh -E 'test(a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear)'
#
# # Why `--run-ignored only` and not `all`
#
# This harness used to pass `--run-ignored all`, which ran the whole crate — the
# hardware tests *and* the pure ones (format tables, handle tagging, the extent
# arithmetic, the present ledger) that pass on a machine with no GPU at all. The
# count the guard below reads was then unit tests plus device tests, and a run in
# which **every device test had vanished** would still report a healthy total and
# clear the zero check. That is the same "check that cannot fail" shape the guard
# exists to prevent, one level up.
#
# `docs/plan/12-testing.md`'s placement rule is what makes the narrower selection
# possible: a test lives ungated in `src/` iff it can pass with no GPU, and a
# test that needs a live device is `#[ignore]`d. So `--run-ignored only` selects
# exactly the device tests, and the number this script prints is the number that
# matters. The pure ones are not lost — they are what
# `cargo nextest run --workspace --all-features` runs on every push.
#
# `--features mtl-e2e` stays, and is not redundant with it: the handful of tests
# that make the GPU execute a shader are feature-gated *as well as* `#[ignore]`d,
# so without the feature they are not compiled and `--run-ignored only` cannot
# select what does not exist.
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

# Reading nextest's summary is `tools/nextest-summary.sh`'s job, in one copy
# rather than eight. This harness rejected a cancelled run only by accident —
# its anchored pattern happened to break on the `/` in `2/15 tests run` — and so
# reported "zero tests run" about a run that had run two.
# shellcheck source=tools/nextest-summary.sh
source "${REPO}/tools/nextest-summary.sh"

LOG="$(mktemp -t crcbl-mtl-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

set +e
cargo nextest run --locked -p crcbl-mtl --features mtl-e2e \
    --run-ignored only --no-tests fail "$@" 2>&1 | tee "$LOG"
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
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl mtl e2e" \
    "The mtl-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi

echo "crcbl mtl e2e: the hardware suite ran $CRCBL_NEXTEST_TESTS_RUN tests against a Metal device"
