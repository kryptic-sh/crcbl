#!/usr/bin/env bash
# A private headless Xvfb, for any harness that needs a real X server.
#
# **Sourced, never run.** It starts a server, exports the display and exits the
# caller's shell on failure, which is the behaviour `run-x11-e2e.sh` had when it
# carried this inline — the same argument `tools/nextest-summary.sh` and
# `crates/crcbl-shell/tests/sway-session.sh` make: in one copy rather than one
# per harness, because the second copy is where the two drift apart.
#
#   REPO_ROOT="…"
#   # shellcheck source=tools/x11-display.sh
#   source "${REPO_ROOT}/tools/x11-display.sh"
#   # $DISPLAY, $SCREEN, $RUNTIME_DIR, $XVFB_PID, $WM_PID and $log_tail are now
#   # the caller's, and the display is up.
#
# The caller gets, on return:
#
#   DISPLAY             exported, pointing at the server this started
#   SCREEN              the geometry it was started with, depth included
#   RUNTIME_DIR         a private 0700 directory, removed by the trap below
#   XVFB_PID            the server, for a liveness check
#   WM_PID              the window manager, unset when there is none
#   DISPLAY_TIMEOUT_S   the deadline, so a caller's own polls share it
#   log_tail            prints the server's log tail, and the manager's
#
# It exits non-zero if Xvfb is missing, if no display number is free, or if the
# display does not answer before the deadline. The server log tail goes out on
# every one of those paths, because "Xvfb exited" with no reason attached costs
# a whole debugging session.
#
# # Decision: bare Xvfb, with no window manager
#
# An X11 window manager is a *separate program*, and its presence changes the
# backend's behaviour more than anything else on this platform — the focus
# policy, the decorations, the size hints, the reparenting, and whether a
# fullscreen request can be granted at all. So nothing is started by default and
# each caller asserts what that implies; `crates/crcbl-shell/tests/run-x11-e2e.sh`
# lists the four consequences its own suite is written against.
#
# Setting `CRCBL_E2E_X11_WM` to a window manager command starts it after the
# display comes up. It is off by default because no window manager is on a stock
# machine, and a harness that failed on a developer's laptop for want of an apt
# package would stop being run. **CI sets it**, to `openbox` — see
# `.github/workflows/ci.yml`. Both answers a window system can give are covered
# because both are exercised, not because one was assumed from the other.
#
# # ENVIRONMENT
#
#   CRCBL_E2E_DISPLAY_TIMEOUT_S  How long to wait for the display, in seconds.
#   CRCBL_E2E_X11_SCREEN         The `-screen 0` geometry, depth included.
#   CRCBL_E2E_X11_WM             A window manager command; none by default.

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
    # Named up front, because the alternative failure is quiet: an unstartable
    # command leaves a display with no window manager on it, and every
    # assertion a caller makes would then be testing the branch this variable
    # exists to get away from. `CRCBL_E2E_EXPECT_WM` catches it a minute later;
    # this catches it now, and says which word was wrong.
    if ! command -v "${CRCBL_E2E_X11_WM%% *}" >/dev/null 2>&1; then
        echo "crcbl e2e: CRCBL_E2E_X11_WM names ${CRCBL_E2E_X11_WM%% *}, which is not installed" >&2
        exit 1
    fi
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
