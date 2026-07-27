#!/usr/bin/env bash
# Run `crcbl-shell`'s X11 end-to-end suite against a private headless Xvfb.
#
#   crates/crcbl-shell/tests/run-x11-e2e.sh [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` on a machine with no display
# stays green. This script is the only thing that turns them on, and CI runs
# this script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a
# known trap, so the script fails when the suite reports zero tests run.
#
# Exits non-zero if Xvfb will not start, if the display does not answer before
# the deadline, if no tests ran, or if any test fails. The server log tail is
# printed on every failure path, because "Xvfb exited" with no reason attached
# costs a whole debugging session.
#
# # Decision: bare Xvfb, with no window manager
#
# An X11 window manager is a *separate program*, and its presence changes the
# backend's behaviour more than anything else on this platform — which is
# exactly why the suite runs without one by default, and asserts what that
# implies:
#
#   * `ShellCaps::ASPECT_HINT_HONORED` and `ShellCaps::SERVER_DECORATIONS` are
#     **clear**, because nothing is running that would honour a size hint or
#     draw a title bar. The backend detects this through
#     `_NET_SUPPORTING_WM_CHECK` and the suite asserts the honest answer rather
#     than the convenient one.
#   * `_NET_WM_STATE_FULLSCREEN` is a client message sent into the void, so
#     `set_mode(Borderless)` updates `requested_mode` and never becomes the
#     effective mode. `WindowState::mode_request_honoured()` stays false — the
#     exact case the seam separates "requested" from "effective" for.
#   * There is no reparenting, so a window's `ConfigureNotify` position is the
#     desktop position.
#   * Nothing sets the input focus, so the suite's peer client does it
#     (`Peer::focus`) — otherwise key events would follow the pointer and the
#     keyboard tests would assert against nothing.
#
# Setting `CRCBL_E2E_X11_WM` to a window manager command starts it after the
# display comes up, which flips the first two bullets. It is off by default
# because no window manager is guaranteed on a CI runner and a job that quietly
# skipped the harder assertions would be worse than one that never made them.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# How long to wait for the display to answer. Generous, because a cold CI
# runner starting an X server for the first time is slow — and bounded, because
# `docs/plan/12-testing.md` requires a deadline rather than a sleep.
DISPLAY_TIMEOUT_S="${CRCBL_E2E_DISPLAY_TIMEOUT_S:-20}"
POLL_INTERVAL_S=0.1
SCREEN="${CRCBL_E2E_X11_SCREEN:-1920x1080x24}"

if ! command -v Xvfb >/dev/null 2>&1; then
    echo "crcbl e2e: Xvfb is not installed; install it or run the harness elsewhere" >&2
    exit 1
fi

RUNTIME_DIR="$(mktemp -d -t crcbl-x11-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
XVFB_LOG="${RUNTIME_DIR}/xvfb.log"
WM_LOG="${RUNTIME_DIR}/wm.log"

# Inherit nothing from an outer session: a developer running this on a live
# desktop must not have the suite connect to their real display, and the
# backend would otherwise prefer Wayland and never open X11 at all.
unset WAYLAND_DISPLAY
unset DISPLAY
unset XAUTHORITY

log_tail() {
    echo "--- Xvfb log tail ---" >&2
    tail -n 40 "$XVFB_LOG" >&2 || true
    echo "--- end Xvfb log ---" >&2
    if [ -s "$WM_LOG" ]; then
        echo "--- window manager log tail ---" >&2
        tail -n 20 "$WM_LOG" >&2 || true
        echo "--- end window manager log ---" >&2
    fi
}

cleanup() {
    local status=$?
    if [ -n "${WM_PID:-}" ]; then
        kill "$WM_PID" 2>/dev/null || true
        wait "$WM_PID" 2>/dev/null || true
    fi
    if [ -n "${XVFB_PID:-}" ]; then
        kill "$XVFB_PID" 2>/dev/null || true
        wait "$XVFB_PID" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

# Find a display number nobody is using. Xvfb's own `-displayfd` would be
# tidier and is not portable to every packaged build, so this claims a number
# by checking for the lock *and* the socket, then lets the readiness poll below
# catch a race with another process that claimed it first.
DISPLAY_NUM=""
for candidate in $(seq 90 120); do
    if [ ! -e "/tmp/.X${candidate}-lock" ] && [ ! -e "/tmp/.X11-unix/X${candidate}" ]; then
        DISPLAY_NUM="$candidate"
        break
    fi
done
if [ -z "$DISPLAY_NUM" ]; then
    echo "crcbl e2e: no free X display number in :90-:120" >&2
    exit 1
fi

echo "crcbl e2e: starting Xvfb on :${DISPLAY_NUM} (${SCREEN})"
# `-nolisten tcp` keeps the server on its Unix socket, so nothing outside this
# machine can reach it and the suite cannot accidentally talk to a remote
# display. RANDR and XTEST are what the backend and the harness's peer client
# need; both are built into Xvfb and are enabled explicitly so a build that
# defaults them off fails here rather than three tests in.
Xvfb ":${DISPLAY_NUM}" \
    -screen 0 "$SCREEN" \
    -nolisten tcp \
    +extension RANDR \
    +extension XTEST \
    >"$XVFB_LOG" 2>&1 &
XVFB_PID=$!

# Poll for readiness with a deadline and a liveness check on the child. Never a
# fixed sleep: a sleep long enough for the slowest runner wastes time on every
# other one, and a sleep short enough to be cheap is a flake.
#
# The socket appearing is necessary and not sufficient — the server creates it
# before it is accepting connections — so the poll finishes with a real
# connection attempt through the backend itself.
DEADLINE=$(( $(date +%s) + DISPLAY_TIMEOUT_S ))
while [ ! -S "/tmp/.X11-unix/X${DISPLAY_NUM}" ]; do
    if ! kill -0 "$XVFB_PID" 2>/dev/null; then
        echo "crcbl e2e: Xvfb exited before creating its socket" >&2
        log_tail
        exit 1
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "crcbl e2e: no X socket for :${DISPLAY_NUM} after ${DISPLAY_TIMEOUT_S}s" >&2
        log_tail
        exit 1
    fi
    sleep "$POLL_INTERVAL_S"
done

export DISPLAY=":${DISPLAY_NUM}"

if [ -n "${CRCBL_E2E_X11_WM:-}" ]; then
    echo "crcbl e2e: starting window manager: ${CRCBL_E2E_X11_WM}"
    # shellcheck disable=SC2086
    ${CRCBL_E2E_X11_WM} >"$WM_LOG" 2>&1 &
    WM_PID=$!
    # A window manager takes `_NET_SUPPORTING_WM_CHECK` some time *after* it
    # starts, and the backend latches its capabilities from that property at
    # connect time — so a shell opened too early would report no window manager
    # and every window-manager assertion would quietly invert.
    #
    # Waiting for it is the suite's job rather than this script's, because
    # checking the property means being an X client and the suite already is
    # one: `CRCBL_E2E_EXPECT_WM` makes `Session::open` poll, with its own
    # deadline, until the connection it opens sees a window manager. Doing it
    # here would need `xprop`, which is a package this harness otherwise does
    # not require.
    export CRCBL_E2E_EXPECT_WM=1
else
    echo "crcbl e2e: no window manager (set CRCBL_E2E_X11_WM to add one)"
fi

echo "crcbl e2e: display up at ${DISPLAY} (Xvfb pid ${XVFB_PID})"

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
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl e2e: the suite failed" >&2
    log_tail
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job. `Summary [ 0.1s] 7 tests run: …`
#
# Parse a colour-stripped copy: CI sets `CARGO_TERM_COLOR: always`, so nextest
# emits the count as `\e[1m10\e[0m tests run` and a plain-text match sees no
# digits next to "tests run". That is how the Wayland harness's guard first
# fired — on a run where all ten tests had in fact passed.
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"$PLAIN"
RAN="$(grep -Eo '[0-9]+ tests? run' "$PLAIN" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl e2e: the suite reported no tests run — the gate is not gating" >&2
    log_tail
    exit 1
fi
echo "crcbl e2e: $RAN tests ran against Xvfb on ${DISPLAY}"

# The X11 half of `docs/plan/01-foundations.md`'s sandbox exit criterion — see
# the same block in `run-wayland-e2e.sh`. `CRCBL_SHELL=x11` because Wayland is
# tried first by the registry and a silent fallback would report success for
# the wrong backend; here it would in fact fail (nothing is listening), but
# stating the intent is what keeps this job honest if that ever changes.
#
# Since P1.1 this runs on `crcbl-vk` as well as on the null backend, and the
# Vulkan pass is the interesting one: X11 is where
# `VkSurfaceCapabilitiesKHR::currentExtent` is a *real* size and
# `minImageExtent == maxImageExtent == currentExtent`, which is the case that
# tests the seam's extent obligations hardest — the shell's size is
# authoritative, and Vulkan may still refuse to configure at it.
SANDBOX_LOG="${RUNTIME_DIR}/sandbox.log"
run_sandbox() {
    local backend="$1"
    echo "crcbl e2e: running the sandbox against Xvfb on the $backend GPU backend"
    set +e
    CRCBL_SHELL=x11 \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SANDBOX_LOG:-info}" \
        cargo run --locked --quiet --package sandbox -- \
        --backend "$backend" --frames 30 --title "crcbl e2e sandbox" 2>&1 | tee "$SANDBOX_LOG"
    local status=${PIPESTATUS[0]}
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl e2e: the sandbox failed against Xvfb on $backend (exit $status)" >&2
        log_tail
        exit "$status"
    fi
    if ! grep -q "30 frames" "$SANDBOX_LOG" || ! grep -q "x11 shell" "$SANDBOX_LOG"; then
        echo "crcbl e2e: the sandbox did not report 30 frames on the x11 shell" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit 1
    fi
    echo "crcbl e2e: the sandbox presented 30 frames on x11/$backend"
}

# See the equivalent block in `run-wayland-e2e.sh` for why the loader probe is a
# skip on a developer machine and a hard failure in CI.
if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    run_sandbox vk
else
    echo "crcbl e2e: no Vulkan loader; skipping the vk sandbox pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
fi
run_sandbox null
