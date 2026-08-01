# Roadmap — Interleaved Build Order (canonical)

This is the **canonical build order** for the engine. The numbered stage docs
(`01`–`10`) are topical deep-dives — architecture, tasks, risks per subsystem;
this roadmap interleaves _slices_ of those stages with the samples that consume
them.

Two hard rules govern the ordering:

1. **Engine base before any sample.** Phases 0–4 build the complete core (window
   → render → sim → physics slice → UI slice) before the first line of sample
   code. No sample ever starts before every module it needs exists.
2. **Component → system → sample.** After the base, work proceeds in pulls:
   build the component(s), build the system(s) on them, then ship the sample
   that proves them. The sample is the phase's exit criterion.

**Wasm is early** (phase 5, right after the first playable): every sample from
breakout onward ships as a browser demo on GitHub Pages. The demo site is the
engine's public face and its continuous cross-backend regression test.

## Status (as of 2026-08-01)

**P5 is complete.** Nine slices merged: polled device creation across the HAL
seam (P5.4), AudioWorklet output (P5.5), the wasm32 dependency graph (P5.6),
browser storage over fetch and OPFS (P5.7), the JS shim, wasm entry point and
Pages deploy (P5.8), WGSL across the seam (P5.9), a Tier B constants path
(P5.10), wgpu present + offscreen (P5.11), the cross-backend image gate (P5.12),
and the headless-browser gate (P5.13).

**The gate is closed: breakout renders and plays in a browser.**
`web/run-browser-e2e.sh` drives a real Chromium over the built site and reports
18/18 — the page boots, opens a WebGPU device, configures a swapchain, takes a
real click and a real `Space` key, launches the ball, breaks a brick, and draws
16 distinct frames across 16 samples with no device errors. The captured canvas
shows the brick grid, the paddle and the engine-drawn HUD.

Two blockers stood in the way and both are recorded below rather than forgotten:
naga rejected every SPIR-V artifact the engine shipped (closed by P5.9 carrying
WGSL across the seam), and Dawn — stricter than naga about WGSL's uniformity
rule — rejected the UI shader for sampling a texture under a branch on a
varying, which invalidated the whole frame's command buffer and left the canvas
black while the game ran normally. Both were found by building the check before
believing the code.

**The HAL seam is now frozen**, on the roadmap's own criterion: two backends
implement it, and `crcbl screenshot` renders the same scene through `crcbl-vk`
and `crcbl-wgpu` to byte-identical PNGs on one driver, one channel level apart
across two.

**A full-workspace code review** (`docs/code-review.md`, 2026-08-01) read every
line of the tree and its findings were fixed across eight commits before this
phase resumed — among them a BVH refit that hid untouched colliders from every
query, an unsound `Send`/`Sync` on the audio mixer, three allocation bombs
reachable from ordinary files, path traversal out of the storage root, an
unauthenticated ack that could permanently desync a client, and a `crcbl-wgpu`
that could not complete a frame. Breakout's determinism tests never launched the
ball, which is why three gameplay bugs had survived; they do now.

**P0 and P1 are complete and merged to `main`.** The `crcbl screenshot` CLI
subcommand (offscreen render → PNG), explicit `PipelineLayoutHandle` on
`bind_group`/`push_constants`, `BindGroupDesc::variable_count`, render-graph
sub-resource vocabulary, and three HAL seam findings closed on 2026-07-31. P2a
is complete: per-entity `Transform` replication (`SystemTrait::replicate` seam,
`PhysicsSystem` impl) and real client-side interpolation landed on 2026-07-31.
P2b is complete: protocol negotiation and reconnect, condition simulation,
per-sector ack-baseline streams, replication integration, hostile-input
hardening, per-client ingress limits, and decoder fuzzing in CI have landed. P2c
is complete: `crcbl-store` (StorageSource, settings, saves, replay), GameModule
API + static binding, and `.crpl` replay writer/reader with FileTransport. **P3
L0 is done**: overlap queries, dynamic BVH refit, swept-sphere-vs-capsule TOI,
trigger-volume support, ECS component types (RigidBody, Transform,
ColliderComponent), and PhysicsSystem (SystemTrait impl bridging entities to the
spatial world). **P4 is done**: `crcbl-ui` draw-list, glyph atlas text,
label/button widgets, HUD skeleton, draw-list snapshot hash tests; `crcbl-store`
crash ring buffer; `crcbl replay` CLI. **P4A audio is done**: device seam
(cpal), audio thread, mixer/voices, WAV and QOA decoders, full spatial cue
grammar rules 1–4, and golden-buffer test.

The phase table below is the plan; this section is the record. Where the two
disagree about what was built, this section is right and the phase row says what
was intended.

| Phase             | Status   | Landed as                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **P0** — base     | **done** | `f922ca3`, `3198f7a`, `6dd4b46`, `84af231`, `c058f45`, `bad7186`, `a991e42`, `063fd99`, `421ce69`, `f06e6cd`, `36dd636`, `e094d39`                                                                                                                                                                                                                                                                                                                                                                                    |
| **P1** — Vulkan   | **done** | `91fd871`, `236f19b`, `c6dc4a4`, `a54990d`, `dc36d32`, `8a4e303`, `cbd6153`                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| **P2** — sim core | **done** | `7b8efb5` scaffold, `f8e8117` net, `9e55569` ecs, `9d31e2d` input, `2b2e5bd` server/client, `d4d0330` sim harness; `7f2d920` interpolation; P2b through `f1e625c`, `e036705`, `ec0597e`, `fb7e7bf`, `9dcc30d`, `b6c9d7d`, `27390fd`, `cb3b110`, `b37e4d5`, `53405f6`, `4156345`; P2c through `4ff402e`, `fa16710`, `c4688f1`, `83362f3`, `1270c72`                                                                                                                                                                    |
| **P3** — phys L0  | **done** | `5665da2` overlaps, `cbfd6b1` dynamic BVH refit, `b1924dd` swept-capsule TOI + triggers, `c4db85d` ECS components, `b765640` PhysicsSystem, `a2be1f6` integrator, `6b30532` force providers, `60dd95e` integration loop, `05dd23a` property tests                                                                                                                                                                                                                                                                     |
| **P4** — UI L0    | **done** | `49ec170` draw-list, `b40ca95` label/button/HUD, `ab17700` `0352112` `510ce31` triangulation, `9f65472` snapshot tests; `1270c72` replay; `264a7fd` crash ring                                                                                                                                                                                                                                                                                                                                                        |
| **P4A** — audio   | **done** | `6bd33b2` device seam, `a7e94c2` mixer/voices/golden, `912234f` WAV, `2abbd2d` QOA, `916d51f` cue grammar rules 1–4, `bf9a245` clippy                                                                                                                                                                                                                                                                                                                                                                                 |
| **S1** — breakout | **done** | `d747a84` scaffold, `71d931c` paddle+input, `5989f1b` ball+physics, `495a7fb` bricks+scoring, `ee3bea5` audio+HUD, `fa4e20b` spatial panning, `3bcf327` client interpolation, `a6c2e6b` high score, `ecfd85a` determinism tests, `ab71b1e` input fix, `968b65a` launch fix, `e3fd64d` persistence test, `5f47a12` UI compositing pass                                                                                                                                                                                 |
| **P5** — wasm     | **done** | `a61bd26` polled device creation, `b932e28` AudioWorklet output, `325b8ba` wasm32 dependency graph, `9c1b48e` fetch + OPFS storage, `c5c2a13` JS shim + entry point + Pages deploy, `fd0bc23` WGSL across the seam, `df45682` Tier B constants, `33d4dc0` wgpu push-constant capability, `afb4579` wgpu present + offscreen, `e52e28c` cross-backend gate, `4d7c7c8` uniform-control-flow shader fix, `ed3e726` headless-browser gate. Earlier: `84e531b` WebShell, `afd63bf` wasm32 target, `f7d28ad` WGSL artifacts |

### What exists now

- **Workspace + CI**: 20 crates, 5 apps/support crates. CI is 16 required jobs —
  fmt, clippy `-D warnings`, rustdoc `-D warnings`, `cargo-machete`,
  `cargo-deny`, nextest on Linux and cross-platform (macOS + Windows), coverage,
  a weekly advisory cron, seven e2e suites (Wayland under nested sway, X11 under
  Xvfb, Vulkan on lavapipe, wgpu on lavapipe under Xvfb, the cross-backend
  vk↔wgpu image compare, the CLI scaffold, and the shader-manifest check), and a
  decoder fuzz job.
- **`crcbl-core`**: `Handle`/`Pool`, sector-tiled `WorldPos` (`I64Vec3` sectors,
  2^20 m cells), `FrameArena`, `FrameClock` with an injected `TimeSource`, the
  input vocabulary, `SurfaceTarget`, logging.
- **`crcbl-hal`**: the backend seam — object-safe traits, POD descriptors,
  handle-based resources, reversed-Z defaults, request/poll readback, and a
  recording `NullBackend` that is a test tool rather than a stub.
- **`crcbl-shell`**: the platform-agnostic seam plus `HeadlessShell`, and
  **working Wayland and X11 backends** built on our own protocol codegen
  (`crcbl-wl-scanner`) and hand-written `dlopen`'d FFI. Windows, monitors,
  input, XKB keymaps, pointer lock and raw motion, fractional scale, clipboard.
  XDND on X11 is the one deliberate gap — see
  [15-windowing.md](15-windowing.md).
- **`crcbl-vk`**: loader through present on real hardware — device, swapchain,
  pipelines, bind groups, samplers, deletion queue, timestamp queries.
  Validation is enabled and provably non-vacuous.
- **`crcbl-render`**: the render graph. Passes declare reads and writes; compile
  is pure and testable with no GPU; it is the only thing that emits a barrier in
  a frame, and it can dump its own pass order and barriers as text.
- **`crcbl-shaders`**: Slang sources with committed SPIR-V, drift caught by a
  SHA-256 manifest that verifies with no compiler installed.
- **`crcbl-golden`**: the image comparator, tolerance calibrated by measurement
  against radv-vs-lavapipe rather than guessed.
- **`crcbl-cli`** (`new`/`run`/`build`) and **`apps/sandbox`**, which draws a
  reversed-Z lit spinning cube through the graph into an HDR target on both
  Wayland and X11.
- **`apps/breakout`** (S1, 12 slices merged): paddle movement via `ActionMap`,
  server-authoritative over `InMemoryTransport`, `PhysicsSystem` with
  swept-sphere CCD (ball, paddle, 40-brick grid, left/right/top walls), game
  state machine (Waiting→Playing→Won/Lost), 3 lives + fall-out detection,
  spatial audio panning (`compute_cue` per ball position), client-interpolated
  ball state with replication drift detection, high score persistence via
  `crcbl_store::write_atomic`, on-screen HUD via `UiRenderer` (glyph atlas
  texture, per-frame vertex/index upload, alpha-blended compositing pass), 7
  unit/e2e tests including scripted-input determinism and persistence RT.
- **`crcbl-wgpu`** (P5): wgpu 30 backend — `WgpuInstance` (adapter enumeration,
  registered in backend table), `WgpuDevice` with full resource/pipeline/
  bind-group creation and pool-based handle tracking, command encoder finish
  flow with pre-allocated handle slots, `conv` mappings (formats, blend states,
  rasteriser, binding kinds). Available via `CRCBL_GPU=wgpu` on native.
  Surface/swapchain (Wayland, Xcb via raw-window-handle) and full command
  recording (render/compute passes, draw/bind/state calls via forget_lifetime)
  implemented. Push constants remain a no-op (wgpu 30 lacks the API), indirect
  draws and copies remain stubbed (P6+).
- **`crcbl-wgpu` cross-platform**: Compiles on both native (Vulkan/Metal/DX12)
  and `wasm32-unknown-unknown` (WebGPU) via `cell` module (polymorphic
  `Arc`/`Mutex` ↔ `Rc`/`RefCell`). `HalThreadSafe` marker trait in `crcbl-hal`
  relaxes `Send + Sync` bounds on wasm32. `create_native()` gated to non-wasm
  targets.
- **`crcbl-shell`** (P5): `WebShell` backend — single-canvas window,
  `extern "C"` entry points for JS→wasm input (resize/key/pointer/frame), event
  queue over `RefCell<VecDeque>`, `SurfaceTarget::Web { canvas_id }`. Registered
  in backend table as non-auto (`CRCBL_SHELL=web`). Caps exclude `MULTI_WINDOW`,
  `POINTER_WARP`, `CLIPBOARD`, `DRAG_DROP`, `EVENT_WAIT`. 8 unit tests. JS shim
  counterpart (P5.3) follows.
- **Polled device creation** (P5.4): `Instance::request_device` returns a
  `Box<dyn PendingDevice>` whose `poll` yields `Pending` or `Ready(device)`,
  following the `request_readback`/`poll_readback` idiom the seam already had.
  `wgpu`'s `requestDevice` is a promise and the browser main thread cannot block
  on it, so before this `crcbl-wgpu` returned `Unsupported` on wasm32 and
  nothing could render in a browser. Everything decidable synchronously (unknown
  adapter, missing features, foreign surface) is still an error from the
  request. `create_device` remains as a provided blocking wrapper and is
  `#[cfg]`-ed out on wasm32, so a browser build that reaches for it fails to
  compile rather than at run time; the ~35 native call sites are untouched.
  Vulkan's first poll is ready (no faked latency); the null backend models both
  outcomes and gained `set_device_latency` beside `set_readback_latency`, so the
  polled path is testable with no GPU. `crcbl::backend::request_open` and
  `GpuContext::request_open` are the browser-shaped entry points.
- **AudioWorklet output** (P5.5): `crcbl-audio` gained a web output and gated
  `cpal` to non-wasm targets — its wasm dependency graph is now zero third-party
  crates. The seam is a pure pull (`render(frames)` into wasm-owned planar
  buffers), which needs no `SharedArrayBuffer` and so survives Pages' inability
  to set COOP/COEP, as the 2026-07-27 correction in
  [10-wasm-webgpu.md](10-wasm-webgpu.md) required. The source is always driven
  at the fixed 48 kHz internal rate and resampled to the device rate with phase
  and carry frames across blocks — passing a 44.1 kHz device rate down would
  detune every asset by 147 cents and corrupt the pitch cues rules 3 and 4
  depend on. An underrun returns a short block and is counted, never a repeat.
- **Browser storage** (P5.7): `crcbl-store::web` — `FetchSource` (pre-load as
  the intended mode, request/poll underneath for anything discovered at run
  time; a miss returns `StorageError::Pending`) and `OpfsStorage`. OPFS has no
  `rename`, so `write_atomic`'s guarantee is split honestly rather than quietly
  weakened: generation ping-pong with SHA-256-framed records keeps "no torn
  value", "no older value after a newer one" and "nothing left behind by a
  failure", and drops durability-on-return — `write` returns when queued, and
  the answerable form is `queued + in_flight == 0`. Nothing needs an OPFS sync
  access handle, so the engine may run on the main thread or in a Worker; the
  shim declares which and a shim that never drains fills the queue and is
  refused rather than silently dropping writes. One `canonical_key` guards every
  key from the engine and from the shim's manifest, with a charset that excludes
  every URL metacharacter, so no key can escape the asset root.
- **wasm32 dependency graph** (P5.6): `apps/breakout` and the `crcbl` umbrella
  build for `wasm32-unknown-unknown`. `crcbl-vk` moved to a
  `cfg(not(target_arch = "wasm32"))` dependency of the umbrella (`ash` reaches
  `libloading`, which has no wasm build), and its registry entry is `#[cfg]`-ed
  out to match; `wgpu` becomes the auto-selectable backend on wasm only, so
  native selection order is unchanged. `crcbl::screenshot` is native-only —
  every step of it blocks. `getrandom`'s `wasm_js` backend is enabled from
  `crcbl-server`'s wasm target section, so `wasm-bindgen`/`js-sys` stay out of
  native binaries. CI's wasm32 job now checks and clippies the whole workspace
  minus `crcbl-vk`, with `apps/breakout` first and by name.
- **JS shim, wasm entry point, Pages deploy** (P5.8): `apps/breakout` is a
  library with two front ends — `main.rs` (argv, exit codes) and `src/web.rs`
  (`cdylib`, `extern "C"`, driven by `requestAnimationFrame`). Start-up is
  polled: `PendingLoop` turns `wait_for_configure` and `Gpu::open`'s two blocks
  inside out, so the device promise is polled across rAF frames instead of being
  waited on inside the loop that resolves it. The clock is the browser's
  (`Loop::set_frame_step`, clamped) because `Instant::now` panics on
  `wasm32-unknown-unknown`, and so does `crcbl_core::log::init_logging` — the
  browser build queues log lines in wasm and the page drains them. The high
  score moved to `OpfsStorage` on wasm32. `web/` holds the shim: canvas/DPI/
  input, the AudioWorklet feed (shape B, `postMessage`, no `SharedArrayBuffer`),
  fetch pre-load, OPFS restore/drain, plus the site's index and the breakout
  page. `.github/workflows/pages.yml` builds on PRs and deploys on main, with
  `pages: write`/`id-token: write` scoped to the deploy job only.
  `web/tools/check-exports.mjs` is the gate that can run without a browser: it
  compares what Rust declares, what the shim calls, and what the artifact
  exports, and fails on any of the three disagreeing. **`wasm-bindgen` is
  adopted as a build tool only** — no `#[wasm_bindgen]`, no crate depends on it,
  the version comes out of `Cargo.lock` — because `wgpu`→`web-sys` leaves ~320
  unresolvable `__wbindgen_placeholder__` imports that only its CLI can link.
  The gate this slice could not close — no browser had ever loaded the page — is
  closed by P5.13.
- **Web key ABI correction** (P5.8): `crcbl-shell`'s `__crcbl_web_key` takes two
  `(ptr, len)` pairs and the module had no way for a browser to obtain an
  address inside wasm memory, so the entry point was specified but not callable.
  `__crcbl_web_key_scratch_ptr`/`_capacity` publish a wasm-owned scratch, the
  same shape `crcbl-store`'s fetch and OPFS ABIs already used. Found by writing
  the shim; a test now drives a key event through the real path.
- **`crcbl-shaders`** (P5.3): WGSL artifacts committed alongside SPIR-V —
  `compile-shaders.sh` produces `wgsl/*.wgsl` per shader; `Shader::wgsl()`
  returns the source for wgpu backends. Manifest tracks both formats. Swapchain,
  surface, and command-encoder recording remain stubbed (P6+).
- **`crcbl-ecs`** (P2a): system-owned-array ECS — `World`, `System<T>` (dense
  SoA + sparse entity→index), `Schedule` (ordered tick sequence), `Inspector`
  (per-system stats). Entity lifecycle: spawn, deferred despawn, generational
  sweep across all systems.
- **`crcbl-net`** (P2a): transport seam and replication protocol — `Transport`
  trait (reliable/unreliable channels, non-blocking), `InMemoryTransport` (SPSC
  pair for single-player and tests), `SnapshotWriter`/`SnapshotReader`
  (per-system snapshot encoding). **P2b additions**: protocol handshake +
  schema-hash gate, session management + reconnect, `ConditionSimulator`
  loss/reorder wrapper, hardened wire codec, ack-baseline delta encoding with
  entity-removal and keyframe fallback, sector-scoped envelopes, per-client
  ingress limits, and decoder fuzzing in CI.
- **`crcbl-input`** (P2a): device-agnostic action system — `ActionMap`
  (bindings→actions, WASD composite, mouse motion/scroll), `ButtonAction` with
  just-pressed/just-released edges, `InputTickState` for client→server tick
  snapshots.
- **`crcbl-server`** (P2a): authoritative fixed-tick server — drains client
  inputs, advances the ECS schedule, emits per-tick snapshots over the
  transport. Headless-runnable; no render dependency.
- **`crcbl-client`** (P2a): rendering client — sends input each tick, buffers
  the two most recent snapshots for interpolation, handles snapshot reordering.
- **`crcbl-sim`** (P2a): determinism harness — runs N ticks of a seed-generated
  world using `ManualTime`, prints a state hash. Same input → same hash across
  runs, verified.
- **`crcbl-phys`** (P3 L0): query + kinematics pillar — `PhysicsWorld` with
  sphere/box/capsule colliders, static BVH with O(log n) refit, overlap queries
  (sphere + AABB), ray-vs-shape and swept-sphere TOI (including exact capsule),
  trigger-volume support, ECS component types (`RigidBody`, `Transform`,
  `ColliderComponent`), `PhysicsSystem` (full integration loop: force providers,
  SemiImplicitEuler integration, collider sync), L1 dynamics (terminal velocity
  emergence, determinism, bounded energy drift), and 1000-body stress test with
  state-hash determinism.
- **`crcbl-ui`** (P4): immediate-mode draw list with rect/text/outline commands,
  CPU triangulation into screen-space vertex+index buffers, baked-in monospace
  glyph atlas (95 ASCII glyphs at 8×13 px), `FontAtlas` text layout (greedy
  line-break), `Label`/`Button` widgets with state-based styling,
  `Hud`/`HudPanel` anchored overlay container, draw-list snapshot hash tests.
  **S1 completion** added `UiRenderer` in `crcbl-render` (GPU pipeline, glyph
  atlas texture upload, per-frame vertex/index buffer write, alpha-blended
  compositing pass on top of the tonemap target) and the `ui.slang` shader in
  `crcbl-shaders` (screen-space vertex pulling, push-constant viewport
  transform, glyph atlas sampling).
- **`crcbl-audio`** (P4A): `AudioStream` device seam (cpal native, null for CI),
  audio thread polling at hardware sample rate, `Mixer` with pooled `Voice`s
  (loop, stop, volume), stereo f32 internal format, WAV decoder (mono/stereo
  8/16/ 24/32-bit PCM + float), QOA decoder, `SpatialCue` compute with
  deterministic cue grammar rules 1–4 (ITD + ILD for pan, rear attenuation +
  pitch for behind, elevation pitch for above/below, distance rolloff), and
  golden-buffer test.

**1550 unit/integration tests**, plus 29 Vulkan e2e (run on both radv and
lavapipe), 33 Wayland e2e, 29 X11 e2e and 1 CLI e2e. The whole workspace suite
passes with no Vulkan driver present at all, and the shell suites pass under
32-way CPU contention. The browser half adds two checks that need no browser:
`web/tools/check-exports.mjs` (Rust's declared symbols == the artifact's exports
== what the shim calls) and `web/tools/smoke.mjs`, which instantiates the
deployed artifact under node with every import stubbed and drives the documented
boot order.

### Known gaps, carried forward deliberately

- **XDND on X11** is not implemented (`ShellCaps::DRAG_DROP` is honestly clear
  there); owed before the editor's asset browser at P12.
- **One HAL seam finding remains open**, recorded in `crcbl-vk`'s crate docs:
  vertex pulling depends on `shaderDrawParameters`, for which the seam has no
  vocabulary.
- **Two shader stops, both closed, both worth remembering (P5.9, P5.13).** Every
  SPIR-V artifact the engine ships declares `OpCapability DrawParameters` —
  Slang emits it because `SV_VertexID` lowers to
  `gl_VertexIndex - gl_BaseVertex` — and naga does not implement it, so
  `crcbl-wgpu` had never created a shader module on any target. The WGSL to
  sidestep it had been committed since P5.3 and was consumed by nothing, because
  `ShaderModuleDesc` carried only SPIR-V. P5.9 gave the seam a WGSL half.

  Then a browser found the second one. Dawn enforces WGSL's uniformity rule
  where naga does not, and both UI shaders sampled the glyph atlas inside
  `if (input.uv.x > 0.0 || input.uv.y > 0.0)` — a branch on a varying:

  ```
  error: 'textureSample' must only be called from uniform control flow
  note: control flow depends on possibly non-uniform value
  ```

  The consequence was total rather than cosmetic: the invalid module invalidated
  the `ui compositing` pipeline, that invalidated the whole `breakout frame`
  command buffer, and `Queue.submit` dropped it — discarding the forward and
  tonemap passes too, so **the canvas stayed black while the game ran
  normally**. Hoisting the sample above the branch and selecting afterwards is
  the same image with no non-uniform sample; Vulkan's output is unchanged,
  verified by the cross-backend PNGs staying byte-identical.

  The lesson is the one this project keeps re-learning: **a second
  implementation is the only thing that finds this class of bug.** Both stops
  were invisible to a green native run, and neither would have been found by
  reading the code.

- **`crcbl-wgpu` cannot see a pipeline it failed to create.** WebGPU delivers
  creation failures to the device error callback, not to the call, and nothing
  pushes an error scope or installs an uncaptured-error handler. The run that
  found the shader bug submitted 384 invalid command buffers while reporting a
  healthy status and a page that said "Playing". Every browser-only pipeline
  failure will present as a black canvas with the game apparently running —
  which is exactly how this one presented. Owed before the demo site carries a
  second sample.

- **`VUID-vkAcquireNextImageKHR-fence-10066` fires once per acquire on Linux
  under wgpu**, and is not reachable from this workspace: the fence is created
  and reused inside `wgpu-hal`'s `NativeSwapchain`, which waits on and resets it
  only under `cfg(windows)`. 30.0.0 is the newest published version. Frames are
  correct and the runs pass; recorded rather than silenced by turning wgpu's
  validation off.

- **RenderDoc capture** has not been verified by hand; every object and pass
  carries a debug label and `DEBUG_MARKERS` is requested.
- **A green local Vulkan run is weaker than CI's.** Synchronisation validation
  has three reaches, and many installed layers report hazards at record time and
  within one submission but **not across submissions** — which is where every
  cross-frame hazard lives. `run-vk-e2e.sh` measures its own reach, prints it,
  and says so loudly when the run is weaker than the CI job it stands in for.
  The real gate for that bug class is the no-GPU cross-frame graph suite, which
  runs everywhere.
- The seam does **not** freeze until P5 exit, when a second backend
  (`crcbl-wgpu`) implements it. Changes before then are expected and cheap.

### How work proceeds (read this before starting a phase)

Phases are cut into **slices** small enough to review in one sitting. Each slice
is a branch, verified in full, merged fast-forward into `main`, pushed, and
watched through CI before the next one starts. A slice is not done when it
compiles; it is done when the gates below are green and CI agrees.

**The gates.** Every one of these must pass before a merge, and CI runs all of
them:

```
cargo build --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo machete
cargo deny --all-features check
```

**The e2e harnesses.** These need real display servers or a GPU, so they are
feature-gated and `#[ignore]`d — a plain `nextest` run skips them, which is why
each has a script that sets the environment up and **fails if zero tests ran**:

| Harness                                       | What it needs        | Tests |
| --------------------------------------------- | -------------------- | ----- |
| `crates/crcbl-shell/tests/run-wayland-e2e.sh` | nested headless sway | 33    |
| `crates/crcbl-shell/tests/run-x11-e2e.sh`     | Xvfb                 | 29    |
| `crates/crcbl-vk/tests/run-vk-e2e.sh`         | any Vulkan ICD       | 26    |
| `crates/crcbl-wgpu/tests/run-wgpu-e2e.sh`     | a wgpu adapter, Xvfb | 7     |
| `crates/crcbl/tests/run-cross-backend-e2e.sh` | both backends        | 2     |
| `crates/crcbl-cli/tests/run-cli-e2e.sh`       | nothing              | 1     |
| `web/run-browser-e2e.sh`                      | Chrome + Xvfb        | 18    |

`web/run-browser-e2e.sh` is the P5 gate itself and needs no GPU: it serves the
built site, drives it in a real browser over the DevTools protocol, sends a real
click and a real Space key, and **reads the canvas back** to prove the frame is
neither blank nor still. Its header carries the measured table of which
display/adapter combinations can report canvas pixels at all — three of the four
obvious ones cannot, silently — and it runs a known-colour clear as a control
before it believes any render result.

The cross-backend row counts **comparisons**, not `#[test]`s: it renders one
frame through each backend per size and compares the pair with `crcbl-golden`'s
measured tolerance, and it fails when zero comparisons ran for the same reason
the others fail when zero tests ran. Cross the ICDs (`CRCBL_VK_ICD` and
`CRCBL_WGPU_ICD` pointing at different drivers) to run it in the configuration
the tolerance was measured in; CI has only lavapipe and says so.

**Two runs that are not optional**, because both have caught bugs a normal run
could not:

```
# No GPU at all — the plain `test (linux)`, macOS and Windows CI condition.
VK_ICD_FILENAMES=/nonexistent.json cargo nextest run --workspace --all-features --locked

# Under contention — a shared CI runner is far slower than a dev box.
for i in $(seq 1 32); do (while :; do :; done) & done
./crates/crcbl-shell/tests/run-x11-e2e.sh
jobs -p | xargs -r kill
```

**Lessons this project has already paid for.** Each of these was a real CI
failure that a green local run had reported as fine:

- **The dev machine is the unusual one.** A long uptime hid a signed-arithmetic
  bug in the X11 event clock; a local GPU hid a missing-ICD failure; a Linux
  checkout hid a CRLF hash mismatch on Windows. If a test can only fail on a
  machine unlike this one, it is not yet a test.
- **A check that cannot run is not a check.** Sync validation was enabled for
  two phases without actually being on; a golden gate conditional on an optional
  shader compiler would have run nowhere; a test-count guard silently matched
  nothing because CI colours its output. Verify the checker, not just the code —
  the pattern is to break the thing deliberately and confirm the test notices.
- **A flaky test is a bug.** `.config/nextest.toml` says so and CI adds no
  retries. Two "flakes" turned out to be a phantom key-release under load and a
  missing cross-frame barrier.
- **Poll for the condition, never sleep**, and demand a _new_ event rather than
  accepting a stale one. Both harnesses do this; new tests must too.

**Conventions.** Conventional Commits; stage explicit paths, never `git add -A`
blindly; nothing above `crcbl-hal` may name a Vulkan type; every `unsafe` block
carries a `SAFETY:` comment naming its invariant; and a seam gap found mid-slice
gets recorded in the relevant crate's docs rather than worked around silently.

## Phase table

| Phase      | Build                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | From stage        | Gate / deliverable                                                                 |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------------- |
| **P0** ✅  | Workspace, CI, **test infra** (nextest, coverage, NullBackend suite — topic 12), `crcbl-cli` scaffold (`new`/`run`/`build` — topic 11), `crcbl-core` (handles, pools, `WorldPos`, clock, input), HAL seam + NullBackend, `crcbl-shell` (topic 15): Wayland (wayr) + X11 backends, window loop                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | 1, 11, 12, 15     | Window opens; CI green                                                             |
| **P1** ✅  | `crcbl-vk`: device, swapchain, frames-in-flight, render graph, pipelines; milestone ladder → lit mesh; ortho/2D path; offscreen render + `crcbl screenshot` + lavapipe golden-image e2e                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | 2, 11, 12         | Lit spinning mesh, zero validation errors, golden frame in CI                      |
| **P2** ✅  | `crcbl-ecs` (system-owned arrays), `crcbl-net` (transport trait, in-memory), `crcbl-server`/`crcbl-client`, replication, interpolation, tick determinism; `crcbl sim --hash` harness; **action-map input layer** kb/mouse on its own input thread w/ stacked tick states (topics 19+21); **tick-id protocol + client tick alignment** (topic 21); `crcbl-store`: `StorageSource`, settings layers, save/load container over snapshots + atomic writes (topic 14); `GameModule` API + static binding (topic 16); `.crpl` replay writer/reader + FileTransport playback (topic 22); protocol foundations: handshake+schema-hash gate, session reconnect, condition simulator, decode hardening, sector-scoped envelope + ack-baseline deltas (topic 23)                                                                                                                                                                                                                        | 4, 11, 12, 14, 16 | Server-simulated entities render via interpolation; save→load→hash green           |
| **P3** ✅  | `crcbl-phys` slice 1 (L0): box/sphere colliders, BVH, ray/segment, swept-sphere TOI + contact normals                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | 5                 | Physics unit+property suite + debug draw                                           |
| **P4** ✅  | `crcbl-ui` slice 1: draw-list pass, glyph atlas text, label/button, HUD basics; draw-list snapshot tests; black-box replay ring + crash dump + `crcbl replay` CLI (topic 22)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 7, 12             | Text + score HUD renders through engine                                            |
| **P4A** ✅ | `crcbl-audio`: device seam (cpal), audio thread, mixer/buses, WAV/QOA, voices, **full spatial cue grammar** (topic 13), server-event wiring, golden-buffer e2e                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | 13                | Grammar-trainer orbit test audible + green e2e                                     |
| **S1** ✅  | **Sample: breakout** — first playable; all deps (P0–P4A) exist. **12 slices merged**: scaffold, paddle+input, ball+physics (swept-sphere CCD), bricks+scoring+game states, audio (sine-wave bounce/brick-break), spatial panning via `compute_cue`, client-interpolated ball state with drift detection, high score persistence via `crcbl_store::write_atomic`, determinism tests, input-queue fix, launch-speed fix, persistence RTT, render-graph UI compositing pass (glyph atlas, per-frame vertex/index upload, alpha-blended on-screen HUD with score/lives/state).                                                                                                                                                                                                                                                                                                                                                                                                   | —                 | Winnable/losable native game with on-screen HUD                                    |
| **P5**     | **Wasm early**: `crcbl-wgpu` Tier B backend, canvas/rAF platform, Slang→WGSL, `FetchSource`-lite, AudioWorklet output; cross-backend image compare (vk↔wgpu); **GitHub Pages demo site + CI deploy**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | 10 (part), 12, 13 | **breakout playable in browser at the Pages URL**                                  |
| **P6**     | Phys slice 2: dynamic BVH churn, overlap queries, L1 integrator start (thrust, damping)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | 5                 |                                                                                    |
| **S2**     | **Sample: asteroids**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | —                 | Native + published web demo                                                        |
| **P6A**    | Wasm module host (topic 16): `wasmtime` behind `WasmHost` seam (NaN canonicalization, fuel limits), Rust guest SDK, component schemas                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | 16                | breakout-as-`.wasm` equivalence gate: state hash == static build, native + browser |
| **P7**     | GPU-driven rendering full: geometry pools, instance deltas, GPU culling, indirect draws — Tier A _and_ Tier B paths; **HDR target + tonemap + FXAA; sun CSM shadows** (culling-integrated, topic 18); **LOD mechanism**: chains + cull-shader selection + hand-LOD import (topic 25)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | 3                 | 10k-instance sandbox, flat CPU cost, both backends                                 |
| **P8**     | Phys slice 3: batch queries at scale, sleeping/islands pressure; **crcbl-jobs core** (topic 21): work-stealing pool, par_for deterministic mode, mailbox/ring primitives, ECS parallel schedule, pipeline threads + timeline profiler; threads-1-vs-N hash test in CI                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | 5                 |                                                                                    |
| **S3**     | **Sample: horde**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | —                 | 10k enemies @60; perf numbers recorded; web demo (smaller budget)                  |
| **P9**     | Assets + scenes: `AssetSource` (Dir/Fetch), glTF import, RON scene format, hot reload; **material templates+instances + render↔surface link + `mat check` lint** (topic 37); `crcbl import`; glTF corpus e2e                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 6, 11, 12         | Sponza-class scene through full path                                               |
| **P10**    | UI slice 2 + debug tools: widget set, panels/splitters, profiler HUD, inspector, console, **UI inspector**; music streaming + ducking + cue inspector; audio occlusion; **bloom** (18); **gamepad evdev+web + rebind UI** (19); **UI focus + spatial pad/kb navigation** (7 — arrows/WASD 1:1 dpad); **time-scrub debugger + replay browser + CI determinism verifier** (22)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 7, 13             | Debug overlay live over loaded scene                                               |
| **S4**     | **Samples: hud complete + viewer** (hud skeleton exists since P4 as the UI fixture; viewer = native tool, web build stretch)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | —                 | Widget gallery + themes golden-framed; arbitrary glTF opens, panels work           |
| **P11**    | Phys slice 4 (L1 full): sector frames + SOI, gravity/drag/atmosphere, Kepler on-rails, bubbles, timewarp                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | 5                 | Physics stage exit nears                                                           |
| **S5**     | **Sample: orbit** — physics acceptance test                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | —                 | Full mission; **flashiest web demo**                                               |
| **P12**    | Editor: edit-mode schedule, viewport+picking (phys raycast), outliner, properties, gizmos, undo commands, play mode; `crcbl scene`/`edit --serve` CLI protocol + command/undo property suite                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 8, 11, 12         | Scene authored start-to-finish in editor **and** modifiable headless via CLI       |
| **S6**     | **Sample: towers** (flagship) — solo loop first, then editor-authored map                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | —                 | Solo TD on editor-built map; web demo                                              |
| **P13**    | **UDP + own reliability layer** (acks/resend/fragmentation/tokens) + WebTransport + WebSocket (topic 23); auth-tier handshake plumbing (topic 27); quantization + priority/budget encoder; dedicated headless server; lobby-lite; **live spectator relay + broadcast delay** (22)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 10 (rest)         | Browser client ↔ native server                                                     |
| **S6+**    | **towers co-op** — marquee demo                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | —                 | 4-player mixed native/browser session                                              |
| **P14**    | Metal + DX12 backends (MoltenVK spike gate first)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 9                 | Sandbox + samples on macOS/Windows                                                 |
| post       | **Wave 1**: skeletal animation (17) + **player kit** (30 — puppet adopts it) → **puppet** sample; particles/VFX (20) → **sparks** workbench + towers/horde retrofit; gamepad XInput/GameController (19); on-screen touch controls; TAA + auto-exposure + point shadows (18). Also wave 1: QEM auto-LOD generation (25); projected+parallax decals (33); UI drag-drop capability (34). Then **Wave 2**: **prediction + lag comp** (26), navigation (24), HLOD sector proxies (25), **contact solver L2+L3** (36) → **ragdolls** (35) → **arena** (forcing function for 26+24, fairness harness), L3 joints, auth tiers PSK+token w/ crcbl-mint (27), QUIC native, packaging. **FPS-era** → **breach** sample (5v5 comp shooter, sample 11): ballistic penetration + kinetic impact (28), first-person rendering (29), player kit (30), visibility culling + integrity gate (31), auth tier 3 (27), VOIP (32), decal carve tier (33), grid-inventory kit (34), weapon kit (38) | 5, post           |                                                                                    |

## Demo site (GitHub Pages)

- `gh-pages` deploy workflow: on main push, build all wasm-ready samples
  (`wasm32-unknown-unknown` + bindgen), assemble static site (index page listing
  demos + engine README blurb), deploy to `https://crcbl.kryptic.sh/`.
- Every wasm sample = a Pages demo from the moment it exists; the site grows one
  demo per S-phase. Broken wasm build = broken CI = blocked merge — the browser
  target can't rot.
- The demos are a **subdomain, not a subpath under the org site**, and that is
  deliberate. `www.kryptic.sh` is prose built from one template by a stdlib
  Python script; this site is build output — `cargo build --target wasm32`,
  `wasm-bindgen`, committed shader artifacts, and a headless-Chromium gate.
  Folding it into the org site would put that whole toolchain into the website's
  CI and make every demo change wait on a second repo's deploy. The subdomain
  keeps the pipeline where the code is. `www.kryptic.sh/crcbl/` remains the
  project's landing page and links here.
- `web/CNAME` carries the domain into the published artifact and the deploy
  workflow fails without it: a missing `CNAME` file silently reverts the custom
  domain on the next deploy, so it is checked rather than assumed.

## Cross-cutting tracks

Five pillars deliver in slices across every phase (their own docs carry the
slice tables):

- **CLI/headless** ([11-cli-headless.md](11-cli-headless.md)) — `crcbl` binary
  grows a phase at a time; every subsystem is scriptable the phase it lands.
- **Testing** ([12-testing.md](12-testing.md)) — unit + property + e2e per
  subsystem, in the same phase as the subsystem, never later. Golden
  images/buffers, determinism hashes, lavapipe CI.
- **Audio** ([13-audio.md](13-audio.md)) — spatial cue grammar lands P4A, before
  the first sample; every sample ships with directional sound.
- **Persistence** ([14-persistence.md](14-persistence.md)) — settings +
  save/load ride the P2 snapshot machinery; profiles at P4 (breakout high
  score); OPFS wasm at P5; settings UI at P10; co-op world save/resume proven in
  towers (S6).
- **Debug tools** (in [07-ui-debug.md](07-ui-debug.md)) — each system lands with
  its overlay/inspector hooks.

## Notes

- Stage-doc numbering (01–10) is **topic identity, not order**. Topics 11–13
  (CLI, testing, audio) are cross-cutting tracks. Where this roadmap and a stage
  doc disagree on sequencing, the roadmap wins.
- Physics slices P3/P6/P8/P11 are the demand-driven delivery from
  [05-physics.md](05-physics.md); the slice↔sample mapping there matches the
  S-phases here.
- Sample docs numbered in build order: 01 breakout, 02 asteroids, 03 horde, 04
  hud (P4 skeleton → P10 complete), 05 viewer, 06 orbit, 07 towers, 08 arena, 09
  puppet (post-MVP wave 1 — animation/input/shadows showcase).
- HAL freeze moves in practice to P5 exit: the seam isn't frozen until _two_
  backends (vk + wgpu) implement it — earlier and stronger than the old "freeze
  at stage 2 exit", superseding it.

## Corrections (design review, 2026-07-27)

- **P2 is split into three gated sub-phases.** As written it held ECS,
  transport, server/client, replication, interpolation, determinism harness,
  input thread + tick stack, tick-id protocol, `crcbl-store`, `GameModule` API,
  `.crpl` writer/reader, and the whole protocol foundation — more scope than
  P0+P1 combined behind one modest exit, with several architecture-shaping
  decisions riding along invisibly:
  - **P2a — sim core**: `crcbl-ecs`, `crcbl-server`/`crcbl-client`,
    interpolation, tick-id protocol + client tick alignment, input thread +
    `InputTickState`, determinism harness (`crcbl sim --hash`). _Exit_:
    server-simulated entities render via interpolation; hash stable.
  - **P2b — protocol foundations**: transport trait + InMemory, handshake +
    schema-hash gate, session/reconnect, sector-scoped envelope, **ack-baseline
    deltas incl. entity-removal encoding and delta-base-too-old keyframe
    fallback**, encoded-space change detection, condition simulator, decode
    hardening + fuzz corpus. _Exit_: replication survives scripted loss/reorder
    with state equality.
  - **P2c — durability + modules**: `crcbl-store` (settings, saves as sector-set
    snapshots), `GameModule` API + static binding, `.crpl` tick-linear replay
    writer/reader + `FileTransport`. _Exit_: save→load→hash green; a recorded
    session replays identically.
- **P4A gate wording**: the audible gate is **cue grammar rules 1–4**; occlusion
  (rule 5) needs the physics BVH and lands at P10 per topic 13's own delivery
  table. A gate must be checkable when it's claimed.
- **HDR from P1**: render to `RGBA16F` + trivial tonemap from the first lit mesh
  so breakout/asteroids goldens and web demos aren't re-blessed wholesale when
  18's stack lands at P7.
