# Stage 3 — GPU-Driven Rendering

Turn the stage 2 "draws a mesh" renderer into the infr-style GPU-bound renderer:
the CPU uploads deltas and records a near-constant command stream; the GPU
decides what actually draws.

## Goals

- Scene size decoupled from CPU cost: 10 objects and 10,000 objects record
  roughly the same commands.
- Host↔GPU round trips minimized: no per-object descriptor updates, no
  per-object buffer binds, no readbacks in the frame loop.
- Device capability decides which geometry and binding path is selected, with
  degradation monotonic and logged — see
  [39-capabilities.md](39-capabilities.md).

## Paths, not tiers

**The two-valued `Tier A` / `Tier B` model is superseded by
[39-capabilities.md](39-capabilities.md)**, which explains why: Metal has
multi-draw-indirect and no GPU-side count, D3D12 has both in the API, `wgpu` on
native reports nearly the full native set, and only WebGPU in a browser is the
thing "Tier B" was ever describing. What this stage owns is the two selectors
the renderer branches on:

| `GeometryPath`     | Emit tail                                                | Needs                  |
| ------------------ | -------------------------------------------------------- | ---------------------- |
| `MeshShader`       | per-cluster amplification → mesh dispatch (see 3.5)      | `MESH_SHADER`          |
| `IndirectCount`    | one indirect-count call per bucket                       | `DRAW_INDIRECT_COUNT`  |
| `IndirectPerBatch` | compacted instance list + one `draw_indirect` per bucket | nothing beyond compute |

| `BindingModel` | Material lookup                           | Needs                 |
| -------------- | ----------------------------------------- | --------------------- |
| `Bindless`     | full descriptor indexing, one array       | `DESCRIPTOR_INDEXING` |
| `ArrayPages`   | fixed-size texture array pages + batching | nothing               |

Buffer device address is not a selector: where it is absent the shaders use
indexed SSBO lookups, which is a data-layout rule rather than a second path.

Design rule, unchanged and now the load-bearing one: **the lesser path is a
constraint on data layout, not a separate renderer.** Global geometry pools,
instance buffers and material tables are laid out so every path consumes them;
only the emit tail and the material lookup differ.

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
- Material table (SSBO) + a bindless texture array (`BindingModel::Bindless`) or
  texture array pages (`ArrayPages`). Material id indexes the table; the table
  holds texture indices + factors.
- Camera/frame constants in one uniform buffer; everything else is storage
  buffers indexed by ids the GPU reads.

**2026-08 — the table landed, and its factors half only.**
`crcbl_render::MaterialTable` is the SSBO, `crcbl_shaders::mesh::GpuMaterial` is
a row, and `GpuInstance::material` indexes it: `mesh.slang`'s fragment stage
multiplies the row's `base_color` into the interpolated albedo at binding 6. So
the id, which had been reserved since the record was written, now means
something on all four targets.

**2026-08 — the lookup moved to the fragment stage, on its own, to find out
whether a flat integer varying lowers the same way everywhere.** It read the
table in the vertex stage first, which the texture half cannot do: a texture
fetch is not constant across a primitive, so nothing it feeds can be folded into
a varying at the corners. Moving the multiply needs the material id in the
fragment stage, and this file's two worst bugs — `SV_InstanceID`, `SV_VertexID`
— were both integers the four targets disagreed about, so the move was made by
itself, with no texture beside it. It commutes with interpolation, so no golden
was re-blessed: **the cube, sprite and UI frames are bit-identical on `vk` and
on `wgpu`, and so are `crcbl-vk`'s three mesh goldens.** All four targets emit
the flat qualifier — SPIR-V `OpDecorate … Flat`, WGSL `@interpolate(flat)`, MSL
`[[flat]]`, DXIL `nointerpolation` in the input signature — but only `vk` and
`wgpu` were _rendered_ here; `msl` and `dxil` are CI's verdict.

**There are no texture indices, deliberately.** A row would have to say whether
its index is a `Bindless` slot or an `ArrayPages` page, and the engine has
chosen neither; a column carried ahead of that choice is a field nothing reads,
which is the "premature material system" the risk list below warns about. The
factors half proves the whole indexing path — cull, draw generation, the
fragment stage, four backends — and is observable in
`crates/crcbl/tests/golden/cube.png`, where two instances of one mesh differing
in nothing but their material id are two colours. Adding the texture column
later is a field; the mechanism is already paid for.

The table is one buffer with no ring, unlike the instance array beside it: a
material is written when it is created, which is the mesh table's lifetime, so
the delta upload this section asks for applies to the instances and not to this.
An animated material is what makes it a ring.

### 3.3 GPU culling + draw generation

- Compute pass: frustum cull against instance AABBs → compacted visible instance
  list → `draw_indexed_indirect` records + count buffer.
- Per `GeometryPath`: `MeshShader` culls per cluster in the amplification stage
  and never builds a draw list at all (3.6); `IndirectCount` issues one
  indirect-count call per bucket; `IndirectPerBatch` walks the compacted list
  and issues one `draw_indirect` per bucket. **The cull pass itself is identical
  in all three** — only what consumes its output differs.
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

### 3.5 Meshlet geometry — the primary path (MVP)

**Mesh shaders are the primary geometry path, not an optimisation.** Every
native backend has them (`VK_EXT_mesh_shader`, D3D12 SM6.5 amplification/mesh,
Metal 3 object/mesh), Slang emits them for all three, and the paths in the table
above are what a device without them falls back to.

- **Meshlet build is a bake step** (topic 6, `crcbl import`): a mesh becomes
  clusters of a bounded triangle count with per-cluster bounds and a normal
  cone. Deterministic — same input hash, same clusters, so the bake cache and
  the golden-mesh tests in topic 12 work the way they do for every other cooked
  artifact.
- **Per-cluster culling in the amplification stage**: frustum, normal-cone
  backface, and (post-MVP) occlusion against the depth pyramid. The instance
  cull above survives unchanged and runs first — instance rejection is cheaper
  than cluster rejection and neither replaces the other.
- **Cluster LOD is the point.** Selection is per cluster rather than per
  instance, which is what lets one mesh be several detail levels at once across
  its own surface. This is why QEM auto-simplification moves into the MVP — see
  [25-lod.md](25-lod.md); a cluster hierarchy with nothing to select between is
  the culling win without the detail win.
- **The fallback is not second-class.** `IndirectCount` and `IndirectPerBatch`
  draw the same clusters as ordinary index ranges, selecting cluster LOD in the
  existing cull compute pass instead of in an amplification stage. Same
  geometry, same pools, same picture at a coarser selection granularity.

### 3.6 Debug instrumentation (built now, not later)

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
- **Every `GeometryPath` and `BindingModel` value renders the sandbox scene**,
  and a golden image exists per combination a backend actually selects. A path
  no device in CI selects is named as a coverage gap in `docs/backlog.md` rather
  than left to imply coverage — see [39-capabilities.md](39-capabilities.md).
- Capability flags and the derived selectors exist in the HAL; the lesser paths'
  data-layout constraints are documented in `crcbl-render`.

## Risks

- **The lesser paths forgotten until stage 10 → bindless assumptions
  everywhere.** Mitigation: capability flags + layout rules land now; CI
  grep-level lint for direct bindless use outside the `Bindless` material
  lookup. The real risk is not that they are unwritten but that they are
  unexercised — see the exit criterion above.
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
  an overflow counter. `IndirectPerBatch` emits the same buckets as per-batch
  indirect draws.
- **Transparent sorting**: GPU **radix sort over packed depth keys** (bitonic is
  the fallback for small counts) — named so it isn't rediscovered at P7.
- **Per-path shader authoring is one source**: Slang with per-path
  specialization; **each path selector is a permutation axis** with its own line
  in 37's permutation budget. Decided before any shader is written, because P1's
  shaders become P5's inputs. The mechanism is per-target `-D` defines from
  `crates/crcbl-shaders/tools/compile-shaders.sh` plus a declared target list
  per shader — a ray-tracing or mesh shader has no WGSL form at all and must say
  so rather than emitting a broken artifact.

## Correction (capability model, 2026-08-09)

**The two-valued tier is replaced by capability-derived path selectors**;
[39-capabilities.md](39-capabilities.md) is the canonical description and this
document defers to it. The tier model was written when the only two
implementations in view were Vulkan and WebGPU, and it stopped describing
reality once Metal, D3D12 and native `wgpu` each landed in a different place in
the capability space.

**Mesh shaders and ray tracing move into the MVP** (see §3.5 here and
[18-render-features.md](18-render-features.md)), which adds two independent axes
the tier model could not have carried at all: a device can have mesh shaders
without ray tracing, ray tracing without a GPU-side draw count, or — as Metal
does — mesh shaders and multi-draw-indirect but neither of the other two.
