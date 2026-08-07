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
# display comes up, which flips the first two bullets — and flips the sandbox's
# `--fullscreen` pass from "the request is refused" to "the request is granted",
# which is the only branch on this platform that resembles what a player gets.
#
# It is off by default because no window manager is on a stock machine, and a
# harness that failed on a developer's laptop for want of an apt package would
# stop being run. **CI sets it**, to `openbox`, and runs this script twice — see
# `.github/workflows/ci.yml`. Both answers a window system can give are covered
# because both are exercised, not because one was assumed from the other.

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
    # Named up front, because the alternative failure is quiet: an unstartable
    # command leaves a display with no window manager on it, and every
    # assertion below would then be testing the branch this variable exists to
    # get away from. `CRCBL_E2E_EXPECT_WM` catches it a minute later; this
    # catches it now, and says which word was wrong.
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

# **A window manager that died mid-run is not a test failure, it is a lost
# gate.** Every assertion after it would have been measuring the branch this
# variable exists to get away from, and the only symptom is a handful of tests
# timing out for reasons that read like backend bugs. Checked before the suite's
# own status, because "openbox is gone" is the more useful sentence.
if [ -n "${WM_PID:-}" ] && ! kill -0 "$WM_PID" 2>/dev/null; then
    echo "crcbl e2e: the window manager (${CRCBL_E2E_X11_WM}) exited during the suite" >&2
    log_tail
    exit 1
fi

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
# It also runs **with `--fullscreen`**, and X11 is where both answers live.
#
# With **no window manager** — this script's default — `_NET_WM_STATE_FULLSCREEN`
# is a client message to a root window nobody is listening at:
# `requested_mode` becomes borderless and the effective mode never does. The
# summary line reports the *effective* one, so a refused fullscreen has to read
# `windowed` — and a game that echoed its own request would say `borderless`
# here and be wrong on every WM-less X session, every kiosk and every tiling
# setup that ignores the hint.
#
# With **`CRCBL_E2E_X11_WM` set** the same request is granted, and the summary
# has to read `borderless` at the screen size instead. That branch is not a
# variant of the first one: it goes through a different mechanism end to end —
# a window manager takes ownership of `_NET_WM_STATE`, resizes the window, and
# the resize comes back as a `ConfigureNotify` the swapchain has to be rebuilt
# for. CI runs this script both ways.
#
# Together with `run-wayland-e2e.sh` that is every combination of {honoured,
# refused} × {Wayland, X11} actually executed.
SANDBOX_LOG="${RUNTIME_DIR}/sandbox.log"
SANDBOX_FRAMES=120

# The size the sandbox asks for. A window manager honours it — it decorates
# *around* the client area rather than shrinking it — and without one nothing
# resizes the window at all, so this is the windowed extent either way.
SANDBOX_WINDOWED="1280x720"
# What fullscreen means here: the Xvfb screen, minus the depth `$SCREEN` carries.
SANDBOX_BORDERLESS="${SCREEN%x*}"

# `run_sandbox <backend> [windowed|fullscreen]`
run_sandbox() {
    local backend="$1"
    local mode="${2:-windowed}"
    local flags=(--backend "$backend" --frames "$SANDBOX_FRAMES" --title "crcbl e2e sandbox")
    [ "$mode" = "fullscreen" ] && flags+=(--fullscreen)

    echo "crcbl e2e: running the sandbox $mode against Xvfb on the $backend GPU backend"
    set +e
    CRCBL_SHELL=x11 \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SANDBOX_LOG:-info}" \
        cargo run --locked --quiet --package sandbox -- \
        "${flags[@]}" 2>&1 | tee "$SANDBOX_LOG"
    local status=${PIPESTATUS[0]}
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl e2e: the sandbox failed against Xvfb on $backend (exit $status)" >&2
        log_tail
        exit "$status"
    fi
    if ! grep -q "$SANDBOX_FRAMES frames" "$SANDBOX_LOG" \
        || ! grep -q "x11 shell" "$SANDBOX_LOG"; then
        echo "crcbl e2e: the sandbox did not report $SANDBOX_FRAMES frames on the x11 shell" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit 1
    fi

    # What the run must have reported, which is the *effective* mode and the
    # extent that goes with it — both off one line, so a run that said
    # borderless at the windowed size fails here.
    local want_mode="windowed"
    local want_extent="$SANDBOX_WINDOWED"
    local how="the sandbox presented $SANDBOX_FRAMES frames"
    if [ "$mode" = "fullscreen" ]; then
        if [ -n "${CRCBL_E2E_X11_WM:-}" ]; then
            # A window manager owns `_NET_WM_STATE`, so it can grant this.
            want_mode="borderless"
            want_extent="$SANDBOX_BORDERLESS"
            how="--fullscreen was granted and the swapchain followed"
        else
            # Nobody is listening at the root window, so it cannot be. The
            # summary reports the effective mode, and a run that echoed its own
            # request would say "borderless" and pass a check it should fail.
            how="--fullscreen was refused and reported as refused"
        fi
    fi

    if ! grep -q "at ${want_extent}, ${want_mode} " "$SANDBOX_LOG"; then
        echo "crcbl e2e: asked for $mode and did not get '${want_extent}, ${want_mode}'" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit 1
    fi
    echo "crcbl e2e: $how on x11/$backend"
}

# The X11 half of the `F11` pass — see `run_sandbox_toggle` in
# `run-wayland-e2e.sh` for the shape and for what the two ends prove. What is
# different here is *how a key reaches another program*: `XTEST` is a server
# extension, so the sender needs a connection and nothing else, where the
# Wayland one has to plug a virtual keyboard into a seat and be started first.
#
# The other difference is where the key goes. With no window manager the focus
# is left at `PointerRoot` and keys follow the *pointer*, so the sender parks it
# inside the sandbox's window; with one, the manager focuses the window it just
# mapped and the pointer is irrelevant. The pass therefore runs both ways, like
# everything else in this script.
KEY_F11_X11=95
BIN_DIR="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/debug"
TOGGLE_POLL_S=0.1

# Polls a file for a line, or fails naming what never appeared. A deadline and a
# poll, never a fixed sleep.
wait_for_line() {
    local what="$1" file="$2" pattern="$3"
    local deadline=$(( $(date +%s) + DISPLAY_TIMEOUT_S ))
    while ! grep -q "$pattern" "$file" 2>/dev/null; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: timed out after ${DISPLAY_TIMEOUT_S}s waiting for $what" >&2
            cat "$file" >&2 || true
            log_tail
            exit 1
        fi
        sleep "$TOGGLE_POLL_S"
    done
}

# `run_sandbox_toggle <backend>`
run_sandbox_toggle() {
    local backend="$1"
    local keys_in="${RUNTIME_DIR}/keys.fifo"
    local keys_log="${RUNTIME_DIR}/keys.log"

    echo "crcbl e2e: running the sandbox windowed on $backend and pressing F11 at it"
    for binary in sandbox crcbl-e2e-x11-key; do
        if [ ! -x "${BIN_DIR}/${binary}" ]; then
            echo "crcbl e2e: ${BIN_DIR}/${binary} was not built" >&2
            exit 1
        fi
    done

    # The binaries directly rather than `cargo run`: this one has to be killed
    # from the outside, and killing a `cargo run` leaves the child orphaned
    # holding the window. No frame budget either — it runs until the window
    # closes, which is what a player's session is.
    CRCBL_SHELL=x11 \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SANDBOX_LOG:-info}" \
        "${BIN_DIR}/sandbox" --backend "$backend" --title "crcbl e2e sandbox" \
        >"$SANDBOX_LOG" 2>&1 &
    local sandbox_pid=$!

    # It starts windowed, and reading that back is what stops the wait below
    # from being satisfied by a state that was already true before F11.
    wait_for_line "the sandbox to report itself windowed" \
        "$SANDBOX_LOG" "shell: the window is windowed"

    # A point inside the client area. With no window manager the window is at
    # the origin, and with one it is placed — but `openbox` centres a window
    # this much smaller than the screen, so a point a quarter of the way into
    # the *screen* is inside it either way. The sender only needs the pointer
    # there for the WM-less case; see its docs.
    local x=$(( ${SANDBOX_WINDOWED%x*} / 4 ))
    local y=$(( ${SANDBOX_WINDOWED#*x} / 4 ))

    rm -f "$keys_in"
    mkfifo "$keys_in"
    # The third argument is the sandbox's `app_id`, the instance half of the
    # `WM_CLASS` its window carries — what the `close` line below is matched
    # against.
    "${BIN_DIR}/crcbl-e2e-x11-key" "$x" "$y" "sh.kryptic.crcbl.sandbox" \
        <"$keys_in" >"$keys_log" 2>&1 &
    local keys_pid=$!
    # Holds the write end open, so the sender blocks on an empty stream instead
    # of seeing EOF from the first writer that finishes.
    exec 9>"$keys_in"
    wait_for_line "the key sender to reach the display" "$keys_log" "crcbl-e2e-x11-key: ready"

    echo "$KEY_F11_X11" >&9

    # The game's own account of the answer, and there are two of them because
    # this platform can give either. The engine logs the honoured case at info
    # and the refused case at warn, both naming the modes, so each branch is
    # asserted rather than one being assumed from the other.
    #
    # Then the pass closes the sandbox **cleanly**, which is what makes the
    # extent assertable at all: the key sender finds the sandbox's window by
    # the instance half of its `WM_CLASS` and sends it `WM_DELETE_WINDOW`,
    # exactly as a window manager's close button would. The sandbox answers
    # the question, tears down, and prints its end-of-run summary — and that
    # summary is the assertion, closing the gap this pass used to leave open:
    # it asserted only the engine's log line about the mode and then SIGTERMed
    # the sandbox, so the *extent* after F11 was never checked. A SIGTERM is
    # not a close request, and the sandbox would not have printed a summary to
    # assert against.
    if [ -n "${CRCBL_E2E_X11_WM:-}" ]; then
        wait_for_line "F11 to reach the sandbox and be honoured" \
            "$SANDBOX_LOG" "shell: the window is borderless"
        local how="F11 took the sandbox to borderless"
    else
        wait_for_line "F11 to reach the sandbox and be refused" \
            "$SANDBOX_LOG" "shell: asked for borderless and got windowed"
        local how="F11 was requested, refused, and reported as refused"
    fi

    # The summary line is the same one `run_sandbox` asserts — the *effective*
    # mode and the extent that goes with it, both off the one line the sandbox
    # prints as it exits.
    local want_extent="$SANDBOX_WINDOWED"
    local want_mode="windowed"
    if [ -n "${CRCBL_E2E_X11_WM:-}" ]; then
        want_extent="$SANDBOX_BORDERLESS"
        want_mode="borderless"
    fi

    echo close >&9
    wait_for_line "the sandbox's summary after F11" \
        "$SANDBOX_LOG" "at ${want_extent}, ${want_mode} "

    # The close was a *question*; the sandbox answered it and is exiting on its
    # own. If it refuses or hangs, the deadline poll fails the script rather
    # than hanging CI on a bare `wait`.
    local deadline=$(( $(date +%s) + DISPLAY_TIMEOUT_S ))
    while kill -0 "$sandbox_pid" 2>/dev/null; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: the sandbox did not exit after the close request" >&2
            cat "$SANDBOX_LOG" >&2 || true
            log_tail
            exit 1
        fi
        sleep "$TOGGLE_POLL_S"
    done
    wait "$sandbox_pid"
    exec 9>&-
    wait "$keys_pid" || true
    rm -f "$keys_in"
    echo "crcbl e2e: $how, and the summary read 'at ${want_extent}, ${want_mode}' on x11/$backend"
}

# See the equivalent block in `run-wayland-e2e.sh` for why the loader probe is a
# skip on a developer machine and a hard failure in CI.
if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    run_sandbox vk windowed
    run_sandbox vk fullscreen
    run_sandbox_toggle vk
else
    echo "crcbl e2e: no Vulkan loader; skipping the vk sandbox pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
fi
run_sandbox null windowed
