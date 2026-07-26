# Stage 9 — Wasm + WebGPU

Run games (and eventually the editor) in the browser. The browser has no Vulkan
— WebGPU is the graphics API, wasm32 is the target, and several platform
assumptions (threads, blocking IO, UDP) change. This stage exists as its own
track, but it _taxes every earlier stage_ with design constraints, called out
below and back-referenced from the relevant stage docs.

## Constraints wasm imposes on the whole engine (already in earlier stages)

| Constraint                              | Where it's handled                                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| No bindless / no MDI / no BDA in WebGPU | Stage 3 renderer tiers: Tier B data layout rules                                                             |
| No blocking file IO                     | Stage 5 `AssetSource` async from day one; `FetchSource`                                                      |
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
  browser. winit's web backend evaluated first — if its canvas/rAF handling is
  solid, `crcbl-web` shrinks to glue.
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
- **wgpu version churn.** Pin and upgrade deliberately (same policy as winit).
- **Tier B perf disappointment.** Set the budget expectation in this doc's perf
  task — Tier B is the reach-everyone tier, not the showcase tier.
