# Stage 2 — Vulkan Backend (Linux)

`crcbl-vk`: ash implementation of the HAL, plus the render-graph layer in
`crcbl-render` that everything above draws through. Ends with a lit mesh on
screen, drawn through the graph, with GPU timers visible.

## Goals

- Full HAL implementation on Vulkan 1.3 (dynamic rendering, sync2, no legacy
  render passes).
- Render graph in `crcbl-render` with automatic barrier/layout handling.
- Frames-in-flight loop with correct sync — boring, correct, measured.
- HAL seam frozen at stage exit.

## Baseline requirements

Target Vulkan 1.3 core + these features as hard requirements (all fine on Linux
mesa/NVIDIA of the last several years):

- `dynamicRendering`, `synchronization2`, `timelineSemaphore`
- `descriptorIndexing` (full bindless set: partially bound, update-after-bind,
  runtime descriptor arrays) — stage 3 depends on it, enable now
- `bufferDeviceAddress` — GPU pointers for per-draw data, stage 3
- `drawIndirectCount`
- `maintenance4`

No fallback paths for missing features in MVP. If the device lacks them, error
clearly and exit. Fallbacks are post-MVP scope creep.

## Tasks

### 2.1 Instance / device / queues

- Instance with validation layers in debug builds; debug-utils messenger routed
  into `log` with severity mapping. Object naming
  (`vkSetDebugUtilsObjectNameEXT`) wired through the HAL's debug-name calls —
  names show up in RenderDoc from the first triangle onward.
- Physical device selection: prefer discrete, check required features, log the
  choice. Single graphics+compute queue for MVP; async compute is post-MVP but
  the HAL already models queues plural.
- VMA-style allocator: use `gpu-allocator` (proven in the Rust ecosystem) rather
  than hand-rolling suballocation. Wrap it behind the HAL memory types
  (`DeviceLocal`, `HostVisible` upload, `Readback`).

  **Not what shipped, and deliberately so.** `crates/crcbl-vk/src/mem.rs` cites
  this line and defers it: `crcbl-vk` takes no dependency on `gpu-allocator`,
  and does one `vkAllocateMemory` per resource. Its header gives the reason —
  device bring-up only ever needs the offscreen swapchain's images and a staging
  buffer, and a per-resource allocation has no fragmentation behaviour to get
  wrong — and names the point it stops working: `maxMemoryAllocationCount` is
  guaranteed to be only 4096, which is a ceiling the engine reaches as soon as
  it allocates per mesh. Memory-type _selection_ is the part that is built and
  tested. The suballocator itself is still owed, and this line is still the plan
  for it.

### 2.2 Swapchain + frame loop

- Swapchain with resize/out-of-date handling, mailbox preferred, FIFO fallback.
- Frames-in-flight (2): timeline semaphore for frame pacing, binary semaphores
  for acquire/present. Per-frame: command pool, transient bump allocator,
  descriptor recycling, deletion queue (resources retire N frames later).
- This loop is the reference implementation of "correct Vulkan sync" for the
  whole project — document invariants in code.

### 2.3 Pipelines + shaders

- SPIR-V loading; shaders written in Slang, compiled to SPIR-V at build time
  (build.rs) with a runtime-recompile dev path for stage 6 hot reload.
  - Why Slang over GLSL: first-class SPIR-V target, HLSL-compatible syntax eases
    the DX12 stage, good buffer-device-address support.
- Graphics + compute pipeline creation from HAL POD descriptors; pipeline cache
  persisted to disk.

### 2.4 Render graph (`crcbl-render`, above the seam)

- Declarative pass graph: passes declare reads/writes of virtual resources;
  graph compiles to ordered passes + exact `sync2` barriers + image layout
  transitions. No manual barriers outside the graph, ever.
- Transient resource pool: graph-owned images/buffers aliased across
  non-overlapping passes.
- Graph debug output: dump pass order + barriers as text (debug-tools principle
  — the graph must be able to explain itself).
- GPU timestamp per pass, exposed as a frame-timing report (feeds the stage 7
  profiler HUD).

### 2.5 First pixels

Milestone ladder, each a sandbox commit:

1. Clear color through the graph.
2. Triangle (vertex pulling from a storage buffer — no vertex input state; this
   is the pattern stage 3 scales up).
3. Depth-tested spinning mesh (hardcoded cube/sphere), simple perspective camera
   in a uniform buffer.
4. Basic forward lit pass (single directional light, Lambert+Blinn) — enough to
   see geometry properly; real material model comes with stage 3/5.
5. Orthographic camera mode proving the 2D story (z = z-index) is just a
   projection matrix swap.

## Exit criteria

- Sandbox renders the lit mesh at stable frame rate; resize, minimize, vsync-off
  all correct; zero validation errors/warnings.
- RenderDoc capture shows named objects and passes.
- Graph dump readable and correct for the sandbox frame.
- HAL seam frozen — stage 9 backends implement it as-is; changes after this
  point need explicit justification.
- CI: shader compilation in build, `crcbl-vk` unit tests (allocator retire,
  graph compile) green. Lavapipe/swiftshader smoke test in CI if practical;
  otherwise graph-compile tests stay CPU-only.

## Risks

- **Sync bugs.** Mitigation: sync2 only through the graph, validation layers
  with sync-validation enabled in CI runs of the sandbox where possible.
- **Slang toolchain friction in build.rs.** Fallback: check in compiled SPIR-V
  alongside sources until the toolchain story is smooth.
- **Graph over-engineering.** MVP graph = linear pass list with computed
  barriers. No multi-queue scheduling, no reordering. Resist.

## Corrections (design review, 2026-07-27)

- **Reversed-Z is LOCKED** (was missing entirely): depth buffer is `D32_SFLOAT`,
  projection uses an **infinite far plane with reversed depth**, compare op
  `GREATER`, clear to 0.0. A sector-tiled world with 300 m+ sightlines z-fights
  immediately on a conventional 0..1 buffer, and retrofitting after P1
  invalidates every blessed golden frame. This binds projection math, the
  viewmodel depth-slice remap (29), soft particles and depth collision (20), and
  every AA/post input (18).
- **HDR target from P1, not P7**: render to `RGBA16F` + a trivial tonemap pass
  from the first lit mesh, even with no HDR content. Costs nothing now and
  avoids re-blessing breakout/asteroids goldens (and their web demos) when 18's
  real stack lands.
- **Transfer queue**: MVP uploads share the graphics+compute queue — stated, not
  assumed. The render graph's barrier model must nonetheless represent
  queue-family **acquire/release** from the start so a dedicated transfer queue
  is additive later rather than a barrier-model rewrite.
- **HAL freeze wording**: the seam is **provisional** at stage-2 exit and
  **frozen at P5 exit**, when a second backend has implemented it — that backend
  is `crcbl-webgpu`; this said `crcbl-wgpu`, which was deleted 2026-08-21.
  ROADMAP is canonical; this doc's earlier "frozen at stage exit" is superseded.
- **Lavapipe golden-image e2e is a hard P1 gate**, not "if practical" — a gate
  cannot be optional.

## Shader portability (2026-08-09) — one source, four targets, and the gaps

§2.3 chose Slang. `crates/crcbl-shaders/tools/compile-shaders.sh` emits four
artifacts from each `.slang` source — SPIR-V, WGSL, MSL and DXIL — all committed
with a SHA-256 manifest, so `cargo build` needs no shader compiler on any
platform and `--check` catches drift in CI. That much works, and it is stronger
than the comparable engines: neither bevy nor Godot validates shaders offline at
all, and both discover shader errors at runtime, per permutation, on a user's
machine.

What follows are the gaps that survey turned up, and the rules that close them.

### Slang stays; a home-grown shading language was declined

The problems are **API gaps, not language gaps** — Metal has no ray tracing in
Slang, WebGPU has no bindless — and a language of our own would give neither. It
would also buy only the front half: DXIL must go through `dxc` and be signed by
`libdxil.so`, and MSL is compiled by Metal at run time. This is the same call
topic 16 makes for `wasmtime` and topic 7 makes for font parsing: a
standards-compliance surface that is not this project's learning goal.

**What would reopen it:** Slang blocking us a second time on something with no
workaround. Recorded in `docs/backlog.md` rather than argued again here.

### Rules

1. **Every shader declares which targets it must support**, and the compile
   script **fails** when a required target will not take it. A ray-tracing or
   mesh shader has no WGSL form at all and must say so rather than emitting a
   broken artifact — which is what happens today, silently, when Slang drops an
   attribute it cannot express.
2. **Per-target `-D` defines**, passed by the compile script, because Slang
   defines no target macro of its own — probing found only `__SLANG_COMPILER__`.
   Without them the only way to differ per target is to fork the file, which is
   what `ui_tier_b.slang` was, before it was deleted: a twin whose sole
   substantive difference was `[[vk::push_constant]]` versus a bound binding,
   held in step by a comment. **A forked shader is never the answer** — the
   uniform-buffer form `sprite.slang` uses is preferred where it is free, and
   the defines are what cover the rest.
3. **Declaration order must equal binding order**, enforced by a test that
   parses the sources. Slang's Metal target **ignores `[[vk::binding]]`** and
   assigns indices in declaration order, while `crcbl-mtl` binds by ascending
   `(set, binding)`. When `ui.slang` disagreed with itself, its MSL put the
   constants where the vertex buffer should have been and the UI pass drew
   nothing on macOS.
4. **Validate all four artifacts, not one.** A validator per target, because an
   artifact nothing reads is an artifact nothing checks — with the caveat that
   naga accepting a WGSL module is not Dawn accepting it, which is exactly how
   the uniformity bug shipped.
5. **Semantic divergence is caught by rendering, not by reading.**
   `SV_InstanceID` lowers to `InstanceIndex - BaseInstance` on SPIR-V and to a
   bare `@builtin(instance_index)` on WGSL; the source compiles cleanly to both
   and draws different pictures, which is why every batch after the first
   rendered the first batch's instances. No lint can find this class. The only
   thing that can is `web/run-cross-backend-e2e.sh` — **extend it to every
   engine shader and every backend**, which is also what sample rule 12 asks of
   the samples.

> **Where the five rules stand.** The first four are built and gated:
> `crates/crcbl-shaders/tools/compile-shaders.sh` enforces the single
> `// crcbl-targets:` declaration and passes the per-target `CRCBL_TARGET_*`
> defines, `crcbl-shaders`'s `declaration_order` module parses every source for
> rule 3, and rule 4's validators are `spirv-val`, `crcbl-shaders`'s
> `wgsl_validation` test, `xcrun metal -c` over every committed `.metal` in
> `ci.yml`'s `mtl e2e` job, and a signed-container assertion on every DXIL
> artifact — an unsigned one compiles, hashes and commits happily and is then
> refused by every real driver, WARP included.
>
> **Rule 5 is half done.** `web/run-cross-backend-e2e.sh` holds every scene the
> browser draws against a live native render, `--reference vk` and
> `--reference mtl`, so Metal is inside the compare. D3D12 is the one target
> still held against a golden alone, and the divergence class stays undetected
> for it.

### Considered, and reopenable: SPIR-V as the single native IR

Godot compiles GLSL to SPIR-V once and translates _that_ to MSL (SPIRV-Cross)
and DXIL (Mesa NIR). One lowering of the source semantics, so the divergence in
rule 5 is structurally impossible — it is fixed the moment glslang emits
`InstanceIndex`.

Not adopted now: it costs two vendored C/C++ translators, and the WGSL leg
cannot use it anyway — naga's SPIR-V frontend rejects the `DrawParameters`
capability every artifact here declares, which is why P5.9 carried WGSL across
the seam in the first place. Neither route solves Metal ray tracing.

**Reopen when** a Metal shader disagrees with its Vulkan twin a second time, or
when the differential gate above proves too coarse to localise one.
