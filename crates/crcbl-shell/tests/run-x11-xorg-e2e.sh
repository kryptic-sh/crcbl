#!/usr/bin/env bash
# The X11 suite's `xorg_` tests, against Xorg's dummy driver.
#
#   crates/crcbl-shell/tests/run-x11-xorg-e2e.sh [extra nextest args…]
#
# `run-x11-e2e.sh` runs the suite against `Xvfb` and leaves out every `xorg_`
# test, each of which needs something Xvfb lacks: a second CRTC for the
# two-head tests — the monitors a second head adds, the events an unplugged one
# sends, the ids a replugged one gets, a borderless window sent to the second —
# and a layout `setxkbmap` can change for the dead-key test, which Xvfb 21.1
# compiles and then ignores. This runs exactly those, on a server
# `tools/x11-display.sh` starts with `CRCBL_E2E_X11_SERVER=xorg`.
# `CRCBL_E2E_X11_WM` works as it does there.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
export CRCBL_E2E_X11_SERVER=xorg
# shellcheck source=tools/x11-display.sh
source "${REPO_ROOT}/tools/x11-display.sh"

cd "$REPO_ROOT"
# The compose table the dead-key test types through, as `run-wayland-e2e.sh`
# names it; the test sets the layout whose apostrophe is the dead key.
export XCOMPOSEFILE="${REPO_ROOT}/crates/crcbl-shell/tests/e2e.XCompose"
OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-shell \
    --features x11-e2e \
    --test x11_e2e \
    --run-ignored all \
    --test-threads 1 \
    -E 'test(/^xorg_/)' \
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
    "No test matched xorg_; the prefix or the ignore attribute moved."; then
    echo "crcbl e2e: the xorg_ tests failed" >&2
    log_tail
    exit 1
fi
echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN xorg_ tests ran against Xorg's dummy driver on ${DISPLAY}"
