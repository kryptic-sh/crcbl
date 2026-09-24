# Topic 59 — Tessellation: displacement baked into clusters, and patterns diced at run time

Written 2026-09-15, from a survey of the tree and a research brief on how
shipped engines tessellate now that the hardware stage is on its way out.
Nothing in this document is built. Its place in the set is
[18-render-features.md](18-render-features.md)'s index; the geometry system it
extends is topic 25's cluster DAG and stage 3's GPU-driven renderer (the rules
of both in [rendering notes](../notes/rendering.md)); the fixture that proves it
is [sample/25-relief.md](sample/25-relief.md).

**Tessellation here is not a shader stage.** WebGPU has no hull, domain,
geometry or mesh stage, and its vertex stage may bind only uniforms and
read-only storage — only compute can write geometry. The browser is a
first-class target, so tessellation is built from what every backend has:
geometry baked at import, and patterns chosen in compute and evaluated in the
vertex stage. This is also where the industry went: Unreal Engine 5 removed
hardware tessellation ("Hardware Tesselation has been removed from UE5", its
migration guide) and replaced it with Nanite Tessellation, which splits and
dices patches in compute and never touches the hardware stage.

## Where this is

Absent, and the tree has most of what it needs:

- **The cluster hierarchy is already crack-free.** Topic 25's construction locks
  each group's outer boundary while simplifying inside it, so any cut through
  the hierarchy meets itself on identical vertices. `build_cluster_dag`
  (`crates/crcbl-scene/src/cluster_dag.rs`) and `build_meshlets`
  (`crates/crcbl-scene/src/meshlet.rs`) build it deterministically, and
  `crates/crcbl-shaders/tools/cook-clusters.rs` commits a cooked hierarchy with
  a `--check` mode.
- **Quarry is already displacement baked at build time in all but name.**
  `apps/quarry/src/face.rs` bakes a hashed heightfield into a cluster hierarchy,
  and `apps/quarry/src/tile.rs` proves tiles meet across locked borders.
- **Selection differs by path**: the mesh-shader path chooses detail per
  cluster; `IndirectCount` and `IndirectPerBatch` choose one level per instance
  (`crates/crcbl-shaders/src/cluster_select.rs`, `level_select.rs`). A meshlet
  holds at most 64 vertices and 124 triangles.
- **The vertex pool is rewritten at run time only by skinning**, which binds it
  once as read-write storage because WebGPU refuses a read view and a write view
  of one buffer.
- **The browser's storage budget is already tight**: `draw_gen.slang` packs five
  tables into one binding to stay within eight storage buffers per stage.
- **Materials have no height slot.** Texture arrays exist for base colour,
  normal, metal/roughness/occlusion and emissive
  ([37-materials.md](37-materials.md)), and every layer of a page shares one
  size and format. Trilinear sampling has landed
  (`crates/crcbl-render/src/material_table.rs`).
- **Parallax occlusion mapping is refused** by topic 44 (recorded in the
  [rendering notes](../notes/rendering.md)) and by the gap survey's refusals in
  the same notes; the only planned height march is the decals' T1 tier
  ([33-decals.md](33-decals.md)).
- **The ~2 px triangle floor** topic 25 specified for the forward renderer is
  not implemented (`docs/backlog.md`, _LOD: the ~2 px triangle floor_), and it
  is the floor tessellation must respect.

## The decisions

### 1. Two rungs, and neither needs a stage WebGPU lacks

- **R0 — displacement baked at import.** A height texture and scale are applied
  when the asset is cooked: subdivide the welded base mesh to the height map's
  texel density, displace with bilinear sampling in plain arithmetic, and hand
  the result to `build_cluster_dag`. Simplification then measures the displaced
  surface, so the error metric and the crack-free cut apply unchanged and all
  three geometry paths draw it with no new run-time code. The cost is storage
  and bake time — Brian Karis's objection to it for Nanite is that "far more
  triangles and vertices will need to be stored and rendered" — which is why it
  is the first rung and not the only one.
- **R1 — pattern-table tessellation of the drawn cut at run time.** After
  cluster selection and culling, a compute pass computes a factor for each edge
  of each drawn triangle of a tessellated material, picks a pattern from a
  precomputed table, and appends a patch record into a fixed budget with an
  overflow counter. The vertex stage pulls the pattern's barycentrics, evaluates
  the smooth surface (Phong or PN triangles) and adds displacement sampled with
  `SampleLevel`, faded out as the triangle shrinks toward the pixel floor.

**Refused, with the reason**: a compute tessellator that reads its output size
back to the CPU (the DirectX SDK's `AdaptiveTessellationCS40` shape — a stall
natively and a frame late in a browser); generating clusters at run time
("running the Nanite build process in reverse", Karis); and, for now, concurrent
binary trees over longest-edge bisection (Dupuy, HPG 2020) — they are the right
answer for huge terrain, but they cost 0.5–1.5 ms of sum-reduction, no WGSL
implementation was found anywhere, and water's rings and quarry's tiles already
cover the terrain this engine draws. They are recorded as a later rung for a
terrain topic that needs them.

### 2. R1 is crack-free by edge-only factors, symmetric quantization and the cut's locked boundaries

Karis states the whole rule: "so long as the only data about a patch that
affects the placement of vertices on an edge is data about the edge itself,
those vertices will match". So:

- **An edge's factor comes from that edge alone** — its undisplaced world-space
  length projected to pixels, divided by the dice rate, clamped to the pixel
  floor.
- **Barycentrics are quantized symmetrically** — Nanite's patterns store them as
  integers from 1 to 65534 so rounding is the same from either end of an edge —
  and edge endpoints are evaluated in a canonical order.
- **Every drawn triangle of the material is tessellated, at whatever level of
  the cut it came from.** Where two levels meet, the shared edge is a locked
  group boundary with identical vertices on both sides, so its factor, its diced
  vertices and its displacement are identical too. **Tessellating only the
  finest level would crack** at every level boundary.
- **Normals must not split along an edge.** Phong and PN triangles both crack
  where a vertex carries two normals; R1 averages split normals for tessellated
  materials, or takes PN-AEN's second index set where an asset needs a hard
  edge. Epic ships Nanite Tessellation stating "there is currently no solution
  for crack-free displacement … UV seams, hard edge normals … will cause
  cracks"; this engine takes the dominant-UV rule from NVIDIA's DirectX 11 talks
  (McDonald, Dudash) instead of leaving it open, and it rests on the attribute
  and seam handling in QEM that quarry still owes (`docs/backlog.md`, _Three of
  the four QEM properties quarry claims to prove are not implemented_).

### 3. One record producer for every geometry path

Stage 3's rule (in the [rendering notes](../notes/rendering.md)) is that the
lesser path is a constraint on data layout, not a separate renderer, so the
patch records are produced once:

- **`IndirectPerBatch`** (the browser): records into a fixed-budget storage
  buffer; one `drawIndirect` per tessellated bucket with its `vertexCount`
  written by compute and zero in an unused slot; the vertex stage reads the
  pattern atlas and the records as read-only storage. That is two storage
  bindings, and `draw_gen.slang`'s packing is the precedent for fitting them
  under the ceiling.
- **`IndirectCount`**: the same records and argument layout.
- **`MeshShader`**: the same records, but a mesh shader emits a patch directly
  with no output buffer. A uniform n-segment triangle has (n+1)(n+2)/2 vertices,
  so **at most nine segments per edge fit a 64-vertex meshlet** (55 vertices, 81
  triangles); NVIDIA's `vk_tessellated_clusters` caps at eleven, which needs 78.

At 128 MiB per storage binding — WebGPU's default — and 16 bytes per record the
ceiling is about eight million records; the browser budget is set far below it
and the overflow count goes on the stats panel.

### 4. Culling, shadows and error with displacement

- **Bounds grow by the maximum displacement**, the same constant for every
  meshlet and group sphere, so a parent still contains its children and the
  hysteresis argument in `cluster_select.rs` still holds. Nanite's material
  Magnitude sets its bounds the same way.
- **Normal-cone culling is widened by the displacement's steepest slope** for a
  tessellated cluster, or switched off for it; a displaced surface can face the
  camera where its base triangle does not.
- **Group error includes the displacement magnitude** at R1, or the cut picks a
  level too coarse for what the tessellation will add.
- **Shadows**: the shadow cut is coarser by design. Tessellating a cascade at a
  coarser dice rate than the camera moves shadow edges off the surface the
  camera sees, so the near cascade uses the camera's dice rate and far cascades
  a coarse one with a depth bias; R0-baked geometry has no such mismatch.

### 5. No transcendental in the surface

Phong tessellation is a quadratic blend of three tangent-plane projections; PN
triangles are a cubic patch; bilinear displacement is arithmetic. None needs a
platform `sin`, `cos`, `exp` or `pow`, so R1 meets topic 44's shading rule
([rendering notes](../notes/rendering.md)) as written, and its GPU result is
equal across backends within the same bound every shading path already meets.

### 6. Height enters materials as its own page

**A height layer is a single-channel 16-bit texture array of its own**, not an
eight-bit alpha channel in an existing page: a page is one size and format, and
eight bits of height band visibly under grazing light. That grows
`GpuMaterial`'s stride by an index and a scale, which is a golden-visible change
and is priced when R0's import lands.

**glTF has no ratified displacement**: `KHR_materials_displacement` has been an
open proposal since 2017 (KhronosGroup/glTF issue 948) with no merged pull
request, `EXT_materials_bump` perturbs normals only, and the Khronos Sample
Viewer, three.js's loader, Babylon's loader and Bevy's loader all ignore height.
So height arrives through [37-materials.md](37-materials.md)'s material
instances first; reading issue 948's field names as an unofficial extension —
NVIDIA's sample does — is recorded as a decision below.

## The rungs

Each rung is priced on the desktop adapter, lavapipe and the browser before it
counts as built, per the pricing rule in the
[rendering notes](../notes/rendering.md) (_What the deleted 43-render-standards
plan left behind_), and runs on the WebGPU backend in the fixture's browser
demo.

| Rung  | What it buys                                                                                                                                                                          | What it costs                                                            | Needs                                                                |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| R0    | Displacement baked into clusters at import; real silhouettes, depth and shadows with LOD on every path; the height page                                                               | storage and bake time per asset; a material stride change                | the height page; QEM attribute and seam handling for textured assets |
| R1a   | Smooth silhouettes on coarse assets: Phong tessellation of the drawn cut, edge-only factors, the pattern atlas, patch records, one producer for all three paths, the overflow counter | a compute pass; a few MB of records; two storage bindings; more vertices | R0's culling-bound and cone changes                                  |
| R1b   | Run-time displacement on R1a's patches, faded by pixel size; shadows with per-cascade dice rates                                                                                      | a height sample per vertex                                               | R1a, the height page                                                 |
| R1c   | PN triangles and PN-AEN hard edges, where Phong's quadratic is too soft                                                                                                               | a second index set for hard-edged assets                                 | R1a                                                                  |
| later | Concurrent binary trees for huge terrain                                                                                                                                              | 0.5–1.5 ms of reduction; a first WGSL implementation                     | a terrain topic                                                      |

## What each rung is checked by

- **No cracks**: a golden of a tessellated sphere-like surface against a
  contrasting background, with a claim that no background pixel shows through
  the surface at any dice rate — sabotaged by making one edge's factor depend on
  its triangle, which must go red.
- **Composition with the cut**: a cut that mixes two levels over a tessellated
  surface draws no crack at the level boundary — sabotaged by tessellating only
  the finest level.
- **Bounds**: a displaced cluster at the frustum edge is not culled while its
  displaced geometry is on screen.
- **Budget**: overflowing the record budget drops patches visibly and counts
  them, rather than writing past the buffer.
- **Paths**: the same scene on every geometry path within the cross-path budget
  `render_e2e` already applies.
- **Price**: the tessellation compute and the extra vertex cost from `PassStats`
  on three tiers.

## Sources

Research brief, 2026-09-15.

- Karis — _How to tessellate_, _Nanite Reyes_ and _Possible approaches for
  tessellation_, Graphic Rants, 2026; Unreal Engine 5 migration guide and Nanite
  tessellation documentation.
- NVIDIA — `vk_tessellated_clusters` (nvpro-samples).
- McDonald — _Crack-Free Point-Normal Triangles using Adjacent Edge Normals_,
  GDC 2011; Dudash — _My Tessellation Has Cracks_, GDC 2012; Story, Cebenoyan —
  _Tessellation Performance_, GDC 2010; the PN-AEN whitepaper.
- Vlachos et al. — _Curved PN Triangles_, 2001; Boubekeur, Alexa — _Phong
  Tessellation_, 2008; Boubekeur, Schlick — _Generic Adaptive Mesh Refinement_,
  GPU Gems 3 chapter 5.
- Khoury, Dupuy, Riccio — _Adaptive GPU Tessellation with Compute Shaders_, GPU
  Zen 2; Dupuy — _Concurrent Binary Trees_, HPG 2020; Benyoub, Dupuy —
  concurrent binary trees for large terrain, 2024.
- Widmark — _Terrain in Battlefield 3_, GDC 2012; Strugar — _CDLOD_.
- Apple — Metal tessellation programming guide; gpuweb issue 445; wgpu issue
  222; the W3C WebGPU and WGSL specifications.
- KhronosGroup/glTF issue 948 (`KHR_materials_displacement`), pull requests
  2339, 2196 and 2273.

**Not read, and so not claimed**: the Boubekeur 2005 and 2008 papers directly
(the GPU Gems chapter was used), primary UE4 tessellation documentation, Unigine
Heaven and AMD demo specifics.
