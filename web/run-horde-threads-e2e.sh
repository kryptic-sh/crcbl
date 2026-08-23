#!/usr/bin/env bash
# Run the horde demo in a real browser and prove its simulation left the main
# thread.
#
#   ./web/run-horde-threads-e2e.sh [--no-build]
#
# WHAT THIS IS THE ONLY GATE FOR
#   `web/run-jobs-e2e.sh` proves the worker backend works in a browser, against
#   `crates/crcbl-jobs/examples/web_worker_gate.rs` — a page with no engine, no
#   canvas and no assets, whose exports exist to be observed. That is the right
#   shape for the backend and it is not a sample. This script is the other
#   claim, and the one P5B's last exit criterion names: **a sample's sim runs off
#   the main thread in a browser.**
#
#   It drives `demos/horde/` — the page a visitor loads, the shim a visitor runs,
#   the loader swapped for one that constructs a shared memory — and asserts
#   `__crcbl_horde_sim_threads() >= 2`.
#
#   That assertion exists because nothing else can make it. `steer_enemies` is
#   bit-identical at any worker count by construction (`apps/horde/src/game.rs`
#   says so, and `steering_is_bit_identical_however_many_workers_run_it` holds
#   it), so a threaded run and an inline run draw the same frames: a screenshot,
#   a status code and a log line are all satisfied by a run that never left the
#   main thread. Red check C below is that exact run.
#
# THE THREE RED CHECKS ARE THE GATE, NOT A DEBUGGING AID
#   Each turns the exit criterion red, and each leaves a different neighbour
#   green — because "something failed" is not evidence that the assertion under
#   test is the thing that noticed.
#
#   A  ?no-host-ready   the page never announces itself, so `Spawn::threaded()`
#                       stays false and every chunk runs on the caller. The demo
#                       still plays and still steers.
#   B  --prefill 0      no crowd, so horde waits at its title screen and the
#                       pass never runs. The workers are up and announced.
#   C  the published site   the artifacts `web/build.sh` builds, loaded by
#                       `web/tools/wasm-loader.js` through the demo path a
#                       visitor takes. It must fail this gate and pass
#                       everything else — which is simultaneously the red check
#                       and the proof that publishing is unaffected.
#
# WHAT IT NEEDS
#   * **A Chromium or Chrome with WebGPU.** Horde draws, and it will not tick
#     until a device opens: `__crcbl_horde_frame` polls the request while the
#     status is BOOTING, so a browser with no adapter never reaches a steering
#     pass at all. `CRCBL_CHROMIUM` pins one.
#   * **Xvfb**, for the reason `web/run-browser-e2e.sh` documents at length:
#     headless plus SwiftShader is the box a machine with no GPU lands in, and
#     it is the one that reports a rendered frame as blank. Nothing here reads a
#     pixel, but the device loss that table describes takes the demo down with
#     it.
#   * **Node 22 or newer**, for the global `WebSocket` the DevTools client uses.
#   * **The `--threads` toolchain**, unless `--no-build`: `web/build.sh` names
#     the nightly and the `rust-src` component, and fails saying which is
#     missing.
#
# ENVIRONMENT
#   SITE_DIR, THREADED_SITE_DIR   Where the two sites are assembled.
#   PROFILE                       `release` (default) or `debug`.
#   CRCBL_CHROMIUM, CRCBL_CHROMIUM_FLAGS, CRCBL_CHROMIUM_NO_SANDBOX
#                                 As the other browser gates here.
#
# EXIT CODES
#   0  the green run passed every check, and all three red checks broke the
#      exit criterion while leaving their named neighbour alone.
#   1  a check failed, or a red check did not go red where it should have.
#   2  it could not run at all — no browser, no node, nothing built.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="${SITE_DIR:-$REPO/target/site}"
THREADED_SITE="${THREADED_SITE_DIR:-$REPO/target/site-threaded}"
BUILD=1

# The exit criterion, spelled exactly as `web/tools/horde-threads-e2e.mjs` names
# it. Written once because every guard below matches it literally: renaming it
# there is meant to fail here and be renamed here too.
CRITERION='a steering chunk ran on a thread that is not the main thread'

# How long a red run is given. It can never satisfy the criterion, so it always
# runs to its deadline — short enough that three of them are not the whole cost
# of this script, long enough that the demo has certainly booted a device and
# steered.
RED_TIMEOUT_MS=45000

# How long to wait for an X socket, in seconds.
DISPLAY_TIMEOUT_S=15

while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-build)
            BUILD=0
            shift
            ;;
        *)
            echo "run-horde-threads-e2e.sh: unknown argument $1" >&2
            echo "usage: ./web/run-horde-threads-e2e.sh [--no-build]" >&2
            exit 2
            ;;
    esac
done

if ! command -v node >/dev/null 2>&1; then
    echo "crcbl horde threads e2e: node not found; this harness needs Node 22 or newer" >&2
    exit 2
fi
NODE_MAJOR="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
if [ "$NODE_MAJOR" -lt 22 ]; then
    echo "crcbl horde threads e2e: node $(node --version) is too old; the DevTools client needs the global WebSocket from Node 22" >&2
    exit 2
fi

if [ "$BUILD" = "1" ]; then
    # Both sites, because red check C is the published one. They are built by
    # the same script this repository's Pages workflow runs, so "it works in CI"
    # and "it works here" stay the same claim.
    echo "==> building the threaded site"
    "$REPO/web/build.sh" --threads
    echo "==> building the published site"
    "$REPO/web/build.sh"
fi

for site in "$THREADED_SITE" "$SITE"; do
    if [ ! -f "$site/demos/horde/index.html" ]; then
        echo "crcbl horde threads e2e: $site/demos/horde is missing; re-run without --no-build" >&2
        exit 2
    fi
done

RUNTIME_DIR="$(mktemp -d -t crcbl-horde-threads-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
XVFB_PID=""
cleanup() {
    status=$?
    if [ -n "$XVFB_PID" ]; then
        kill "$XVFB_PID" 2>/dev/null || true
        wait "$XVFB_PID" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

# Inherit nothing from an outer session: a developer running this on a live
# desktop must not have a browser window appear on their screen, and a stale
# `WAYLAND_DISPLAY` would send Chromium looking for a compositor this did not
# start.
unset WAYLAND_DISPLAY
unset DISPLAY
unset XAUTHORITY

if ! command -v Xvfb >/dev/null 2>&1; then
    echo "crcbl horde threads e2e: Xvfb is not installed." >&2
    echo "               On a machine with no GPU, headless plus SwiftShader is the" >&2
    echo "               configuration web/run-browser-e2e.sh measured as losing the" >&2
    echo "               WebGPU device part-way through a run — so this refuses rather" >&2
    echo "               than reporting a demo that died as a demo with no threads." >&2
    exit 2
fi

DISPLAY_NUM=""
for candidate in $(seq 90 120); do
    if [ ! -e "/tmp/.X${candidate}-lock" ] && [ ! -e "/tmp/.X11-unix/X${candidate}" ]; then
        DISPLAY_NUM="$candidate"
        break
    fi
done
if [ -z "$DISPLAY_NUM" ]; then
    echo "crcbl horde threads e2e: no free X display number in :90-:120" >&2
    exit 1
fi

echo "==> starting Xvfb on :${DISPLAY_NUM}"
Xvfb ":${DISPLAY_NUM}" \
    -screen 0 1280x800x24 \
    -nolisten tcp \
    +extension RANDR \
    >"$RUNTIME_DIR/xvfb.log" 2>&1 &
XVFB_PID=$!

DEADLINE=$(( $(date +%s) + DISPLAY_TIMEOUT_S ))
while [ ! -S "/tmp/.X11-unix/X${DISPLAY_NUM}" ]; do
    if ! kill -0 "$XVFB_PID" 2>/dev/null; then
        echo "crcbl horde threads e2e: Xvfb exited before creating its socket" >&2
        tail -n 40 "$RUNTIME_DIR/xvfb.log" >&2 || true
        exit 1
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "crcbl horde threads e2e: no X socket for :${DISPLAY_NUM} after ${DISPLAY_TIMEOUT_S}s" >&2
        tail -n 40 "$RUNTIME_DIR/xvfb.log" >&2 || true
        exit 1
    fi
    sleep 0.1
done
export DISPLAY=":${DISPLAY_NUM}"
export CRCBL_WEB_E2E_HEADED=1

GREEN="$RUNTIME_DIR/green.log"

echo "==> driving demos/horde on the threaded site"
set +e
node "$REPO/web/tools/horde-threads-e2e.mjs" "$THREADED_SITE" 2>&1 | tee "$GREEN"
STATUS=${PIPESTATUS[0]}
set -e

# CI sets `CARGO_TERM_COLOR: always`, and a coloured pipeline has broken this
# repository's count guards before. `$'\033'` and not `\x1b`: `\x` is a GNU sed
# extension and BSD sed reads that pattern as a literal `x1b[…`, matching
# nothing, silently. Same line, for the same reason, as `web/run-jobs-e2e.sh`.
sed -E $'s/\033\\[[0-9;]*[a-zA-Z]//g' "$GREEN" >"${GREEN}.plain"
GREEN="${GREEN}.plain"

if [ "$STATUS" -eq 2 ]; then
    echo "crcbl horde threads e2e: the gate could not run" >&2
    exit 2
fi

# The guard every harness here carries: a run that checked nothing must not be
# able to report success. The driver exits non-zero on its own in that case; this
# is the second lock, because the failure being guarded against is precisely
# "the thing that was supposed to notice did not".
RAN="$(grep -Eo '[0-9]+/[0-9]+ checks passed' "$GREEN" | tail -1 | grep -Eo '/[0-9]+' | tr -d '/' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl horde threads e2e: the driver reported no checks — the gate is not gating" >&2
    exit 1
fi

# Two assertions by name as well as by count. The criterion is the whole point of
# the script and everything else is a precondition for reading it; the isolation
# check is the one that would otherwise fail late and for a reason that says
# nothing about the headers.
for named in \
    'the document is cross-origin isolated' \
    "$CRITERION"; do
    if ! grep -qF "$named" "$GREEN"; then
        echo "crcbl horde threads e2e: the driver never checked '$named';" >&2
        echo "               the gate is smaller than it reports" >&2
        exit 1
    fi
done

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl horde threads e2e: $RAN checks ran and at least one failed" >&2
    exit "$STATUS"
fi

# ---------------------------------------------------------------------------
# The red checks
# ---------------------------------------------------------------------------

red_log=""
red_label=""

# Assert that a red run turned one named check red. Matched exactly: a check
# whose text moved on has to be updated here, which is the point.
expect_fail() {
    if ! grep -qF "FAIL $1" "$red_log"; then
        echo "crcbl horde threads e2e: $red_label did not turn this check red:" >&2
        echo "                 $1" >&2
        echo "               so that assertion is not gating what it claims to" >&2
        cat "$red_log" >&2
        exit 1
    fi
    echo "    broke:      $1"
}

# And that it left another one alone. Without this pair a run that failed for any
# reason at all would satisfy every red check, and the three of them would stop
# being three.
expect_pass() {
    if ! grep -qF "ok   $1" "$red_log"; then
        echo "crcbl horde threads e2e: $red_label was expected to leave this check alone:" >&2
        echo "                 $1" >&2
        echo "               it did not, so the red runs are not distinguishable" >&2
        cat "$red_log" >&2
        exit 1
    fi
    echo "    left alone: $1"
}

# `$1` a slug for the log file, `$2` what to call the run, `$3` the site, and the
# rest the driver's own arguments. The label is given rather than built from the
# arguments because red check C has none — the site *is* the switch.
red_run() {
    local slug="$1"
    local site="$3"
    red_label="$2"
    shift 3
    red_log="$RUNTIME_DIR/red-$slug.log"
    echo "==> red check: $red_label"
    local status=0
    node "$REPO/web/tools/horde-threads-e2e.mjs" "$site" \
        --timeout "$RED_TIMEOUT_MS" "$@" >"$red_log" 2>&1 || status=$?
    sed -E -i $'s/\033\\[[0-9;]*[a-zA-Z]//g' "$red_log"
    if [ "$status" -eq 0 ]; then
        echo "crcbl horde threads e2e: the run with $red_label PASSED." >&2
        echo "               A gate whose assertion cannot be made to fail is not a gate." >&2
        cat "$red_log" >&2
        exit 1
    fi
    if [ "$status" -ne 1 ]; then
        echo "crcbl horde threads e2e: the run with $red_label exited $status rather than failing" >&2
        echo "               its checks; it did not reach the assertions this is testing" >&2
        cat "$red_log" >&2
        exit 1
    fi
}

# A. No announcement, no threads. `Spawn::threaded()` stays false, every spawn is
# refused, the pool gets no workers and `par_for` runs every chunk on the page's
# own thread — the demo plays and steers exactly as it does on the published
# site, and the criterion is the only thing that notices.
red_run no-host-ready "?no-host-ready" "$THREADED_SITE" --query no-host-ready
expect_fail "$CRITERION"
expect_fail 'the page announced worker threads to the backend'
expect_pass 'the steering pass ran at all'
expect_pass 'the artifact imports a shared memory this page could give it'

# B. No crowd. Horde waits at its title screen, `run_tick` short-circuits before
# `steer_enemies`, and nothing is steered at all — so the criterion goes red for
# a reason that has nothing to do with threads, and the run says which.
red_run no-prefill "--prefill 0" "$THREADED_SITE" --prefill 0
expect_fail "$CRITERION"
expect_fail 'the steering pass ran at all'
expect_pass 'the page announced worker threads to the backend'

# C. The published site, driven through the same page and the same shim. This is
# the run the criterion exists to rule out — a demo that plays, steers a full
# crowd and never leaves the main thread — and it is also the standing proof
# that nothing here changed what a visitor gets: everything except the thread
# claims is green.
red_run published "the published site" "$SITE"
expect_fail "$CRITERION"
expect_fail 'the artifact imports a shared memory this page could give it'
expect_pass 'the steering pass ran at all'
expect_pass 'the demo booted a GPU device and started playing'

echo "crcbl horde threads e2e: $RAN checks ran in a real browser, and three red"
echo "crcbl horde threads e2e: checks broke the exit criterion for three different"
echo "crcbl horde threads e2e: reasons. horde's steering pass ran on a Web Worker;"
echo "crcbl horde threads e2e: the published artifact ran the same pass inline."
