#!/usr/bin/env bash
# The X11 suite's two-head tests, against Xorg's dummy driver.
#
#   crates/crcbl-shell/tests/run-x11-heads-e2e.sh [extra nextest args…]
#
# `run-x11-e2e.sh` runs the suite against `Xvfb`, which drives one CRTC, and
# leaves out every `two_heads_` test. This runs exactly those, on a server
# `tools/x11-display.sh` starts with `CRCBL_E2E_X11_HEADS=2`: the monitors a
# second head adds, the events an unplugged one sends, the ids a replugged one
# gets, and a borderless window sent to the second. `CRCBL_E2E_X11_WM` works as
# it does there.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
export CRCBL_E2E_X11_HEADS=2
# shellcheck source=tools/x11-display.sh
source "${REPO_ROOT}/tools/x11-display.sh"

cd "$REPO_ROOT"
OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-shell \
    --features x11-e2e \
    --test x11_e2e \
    --run-ignored all \
    --test-threads 1 \
    -E 'test(/^two_heads_/)' \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

if [ -n "${WM_PID:-}" ] && ! kill -0 "$WM_PID" 2>/dev/null; then
    echo "crcbl e2e: the window manager (${CRCBL_E2E_X11_WM}) exited during the suite" >&2
    log_tail
    exit 1
fi
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
crcbl_nextest_plain "$OUTPUT" "$PLAIN"
if [ "$STATUS" -ne 0 ] || ! crcbl_nextest_summary "$PLAIN" "crcbl e2e" \
    "No test matched two_heads_; the prefix or the ignore attribute moved."; then
    echo "crcbl e2e: the two-head suite failed" >&2
    log_tail
    exit 1
fi
echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN two-head tests ran against Xorg's dummy driver on ${DISPLAY}"
