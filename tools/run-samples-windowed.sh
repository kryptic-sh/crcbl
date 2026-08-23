#!/usr/bin/env bash
# Run every sample in a real window, on a real GPU backend, and read back what
# it says it did.
#
#   tools/run-samples-windowed.sh
#
# # The gap this closes
#
# CI runs each sample `--headless` against lavapipe, and the shell's own e2e
# harnesses run the *sandbox* windowed against a real server. Nothing ran a
# **sample** windowed. Everything between a sample's `main` and a swapchain on an
# X11 window — the shell it picks, the surface it hands to `crcbl-hal`, the
# extent it opens at, the mode it ends up in — was covered for the sandbox and
# for nobody else, and a regression in a sample's windowed present failed no
# job at all. `--headless` cannot cover it: it never opens a window, so the
# whole path is the part it skips.
#
# So each sample runs here without `--headless`, for a fixed number of frames,
# and the one line it prints as it exits is the assertion: the frames it was
# asked for, the shell it used, the extent it came up at and the *effective*
# mode. A run that opened no window, fell back to another shell, or came up at
# some other size fails here.
#
# # Why the shell is named
#
# `CRCBL_SHELL=x11` for the same reason `run-x11-e2e.sh` names it: the registry
# tries Wayland first, and a silent fallback would report success for a backend
# this script never brought up. Here it would in fact fail — nothing is
# listening — but stating the intent is what keeps the gate honest if that ever
# changes.
#
# # One pass, not two
#
# `run-x11-e2e.sh` runs twice because it asserts things a window manager
# changes: decorations, size hints, whether `--fullscreen` is granted. This
# gate asserts none of them. A sample asks for its own size and gets it either
# way — a manager decorates *around* the client area rather than shrinking it,
# and with no manager nothing resizes the window at all — so setting
# `CRCBL_E2E_X11_WM` is supported and changes no expectation below.
#
# # ENVIRONMENT
#
#   CRCBL_E2E_SAMPLE_LOG   `CRCBL_LOG` for the sample runs. `info` by default,
#                          which is what puts the swapchain line in the log that
#                          gets dumped on failure.
#
# Everything `tools/x11-display.sh` reads applies too, `CRCBL_E2E_X11_WM`
# included.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Starting the display is `tools/x11-display.sh`'s job: it exports `DISPLAY`,
# `RUNTIME_DIR` and the rest, defines `log_tail`, and owns the cleanup trap. It
# exits this shell if the server never comes up.
# shellcheck source=tools/x11-display.sh
source "${REPO_ROOT}/tools/x11-display.sh"

cd "$REPO_ROOT"

SAMPLE_FRAMES=120

# Each sample and the window size it asks for, one line each so a sample that
# changes its own default fails on its own rather than being covered by a
# constant shared with the others. These are measured from the runs, not read
# off the `--size` help text — writing this list is what caught `bare` opening
# at 640x480 while the shared `--size` line in its own help said 960x720.
#
# Every binary in `apps/` that opens a window, less two: `sandbox`, which
# `run-x11-e2e.sh` already drives windowed and asserts far more of, and
# `viewer`, which takes a model path and has no committed `.glb` to open — see
# `docs/backlog.md` for what closing that needs. `render-harness` is a library
# and `sim` is a headless determinism harness, so neither has a window to open.
SAMPLES=(
    "asteroids 960x720"
    "bare 960x720"
    "breakout 960x720"
    "flappy 960x720"
    "horde 960x720"
    "hud 960x720"
    "lantern 960x720"
    "quarry 960x720"
)

# `run_sample <name> <WxH>`
#
# The failure discipline is `run_sandbox`'s in `run-x11-e2e.sh`: the run itself
# is the only thing inside `set +e`, the status comes off `PIPESTATUS` rather
# than `tee`'s, and every path out prints the sample's log and the server's
# before it exits.
run_sample() {
    local sample="$1" want_extent="$2"
    local log="${RUNTIME_DIR}/${sample}.log"

    echo "crcbl e2e: running ${sample} windowed against Xvfb on the vk GPU backend"
    set +e
    CRCBL_SHELL=x11 \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SAMPLE_LOG:-info}" \
        cargo run --locked --quiet --package "$sample" -- \
        --backend vk --frames "$SAMPLE_FRAMES" 2>&1 | tee "$log"
    local status=${PIPESTATUS[0]}
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl e2e: ${sample} failed against Xvfb on vk (exit $status)" >&2
        log_tail
        exit "$status"
    fi

    # The summary is one line, and everything below is read off *that* line: a
    # run whose frame count came from the engine's own "last 119 frames" pacing
    # log and whose extent came from a swapchain line it rebuilt on the way out
    # would pass four separate greps and never have been the same run.
    local summary
    summary="$(grep -m1 -E "^${sample}: " "$log" || true)"
    if [ -z "$summary" ]; then
        echo "crcbl e2e: ${sample} exited 0 and printed no summary line" >&2
        cat "$log" >&2
        log_tail
        exit 1
    fi

    # What it was asked for, so a run that stopped early fails rather than
    # reporting whatever it reached.
    if [[ "$summary" != "${sample}: ${SAMPLE_FRAMES} frames, "* ]]; then
        echo "crcbl e2e: ${sample} did not present ${SAMPLE_FRAMES} frames: ${summary}" >&2
        cat "$log" >&2
        log_tail
        exit 1
    fi

    # The shell it actually used, which is the point of naming one.
    if [[ "$summary" != *" on the x11 shell at "* ]]; then
        echo "crcbl e2e: ${sample} did not run on the x11 shell: ${summary}" >&2
        cat "$log" >&2
        log_tail
        exit 1
    fi

    # The extent and the *effective* mode, off the one line together — a sample
    # that reported its own request rather than what the window system did
    # would say something else here. The trailing space is what stops
    # "windowed" from matching a longer mode name.
    if [[ "$summary" != *" at ${want_extent}, windowed "* ]]; then
        echo "crcbl e2e: ${sample} did not come up at ${want_extent} windowed: ${summary}" >&2
        cat "$log" >&2
        log_tail
        exit 1
    fi

    # **What the sample was still holding when its device went away.**
    # `crcbl-vk` warns at teardown for every object nobody destroyed, naming the
    # kinds and formats, and a warning fails nothing — `run-vk-e2e.sh` reads the
    # same line for the same reason. That gate covers the *suite*; this is the
    # only place a **sample** is asked the question, because a sample's teardown
    # runs only when a real run ends, which is what this script is. The browser
    # gate's group I asks it of the same games on the WebGPU side.
    #
    # Zero lines, not a judgement call: a sample that must leave something alive
    # has to say so here deliberately rather than inside a warning nobody reads.
    local leaks
    leaks="$(grep -F 'object(s) still alive at device teardown' "$log" || true)"
    if [ -n "$leaks" ]; then
        echo "crcbl e2e: ${sample} destroyed its device with objects still alive:" >&2
        while IFS= read -r line; do
            echo "               $line" >&2
        done <<<"$leaks"
        echo "           The sample's own teardown reporter wrote that. Destroy them" >&2
        echo "           where they were made — the kinds and formats above are what" >&2
        echo "           it saw — rather than leaving the line in a log." >&2
        log_tail
        exit 1
    fi

    echo "crcbl e2e: ${sample} presented ${SAMPLE_FRAMES} frames at ${want_extent} windowed on x11/vk, nothing left alive"
}

# See the equivalent block in `run-x11-e2e.sh` for why the loader probe is a
# skip on a developer machine and a hard failure in CI. There is no null-backend
# fallback here: a sample that never reached a swapchain is exactly what this
# gate is for, so with no loader there is nothing left worth running.
if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    for entry in "${SAMPLES[@]}"; do
        read -r sample extent <<<"$entry"
        run_sample "$sample" "$extent"
    done
    echo "crcbl e2e: ${#SAMPLES[@]} samples ran windowed against Xvfb on ${DISPLAY}"
else
    echo "crcbl e2e: no Vulkan loader; skipping the windowed sample pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
fi
