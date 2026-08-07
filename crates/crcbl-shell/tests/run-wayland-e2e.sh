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

# Starting a compositor is `sway-session.sh`'s job, because the CLI's scaffold
# suite needs the same thing and two copies of a socket poll is where the two
# drift apart. It sets `SWAY_RUNTIME_DIR`, `WAYLAND_DISPLAY`, `SWAYSOCK`,
# `SWAY_LOG` and `SWAY_PID`, and defines `sway_log_tail`.
# shellcheck source=crates/crcbl-shell/tests/sway-session.sh
source "${CRATE_DIR}/tests/sway-session.sh"

cleanup() {
    local status=$?
    sway_session_stop
    exit "$status"
}
trap cleanup EXIT INT TERM

sway_session_start "$CONF"

cd "$REPO_ROOT"
OUTPUT="${SWAY_RUNTIME_DIR}/nextest.log"
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
    sway_log_tail
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job. `Summary [ 0.1s] 7 tests run: …`
#
# Parse a colour-stripped copy: CI sets `CARGO_TERM_COLOR: always`, so nextest
# emits the count as `\e[1m10\e[0m tests run` and a plain-text match sees no
# digits next to "tests run". That is how this guard first fired — on a run
# where all ten tests had in fact passed.
PLAIN="${SWAY_RUNTIME_DIR}/nextest.plain.log"
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"$PLAIN"
RAN="$(grep -Eo '[0-9]+ tests? run' "$PLAIN" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl e2e: the suite reported no tests run — the gate is not gating" >&2
    sway_log_tail
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
SANDBOX_LOG="${SWAY_RUNTIME_DIR}/sandbox.log"

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
        sway_log_tail
        exit "$status"
    fi
    if ! grep -q "$SANDBOX_FRAMES frames" "$SANDBOX_LOG" \
        || ! grep -q "wayland shell" "$SANDBOX_LOG"; then
        echo "crcbl e2e: the sandbox did not report $SANDBOX_FRAMES frames on the wayland shell" >&2
        cat "$SANDBOX_LOG" >&2
        sway_log_tail
        exit 1
    fi
    # Only a backend that attaches buffers has a window the compositor can have
    # an opinion about — see the header above.
    if [ "$backend" = "null" ]; then
        echo "crcbl e2e: the sandbox presented $SANDBOX_FRAMES frames on wayland/null \
(no mode assertion: nothing is mapped)"
        return
    fi

    # Closed-loop pacing, which is the only place in this repo where
    # `vkWaitForPresentKHR` runs against a real `VkSwapchainKHR`: the vk suite is
    # entirely offscreen and has no swapchain to wait on at all.
    #
    # The two halves have to agree rather than the fast path being demanded,
    # because `VK_KHR_present_id` + `VK_KHR_present_wait` are **driver
    # conditional**: radv has them, lavapipe does not, and CI runs lavapipe. So
    # a run that enabled the extensions and then did not pace on them is a
    # failure, and a driver that has neither says so out loud instead of passing
    # quietly — a green run here is not evidence about the wait unless it
    # printed that it exercised it.
    # Both lines, and the second is the one that cannot be faked. "Pacing on
    # presents" only says the engine took the branch; a `wait_until_presented`
    # that returned `Ok(())` without calling anything — which is what every
    # backend still does — prints it just the same, and **the difference does
    # not show up in a frame time**, because FIFO already paces the loop
    # through `vkQueuePresentKHR`. The backend's own line is emitted from
    # inside the wait, so it is there only if the driver was really asked.
    if grep -q "crcbl-vk: present feedback enabled" "$SANDBOX_LOG"; then
        if ! grep -q "hal: pacing on presents" "$SANDBOX_LOG"; then
            echo "crcbl e2e: the device enabled present feedback and the loop did not pace on it" >&2
            cat "$SANDBOX_LOG" >&2
            sway_log_tail
            exit 1
        fi
        if ! grep -q "crcbl-vk: vkWaitForPresentKHR on present" "$SANDBOX_LOG"; then
            echo "crcbl e2e: the loop reported pacing on presents and never waited on one" >&2
            cat "$SANDBOX_LOG" >&2
            sway_log_tail
            exit 1
        fi
        echo "crcbl e2e: the sandbox paced $SANDBOX_FRAMES frames on vkWaitForPresentKHR"
    else
        echo "crcbl e2e: no VK_KHR_present_wait on this driver — the present-wait \
path was NOT exercised, only the absent-capability path" >&2
    fi

    # What the display is doing with those frames, which is a different question
    # from whether the loop paced on them — and driver conditional for the same
    # reason: radv has `VK_EXT_present_timing`, lavapipe does not, and CI runs
    # lavapipe.
    #
    # **The assertion is that the engine asked, not what it heard.**
    # "present timing enabled" only says the extension chain was negotiated;
    # the engine's own line is printed from `settle_pacing`, after the first
    # present, so it is there only if the query really reached the device. It is
    # absent the moment the engine goes back to never calling `display_timing`,
    # which is the state this check exists to stop returning.
    #
    # It deliberately does **not** demand a cadence. On the machine this was
    # written on — radv on a nested headless sway — the answer is `Unknown`
    # every frame, so an assertion naming `Fixed`/`Variable`/`Stepped` would be
    # one nobody has ever seen pass. That case shouts instead, so a run on a
    # compositor that does report a refresh says so out loud rather than
    # quietly passing the same check.
    local timing
    if grep -q "crcbl-vk: present timing enabled" "$SANDBOX_LOG"; then
        # `|| true` because the empty case is the one being *tested for*: this
        # runs under `set -o pipefail`, so a `grep` that matches nothing would
        # otherwise abort the script here and the message below — the whole
        # point of the check — would never be printed. Found by breaking the
        # engine's log line and watching this exit non-zero silently.
        timing="$(grep -Eo 'hal: display timing [A-Za-z]+' "$SANDBOX_LOG" | tail -1 \
            | sed -E 's/.* //' || true)"
        if [ -z "$timing" ]; then
            echo "crcbl e2e: the device enabled present timing and the engine never read it" >&2
            cat "$SANDBOX_LOG" >&2
            sway_log_tail
            exit 1
        fi
        if [ "$timing" = "Unknown" ]; then
            echo "crcbl e2e: the engine read the display timing and got Unknown — the query \
path ran, but no real DisplayTiming arm was exercised" >&2
        else
            echo "crcbl e2e: the display reports $timing"
        fi
    else
        echo "crcbl e2e: no VK_EXT_present_timing on this driver — the display-timing \
path was NOT exercised, only the absent-capability path" >&2
    fi

    # The mode the compositor settled on, and the extent that goes with it. Both
    # come off one line, so a run that reported borderless at the windowed size
    # — a swapchain that never caught up with the configure — fails here.
    if ! grep -q "at ${want_extent}, ${want_mode} " "$SANDBOX_LOG"; then
        echo "crcbl e2e: asked for $mode and did not get '${want_extent}, ${want_mode}'" >&2
        cat "$SANDBOX_LOG" >&2
        sway_log_tail
        exit 1
    fi
    echo "crcbl e2e: the sandbox presented $SANDBOX_FRAMES frames \
$want_mode at $want_extent on wayland/$backend"
}

# How many frames the paced pass presents, and at what rate. Small, because
# `--fps 30` means it really does take a second: the limiter is the only thing
# pacing a run that asked for no display sync, which is the point.
PACED_FRAMES=30
PACED_FPS=30
# The floor the *measured* mean frame time has to clear, in whole milliseconds.
# `PACED_FPS` asks for a 33 ms period; this is a little under it, because the
# first frame of a run never waits — there is no previous frame to be early
# against — and it is one of the samples the mean is taken over. An ignored
# limiter measures around 4 ms here, so the gap this has to resolve is large.
PACED_MIN_FRAME_MS=30

# `run_sandbox_paced <backend> <pacing> <present-mode-pattern>`
#
# The pass that proves `--pacing` and `--fps` are wired to something. Every
# other run here takes the defaults, so a `Common` field parsed and then dropped
# on the floor — never reaching `GpuContextDesc` or the clock — would look
# exactly like a working engine from the outside.
#
# Three assertions on the engine's own lines, and one on what it measured:
#
#  * `engine: the frame limit is 30 fps` is printed from `Clock::set_limit`, so
#    it is there only if the value reached the clock the loop advances.
#  * ...and the run's own end-of-run measurement says the limiter then *waited*,
#    which a line about a setting cannot. That number is worth asserting here
#    rather than only in the unit test because this is a real display, a real
#    swapchain and a real scheduler: bring-up is a few tens of milliseconds and
#    the paced second is the whole run.
#  * `asked for <pacing>, pacing <pacing>` comes off the same `settle_pacing`
#    line the display-timing block above reads. It is the half that says the
#    request survived: `asked for Auto` — the default, and what every other pass
#    in this file produces — would still print, and would still name a pacing.
#
# And the mode the swapchain actually opened on, which is the difference a
# player would feel — and the reason `adaptive` earns a pass of its own: no run
# in this repository had ever opened a swapchain on `FifoRelaxed`, the mode a
# VRR panel actually wants, until this one. `Pacing::Off` prefers Mailbox and
# falls back to Immediate; `Pacing::Adaptive` prefers FifoRelaxed and falls back
# to Mailbox. Headless sway only offers Fifo and Mailbox, so the adaptive pass
# here matches both — what a Fifo alone would mean is that the request did not
# reach `choose_present_mode` at all.
run_sandbox_paced() {
    local backend="$1"
    local pacing="$2"
    local mode_pattern="$3"
    # The engine spells the pacing with a capital (its `Debug` form); the CLI
    # takes lowercase. One transform, used by every grep below.
    local log_pacing="${pacing^}"
    local log="${SWAY_RUNTIME_DIR}/sandbox-paced.log"

    echo "crcbl e2e: running the sandbox on $backend with --pacing $pacing --fps $PACED_FPS"
    set +e
    CRCBL_SHELL=wayland \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SANDBOX_LOG:-info}" \
        cargo run --locked --quiet --package sandbox -- \
        --backend "$backend" --frames "$PACED_FRAMES" --title "crcbl e2e sandbox" \
        --pacing "$pacing" --fps "$PACED_FPS" 2>&1 | tee "$log"
    local status=${PIPESTATUS[0]}
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl e2e: the paced sandbox failed against sway on $backend (exit $status)" >&2
        sway_log_tail
        exit "$status"
    fi

    if ! grep -q "engine: the frame limit is ${PACED_FPS} fps" "$log"; then
        echo "crcbl e2e: --fps $PACED_FPS never reached the loop's clock" >&2
        cat "$log" >&2
        sway_log_tail
        exit 1
    fi
    # The mean frame time the engine measured on the real clock, as whole
    # milliseconds — no float arithmetic in shell, and none needed to tell 33
    # from 4. `|| true` because an absent line is one of the cases being tested
    # for, and a `grep` that matches nothing under `set -o pipefail` would
    # otherwise abort the script before the message below could print.
    local mean_ms
    mean_ms="$(grep -Eo 'frame cpu \(real clock[^)]*\): mean [0-9]+' "$log" | tail -1 \
        | sed -E 's/.* //' || true)"
    if [ -z "$mean_ms" ] || [ "$mean_ms" -lt "$PACED_MIN_FRAME_MS" ]; then
        echo "crcbl e2e: asked for $PACED_FPS fps and the run measured \
'${mean_ms:-no}' ms a frame, under the ${PACED_MIN_FRAME_MS} ms floor — the limit was \
logged and not obeyed" >&2
        cat "$log" >&2
        sway_log_tail
        exit 1
    fi
    if ! grep -qE "hal: display timing [A-Za-z]+; asked for ${log_pacing}, pacing ${log_pacing}" "$log"; then
        echo "crcbl e2e: --pacing $pacing never reached the swapchain; the engine reported \
something other than 'asked for $log_pacing, pacing $log_pacing'" >&2
        cat "$log" >&2
        sway_log_tail
        exit 1
    fi
    if ! grep -qE "hal: swapchain [0-9]+x[0-9]+ [A-Za-z0-9]+ ${mode_pattern} " "$log"; then
        echo "crcbl e2e: the run asked for ${pacing} pacing and still opened its \
swapchain on a different present mode" >&2
        cat "$log" >&2
        sway_log_tail
        exit 1
    fi
    echo "crcbl e2e: the sandbox ran on ${log_pacing} pacing at $PACED_FPS fps on wayland/$backend, \
measuring ${mean_ms} ms a frame"
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
    local deadline=$(( $(date +%s) + SWAY_SESSION_TIMEOUT_S ))
    while ! grep -q "$pattern" "$file" 2>/dev/null; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: timed out after ${SWAY_SESSION_TIMEOUT_S}s waiting for $what" >&2
            cat "$file" >&2 || true
            sway_log_tail
            exit 1
        fi
        sleep "$SWAY_SESSION_POLL_S"
    done
}

# `run_sandbox_toggle <backend>`
run_sandbox_toggle() {
    local backend="$1"
    local keys_in="${SWAY_RUNTIME_DIR}/keys.fifo"
    local keys_log="${SWAY_RUNTIME_DIR}/keys.log"
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
    local deadline=$(( $(date +%s) + SWAY_SESSION_TIMEOUT_S ))
    while ! swaymsg -t get_tree | grep -q "\"app_id\": \"${SANDBOX_APP_ID}\""; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: the sandbox never mapped a window sway could see" >&2
            cat "$SANDBOX_LOG" >&2
            sway_log_tail
            exit 1
        fi
        sleep "$SWAY_SESSION_POLL_S"
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
        sway_log_tail
        exit "$status"
    fi
    # And the swapchain followed: the summary's extent is the surface's, not the
    # window's, so a mode change that never reached the GPU fails here.
    # `borderless` may carry the monitor it landed on — `DisplayMode`'s
    # `Display` prints "borderless on monitor 2" once the backend can say which
    # output the surface is on — so the suffix is optional and the exit reason
    # still has to be there. Anchoring on `(CloseRequested)` is what makes this
    # a check that the run *ended* borderless rather than passed through it.
    if ! grep -qE "at ${SANDBOX_BORDERLESS}, borderless( on monitor [0-9]+)? \(CloseRequested\)" \
        "$SANDBOX_LOG"; then
        echo "crcbl e2e: F11 did not leave the sandbox borderless at ${SANDBOX_BORDERLESS}" >&2
        cat "$SANDBOX_LOG" >&2
        sway_log_tail
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
    # And with the display sync turned off and a frame cap in its place, which
    # is the one pass where either flag is anything but its default.
    run_sandbox_paced vk off "(Mailbox|Immediate)"
    # And the same with adaptive asked for by name — the pacing a VRR panel
    # actually wants, and the one mode no pass in this file had ever opened a
    # swapchain on (its unit coverage is the whole of its coverage).
    run_sandbox_paced vk adaptive "(FifoRelaxed|Mailbox)"
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
