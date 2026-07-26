# Stage 3 — GPU-Driven Rendering

Turn the stage 2 "draws a mesh" renderer into the infr-style GPU-bound renderer:
the CPU uploads deltas and records a near-constant command stream; the GPU
decides what actually draws.

## Goals

- Scene size decoupled from CPU cost: 10 objects and 10,000 objects record
  roughly the same commands.
- Host↔GPU round trips minimized: no per-object descriptor updates, no
  per-object buffer binds, no readbacks in the frame loop.
- Two renderer tiers defined (native full-featured vs portable/WebGPU), with the
  tier seam explicit from the start — see wasm stage (10).

## Renderer tiers

| Capability                  | Tier A (native vk/mtl/dx12) | Tier B (WebGPU/wasm)                         |
| --------------------------- | --------------------------- | -------------------------------------------- |
| Bindless resource arrays    | full descriptor indexing    | fixed-size texture arrays + batching         |
| Multi-draw-indirect + count | yes                         | one `draw_indirect` per batch, or instancing |
| Buffer device address       | yes                         | no — indexed SSBO lookups instead            |
| GPU culling                 | compute → indirect count    | compute → compacted instance list            |

Design rule: **Tier B is a constraint on data layout, not a separate renderer.**
Global geometry pools, instance buffers, and material tables are laid out so
both tiers consume them; only the "emit draws" tail differs. Decided per-device
via HAL capability flags (added to the HAL this stage).

## Tasks

### 3.1 Global geometry pools

- One large vertex pool + one index pool (device-local, suballocated,
  defragmented offline/on-load, not per-frame). Meshes are
  `{base_vertex, base_index, count}` ranges — a mesh handle is three integers.
- Vertex pulling everywhere (established in stage 2): position/attr streams as
  storage buffers, no pipeline vertex-input state to vary per mesh.
- Upload path: staging ring buffer + transfer submits with timeline semaphore
  tracking; renderer consumes meshes only after the timeline value passes.

### 3.2 Instance + material data

- `GpuInstance` array (SSBO): transform, mesh id, material id, flags. Written by
  delta upload each frame (changed instances only — dirty ranges, not full
  re-upload).
- Material table (SSBO) + bindless texture array (Tier A) / texture array pages
  (Tier B). Material id indexes the table; the table holds texture indices +
  factors.
- Camera/frame constants in one uniform buffer; everything else is storage
  buffers indexed by ids the GPU reads.

### 3.3 GPU culling + draw generation

- Compute pass: frustum cull against instance AABBs → compacted visible instance
  list → `draw_indexed_indirect` records + count buffer.
- Tier A: one `vkCmdDrawIndexedIndirectCount` per pass. Tier B: compacted
  instance list + fixed indirect draws per batch.
- Occlusion culling (depth pyramid / two-phase) is **post-MVP**; leave the
  compute pass structured so it can be inserted (visibility buffer slot in the
  pass inputs).

### 3.4 Sorting + passes

- Opaque pass sorted by pipeline/material via GPU-side binning (or CPU sort of
  batch headers only — batches, not objects). Transparent pass: depth-sorted;
  correct-enough for MVP.
- 2D/ortho content flows through the same instance path — a sprite/quad is a
  mesh range with an ortho camera; z is z-index (locked decision from overview).
  No separate 2D renderer.

### 3.5 Debug instrumentation (built now, not later)

- Per-pass GPU timestamps aggregated into a rolling frame report (feeds stage 7
  HUD).
- Culling stats readback (visible/total counts) on a delayed ring — the one
  permitted readback, N frames latent, debug builds only.
- Debug draw layer: line/AABB/sphere immediate primitives accumulated into an
  instance buffer, drawn in one pass. Systems from stage 4 onward use this to
  visualize themselves.

## Exit criteria

- Sandbox scene: 10k+ instanced meshes, camera fly-through, culling visibly
  working (stats HUD), CPU frame time flat vs instance count.
- Zero per-frame descriptor writes in steady state (RenderDoc-verified); zero
  frame-loop readbacks except the delayed debug ring.
- Tier flags exist in HAL; Tier B data-layout constraints documented in
  `crcbl-render` (even though the WebGPU backend arrives in stage 10).

## Risks

- **Tier B forgotten until stage 10 → bindless assumptions everywhere.**
  Mitigation: tier flags + layout rules land now; CI grep-level lint for direct
  bindless use outside the tier-A draw tail.
- **Suballocator complexity.** MVP: free-list + offline compaction on load only.
  No live defrag.
- **Premature material system.** Materials here are a table + textures. Shading
  models/graphs are post-MVP.

## Corrections (design review, 2026-07-27)

- **Camera-relative rendering vs delta-only instance upload** (these
  contradicted each other): instances store **sector-local f32 transforms + a
  sector id**, which are static while an object doesn't move — so delta upload
  survives camera motion. Per frame the CPU computes a small **sector→camera
  offset table in f64** (one entry per resident sector) and uploads only that;
  the vertex/cull shaders add the offset. This also defines the space cull AABBs
  live in. Must exist before P7.
- **Draw binning is a fixed bucket table**, not "GPU binning or CPU sort": at
  load, enumerate reachable `(material template, permutation, pass)` combos
  (37's declared permutations make this finite) into a bucket table; the cull
  shader scatters compacted instances into per-bucket indirect draws with
  per-bucket count buffers. Per-bucket capacity is sized from scene stats with
  an overflow counter. Tier B emits the same buckets as per-batch indirect
  draws.
- **Transparent sorting**: GPU **radix sort over packed depth keys** (bitonic is
  the fallback for small counts) — named so it isn't rediscovered at P7.
- **Per-tier shader authoring is one source**: Slang with a `TIER_B` capability
  specialization; **tier is a permutation axis** with its own line in 37's
  permutation budget. Decided before any shader is written, because P1's shaders
  become P5's inputs.
