# Stage 10 — Wasm + WebGPU

> **Two corrections at the bottom of this file supersede parts of the body, and
> the body is read first — so, up front: the *networking* half of this stage is
> **removed** (browsers have no network transport; see
> [23-netcode.md](23-netcode.md)'s LAN correction), and the **"Tier B"
> vocabulary throughout is superseded\*\* by
> [39-capabilities.md](39-capabilities.md) — the reduced capability set belongs
> to the browser, not to `wgpu`, which on native reports very nearly the full
> native set. Read "Correction (scope and browser boundary, 2026-08-09)" before
> relying on anything below.

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

## Backend choice: reversed — `crcbl-webgpu`, not `crcbl-wgpu`

> **This decision was taken and then overturned.** What follows is the original
> reasoning, kept because the trade-off it weighed is the one anybody revisiting
> this would weigh again. **Do not build on it**: `crcbl-wgpu` was deleted on
> 2026-08-21 in `6b5e17a`, along with the whole `wgpu` dependency family, both
> its CI jobs and `CRCBL_GPU=wgpu`. The backend is `crcbl-webgpu` — wasm
> serialises HAL calls into a buffer it owns and JS decodes and replays them
> against WebGPU, which is `docs/plan/41-webgpu-stream.md`'s subject. The native
> portability fallback the argument below rests on does not exist; there is no
> triage backend, and `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` are the native
> set.

The original reasoning — implement the HAL on the **`wgpu` crate**, not raw
`web-sys` WebGPU:

- One implementation gives wasm/WebGPU **and** a native portability fallback
  (wgpu runs on vk/mtl/dx12/GL) — useful for old-Intel-iGPU Windows machines and
  as a triage tool ("does it repro on wgpu?").
- wgpu's API is close to the HAL's Tier B subset; the impedance mismatch is
  small compared to browser-API bindings by hand.
- Cost: dependency weight and wgpu's own abstraction overhead — acceptable for
  the portability tier; the performance tier remains `crcbl-vk`/mtl/dx12.

The data-layout consequences that argument leaned on are unchanged and are
stated canonically in "The browser boundary" below. Culling still runs in
compute on the GPU — the GPU-bound principle holds, only the draw-emission tail
differs.

## Platform work

- **Build**: `wasm32-unknown-unknown`, and a small `crcbl-web` crate for canvas
  setup, rAF loop driving `tick(dt)`, resize/DPI from the browser. crcbl-shell's
  canvas backend (topic 15, own JS shim) provides this — if its canvas/rAF
  handling is solid, `crcbl-web` shrinks to glue. See the deviations below for
  what that turned into.
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

1. WebTransport/WebSocket transport + server listener; sandbox client-in-browser
   connecting to native server — **the multiplayer first-class story,
   demonstrated**.
2. Perf pass: Tier B scene budget defined (smaller than native, explicitly);
   document the tier gap honestly.
3. (Stretch) Editor-in-browser smoke — should mostly work by construction; fix
   what doesn't, don't polish.

## Exit criteria

- Sandbox scene runs in Chrome/Firefox-with-WebGPU at target frame rate for the
  Tier B budget.
- Browser client connects to a native dedicated server over WebTransport and
  plays the stage 4 demo interactively.
- Same debug overlay works in-browser (UI is engine-rendered — free by
  construction; profiler shows WebGPU timestamps where available, degrades
  gracefully where not).
- CI: a wasm build and a headless browser run. `web/run-browser-e2e.sh` is what
  that became — a real Chromium over the DevTools protocol, not graph tests.

## Risks

- **WebGPU timestamp/feature availability varies by browser.** Debug tooling
  degrades feature-by-feature, never breaks the build.
- **Tier B perf disappointment.** Set the budget expectation in this doc's perf
  task — Tier B is the reach-everyone tier, not the showcase tier.

## Deliberate deviations from this document

**The sequencing lesson first, because it is the transferable part.** The tasks
above are written native-backend-first — "faster iteration, same code" — and
that order was right; what actually happened is that the browser work reached
the end of the platform half (page, shim, deploy, export checks) while the
graphics half still had no shader a browser would accept. A platform track can
be finished and demonstrate nothing.

- **`wasm-bindgen` is a build tool, not a binding strategy** — and it is now not
  even that. `docs/plan/15-windowing.md` rejects it for the shell and the same
  rule held everywhere: no crate depends on it, `#[wasm_bindgen]` appears
  nowhere, and every `__crcbl_*` symbol is hand-written `extern "C"`. The CLI
  was mandatory only for as long as something reached WebGPU through `web-sys`
  and left `__wbindgen_placeholder__` imports nothing else could resolve; with
  that gone `web/build.sh` runs no `wasm-bindgen` at all, and
  `web/tools/check-exports.mjs` asserts per demo that the single-threaded
  artifact imports **nothing** — the threaded one importing only a shared
  `env.memory`, which a module cannot own and be attached to from a worker.
- **The audio feed is shape B** (render on the main thread, `postMessage`
  transferred blocks) rather than the shape A `crcbl-audio` prefers. The reason
  that survives is the one about memory: a second wasm instance in the worklet
  would have its own linear memory and none of the voices the game queued, and
  there is no `play(id)` in the audio ABI for it to be told about them. The cost
  is the buffered lead stated in `web/engine/audio-worklet.js`.
- **There is no `crcbl-web` crate.** The build bullet allowed for one "if
  crcbl-shell's canvas/rAF handling is solid". It is, and the split a second
  sample forced is `crcbl::web` — a module in `crcbl` itself rather than a
  crate. It owns the protocol: the status codes the page polls, the log queue
  the page drains, the asset base, and a `web_exports!` macro that writes a
  sample's exports for it. What stays in the sample is its `WebPending` impl,
  because the options a game boots with and the error it fails with are the
  game's own. The _symbols_ still have to be per-demo — two demos can be open in
  one browser and the shim looks each up by name — but nothing behind them is.

## The gate, and what it found

`web/run-browser-e2e.sh` serves the built site, drives it in a real Chromium
over the DevTools protocol, and reads the canvas back. It is the gate this
document's exit criteria are measured by, and it needs no GPU — the default
configuration is Xvfb plus Chromium's bundled SwiftShader. ROADMAP's status
section carries what it currently checks; this section is about why it is shaped
the way it is.

**The two things the browser found that nothing else could.** Dawn enforces
WGSL's uniformity rule where naga does not, and rejected the UI shader for
sampling the glyph atlas under a branch on a varying — which invalidated the
frame's whole command buffer and left the canvas black while the simulation ran
normally. And a WebGPU backend cannot observe a pipeline it failed to create,
because WebGPU reports creation failures to the device error callback — a run
submitted invalid command buffers by the hundred while reporting a healthy
status. Both are fixed, the second by `Device::take_error`, which `Gpu::acquire`
drains before it records anything; see
[41-webgpu-stream.md](41-webgpu-stream.md)'s error-attribution section for the
shape that gives it.

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

### The editor is a native target

Task 6's "editor-in-browser smoke — should mostly work by construction" was
never examined; the asset browser, OS drag-drop and the notify-based file
watcher are all native-shaped. See [08-editor.md](08-editor.md).
