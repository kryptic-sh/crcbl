#!/usr/bin/env bash
# Run `crcbl-wgpu`'s end-to-end suite against a real GPU through wgpu.
#
#   crates/crcbl-wgpu/tests/run-wgpu-e2e.sh [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` on a machine with no adapter
# stays green. This script is the only thing that turns them on, and CI runs this
# script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a known
# trap, so the script fails when the suite reports zero tests run.
#
# WHAT THIS COVERS, IN TWO HALVES
#   1. **Offscreen**, via `cargo nextest`: the image ring, the acquire/present
#      cycle over it, and the `map_async` readback that turns a rendered frame
#      into bytes. No display needed. This is the half the cross-backend image
#      comparison depends on.
#
#   2. **Windowed**, via `apps/sandbox` and `apps/breakout` under Xvfb, because a
#      swapchain needs a window system and no `#[test]` can conjure one. This
#      half is not decoration. `wgpu::SurfaceTexture`'s `Drop` *discards* the
#      acquired image instead of presenting it, so a backend that dropped it ran
#      for exactly `image_count` frames and then blocked until it timed out —
#      P5.11's headline bug. **The frame budget below must stay larger than the
#      swapchain's image count**, or this stops testing anything: a 3-frame run
#      passes with the bug still in place.
#
# ENVIRONMENT
#   CRCBL_WGPU_ICD    Pin a Vulkan ICD manifest for wgpu's Vulkan backend, e.g.
#                     lavapipe's `lvp_icd.json`. CI sets this so a runner that
#                     grows a GPU does not silently stop testing the software
#                     path.
#   CRCBL_WGPU_DISPLAY  X display number for the Xvfb half. Default `:97`.
#   CRCBL_WGPU_FRAMES   Frame budget for the windowed runs. Default 60.
#
# Exits non-zero if no tests ran, if any test fails, if Xvfb is unavailable, or
# if either windowed run failed to reach its frame budget.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"
DISPLAY_NUM="${CRCBL_WGPU_DISPLAY:-:97}"
FRAMES="${CRCBL_WGPU_FRAMES:-60}"

if [ -n "${CRCBL_WGPU_ICD:-}" ]; then
    if [ ! -f "$CRCBL_WGPU_ICD" ]; then
        # Distributions disagree about the suffix: Debian ships
        # `lvp_icd.x86_64.json` and Arch ships `lvp_icd.json`. A miss is still a
        # hard failure — a pinned ICD that silently fell back to whatever the
        # loader found would defeat the point of pinning it.
        ICD_DIR="$(dirname "$CRCBL_WGPU_ICD")"
        ICD_STEM="$(basename "$CRCBL_WGPU_ICD")"
        ICD_STEM="${ICD_STEM%%.*}"
        FOUND=""
        for CANDIDATE in "${ICD_DIR}/${ICD_STEM}".json "${ICD_DIR}/${ICD_STEM}".*.json; do
            if [ -f "$CANDIDATE" ]; then
                FOUND="$CANDIDATE"
                break
            fi
        done
        if [ -z "$FOUND" ]; then
            echo "crcbl wgpu e2e: CRCBL_WGPU_ICD=$CRCBL_WGPU_ICD does not exist, and no sibling matched" >&2
            ls -la "$ICD_DIR" >&2 || true
            exit 1
        fi
        echo "crcbl wgpu e2e: $CRCBL_WGPU_ICD is absent; using $FOUND"
        CRCBL_WGPU_ICD="$FOUND"
    fi
    export VK_DRIVER_FILES="$CRCBL_WGPU_ICD"
    export VK_ICD_FILENAMES="$CRCBL_WGPU_ICD"
    echo "crcbl wgpu e2e: pinned ICD $CRCBL_WGPU_ICD"
fi

cd "$REPO_ROOT"
OUTPUT="$(mktemp -t crcbl-wgpu-e2e.XXXXXX.log)"
XVFB_PID=""
cleanup() {
    local status=$?
    rm -f "$OUTPUT" "${OUTPUT}.plain"
    if [ -n "$XVFB_PID" ]; then
        kill "$XVFB_PID" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

# ---------------------------------------------------------------------------
# Half 1: the offscreen suite
# ---------------------------------------------------------------------------

set +e
cargo nextest run \
    --locked \
    --package crcbl-wgpu \
    --features wgpu-e2e \
    --test wgpu_e2e \
    --run-ignored all \
    --test-threads 1 \
    --no-capture \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

# The colour-stripped copy is load-bearing for every match below — CI sets
# `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and a
# plain-text match sees no digits next to "tests run".
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"${OUTPUT}.plain"

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl wgpu e2e: the offscreen suite failed" >&2
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job.
RAN="$(grep -Eo '[0-9]+ tests? run' "${OUTPUT}.plain" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl wgpu e2e: the suite reported no tests run — the gate is not gating" >&2
    exit 1
fi

# Which adapter actually ran, from the suite rather than from the manifest that
# was asked for. A pinned ICD the loader quietly ignored shows up here.
ADAPTER="$(grep -Eo 'wgpu e2e: adapter .*' "${OUTPUT}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl wgpu e2e: the suite never named an adapter — it did not open a device" >&2
    exit 1
fi
echo "crcbl wgpu e2e: ${ADAPTER#wgpu e2e: }"
echo "crcbl wgpu e2e: $RAN offscreen tests ran against a real adapter"

# ---------------------------------------------------------------------------
# Half 2: the windowed acquire/present cycle, under Xvfb
# ---------------------------------------------------------------------------

if ! command -v Xvfb >/dev/null 2>&1; then
    echo "crcbl wgpu e2e: Xvfb is not installed, so the windowed half cannot run." >&2
    echo "                A run without it checks the offscreen ring only, and the" >&2
    echo "                present bug it stands in for is windowed-only." >&2
    echo "  Debian/Ubuntu: xvfb" >&2
    echo "  Arch:          xorg-server-xvfb" >&2
    exit 1
fi

Xvfb "$DISPLAY_NUM" -screen 0 1280x720x24 -ac >/dev/null 2>&1 &
XVFB_PID=$!
# Poll for the socket with a deadline — never a fixed sleep
# (`docs/plan/12-testing.md`).
SOCKET="/tmp/.X11-unix/X${DISPLAY_NUM#:}"
for _ in $(seq 1 100); do
    [ -e "$SOCKET" ] && break
    sleep 0.05
done
if [ ! -e "$SOCKET" ]; then
    echo "crcbl wgpu e2e: Xvfb never created $SOCKET" >&2
    exit 1
fi
echo "crcbl wgpu e2e: Xvfb is up on $DISPLAY_NUM"

run_windowed() {
    local app="$1"
    local log
    log="$(mktemp -t "crcbl-wgpu-${app}.XXXXXX.log")"
    set +e
    DISPLAY="$DISPLAY_NUM" CRCBL_SHELL=x11 \
        cargo run --locked --quiet --package "$app" -- \
        --frames "$FRAMES" --backend wgpu 2>&1 | tee "$log"
    local status=${PIPESTATUS[0]}
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl wgpu e2e: $app failed against wgpu (exit $status)" >&2
        rm -f "$log"
        exit "$status"
    fi
    # The frame count is the assertion. A backend that discarded the acquired
    # image instead of presenting it stops after `image_count` frames with
    # "timed out waiting for a swapchain image", which exits non-zero above —
    # but a future regression that merely *skipped* frames would not, so the
    # count is checked rather than the exit code alone.
    if ! grep -q "${FRAMES} frames" "$log"; then
        echo "crcbl wgpu e2e: $app did not report ${FRAMES} frames" >&2
        cat "$log" >&2
        rm -f "$log"
        exit 1
    fi
    rm -f "$log"
    echo "crcbl wgpu e2e: $app presented ${FRAMES} frames through wgpu on X11"
}

run_windowed sandbox
run_windowed breakout

# And headless, which is the offscreen ring driven by the whole engine rather
# than by a test — the shape CI runs everything in.
HEADLESS_LOG="$(mktemp -t crcbl-wgpu-headless.XXXXXX.log)"
set +e
cargo run --locked --quiet --package breakout -- \
    --headless --frames 30 --backend wgpu 2>&1 | tee "$HEADLESS_LOG"
HEADLESS_STATUS=${PIPESTATUS[0]}
set -e
if [ "$HEADLESS_STATUS" -ne 0 ] || ! grep -q "30 frames" "$HEADLESS_LOG"; then
    echo "crcbl wgpu e2e: breakout headless did not reach 30 frames on wgpu" >&2
    cat "$HEADLESS_LOG" >&2
    rm -f "$HEADLESS_LOG"
    exit 1
fi
rm -f "$HEADLESS_LOG"
echo "crcbl wgpu e2e: breakout rendered 30 frames into the offscreen ring"
