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
