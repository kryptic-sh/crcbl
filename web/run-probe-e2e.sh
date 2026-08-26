#!/usr/bin/env bash
# Gate `crcbl-webgpu`'s HAL seam in a real browser, against a real `GPUDevice`.
#
#   ./web/run-probe-e2e.sh [--build] [--headless] [driver args…]
#
# Anything it does not recognise is forwarded to `web/tools/probe-e2e.mjs`
# untouched — `--adapter`, `--timeout` and `--expect-fail` are all the driver's.
#
# The same shape as `web/run-browser-e2e.sh`: it brings its own environment up,
# says what it needs and why, prints what it actually checked, and **fails when
# zero checks ran** — `docs/plan/12-testing.md` names a silently-skipped e2e job
# as a known trap and this is the guard against it.
#
# WHAT THIS IS THE ONLY GATE FOR. The seam groups in
# `web/tools/probe-groups.mjs` drive `crcbl-webgpu`'s command stream directly
# rather than through the engine: the wasm→JS→wasm round trip and the device it
# opens, a surface, the capability query held against what `navigator.gpu` tells
# the same page, then a buffer, an image and its view, a sampler, a bind-group
# layout, a bind group, a shader module, a pipeline layout and both pipelines —
# and from S onward the ones that read bytes back and compare their *values*: a
# cleared texture, a drawn triangle, a compute shader's storage writes, a copy
# chain, a sub-range fill, a presented canvas frame, a reconfigured swapchain, an
# indirect draw, a dispatch that reads its workgroup counts out of a buffer, and
# a triangle past the far plane that one pipeline clamps and the pipeline beside
# it clips, two indirect draws whose argument structures differ only in
# `firstInstance` landing a half-target apart, and a fullscreen quad whose
# fragment shader samples a four-texel texture. `web/run-browser-e2e.sh` proves
# the demos render through this
# backend; nothing but this proves the seam underneath them command by command.
#
# WHY IT IS A SEPARATE PAGE. The probe exports install their own command-stream
# channel and a page has exactly one, so they run only where nothing else has
# claimed it. `web/probe/` is a page with **no engine running**: it loads a
# demo's wasm module without booting it and pumps the channel itself. The groups
# used to need a site built on `wgpu` instead — `crcbl-webgpu` linked but idle
# under a demo rendering through something else — which tied twenty groups of
# seam coverage to a second backend being present.
#
# WHAT IT NEEDS
#   * A built site with `probe/index.html` in it — `web/build.sh` puts it there,
#     and `--build` runs it. Every wasm build links `crcbl-webgpu` — it is the
#     only GPU backend the umbrella names on `wasm32` — so every artifact
#     carries the probe exports; the shipped one is what is under test.
#   * Node 22+, for the global `WebSocket` the DevTools client uses.
#   * A Chromium/Chrome with WebGPU. `CRCBL_CHROMIUM` pins one.
#   * Xvfb, unless `--headless`. **It is not optional on a machine with no GPU**:
#     under `--headless=new` plus SwiftShader group X — the first that presents a
#     canvas frame — never resolves its readback map, and Y, Z and AA never
#     resolve theirs either, measured on Chromium 151, while all four pass in the
#     same browser inside Xvfb. A headless run on such a machine therefore fails
#     loudly rather than quietly skipping, which is the point.
#
#     Z AND AA ARE COLLATERAL, not canvas groups: Z draws indirectly into a
#     texture the replayer owns and AA copies a depth plane, and neither touches a
#     canvas. The page has one `GPUDevice` and therefore one queue, and a
#     `mapAsync` resolves only behind the submits ahead of it — so the stuck
#     present in X strands every readback queued after it. Stubbing X and Y out so
#     they submit nothing makes Z and AA pass in that same headless browser, which
#     is how that was established rather than assumed.
#
#     XVFB PLUS A HARDWARE ADAPTER IS THE OTHER COMBINATION THAT CANNOT SERVE
#     GROUP X, and it fails the opposite way round: the readback resolves, and
#     what it hands back is transparent black, alongside a device error reading
#     "[Invalid Texture] is invalid due to a previous error". Every other group,
#     including AH, passes exactly as it does on SwiftShader — 86/87 on this
#     repo's own RX 7900 XTX, against 87/87 on SwiftShader. `web/run-browser-e2e.sh`'s header
#     has carried that row for the demo harness all along — the table was in one
#     harness and the failure in the other, so anyone running this gate on
#     hardware for the first time met a red group and no explanation anywhere
#     they were looking. The run now says it when it sees the combination.
#
# WHAT A PLATFORM CANNOT SERVE, when it comes to that. `--expect-fail X,Y,Z,AA`
# tells the driver those groups produce no verdict here. They still run, still
# print and still count; only the verdict changes — and it changes in both
# directions, because a named group that *passes* fails the run as a stale list.
# `web/tools/probe-e2e.mjs`'s EXIT STATUS carries the rest. The guards below are
# untouched by it: every letter must still appear and the run must still have
# checked something, so the list cannot empty a run and call it a pass.
#
# THE OTHER TWO PLATFORMS, and why `--headless` means something different on each.
# Xvfb is an X server, so it exists on neither.
#
#   macOS    `--headless` is the whole answer: a headless Chrome there can read a
#            WebGPU canvas back, and `macos-15` runners have a real Metal device
#            (an Apple Paravirtual one). `macos-14` must not be used — its
#            `MTLCreateSystemDefaultDevice()` returns nil, which `ci.yml`
#            records.
#   Windows  `--headless` *plus* `CRCBL_WEB_E2E_HEADED=1`, which reads as a
#            contradiction and is not: the flag tells **this script** not to go
#            looking for an Xvfb it cannot have, and the variable tells the
#            **driver** not to pass `--headless=new`. A Windows runner has its
#            own desktop session, and it needs it — a headless Chrome there
#            renders the canvas but never gets it to a compositor that can hand
#            the pixels back, the same phenomenon as the Linux row above.
#
# ENVIRONMENT
#   SITE_DIR                   Where the built site is. Default `target/site`.
#   CRCBL_CHROMIUM             Path to the browser binary.
#   CRCBL_WEB_E2E_ADAPTER      `auto` (default), `hardware` or `swiftshader`;
#                              `--adapter <mode>` is the same switch. `auto`
#                              resolves by platform — SwiftShader on Linux, the
#                              real device on macOS and Windows, both of which
#                              have no SwiftShader path that works. The reasoning
#                              is in `web/tools/probe-e2e.mjs`.
#   CRCBL_CHROMIUM_FLAGS       Extra flags, space separated. It only appends, so
#                              it cannot turn a GPU flag off; the adapter switch
#                              above is what picks between the sets.
#   CRCBL_CHROMIUM_NO_SANDBOX  `1` adds --no-sandbox.
#   CRCBL_WEB_E2E_HEADED       `1` drops `--headless=new` from the browser.
#   CRCBL_WEB_E2E_TIMEOUT_MS   How long each poll is given. SwiftShader is slow.
#   CRCBL_WEB_E2E_EXPECT_FAIL  Group letters that produce no verdict on this
#                              platform, comma- or space-separated;
#                              `--expect-fail X,Y,Z,AA` is the same switch.
#   CRCBL_WEB_E2E_SCREEN       Xvfb geometry. Default 1280x800x24.
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
        *)
            break
            ;;
    esac
done

# Node's global `WebSocket` landed in 22. Without it the DevTools client fails
# with `WebSocket is not defined`, which is a confusing way to learn that the
# runtime is too old.
if ! command -v node > /dev/null 2>&1; then
    echo "crcbl probe e2e: node not found; this harness needs Node 22 or newer" >&2
    exit 1
fi
NODE_MAJOR="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
if [ "$NODE_MAJOR" -lt 22 ]; then
    echo "crcbl probe e2e: node $(node --version) is too old; the DevTools client needs the global WebSocket from Node 22" >&2
    exit 1
fi

if [ "$BUILD" = "1" ] || [ ! -d "$SITE" ]; then
    echo "crcbl probe e2e: building the site into $SITE"
    SITE_DIR="$SITE" "$REPO/web/build.sh"
else
    # **A REUSED SITE IS THE ONE WAY THIS HARNESS LIES**, for the reason
    # `web/run-browser-e2e.sh` spells out: without `--build` the run drives
    # whatever `$SITE` already holds, so an edit to the page or to the engine
    # modules is tested in its previous form. A warning rather than a rebuild,
    # because CI runs `web/build.sh` itself before calling this.
    #
    # **THE RUST SIDE COUNTS TOO, AND USED TO BE MISSED.** Half of what this
    # harness drives is `crcbl-webgpu` compiled into the probe's wasm, and this
    # check watched only the JS — so editing `crates/crcbl-webgpu/src/probe.rs`
    # and re-running produced no warning and silently tested the previous wasm.
    # Found 2026-08-22 by a red check that flipped `depth_clamp` to `false` in
    # `probe_clamp_clamped_pipeline_desc` and still saw the run pass: exactly
    # the shape this warning exists to prevent, arriving through the input it
    # did not cover. `.rs` is matched as well as `.js` now, over the crates the
    # probe's wasm is built from.
    STAMP="$SITE/probe/main.js"
    if [ -f "$STAMP" ]; then
        NEWER="$(find "$REPO/web/engine" "$REPO/web/probe" \
            "$REPO/crates/crcbl-webgpu/src" "$REPO/crates/crcbl-shell/src" \
            "$REPO/crates/crcbl-core/src" "$REPO/crates/crcbl-hal/src" \
            \( -name '*.js' -o -name '*.rs' \) -newer "$STAMP")"
        if [ -n "$NEWER" ]; then
            echo "crcbl probe e2e: WARNING — $SITE is older than these sources, so this run does not test them:" >&2
            # Indented with a read loop rather than `sed 's|^|  |'`, which is
            # what SC2001 asks for. `printf '  %s\n' "$NEWER"` is the obvious
            # rewrite and is **wrong**: it indents the first line only, because
            # the whole variable arrives as one argument. Checked against the
            # `sed` it replaces, on a multi-line list.
            while IFS= read -r stale; do
                printf '  %s\n' "$stale" >&2
            done <<<"$NEWER"
            echo "crcbl probe e2e: re-run with --build (or run ./web/build.sh) before believing the result" >&2
        fi
    fi
fi

if [ ! -f "$SITE/probe/index.html" ]; then
    echo "crcbl probe e2e: $SITE has no probe page; run ./web/build.sh" >&2
    exit 1
fi

RUNTIME_DIR="$(mktemp -d -t crcbl-probe-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
XVFB_LOG="${RUNTIME_DIR}/xvfb.log"
OUTPUT="${RUNTIME_DIR}/driver.log"

cleanup() {
    status=$?
    if [ -n "${XVFB_PID:-}" ]; then
        kill "$XVFB_PID" 2> /dev/null || true
        wait "$XVFB_PID" 2> /dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

# Inherit nothing from an outer session. A developer running this on a live
# desktop must not have a browser window appear on their screen, and a stale
# `WAYLAND_DISPLAY` would send Chromium looking for a compositor that is not the
# one this script started.
unset WAYLAND_DISPLAY
unset DISPLAY
unset XAUTHORITY

if [ "$HEADLESS" = "0" ] && ! command -v Xvfb > /dev/null 2>&1; then
    echo "crcbl probe e2e: Xvfb is not installed; falling back to --headless."
    echo "                 On a machine with no GPU that combination cannot resolve the"
    echo "                 canvas-frame readback in group X, and Y, Z and AA are queued"
    echo "                 behind it, so the run will say so rather than pass."
    HEADLESS=1
fi

if [ "$HEADLESS" = "0" ]; then
    # Claim a display number nobody is using, the same way
    # `web/run-browser-e2e.sh` does: check for the lock *and* the socket, then
    # let the readiness poll below catch a race with whoever claimed it first.
    DISPLAY_NUM=""
    for candidate in $(seq 90 120); do
        if [ ! -e "/tmp/.X${candidate}-lock" ] && [ ! -e "/tmp/.X11-unix/X${candidate}" ]; then
            DISPLAY_NUM="$candidate"
            break
        fi
    done
    if [ -z "$DISPLAY_NUM" ]; then
        echo "crcbl probe e2e: no free X display number in :90-:120" >&2
        exit 1
    fi

    echo "crcbl probe e2e: starting Xvfb on :${DISPLAY_NUM} (${SCREEN})"
    # `-nolisten tcp` keeps the server on its Unix socket. RANDR is what Chromium
    # queries for the display's geometry and refresh rate; without it the browser
    # starts but `requestAnimationFrame` has no rate to lock to — and this page
    # pumps the command stream on rAF, so that is the loop under test.
    Xvfb ":${DISPLAY_NUM}" \
        -screen 0 "$SCREEN" \
        -nolisten tcp \
        +extension RANDR \
        > "$XVFB_LOG" 2>&1 &
    XVFB_PID=$!

    # Poll for readiness against a deadline, with a liveness check on the child.
    # Never a fixed sleep: one long enough for the slowest runner wastes time on
    # every other one, and one short enough to be cheap is a flake.
    DEADLINE=$(($(date +%s) + DISPLAY_TIMEOUT_S))
    while [ ! -S "/tmp/.X11-unix/X${DISPLAY_NUM}" ]; do
        if ! kill -0 "$XVFB_PID" 2> /dev/null; then
            echo "crcbl probe e2e: Xvfb exited before creating its socket" >&2
            tail -n 40 "$XVFB_LOG" >&2 || true
            exit 1
        fi
        if [ "$(date +%s)" -ge "$DEADLINE" ]; then
            echo "crcbl probe e2e: no X socket for :${DISPLAY_NUM} after ${DISPLAY_TIMEOUT_S}s" >&2
            tail -n 40 "$XVFB_LOG" >&2 || true
            exit 1
        fi
        sleep 0.1
    done

    export DISPLAY=":${DISPLAY_NUM}"
    export CRCBL_WEB_E2E_HEADED=1
    echo "crcbl probe e2e: display up at ${DISPLAY} (Xvfb pid ${XVFB_PID})"
elif [ "${CRCBL_WEB_E2E_HEADED:-0}" = "1" ]; then
    # The Windows shape: no Xvfb to bring up, and no `--headless=new` either,
    # because the runner's own desktop session is what the canvas reaches a
    # compositor through. Said out loud rather than left implicit — "no display"
    # would be the wrong sentence and the wrong thing to debug from.
    echo "crcbl probe e2e: no Xvfb; CRCBL_WEB_E2E_HEADED=1, so the browser runs on this session's desktop"
else
    echo "crcbl probe e2e: no display; the browser runs --headless=new"
fi

# The adapter the driver will resolve to, read the way the driver reads it: a
# forwarded `--adapter` beats the environment. Only used to say something true
# about this combination — the switch itself stays the driver's.
ADAPTER_MODE="${CRCBL_WEB_E2E_ADAPTER:-auto}"
PREV_ARG=""
for arg in "$@"; do
    case "$arg" in
        --adapter=*) ADAPTER_MODE="${arg#--adapter=}" ;;
    esac
    if [ "$PREV_ARG" = "--adapter" ]; then
        ADAPTER_MODE="$arg"
    fi
    PREV_ARG="$arg"
done

# See the header: inside Xvfb a hardware adapter presents a canvas nobody can
# read back as anything but transparent black, so group X fails and the rest of
# the run is unaffected. Said before the run rather than after it, because the
# reader's question arrives the moment the group goes red.
XVFB_HARDWARE=0
if [ -n "${XVFB_PID:-}" ] && [ "$ADAPTER_MODE" = "hardware" ]; then
    XVFB_HARDWARE=1
    echo "crcbl probe e2e: --adapter hardware inside Xvfb — group X, the presented canvas"
    echo "                 frame, reads transparent black on this combination and fails."
    echo "                 Nothing else is affected; web/run-browser-e2e.sh's header has the"
    echo "                 same row for the demo harness. Pass --expect-fail X to turn that"
    echo "                 into a verdict, which also fails the run if it ever stops failing."
fi

set +e
node "$REPO/web/tools/probe-e2e.mjs" --site "$SITE" "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

# CI sets `CARGO_TERM_COLOR: always`, and a coloured pipeline has broken this
# repository's test-count guards before, so strip escapes before matching.
#
# The escape is spelled `$'\033'` and not `\x1b`, because `\x` is a GNU sed
# extension: BSD sed reads that pattern as a literal `x1b[…`, matches nothing,
# and strips nothing — silently, and only on macOS, where this gate now runs.
# `$'…'` is bash's own escape, so the byte reaches both seds already expanded.
sed -E $'s/\033\\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" > "${OUTPUT}.plain"

# The guard every harness here has: a run that checked nothing must not be able
# to report success. The driver exits non-zero on its own in that case; this is
# the second lock, because the failure being guarded against is precisely "the
# thing that was supposed to notice did not".
RAN="$(grep -Eo '[0-9]+/[0-9]+ checks passed' "${OUTPUT}.plain" | tail -1 | grep -Eo '/[0-9]+' | tr -d '/' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl probe e2e: the driver reported no checks — the gate is not gating" >&2
    exit 1
fi

# **EVERY GROUP, BY LETTER.** The count above cannot tell "many groups ran" from
# "one group ran many checks", and a page that silently drives only some of the
# probes is the failure this gate has to be able to see. The driver prints the
# letters it actually recorded a check under; this insists every one is there.
# `AA` is a two-letter tag because the alphabet ran out at the indirect probe;
# the match below is space-delimited, so it cannot be confused with `A`.
LETTERS="$(grep -E '^probe e2e: groups ' "${OUTPUT}.plain" | tail -1 || true)"
if [ -z "$LETTERS" ]; then
    echo "crcbl probe e2e: the driver never printed which groups it ran" >&2
    exit 1
fi
MISSING=""
for letter in G H I J K L M N O P Q R S T U V W X Y Z AA AB AC AD AE AF AG AH AI AJ AK AL AM; do
    case " ${LETTERS#probe e2e: groups } " in
        *" $letter "*) ;;
        *) MISSING="$MISSING $letter" ;;
    esac
done
if [ -n "$MISSING" ]; then
    echo "crcbl probe e2e: these groups ran no checks at all:$MISSING" >&2
    echo "                 ${LETTERS}" >&2
    exit 1
fi

# THE NAMED CHECKS. Beside the count and the letters rather than instead of them:
# a group can keep its letter while the one claim that makes it worth running
# stops being made, and each of these is a claim nothing else in the repository
# makes. They were `web/run-browser-e2e.sh`'s while these groups lived there.

# Group G, the round trip. It is the only thing anywhere that watches a
# wasm → JS → wasm crossing go through the real transport in a real frame loop;
# every other check passes on a build whose `crcbl-webgpu` exports are never
# called, because nothing else calls them.
ROUND_TRIP="$(grep -F 'wasm encoded an adapter enumeration and the page loop replayed it' "${OUTPUT}.plain" || true)"
if [ -z "$ROUND_TRIP" ]; then
    echo "crcbl probe e2e: the driver never made a command-stream round trip;" >&2
    echo "                 crcbl-webgpu's transport is ungated in a real browser" >&2
    exit 1
fi

# The device half of group G, by name and separately, because it can stop
# happening on its own: the adapter checks pass on a build whose device request
# is never encoded, so the group's presence says nothing about this. It is also
# the seam's whole exit criterion — a browser opening a device and the seam
# reporting what it has — and an exit criterion nothing checks is a claim.
DEVICE_TRIP="$(grep -F 'the browser opened a device and wasm read its capabilities back' "${OUTPUT}.plain" || true)"
if [ -z "$DEVICE_TRIP" ]; then
    echo "crcbl probe e2e: the driver never opened a device through the command stream;" >&2
    echo "                 crcbl-webgpu's device request is ungated in a real browser" >&2
    exit 1
fi

# And once more for the surface, which is group H and is its own thing again:
# every check above passes on a build that never encodes a `CreateSurface`, and
# the node suite that does encode one resolves it against a stub canvas whose
# `getContext` hands back a plain object. Only this group asks a real element for
# a real `GPUCanvasContext`.
SURFACE_TRIP="$(grep -F 'the surface resolved the canvas the page registered and not the decoy' "${OUTPUT}.plain" || true)"
if [ -z "$SURFACE_TRIP" ]; then
    echo "crcbl probe e2e: the driver never resolved a surface against a real canvas;" >&2
    echo "                 crcbl-webgpu's CreateSurface is ungated in a real browser" >&2
    exit 1
fi

# And once more for the capability query, which is group I and is its own thing
# for a reason none of the above share: it is the only check anywhere that holds
# a value wasm received against what `navigator.gpu` tells the same page *at the
# same moment*, rather than against a fixture. Everything above passes on a
# machine whose preferred canvas format nobody ever asked for, and the node suite
# that drives this command answers it from a stub.
CAPS_TRIP="$(grep -F 'the preferred format wasm received is the one this browser prefers' "${OUTPUT}.plain" || true)"
if [ -z "$CAPS_TRIP" ]; then
    echo "crcbl probe e2e: the driver never held a surface capability against the browser's own answer;" >&2
    echo "                 crcbl-webgpu's SurfaceCaps query is ungated in a real browser" >&2
    exit 1
fi

# And once more for the buffer, which is group J and is the first check anywhere
# that watches this seam *make* something rather than ask a question. The node
# suite that encodes a `CreateBuffer` hands it to a stub device whose
# `createBuffer` returns a plain object built from the descriptor — so only this
# asks a real device for a real `GPUBuffer` and reads its size back.
BUFFER_TRIP="$(grep -F 'a real GPUBuffer came back from the device with the size that was asked for' "${OUTPUT}.plain" || true)"
if [ -z "$BUFFER_TRIP" ]; then
    echo "crcbl probe e2e: the driver never created a buffer on a real device;" >&2
    echo "                 crcbl-webgpu's CreateBuffer is ungated in a real browser" >&2
    exit 1
fi

# And once more for the image and its view, which is group K: the only check
# anywhere that watches this seam make a resource out of *another* resource it
# made. A `GPUTextureView` comes from the texture rather than from the device, so
# the image handle on the wire has to resolve in the replayer's own table first,
# and the whole-image subresource range has to reach the browser as an absent
# descriptor member rather than as the `u32::MAX` sentinel the wire carries.
IMAGE_TRIP="$(grep -F 'a real GPUTextureView came back from that texture with the whole-image range accepted' "${OUTPUT}.plain" || true)"
if [ -z "$IMAGE_TRIP" ]; then
    echo "crcbl probe e2e: the driver never made an image and a view of it on a real device;" >&2
    echo "                 crcbl-webgpu's CreateImage and CreateImageView are ungated in a real browser" >&2
    exit 1
fi

# And once more for the sampler, which is group L and is its own thing for a
# reason none of the above share: it is the only resource this seam makes whose
# object reports **nothing** about the descriptor it was made from, so the
# browser's only way to disagree is the device's error queue. What that reads for
# is the `lod_max` sentinel: `f32::MAX` crosses the wire verbatim and has to reach
# WebGPU as an explicit `lodMaxClamp`, because omitting the member — which is how
# the image view's range sentinel is spelled — substitutes WebGPU's own default
# and silently changes which mips every sampler can reach.
SAMPLER_TRIP="$(grep -F 'a real GPUSampler came back from the device with the no-limit lod clamp accepted' "${OUTPUT}.plain" || true)"
if [ -z "$SAMPLER_TRIP" ]; then
    echo "crcbl probe e2e: the driver never created a sampler on a real device;" >&2
    echo "                 crcbl-webgpu's CreateSampler is ungated in a real browser" >&2
    exit 1
fi

# And once more for the bind-group layout, which is group M and is its own thing
# for a reason none of the above share: it is the only command anywhere whose
# body is a **list**. Every one before it is a fixed set of fields, so a stride
# cannot be wrong; an entry here is five fields deep and carries an enum whose
# variants have different-length payloads, and a stride out by a byte decodes the
# next entry out of the middle of this one and produces a layout that is
# well-formed and describes different resources.
LAYOUT_TRIP="$(grep -F 'a real GPUBindGroupLayout came back from the device with every entry accepted' "${OUTPUT}.plain" || true)"
if [ -z "$LAYOUT_TRIP" ]; then
    echo "crcbl probe e2e: the driver never created a bind-group layout on a real device;" >&2
    echo "                 crcbl-webgpu's CreateBindGroupLayout is ungated in a real browser" >&2
    exit 1
fi

# And once more for the bind group, which is group N and is its own thing for a
# reason none of the above share: it is the only command whose entries name
# *other* resources — a layout, a buffer, an image view and a sampler that have
# to exist first — so its export encodes a whole frame, and its entries carry one
# handle into each of three resource tables where the discriminant is the only
# thing that says which.
GROUP_TRIP="$(grep -F 'a real GPUBindGroup came back from the device with the whole-buffer binding and all three resource kinds accepted' "${OUTPUT}.plain" || true)"
if [ -z "$GROUP_TRIP" ]; then
    echo "crcbl probe e2e: the driver never created a bind group on a real device;" >&2
    echo "                 crcbl-webgpu's CreateBindGroup is ungated in a real browser" >&2
    exit 1
fi

# And once more for the shader module, which is group O and is its own thing for
# a reason none of the above share: it is the only object this seam makes where
# *compilation* happens. WebGPU answers bad WGSL not by throwing but through
# `getCompilationInfo()`, so a module that came back yet would not compile is
# invisible to every other check here.
SHADER_TRIP="$(grep -F 'a real GPUShaderModule came back from the device with clean compilation info for the known-good WGSL' "${OUTPUT}.plain" || true)"
if [ -z "$SHADER_TRIP" ]; then
    echo "crcbl probe e2e: the driver never compiled a shader module on a real device;" >&2
    echo "                 crcbl-webgpu's CreateShaderModule is ungated in a real browser" >&2
    exit 1
fi

# And once more for the pipeline layout, which is group P and is the last thing a
# pipeline is built from. What the device's error queue guards here is the set
# resolution: a bind-group layout the pipeline layout could not find, or a set
# list the browser refused.
PIPELINE_LAYOUT_TRIP="$(grep -F 'a real GPUPipelineLayout came back from the device with the bind-group layout set accepted' "${OUTPUT}.plain" || true)"
if [ -z "$PIPELINE_LAYOUT_TRIP" ]; then
    echo "crcbl probe e2e: the driver never created a pipeline layout on a real device;" >&2
    echo "                 crcbl-webgpu's CreatePipelineLayout is ungated in a real browser" >&2
    exit 1
fi

# And once more for the compute pipeline, which is group Q and is its own thing
# for a reason none of the above share: it is the first command anywhere that
# resolves handles into two *different* tables — its layout out of the
# pipeline-layout table and its compute module out of the shader-module table —
# and the first where the shader is bound to the layout. `getBindGroupLayout(0)`
# is the derived layout only a genuinely-built pipeline can hand back.
COMPUTE_PIPELINE_TRIP="$(grep -F 'a real GPUComputePipeline came back from the device and answered getBindGroupLayout' "${OUTPUT}.plain" || true)"
if [ -z "$COMPUTE_PIPELINE_TRIP" ]; then
    echo "crcbl probe e2e: the driver never built a compute pipeline on a real device;" >&2
    echo "                 crcbl-webgpu's CreateComputePipeline is ungated in a real browser" >&2
    exit 1
fi

# And once more for the graphics pipeline, which is group R and is the largest
# descriptor on the seam — the whole nested tree of a raster pipeline: a
# primitive state, a reversed-Z depth-stencil, a multisample state, and a blended
# colour target, all of which a real `createRenderPipeline` has to accept
# together.
GRAPHICS_PIPELINE_TRIP="$(grep -F 'a real GPURenderPipeline came back from the device and answered getBindGroupLayout' "${OUTPUT}.plain" || true)"
if [ -z "$GRAPHICS_PIPELINE_TRIP" ]; then
    echo "crcbl probe e2e: the driver never built a graphics pipeline on a real device;" >&2
    echo "                 crcbl-webgpu's CreateGraphicsPipeline is ungated in a real browser" >&2
    exit 1
fi

# And once more for the readback, which is group S and is the decisive one — the
# first that reads *pixels* rather than confirming an object. A real device
# clears a real texture to a colour exact in 8 bits, a real copyTextureToBuffer
# and mapAsync carry it into host memory, and the reply channel hands the bytes
# back for every one of the 64×64 texels to be checked against the clear colour.
# The node suite proves the encoding and the state machine against a stub whose
# mapped buffer hands back whatever it likes — which is exactly why only a real
# browser can prove the *values*.
READBACK_TRIP="$(grep -F 'the cleared pixels came back from memory as the clear colour, every one' "${OUTPUT}.plain" || true)"
if [ -z "$READBACK_TRIP" ]; then
    echo "crcbl probe e2e: the driver never read cleared pixels back from a real device;" >&2
    echo "                 crcbl-webgpu's readback path put no right pixels in memory" >&2
    exit 1
fi

# And once more for the sRGB encode of a presented canvas frame, which is group X
# and is its own thing for a reason none of the above share: it is the only check
# anywhere that reads the pixels of a frame a *canvas* handed back and holds them
# against a transfer function. A canvas cannot be configured `-srgb`, so the page
# has to configure the base format, name the counterpart in `viewFormats` and
# create the acquired frame's view in it — three separate things to get wrong, and
# every one of them fails silently. Group S's clear proves the readback path but
# renders offscreen, where nothing is reinterpreted; the demo gates watch the
# canvas change without ever asking what colour it changed to. Both passed while
# the deployed site presented every frame a transfer function too dark.
SRGB_TRIP="$(grep -F 'the presented canvas frame came back from memory sRGB-encoded as the render colour, every pixel' "${OUTPUT}.plain" || true)"
if [ -z "$SRGB_TRIP" ]; then
    echo "crcbl probe e2e: the driver never held a presented canvas frame against the sRGB encode;" >&2
    echo "                 crcbl-webgpu could present every frame unencoded and nothing would say so" >&2
    exit 1
fi

# And the depth plane, which is group AA and is the only exercise this backend's
# `Capability::DepthImageCopy` has anywhere. Every other backend's capability
# declarations are held to what they do by `crcbl/tests/hal_seam_e2e.rs`, which is
# a native binary and cannot open this backend at all: the browser is where a
# depth plane crosses `copyTextureToBuffer`, so without this the declaration is a
# sentence nothing tests. It is worth its own line because of what a wrong answer
# looks like — a shadow atlas that read back as nothing renders a frame in which
# every surface is lit and nothing looks broken.
DEPTH_TRIP="$(grep -F 'the depth plane came back from the browser as the cleared depth, every texel' "${OUTPUT}.plain" || true)"
if [ -z "$DEPTH_TRIP" ]; then
    echo "crcbl probe e2e: the driver never read a depth plane back from a real device;" >&2
    echo "                 crcbl-webgpu declares Capability::DepthImageCopy and nothing tried it" >&2
    exit 1
fi

# And the two ticks a pass reported, which is group AF and is the only exercise
# `Capability::TimestampQuery` has on this backend anywhere. It is worth its own
# line because of what a wrong answer looks like: not an error but two zeros —
# a pass whose `timestampWrites` the browser took and never wrote, read back by a
# profiler as a frame that cost nothing. This backend refused timestamp query
# sets outright until the seam's timestamps moved into the pass descriptor,
# precisely so that a handle nothing could write never existed; this is what
# holds the new answer to a value.
TIMESTAMP_TRIP="$(grep -F 'the browser wrote both queries the pass named' "${OUTPUT}.plain" || true)"
if [ -z "$TIMESTAMP_TRIP" ]; then
    echo "crcbl probe e2e: the driver never read a timed pass's two queries back;" >&2
    echo "                 crcbl-webgpu declares Capability::TimestampQuery and nothing tried it" >&2
    exit 1
fi

# And the workgroup counts an indirect dispatch actually used, which is group AG
# and is the only exercise `dispatch_indirect` has on this backend anywhere. It
# is worth its own line because of what a wrong answer looks like: not an error
# but a dispatch that ran — with the wrong extents. A GPU culling pass whose
# indirect counts were read from the wrong offset still submits, still writes its
# buffer, and quietly does a fraction of the work, which is why this group asserts
# the counts it read and not merely that something was dispatched.
INDIRECT_DISPATCH_TRIP="$(grep -F 'the per-axis tally spells the three workgroup counts the args buffer named' "${OUTPUT}.plain" || true)"
if [ -z "$INDIRECT_DISPATCH_TRIP" ]; then
    echo "crcbl probe e2e: the driver never read back which workgroup counts an indirect dispatch used;" >&2
    echo "                 crcbl-webgpu's dispatch_indirect is ungated in a real browser" >&2
    exit 1
fi

# And the two targets a clamped pipeline and its control drew into, which is
# group AH and is the only exercise `Features::DEPTH_CLAMP` has against a GPU
# anywhere. It is worth its own line because of what a wrong answer looks like:
# not an error but a pipeline that was built, accepted and quietly clipped
# anyway — geometry a caller asked to have clamped disappearing, which is a
# shadow cascade with holes in it rather than a crash. This crate reports the
# feature to callers, and until this group existed the only thing that had ever
# seen `depth_clamp` set was a node stub recording the descriptor it was handed.
CLAMP_TRIP="$(grep -F 'depth clamping decided which fragments survived' "${OUTPUT}.plain" || true)"
if [ -z "$CLAMP_TRIP" ]; then
    echo "crcbl probe e2e: the driver never read back what depth clamping changed;" >&2
    echo "                 crcbl-webgpu reports Features::DEPTH_CLAMP and no GPU ever tried it" >&2
    exit 1
fi

# And the texels a fragment shader read out of a texture, which is group AJ and
# is the only exercise `BindingKind::SampledImage` and `BindingKind::Sampler`
# have past *creation* anywhere. Groups M and N build a layout and a group naming
# both, and a `GPUBindGroupLayout` reports its `label` and nothing else, so that
# is as far as they reach. It is worth its own line because of what a wrong
# answer looks like: not an error but a picture — a flipped V axis, a transposed
# uv or a swapped channel each render a plausible frame, and the readback is the
# only place any of them shows.
SAMPLED_TEXTURE_TRIP="$(grep -F 'a texture reached the fragment shader and the texel it delivered was the right one' "${OUTPUT}.plain" || true)"
if [ -z "$SAMPLED_TEXTURE_TRIP" ]; then
    echo "crcbl probe e2e: the driver never read back what a shader sampled out of a texture;" >&2
    echo "                 crcbl-webgpu's sampled-image and sampler bindings are ungated in a real browser" >&2
    exit 1
fi

# And the browser's own answer about a compressed texture, which is group AK and
# is the only exercise `Features::TEXTURE_COMPRESSION_BC` has against a GPU
# anywhere. **This is the one line here that holds on every platform**: the
# decoded block is excused where the adapter lacks the feature — which is this
# harness's own SwiftShader run — so the claim that survives everywhere is that
# the browser refuses a bc1-rgba-unorm texture exactly where wasm declined to
# encode a frame for one. Without it the whole group would be excusable, and a
# runner that silently stopped asking would look identical to one that asked and
# was told no.
BC_ANSWER_TRIP="$(grep -F "the browser's own answer about bc1-rgba-unorm is the one wasm acted on" "${OUTPUT}.plain" || true)"
if [ -z "$BC_ANSWER_TRIP" ]; then
    echo "crcbl probe e2e: the driver never asked the browser whether it takes a BC texture;" >&2
    echo "                 crcbl-webgpu reports Features::TEXTURE_COMPRESSION_BC and nothing tried it" >&2
    exit 1
fi

# And the browser's own answer about a timestamp query set, which is group AL and
# is the only exercise anywhere showing that `Features::TIMESTAMP_QUERY`'s
# numbers track work rather than merely existing. **This is the line here that
# holds on every platform**: the separation between a busy pass and an empty one
# is excused where the device lacks the feature — which is CI's macOS runner,
# every run — so the claim that survives everywhere is that the browser refuses a
# 'timestamp' GPUQuerySet exactly where wasm declined to encode a frame using
# one. Without it the whole group would be excusable on that runner, and a probe
# that silently stopped asking would look identical to one that asked and was
# told no.
TIMESTAMP_ANSWER_TRIP="$(grep -F "the browser's own answer about a 'timestamp' query set is the one wasm acted on" "${OUTPUT}.plain" || true)"
if [ -z "$TIMESTAMP_ANSWER_TRIP" ]; then
    echo "crcbl probe e2e: the driver never asked the browser whether it takes a timestamp query set;" >&2
    echo "                 crcbl-webgpu reports Features::TIMESTAMP_QUERY and nothing tried it" >&2
    exit 1
fi

# Group AM, which is not about the seam at all and is here because this is the
# only one of the three browser gates `.github/workflows/pages.yml` runs on all
# three operating systems. `web/tools/browser-e2e.mjs` never runs on Windows —
# that job invokes this script and nothing else — and Windows is one of the two
# platforms that implement `unadjustedMovement`, which is the whole subject.
#
# Both names are matched, because the two halves fail for opposite reasons and
# either alone would leave the other unstated:
#
#  - the answer check is the per-platform one. On Windows and macOS it asserts
#    the unadjusted request was GRANTED, which is what
#    `ShellCaps::RAW_POINTER_MOTION` promises there; on Linux it asserts it was
#    REFUSED with NotSupportedError, which is what `docs/backlog.md` records and
#    what `takeLock` in `web/engine/shell.js` is built to recover from. Neither
#    side is a skip: a platform that quietly changed its mind fails here rather
#    than passing by having nothing asserted about it.
#  - the held check is what says the recovery works. Without it, a Linux run
#    could assert the refusal and never notice that the fallback stopped taking
#    the lock — which is a browser with no mouselook at all, reported as a pass.
UNADJUSTED_ANSWER="$(grep -F 'unadjusted pointer motion is answered the way this platform answers it' "${OUTPUT}.plain" || true)"
if [ -z "$UNADJUSTED_ANSWER" ]; then
    echo "crcbl probe e2e: the driver never asked this platform for an unadjusted" >&2
    echo "                 pointer lock; ShellCaps::RAW_POINTER_MOTION is a claim" >&2
    echo "                 about the option nothing here tried, and this is the only" >&2
    echo "                 gate that runs on Windows at all" >&2
    exit 1
fi
LOCK_HELD="$(grep -F 'and the request that was meant to take the lock is holding it' "${OUTPUT}.plain" || true)"
if [ -z "$LOCK_HELD" ]; then
    echo "crcbl probe e2e: the driver never checked that the pointer lock was taken;" >&2
    echo "                 'unadjusted pointer motion is answered the way this platform" >&2
    echo "                 answers it' has no control, and on a platform that refuses" >&2
    echo "                 the option it passes for a browser with no lock at all" >&2
    exit 1
fi

if [ "$STATUS" -ne 0 ]; then
    # Which of the two the driver decided on. "at least one failed" is the wrong
    # sentence for a run where nothing failed and the `--expect-fail` list had
    # simply gone stale — and that run is the entire reason the list is safe to
    # keep, so the line that closes it has to say what actually happened.
    WENT_STALE="$(grep -E '^probe e2e: THE EXPECTED-FAIL LIST' "${OUTPUT}.plain" || true)"
    REAL_FAILURE="$(grep -E '^probe e2e: FAILED$' "${OUTPUT}.plain" || true)"
    if [ -n "$WENT_STALE" ] && [ -z "$REAL_FAILURE" ]; then
        echo "crcbl probe e2e: $RAN checks ran, nothing failed unexpectedly, and the --expect-fail list no longer describes them" >&2
    else
        echo "crcbl probe e2e: $RAN checks ran and at least one failed" >&2
        if [ "$XVFB_HARDWARE" = "1" ]; then
            echo "                 X alone failing is the expected shape here — see the Xvfb-plus-" >&2
            echo "                 hardware note above. Any other letter is a real failure." >&2
        fi
    fi
    exit "$STATUS"
fi

# **THE CLOSING SENTENCE HAS TO MATCH WHAT HAPPENED.** With `--expect-fail` in
# play a green run is "every group but these" and not "every byte back", and a
# harness that says the stronger thing anyway is the same defect as the list it
# is reporting on. The driver is the authority on which groups were excused — it
# is the half that owns the verdict — so this reads its line back rather than
# re-deriving one from the flags, and the two cannot drift.
EXCUSED="$(grep -E '^probe e2e: expected to fail on this platform: ' "${OUTPUT}.plain" | tail -1 || true)"
echo "crcbl probe e2e: $RAN checks ran in a real browser, over ${LETTERS#probe e2e: groups }"
if [ -n "$EXCUSED" ]; then
    echo "crcbl probe e2e: crcbl-webgpu opened a device and made every resource, and every"
    echo "                 group but ${EXCUSED#probe e2e: expected to fail on this platform: } read its bytes back"
    echo "                 those are excused here by --expect-fail, and this run would have failed"
    echo "                 had any of them passed"
else
    echo "crcbl probe e2e: crcbl-webgpu opened a device, made every resource, and read every byte back"
fi
