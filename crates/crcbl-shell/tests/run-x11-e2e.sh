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
# Bringing the display up is `tools/x11-display.sh`'s job, and so is the
# decision to run no window manager on it by default; that file documents both,
# along with `CRCBL_E2E_X11_WM`. What belongs here is what the default implies
# for *this* suite, which asserts it rather than working around it:
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
# Setting `CRCBL_E2E_X11_WM` flips the first two bullets — and flips the
# sandbox's `--fullscreen` pass from "the request is refused" to "the request is
# granted", which is the only branch on this platform that resembles what a
# player gets. **CI sets it**, to `openbox`, and runs this script twice — see
# `.github/workflows/ci.yml`. Both answers a window system can give are covered
# because both are exercised, not because one was assumed from the other.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# Reading nextest's summary is `tools/nextest-summary.sh`'s job, in one copy
# rather than eight — five of those copies read a cancelled run's `2/15 tests
# run` as a healthy fifteen, this one among them.
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

# And starting the display is `tools/x11-display.sh`'s, for the same reason: it
# exports `DISPLAY`, `SCREEN`, `RUNTIME_DIR`, `XVFB_PID` and `WM_PID`, defines
# `log_tail`, and owns the cleanup trap. It exits this shell if the server never
# comes up.
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
# and reports success is worse than no job — and so is one nextest cancelled
# after two tests, whose `Summary [ 0.1s] 2/15 tests run` still ends in the
# total it never reached. The server's log tail is printed on the way out here,
# which is why the helper returns rather than exiting.
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
crcbl_nextest_plain "$OUTPUT" "$PLAIN"
if ! crcbl_nextest_summary "$PLAIN" "crcbl e2e" \
    "The x11-e2e feature or the ignore attribute stopped matching the tests."; then
    log_tail
    exit 1
fi
echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN tests ran against Xvfb on ${DISPLAY}"

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
# `assert_nothing_left_alive <log> <what>`
#
# **What the sandbox was still holding when its device went away.** `crcbl-vk`
# warns at teardown for every object nobody destroyed, naming the kinds, and a
# warning fails nothing. `run-vk-e2e.sh` reads that line for its own suite and
# `tools/run-samples-windowed.sh` reads it for every sample — and that runner
# deliberately leaves the sandbox to this one, so without this the sandbox is
# the single windowed binary nobody asks. It matters here in particular: this is
# the run that resizes and changes display mode, which is where a swapchain
# rebuild leaks if it leaks at all.
#
# The null backend has no such reporter, so this is a `vk` question and the grep
# simply finds nothing on the other one.
assert_nothing_left_alive() {
    local log="$1" what="$2" leaks
    leaks="$(grep -F 'object(s) still alive at device teardown' "$log" || true)"
    [ -z "$leaks" ] && return 0
    echo "crcbl e2e: ${what} destroyed its device with objects still alive:" >&2
    while IFS= read -r line; do
        echo "               $line" >&2
    done <<<"$leaks"
    echo "           The sandbox's own teardown reporter wrote that. Destroy them" >&2
    echo "           where they were made rather than leaving the line in a log." >&2
    log_tail
    exit 1
}

# `assert_validation_saw_nothing <log> <what>` — vk runs only.
#
# **`CRCBL_VK_VALIDATION=1` is set on every run below and used to prove
# nothing.** A validation error reaches `crcbl_core::log::error!` in
# `crcbl-vk`'s `debug` module and the process still exits 0, so each run
# advertised that it was validating and no run could fail because of it. This is
# the shell's copy of `ValidationReport::assert_clean`, which the vk and
# windowed e2e suites reach from Rust and nothing above the seam could.
#
# Both of `assert_clean`'s halves, because the second is worthless without the
# first: a log with no validation errors in it is exactly what a run with no
# messenger produces. `crcbl-vk` prints the "validation enabled" line only once
# the debug messenger really exists, so its absence means the layer was missing,
# `VK_EXT_debug_utils` was, or the messenger failed to be created — every one of
# which turns the grep below into a green light wired to nothing.
#
# Errors **and** warnings, which is where `assert_clean` draws the line and what
# `docs/plan/02-vulkan-backend.md`'s P1 exit criterion says. The messenger only
# ever subscribes to those two severities, so there is no informational chatter
# to filter out here. The pattern names the level, the module and the callback's
# own `vk <kind>:` prefix — the teardown leak warning above comes from
# `crcbl_vk::device` and is a different question, asked separately.
assert_validation_saw_nothing() {
    local log="$1" what="$2" complaints
    if ! grep -qF 'crcbl-vk: validation enabled (' "$log"; then
        echo "crcbl e2e: ${what} ran with CRCBL_VK_VALIDATION=1 and never loaded the layer," >&2
        echo "           so a clean log here proves nothing. Install" >&2
        echo "           VK_LAYER_KHRONOS_validation (Arch: vulkan-validation-layers," >&2
        echo "           Debian/Ubuntu: vulkan-validationlayers) — crcbl-vk warns by name" >&2
        echo "           when it is missing, and the warning is in the log above." >&2
        cat "$log" >&2
        log_tail
        exit 1
    fi
    if grep -qF 'a panic escaped the Vulkan debug messenger callback' "$log"; then
        echo "crcbl e2e: ${what} lost validation messages — a panic escaped the messenger" >&2
        echo "           callback, so the check below cannot see what the layer said." >&2
        cat "$log" >&2
        log_tail
        exit 1
    fi
    complaints="$(grep -E '(ERROR|WARN) +crcbl_vk::debug] vk ' "$log" || true)"
    [ -z "$complaints" ] && return 0
    echo "crcbl e2e: the validation layer complained about ${what}:" >&2
    while IFS= read -r line; do
        echo "               $line" >&2
    done <<<"$complaints"
    echo "           Those are specification violations this run committed. Fix them" >&2
    echo "           where they were recorded rather than leaving the line in a log." >&2
    log_tail
    exit 1
}

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
    assert_nothing_left_alive "$SANDBOX_LOG" "the sandbox"
    # The null backend opens no Vulkan instance, so it has no layer to have been
    # watching and nothing to assert about one — and the summary line says which
    # of the two runs this was rather than claiming the check on both.
    local validated=""
    if [ "$backend" = "vk" ]; then
        assert_validation_saw_nothing "$SANDBOX_LOG" "the sandbox"
        validated=", validation silent"
    fi

    echo "crcbl e2e: $how on x11/$backend, nothing left alive${validated}"
}

# `self_test_validation <backend> [windowed|fullscreen]` — the arguments
# `run_sandbox` takes, and it runs `run_sandbox` itself.
#
# **The pass that proves `assert_validation_saw_nothing` can fail.** Its two
# greps ask whether the messenger exists and whether it said anything; neither
# can tell "the layer had nothing to report" from "nothing could have reached
# this log if it had". A messenger the loader never calls back, a callback that
# stops reaching `log::error!`, a record whose module path or level moves out
# from under that `(ERROR|WARN) +crcbl_vk::debug] vk ` pattern, a `CRCBL_LOG`
# that filters it — each of those makes the second grep a green light wired to
# nothing, and a quiet log is what every one of them looks like.
#
# So this pass sets `CRCBL_VK_VALIDATION_SELF_TEST=1`, which asks a debug build
# of `crcbl-vk` to submit one synthetic message through
# `vkSubmitDebugUtilsMessageEXT` as the instance opens, and requires the whole
# of `run_sandbox` to come back **red** on it. `run_sandbox` in a subshell
# rather than the greps again, so what is shown able to fail is the check the
# other runs are graded by and not a copy that could drift from it.
#
# What it does NOT prove is that the layer is *checking* anything: a submitted
# message reaches the messenger even with the layer's own checks disabled
# (measured on layer 1.4.357 with `VALIDATE_CORE=false` and with
# `DISABLES=VK_VALIDATION_FEATURE_DISABLE_ALL`). The deliberate violation in
# `crates/crcbl-vk/tests/vk_e2e/validation_gate.rs` is what asks that question,
# and it needs a device and a command buffer to ask it with.
self_test_validation() {
    local refusal="${RUNTIME_DIR}/sandbox.self-test.log"
    # The `pMessageIdName` `crcbl-vk` gives the injected message. Nothing else
    # emits it, so the greps below can be exact — and they match the whole log
    # line rather than the id alone, because a rebuild's compiler output is
    # tee'd into this same log and a warning naming the constant would answer
    # the question the engine's own line is there to answer.
    local id="CRCBL-VALIDATION-SELF-TEST"
    local line="crcbl_vk::debug\] vk validation: ${id}"

    echo "crcbl e2e: re-running the sandbox with CRCBL_VK_VALIDATION_SELF_TEST=1, which must fail"
    # Everything this pass prints is captured: the run is *expected* to fail,
    # and a failure printed at the top level would read like a broken gate.
    local failed=0
    (
        export CRCBL_VK_VALIDATION_SELF_TEST=1
        run_sandbox "$@"
    ) >"$refusal" 2>&1 || failed=1

    # Did the message arrive at all? Asked first, because a self-test that was
    # never injected explains every other failure below it.
    if ! grep -qE "$line" "$SANDBOX_LOG"; then
        echo "crcbl e2e: the sandbox ran with CRCBL_VK_VALIDATION_SELF_TEST=1 and no ${id}" >&2
        echo "           line reached its log, so the validation check the other runs are" >&2
        echo "           graded by has never been seen to fail. Either the debug messenger" >&2
        echo "           is not calling back, or the callback no longer reaches" >&2
        echo "           crcbl_core::log::error!, or CRCBL_LOG dropped it." >&2
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    if [ "$failed" -eq 0 ]; then
        echo "crcbl e2e: the sandbox logged the injected ${id} message and still passed, so" >&2
        echo "           assert_validation_saw_nothing is not reading what the messenger writes." >&2
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    # ...and it failed *there* rather than somewhere else on the way. Both
    # halves: the check's own headline, and this message inside what it printed.
    if ! grep -qF "the validation layer complained about the sandbox" "$refusal" \
        || ! grep -qE "$line" "$refusal"; then
        echo "crcbl e2e: the sandbox failed with the self-test injected, but not on the" >&2
        echo "           validation check — so that check still has not been shown to fail." >&2
        echo "           What it did fail on:" >&2
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    echo "crcbl e2e: the sandbox went red on the injected ${id}, so the validation check is live"
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
    assert_nothing_left_alive "$SANDBOX_LOG" "the sandbox, after F11"
    local validated=""
    if [ "$backend" = "vk" ]; then
        assert_validation_saw_nothing "$SANDBOX_LOG" "the sandbox, after F11"
        validated=", validation silent"
    fi
    echo "crcbl e2e: $how, and the summary read 'at ${want_extent}, ${want_mode}' on x11/$backend, nothing left alive${validated}"
}

# See the equivalent block in `run-wayland-e2e.sh` for why the loader probe is a
# skip on a developer machine and a hard failure in CI.
if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    run_sandbox vk windowed
    run_sandbox vk fullscreen
    run_sandbox_toggle vk
    # One more windowed run, this time required to fail. After the three above
    # rather than instead of any of them: each of those was still graded by the
    # ordinary checks with nothing injected.
    self_test_validation vk windowed
else
    echo "crcbl e2e: no Vulkan loader; skipping the vk sandbox pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
fi
run_sandbox null windowed
