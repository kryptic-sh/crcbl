# Stage 10 — Wasm + WebGPU

> **Scheduling note (ROADMAP wins):** this doc's _backend/platform_ half
> (crcbl-wgpu, canvas/rAF, Slang→WGSL, FetchSource, AudioWorklet) lands
> **early**, at roadmap phase P5 — right after the first sample — so every
> sample publishes to the GitHub Pages demo site from then on. The _networking_
> half (WebTransport/WebSocket + dedicated-server listener) lands at P13 for
> towers co-op.

Run games (and eventually the editor) in the browser. The browser has no Vulkan
— WebGPU is the graphics API, wasm32 is the target, and several platform
assumptions (threads, blocking IO, UDP) change. This stage exists as its own
track, but it _taxes every earlier stage_ with design constraints, called out
below and back-referenced from the relevant stage docs.

## Constraints wasm imposes on the whole engine (already in earlier stages)

| Constraint                              | Where it's handled                                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| No bindless / no MDI / no BDA in WebGPU | Stage 3 renderer tiers: Tier B data layout rules                                                             |
| No blocking file IO                     | Stage 6 `AssetSource` async from day one; `FetchSource`                                                      |
| No UDP/QUIC sockets                     | Stage 4 transport trait is message-oriented, not socket-shaped                                               |
| No spawning blocking threads by default | Engine core loop is single-thread-capable; job system (if added post-MVP) must have a single-thread fallback |
| Main loop owned by browser (rAF)        | Stage 1 frame loop is a `fn tick(dt)` driven by an outer loop, not a `loop {}` that owns the thread          |
| Swapchain acquire is implicit/async     | HAL surface API shaped so "acquire" can be trivial (WebGPU `getCurrentTexture`)                              |

If any of these turn out violated when this stage starts, that's a bug in the
earlier stage, not a wasm special case.

## Backend choice: `crcbl-wgpu`

Implement the HAL on the **`wgpu` crate**, not raw `web-sys` WebGPU:

- One implementation gives wasm/WebGPU **and** a native portability fallback
  (wgpu runs on vk/mtl/dx12/GL) — useful for old-Intel-iGPU Windows machines and
  as a triage tool ("does it repro on wgpu?").
- wgpu's API is close to the HAL's Tier B subset; the impedance mismatch is
  small compared to browser-API bindings by hand.
- Cost: dependency weight and wgpu's own abstraction overhead — acceptable for
  the portability tier; the performance tier remains `crcbl-vk`/mtl/dx12.

Tier B refresher (from stage 3): no descriptor-indexing bindless → texture array
pages + batching; no `draw_indirect_count` → compacted instance list + per-batch
`draw_indirect`; no BDA → indexed SSBO lookups. Culling still runs in compute on
the GPU — the GPU-bound principle holds, only the draw-emission tail differs.

## Platform work

- **Build**: `wasm32-unknown-unknown` + `wasm-bindgen`; a small `crcbl-web`
  crate for canvas setup, rAF loop driving `tick(dt)`, resize/DPI from the
  browser. crcbl-shell's canvas backend (topic 15, own JS shim) provides this —
  if its canvas/rAF handling is solid, `crcbl-web` shrinks to glue.
- **Shaders**: Slang → WGSL (via SPIR-V → naga if Slang's WGSL target isn't
  clean at the time). Same shader-hash pipeline, third artifact format.
- **Assets**: `FetchSource` (HTTP fetch → async decode). Asset packs matter more
  here (request count); pack format can arrive in this stage if needed.
- **Networking**: transport impl over **WebTransport** (datagram + streams maps
  well onto the reliable/unreliable channel semantics) with a **WebSocket**
  fallback (reliable-only; unreliable channel degrades to reliable). Server side
  gains the matching listener. This doubles as the first _real_ network
  transport for the engine overall — native QUIC can share the WebTransport
  implementation's protocol layer.
- **Threading**: MVP wasm build is single-threaded (sim + render on main thread,
  budgeted). wasm-threads/SharedArrayBuffer is a post-MVP optimization behind
  the same seam.

## Tasks

1. `crcbl-wgpu` HAL impl (native first — faster iteration, same code), Tier B
   renderer path validated on native wgpu against the stage 3 scene.
2. wasm build of sandbox: canvas, rAF, FetchSource, single-thread loop.
3. Slang→WGSL artifact pipeline.
4. WebTransport/WebSocket transport + server listener; sandbox client-in-browser
   connecting to native server — **the multiplayer first-class story,
   demonstrated**.
5. Perf pass: Tier B scene budget defined (smaller than native, explicitly);
   document the tier gap honestly.
6. (Stretch) Editor-in-browser smoke — should mostly work by construction; fix
   what doesn't, don't polish.

## Exit criteria

- Sandbox scene runs in Chrome/Firefox-with-WebGPU at target frame rate for the
  Tier B budget.
- Browser client connects to a native dedicated server over WebTransport and
  plays the stage 4 demo interactively.
- Same debug overlay works in-browser (UI is engine-rendered — free by
  construction; profiler shows WebGPU timestamps where available, degrades
  gracefully where not).
- CI: wasm build + headless WebGPU (via wgpu) graph tests.

## Risks

- **WebGPU timestamp/feature availability varies by browser.** Debug tooling
  degrades feature-by-feature, never breaks the build.
- **wgpu version churn.** Pin and upgrade deliberately (same policy as other
  pinned deps).
- **Tier B perf disappointment.** Set the budget expectation in this doc's perf
  task — Tier B is the reach-everyone tier, not the showcase tier.

## Status after P5.8 (the shim, the entry point, the deploy)

> **Superseded by "Status after P5.13" above.** The blocker named here — `naga`
> and `DrawParameters` — was closed by P5.9, and task 1 is done. The section is
> kept because the rest of it still holds and because the sequencing lesson at
> its end is the one worth keeping.

The page, the shim and the deploy exist; **the demo does not render yet.**
Recorded here rather than discovered by whoever opens the URL.

**What is built and checkable without a browser.** `apps/breakout` is a `cdylib`
with an `extern "C"` entry point (`src/web.rs`); `web/` is the shim, in plain ES
modules with no bundler and no npm; `.github/workflows/pages.yml` builds on PRs
and deploys on main. The artifact builds and exports all 60 `__crcbl_*` symbols;
the shim calls 56 of them and every one exists; the module imports nothing
outside the `wasm-bindgen` glue. `web/tools/check-exports.mjs` asserts all three
of those on every PR, and both of its failure directions were verified by
deliberately breaking them.

**What blocks it.** `naga` refuses every one of the engine's SPIR-V modules
(`UnsupportedCapability(DrawParameters)`), so `crcbl-wgpu` cannot create a
shader module on **any** target — the native `--backend wgpu` run fails the same
way. The WGSL artifacts this document's task 3 asks for already exist and are
complete; the HAL seam has no field to carry them. See ROADMAP's "Known gaps"
for the full diagnosis and the next slice.

**Task 1 of this stage is therefore not done.** The Tier B renderer path has not
been validated on native wgpu against any scene, and the exit criterion "sandbox
scene runs in Chrome/Firefox-with-WebGPU" cannot be attempted until it has been.
The order the tasks are written in — native wgpu first, "faster iteration, same
code" — was right, and the browser work reached the end of the platform half
before the graphics half had a working shader.

**Deliberate deviations from this document, both forced:**

- **`wasm-bindgen` is a build tool, not a binding strategy.** The plan's build
  bullet names it, and `docs/plan/15-windowing.md` rejects it for the shell.
  Both hold: no crate depends on `wasm-bindgen`, `#[wasm_bindgen]` appears
  nowhere, and every `__crcbl_*` symbol is hand-written `extern "C"`. The CLI is
  mandatory anyway, because `wgpu` reaches WebGPU through `web-sys` and leaves
  ~320 `__wbindgen_placeholder__` imports that nothing else can resolve —
  `WebAssembly.instantiateStreaming` on a raw artifact is a `LinkError`. Its
  version is read from `Cargo.lock` in one place (`web/build.sh`) so a
  mismatched CLI fails the build rather than a visitor's browser.
- **The audio feed is shape B** (render on the main thread, `postMessage`
  transferred blocks) rather than the shape A `crcbl-audio` prefers. Two
  independent reasons, either sufficient: `AudioWorkletGlobalScope` cannot
  satisfy the `wasm-bindgen` imports, and a second wasm instance in the worklet
  would have its own linear memory and none of the voices the game queued. The
  cost is ~21–43 ms of buffered lead, stated in `web/engine/audio-worklet.js`.
- **There is no `crcbl-web` crate.** The build bullet allowed for one "if
  crcbl-shell's canvas/rAF handling is solid". It is, and the glue that was left
  fitted in `apps/breakout/src/web.rs` and `web/engine/*.js`. A second sample is
  what will show which parts of that are engine and which are sample.

## Status after P5.13 — exit criteria met

`web/run-browser-e2e.sh` serves the built site, drives it in a real Chromium
over the DevTools protocol, and reads the canvas back. It is the gate this
document's exit criteria are measured by, and it needs no GPU — the default
configuration is Xvfb plus Chromium's bundled SwiftShader.

**It reports 18/18.** breakout boots, `crcbl-wgpu` opens a WebGPU device, a
960x511 `Rgba8Unorm` Fifo swapchain is configured on the canvas, the rAF loop
runs, a real click focuses the canvas, a real `Space` keydown reaches
`__crcbl_web_key` and launches the ball, a brick breaks (score 10, 39 bricks
left, the audio cue fires), and the canvas holds 13 distinct colours across 16
distinct frames with no WebGPU device errors behind them. Task 2 of this
document — "wasm build of sandbox: canvas, rAF, FetchSource, single-thread loop"
— holds for breakout.

**Task 1 is discharged too.** "Tier B renderer path validated on native wgpu"
was skipped when the browser work ran ahead of it, and P5.11 went back for it:
`crcbl-wgpu` now presents windowed frames and renders offscreen with readback,
and the offscreen frame is byte-identical to Vulkan's on the same driver.

**The two things the browser found that nothing else could.** Dawn enforces
WGSL's uniformity rule where naga does not, and rejected the UI shader for
sampling the glyph atlas under a branch on a varying — which invalidated the
frame's whole command buffer and left the canvas black while the simulation ran
normally. And `crcbl-wgpu` cannot observe a pipeline it failed to create,
because WebGPU reports creation failures to the device error callback; the run
submitted 384 invalid command buffers while reporting a healthy status. The
first is fixed; the second is recorded in ROADMAP's known gaps.

**A readback trap worth keeping.** Three of the four obvious ways to read a
WebGPU canvas back return transparent black regardless of what was drawn,
varying by display and adapter — a first harness used `drawImage` and would have
blamed the engine for a working renderer. The gate therefore runs a known-colour
clear as a control in the same browser with the same flags, and refuses to
interpret the render checks unless the control reads back.

**Not yet measured:** frame rate against a Tier B budget (task 5), and any
browser other than Chromium.

## Correction (design review, 2026-07-27)

**GitHub Pages cannot set COOP/COEP headers**, so `SharedArrayBuffer` is
unavailable on the flagship deploy target. Consequences, stated rather than
discovered: (a) the "wasm-threads later" plan does not apply to the Pages demos
— those stay single-threaded; (b) the **AudioWorklet feed must not depend on an
SAB ring buffer** — design it around `postMessage`/worklet-pull from P5; (c) if
SAB is ever wanted on Pages, the standard workaround is the `coi-serviceworker`
shim, adopted deliberately rather than accidentally. Module memory (16) is
unaffected — an imported `WebAssembly.Memory` needs no SAB unless shared across
threads.

## Correction (scope and browser boundary, 2026-08-09)

### The networking half is removed

This document's task 4 (WebTransport/WebSocket transport + server listener) and
its exit criterion "browser client connects to a native dedicated server" are
**dropped**. Native multiplayer is LAN and web builds are single player, so no
browser client has a server to reach. See [23-netcode.md](23-netcode.md)'s LAN
correction for the full reasoning and for the WebRTC route that was deferred
rather than refused.

What remains of this stage is what it was always mostly about: the backend, the
platform, the shaders, the assets and the demo site.

### The threading section is superseded

"MVP wasm build is single-threaded … wasm-threads/SharedArrayBuffer is a
post-MVP optimization", and the constraint table's "job system (if added
post-MVP)", both predate **P5B**, which moved `crcbl-jobs` ahead of P6–P8 and
set wasm thread-topology parity as the target. [21-jobs.md](21-jobs.md)'s
2026-08-03 correction is canonical. The COOP/COEP gate below still stands and is
still a gate: Pages cannot set the headers, and if the `coi-serviceworker` shim
is declined the demos run single-threaded through the `Inline` spawner.

### The browser boundary, canonically

This table is the canonical list of what a browser cannot do. Anything relying
on a row here needs a stated fallback or an honest absence.

| Gap                                                 | Consequence                                                                  |
| --------------------------------------------------- | ---------------------------------------------------------------------------- |
| No bindless / binding arrays                        | `BindingModel::ArrayPages` — texture array pages + batching                  |
| No multi-draw-indirect or count                     | `GeometryPath::IndirectPerBatch` — compacted list, per-bucket draws          |
| No mesh shaders                                     | same; per-instance LOD instead of per-cluster                                |
| No ray tracing                                      | `LightingPath::Rasterised` — the raster twin is MVP for this reason          |
| No buffer device address                            | indexed SSBO lookups                                                         |
| No persistent mapped buffers                        | staging copies on every upload                                               |
| No pipeline cache                                   | every page load recompiles every shader — keep permutations low              |
| No threads without COOP/COEP                        | `Inline` spawner; sim on the main thread                                     |
| No listening socket, no LAN discovery, no HTTPS→LAN | no networking at all; web builds are single player                           |
| No NaN canonicalization, no fuel                    | module determinism unguarded; no hostile-module containment (topic 16)       |
| WebCodecs audio encode uneven                       | libopus compiled to wasm if VOIP ever ships to a browser (topic 32)          |
| `wasm32` address space                              | 4 GB architectural ceiling, browsers often lower — a wall, not a degradation |

Timestamp queries, compute, indirect draw, `INDIRECT_FIRST_INSTANCE`, f16,
dual-source blending and the BC/ETC2/ASTC families **are** available, so the
profiler, GPU culling and the post stack all work. The gap is narrower than
"Tier B" implied — see [39-capabilities.md](39-capabilities.md).

### Two smaller items

- **The editor is a native target.** Task 6's "editor-in-browser smoke — should
  mostly work by construction" was never examined; the asset browser, OS
  drag-drop and the notify-based file watcher are all native-shaped. See
  [08-editor.md](08-editor.md).
- **The P5.13 status section reports 18/18.** The browser gate has grown since;
  the ROADMAP status section carries the current count and is authoritative.
