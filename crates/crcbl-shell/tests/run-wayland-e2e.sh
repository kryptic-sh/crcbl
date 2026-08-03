#!/usr/bin/env bash
# Run `crcbl-shell`'s Wayland end-to-end suite against a private headless sway.
#
#   crates/crcbl-shell/tests/run-wayland-e2e.sh [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` on a machine with no compositor
# stays green. This script is the only thing that turns them on, and CI runs
# this script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a
# known trap, so the script fails when the suite reports zero tests run.
#
# Exits non-zero if sway will not start, if the socket does not appear before
# the deadline, if no tests ran, or if any test fails. The compositor log tail is
# printed on every failure path, because "sway exited" with no reason attached
# costs a whole debugging session.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"
CONF="${CRATE_DIR}/tests/wayland-e2e-sway.conf"

# How long to wait for sway to publish its socket. Generous, because a cold CI
# runner starting a compositor for the first time is slow — and bounded, because
# `docs/plan/12-testing.md` requires a deadline rather than a sleep.
SOCKET_TIMEOUT_S="${CRCBL_E2E_SOCKET_TIMEOUT_S:-20}"
POLL_INTERVAL_S=0.1

if ! command -v sway >/dev/null 2>&1; then
    echo "crcbl e2e: sway is not installed; install it or run the harness elsewhere" >&2
    exit 1
fi

# A private XDG_RUNTIME_DIR so this never collides with a real session when a
# developer runs it on a live desktop, and so the socket poll below cannot
# accidentally find someone else's compositor.
RUNTIME_DIR="$(mktemp -d -t crcbl-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
SWAY_LOG="${RUNTIME_DIR}/sway.log"

# wlroots: no real outputs, no real input devices, no GPU required.
export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER_ALLOW_SOFTWARE=1
# Inherit nothing from an outer session.
unset WAYLAND_DISPLAY
unset DISPLAY

log_tail() {
    echo "--- sway log tail ---" >&2
    tail -n 40 "$SWAY_LOG" >&2 || true
    echo "--- end sway log ---" >&2
}

cleanup() {
    local status=$?
    if [ -n "${SWAY_PID:-}" ]; then
        kill "$SWAY_PID" 2>/dev/null || true
        wait "$SWAY_PID" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

echo "crcbl e2e: starting headless sway (XDG_RUNTIME_DIR=$RUNTIME_DIR)"
sway --config "$CONF" >"$SWAY_LOG" 2>&1 &
SWAY_PID=$!

# Poll for the socket with a deadline and a liveness check on the child. Never a
# fixed sleep: a sleep long enough for the slowest runner wastes time on every
# other one, and a sleep short enough to be cheap is a flake.
SOCKET_NAME=""
DEADLINE=$(( $(date +%s) + SOCKET_TIMEOUT_S ))
while [ -z "$SOCKET_NAME" ]; do
    SOCKET_NAME="$(find "$RUNTIME_DIR" -maxdepth 1 -name 'wayland-[0-9]*' ! -name '*.lock' \
        -printf '%f\n' 2>/dev/null | sort | head -1 || true)"
    [ -n "$SOCKET_NAME" ] && break
    if ! kill -0 "$SWAY_PID" 2>/dev/null; then
        echo "crcbl e2e: sway exited before opening a socket" >&2
        log_tail
        exit 1
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "crcbl e2e: no wayland socket in $RUNTIME_DIR after ${SOCKET_TIMEOUT_S}s" >&2
        log_tail
        exit 1
    fi
    sleep "$POLL_INTERVAL_S"
done

export WAYLAND_DISPLAY="$SOCKET_NAME"
# The IPC socket can appear a moment after the display socket, and the close
# and resize tests are driven through it, so poll for it under the same
# deadline rather than racing.
SWAYSOCK=""
while [ -z "$SWAYSOCK" ]; do
    SWAYSOCK="$(find "$RUNTIME_DIR" -maxdepth 1 -name 'sway-ipc.*' -print -quit 2>/dev/null || true)"
    [ -n "$SWAYSOCK" ] && break
    if ! kill -0 "$SWAY_PID" 2>/dev/null; then
        echo "crcbl e2e: sway exited before opening its IPC socket" >&2
        log_tail
        exit 1
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "crcbl e2e: no sway IPC socket after ${SOCKET_TIMEOUT_S}s" >&2
        log_tail
        exit 1
    fi
    sleep "$POLL_INTERVAL_S"
done
export SWAYSOCK
echo "crcbl e2e: socket up at \$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY (sway pid $SWAY_PID)"
echo "crcbl e2e: sway IPC at $SWAYSOCK"

cd "$REPO_ROOT"
OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-shell \
    --features wayland-e2e \
    --test wayland_e2e \
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
# digits next to "tests run". That is how this guard first fired — on a run
# where all ten tests had in fact passed.
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"$PLAIN"
RAN="$(grep -Eo '[0-9]+ tests? run' "$PLAIN" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl e2e: the suite reported no tests run — the gate is not gating" >&2
    log_tail
    exit 1
fi
echo "crcbl e2e: $RAN tests ran against headless sway"

# `docs/plan/01-foundations.md`'s sandbox exit criterion is about the *sample*,
# not about the shell crate's tests: "sandbox opens a window on Linux/Wayland
# and X11". So the sandbox runs here too, against this compositor, with a frame
# budget so it terminates. It is the only thing in CI that drives the whole
# join — window, first configure, `SurfaceTarget`, HAL surface, swapchain,
# acquire/present — against a real window system.
#
# Since P1.1 it runs **twice**: once on `crcbl-vk` and once on the null backend.
# The Vulkan pass is the one that matters — it is the only place a real
# `vkCreateWaylandSurfaceKHR` and a real `VkSwapchainKHR` are exercised, and it
# is where the seam's extent obligations meet Wayland's `0xFFFFFFFF`
# `currentExtent` for real. The null pass proves the runtime backend choice
# still works from the same binary.
#
# `CRCBL_SHELL=wayland` rather than letting the registry choose: this job exists
# to test *this* backend, and a silent fallback would report success for the
# other one.
# On Vulkan it also runs in **both display modes**, which is the only place in
# CI where a game-level fullscreen is asked for and actually granted. The shell
# suite above covers `DisplayMode` at the seam; this covers what a player gets —
# a window system that honours the request, a swapchain rebuilt at the size it
# hands back, and a summary line reporting the mode the compositor settled on
# rather than the one that was asked for.
#
# The sway config floats this `app_id` on purpose. Tiled, a lone window fills the
# output, and "windowed" and "borderless" would report the same extent — an
# assertion that cannot fail.
#
# **The null backend is excluded from the mode assertions, and not out of
# caution.** It presents by doing nothing, so no `wl_buffer` is ever attached to
# the surface, so the surface never maps: `swaymsg -t get_tree` lists no
# `app_id` at all for a null-backend run, where a Vulkan one lists
# `sh.kryptic.crcbl.sandbox`. An unmapped surface gets no fullscreen configure —
# the same fact `an_unmapped_surface_gets_no_fullscreen_configure` asserts in
# the suite above — so the window it reports is the size it asked for and the
# mode it asked for, whatever the compositor thinks. Asserting a mode there
# would be asserting against a window the compositor does not have.
SANDBOX_LOG="${RUNTIME_DIR}/sandbox.log"

# The size the sandbox asks for, which a floating window gets.
SANDBOX_WINDOWED="1280x720"
# The output declared in `wayland-e2e-sway.conf`, which is what fullscreen means.
SANDBOX_BORDERLESS="1920x1080"
# Long enough that the fullscreen configure has certainly arrived and been acted
# on: bring-up waits for the *first* configure, and the compositor's answer to
# the fullscreen request can be the second. The null backend presents a frame in
# well under a millisecond, so a small budget here would be a race rather than a
# test.
SANDBOX_FRAMES=120

# `run_sandbox <backend> [windowed|fullscreen]`
run_sandbox() {
    local backend="$1"
    local mode="${2:-windowed}"
    local flags=(--backend "$backend" --frames "$SANDBOX_FRAMES" --title "crcbl e2e sandbox")
    local want_mode="windowed"
    local want_extent="$SANDBOX_WINDOWED"
    if [ "$mode" = "fullscreen" ]; then
        flags+=(--fullscreen)
        want_mode="borderless"
        want_extent="$SANDBOX_BORDERLESS"
    fi

    echo "crcbl e2e: running the sandbox $mode against sway on the $backend GPU backend"
    set +e
    CRCBL_SHELL=wayland \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SANDBOX_LOG:-info}" \
        cargo run --locked --quiet --package sandbox -- \
        "${flags[@]}" 2>&1 | tee "$SANDBOX_LOG"
    local status=${PIPESTATUS[0]}
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl e2e: the sandbox failed against sway on $backend (exit $status)" >&2
        log_tail
        exit "$status"
    fi
    if ! grep -q "$SANDBOX_FRAMES frames" "$SANDBOX_LOG" \
        || ! grep -q "wayland shell" "$SANDBOX_LOG"; then
        echo "crcbl e2e: the sandbox did not report $SANDBOX_FRAMES frames on the wayland shell" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit 1
    fi
    # Only a backend that attaches buffers has a window the compositor can have
    # an opinion about — see the header above.
    if [ "$backend" = "null" ]; then
        echo "crcbl e2e: the sandbox presented $SANDBOX_FRAMES frames on wayland/null \
(no mode assertion: nothing is mapped)"
        return
    fi

    # The mode the compositor settled on, and the extent that goes with it. Both
    # come off one line, so a run that reported borderless at the windowed size
    # — a swapchain that never caught up with the configure — fails here.
    if ! grep -q "at ${want_extent}, ${want_mode} " "$SANDBOX_LOG"; then
        echo "crcbl e2e: asked for $mode and did not get '${want_extent}, ${want_mode}'" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit 1
    fi
    echo "crcbl e2e: the sandbox presented $SANDBOX_FRAMES frames \
$want_mode at $want_extent on wayland/$backend"
}

# Vulkan first, and only when there is a loader to run it on: this harness is
# also used on developer machines, and `docs/plan/12-testing.md`'s "no silently
# skipped gate" rule is served by the message rather than by failing a machine
# that never claimed to have Vulkan. CI installs the drivers, so CI runs it.
# `--fullscreen` covers the mode a window is *born* in. `F11` covers the mode it
# is switched to while running, which is a different path end to end: a key has
# to arrive from the compositor, survive the loop's repeat filter, become a
# `set_mode`, come back as a configure, and rebuild the swapchain — none of
# which a creation-time flag touches.
#
# Nothing in-process can drive it. The sample is a separate program, so the key
# has to be a real one: `tests/bin/send_key.rs` builds a
# `zwp_virtual_keyboard_v1` on this compositor's seat and taps it, and the
# keystroke then goes through sway's whole input path — focus, serials, XKB —
# before reaching the sandbox, which cannot tell it from a physical keyboard.
# The sender never presents to its own window, so sway never maps it and it
# cannot take the focus it is trying to type into.
#
# Both ends are checked, because either alone would be half a test: sway's tree
# says the *compositor* has the window, and the game's own log and summary line
# say it saw the answer and rebuilt its swapchain at the new size.
KEY_F11=87
SANDBOX_APP_ID="sh.kryptic.crcbl.sandbox"
BIN_DIR="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/debug"

# Polls a file for a line, or fails naming what never appeared. A deadline and
# a poll, never a fixed sleep — `docs/plan/12-testing.md` makes that the rule
# for anything asynchronous, and it is the same one the Rust suite's
# `pump_until` follows.
wait_for_line() {
    local what="$1" file="$2" pattern="$3"
    local deadline=$(( $(date +%s) + SOCKET_TIMEOUT_S ))
    while ! grep -q "$pattern" "$file" 2>/dev/null; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: timed out after ${SOCKET_TIMEOUT_S}s waiting for $what" >&2
            cat "$file" >&2 || true
            log_tail
            exit 1
        fi
        sleep "$POLL_INTERVAL_S"
    done
}

# `run_sandbox_toggle <backend>`
run_sandbox_toggle() {
    local backend="$1"
    local keys_in="${RUNTIME_DIR}/keys.fifo"
    local keys_log="${RUNTIME_DIR}/keys.log"
    echo "crcbl e2e: running the sandbox windowed on $backend and pressing F11 at it"

    # The binaries directly rather than `cargo run`: these have to be killed
    # from the outside, and killing a `cargo run` leaves the child orphaned
    # holding the window the next step is waiting on. The sandbox gets no frame
    # budget either — it runs until the window closes, which is what a player's
    # session is.
    for binary in sandbox crcbl-e2e-key; do
        if [ ! -x "${BIN_DIR}/${binary}" ]; then
            echo "crcbl e2e: ${BIN_DIR}/${binary} was not built" >&2
            exit 1
        fi
    done

    # The keyboard is plugged in *before* the game starts — see the sender's own
    # docs for why typing straight after the hotplug loses the key.
    rm -f "$keys_in"
    mkfifo "$keys_in"
    "${BIN_DIR}/crcbl-e2e-key" <"$keys_in" >"$keys_log" 2>&1 &
    local keys_pid=$!
    # Holds the write end open, so the sender blocks on an empty stream instead
    # of seeing EOF from the first writer that finishes.
    exec 9>"$keys_in"
    wait_for_line "the key sender to plug in a keyboard" "$keys_log" "crcbl-e2e-key: ready"

    CRCBL_SHELL=wayland \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SANDBOX_LOG:-info}" \
        "${BIN_DIR}/sandbox" --backend "$backend" --title "crcbl e2e sandbox" \
        >"$SANDBOX_LOG" 2>&1 &
    local sandbox_pid=$!

    # Mapped, and therefore focused: sway focuses a window when it appears, and
    # a window appears when a buffer is attached. Nothing else in this session
    # is mapped to take it away — the sender never presents.
    local deadline=$(( $(date +%s) + SOCKET_TIMEOUT_S ))
    while ! swaymsg -t get_tree | grep -q "\"app_id\": \"${SANDBOX_APP_ID}\""; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: the sandbox never mapped a window sway could see" >&2
            cat "$SANDBOX_LOG" >&2
            log_tail
            exit 1
        fi
        sleep "$POLL_INTERVAL_S"
    done
    # It starts windowed, and reading that back is what stops the wait below
    # from being satisfied by a state that was already true before F11.
    wait_for_line "the sandbox to report itself windowed" \
        "$SANDBOX_LOG" "shell: the window is windowed"

    echo "$KEY_F11" >&9

    # The game's own account of the compositor's answer: `mode_request_honoured`
    # went true for a mode it did not start in, which is the whole F11 path —
    # key, repeat filter, `set_mode`, configure — in one line.
    wait_for_line "F11 to reach the sandbox and be honoured" \
        "$SANDBOX_LOG" "shell: the window is borderless"

    # Closing it is how the run ends, so the summary is written and the exit
    # reason says the close arrived rather than a budget running out.
    swaymsg "[app_id=\"${SANDBOX_APP_ID}\"]" kill >/dev/null
    local status=0
    wait "$sandbox_pid" || status=$?
    exec 9>&-
    wait "$keys_pid" || true
    rm -f "$keys_in"
    if [ "$status" -ne 0 ]; then
        echo "crcbl e2e: the sandbox failed after F11 on $backend (exit $status)" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit "$status"
    fi
    # And the swapchain followed: the summary's extent is the surface's, not the
    # window's, so a mode change that never reached the GPU fails here.
    if ! grep -q "at ${SANDBOX_BORDERLESS}, borderless (CloseRequested)" "$SANDBOX_LOG"; then
        echo "crcbl e2e: F11 did not leave the sandbox borderless at ${SANDBOX_BORDERLESS}" >&2
        cat "$SANDBOX_LOG" >&2
        log_tail
        exit 1
    fi
    echo "crcbl e2e: F11 took the sandbox to $SANDBOX_BORDERLESS borderless on wayland/$backend"
}

if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    run_sandbox vk windowed
    # Both modes on the backend that builds a real `VkSwapchainKHR`, because
    # a fullscreen configure is a swapchain recreation and that is the half a
    # recording backend cannot get wrong.
    run_sandbox vk fullscreen
    # And the switch between them, which neither of those two makes.
    cargo build --locked --quiet --package sandbox
    cargo build --locked --quiet --package crcbl-shell \
        --features wayland-e2e --bin crcbl-e2e-key
    run_sandbox_toggle vk
else
    echo "crcbl e2e: no Vulkan loader; skipping the vk sandbox pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
fi
run_sandbox null windowed
