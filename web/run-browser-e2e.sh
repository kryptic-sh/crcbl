#!/usr/bin/env bash
# Load the built demo site in a real browser and check that a demo renders.
#
# CRCBL_WEB_E2E_DEMO picks which one; `breakout` by default. One demo per run —
# the driver launches a browser and reads one canvas — so CI runs this script
# once per demo and a failure names the game.
#
#   ./web/run-browser-e2e.sh [--build] [--headless] [--hardware] [driver args…]
#
# The same shape as `crates/crcbl-vk/tests/run-vk-e2e.sh` and
# `crates/crcbl-shell/tests/run-x11-e2e.sh`: it brings its own environment up,
# says what it needs and why, prints what it actually checked, and **fails when
# zero checks ran** — `docs/plan/12-testing.md` names a silently-skipped e2e job
# as a known trap and this is the guard against it.
#
# WHAT THIS IS THE ONLY GATE FOR
#   `web/tools/check-exports.mjs` proves the wasm artifact's symbols line up
#   with what the shim calls. `web/tools/smoke.mjs` instantiates that artifact
#   under Node with every import stubbed and drives the documented boot order.
#   Neither can see a GPU, so both stop one call short of the P5 gate: a browser
#   that loads the page, opens a WebGPU device, and puts pixels on a canvas.
#
#   This script is that last step, and it reads the canvas back rather than
#   trusting a status code — a black canvas satisfies every other check in the
#   repository.
#
#   It is also the only gate on **cross-origin isolation**. `web/tools/serve.mjs`
#   sends COOP and COEP, `web/build.sh --serve` runs that same server, and the
#   driver asserts `crossOriginIsolated === true` inside the loaded document —
#   the precondition for `SharedArrayBuffer`, and therefore for any wasm build
#   with `+atomics`. Nothing else in the repository would notice those headers
#   going missing, so the named-check guard below insists that assertion ran.
#
# WHAT IT NEEDS
#   * **A Chromium or Chrome with WebGPU.** `CRCBL_CHROMIUM` pins one;
#     otherwise `google-chrome`, `google-chrome-stable`, `chromium` and
#     `chromium-browser` are tried in that order and a miss is a hard failure
#     with the list printed. GitHub's `ubuntu-latest` image ships
#     `/usr/bin/google-chrome`; that is the assumption this makes in CI, and it
#     fails loudly rather than skipping if the assumption stops holding.
#   * **Node 22 or newer**, for the global `WebSocket` the DevTools client uses.
#     There is no npm step and no `node_modules` — the shim's no-npm policy
#     applies to its tests too.
#   * **Xvfb**, if there is one. See below: it is what makes this work on a
#     machine with no GPU, which is every CI runner.
#   * **No GPU.** Nice to have, not needed.
#
# WHY Xvfb AND NOT `--headless`
#   Chromium's WebGPU canvas can be read back in some configurations and not
#   others, and the difference is not documented anywhere. Measured on Chromium
#   151 with a page that does nothing but clear a canvas to a known colour:
#
#     display   adapter        canvas.toDataURL()
#     --------  -------------  ---------------------------------------
#     headless  hardware       the colour it drew
#     headless  SwiftShader    transparent black — *silently*
#     Xvfb      SwiftShader    the colour it drew
#     Xvfb      hardware       transparent black — *silently*; the WebGPU
#                              device is lost part-way through the run
#
#   A CI runner has no GPU, so headless plus SwiftShader is the box CI would
#   land in, and it is the one that reports a perfectly rendered frame as blank.
#   Running inside Xvfb moves it to the row that works, which is why this script
#   starts one; `--headless` skips it for a machine that has no Xvfb or where
#   the hardware path is what is wanted.
#
#   The shape of that table survived Chromium 151; the SwiftShader row that
#   works did not survive on its own. It read transparent black there too until
#   `browser-e2e.mjs` learned to put Chromium's *shared image* device on
#   SwiftShader alongside WebGPU's, which is where the reasoning lives.
#
#   None of that is trusted rather than checked: `browser-e2e.mjs` runs the
#   known-colour clear first, in the same browser with the same flags, and
#   refuses to interpret the render checks unless it comes back with the colour
#   it drew. It also tries both adapters and keeps whichever one passes, so the
#   table above is a description of what happens rather than a configuration
#   anyone has to get right.
#
# THE CHROMIUM FLAGS
#   Measured, not copied. Each is commented at its use in `browser-e2e.mjs`; the
#   short version:
#
#     --headless=new                    old headless has no GPU stack at all,
#                                       so `navigator.gpu` is simply absent
#     --enable-unsafe-webgpu            Chrome refuses WebGPU when the GPU
#       --use-webgpu-adapter=swiftshader  feature status is
#                                       `unavailable_software`, which is what a
#                                       machine with no GPU reports; this pair
#                                       lifts the refusal and asks for
#                                       Chromium's bundled software Vulkan
#     --enable-features=Vulkan          the hardware mode: without them the GPU
#       --use-angle=vulkan                process falls back to ANGLE
#                                       SwiftShader GL and `chrome://gpu` still
#                                       says `webgpu: unavailable_software`
#     --enable-features=Vulkan          the software mode's other half: the
#       --use-vulkan=swiftshader          device Chromium hands canvases around
#                                       on has to be the same one WebGPU renders
#                                       with, or the compositor cannot read the
#                                       canvas back and `toDataURL` returns
#                                       uninitialised memory
#     XDG_CONFIG_HOME=<throwaway>       not a flag: some distributions' launcher
#                                       appends `~/.config/chromium-flags.conf`
#                                       to the command line. One containing
#                                       `--ozone-platform=wayland` takes the GPU
#                                       process down in a headless run and hides
#                                       WebGPU entirely, which looks exactly
#                                       like a browser without WebGPU support
#
# ENVIRONMENT
#   CRCBL_CHROMIUM             Path to the browser binary.
#   CRCBL_CHROMIUM_NO_SANDBOX  `1` adds `--no-sandbox`. Needed where user
#                              namespaces are unavailable; added automatically
#                              when running as root.
#   CRCBL_CHROMIUM_FLAGS       Extra flags, space separated. The escape hatch
#                              for a runner nobody here has.
#   CRCBL_WEB_E2E_ADAPTER      `auto` (default), `hardware` or `swiftshader`.
#   CRCBL_WEB_E2E_TIMEOUT_MS   How long start-up is given. SwiftShader is slow.
#   SITE_DIR                   Where the built site is. Default `target/site`.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="${SITE_DIR:-$REPO/target/site}"
BUILD=0
HEADLESS=0
SCREEN="${CRCBL_WEB_E2E_SCREEN:-1280x800x24}"
DISPLAY_TIMEOUT_S="${CRCBL_WEB_E2E_DISPLAY_TIMEOUT_S:-20}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --build)
            BUILD=1
            shift
            ;;
        --headless)
            HEADLESS=1
            shift
            ;;
        --hardware)
            export CRCBL_WEB_E2E_ADAPTER=hardware
            shift
            ;;
        *)
            break
            ;;
    esac
done

# Node's global `WebSocket` landed in 22. Without it the DevTools client fails
# with `WebSocket is not defined`, which is a confusing way to learn that the
# runtime is too old.
if ! command -v node >/dev/null 2>&1; then
    echo "crcbl web e2e: node not found; this harness needs Node 22 or newer" >&2
    exit 1
fi
NODE_MAJOR="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
if [ "$NODE_MAJOR" -lt 22 ]; then
    echo "crcbl web e2e: node $(node --version) is too old; the DevTools client needs the global WebSocket from Node 22" >&2
    exit 1
fi

if [ "$BUILD" = "1" ] || [ ! -d "$SITE" ]; then
    echo "crcbl web e2e: building the site into $SITE"
    SITE_DIR="$SITE" "$REPO/web/build.sh"
fi

# Which demo this run drives. One at a time, because the driver launches its own
# browser and reads one canvas; the loop over every demo is the caller's, and CI
# runs the script once per demo so a failure names the game.
DEMO="${CRCBL_WEB_E2E_DEMO:-breakout}"
if [ ! -f "$SITE/demos/$DEMO/index.html" ]; then
    echo "crcbl web e2e: $SITE has no $DEMO demo; run ./web/build.sh" >&2
    exit 1
fi

RUNTIME_DIR="$(mktemp -d -t crcbl-web-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
XVFB_LOG="${RUNTIME_DIR}/xvfb.log"
OUTPUT="${RUNTIME_DIR}/driver.log"

cleanup() {
    status=$?
    if [ -n "${XVFB_PID:-}" ]; then
        kill "$XVFB_PID" 2>/dev/null || true
        wait "$XVFB_PID" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

# Inherit nothing from an outer session. A developer running this on a live
# desktop must not have a browser window appear on their screen, and a stale
# `WAYLAND_DISPLAY` would send Chromium looking for a compositor that is not
# the one this script started.
unset WAYLAND_DISPLAY
unset DISPLAY
unset XAUTHORITY

if [ "$HEADLESS" = "0" ] && ! command -v Xvfb >/dev/null 2>&1; then
    echo "crcbl web e2e: Xvfb is not installed; falling back to --headless."
    echo "               On a machine with no GPU that combination cannot read the"
    echo "               canvas back, and the run will say so rather than pass."
    HEADLESS=1
fi

if [ "$HEADLESS" = "0" ]; then
    # Claim a display number nobody is using, the same way `run-x11-e2e.sh`
    # does: check for the lock *and* the socket, then let the readiness poll
    # below catch a race with whoever claimed it first.
    DISPLAY_NUM=""
    for candidate in $(seq 90 120); do
        if [ ! -e "/tmp/.X${candidate}-lock" ] && [ ! -e "/tmp/.X11-unix/X${candidate}" ]; then
            DISPLAY_NUM="$candidate"
            break
        fi
    done
    if [ -z "$DISPLAY_NUM" ]; then
        echo "crcbl web e2e: no free X display number in :90-:120" >&2
        exit 1
    fi

    echo "crcbl web e2e: starting Xvfb on :${DISPLAY_NUM} (${SCREEN})"
    # `-nolisten tcp` keeps the server on its Unix socket. RANDR is what
    # Chromium queries for the display's geometry and refresh rate; without it
    # the browser starts but `requestAnimationFrame` has no rate to lock to.
    Xvfb ":${DISPLAY_NUM}" \
        -screen 0 "$SCREEN" \
        -nolisten tcp \
        +extension RANDR \
        >"$XVFB_LOG" 2>&1 &
    XVFB_PID=$!

    # Poll for readiness against a deadline, with a liveness check on the
    # child. Never a fixed sleep: one long enough for the slowest runner wastes
    # time on every other one, and one short enough to be cheap is a flake.
    DEADLINE=$(( $(date +%s) + DISPLAY_TIMEOUT_S ))
    while [ ! -S "/tmp/.X11-unix/X${DISPLAY_NUM}" ]; do
        if ! kill -0 "$XVFB_PID" 2>/dev/null; then
            echo "crcbl web e2e: Xvfb exited before creating its socket" >&2
            tail -n 40 "$XVFB_LOG" >&2 || true
            exit 1
        fi
        if [ "$(date +%s)" -ge "$DEADLINE" ]; then
            echo "crcbl web e2e: no X socket for :${DISPLAY_NUM} after ${DISPLAY_TIMEOUT_S}s" >&2
            tail -n 40 "$XVFB_LOG" >&2 || true
            exit 1
        fi
        sleep 0.1
    done

    export DISPLAY=":${DISPLAY_NUM}"
    export CRCBL_WEB_E2E_HEADED=1
    echo "crcbl web e2e: display up at ${DISPLAY} (Xvfb pid ${XVFB_PID})"
else
    echo "crcbl web e2e: no display; the browser runs --headless=new"
fi

set +e
node "$REPO/web/tools/browser-e2e.mjs" --site "$SITE" --demo "demos/$DEMO/" "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

# CI sets `CARGO_TERM_COLOR: always`, and a coloured pipeline has broken this
# repository's test-count guards before, so strip escapes before matching. This
# harness does not colour its own output, but a browser or a Node warning might.
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"${OUTPUT}.plain"

# The guard the other harnesses have, spelled the same way: a run that checked
# nothing must not be able to report success. The driver exits non-zero on its
# own in that case; this is the second lock, because the failure being guarded
# against is precisely "the thing that was supposed to notice did not".
RAN="$(grep -Eo '[0-9]+/[0-9]+ checks passed' "${OUTPUT}.plain" | tail -1 | grep -Eo '/[0-9]+' | tr -d '/' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl web e2e: the driver reported no checks — the gate is not gating" >&2
    exit 1
fi

# The isolation assertion by name, not just by count — beside the count guard
# rather than after the verdict, because both are about the harness rather than
# about the engine, and a harness that stopped gating is worth saying whichever
# way the run went. Every other check here is about the engine and would still
# run, and still pass, on an origin with no COOP/COEP at all, so "some checks
# ran" is not evidence that this one did. Renaming the check in the driver is
# meant to fail here and be renamed here too.
ISOLATION="$(grep -F 'the document is cross-origin isolated' "${OUTPUT}.plain" || true)"
if [ -z "$ISOLATION" ]; then
    echo "crcbl web e2e: the driver never asked whether the origin is cross-origin isolated;" >&2
    echo "               the COOP/COEP headers in web/tools/serve.mjs are ungated" >&2
    exit 1
fi

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl web e2e: $RAN checks ran and at least one failed" >&2
    echo "crcbl web e2e: the canvas and the page log are in target/web-e2e/" >&2
    exit "$STATUS"
fi

# Which configuration actually served, from the driver rather than from what was
# asked for. A run that fell back to the other adapter, or that never left the
# boot checks, shows up here and nowhere else.
CONFIG="$(grep -Eo 'running against the ".*" adapter — .*' "${OUTPUT}.plain" | head -1 || true)"
if [ -z "$CONFIG" ]; then
    echo "crcbl web e2e: the driver never named the adapter it used" >&2
    exit 1
fi
echo "crcbl web e2e: $RAN checks ran in a real browser, ${CONFIG#running against the }"
# The key event is not part of this sentence any more, because it is not part of
# every run: `apps/hud` takes no input at all and its `EXPECTATIONS` row says so,
# so the driver dispatches no key for it. What every demo does do is the three
# claims left here, and the per-check lines above name the key where there was
# one.
echo "crcbl web e2e: $DEMO booted, opened a WebGPU device, and drew moving frames"
echo "crcbl web e2e:${ISOLATION#*ok  }"
