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

# And what the validation layer said is `tools/vk-validation-log.sh`'s, for
# the same reason: three harnesses carried the same two greps and the same
# five error messages, and a fourth was about to.
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

cd "$REPO_ROOT"

SAMPLE_FRAMES=120

# The one model this gate opens, written into the display's own scratch
# directory by `generate_viewer_model` below just before the loop runs.
#
# The name is letters and a dot deliberately: `viewer` makes the file's own
# directory the asset root and the file's name the asset key, and its `USAGE`
# says an asset key holds only letters, digits, `.`, `_` and `-`. `RUNTIME_DIR`
# is `mktemp -d -t`'s, so the path carries no spaces — which the table below
# needs, since it splits each entry on whitespace.
VIEWER_MODEL="${RUNTIME_DIR}/triangle.glb"

# Each sample and the window size it asks for, one line each so a sample that
# changes its own default fails on its own rather than being covered by a
# constant shared with the others. These are measured from the runs, not read
# off the `--size` help text — writing this list is what caught `bare` opening
# at 640x480 while the shared `--size` line in its own help said 960x720.
#
# Every binary in `apps/` that opens a window, less one: `sandbox`, which
# `run-x11-e2e.sh` already drives windowed and asserts far more of.
# `render-harness` is a library and `sim` is a headless determinism harness, so
# neither has a window to open.
#
# Anything after the extent is handed to the sample itself. `viewer` is the only
# entry that needs it — it takes the model as a positional argument — so the
# table stays one line per sample rather than growing a second shape for one
# member.
SAMPLES=(
    "asteroids 960x720"
    "bare 960x720"
    "bracket 960x720"
    "breach 960x720"
    "breakout 960x720"
    "flappy 960x720"
    "horde 960x720"
    "hud 960x720"
    "lantern 960x720"
    "orbit 960x720"
    "puppet 960x720"
    "quarry 960x720"
    "shard 960x720"
    "sparks 960x720"
    "viewer 960x720 ${VIEWER_MODEL}"
)

# Write the model `viewer` opens, and stop the gate if it is not a document this
# engine can read back.
#
# `crcbl-scene`'s `gltf-fixture` feature is what builds it: the same triangle
# that crate's own tests import, through the same code, so a fixture that stops
# being valid glTF fails in the crate that owns glTF instead of reaching
# `viewer` as a confusing load error. Committing a `.glb` was the alternative
# and is the thing that module exists to avoid — a binary container is a fixture
# nobody reviewing a change can read.
#
# The writer refuses to overwrite anything, which is why the path is a fresh one
# under `RUNTIME_DIR` on every run, and it re-imports what it wrote before it
# exits 0.
generate_viewer_model() {
    echo "crcbl e2e: writing ${VIEWER_MODEL} from crcbl-scene's gltf-fixture triangle"
    cargo run --locked --quiet --package crcbl-scene --features gltf-fixture \
        --example write-triangle-glb -- "$VIEWER_MODEL"
    echo "crcbl e2e: viewer opens $(basename "$VIEWER_MODEL"), $(wc -c <"$VIEWER_MODEL") bytes, from ${RUNTIME_DIR}"
}

# `run_sample <name> <WxH> [sample args...]`
#
# Everything after the extent goes to the sample ahead of the shared flags.
#
# The failure discipline is `run_sandbox`'s in `run-x11-e2e.sh`: the run itself
# is the only thing inside `set +e`, the status comes off `PIPESTATUS` rather
# than `tee`'s, and every path out prints the sample's log and the server's
# before it exits.
run_sample() {
    local sample="$1" want_extent="$2"
    shift 2
    local log="${RUNTIME_DIR}/${sample}.log"

    echo "crcbl e2e: running ${sample} windowed against Xvfb on the vk GPU backend"
    set +e
    CRCBL_SHELL=x11 \
    CRCBL_VK_VALIDATION=1 \
    CRCBL_LOG="${CRCBL_E2E_SAMPLE_LOG:-info}" \
        cargo run --locked --quiet --package "$sample" -- "$@" \
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

    # **And what the validation layer said, which until now was nothing this
    # script could act on.** `CRCBL_VK_VALIDATION=1` is set on the run above
    # and a violation only reaches `crcbl_core::log::error!` — the sample still
    # exits 0, so every run advertised that it was validating and none could
    # fail because of it. `tools/vk-validation-log.sh` carries both halves of
    # the question and why neither stands without the other; this is the third
    # harness to ask it and the first not to spell it out again.
    if ! crcbl_validation_saw_nothing "$log" "$sample"; then
        log_tail
        exit 1
    fi

    echo "crcbl e2e: ${sample} presented ${SAMPLE_FRAMES} frames at ${want_extent} windowed on x11/vk, nothing left alive, validation silent"
}

# `self_test_validation <name> <WxH> [sample args...]` — the same arguments
# `run_sample` takes, and it runs `run_sample` itself.
#
# **The pass that proves the validation check above can fail.** The two greps in
# `run_sample` ask whether the messenger exists and whether it said anything;
# neither can tell "the layer had nothing to report" from "nothing could have
# reached this log if it had". A messenger the loader never calls back, a
# callback that stops reaching `log::error!`, a record whose module path or
# level moves out from under `tools/vk-validation-log.sh`'s pattern, a
# `CRCBL_LOG` that filters it — each of those makes the second grep a green
# light wired to nothing, and each is invisible from a log that is simply
# quiet.
#
# So this pass sets `CRCBL_VK_VALIDATION_SELF_TEST=1`, which asks a debug build
# of `crcbl-vk` to submit one synthetic message through
# `vkSubmitDebugUtilsMessageEXT` as the instance opens, and then requires the
# whole of `run_sample` to come back **red** on it. It calls `run_sample` in a
# subshell rather than repeating the greps, so what is proven able to fail is
# the check the other runs are graded by, not a copy of it that could drift
# from it.
#
# What this does NOT prove is that the layer is *checking* anything: a
# submitted message reaches the messenger even with the layer's own checks
# disabled (measured on layer 1.4.357 with `VALIDATE_CORE=false` and with
# `DISABLES=VK_VALIDATION_FEATURE_DISABLE_ALL`). The deliberate violation in
# `crates/crcbl-vk/tests/vk_e2e/validation_gate.rs` is what asks that question,
# and it needs a device and a command buffer to ask it with.
self_test_validation() {
    local sample="$1"
    local log="${RUNTIME_DIR}/${sample}.log"
    local refusal="${RUNTIME_DIR}/${sample}.self-test.log"
    # The `pMessageIdName` `crcbl-vk` gives the injected message. Nothing else
    # emits it, so the greps below can be exact — and they match the whole log
    # line rather than the id alone, because a rebuild's compiler output is
    # tee'd into this same log and a warning naming the constant would answer
    # the question the engine's own line is there to answer.
    local id="CRCBL-VALIDATION-SELF-TEST"
    local line="crcbl_vk::debug\] vk validation: ${id}"

    echo "crcbl e2e: re-running ${sample} with CRCBL_VK_VALIDATION_SELF_TEST=1, which must fail"
    # Everything the pass says is captured: this run is *expected* to print a
    # failure, and printing one at the top level would read like a broken gate.
    local failed=0
    (
        export CRCBL_VK_VALIDATION_SELF_TEST=1
        export CRCBL_VK_VALIDATION_PROVOKE=1
        run_sample "$@"
    ) >"$refusal" 2>&1 || failed=1

    # Did the message arrive at all? Asked first, because a self-test that was
    # never injected explains every other failure below it.
    if ! grep -qE "$line" "$log"; then
        echo "crcbl e2e: ${sample} ran with CRCBL_VK_VALIDATION_SELF_TEST=1 and no ${id}" >&2
        echo "           line reached its log, so the validation check the other runs" >&2
        echo "           are graded by has never been seen to fail. Either the debug" >&2
        echo "           messenger is not calling back, or the callback no longer" >&2
        echo "           reaches crcbl_core::log::error!, or CRCBL_LOG dropped it." >&2
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    if [ "$failed" -eq 0 ]; then
        echo "crcbl e2e: ${sample} logged the injected ${id} message and still passed," >&2
        echo "           so run_sample's validation grep is not reading what the" >&2
        echo "           messenger writes." >&2
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    # ...and it failed *there* rather than somewhere else on the way. Both
    # halves: the check's own headline, and this message inside what it printed.
    if ! grep -qF "the validation layer complained about ${sample}" "$refusal" \
        || ! grep -qE "$line" "$refusal"; then
        echo "crcbl e2e: ${sample} failed with the self-test injected, but not on the" >&2
        echo "           validation check — so that check still has not been shown to" >&2
        echo "           fail. What it did fail on:" >&2
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    # **And whether the layer was checking at all** — a different question from
    # everything above it, and the only one the injected message cannot answer:
    # a submitted message is delivered whatever the layer's checks are set to.
    # `CRCBL_VK_VALIDATION_PROVOKE=1` is exported beside the self-test, so this
    # is graded off the same run and costs no extra binary.
    if ! crcbl_validation_layer_checked "$log" "${sample}"; then
        cat "$refusal" >&2
        log_tail
        exit 1
    fi
    echo "crcbl e2e: ${sample} went red on the injected ${id} and the layer answered a"
    echo "           provoked violation, so the validation check is live and checking"
}

# See the equivalent block in `run-x11-e2e.sh` for why the loader probe is a
# skip on a developer machine and a hard failure in CI. There is no null-backend
# fallback here: a sample that never reached a swapchain is exactly what this
# gate is for, so with no loader there is nothing left worth running.
if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    generate_viewer_model
    for entry in "${SAMPLES[@]}"; do
        # Name, extent, and whatever else the entry carries for the sample
        # itself — an array rather than three `read` variables so a sample with
        # no extra arguments passes none rather than an empty string.
        read -r -a fields <<<"$entry"
        run_sample "${fields[@]}"
    done
    echo "crcbl e2e: ${#SAMPLES[@]} samples ran windowed against Xvfb on ${DISPLAY}"
    # One more run of the first sample, this time required to fail. It runs
    # after the loop, not instead of any of it: every sample above was still
    # graded by the ordinary passes with nothing injected.
    read -r -a fields <<<"${SAMPLES[0]}"
    self_test_validation "${fields[@]}"
else
    echo "crcbl e2e: no Vulkan loader; skipping the windowed sample pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
fi
