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
#   It is also the only gate on **touch**. `Input.dispatchMouseEvent` is a mouse:
#   `pointerType` is "mouse", `isPrimary` is never false, no `pointercancel` is
#   ever raised, and the browser does not consult `touch-action` on the way. So
#   the shim's touch handling and the CSS that decides whether the browser hands
#   a gesture to the page at all are invisible to every check outside group F,
#   which turns touch emulation on and dispatches real contacts. The named guard
#   below insists that group ran.
#
#   It is also the only gate that reads **the sRGB encode off a picture**.
#   `web/run-probe-e2e.sh` covers both halves of the mechanism and covers them
#   well — its group I fails if the caps offer no sRGB format, its group X if the
#   canvas is configured or viewed without one — but every byte either of them
#   compares is copied out on the GPU and handed to wasm, and both drive
#   `crcbl-webgpu`'s probe exports on a page with no engine running. Neither ever
#   asks the browser what it composited, and neither is a demo. Group G here
#   reads a demo's own flat clear colour off the element a visitor looks at,
#   through `toDataURL`, and compares it against the byte an sRGB target holds —
#   a mid-range colour, because 0.0 and 1.0 encode to themselves and a check
#   against either cannot fail. It is the shape of the check the "frames come
#   back dark" bug got past, and the guard below insists it ran.
#
#   It is also the only gate on **cross-origin isolation**. `web/tools/serve.mjs`
#   sends COOP and COEP, `web/build.sh --serve` runs that same server, and the
#   driver asserts `crossOriginIsolated === true` inside the loaded document —
#   the precondition for `SharedArrayBuffer`, and therefore for any wasm build
#   with `+atomics`. Nothing else in the repository would notice those headers
#   going missing, so the named-check guard below insists that assertion ran.
#
#   And it is the only gate on **its own reporting channels**. Three of the
#   driver's checks assert that nothing was reported — no uncaught exception, no
#   missing asset, no WebGPU device error — and each of them passes just as
#   happily against a listener that was never attached, a filter that swallows
#   everything, or a server that stopped recording its 404s. Group H breaks all
#   three on purpose and asserts the break was seen, which is the shape
#   `crcbl-vk`'s `validation_gate` had to grow after a green suite turned out to
#   be reading a log sink nobody installed. The guard below insists it ran.
#
#   And it is the only gate on **what the WebGPU backend never let go of**.
#   `crcbl-vk` warns at device teardown for every object a caller never
#   destroyed, named by kind, and its e2e runners fail on that line;
#   `crcbl-dx12` and `crcbl-mtl` carry the same warning. `crcbl-webgpu` has no
#   device teardown on the Rust side to hang one on — it is a command stream —
#   but the browser side of it is a handle table per kind, and an object the
#   stream created and never destroyed is a slot still occupied. So the warning
#   is written there instead: `Replayer#replay` in `web/engine/gpu-replay.js`
#   emits the same line, in the same words, when the stream ends.
#
#   This script therefore gates it twice, and the two are not the same check.
#   The grep near the bottom fails the run on that line, exactly as
#   `run-vk-e2e.sh` does. Group I in the driver — the demo stopped through its
#   own button, then `Replayer#liveObjects` and `Replayer#teardownReport` asked
#   what is left and whether anything asked at all — is what stops that grep
#   from being a pattern that can never match: a clean run is silent and so is a
#   reporter that never fired. `web/tools/gpu-replay.mjs` covers the reading
#   itself against a stub, the counts following the tables down as well as up.
#   The guards below insist both halves ran.
#
#   It is **not** the gate on `crcbl-webgpu`'s command stream. Those are the
#   PROBE groups in `web/tools/probe-groups.mjs`, driven by
#   `web/run-probe-e2e.sh` against a page with no engine running on it. This
#   script is the demo gate, and the demo renders through `crcbl-webgpu` because
#   that is the only GPU backend a wasm build of the umbrella links.
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
#     --enable-features=Vulkan          the hardware mode **on Linux**: without
#       --use-angle=vulkan                them the GPU process falls back to
#                                       ANGLE SwiftShader GL and `chrome://gpu`
#                                       still says
#                                       `webgpu: unavailable_software`. macOS
#                                       and Windows want their own graphics API
#                                       here — `--use-angle=metal` and
#                                       `--use-angle=d3d11`; Chrome's Dawn has
#                                       no Vulkan backend on macOS at all, so
#                                       this pair asks it there for an adapter
#                                       that cannot exist. The branch is in
#                                       `web/tools/browser-launch.mjs`
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
else
    # **A REUSED SITE IS THE ONE WAY THIS HARNESS LIES.** Without `--build` the
    # run drives whatever `target/site` already holds, so an edit to
    # `web/engine/*.js` — or to the Rust the wasm is built from — is tested in
    # its previous form and the gate passes on code that is not the code under
    # test. That is not theoretical: it produced three green runs in a row for
    # edits deliberately made to fail, and on 2026-08-20 it did it again for a
    # Rust edit, reporting a frozen camera as "it took 2 values".
    #
    # A warning rather than a rebuild, because CI already runs `web/build.sh`
    # itself before calling this and would then pay for the build twice on every
    # demo.
    #
    # **The Rust half is checked here rather than left to cargo.** This comment
    # used to say "the wasm has `build.sh`'s own staleness handling", which is
    # true of `build.sh` and irrelevant in this branch: `build.sh` is exactly
    # what does not run here, so no `cargo` invocation ever compares a source
    # against the artifact. A `.rs` or `.slang` edit therefore produced no
    # warning at all — the silent half of the failure this block exists to make
    # loud.
    STAMP="$SITE/engine/demo.js"
    if [ -f "$STAMP" ]; then
        # `-newer` rather than a shell loop comparing timestamps: under `set -e`
        # a loop whose last comparison is false exits non-zero and takes the
        # whole run with it, silently, which is how this guard first shipped.
        # The parentheses are load-bearing — without them `-newer` binds to the
        # second `-name` alone and every `.js` file matches.
        #
        # `apps` and `crates` rather than the whole repository: `$REPO/target`
        # holds build output newer than the site by construction, and every
        # source the demos are built from is under one of those two.
        NEWER="$(find "$REPO/web/engine" "$REPO/web/tools" \
            \( -name '*.js' -o -name '*.mjs' \) -newer "$STAMP"
            find "$REPO/apps" "$REPO/crates" \
            \( -name '*.rs' -o -name '*.slang' -o -name 'Cargo.toml' \) \
            -newer "$STAMP")"
        if [ -n "$NEWER" ]; then
            echo "crcbl web e2e: WARNING — $SITE is older than these sources, so this run does not test them:" >&2
            # Indented with a read loop rather than `sed 's|^|  |'`, which is
            # what SC2001 asks for. `printf '  %s\n' "$NEWER"` is the obvious
            # rewrite and is **wrong**: it indents the first line only, because
            # the whole variable arrives as one argument. Checked against the
            # `sed` it replaces, on a multi-line list.
            while IFS= read -r stale; do
                printf '  %s\n' "$stale" >&2
            done <<<"$NEWER"
            echo "crcbl web e2e: re-run with --build (or run ./web/build.sh) before believing the result" >&2
        fi
    fi
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
# `$'\033'` and not `\x1b`: `\x` is a GNU sed extension, and BSD sed reads that
# pattern as a literal `x1b[…` — it matches nothing and strips nothing, silently.
# `web/run-probe-e2e.sh` carries the same line for the same reason.
sed -E $'s/\033\\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"${OUTPUT}.plain"

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

# The same argument for touch, and by name for the same reason. Every check
# outside group F passes on a page whose canvas has no `touch-action` at all and
# whose shim drops every contact — a mouse is not a finger and nothing else here
# sends one. This one drag is the check every demo makes, so its absence means
# the touch group did not run rather than that this demo has no bindings.
TOUCH="$(grep -F 'the canvas keeps a drag the browser would otherwise take' "${OUTPUT}.plain" || true)"
if [ -z "$TOUCH" ]; then
    echo "crcbl web e2e: the driver never dragged a finger across the canvas;" >&2
    echo "               'touch-action: none' in web/style.css is ungated" >&2
    exit 1
fi

# And the same argument for the sRGB encode, which is the one claim in the
# driver that a *silently absent* row would take away without failing anything.
# Every other check in the file passes on a canvas presenting a transfer function
# too dark — that is how the bug reached a visitor — so group G going missing
# leaves a green run and no encode gate anywhere on the demo path.
#
# The demo list is spelled out here as well as in the driver's `EXPECTATIONS`,
# and the duplication is the point: these three are the demos whose clear colour
# actually reaches the screen, so dropping `backdrop` from one of their rows has
# to fail somewhere rather than shrink the gate. Every other demo in
# `EXPECTATIONS` cannot make the claim at all, and the driver says why per row. Renaming the check in the driver
# is meant to fail here and be renamed here too.
case "$DEMO" in
    breakout | flappy | hud)
        ENCODE="$(grep -F 'clear reaches the canvas sRGB-encoded' "${OUTPUT}.plain" || true)"
        if [ -z "$ENCODE" ]; then
            echo "crcbl web e2e: the driver never compared $DEMO's clear colour against its sRGB encode;" >&2
            echo "               the canvas viewFormats in web/engine/gpu-replay.js are ungated on the demo path" >&2
            exit 1
        fi
        ;;
esac

# And the same argument for the viewer's drop target, which is the one thing on
# this page a visitor brings themselves. Every other check passes against a page
# that ignores a dropped file entirely — the demo document is compiled into the
# module and opens without anyone touching anything — so these four rows going
# missing leaves a green run and no gate at all on V-F5. `viewer` alone, because
# it is the only demo that takes a document.
#
# One name is matched rather than four, because they are one block in the driver
# and cannot go missing separately. Renaming it there is meant to fail here and
# be renamed here too.
case "$DEMO" in
    viewer)
        DROPPED="$(grep -F 'a dropped document replaces the one the page opened with' "${OUTPUT}.plain" || true)"
        if [ -z "$DROPPED" ]; then
            echo "crcbl web e2e: the driver never dropped a document onto the canvas;" >&2
            echo "               the drop target in web/demos/viewer/main.js is ungated" >&2
            exit 1
        fi
        ;;
esac

# And the same argument for the clip the viewer plays, which is the only gate
# anywhere on the browser path that `crcbl-anim` has a consumer at all. Every
# other check here passes against a page that converts no skin, samples no clip
# and composes no palette: the document still draws, the turntable still turns,
# and `crate::anim`'s own row going missing from the driver would leave a green
# run with nothing asking whether the animation in the file ever moved. `viewer`
# alone, because it is the only demo that opens a rigged document.
#
# The `moving` check next to it is *not* this claim and cannot stand in for it:
# it reads the turntable, which is the page's own camera and turns over a
# skeleton that was never posed. Renaming the check in the driver is meant to
# fail here and be renamed here too.
case "$DEMO" in
    viewer)
        PLAYED="$(grep -F 'the clip in the document plays under its own steam' "${OUTPUT}.plain" || true)"
        if [ -z "$PLAYED" ]; then
            echo "crcbl web e2e: the driver never asked whether the document's own clip is playing;" >&2
            echo "               the rig conversion and the clip player in apps/viewer/src/anim.rs are ungated" >&2
            exit 1
        fi
        ;;
esac

# And the strongest form of the same argument, for group H. Three of the driver's
# checks assert that *nothing* was reported — no uncaught exception, no missing
# asset, no WebGPU device error — and every one of them passes just as happily
# against a listener that was never attached. Group H breaks each channel on
# purpose and asserts the break was seen, so its absence is not a smaller gate:
# it is three checks going back to proving nothing while still printing `ok`.
# Every demo runs it, so unlike group G above there is no per-demo case here.
#
# One name is matched rather than three, because they are one block in the driver
# and cannot go missing separately. Renaming it there is meant to fail here and
# be renamed here too.
CHANNELS="$(grep -F 'a deliberate uncaught exception reaches the page-error channel' "${OUTPUT}.plain" || true)"
if [ -z "$CHANNELS" ]; then
    echo "crcbl web e2e: the driver never provoked its own reporting channels;" >&2
    echo "               the three silences it asserts are unproven, and a closed channel reports silence too" >&2
    exit 1
fi

# And for group I, which is the only gate anywhere on what `crcbl-webgpu` never
# let go of. `crcbl-vk` warns at device teardown for every object a caller never
# destroyed and its e2e runners fail on that line; `crcbl-dx12` and `crcbl-mtl`
# carry the same warning. The browser side has no device teardown to hang one
# on, so this driver reads the replayer's handle tables after stopping the demo
# instead — and a group that went missing would take the whole claim with it
# while every other check here went on passing, because nothing else in the
# repository looks at those tables at all. Every demo runs it. Renaming the
# check in the driver is meant to fail here and be renamed here too.
RELEASED="$(grep -F 'the demo destroyed every GPU object it created' "${OUTPUT}.plain" || true)"
if [ -z "$RELEASED" ]; then
    echo "crcbl web e2e: the driver never asked what the replayer was still holding after shutdown;" >&2
    echo "               the handle tables in web/engine/gpu-replay.js are ungated" >&2
    exit 1
fi

# And the named guard for the half of group I that is about the *engine*
# reporting for itself rather than about this harness reading its tables. The
# grep below is the gate proper; this is what makes the grep worth having.
#
# A clean run writes no teardown line, and so does a run whose reporter never
# fired at all — a `WebGpuDevice::drop` that did not park the channel, a shim
# that stopped pumping, a last frame that threw. The driver's check reads
# `Replayer#teardownReport`, which is `null` in the second case and an empty
# list in the first, so it is the one thing that can tell them apart. Without it
# the grep below is a green light wired to nothing. Renaming the check in the
# driver is meant to fail here and be renamed here too.
REPORTED="$(grep -F 'the engine reported for itself that its command stream ended' "${OUTPUT}.plain" || true)"
if [ -z "$REPORTED" ]; then
    echo "crcbl web e2e: the driver never asked whether the command stream reported its own end;" >&2
    echo "               the teardown warning in web/engine/gpu-replay.js is ungated, and the" >&2
    echo "               leak grep below cannot tell a clean run from a reporter that never ran" >&2
    exit 1
fi

# **The teardown leak report, read rather than left in the log**, spelled the
# way `crates/crcbl-vk/tests/run-vk-e2e.sh` spells it and matching the same
# literal. `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl` each warn from their device's
# destructor for every object a caller never destroyed; `crcbl-webgpu` has no
# device teardown on the Rust side, so `Replayer#replay` warns from the browser
# when the command stream ends — and a warning fails nothing, which is how the
# four leaks the vk line found the afternoon it landed were found by a person
# reading a job log.
#
# The lines come from the page's own console, which `web/tools/browser-e2e.mjs`
# records and echoes onto its output above the page log it writes; they are not
# this script's reading of anything.
#
# The expectation is zero lines, not a judgement call: a demo that stops through
# its own button has torn down everything it built, so a line here names an
# object the engine or the game stopped destroying.
LEAKS="$(grep -F 'object(s) still alive at device teardown' "${OUTPUT}.plain" || true)"
if [ -n "$LEAKS" ]; then
    echo "crcbl web e2e: the command stream ended with objects still alive:" >&2
    while IFS= read -r line; do
        echo "                $line" >&2
    done <<<"$LEAKS"
    echo "              The engine's own teardown reporter wrote that, from" >&2
    echo "              web/engine/gpu-replay.js. The kinds above are property" >&2
    echo "              names on the replayer, so crcbl.gpu.replayer.<kind> in a" >&2
    echo "              console is the next step. Destroy them where they were" >&2
    echo "              created rather than leaving the line in the log for" >&2
    echo "              somebody to notice." >&2
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
# every run: `apps/hud` takes no input at all and `apps/lantern` has no run to
# begin, and both say so in their `EXPECTATIONS` row, so the driver dispatches no
# key for either. What every demo does do is the three
# claims left here, and the per-check lines above name the key where there was
# one.
echo "crcbl web e2e: $DEMO booted, opened a WebGPU device, and drew moving frames"
echo "crcbl web e2e: its command stream ended with nothing left alive"
echo "crcbl web e2e:${ISOLATION#*ok  }"
