#!/usr/bin/env bash
# Gate `crcbl-webgpu`'s HAL seam in a real browser, against a real `GPUDevice`.
#
#   ./web/run-probe-e2e.sh [--build] [--headless] [driver args…]
#
# The same shape as `web/run-browser-e2e.sh`: it brings its own environment up,
# says what it needs and why, prints what it actually checked, and **fails when
# zero checks ran** — `docs/plan/12-testing.md` names a silently-skipped e2e job
# as a known trap and this is the guard against it.
#
# WHAT THIS IS THE ONLY GATE FOR. Groups G through Z in
# `web/tools/probe-groups.mjs` drive `crcbl-webgpu`'s command stream directly
# rather than through the engine: the wasm→JS→wasm round trip and the device it
# opens, a surface, the capability query held against what `navigator.gpu` tells
# the same page, then a buffer, an image and its view, a sampler, a bind-group
# layout, a bind group, a shader module, a pipeline layout and both pipelines —
# and from S onward the ones that read bytes back and compare their *values*: a
# cleared texture, a drawn triangle, a compute shader's storage writes, a copy
# chain, a sub-range fill, a presented canvas frame, a reconfigured swapchain and
# an indirect draw. `web/run-browser-e2e.sh` proves the demos render through this
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
#     under `--headless=new` plus SwiftShader the three groups that acquire a
#     canvas frame or present one — X, Y and Z — never resolve their readback
#     map, measured on Chromium 151, while every one of them passes in the same
#     browser inside Xvfb. A headless run on such a machine therefore fails
#     loudly rather than quietly skipping, which is the point.
#
# ENVIRONMENT
#   SITE_DIR                   Where the built site is. Default `target/site`.
#   CRCBL_CHROMIUM             Path to the browser binary.
#   CRCBL_CHROMIUM_FLAGS       Extra flags, space separated. How a machine with a
#                              real GPU points the run at it.
#   CRCBL_CHROMIUM_NO_SANDBOX  `1` adds --no-sandbox.
#   CRCBL_WEB_E2E_TIMEOUT_MS   How long each poll is given. SwiftShader is slow.
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
    STAMP="$SITE/probe/main.js"
    if [ -f "$STAMP" ]; then
        NEWER="$(find "$REPO/web/engine" "$REPO/web/probe" \
            -name '*.js' -newer "$STAMP")"
        if [ -n "$NEWER" ]; then
            echo "crcbl probe e2e: WARNING — $SITE is older than these sources, so this run does not test them:" >&2
            echo "$NEWER" | sed 's|^|  |' >&2
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
    echo "                 canvas-frame readbacks in groups X, Y and Z, and the run will"
    echo "                 say so rather than pass."
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
else
    echo "crcbl probe e2e: no display; the browser runs --headless=new"
fi

set +e
node "$REPO/web/tools/probe-e2e.mjs" --site "$SITE" "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

# CI sets `CARGO_TERM_COLOR: always`, and a coloured pipeline has broken this
# repository's test-count guards before, so strip escapes before matching.
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" > "${OUTPUT}.plain"

# The guard every harness here has: a run that checked nothing must not be able
# to report success. The driver exits non-zero on its own in that case; this is
# the second lock, because the failure being guarded against is precisely "the
# thing that was supposed to notice did not".
RAN="$(grep -Eo '[0-9]+/[0-9]+ checks passed' "${OUTPUT}.plain" | tail -1 | grep -Eo '/[0-9]+' | tr -d '/' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl probe e2e: the driver reported no checks — the gate is not gating" >&2
    exit 1
fi

# **EVERY GROUP, BY LETTER.** The count above cannot tell "twenty groups ran" from
# "one group ran twenty checks", and a page that silently drives only some of the
# probes is the failure this gate has to be able to see. The driver prints the
# letters it actually recorded a check under; this insists all twenty are there.
LETTERS="$(grep -E '^probe e2e: groups ' "${OUTPUT}.plain" | tail -1 || true)"
if [ -z "$LETTERS" ]; then
    echo "crcbl probe e2e: the driver never printed which groups it ran" >&2
    exit 1
fi
MISSING=""
for letter in G H I J K L M N O P Q R S T U V W X Y Z; do
    case " ${LETTERS#probe e2e: groups } " in
        *" $letter "*) ;;
        *) MISSING="$MISSING $letter" ;;
    esac
done
if [ -n "$MISSING" ]; then
    echo "crcbl probe e2e: these seam groups ran no checks at all:$MISSING" >&2
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

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl probe e2e: $RAN checks ran and at least one failed" >&2
    exit "$STATUS"
fi

echo "crcbl probe e2e: $RAN checks ran in a real browser, over ${LETTERS#probe e2e: groups }"
echo "crcbl probe e2e: crcbl-webgpu opened a device, made every resource, and read every byte back"
