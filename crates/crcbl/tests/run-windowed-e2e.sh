#!/usr/bin/env bash
# Run the windowed-swapchain suite against a private headless Xvfb.
#
#   crates/crcbl/tests/run-windowed-e2e.sh [extra nextest args…]
#
# # What this is for
#
# Every other GPU harness in this workspace runs against
# `SurfaceTarget::Offscreen`, and on `crcbl-vk` a null `VkSurfaceKHR` is the
# discriminator the whole backend branches on: `acquire_next_frame` and
# `present` return from their offscreen arms before `vkAcquireNextImageKHR` and
# `vkQueuePresentKHR` are ever reached. So the acquire semaphores, the per-slot
# acquire fence, `VkPresentIdKHR`, the suboptimal flag, the `oldSwapchain`
# handoff and the extent clamp against a *real* `VkSurfaceCapabilitiesKHR` were
# covered by nothing. `tests/windowed_e2e.rs` reaches them, and this is the only
# thing that turns it on.
#
# It is the shell's `run-x11-e2e.sh` with the halves swapped. That script runs
# the **window system** against a real server and takes a GPU along; this one
# runs the **GPU's presentation path** and takes a window system along. Neither
# subsumes the other: that suite would still pass with every swapchain call
# stubbed, and this one asserts nothing about focus, decorations, clipboard or
# input.
#
# Exits non-zero if Xvfb will not start, if the display does not answer before
# the deadline, if no tests ran, if the suite never named the adapter it
# presented on, or if any test fails. The server log tail goes out on every
# failure path.
#
# # Why X11, and why a window manager changes nothing here
#
# X11 is the interesting server for this suite and `crates/crcbl-vk/src/swapchain.rs`
# says why: the server reports `minImageExtent == maxImageExtent ==
# currentExtent`, so the legal range for `imageExtent` is a single point and
# clamping is forced rather than chosen. Measured on this harness's own display
# — `vulkaninfo` under Xvfb reports all three equal on llvmpipe — rather than
# taken on trust.
#
# `CRCBL_E2E_X11_WM` is supported and changes no expectation below, which is the
# opposite of `run-x11-e2e.sh`'s position and is deliberate: that suite asserts
# things a window manager owns (decorations, size hints, whether a fullscreen
# request is granted), and this one asserts only that the swapchain agrees with
# whatever geometry the window ended up with. `openbox` honours a client's
# requested size — it decorates *around* the client area rather than shrinking
# it — and the tests read the size back from the shell rather than assuming it.
# **CI runs it both ways all the same**, because "changes nothing" is a claim,
# and a claim nothing executes is one that stops being true quietly.
#
# # Xvfb has no DRI3, and that is expected
#
# So RADV refuses to present to this display and the suite's surface-aware
# adapter loop falls through to Mesa's `llvmpipe`. Nothing here asserts which
# adapter was chosen — only that *one* was, and that it presented.
#
# # ENVIRONMENT
#
# Everything `tools/x11-display.sh` reads applies, `CRCBL_E2E_X11_WM` included.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# Reading nextest's summary is `tools/nextest-summary.sh`'s job, in one copy
# rather than one per harness: five of the inline copies it replaced read a
# cancelled run's `2/15 tests run` as a healthy fifteen.
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

# And starting the display is `tools/x11-display.sh`'s: it exports `DISPLAY`,
# `SCREEN`, `RUNTIME_DIR`, `XVFB_PID` and `WM_PID`, defines `log_tail`, owns the
# cleanup trap, and starts a window manager when `CRCBL_E2E_X11_WM` is set. It
# exits this shell if the server never comes up.
# shellcheck source=tools/x11-display.sh
source "${REPO_ROOT}/tools/x11-display.sh"

cd "$REPO_ROOT"

# The loader probe, in `run-x11-e2e.sh`'s shape and for its reason: a developer
# machine without a Vulkan runtime should say so and move on, and CI installs
# one on purpose, so a skip there is the silently-skipped-e2e trap
# `docs/plan/12-testing.md` names. There is no null-backend fallback to fall
# through to — a run that never reached a `VkSwapchainKHR` is exactly what this
# gate is about.
if [ -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] || [ -e /usr/lib/libvulkan.so.1 ] \
    || ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1'; then
    :
else
    echo "crcbl windowed e2e: no Vulkan loader; skipping the windowed swapchain pass" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl windowed e2e: ...and this is CI, where the loader is installed on purpose" >&2
        exit 1
    fi
    exit 0
fi

OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
# `--test-threads 1` because each test opens its own X connection, its own
# Vulkan instance and its own window on one shared server, and because the
# validation report each fixture asserts on is per-instance: two tests racing
# would still each read their own report, but the display would be answering
# geometry questions about two windows at once.
#
# `--success-output immediate` because the lines this suite prints — the adapter
# it presented on, whether present feedback was granted, how many swapchain
# images the run rotated through — are only interesting on a green run, which is
# the run nextest captures them on.
CRCBL_VK_VALIDATION=1 \
CRCBL_LOG="${CRCBL_E2E_WINDOWED_LOG:-info}" \
    cargo nextest run \
    --locked \
    --package crcbl \
    --features windowed-e2e \
    --test windowed_e2e \
    --run-ignored all \
    --no-tests fail \
    --test-threads 1 \
    --success-output immediate \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

# **A window manager that died mid-run is not a test failure, it is a lost
# gate** — `run-x11-e2e.sh`'s check, and here it matters for the geometry: every
# extent assertion below is against whatever size the window ended up at, and a
# manager that vanished halfway leaves half the run measuring a different
# configuration from the other half. Checked before the suite's own status,
# because "openbox is gone" is the more useful sentence.
if [ -n "${WM_PID:-}" ] && ! kill -0 "$WM_PID" 2>/dev/null; then
    echo "crcbl windowed e2e: the window manager (${CRCBL_E2E_X11_WM}) exited during the suite" >&2
    log_tail
    exit 1
fi

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl windowed e2e: the suite failed" >&2
    log_tail
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job — and so is one nextest cancelled
# after two tests, whose `Summary [ 0.1s] 2/15 tests run` still ends in the total
# it never reached. The colour-stripped copy is load-bearing because CI sets
# `CARGO_TERM_COLOR: always`, which wraps the counts in escapes.
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
crcbl_nextest_plain "$OUTPUT" "$PLAIN"
if ! crcbl_nextest_summary "$PLAIN" "crcbl windowed e2e" \
    "The windowed-e2e feature or the ignore attribute stopped matching the tests."; then
    log_tail
    exit 1
fi

# Which adapter actually presented, read off the suite's own output. Every
# fixture prints this line as it opens a device, so its absence means no test
# reached a presenting device at all — a state a green nextest run cannot
# distinguish from a healthy one, because "all zero tests I ran passed" and "the
# fixture never got that far" both end in a summary.
ADAPTER="$(grep -F 'crcbl windowed e2e: presenting on adapter ' "$PLAIN" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl windowed e2e: the suite never named the adapter it presented on." >&2
    echo "  The fixture must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a swapchain nobody wrote down." >&2
    log_tail
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl windowed e2e: ${ADAPTER#*crcbl windowed e2e: }"

echo "crcbl windowed e2e: $CRCBL_NEXTEST_TESTS_RUN tests presented to a real window on ${DISPLAY}"
