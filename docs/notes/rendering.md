# Rendering — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`. GPU
skinning's rules are in `docs/notes/simulation.md` (_What the deleted
17-animation plan left behind_), beside the pose rules they share a pipeline
with.

### Considered and declined: a per-scene golden tolerance

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

`specular_aa` failed the browser gate on its first geometry — 4788 pixels over
`Tolerance::RASTERISER`'s two levels (9.7412%) against an allowance of one per
cent — and the obvious lever was a third tolerance in `crcbl-golden` that a
scene could name, so an awkward frame could be compared on its population rather
than pixel by pixel. **Not built, and now not needed.** The cause was the
fixture's geometry, not the comparison: its strips were 1.49 pixels wide and
landed at arbitrary sub-pixel positions, and Vulkan guarantees only four
`subPixelPrecisionBits`, so SwiftShader's sixteenth-of-a-pixel vertex grid put
each strip edge somewhere radv's eighth-bit grid did not. Sizing the strips to
exactly two pixels on integer columns — `screenshot`'s `SPECULAR_STRIP_PITCH`,
which asserts the property vertex by vertex — took the disagreement to **one
pixel over tolerance, max channel delta 3**. Anyone reaching for a per-scene
tolerance again should first check whether the fixture's own edges are on the
pixel grid.

## What the deleted 03-gpu-driven-rendering plan left behind (2026-09-24)

Record; the built part of the plan is `crcbl_render`'s `mesh_pool`,
`instance_pool`, `material_table`, `cull`, `draw_gen`, `cluster_pool`,
`occlusion_cull`, `timing`, `cull_stats` and `debug_draw`, with `cull.slang`,
`draw_gen.slang` and `mesh_cluster.slang`, and `apps/quarry` rendering each
`GeometryPath` against a golden. What it left open is in `docs/backlog.md` under
_`Bindless` has no implementation_, _No transparent pass, and therefore no depth
sort_, _Camera-relative rendering: the f64 sector offset table_, the per-cluster
occlusion bullet in _What occlusion culling shipped without_, _The GPU-driven
exit criteria have never been measured_ and the RenderDoc bullet in _Findings
the roadmap carried that nothing else did_. It specified stage 3: turning "draws
a mesh" into a GPU-driven renderer where the CPU uploads deltas and records a
near-constant command stream and the GPU decides what draws.

Code cites the plan as "topic 03 §3.5", "§3.3's second half" or "the 2026-07-27
correction". Those numbers resolve here:

| Citation                     | What it specified                                                                                                                                                                          |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Goals                        | Scene size decoupled from CPU cost (10 objects and 10,000 record roughly the same commands); no per-object descriptor updates or buffer binds, and no readbacks in the frame loop          |
| Paths, not tiers             | The two selectors, `GeometryPath` (`MeshShader`, `IndirectCount`, `IndirectPerBatch`) and `BindingModel` (`Bindless`, `ArrayPages`), replacing the old Tier A / Tier B pair                |
| §3.1                         | Global geometry pools: one vertex pool and one index pool, a mesh is three integers (`base_vertex`, `base_index`, `count`), vertex pulling everywhere, uploads gated on a timeline value   |
| §3.2                         | Instance and material data: the `GpuInstance` array written by delta upload, the material table, texture array pages or a bindless array, camera constants in one uniform buffer           |
| §3.3                         | GPU culling and draw generation: a compute frustum cull into a compacted visible list, indirect arguments and a count buffer; two-phase occlusion against the depth pyramid                |
| §3.4                         | Sorting and passes: opaque binning by material, a depth-sorted transparent pass, and 2D content through the same instance path with z as z-index                                           |
| §3.5                         | Meshlet geometry, the primary path: the meshlet build as a bake step, per-cluster culling in the amplification stage, cluster LOD over a DAG, and the non-second-class fallback            |
| §3.6                         | Debug instrumentation: per-pass GPU timestamps, the cull-stats readback on a delayed ring, and the debug draw layer                                                                        |
| Exit criteria                | 10k+ instanced meshes with CPU frame time flat against instance count; zero per-frame descriptor writes and zero frame-loop readbacks bar the ring; a golden per selected path combination |
| The 2026-07-27 correction(s) | Camera-relative instances, the fixed bucket table, GPU radix sort for transparency, and each path selector as a permutation axis — see the rules below                                     |

- **The lesser path is a constraint on data layout, not a separate renderer.**
  Pools, instance buffers and material tables are laid out so every path
  consumes them; only the emit tail and the material lookup differ. **The cull
  pass is identical on every `GeometryPath`**: `MeshShader` culls per cluster in
  the amplification stage and builds no draw list, `IndirectCount` issues one
  indirect-count call per bucket, and `IndirectPerBatch` issues one
  `draw_indirect` per bucket over the compacted list. Buffer device address is
  not a selector: without it the shaders use indexed storage-buffer lookups.
- **Mesh shaders are the primary geometry path, not an optimisation.** Every
  native backend has them (`VK_EXT_mesh_shader`, D3D12 SM6.5, Metal 3) and Slang
  emits all three; the other paths are what a device without them falls back to.
  **The fallback is not second-class**: the indirect paths draw the same
  clusters as index ranges and select cluster LOD in the cull pass instead, so
  the same geometry and pools give the same picture at a coarser granularity.
- **The meshlet build is a bake step**, deterministic: same input hash, same
  clusters, so the bake cache and the golden-mesh tests work as for every other
  cooked artifact. **The instance cull runs first** and survives beside the
  cluster cull: instance rejection is cheaper, and neither replaces the other.
- **The cluster hierarchy is a DAG, not a chain of levels** (locked 2026-08-12).
  A chain simplifies each level independently, so adjacent clusters at different
  levels crack along their shared edge; the build groups neighbours, locks each
  group's outer boundary while simplifying its interior, re-splits and repeats
  with different groupings, so every cut is crack-free. Cluster LOD is the point
  of the hierarchy — a hierarchy with nothing to select between is the culling
  win without the detail win — which is why QEM simplification moved into the
  MVP.
- **Draw binning is a fixed bucket table**, not a sort. The cull scatters
  compacted instances into per-bucket indirect draws with per-bucket counts,
  capacity sized from scene stats with an overflow counter, and
  `IndirectPerBatch` emits the same buckets. **The key today is
  `(resident mesh, material mode)`** — the mesh because an argument structure's
  index range is per draw, the mode because the depth prepass and the shadow
  atlas bind a pipeline per bucket — and it grows to
  `(material template, permutation, pass)` as the same table with a longer key.
- **A page is one image**: every layer of the `ArrayPages` texture array shares
  an extent, a format and a mip count. That is the constraint `Bindless` exists
  to lift, and real imported content does not have one extent. `Bindless` was
  not built because `crcbl-mtl` withdraws `DESCRIPTOR_INDEXING`, so a bindless
  lookup would leave Metal with no texture path; a `Bindless` device runs the
  `ArrayPages` layout, and what it would gain is capacity, not a second path.
- **The material id is read in the fragment stage as a flat varying**, moved
  there alone before any texture joined it, because the file's two worst bugs
  (`SV_InstanceID`, `SV_VertexID`) were integers the four targets disagreed
  about. All four emit the flat qualifier (SPIR-V `Flat`, WGSL
  `@interpolate(flat)`, MSL `[[flat]]`, DXIL `nointerpolation`). The material
  table has no ring: a row is written when it is created, and an animated
  material is what would make it one.
- **Instances store sector-local `f32` transforms plus a sector id**, static
  while an object does not move, so delta upload survives camera motion. Per
  frame the CPU computes a small **sector→camera offset table in f64** and the
  vertex and cull shaders add it; that also defines the space cull AABBs live
  in. Only the instance half is built.
- **Transparency sorts with a GPU radix sort over packed depth keys**, bitonic
  for small counts — named so it is not rediscovered; the design is now
  `docs/plan/53-transparency.md`'s.
- **Each path selector is a permutation axis in one Slang source**, through
  per-target `-D` defines and a declared target list per shader, decided before
  any shader was written because the first shaders became the later stages'
  inputs.
- **The cull-stats ring is the only permitted readback**: N frames latent, debug
  builds only. Everything else the GPU decides stays on the GPU.
- **Occlusion culling is two-phase and off by default.** The cull tests frustum
  survivors against the previous frame's farthest-depth pyramid, draws what
  passes, reduces this frame's depth and retests the rest before a late prepass;
  frames with it on are pixel-identical to frames with it off on every path.
  Measured at 1920×1080 in `Scene::Occluders`, it hid two thirds of the
  survivors and took lavapipe's frame from 73.87 to 64.66 ms but cost an RX 7900
  XTX 0.905 against 0.859 ms, where the saved draws are cheap. The later
  measurements are in `docs/backlog.md` under _What occlusion culling shipped
  without_. The "visibility buffer slot" the plan once named is this pass's
  input, not a visibility-buffer renderer, which the forward rule refuses.
- **`draw_gen.slang::lateFinishMain` finalizes buckets in parallel.** On a
  Vulkan/radv fixture with 512 crate buckets it took the sum of GPU pass
  durations from 4.769 to 4.579 ms and the finalizer from 0.181 to 0.002 ms
  (repeated-run pass times, not frame time; the original bucket layout showed
  negligible benefit). `mesh_e2e::occlusion_finish` is its check, and forcing a
  single workgroup made it fail.
- **Sprites are instances, not mesh ranges.** `crcbl_render::sprite_pass`
  generates its quad from `SV_VertexID` and reads per-sprite data by
  `SV_InstanceID`, outside `draw_gen`'s buckets. What the "no second 2D
  renderer" decision protected still holds: one pass in the same graph with the
  same computed barriers.

## What the deleted 25-lod plan left behind (2026-09-24)

Record; the built part of the plan is the QEM simplifier
(`crcbl_scene::simplify`), the cluster DAG builder
(`crcbl_scene::cluster_dag::build_cluster_dag`, with `crcbl_scene::lod`'s chain
kept as the proof of failure), the cooked dunes artifact
(`crates/crcbl-shaders/tools/cook-clusters.rs`,
`crates/crcbl-shaders/clusters/dunes.dag`, `crcbl_shaders::cluster_dag`),
app-built DAGs through `crcbl_render::scene::Geometry::Dag` (`apps/quarry`),
per-cluster selection on `MeshShader` and the uniform cut on both indirect tails
with per-group hysteresis (`draw_gen.slang`, `mesh_cluster.slang`,
`crcbl_shaders::cluster_select`, `crcbl_shaders::level_select`), the shadow LOD
bias (`SHADOW_LOD_BIAS`), hand-authored precedence at import
(`crcbl_scene::lod_resolve`), `crcbl lod stats|gen`, and the LOD tint,
screen-error heatmap and frozen-selection debug views. What it left open is in
`docs/backlog.md` under _Topic 25's MVP is closed; what remains, and one
coverage hole it found_ and its entries, _LOD: joint-weight-aware collapse for
skinned meshes_, _Three of the four QEM properties quarry claims to prove are
not implemented_, _What `crcbl lod` left owed_ and _What `crcbl_scene::simplify`
owes_.

Code cites the plan as "topic 25's hysteresis", "topic 25's uniform cut", "topic
25's observable", by a section's name, or by a step of the build. Those resolve
here:

| Citation                                               | What it specified                                                                                                                                         |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The cluster DAG, "Why a chain cannot work"             | A mesh is a DAG of clusters, not a chain of levels, because independently simplified levels crack where they meet                                         |
| The build, steps 1 to 5                                | Cluster; group by shared-edge adjacency; lock the group's outer boundary and simplify to about half; re-split; regroup                                    |
| Why every cut is crack-free                            | A cut's level boundaries were group boundaries in the coarser level, locked when it was simplified; error carried per group                               |
| How a DAG reaches the renderer (the boundary to cross) | The cooked artifact in `crcbl-shaders`, committed and checked, and the generator as an example                                                            |
| What the fallback paths do, the uniform cut            | `IndirectCount` and `IndirectPerBatch` draw every cluster at one depth, which is a chain level                                                            |
| Hand-authored levels keep their precedence             | Hand levels first and verbatim, generated levels fill the gaps, and no silent substitution                                                                |
| The chain, "LOD chains", `name_LOD1`                   | The pre-DAG chain of ratio levels (`build_lod_chain`, `DEFAULT_LOD_RATIOS`) and the glTF naming convention                                                |
| Auto-LOD: QEM simplification                           | Garland–Heckbert collapse, attribute-aware, with caller-locked edges, the link condition and a recorded `max_error`                                       |
| The attribute slice                                    | UV and normal seams, material boundaries and skinning weights carried through a collapse (unbuilt)                                                        |
| Runtime selection, the descent, the metric             | Projected screen-space error against a pixel budget, descending from the root, with the granularity following the path                                    |
| The two granularities                                  | Per cluster in the amplification stage on `MeshShader`; one uniform cut per instance in the cull pass otherwise                                           |
| Hysteresis, "switch-up and switch-down differ"         | Two budgets, a band between them, and the history per (instance, group)                                                                                   |
| The three selection tables, the selection numbers      | The host-written selection records (`crcbl_shaders::level_select`'s `MeshLevels` and `LevelGroup` among them), and the frame's scale and two budget lanes |
| The observable, the record of a cut                    | The per-view, per-cluster readback of which clusters were drawn, the only thing that shows a cut                                                          |
| Shadow LOD bias                                        | A positive budget multiplier per pass, selecting from the camera's eye                                                                                    |
| The ~2 px floor                                        | No level whose triangles fall under about two pixels (unbuilt)                                                                                            |
| Global LOD bias                                        | A quality setting, and the browser tier's default (unbuilt)                                                                                               |
| Far ranges                                             | HLOD per sector, then octahedral impostors (unbuilt)                                                                                                      |
| Tooling, Testing                                       | The debug views, stats rows, LOD panel, `crcbl lod gen\|stats\|preview`, golden meshes and the error-bound property test                                  |

**The DAG replaced the chain (locked 2026-08-12).** A chain simplifies each
level from the base independently and clusters each on its own, so two levels'
cluster boundaries have no relationship: drawing one cluster at LOD0 beside its
neighbour at LOD2 puts two differently decimated versions of one edge side by
side, and the surface opens. That is what "simplify each level independently"
means, not a defect of one implementation — `crcbl_scene::lod::build_lod_chain`
is the chain that proved it. Locking every cluster boundary is not the fix
either: boundaries are everywhere, so nothing would simplify. **The build is
group–lock–simplify–resplit**, Nanite's shape (Karis, SIGGRAPH 2021): (1)
cluster the base mesh with `crcbl_scene::meshlet`, the leaves; (2) group
neighbouring clusters a handful at a time by partitioning the adjacency graph,
**where adjacent means sharing an edge, not nearly touching**; (3) lock the
group's outer boundary and simplify its interior to roughly half its triangles;
(4) re-split into fresh clusters, the parents of every cluster in the group; (5)
repeat, grouping differently each level so an edge locked at one level is
interior at the next.

**Every cut is crack-free by construction, and that is the property to test.**
Wherever two detail levels meet across a cut, the boundary was a group boundary
in the coarser level, locked when it was simplified. **Error is carried per
group, not per cluster**: a group simplifies as a unit, so a cut drawing one of
its parents while descending into another would tear along a boundary the group
never locked. A group's error is the worst vertex charge over its parents,
raised to the worst error of any cluster that went into it — monotone up the
DAG, which is what makes a cut well-defined. Detail varies across a level
because groups differ, never within one.

**How a DAG reaches the renderer: a cooked artifact, mirroring the shaders.**
`crcbl-render` cannot see `crcbl-scene` (that would pull `gltf` into the
renderer) and `crcbl-shaders` has no dependencies by design. So a tool generates
the DAG and writes it into `crcbl-shaders`, the artifact is committed, and a
`--check` mode regenerates and compares. **When the real asset pipeline arrives
it replaces the generator, not the consumer.** Rejected as a delivery mechanism,
now and for any later one: generating the data from a dev-dependency at test
time, which gives tests data and the shipping path none; and a conversion in a
crate that sees both, which the renderer still cannot reach. **The generator is
an example because of the dev-dependency cycle**: `crcbl-scene` depends on
`crcbl-shaders`, so a normal dependency back is refused and a `[[bin]]` cannot
see dev-dependencies, while an example can; `cargo build -p crcbl-shaders` still
builds that crate alone. The hand-written `cube_clusters`, `pyramid_clusters`
and `open_box_clusters` are test fixtures, not what the renderer's clusters are.

**The fallback paths are a restriction of the same structure.** `IndirectCount`
and `IndirectPerBatch` select a **uniform cut** — every cluster at one depth,
exactly a chain level — drawn as ordinary index ranges: same hierarchy, same
metric, one decision per instance instead of per cluster. One builder, not two,
and a visible quality difference on the fallback paths that is an honest one.

**Hand-authored first, generated as fallback (locked).** Import resolves each
level in order: a hand-authored level (glTF nodes named `name_LOD1`,
`name_LOD2`, or the `MSFT_lod` extension) is used verbatim and always wins; a
missing level is generated. **Hand levels are never fed to the generator**, and
a fully hand-authored chain is never touched by it. **A hand level is a
whole-mesh level, so a mesh with any hand level is selected per instance even on
the `MeshShader` path** — an artist supplies whole-mesh geometry, not a
crack-free cluster hierarchy. **No silent substitution**: the import report
(`crcbl lod stats`, a future LOD panel) says which level came from where.
**There is no per-asset ratio override**: the chain era's "~50/25/12/6 %,
per-asset overridable in sidecar meta RON" described a generator that took a
ratio list, and the DAG's levels halve structurally, so reinstating one changes
`build_cluster_dag`'s signature; anything owed there belongs with the sidecar
and `AssetId` item in `docs/backlog.md`. **Skinned meshes** build the hierarchy
over the bind pose and GPU skinning skins whichever clusters were selected.

**The simplifier's rules (Garland–Heckbert, 1997).** Tests use hand-derived
values, not values recorded from the simplifier's own output.

- **A caller-supplied locked-edge set is the interface the DAG needs**, and what
  separates this from a plain decimator: the simplifier infers topological
  borders (an edge used by one face) itself, but a group's outer boundary is
  interior to the mesh and only the caller knows it.
- **The link condition** — an edge collapses only when its endpoints share
  exactly two neighbours — or a closed mesh gains a four-face edge and stops
  being closed.
- **`max_error` is a quadric error, not a certified Hausdorff bound**; the
  property test samples a lower bound on the distance and certifies nothing
  finer.
- **Collapse order needs a strict total order**, because a cost keyed on `f32`
  has ties and tie order decides the result; same input, identical output.
- **Flip rejection has to be global as well as per collapse**: a face can turn
  all the way round under a sequence of individually legal collapses.
- **Attribute awareness is the risk auto-LOD tools live or die by**: UV and
  normal seams constrained, material boundaries preserved, weights carried —
  with golden meshes, and a hand level as the escape hatch so no asset is ever
  blocked.

**Runtime selection.** Projected screen-space error — a group's error scaled by
distance and field of view — is compared with a pixel budget, descending from
the root while it exceeds the budget. It is the same maths and thresholds on
every `GeometryPath`, at zero CPU cost. **A parent's error is at least its
children's and its sphere contains theirs**, so the descent has one stopping
point per branch and no cluster is drawn under a drawn ancestor.

- **Hysteresis history is per group, and that is a soundness requirement, not a
  saving.** A group expands above the budget and keeps expanding until its error
  falls to a fraction of it (`ForwardRenderer::lod_hold_ratio`). A cut is a
  cover only while expansion is monotone up the DAG, and per-cluster history can
  leave a child collapsed under an expanded parent: a hole. From an all-zero
  start every later frame is monotone by induction. The key is the instance
  slot, which is why a slot's reuse matters to goldens.
- **The importance metric is not a shared helper (tried 2026-08-31).**
  `GroupCost::projected_error` divides by the distance to the sphere's
  **surface** and answers infinity inside it, a worst-case error;
  `shadow::coverage` divides by the distance to the light's **centre**, an
  angular radius. One helper would need a flag choosing the denominator. Only
  the band's number is shared — `shadow::LEVEL_HOLD_RATIO` is the same fifth
  `lod_hold_ratio` opens, and each names the other.
- **The ~2 px floor exists because forward shading of sub-pixel triangles costs
  quads.** A forward renderer shades a full 2×2 quad for every triangle a pixel
  touches, so triangles under a pixel cost four fragments each; the forward rule
  refuses the visibility buffer that fixes that, so the floor stands in for it.
- **Transitions are an instant swap**: correct thresholds make a pop sub-pixel
  by definition, because the error metric is the pop's size.
- **Graphics-only (locked): LOD never touches simulation.** Colliders are full
  fidelity at every distance and setting — physics, navmesh, audio occlusion and
  every sim query. It is structural: colliders live server-side and selection is
  a client render concern, so a client's bias cannot reach the sim (two players
  at different settings play one physical world, and the tick hash never depends
  on anyone's graphics).
- **HLOD and impostors get their own passes** and never complicate the selection
  shader.

**The shadow LOD bias is one positive budget multiplier per pass, not "+N
levels"** (settled 2026-08-13). The descent has no level parameter, and
level-to-level error ratios are a property of the mesh — on the dunes DAG level
0→1 steps about 2.4×, level 2→3 about 8.8×, and the top three levels report the
same error — so "+2 levels" would mean three things on one mesh. **Cascades
select from the camera's eye**: a coarser caster costs a shadow edge displaced
by the group's error, seen by the camera at the camera's distance, so the budget
is in camera pixels. The light is the eye only for the amplification stage's
normal-cone test. **A per-cascade factor is sound; a per-cluster or per-group
fudge is not**: monotonicity needs one constant over a pass, and two groups on
one branch judged against different budgets is a hole. With a factor above one
and both histories starting empty, the shadow cut is a subset of the camera's.

**Considered and declined:**

- **A chain of independently simplified levels** — cracks, as above.
- **Locking every cluster boundary** — nothing would simplify.
- **Grouping by proximity** — clusters that nearly touch across a gap must not
  be grouped.
- **Per-cluster error or per-cluster hysteresis history** — both tear a cut.
- **Generating DAGs at test time from a dev-dependency, or converting in a crate
  that sees both** — neither gives the shipping path data.
- **Feeding hand-authored levels to the generator, or cutting between them per
  cluster** — they share no locked boundaries.
- **A per-asset ratio override on the DAG generator.**
- **A shared importance-and-hysteresis helper** across LOD and the shadow atlas.
- **The shadow bias as "+N levels", or as a per-cluster or per-group factor.**
- **Light-as-eye for cascade selection** — what looked like it was the camera's
  eye pushed along the sun per cascade, asking two cascades two different
  questions about one caster.

## What the deleted 43-render-standards plan left behind (2026-09-24)

Record; topic 43 was the gap survey — what a current engine ships and where this
one stands — written 2026-08-27 with every "where this one is" row read out of
the code, and its delivery table ordered the gaps by benefit per unit of work.
Built from it: the foundations block's rows (a) vertex v2 with the 64-byte
`GpuMaterial` and `depthVertexMain`, (b) `crcbl_render::stack::CameraStack`, (d)
`PageDesc` over `PageKind`, (e) `crcbl_render::debug_draw` and (g)
`crcbl::settings::presets`; the viewer's PBR shelf; the normal, packed
metallic-roughness-occlusion and emissive pages, with the importer reading all
five glTF maps; alpha-mask and double-sided modes; specular antialiasing; CMAA2;
the probe volume's scroll; the motion-vector target; mips and anisotropy
(`crcbl_render::mip`); height fog and the froxel column; auto-exposure; ACES;
the spatial upscale; the gradient sky and Hillaire's atmosphere. Row (f) was
refused. The open rows — blended transparency, block compression, grading, MSAA,
the motion-vector consumers, HDR output and foundation (c) — are in
`docs/backlog.md` under _The rendering-gap survey's open rows (from the deleted
43-render-standards plan, 2026-09-24)_.

Code cites the plan as "topic 43 §2", "topic 43's filtering rung", "row (d)" or
"topic 43 prices a rung". Those resolve here:

| Citation                                      | What it covered                                                                                                                                                              |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| §1                                            | What is at or above the standard — _What "current" means_ below                                                                                                              |
| §2, rungs 1–4                                 | Materials: the tangent frame (1), the normal page (2), the packed and emissive pages (3), alpha-mask and double-sided (4); the vertex v2 layout                              |
| §2's filtering rung, the filtering subsection | Host-built mips, the trilinear and anisotropic sampler, `texture_quality` as a `lod_min` clamp, block compression                                                            |
| §3                                            | Transparency: sorting before order-independence; the rung itself is `docs/plan/53-transparency.md`'s                                                                         |
| §4                                            | Volumetrics, and the no-transcendental rule's escapes that height fog needed                                                                                                 |
| §5                                            | Global illumination: the probes, the split-sum, which trace family                                                                                                           |
| §6                                            | Post-processing and auto-exposure                                                                                                                                            |
| §7, the render-scale row                      | The spatial upscale                                                                                                                                                          |
| §8                                            | The gradient sky and the atmosphere                                                                                                                                          |
| §9                                            | Motion vectors and the rungs that read them                                                                                                                                  |
| §10                                           | What the survey refused to re-open                                                                                                                                           |
| Delivery, "the rule", "prices a rung"         | The pricing rule and the fused-clears floor                                                                                                                                  |
| Foundations block, rows (a)–(g)               | (a) vertex v2; (b) the render stack as RON; (c) the acceleration-structure seam; (d) the page allocator; (e) debug draw; (f) a shared importance helper; (g) quality presets |

**What "current" means.** The comparand is the feature set common to Unreal 5,
Unity HDRP and Godot 4 — not the frontier of any one of them. A row marked
missing is missing; a row marked refused has its reason written where the
technique is owned, and re-proposing it means arguing with that reason. The
survey stated first where this engine is ahead: GPU-driven submission
(`cull.slang`, `draw_gen.slang`), mesh shaders over a cluster DAG with
screen-space-error LOD (Nanite's shape at a fraction of its scope — no software
rasteriser, no streaming), clustered forward, four backends with byte-comparable
goldens including a browser, reversed-Z with an HDR target and linear lighting,
and the Cook-Torrance BRDF Unreal, HDRP and Filament shade with. The comparand
claims cannot be checked from this tree; `docs/notes/process.md` (_What the plan
audit of 2026-09-03 did not reach_) says so.

**A rung is priced before it is called built** (2026-08-30). Milliseconds per
pass on the desktop adapter, on lavapipe and in the browser, read off
`crcbl_render::PassStats` — not a sentence saying it is cheap. The software and
browser tiers pay tens of times the desktop cost and are the tiers every golden
runs on, so the desktop number alone is not a price. The per-machine baseline is
`docs/backlog.md`'s _Profiling: five of the eight gaps are still open_. Two
prices the delivery table recorded: row (a)'s depth prepass costs 20.6/20.4 ms
against the full stage's 22.9/24.2 ms on lavapipe over 144 dunes patches at
640x480, and 0.073–0.074 ms either way on an RX 7900 XTX; row (e)'s 1024 boxes
at 256x192 cost 0.030/0.031 ms on the RX 7900 XTX against the forward pass's
0.013/0.014, and 1.865/2.187 ms on lavapipe against its 0.134/0.159. Neither has
a browser figure.

**Every `forward` figure includes the pass's fused full-extent clears**
(measured 2026-09-05). The pass clears the scene colour, reflectivity and motion
targets as `LoadOp::Clear`s in its begin; timing them apart would need a pass of
their own. `crates/crcbl/tests/mesh_e2e/depth_only.rs` measures the floor — same
extent and stack, empty draw list: at 640x480 over 48 recorded frames a
`forward` p50 of 0.009 ms on an RX 7900 XTX and 0.258 ms on lavapipe (medians of
three), against the loaded field's 0.135 and 29.255 ms. Subtracting it gives the
draw's own cost; no quoted share has had it subtracted.

**A foundation is scheduled before any feature rung that would be cheaper with
it** (2026-08-30), and the lighting order interleaves the raster rungs with the
foundations: (g) lands with the first rung that needs it, (c) when the
ray-tracing tier's updater is next. The user's rule for the order: best-looking
for the performance, cheapest real win first.

**The tangent frame goes by the vertex route, because of mirrored UVs** (§2 rung
1, corrected 2026-08-27). The derivative route cannot recover the handedness
glTF stores in a tangent's `w`, so every mirrored shell lights inside out; the
earlier argument from determinism was wrong, since `geometric_normal_of` already
takes `ddx`/`ddy` under the cross-backend goldens. The screen-space frame is the
fallback for a mesh without `GpuMesh::MESH_AUTHORED_TANGENTS`, and it applies
the UV Jacobian's sign rather than inheriting it, because that sign also carries
the target's screen-space `y`, which radv runs the other way. A ray-traced hit
has no derivatives, so an unmarked mesh has no normal mapping there — the
argument for MikkTSpace ahead of the ray-tracing tier.

**Colour pages are sRGB, number pages linear.** `BASE_COLOR_PAGE_FORMAT` and
`EMISSIVE_PAGE_FORMAT` are `Rgba8UnormSrgb`; `NORMAL_PAGE_FORMAT` and
`MRO_PAGE_FORMAT` are `Rgba8Unorm`. A number decoded through the sRGB curve
looks merely "shinier than intended", which is why it survives review;
`forward::the_page_formats_split_colour_from_number` is the guard.

**The vertex v2 layout** (landed 2026-08-30; the decision record is _DECIDED —
the vertex and material strides widen once_ below). Stream 0 is a `float3`
position, twelve bytes, all the depth prepass and every shadow pass fetch
through `depthVertexMain`. Stream 1 is twenty bytes: a QTangent in `snorm16x4`
with glTF's handedness in its sign, `uv0` and `uv1` in `unorm16x2` against a
per-mesh `UvRange` in `GpuMesh::uv_range` (both geometry paths fetch that row,
and a cluster DAG needs one range for every level), and an `rgba8` colour. The
streams are two regions of one storage buffer, not two bindings, because a ninth
storage buffer in the vertex stage is a renderer no browser can build; the
boundary travels in `FrameUniforms::vertex_pool.x` and
`skinning::Params::attribute_base`. `every_shader_decodes_a_vertex_the_same_way`
holds the three shader copies equal.

**`GpuMaterial` is sixty-four bytes because two page indices share a word.**
Eighteen plain words is seventy-two bytes, which `std430` rounds to eighty; the
four layer indices ride sixteen bits each (`color_normal_pages`,
`mro_emissive_pages`), bounded by `MAX_PAGE_LAYER`. `GpuMaterial::NO_PAGE`
(`0xFFFF`) is out of band on all four columns, so a row naming no page shades
the literal identity instead of sampling a neutral layer — eight bits cannot
encode a flat normal (`0x80` decodes to `1/255` off).

**The page allocator is not the shadow atlas's** (row (d), decided 2026-08-31).
`PageDesc` is whole layers uploaded once at scene build and indexed by number;
`crcbl_render::shadow::AtlasAllocator` allocates and frees rectangles every
frame with a quadtree. Sharing them would be an abstraction with one real user.
If decal atlases want rectangles, that is the second caller and the moment to
extract one. A kind nothing names costs a 1×1 placeholder, magenta so that a
read which should have early-outed shows in the frame.

**glTF's channel arithmetic, term for term** (§2 rung 3). Roughness is
`material.roughness * texel.g` and metalness `material.metallic * texel.b` (glTF
§3.9.2), resolved once and read by the direct lobe, the reflectivity attachment
SSR reloads and the RSM's `metallic_of`, so the shaded frame and the map that
refills the probes agree. Occlusion `texel.r` multiplies the indirect terms
alone, never the direct lobe (§3.9.5); emission is
`material.emissive * texel.rgb` (§3.9.4). Where `metallicRoughnessTexture` and
`occlusionTexture` name different images the occlusion `r` is resampled into the
packed layer; a channel with no image is `0xFF`, each product's identity.

**Page reads are an unconditional `Sample` with the select below it**, the form
WGSL's uniformity analysis accepts; the normal page is read with `SampleGrad` on
derivatives taken above the early return for the same reason. SPIR-V, MSL and
DXIL accepted every other form; only the browser gate objected.

**Masked and double-sided draws route per bucket** (2026-09-05): the material's
mode is in the bucket key (`GpuMaterial::MODE_MASK` through
`GpuInstance::MATERIAL_MODE_SHIFT`) and `ForwardRenderer::depth_partitions`
splits the depth passes on it. Measured on lantern at 1920x1080: on lavapipe the
split takes back most of the cutout's cost (prepass 3.209 → 1.444 ms, atlas
15.462 → 9.770 ms against an opaque 1.367 and 9.203); on radv a twin bucket per
mode costs twelve empty indirect dispatches per shadow view (`shadow` 0.138 →
0.221 ms) whatever the mask, so a scene of mostly opaque geometry with a little
foliage wins everywhere the fragment stage costs more than an empty dispatch. A
double-sided instance also skips the cluster cone rejection (`cone_may_reject`),
since a cone means "draws nothing" only under back-face culling.

**Mips are built on the host, not in compute** (2026-08-29), for three reasons
each sufficient: a compute pass over an sRGB page needs a `UNORM` view alias
(`ImageDesc::view_formats`, which does not exist, and WebGPU refuses the
reinterpretation without it); a host filter gives the same bytes on all four
backends where a device-built chain is four drivers' rounding; and offline is
what current engines do — a compute pass is for a texture the frame produced.
The filter averages in linear light and re-encodes, weights by alpha, and
renormalises a normal after averaging (`crcbl_render::mip::normal_chain`, which
also copies a one-texel cell byte for byte).

**Exactness claims about a page stay on magnified or `SampleLevel` reads.** The
specification bounds the LOD computation and leaves the anisotropic footprint to
the implementation, so a minified textured surface is where rasterisers may
differ by more than a last bit; those frames compare under
`Tolerance::RASTERISER`. The anisotropy is `ForwardRenderer::anisotropy_for`'s,
`DEFAULT_ANISOTROPY` clamped to the device, and the player's key is
`[engine.video] anisotropic_filtering`, the first key allowed to ask for more
than the engine's default because the device's ceiling bounds it. WebGPU reports
a ceiling of one (_What the filtering rung still owes_ in `docs/backlog.md`).

**Block compression is KTX2 carrying supercompressed UASTC** (decided
2026-09-06): Khronos' own pipeline and what `KHR_texture_basisu` names, so one
asset transcodes at load for every device family — BC7/BC5/BC4 where
`Features::TEXTURE_COMPRESSION_BC` is granted, ASTC or ETC2 otherwise — which
matters because BC is optional in WebGPU. UASTC rather than ETC1S because these
are material pages and ETC1S is known for wrecking normal maps. The encoder is a
pinned `basisu` CLI for reproducibility (two encoder releases produce different
blocks from the same input); the engine only ever transcodes, so the encoder is
never linked into a game or a wasm build. Unbuilt; the rung is in the backlog.

**Sort before order-independence** (§3). Weighted-blended OIT is an
approximation that cannot be blessed against a reference — a golden blessed
against it records the approximation as the answer — so sorted blending first,
and an order-independent scheme only if sorting proves insufficient.

**No transcendental reaches a colour, and there are four ways around it** (§4,
§8). Four platforms' `exp`, `log2` and `pow` differ in the last place, and the
rule keeps the ceiling on that disagreement known rather than absorbed — every
golden compares under `Tolerance::RASTERISER`, and `Tolerance::EXACT` is in no
image test. The escapes: a table cooked on the host (`crcbl_shaders::dfg`); a
construction from exactly specified IEEE operations (`crcbl_shaders::fog`'s
`exp_neg` — range reduction on a two-part `ln 2`, a Horner Taylor kernel, `2^-n`
written into the exponent field, within two units in the last place of
`f64::exp`); a projection done on the host (the sky's spherical harmonics); and
`sqrt` for `pow` where the exponent allows (`d * sqrt(d)` for
Henyey-Greenstein's three-halves power, since IEEE requires a correctly rounded
`sqrt`). The same rule bins the exposure histogram by the float's exponent field
rather than `log2`, steps adaptation linearly rather than by
`1 - exp(-rate * delta)`, and blends the gradient sky by a cubic rather than a
`pow`. `docs/notes/simulation.md` records it as the workspace policy. The fog's
observables are laws, not differences:
`doubling_the_fog_density_squares_the_transmittance` and
`splitting_a_slice_composites_to_the_same_radiance`.

**Which trace family** (§5, 2026-08-27). Screen-space marching (SSR, GTAO) is
contact-scale and never GI on its own. SDF marching (Lumen's software path,
SDFGI) removes the off-screen limit at the cost of a bake, a volume per mesh and
no skinned geometry. Voxel cone tracing leaks through thin walls and is largely
superseded. A march is chosen for reach, not speed — on ray-tracing hardware the
traced path is faster and more accurate — and WebGPU has no ray tracing. Since
2026-08-30 GI is hardware ray tracing only (_DECIDED — GI is hardware ray
tracing only_ below) and the probe volume is every tier's bounce; SSGI was
withdrawn the same day, contact shadows shipped 2026-09-01, and the cone trace
and the SDF path stay recorded as the ray-tracing tier's raster alternatives,
unscheduled.

**The spatial upscale is Catmull-Rom** (§7, 2026-08-27): Mitchell-Netravali at
`B = 0, C = 0.5`, interpolating, sixteen taps of multiplies and adds. Bilinear
is the worse choice at the same cost class, because a resolution slider is
judged on how the frame looks at 0.5. It does not jitter, accumulate or keep a
history, so a temporal upscaler later replaces the pass without moving the seam.
At full scale there is no pass.

**The sky** (§8). A consumer takes the gradient
(`crcbl_shaders::sky::SkyGradient`), not its L1 projection: an ambient term
wants the cosine-weighted integral, which L1 is, and a reflection wants radiance
along one direction. `sky.slang` draws at the reversed-Z far plane tested
`GreaterOrEqual` with writes off, so it binds no depth texture and has no
`discard`; `Sky::NONE` adds no pass. The atmosphere is Hillaire's (EGSR 2020)
over Bruneton and Neyret's Earth: the transmittance and multiple-scattering LUTs
are cooked into `crates/crcbl-shaders/tables/atmosphere.bin` and held by
`cook-atmosphere --check`; the sky-view LUT is marched on the host with no
platform transcendental and indexed by `sign(s)·s²` of the direction's `y` and
by `1 − 2u²` for the azimuth's cosine instead of the paper's angles — the one
deliberate departure. A moving sun is paid a stripe at a time (`SkyViewBuild`,
`SKY_VIEW_BUILD_ROWS` rows per `begin_frame`, restarting on a new move), so the
sky may lag by one build. A mirror reads the LUT and a rough lobe the three
bands, mixed by `sharpness_of`'s ramp, and the reflection pass binds the same
per-slot buffer the background draws from. The sun disc carries the
`DirectionalLight`'s illuminance over its solid angle, with limb darkening spent
into `SUN_LIMB_FIT` rather than a `pow`. Deliberately absent: aerial perspective
in this rung, and a ground bounce below the horizon, which the probe volume
carries. Preetham was declined 2026-08-30: visibly wrong at low sun. Priced
2026-09-05 at 1920x1080: the `sky` pass is 0.004 ms p50 on an RX 7900 XTX and
0.419 ms on lavapipe, inside the whole frame's run-to-run spread; on the host,
`SkyView::build` is 24.57 ms and one `SkyViewBuild::step` 1.496 ms.

**Motion vectors** (§9) are texture-coordinate space, current minus previous,
`+y` down, so a history is read at `uv - motion` — written on `MOTION_FORMAT`
and `TransientImageDesc::motion`, observed by
`crates/crcbl/tests/mesh_e2e/motion.rs` and `skinned_motion.rs`. A slot in
`GpuInstance` is populated rather than reserved, and widening the record is
cheap while few shaders index past `INSTANCE_STRIDE`, which is why
`previous_transform` was taken before its first reader.

**What the survey refused to re-open** (§10), each with its reason where the
technique is owned: deferred shading and visibility buffers, a second material
model (anisotropic GGX, clearcoat, sheen and subsurface, which would arrive
together with a `MATERIAL_STRIDE` widening) and parallax occlusion mapping —
_What the deleted 44-lighting plan left behind_; VSM, EVSM and virtual shadow
maps — _What the deleted 45-shadows plan left behind_; HBAO and HBAO+ and
float-hash or interleaved-gradient rotations — _What the deleted
46-ambient-occlusion plan left behind_.

## What the deleted 44-lighting plan left behind (2026-09-24)

Record; the built part of the plan is clustered forward (`light_cluster.slang`,
`crcbl_render::light_grid`, `crcbl_render::light`), the GGX lobe in
`mesh.slang`, and rungs 1 to 4 and part of 5 of its PBR ladder:
`crcbl_shaders::dfg`, the linear page formats and
`crcbl_render::mip::normal_resample`, `crcbl_shaders::sky_prefilter`,
`specular_aa_kernel`, and `crcbl_shaders::ltc` with `Light::Rect` and
`FLAG_FILL`. What it left open is in `docs/backlog.md` under _Ray-traced
lighting (P7C) is not built_, _What the LTC area-light rung left_, the
normal-length bullet of _Normal maps: what the tangent and page rungs left_,
_Specular IBL: what rung 3 left_, _What specular antialiasing shipped without_
and _What the light list left owed_. It was split out of
`docs/plan/18-render-features.md` on 2026-08-27.

Code cites the plan as "topic 44's rung 3", "topic 44's rule" or "topic 44's
'Clustered forward' section". Those resolve here:

| Citation                      | What it specified                                                                                                                  |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| The two paths                 | `LightingPath::RayTraced` and `Rasterised` as two complete lighting implementations sharing one material model                     |
| Clustered forward             | Lights as rows in an SSBO, assigned by a compute pass to a froxel grid the fragment stage indexes; the forward rule and its budget |
| The BRDF                      | One Cook-Torrance GGX lobe over glTF's `metallic` and `roughness`, with Lambert diffuse                                            |
| The rule, the shading rule    | No platform transcendental reaches a colour, and the two ways out of it                                                            |
| Rung 1                        | Multi-scatter energy compensation, Fdez-Agüera's form over the `DFG` table                                                         |
| Rung 2                        | The inputs: non-colour pages linear, two-channel normal pages, normal mips renormalised and their lost length kept (unbuilt)       |
| Rung 3                        | Specular IBL by the split-sum: the `DFG` pair and the prefiltered gradient sky                                                     |
| Rung 4                        | Specular antialiasing by roughness regularisation (Tokuyoshi-Kaplanyan)                                                            |
| Rung 5                        | LTC area lights (rectangle, sphere, tube and disc) and the fill flag; rectangles and the flag are built                            |
| "One table serves both rungs" | Rung 1 took Fdez-Agüera so the table it reads is the one rung 3 reads                                                              |
| What stays out                | The refusals under _Considered and declined_ below                                                                                 |

**The two paths.**

- **Ray-traced lighting and a complete rasterised twin are both MVP.**
  `LightingPath` selects per device and degrades: a device without `RAY_QUERY`
  and `ACCELERATION_STRUCTURE` gets `Rasterised` and a complete picture that
  merely looks worse. Ray tracing is Vulkan and D3D12 only — WebGPU has none and
  Slang cannot emit it for Metal — and since `crcbl-dx12` and `crcbl-mtl` were
  deferred (2026-08-21) the raster twin is very nearly the only path anyone
  sees, which makes it more clearly the right call, not less. Whether the
  ray-traced implementation is worth building next is a scheduling question, and
  it lives in `docs/backlog.md`.
- **The paths differ in how visibility and radiance are gathered, never in how
  they are shaded.** One material table, one BRDF, one set of inputs; one
  tonemapped output target, with the post stack identical after either path so
  nothing downstream branches on `LightingPath`.
- **Golden images per path, and a human-reviewed pairwise comparison.** The two
  are not expected to match pixel for pixel; a scene that reads correctly on one
  and wrongly on the other is a defect in whichever is wrong. The comparison is
  a reviewed reference, not an automatic tolerance.
- **Acceleration structures are built regardless of who consumes them**, where
  the device supports them: a BLAS per mesh asset at bake or load, a TLAS refit
  per frame from the instance data the cull pass reads. Topics 13 and 24 are the
  other potential consumers and neither may assume the structure exists.

**Many lights (decided 2026-08-13).**

- **Clustered forward, not tiled, deferred or a visibility buffer.** Tiled
  (Forward+) degrades with depth range — a tile spanning a near wall and a far
  sky gathers every light between, which is exactly lantern's and the towers'
  shape. Deferred breaks "one BRDF, one set of inputs" with the ray-traced twin,
  fights MSAA and transparency, and makes the raster path structurally unlike
  the ray-traced one. Clustered forward needs only a compute pass and two
  storage buffers, so it is the same code on all four backends.
- **Shading happens in the forward pass and nowhere else** (restated 2026-08-30
  at the user's request). A pass may write a second attachment beside the lit
  colour — the reflectivity target, the motion target, one day an albedo or a
  normal a screen-space GI rung wants — because each is a by-product of shading
  the forward pass already did, read by one named consumer. No rung may move the
  BRDF, the froxel walk or a light's evaluation into a pass that reads
  attachments: no G-buffer lighting, no deferred decals, no visibility-buffer
  shading. The test for a proposal: after it lands, does `mesh.slang` still
  evaluate every light that reaches a fragment?
- **The forward pass writes at most 16 bytes a pixel on the software and browser
  tiers**, and is at that figure today with no headroom: the lit target's eight,
  `TransientImageDesc::reflectivity`'s four (`Rgba8Unorm`) and
  `TransientImageDesc::motion`'s four (`Rg16Float`). A fourth attachment lands
  only by paying for itself with a measured lavapipe frame beside it; past that
  the pass has bought a G-buffer's bandwidth without a G-buffer's savings.
- **A light is a row, and so is the sun.** Position, radius, colour
  premultiplied by intensity, type, and a spot's direction and cone angles; a
  directional light is flagged as affecting every cluster, so the shader has no
  special case for it. **A cluster holds a bounded number of indices and
  overflow is counted, never silently dropped** —
  `crcbl_shaders::light::CLUSTER_OVERFLOW_WORD` in the culling statistics.
  **Shadowed lights are a small subset** chosen by `crcbl_render::shadow`'s
  coverage ranking (topic 45, below); an unshadowed light still lights.

**The BRDF (decided 2026-08-13).** `mesh.slang` had shaded with Lambert plus a
Blinn-Phong lobe of two constants, so there was one material however many rows
the table held. The row grew glTF's `metallic` and `roughness` and the lobe
became Trowbridge-Reitz `D`, Smith height-correlated visibility, Schlick's
Fresnel and Lambert diffuse, because a roughness-driven Blinn would be a second
material model the ray-traced twin would have to rewrite, and glTF already
speaks GGX. Two consequences:

- **A metal has no ambient term, and that is the model.** Ambient scales the
  diffuse albedo, and a conductor's is zero, so a fully metallic surface out of
  every light's reach is black until SSR or the probes give it something to
  reflect. `GpuMaterial::UNTINTED` is `metallic 0.0`, but `apps/lantern`'s
  mirror slab and brass block are fully metallic and `crcbl_scene`'s glTF paths
  default to metallic as glTF specifies.
- **Neither lobe carries `1 / pi`.** The engine's diffuse is a bare
  `albedo * N·L` — a light's intensity has absorbed the reciprocal — so the `pi`
  is folded out of `D` too and the ratio between the lobes stays physical. The
  textbook `D` against this diffuse would put every highlight a factor of `pi`
  under its surface.

**The shading rule: no platform transcendental reaches a colour.** A platform's
`pow`, `exp`, `sin`, `cos` and the rest are specified to no accuracy and differ
in the last place between the four targets, and the goldens have no tolerance
for that. There are two ways out, both in use: **bake the function into a table
at cook time and sample it** (the `dfg`, `sky_prefilter` and `ltc` tables,
committed and compared as artifacts byte for byte, like the SPIR-V), or **build
it from the permitted operations** — `crcbl_shaders::fog`'s range reduction and
Taylor kernel, `crcbl_shaders::trig`. `sqrt` and divides are permitted: IEEE-754
rounds them correctly. **A transcendental whose result is quantised is safe**:
`froxel_of` calls `log2` three times and floors the result into a slice index,
so a last-place disagreement changes nothing a boundary fragment was not already
free to do.

**Rung 1 — multi-scatter energy compensation (built 2026-08-27).** A
single-scatter GGX lobe drops every multiply-bounced ray, more with roughness,
so a rough conductor came back grey in a furnace test. **Fdez-Agüera's closed
form over the split-sum `DFG` pair, not Kulla-Conty's second table**, because
one table two features read is one somebody keeps correct. The table's filter is
written out in the shader rather than asked of a sampler (fixed-function weights
differ between rasterisers, and it keeps Metal's sampler table fixed); it is
stored as 16-bit fixed point rather than `Rg16Float`, since a share in `[0, 1]`
is finer at `1 / 65535` everywhere. `mesh.slang`'s `specular_compensation`
scales the specular sum only — what the lobe dropped left as specular.

**Rung 2 — the inputs.**

- **Non-colour pages are linear.** A base-colour texel is sRGB-encoded; normal,
  metallic, roughness and occlusion texels are numbers, and decoding them
  through the sRGB curve is the classic PBR bug that reads as "shinier than
  intended". `NORMAL_PAGE_FORMAT`, `MRO_PAGE_FORMAT` and `EMISSIVE_PAGE_FORMAT`
  in `crcbl_render::forward` are the constants and
  `the_page_formats_split_colour_from_number` is the whole guard.
- **Normal pages are two-channel**, `z` rebuilt as `sqrt(1 - x² - y²)`, which
  suits BC5; the neutral texel is `(0.5, 0.5)`.
- **A normal page's mips are renormalised after averaging**
  (`crcbl_render::mip::normal_resample`: no transfer curve, no alpha weight, a
  single-texel cell copied byte for byte). The length the average loses is the
  normal variance Toksvig turns into roughness; keeping it is unbuilt.

**Rung 3 — specular IBL by the split-sum (built 2026-08-29).** The `DFG` pair is
baked and committed, so no platform derives it. **The prefilter is a table, not
a cube**: the gradient sky is linear in its three colours and reads only a
direction's `y`, so its convolution against the lobe is two weights over
`(|R.y|, roughness)` — `crcbl_shaders::sky_prefilter`,
`tables/sky_prefilter.bin`, `cook-sky-prefilter --check` in CI — and the sky's
colours stay run-time. **There is no ambient specular with `REFLECTIONS` off, by
decision**: the pair and the prefiltered sky are read in the reflection pass,
where metals take their ambient specular, and a term in `mesh.slang` too would
count the sky twice. An atmosphere reaches the table through
`crcbl_shaders::atmosphere::SkyView::gradient_fit` (its poles and azimuthal mean
at the horizon); the sun's bright limb, which a gradient cannot hold, is lost
there.

**Rung 4 — specular antialiasing (built 2026-09-05).** `specular_aa_kernel` is
Tokuyoshi and Kaplanyan's isotropic filter transcribed from their listing, with
the paper's `SPECULAR_AA_SIGMA_PX` (half a pixel) and `SPECULAR_AA_KAPPA`
(0.18), mirrored in `crcbl_shaders::mesh` beside a source-text test.

- **Screen-space derivatives are not banned here.** `geometric_normal_of`
  already takes `ddx`/`ddy` of the world position and drives the shadow bias
  under cross-backend goldens. What is ruled out is a derivative-built tangent
  frame, because mirrored UVs get the wrong handedness that way.
- **It widens the direct lobe's `alpha2` alone.** The `dfg` and `ltc` reads
  index a 64-square table in perceptual roughness and move a fraction of a
  texel; the reflectivity attachment must describe the material, or SSR would
  blur a mirror wherever geometry was dense; and regularising `roughness` itself
  needs two square roots whose round trip is not the identity, which would move
  every golden on fragments whose kernel is zero.
- **A zero kernel changes nothing, to the bit** — checked as bytes; only the
  dunes patch, the one curved surface in the tree, moved and was re-blessed.
- **The fixture's strips sit on integer pixel columns** (`screenshot`'s
  `SPECULAR_STRIP_PITCH`), because Vulkan guarantees only four
  `subPixelPrecisionBits` and SwiftShader and radv snapped 1.49-pixel strips
  differently (4788 pixels over tolerance; 1 after the fix).
- **Priced**: `forward` 0.342 → 0.341 ms on an RX 7900 XTX under radv and 35.475
  → 35.488 ms on lavapipe (lantern at 1920×1080, median of three runs' p50s) —
  under what three runs separate from noise.

**Rung 5 — LTC area lights and the fill flag (rectangles built 2026-08-31).**
Decided by the user's "best-looking for the performance" (2026-08-30): point
lights read as pinpricks, and a fixture reads as a fixture only when its
highlight has its shape. Heitz, Dupuy, Hill and Neubelt's linearly transformed
cosines give rectangles, spheres and tubes a plausible specular lobe from one
cooked table — the transcendentals are in the cook. **All three shapes were
chosen**, because sphere and tube are nearly free once the table exists (they
integrate as silhouette quads); only the rectangle is built.

- **The fill flag** — a light that casts no shadow and contributes no specular,
  how a no-bake stack lights the far end of a room — is a flag on the row, not a
  light type, because everything else about it is the ordinary light's.
- **The paper's second table is not cooked.** Its magnitude and Fresnel are
  Karis's scale and bias rearranged, so the `dfg` pair (binding 25, both
  channels) serves; binding 27 is the transform, `tables/ltc.bin`, held by
  `cook-ltc --check`. `GpuLight` grew to `LIGHT_STRIDE` 80 for a `tangent` and a
  `flags` word, with `KIND_RECT` and `FLAG_FILL` the first values.
- **Priced 2026-08-31, re-taken 2026-09-02**, `forward` p50 / p95 at 1920×1080
  over 400 frames with a froxel full of lights (`CLUSTER_LIGHT_CAPACITY`, the
  grid's worst case), the three rows interleaved on one device by `mesh_e2e`'s
  `the_price_of_a_froxel_full_of_area_lights`:

  | forward pass, 1920×1080      | radv (RX 7900 XTX, Mesa 26.2.1) | lavapipe (same Mesa) |
  | ---------------------------- | ------------------------------- | -------------------- |
  | sun alone                    | 0.087 / 0.090 ms                | 9.730 / 10.303 ms    |
  | + a full froxel of point     | 0.225 / 0.231 ms                | 19.139 / 20.294 ms   |
  | + a full froxel of rectangle | 0.559 / 0.578 ms                | 30.937 / 32.037 ms   |

  **A rectangle costs 3.4× a point light on radv and 2.3× on lavapipe.**
  Clustering is its own `light-cluster` pass at 0.31 µs per light,
  kind-independent because `light_cluster.slang` bounds a rectangle by a sphere.
  `forward` also carries two fused full-extent clears that cannot be split out
  without adding a pass. The browser tier is an ALU count, not a measurement:
  four extra `Load`s and two polygon integrals of up to five edges, roughly an
  order of magnitude over a punctual light's arithmetic.

**Considered and declined:**

- **A second BRDF lobe** — anisotropic GGX, clearcoat, sheen, subsurface. Each
  is a second material model, refused until an asset in the tree needs one; the
  row has no room at `MATERIAL_STRIDE`, so the first arrives with a stride
  widening and can bring the rest.
- **Parallax occlusion mapping** — a per-pixel march with a dependent read for
  what normal mapping already approximates; a rung above normal maps, not beside
  them.
- **Burley diffuse — the user's call, 2026-08-30.** Its fifth powers decompose
  into multiplies, so determinism never ruled it out; what it buys is a
  retroreflective rim on rough dielectrics, small beside every rung above, and
  Unreal ships Lambert for the same trade. Lambert stays, improved by the
  multi-scatter compensation, the AO tint and bent normals, the LTC lights and
  the probe bounce.
- **Tiled Forward+, deferred shading and a visibility buffer** — see the
  clustered-forward rule above.

## What the deleted 45-shadows plan left behind (2026-09-24)

Record; the built part of the plan is `crcbl_render::shadow` — sphere-fitted,
texel-snapped cascades, spot and point maps as atlas tiles, `AtlasAllocator` in
`shadow/atlas.rs`, the cadence's `schedule` in `shadow/cadence.rs`, the
`r_shadow_filter` selector — `mesh.slang`'s bias, cross-fade and filter ladder,
`DebugView::Cascades` and `DebugView::ShadowAtlas`, and the contact-shadow pass
(`crates/crcbl-render/src/contact_shadows.rs`, `contact_shadows.slang`), which
is parked outside `RenderEffects::DEFAULT_STACK`. What it left open is in
`docs/backlog.md` under _What the deleted 45-shadows plan left unbuilt_, which
lists the entries that carry it. It was split out of
`docs/plan/18-render-features.md` on 2026-08-27; `apps/sundial` is its
comparison fixture.

Code cites the plan's numbered **decisions** and its named **rungs** — "topic
45's seventh decision", "topic 45's cadence rung". Those resolve here:

| Citation                                          | What it decided                                                                                                                                           |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| First decision (2026-08-13)                       | A point light is `POINT_FACES` atlas tiles, not a cube map                                                                                                |
| Second decision (2026-08-13)                      | Shadowed lights are chosen by projected screen influence, ties broken by light index, with hysteresis                                                     |
| Third decision (2026-08-13)                       | The atlas's tiles: the sun's cascades first, then one per spot and `POINT_FACES` per point, until they run out; a light with none still lights            |
| Fourth decision (2026-08-13)                      | One cull (`DrawGen`) per point light, not one per face; refined 2026-09-17 to per-face draw regions of the same generator                                 |
| Fifth decision (2026-08-14)                       | The sun's bias is denominated in texels of the cascade the fragment landed in                                                                             |
| Sixth decision (2026-08-14)                       | The slope is read off the rasterised facet (`geometric_normal_of`), not the shading normal                                                                |
| Seventh decision (2026-08-28)                     | The slope moves the receiver sideways along its facet normal (a normal offset), not towards the light                                                     |
| Eighth decision (2026-08-28)                      | The cascade switch is a band (`CASCADE_FADE_FRACTION`), both cascades sampled and mixed                                                                   |
| Ninth decision (2026-08-28)                       | A 32-tap rotated Vogel disc replaces the 3×3 box                                                                                                          |
| Tenth decision (2026-08-28)                       | PCSS: the sun's filter width comes from a blocker search                                                                                                  |
| Eleventh decision (2026-08-28)                    | A five-tap probe takes the disc only at an edge                                                                                                           |
| Twelfth decision (2026-08-31)                     | The coverage anchor `WHOLE_CELL_COVERAGE`, the conservative end of a measured sweep; biases follow the tile                                               |
| Thirteenth decision (2026-08-31)                  | `LEVEL_HOLD_RATIO` is the same fifth `ForwardRenderer::lod_hold_ratio` opens                                                                              |
| Fourteenth decision (2026-08-30, the user)        | The atlas is dynamic and cached; the unit is the cull group; a cached tile is not a bake                                                                  |
| Fifteenth decision (2026-09-04)                   | The filter is selected at runtime (`pcss`, `disc`, `box`) and the comparison seam is resolved per fragment                                                |
| The 2026-08-30 decision; the contact march        | Screen-space contact shadows, on for medium and high and off on low; built 2026-09-01 and parked                                                          |
| The atlas rung, the allocator rung                | The grid became an allocator (2026-08-30 / 31): `AtlasAllocator`, a forest of per-cell quadtrees                                                          |
| The priority rung                                 | `shadow::coverage` ranks lights and sizes their tiles (the twelfth and thirteenth decisions)                                                              |
| The cadence rung                                  | Item 3 of the atlas rung: the near cascade every frame, each one out at twice the period, a moving light or a camera cut resetting it (`shadow::cadence`) |
| The static-caching rung                           | A group's static casters rendered once and only dynamic ones redrawn — **unbuilt**; what is built caches a group's whole map                              |
| The ladder, the filter ladder, the quality ladder | The order below, taken 2026-08-27                                                                                                                         |

**What ships, and the frame of it.**

- **Cascades are sphere-fitted and texel-snapped.** `shadow.rs` fits a sphere
  about the eye rather than a box about the frustum, so turning the camera
  cannot change a cascade's extent, and quantises the light-space origin to
  whole texels.
- **GPU-driven all the way.** One cull dispatch per cascade and per shadowed
  light against the same pools, indirect draws into depth-only pipelines, no CPU
  re-traversal; skinned casters come free through the skinned-output pool, and
  nothing in the path depends on the `GeometryPath` or the binding model.
- **What casts.** Every live resident instance, unless
  `ForwardRenderer::set_instance_casts_shadow(handle, false)` sets
  `GpuInstance::CASTS_NO_SHADOW` (2026-09-24): every cull that is no view's — a
  cascade's and a shadowed light's — rejects on that bit through
  `crcbl_shaders::cull::Params::hidden_view`, so the instance lands in no tile
  and, since the reflective shadow maps draw the same survivors, bounces no
  probe light. Every camera still draws it and it still receives. A camera's own
  hidden-views bit does not stop an instance casting. Changing the flag moves
  `InstancePool::revision`, so every cached map redraws.
- **Under `LightingPath::RayTraced` the whole raster shadow path is bypassed,
  not augmented** — ray queries against the TLAS for every light type, no
  cascades and no atlas — which keeps the raster path free of ray-traced special
  cases. Unbuilt, with the rest of that path.

**Lights and the atlas (the first four decisions and the atlas rung).**

- **Tiles, not cubes.** Six tiles reuse one allocator, image, sampler and
  barrier story; a cube is a second image type, view type and sampling path. The
  cost is no hardware filtering across a face seam, mitigated by a border of
  padding per tile (unbuilt — see _What punctual-light shadows left owed_) and
  by PCF sampling within a face. A cube map is the better answer only if seam
  artefacts turn up in practice.
- **Selection by screen influence, the same metric family as LOD**, so there is
  one notion of "how much this matters on screen". Since 2026-08-31 it is
  `shadow::coverage` with `HOLD_RATIO` on the ranking and `LEVEL_HOLD_RATIO` on
  the tile size. It is deliberately **not** topic 25's helper (see _What the
  deleted 25-lod plan left behind_ above) — `GroupCost::projected_error` divides
  by the distance to a sphere's surface and `coverage` by the distance to a
  light's centre — topic 43's row (f) says why (_What the deleted
  43-render-standards plan left behind_ above); the two constants name each
  other.
- **A light that gets no tile still lights and does not occlude**, which makes
  the budget a quality knob rather than a correctness cliff. Spot was built
  before point because a point light is six of a spot plus face selection.
- **One cull per point light** because a `DrawGen` is about five megabytes of
  per-instance LOD hysteresis; six per point light would be thirty for one
  light. The union of the faces is the light's sphere, which is what the cull
  tests; the 2026-09-17 refinement tags each survivor with the faces its box
  reaches and draws each face's own region, still one `DrawGen`.
- **The allocator is a forest of per-cell quadtrees** (Doom 2016 and Unity HDRP
  ship the shape), because the atlas is neither square nor a power of two in
  cells; asking every root for its whole self reproduces the old grid texel for
  texel. `SHADOW_ATLAS_COLUMNS` × `SHADOW_ATLAS_ROWS` cells of `SHADOW_TILE`
  hold the sun's cascades, two point lights and two spots.
- **The tile is the binding constraint on quality.** The 2026-08-26 re-tiling
  bought a second point light by shrinking `SHADOW_TILE` from 1024 to 768 texels
  rather than growing the image. Whether to grow it back is open — see the
  backlog.

**Bias (the fifth to seventh decisions).**

- **Texels, not clip depth.** A clip-depth bias meant that number times the
  cascade's whole depth range, `2 · radius + CASTER_REACH`: 0.83 m of world
  slack on the outer cascade against 0.15 m walls. Offsetting by a multiple of
  one texel's footprint (`2 · radius / TILE`) puts both light types in one unit
  that scales with the map, and biases a near cascade proportionally less.
- **The slope comes off the facet.** `geometric_normal_of` is
  `cross(ddx(world_position), ddy(world_position))`, computed in `fragmentMain`
  and passed in, because derivatives exist only in a fragment stage. **Its sign
  is aligned to the shading normal, never hard-coded**: on radv the bare cross
  product was measured anti-parallel to the authored normal, and the other three
  targets were not measured. Both light types read the same facet, because acne
  is a property of the rasterised triangle.
- **The normal offset moves the receiver sideways.** A move towards the light
  raises the compared depth, so enough to clear acne also lifts a shadow off its
  caster; a move along the facet normal by `sin(acos(Ng·L))` changes which texel
  is read and leaves the depth alone, and `sin` is bounded where `tan` needed a
  clamp. Only the constant term still travels light-ward. On lantern it took the
  wall-foot strip from 0.391 m to none and the cornice lift from 78.3 to 11.7
  luma. `NORMAL_OFFSET_TEXELS` is 2 because two outer-cascade texels are 125 mm
  against lantern's 150 mm shell and three would pass through it;
  `DEPTH_BIAS_TEXELS` fell to one and rose to 1.5 with the ninth decision. **The
  cost is a scalloped fringe** a couple of pixels deep at a silhouette's foot
  (backlog: _The normal offset scallops one silhouette's foot_). Both counts are
  console variables, `r_shadow_bias` and `r_shadow_normal_offset`, and
  `apps/sundial` walks one against the other. `crcbl_shaders::mesh`'s
  `both_shadow_lookups_offset_along_the_facet_normal` holds the direction.

**The cascade band (the eighth decision).** Inside `CASCADE_FADE_FRACTION` (a
tenth) of the selected cascade's reach, `sun_visibility` samples both cascades
and mixes by distance, because the near texel is a sixth of the outer one's here
and both biases and the maps change across the switch. On lantern's split circle
the switch's own step fell from 33.70 to 5.76 mean; **a tenth is the knee** — a
twentieth leaves the ramp steeper, a fifth and more give the step back. Only
fragments in the band pay a second `tile_pcf`. **The cascade tint is reported by
`sun_visibility` itself**, so `DebugView::Cascades` cannot draw a boundary the
lighting does not have; its sentinel is negative so no existing debug threshold
sees it. `DebugView::ShadowAtlas` is **a pass, not a branch**, drawn after the
tonemap in display space, because the atlas is one image the whole frame shares
and a readout that moves with exposure cannot be compared.

**The filter (the ninth to eleventh and fifteenth decisions).**

- **A 32-tap Vogel disc of two tile texels, turned by one of sixteen rotations
  an ordered-dither matrix picks off the pixel.** Integer-indexed, because a
  float hash and a platform `sin`/`cos` differ between drivers and a shadow
  comparison is binary. **Vogel, not Poisson**: its radius `sqrt((i + 0.5) / n)`
  and angle `i π (3 - sqrt 5)` are re-derived by
  `the_shadow_discs_are_the_vogel_spirals_they_claim_to_be`, where a Poisson set
  is constants from a program nobody kept. Thirty-two taps is where the dunes'
  grain fell back to the box's (0.918 against 0.827; 24 taps 1.099), narrowing
  the disc does not substitute for taps, and the dither matrix measured a fifth
  less grainy than the `(3x + 4y) mod 16` lattice, whose constant difference
  draws diagonal stripes.
- **PCSS for the sun alone.** `sun_penumbra_texels` reads sixteen depths with
  `Load` over eight texels — a comparison sampler cannot return a depth and a
  filtering one would invent a blocker height — keeps those nearer the light
  (reversed-Z: larger) and turns the height into a width by a similar triangle.
  **Clamped at both ends**: below at `SHADOW_FILTER_TEXELS`, above at
  `SHADOW_SEARCH_TEXELS`, and a search that finds no blocker takes the lower
  clamp, not "lit", since a thin caster can fall between search taps. **The
  physical sun is a no-op at this tile**: `tan` of its angular radius is
  0.004634, a blocker needs 4.6 m of separation to reach two near-cascade texels
  and 27 m on the outer one, and lantern differed in 36 bytes of 4,915,200. So
  `SHADOW_SUN_TAN_RADIUS` is a softness knob; the shipped 0.02 won an
  edge-wobble sweep on lantern's far boundary (0.58 px against 1.58 fixed), past
  0.03 the estimate saturates. Punctual maps pass `SHADOW_FILTER_TEXELS` — their
  depths are perspective and they have no angular radius — and so does
  `volumetric.slang`, since a froxel has no surface below a caster.
  `SHADOW_CASTER_REACH` has one declaration in `crcbl_shaders::mesh`, so the
  host's box and the shader's inverse cannot drift.
- **The probe.** `tile_pcf` takes `SHADOW_PROBE_TAPS` first — tap 0 at radius
  0.125 and taps 23, 25, 27 and 29, a ring about a quarter turn apart — and
  returns a flat answer when they agree. **Unanimity is exact**: wherever a 2×2
  neighbourhood agrees the comparison sum is exactly 0 or 1, so the arms are
  `probe <= 0.0` and `probe >= float(SHADOW_PROBE_TAPS)` with no margin. At an
  edge the full disc re-reads the five, so an undecided fragment shades bit for
  bit as before; no golden moved. It cut `forward` 27% on radv and 28% on
  llvmpipe — **the cost of a rung is taps, not divergence**.
  `the_shadow_probe_is_a_ring_about_a_centre` re-derives the index set.
- **The selector.** `crcbl_render::shadow::r_shadow_filter` selects `pcss` (the
  default, and what every golden is blessed under), `disc` (no blocker search)
  or `box` (the 3×3 hardware PCF kernel recovered from before 713da9d, through
  the same `tile_tap`). **A uniform branch, not three pipelines**: this is a
  scene pass, and a pipeline switch would draw every triangle twice to put two
  filters in one frame. **The seam is per fragment** —
  `FrameUniforms::shadow_filter` carries both lanes' modes and the column,
  `crcbl_render::split::halves` owns where it falls. Every bias decision applies
  under all three. `volumetric.slang` always takes `disc`.

  Priced 2026-09-04, `apps/sundial --headless --sun-paused` at the goldens'
  pose, `forward` p50 median of five runs (Mesa 26.2.2):

  | Filter | radv, 1920×1080 | llvmpipe, 960×720 |
  | ------ | --------------- | ----------------- |
  | `pcss` | 0.228 ms        | 8.586 ms          |
  | `disc` | 0.199 ms        | 7.349 ms          |
  | `box`  | 0.180 ms        | 6.914 ms          |

  No two ranges overlap; the `shadow` pass is flat across all three (the filter
  is on the sampling side), and the whole ladder is 0.068 ms of a 0.649 ms radv
  frame. Which rung a tier takes is the user's call.

**The priority and hold (the twelfth and thirteenth decisions).**
`shadow::coverage` is how much of the frame's **height** a light's map covers —
a fraction, so a 256×192 golden is evidence about 1080p, and the map's footprint
rather than the light's sphere, so a narrow cone is not demoted for being
narrow. **`WHOLE_CELL_COVERAGE` is a quarter of the frame's height**, the
conservative end of a sweep bounded by fixtures that must not move:
`Scene::PointShadow` 3.06, `Scene::SpotShadow` 1.41, lantern's lamp 1.06 at its
worst phase and its corner downlight 0.37, which binds. **Every bias follows the
tile**: `tile_texels(rect)` reads the map's side from the rectangle the shader
is handed, so a demoted light is not biased by a footprint four times too small
(`crates/crcbl/tests/mesh_e2e/shadow_tiles.rs` holds it). **`LEVEL_HOLD_RATIO`
is deliberately the fifth `ForwardRenderer::lod_hold_ratio` opens** — moving one
without the other makes a light and its mesh disagree about when a rung is worth
taking. A light with no history starts coarsest and climbs, so a frame's first
answer is reproducible.

**The cache (the fourteenth decision, the user's).** A light re-renders its
tiles when it or an instance it covers moves, and not otherwise. **A cached tile
is not a bake** — nothing survives a load. **The unit is the cull group** (a
cascade, or a light slot's run of tiles), because a point light's faces draw one
visible set; per-face granularity was declined (see _Per-face granularity inside
a point light's cube: declined_ below). `mesh.slang`'s `depthClearVertexMain`
clears one group, since a pass-wide `LoadOp::Clear` is the only depth clear the
seam offers.

**Contact shadows (decided 2026-08-30, built 2026-09-01).** A short march along
the light through the depth prepass closes the contact gap no bias or filter
can. Decided as its own `RenderEffects` bit in `DEFAULT_STACK`, not a settings
row, with the low preset clearing it — **which cannot both hold**:
`crcbl::settings::presets` clears an effect through its `VIDEO_KEYS` row, so the
build took "no row" and the low tier does not clear it. **The Hi-Z pyramid is
deliberately not read**: this ray is `MAX_STEPS` texels long, so every cell a
pyramid would skip is one the march was about to leave. The bit is parked
outside `DEFAULT_STACK` until a flip that re-blesses every golden.

**The ladder, in the order it should be climbed** (2026-08-27), named two things
beyond what ships: the tile resolution, which is a question rather than a rung,
and contact shadows. Static caching is the rung above the cadence.

**Considered and declined:**

- **VSM and EVSM.** Moments make a map filterable, and they light-leak through
  thin geometry because two depths summarised into one distribution admit a
  receiver between them. A leak is a correctness artefact — light where the
  scene has none — where every rung above trades quality for cost.
- **Virtual shadow maps.** A page table, a feedback buffer and a page cache — a
  topic, not a rung, since it replaces the tile grid rather than improving it;
  if ever wanted it gets its own document.
- **A cube map per point light** — see the first decision; revisit only if seam
  artefacts show. Dual-paraboloid point shadows are declined too, under _The
  atlas re-tiling's leftovers_ below.
- **One shared importance-and-hysteresis helper with LOD** — refused for the
  reason under the second decision.
- **Per-face cadence inside a point light's cube** — declined below under its
  own heading.

## What the deleted 46-ambient-occlusion plan left behind (2026-09-24)

Record; the plan was built, and what it left open is in `docs/backlog.md` under
_What GTAO left owed_, _What the bent-normal slice left owed_, _What the
occlusion view left owed_, _What screen-space AO left owed_ and the raster
lighting stack's bullet on the low tier's scalar pass. It specified the
occlusion chain: a depth prepass, a gather at half resolution (GTAO in
`ssao.slang`, or the eight-tap hemisphere in `ssao_hemisphere.slang`, chosen by
`crates/crcbl-render/src/ssao.rs`'s `r_ssao_technique`), a depth-weighted blur,
a depth-aware upsample and a consumer in `mesh.slang`. The shader headers
describe the pass as it stands; what follows is the rules and their reasons,
which live nowhere else.

- **The prepass is the shadow pipeline, and the forward pass trusts its depth.**
  `shadow_pipeline` is already the depth-only twin of the colour pipeline, so
  driven with the camera's group and draws it is the scene depth prepass
  (`prepass_groups`) with no new pipeline, shader or bind group. The colour pass
  then tests `GreaterOrEqual` read-only (`MeshModules::color_depth_stencil`).
  `LoadOp::Load` with `Greater` **cannot work**: the prepass wrote the identical
  depth and `Greater` rejects equality, so the frame goes black. The overdraw
  win rests on `SV_Position.z` being bit-identical between the two pipelines,
  which nothing decorates for; a rasteriser that breaks it draws **holes**, and
  the fallback (clear and write depth again) is taken by saying so in the code,
  never by re-blessing a golden around it.
- **AO scales `frame.ambient` alone.** It is produced before the forward pass
  and read inside it as an integer `Load` at `SV_Position.xy`: no sampler, no
  UV, no filtering for four backends to disagree about. **Multiplying the
  tonemap's input is refused**, because it darkens direct light and highlights
  too.
- **Normals are reconstructed from depth, with the four-tap closest-neighbour
  rule.** The two-tap `ddx`/`ddy` form straddles the depth discontinuity at
  every silhouette and draws a dark rim round every object. **A normal
  attachment is refused for AO**: the prepass has no colour target, so it would
  cost a third geometry pipeline per `GeometryPath` and a new fragment entry
  point for a buffer one pass reads, and a wrong reconstructed normal costs a
  pixel only an eighth of its occlusion. The attachment is reserved as the
  remedy for the SSR escalation clause (_What the deleted 47-reflections plan
  left behind_, below), whose trigger is a one-pixel fringe of unrelated colour
  at silhouettes. Escalating is contained to the prepass pipeline and the
  gather's first lines.
- **The determinism rule.** Rotation comes from an integer-indexed constant
  table (`pixel.xy & 3` into sixteen entries), never a float hash, and **the
  blur is not optional**. One binary depth comparison landing on its threshold
  resolves differently on two drivers and swings a pixel by an eighth, far past
  `Tolerance::RASTERISER`; interleaved-gradient noise and `frac(sin(…))` hashes
  amplify float differences by construction, and an integer index is
  bit-identical by inspection. The blur's footprint is the noise tile, so it
  removes the radial banding and divides an isolated flipped sample by as many
  taps as count. It does not remove the tangential banding alone: that takes
  more slice orientations _and_ the wider footprint of a second blur together,
  which is why `r_ssao_slices` and `r_ssao_blur_passes` default to four and two.
- **Continuity is why GTAO replaced the hemisphere, not quality.** A horizon
  integral moves by a hair when a driver disagrees in the last bit, where a sum
  of binary comparisons cliffs, so the technique that looked riskier for the
  goldens is the safer one. **Every angle goes through
  `crcbl_shaders::ssao::acos_approx`** (Abramowitz and Stegun 4.4.45), never the
  target's `acos`, whose accuracy no shading language specifies; the sweep
  against `f64::acos` bounds `MAX_ACOS_ERROR` from below as well, so the
  intrinsic cannot pass it.
- **The trap: the slice tilt is signed against the view-orthogonal tangent, not
  the view axis.** The two coincide only at the frame's centre. Signed against
  the axis, every off-centre pixel puts both horizon clamps on the wrong sides
  and a flat floor picks up a smooth wash growing towards the edges — a
  vignette, which looks like a thing renderers have and would have been blessed.
  `probes`' flatness assertion caught it (three levels against a half-level
  allowance); `the_slice_tilt_is_signed_against_the_view_orthogonal_tangent` is
  the guard.
- **The two bodies are two shaders, not a branch.** The hemisphere is a
  threshold comparison and needs a depth bias (`DEPTH_BIAS_RADII`, a share of
  the sampling radius because the radius is a console variable); a horizon
  integral must not have one, since a sample in the surface's own plane lands on
  the tangent where the integral is stationary. And `Shader::entry_point`
  answers `None` for a stage with two entry points, so two fragment entries in
  one module is a shader nothing can build a pipeline from. They share
  everything else — block, bindings, layout, ring, cache, blur, upsample — which
  is why `r_ssao_technique` reaches a pipeline where every other knob reaches a
  uniform lane. The hemisphere writes no bent direction.
- **The blur weights on view-space Z, as a ramp, never a cut.** A reversed-Z
  delta is not a distance — the same metre is enormous near the eye and nothing
  near the far plane — so the blur unprojects as the gather does, and binds the
  same `SsaoParams` block rather than one of its own. `if (abs(dz) < t)` would
  put a binary decision on the output pixel, which is what the rotation table
  keeps off the input. The tolerance derives from the AO radius, the only length
  the pair has, rather than a uniform nobody adjusts. **The far-plane test is
  the one comparison that stays**, because it compares against an exact
  constant, so two drivers both take it or neither does. The upsample likewise
  keeps a floor on the nearest tap (`NEAREST_TAP_FLOOR`) so a pixel whose every
  tap is rejected still has a divisor.
- **The off-switch is data, not a branch, and the fetch is clamped.** AO has no
  device fact to gate on, and inventing a capability that is really a
  performance opinion is what the capability rules in `docs/notes/backends.md`
  exist to prevent. With the pass off,
  `ForwardRenderer::ambient_occlusion_placeholder` binds an uploaded 1×1 holding
  `AMBIENT_OCCLUSION_NONE` (a clear cannot carry the direction sentinel), and no
  occlusion pass is recorded at all. A `Load` outside an image's extent yields
  **zero**, not its one texel, so the consumer clamps against `GetDimensions` —
  unclamped, the first AO-off frame was black wherever ambient was all the
  light, with nothing reporting an error. `forward_e2e::depth_probe` asks for
  the clamp on every backend.
- **The golden is not the instrument; a structural ratio is.** A pass writing a
  constant 1.0 draws a plausible frame. The check is a band inside a concave
  corner measurably darker than a band on the same surface outside it — same
  normal, distance and lights — in the shape of `SPOT_SHADOW_RATIO`. It survives
  driver drift and fails a no-op pass, an inverted normal and a result that
  never reaches the shading line.
- **The multi-bounce tint is clamped at one on purpose.**
  `multi_bounce_occlusion` is Jimenez et al. 2016's fit, unconditional on every
  tier because it reads no second target. Its coefficients sum to a hair over
  one above an albedo of 0.8, and with AO off every fragment arrives at full
  visibility, so the top-end `min` is what keeps an AO-off frame identical to
  the frame before the pass existed. It is the one departure from the paper.
- **The bent direction sums turns of the normal, not bisectors.** Every slice
  plane contains the eye, so summing bisectors pulls the answer towards the view
  direction by an amount set by screen position — measured at seventeen degrees
  off an unoccluded plane two thirds across a 1920-wide frame. Each slice
  instead turns the normal by its `gamma`, so an unoccluded pixel gets its own
  normal back exactly. **A zero-length direction is the sentinel** for "nothing
  to measure" and for "no pass ran" alike (`BENT_NORMAL_NONE`,
  `BENT_NORMAL_MIN_LENGTH`), and `bent_normal_at` answers it with the fragment's
  own shading normal; nothing reconstructs a normal in the consumer. The
  sentinel decodes to a short vector rather than zero, so `decode_bent` in both
  filters resolves a tap to a unit vector or to exactly nothing. The flat
  `FrameUniforms::ambient` term is not steered: a constant has no direction.
- **One format on every tier; a tier turns off arithmetic, not bandwidth.** The
  2026-08-30 split put scalar occlusion plus the tint on low and bent normals
  plus specular occlusion on medium and high. A per-tier format would mean a
  second pipeline, bind-group layout and `mesh.slang` binding type, so
  `TransientImageDesc::ambient_occlusion` is `Rgba8Unorm` everywhere and
  `[engine.video] ssao_bent_normals` (off on the Low preset) is the switch.
  Which scalar body low runs is still a measurement. The direction costs the two
  filters 0.002 ms and 0.004 ms at 1920×1080 on radv (2026-09-02); the format
  itself has never been priced.
- **Specular occlusion needs a cone angle the channel does not carry, and until
  it exists the SSR row's refusal of specular occlusion (_What the deleted
  47-reflections plan left behind_) stands.** A scalar AO is the wrong term for
  a reflection. The chosen encoding (an octahedral direction in `.gb`, a cone
  angle in `.a`, and GTSO) is in `docs/backlog.md`.
- **Declined: HBAO and HBAO+.** They read the same depth and GTAO supersedes
  them on it, so one is a step onto a rung already obsolete. **Declined: a
  frame-sized transient cleared to 1.0** (`ssao-none`), which shipped first and
  was correct but strictly dearer than the placeholder.

## What the deleted 47-reflections plan left behind (2026-09-24)

Record; the screen-space row was built — the Hi-Z march in `ssr.slang` over
`crcbl_render::hiz`'s pyramid, the blur that is also the composite in
`ssr_blur.slang`, and the probe and prefiltered-sky miss fallback, all in
`crcbl_render::ssr`. The ladder above it is not: **rung 1** the Hi-Z march
(built), **rung 2** planar reflections, **rung 3** cone tracing over a colour
mip chain, **rung 4** ray-traced reflections at P7C, with temporal accumulation
beside them. Those, and the plan's considered-and-declined list, are in
`docs/backlog.md` under _The reflection ladder's upper rungs are unbuilt_ and
the entries after it. The shader headers describe the passes as they stand; what
follows is the rules and their reasons.

- **One reflectivity attachment on the forward pass, and no G-buffer.**
  `Rgba8Unorm`: `rgb` is `F0`, `a` is the roughness quantised to
  `crcbl_shaders::ssr::REFLECTIVITY_LEVELS` (since 2026-08-29; before that it
  held the sharpness ramp, which left the fallback blind to roughness past the
  cutoff), and `NO_REFLECTION` — no `F0`, fully rough — where nothing drew,
  because a zero alpha reads as a mirror. **The AO chain's refusal of a normal
  attachment does not transfer**: every clause of it is a fact about the depth
  prepass, which has no colour target. On the forward pass a further target is
  one more `ColorTargetState` element under the same fragment entry, no new
  pipeline and no new interpolant. The attachment gains a field only when a pass
  reads it, never because a G-buffer "should have" one.
- **The escalation clause.** The normal is reconstructed from depth by the AO
  pass's four-tap technique (`normal_at`, declared independently in each
  shader), which is exact on a plane and wrong on a one-pixel rim at every
  silhouette. For AO a wrong normal costs an eighth of a pixel's occlusion; for
  SSR it is a wrong ray fetching an arbitrary colour. **If a one-pixel fringe of
  unrelated colour appears at silhouettes, the fix is a second attachment
  carrying the view-space normal, never a tuning of the march.** It is contained
  to the fragment stage's return struct, one target state, one transient and the
  first lines of `ssr.slang`, and moves no golden because only SSR reads it.
- **The march is in screen space, and its reach is a share of the frame.** A
  world-unit step is tens of pixels near the eye and a fraction of one far away;
  a pixel step makes the loop bound a property of the screen, which is the whole
  cost on CI's software rasterisers. The reach is
  `REACH_FRACTION * min(width, height)`: a fixed pixel reach made reflections
  _shorter_ as the window grew (`apps/lantern`'s panel reflection, asserted at
  256×192, was absent at 1280×960). Since 2026-08-27 the stride is hierarchical:
  a **`max`** Hi-Z pyramid (one value per texel, the nearest surface below it —
  a min-max pair would be a second image for a bound nothing reads), crossing a
  whole empty cell per step. The segment is clipped to the viewport before the
  walk and ends on a border ramp; the ray starts off the surface along the
  normal, is clipped against the near plane, and fades when it points back at
  the viewer.
- **Behind a surface is no evidence.** A tap is a hit only within a thickness
  bound derived from the ray's own depth advance and floored by
  `THICKNESS_FLOOR`; past it the march continues. Treating any "behind" as a hit
  is the comet-tail smear off every silhouette. **No binary-search refinement**:
  the crossing is interpolated between the last two taps.
- **Determinism: no jitter, and every weight reaches zero where the decision is
  fragile.** A march has no denominator — the first tap whose comparison flips
  is the answer — so the pixels two drivers can disagree on are made, by
  construction, the ones multiplied by almost nothing (distance, border,
  thickness and backward fades). That reduces the exposure; it does not bound
  it. **SSR goldens are review aids only**: every real check is a structural
  ratio between two blocks of one frame, and fixtures reflect large,
  low-frequency content. **If a golden flaps between CI legs, flatten that
  fixture's reflected content or drop the golden and keep the ratio — never
  widen the tolerance, never re-bless per driver.** The Hi-Z pyramid landed on
  those terms; the one number it moved was `apps/lantern`'s `SSR_HIT_TOLERANCE`.
- **The roughness cutoff stays at 0.5 and gates only the screen march.**
  `crcbl_shaders::ssr::ROUGHNESS_CUTOFF`; `GpuMaterial::UNTINTED`'s 0.5 encodes
  as exactly zero sharpness on every target, so nearly every surface skips the
  march and takes the probe specular, which is more honest for a broad lobe than
  one ray. Raising the cutoff puts every `UNTINTED` surface into the march — a
  far larger blast radius than the filter — so it is its own slice with its own
  decision.
- **The blur is the composite.** `ssr.slang` writes the reflection alone into an
  `Rgba16Float` transient; `ssr_blur.slang` filters it and adds the scene colour
  into a second one, which `add_passes` returns in place of the scene colour. A
  frame without the pair returns the old id and is bit-identical: that is the
  off-switch. The kernel is the AO blur's plus a roughness weight; positive
  sharpness blends `lerp(centre, filtered, sqrt(sharpness))` (a linear share
  measured 8.46–8.48 levels of row bend on lavapipe, WARP and Metal against the
  fixture's limit of 8; the square root 4.82 on lavapipe). The depth tolerance
  is `THICKNESS_FLOOR` times `DEPTH_TOLERANCE_THICKNESSES`, because at one
  thickness the filter switches itself off on a floor seen at a shallow angle.
- **A miss returns the probe environment plus the prefiltered sky, times the
  split-sum `env_brdf`**, weighed by the probe's Chebyshev visibility
  (`probe_weight`). Exact zero needs a zeroed probe volume **and** a black sky.
- **What the row refuses, and why** — each is also in the backlog's declined
  entry:
  - **No history.** Reading last frame's colour makes a golden a function of how
    many frames were drawn before it.
  - **No specular occlusion from the AO scalar.** AO scales the ambient term
    alone; a highlight and a reflection do not take the same factor. It is its
    own term (the GTSO decision in `docs/backlog.md`).
  - **No SSR on transparency.** A transparent surface writing the reflectivity
    attachment overwrites the opaque `F0` behind it while the scene colour there
    is a blend, so a blended surface must write it with an empty mask
    (`docs/plan/53-transparency.md` specifies that; the pass is unbuilt).
  - **No half-resolution march, by measurement**: headless `apps/lantern` at
    960×720 on radv (2026-09-05) put `ssr` at 6.3% of a 2.165 ms frame,
    `ssr-blur` 0.6% and the five `hiz` levels 1.0%, against `shadow` 15.6% and
    `forward` 18.1% (17.4% without its fused clears). On a hardware browser
    (quarry, Chrome, 959x463, 2026-09-04) `ssr` was 0.053 ms, 7.2% of 0.737 ms.
    Quote a share and an absolute together: that frame grew from 1.27 ms as
    passes landed, and the march's share fell while its cost rose.
  - **No `LightingPath` gate**, which still has no consumer.

## What the deleted 48-post-processing plan left behind (2026-09-24)

Record; most of the stack is built — the `Rgba16Float` scene, the tonemap
(`tonemap.slang`, `crcbl_shaders::tonemap`), auto-exposure with adaptation
(`exposure.slang`, `crcbl_render::exposure`), bloom (`crcbl_render::bloom`), the
render-scale upscale (`upscale.slang`, `crcbl_render::upscale`), the camera
stack as RON (`crcbl_render::stack`) and the four-layer toggle resolution
(`crcbl_render::effects`). What is not is in `docs/backlog.md` under _Colour
grading, the post-tonemap LUT, is specified and unbuilt_, _Pass fusion: the
tonemap's luma and the histogram's quarter level are unbuilt_, _Depth of field
and lens artefacts are missing, and follow colour grading_ and _What the
camera-stack slice left_. What follows is the rules and their reasons.

- **The order is a contract.** Scene (HDR `Rgba16Float`, lit in linear from the
  start — retrofitting HDR is repainting every material) → bloom → exposure,
  tonemap and grade → the antialiasing resolve → the upscale → the UI at native
  resolution. Every stage before the upscale runs at the internal extent
  `ForwardRenderer::set_render_scale` chose, which is the reason for the order:
  each stage costs what the internal extent says. The UI composites after the
  upscale so glyphs are rasterised sharp and never filtered.
- **Additive zero, or a deliberate re-bless.** A post feature lands behind a
  form where "off" is bit-identical to the frame before it existed: at full
  scale there is no upscale pass and no second image (the stage before writes
  the caller's target directly), a blend of exactly 0 or 1 takes a branch that
  writes the endpoint itself, an unmeasured frame's tonemap reads the host's
  exposure through a lane rather than the buffer. An effect with no such form —
  every AA tier, auto-exposure, TAA — moves every golden it is on for, so
  putting it in `RenderEffects::DEFAULT_STACK` is a decision and a re-bless
  taken once, never a side effect.
- **No transcendental reaches a pixel, so ACES rather than AgX.** AgX takes a
  `log2` and a `pow` per channel, and four platforms' implementations differ in
  the last place. Hill's fit of the ACES RRT and ODT is two changes of primaries
  around a rational polynomial — multiplies, adds and divides — so it can be
  blessed on all four backends. `crcbl_shaders::tonemap::TonemapCurve::apply` is
  the same arithmetic on the CPU, pinned against the ODT's published anchors,
  and a source grep holds the shader to the same constants.
- **The tonemap operator is per view.** The fit is `ForwardRenderer`'s default
  because it shades in linear HDR; the clamp is the identity on `[0, 1]`, so
  display-referred content and a fixture predicting a code value from a host
  model keep it. A debug view resolves to the clamp whatever a caller set
  (`ForwardRenderer::resolved_tonemap_curve`): a readout's pixels are data.
- **Auto-exposure's rules.** The histogram bins by the float's exponent field —
  integer arithmetic, not a `log2`. The reduce is **one invocation** because
  float addition is not associative and a tree sums in the order the device
  schedules. The adaptation step is **linear**, `rate * delta` clamped into
  `[0, 1]`, not `1 - exp(-rate * delta)`, for the transcendental rule; the two
  rates differ by direction, as an eye's do. The previous value is the
  `measured` ring's slot behind the one written, pre-filled with the default
  exposure so the first step starts somewhere defined. It is out of
  `DEFAULT_STACK` because it has no additive-zero form, and the rates are an API
  (`ForwardRenderer::set_exposure_adaptation`) rather than a settings key
  because `crcbl-render` has no clock to take a delta from.
- **Bloom is a lens, and a camera given no stack has been given no lens** — the
  reason `DEFAULT_STACK` leaves it out, recorded on that constant.
- **Toggle layering, resolved in one place.** Camera stack (what the view wants)
  → `[engine.video]` (may only remove; an absent key clamps nothing) →
  programmatic (either way) → device (last and absolute).
  `EffectRequest::resolve` applies the order and `ForwardRenderer::begin_frame`
  freezes the answer per frame, so the halves of a frame cannot disagree. The
  device layer removes nothing today, and that is a fact about the effects
  rather than an unfinished clamp; its first real rule arrives with the
  ray-traced variants `LightingPath` selects.
- **The camera layer is per view and is a file.** `CameraStack` holds one
  optional pass per `RenderEffects` bit; a render-to-texture monitor, a planar
  reflection or a scope's PiP does not want reflections or GI of its own, and
  that is a property of the camera, not of the player's hardware. **A pass
  parameter is a field on that pass's type**, added when the parameter has a
  serialized form — the antialiasing slot is the only one so far, and the
  colour-grading LUT path is designed as the next.
- **The engine does its own scaling**, decided 2026-09-21: the plan's
  `ShellCaps::HW_UPSCALE` "free half" is not a path to build towards — see
  `docs/backlog.md`'s _The engine owns scaling on every platform_.

## What the deleted 49-antialiasing plan left behind (2026-09-24)

Record; the first two rungs and the settings row are built — FXAA 3.11
(`fxaa.slang`, `crcbl_render::fxaa`, `RenderEffects::ANTIALIASING`), CMAA2
(`cmaa2_edges.slang`, `cmaa2_shapes.slang`, `cmaa2_apply.slang`,
`crcbl_render::cmaa2`, `RenderEffects::CMAA2`, the default tier since
2026-09-06) and one `antialiasing` row (`crcbl_render::Antialiasing`). What is
not is in `docs/backlog.md` under _MSAA was reopened rather than reversed_, _TAA
is unbuilt: jitter, history, and the golden decision it owes_, _What the CMAA2
slice left_ and _Considered and declined for post-processing and antialiasing_.

Code cites the plan's decisions by number, so the numbering is kept:

- **The seventh decision (2026-08-27) — MSAA reopened and priced.** The old
  rejection ("fights deferred-ish/HDR pipelines") is deferred-renderer
  reasoning; this renderer is clustered forward, and topic 44 rejected deferred
  partly _because_ it fights MSAA (see _What the deleted 44-lighting plan left
  behind_ above). `crcbl_hal::MultisampleState` has always carried `samples` and
  `alpha_to_coverage`. The price is a multisampled depth prepass and one depth
  resolve before `ssao.slang`, `ssr.slang` and the Hi-Z pyramid. **MSAA is right
  for a forward renderer doing little screen-space work**; FXAA and CMAA2 are
  right for this one as long as SSAO and SSR are in the stack. Opt-in, never the
  default: the software and browser tiers pay for every sample.
- **The eighth decision (2026-08-30) — one AA row, with MSAA as its top rungs.**
  Counter-Strike 2's row is the model: None, CMAA2, MSAA 2×/4×/8×, and no
  temporal option (its filtering row is what `apps/options`' `ANISOTROPIES`
  matches). **Rung 1**, the cycler row, is built; **rung 2**, CMAA2 in SMAA's
  place, is built; **rung 3**, MSAA, is not.

The rules and their reasons:

- **The resolve slot holds one filter.** `Antialiasing` is the ladder (`None`,
  `Fxaa`, `Cmaa2`) and `CameraStack` has one `antialiasing` field naming a tier,
  never a field per bit, so a file cannot ask for both. `EffectRequest::resolve`
  applies the `[engine.video] antialiasing` tier as a **replacement** inside the
  slot — after the video clamp, before the programmatic override and the device
  — because a clamp can only remove and could not choose the higher tier where
  the camera asked for FXAA. It is the first non-clamping video key. A file
  still holding the old boolean reads as it meant: `true` is an unpicked tier,
  `false` is `Antialiasing::None`; neither warns.
- **A fixture that wants no resolve names `Antialiasing::SLOT`**, never one
  tier's bit: forcing `ANTIALIASING` off under the CMAA2 default left CMAA2
  running, and a third rung would walk past such a fixture the same way.
- **Readouts and transparent views take no resolve.** Every debug view but
  `DebugView::Shaded` drops both tiers in `ForwardRenderer::resolved_effects`: a
  pixel's colour is a legend reading, and a blend invents one no cluster holds
  (`apps/quarry`'s colour-count tests went from 2 colours to 64 without it). A
  `ViewBackground::Transparent` view is refused either tier
  (`ViewBackground::REFUSED_ON_TRANSPARENT`): its alpha is coverage, and an edge
  filter would mix an edge pixel with the transparent black beside it — a
  premultiplied colour under a straight-alpha consumer, a dark fringe. The
  upscale does not apply to such a view for the same reason.
- **A resolve changes the frame's shape rather than adding a pass.** With the
  slot empty the tonemap writes the caller's target; with it filled the tonemap
  writes a `display-color` transient at the target's format and the resolve
  writes the target. The ground grid draws with the tonemap, so it is filtered
  (thin high-contrast lines are what an edge filter is for); the UI draws after,
  so it never is.
- **FXAA samples with `SampleLevel` everywhere.** WGSL refuses an implicit-LOD
  sample reached from non-uniform control flow, and every tap in the filter is;
  the native targets compiled the implicit form silently, and only
  `web/run-render-harness-e2e.sh` caught it. **Its luma is corrected to gamma**:
  the tonemap writes linear values into an sRGB target, so the resolve samples
  linear, and FXAA's thresholds were fitted to gamma space. Its template is
  `bloom_composite.slang` (a neighbourhood through an `inv_source` texel size),
  not the 1:1 nearest-sampled `tonemap.slang`.
- **CMAA2's accumulation is integer fixed point**
  (`crcbl_shaders::cmaa2::BLEND_FIXED_POINT_SCALE`), because shares arrive in
  scheduler order and float addition is not associative;
  `the_same_frame_resolves_to_the_same_bytes_twice` holds it. **Nothing is
  queued and nothing is dropped**: both working buffers hold one entry per
  pixel, so no capacity decides which entries survive (_CMAA2's append lists
  made a dense frame a function of the schedule_, below). **No lookup table**,
  so nothing is cooked. **Historyless, so golden-safe** — the property TAA
  lacks. Compute for the two analysis passes (two per-pixel storage buffers
  against `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE`) and a fullscreen draw
  for the apply, because a swapchain image cannot be bound as a storage image.
- **An observer that counts touched pixels is not enough.**
  `cmaa2_changes_a_band_along_the_edges_and_nothing_else` stayed green while
  `cmaa2_shapes.slang` blended the wrong side of every edge;
  `the_resolve_moves_the_silhouette_toward_a_supersampled_reference`, which
  holds the resolved frame against a supersampled unresolved one, is what sees
  direction. The FXAA fixture (`Scene::Aa`) pins a band — at least
  `AA_MIN_SOFT_PIXELS` soft pixels with the resolve, four times fewer without,
  mean moved by at most `AA_MEAN_TOLERANCE` — never a run's own numbers. A new
  rung owes both kinds of check.
- **A new AA bit is not free.** `crcbl_render::effects`' `NAMES` table must name
  it, `every_effect_is_named_exactly_once_and_the_row_prints_them` pins the row
  string every sample prints, and `ForwardRenderer`'s `RENDER_PASSES` and
  `fullscreen_passes` must count its passes so the frame's timer count matches.
- **The cost protocol, and what CMAA2 cost (2026-09-06).**
  `apps/lantern --headless --frames 400 --size 1920x1080 --backend vk --stack <RON naming the tier>`,
  three runs a configuration, medians of each run's per-pass p50s. Lantern's
  monitor view resolves with FXAA whatever `--stack` says, so the room's own
  FXAA is the **difference** of the two configurations' `fxaa` rows. On radv (RX
  7900 XTX) the slot went from FXAA's 0.023 ms to CMAA2's 0.093 ms and the frame
  from 1.469 to 1.571 ms; on lavapipe FXAA's 2.542 ms became CMAA2's 1.744 ms
  and the frame 102.904 to 101.344 ms. CMAA2's cost scales with edges, not
  pixels, which is why the software tier every golden runs on pays less for the
  better filter.
- **There is no single blessing adapter.** Each golden set is re-blessed where
  its own last bless was; _The CMAA2 default flip: what moved and where it was
  blessed_, below, records the sets for that flip. The earlier FXAA flip blessed
  `crates/crcbl/tests/golden/` and `apps/lantern/tests/golden/` on the software
  path and `apps/quarry/tests/golden/` on the discrete adapter, then verified
  every one on both.
- **Alpha-to-coverage exists only on an MSAA view**, so on the default view card
  grass and hair cards ship as cutouts with cooked coverage mips.
- **What is refused** — DLSS, FSR 2/3, any resolve after the UI pass, and a
  second morphological tier beside CMAA2 — is listed with the reasons in the
  backlog's declined entry.

## What the deleted 50-irradiance-probes plan left behind (2026-09-24)

Record; the raster half is built — the L1 grid in `crcbl_render::probe` and
`crcbl_shaders::probe`, read by `probe_irradiance` in `mesh.slang` and as the
SSR miss in `ssr.slang`; the per-probe visibility maps
(`crcbl_shaders::probe_visibility`, `crcbl_render::probe_capture`,
`probe_weight`); the raster updater (`crcbl_render::rsm`,
`crcbl_render::probe_gather`, `ProbeUpdate::EveryFrame`); and the clipmap's
levels and whole-step scroll (`ProbeVolume::level_origin`, `follow`, `exposed`,
`probe_capture::recapture`, `ForwardRenderer::follow_probe_volume`). What is not
is in `docs/backlog.md` under _What the deleted 50-irradiance-probes plan left
unbuilt_ (the traced updater, relocation, and what was declined), _What the RSM
probe updater shipped without_ (the sky through the visibility map) and _What
the probe clipmap's scrolling shipped without_ (the stall, the transients,
recapture on demand, lantern's finer level 0, the `Authored` refusal). The plan
was topic 18's probe section, split out verbatim on 2026-08-27; code that cites
"the probe plan's rung" means the visibility rung below.

**One volume, two updaters, and no leaking** — the user's decision of
2026-08-30, on "best-looking dynamic lighting for decent performance, and above
all no light leaking", taken after the no-bake rule of the same day. The grid
stays; what fills it changed:

- **No static bakes.** `apps/lantern`'s `bounce` and `apps/shard`'s
  `light::probes` once computed a lighting result at load from a sun and torches
  that then moved — a bake whatever thread ran it. Both went with the updater
  that replaced them; the modules stay, and place probes without lighting them.
  **Captured on load, then on scroll — never baked**: the visibility maps are
  captured when a scene loads and the slab a scroll exposes in the frame it
  appears; that is geometry, which is why the word is _captured_. The lighting
  rows are never stored across a load; the updater recomputes them every frame,
  which keeps the sun and every lamp dynamic.
- **No leaking: every reader weighs each of its eight probes by a Chebyshev test
  against that probe's visibility map, on every path** — the diffuse read and
  the specular fallback alike. The map is an octahedral depth and depth² per
  probe (Majercik et al. 2019's contribution, the one thing that stops a probe
  grid leaking), so a probe on the far side of a wall gets no weight.
  `crcbl_shaders::probe_visibility` owns the layout, the mapping and the bound
  and is the Rust mirror the render tests compare the shader against.
- **The raster updater, every frame, on all four backends**: the sun's near
  cascade and every shadowed punctual light's faces are drawn a second time as
  reflective shadow maps (`crcbl_render::rsm` says why they are two), and one
  compute pass gathers both into every probe, **each sample gated by that
  probe's visibility map**, so a texel the probe cannot see adds nothing. A
  fixed sample pattern, every probe every frame, no history: survey constraint
  C2 (a frame is a function of its own inputs) holds.
- **The traced updater fills the same rows** from inline ray queries on
  `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl` — unbuilt, designed in the backlog.
  The volume, the visibility test and the shader readers are the same on both
  tiers; only the pass that writes the rows differs. That is what "the diffuse
  GI twin" meant.
- **Single bounce on both tiers**, until C2's temporal question is answered yes;
  a second bounce is reading the previous frame's rows, which is history.

**One base, two sample producers — nothing else is bespoke.** The diagram is the
design, not the tree:

```text
  placement (clipmap, scroll, relocation)   ── shared
        │
  sample producer: per probe, N directions → (radiance, distance, backface)
        ├── raster tier: depth cube per probe (distance, backface) captured on
        │                load/scroll + the RSM gather (radiance), gated by it
        └── RT tier:     inline ray queries, every frame — one ray gives all
                         three, dynamic objects included
        │
  integrate ── shared: samples → L1 irradiance rows + octahedral depth/depth²
        │
  shading read ── shared: level pick, trilinear, SSR fallback
                  both paths: Chebyshev weight
```

The producer's contract is a sample buffer of a fixed layout; the integrate
pass, the storage, the relocation rule and the shader read never know which
producer ran. As built, the raster capture writes distance and distance² and no
backface channel, and relocation does not exist.

**The clipmap: layered density, camera-centred** (the user's addition,
2026-08-30). A few levels, each a fixed probe count centred on one point, level
`k` spaced `2^k` times level 0; a fragment reads the finest level that contains
it, blended over a band at each level's edge, then the trilinear gather and the
Chebyshev weight within it. **Rows are per level, with an offset per level, so
survey constraint C1 (no ninth storage buffer) holds.**

**Scrolling is by whole probe steps (2026-09-05).** `ProbeVolume::steps` is a
per-level whole-step offset, reduced into `0..count` on the host so the shader's
wrap in `probe_row` is one compare and one subtract. `ProbeVolume::follow`
re-centres every level by the nearest whole step of its own spacing and answers
the rows the move invalidated; `ProbeVolume::exposed` is the rule that a step of
`k` probes along one axis exposes exactly `k` slabs and leaves the other
`count − k` at the rows they had (the union, on several axes).
`probe_capture::recapture` draws only the exposed rows and the gather's position
table is rewritten beside it. lantern follows `bounce::follow_point` every frame
and its volume never moves (less than one whole step of slack), which is what
kept every lantern golden byte-identical.

**The grid itself (designed 2026-08-14):**

- **L1, three dot products.** Four coefficients per channel, packed so
  irradiance for a normal is three dot products against `float4(N, 1)`: no
  `pow`, no trigonometry, one `max(…, 0)` for ringing.
- **Additive, which is what makes it safe to land empty.** The probe term is
  added to `frame.ambient` and the sky; a scene with no probes uploads zeroes
  and `x + 0 == x` exactly on every target, so the frame is bit-identical with
  no branch. An author who wants probes to be the whole ambient zeroes
  `DirectionalLight::ambient`. AO still scales the diffuse environment alone.
- **The specular fallback is two terms, not a `lerp`**:
  `hit_color * fresnel * confidence` plus
  `probe_radiance(…) * fresnel * (1 - confidence)`, so with a zero volume the
  fallback is exactly zero and existing SSR hits keep their multiplication
  order, bit for bit. The rows hold irradiance with the clamped-cosine transfer
  folded in, so `probe_radiance` divides the constant band by `π` and the linear
  band by `2π/3` first; dotting the stored rows would brighten a constant
  environment by `π`. Above `ROUGHNESS_CUTOFF` the probe term is returned at
  zero sharpness and the blur composites it unfiltered. **The honest limit**: an
  L1 probe in a mirror is a gradient, not a room — "not black", which is what a
  metal needed, not "a mirror".
- **No new render pass, no new `Features` flag, no selector** — a read-only
  storage buffer in a fragment stage, which the seam permits. **The probe
  binding is appended after `AMBIENT_OCCLUSION_BINDING`, never inserted**:
  `crcbl-mtl` numbers Metal arguments by counting layout entries while Slang
  numbers by declaration order, and they agree only while both ascend. The index
  is past everything `mesh_cluster.slang` declares, so that file needs no
  mirror.
- **Both sides of both flips meet**, so probe goldens go under
  `Tolerance::RASTERISER` like every 3D golden. The cell index: the far corner's
  trilinear weight is exactly zero where the index changes. `probe_weight`'s
  `to_surface <= moments.x`: at the flip the Chebyshev bound is
  `variance / variance`, one, which is the other branch's answer. There is no
  tap whose comparison is the answer, which is SSR's exposure.

**How the probe claims are tested, and each was shown red:**

- A Rust mirror of the SH evaluation checked against the literature: a constant
  radiance `L` integrates to irradiance `π·L`, and the L1 band's transfer is
  `2π/3`.
- `Scene::Probes`: ambient zero, the sun down, so every pixel is the probe term;
  two probes of opposite-coloured L1, observed as a ratio between two blocks of
  one frame, which fails for a flat ambient (ratio 1) and a zero volume (black).
- `a_probe_behind_a_wall_lights_nothing_through_it` and
  `a_probe_behind_a_wall_reflects_nothing_through_it` in
  `crates/crcbl/tests/render_e2e.rs`: one fixture drawn with and without a wall;
  the walled band must drop by `LEAK_RATIO` **and** the other band gain
  `LEAK_MIN_GAIN`, so a run that simply darkens the room fails too. Red by
  forcing the Chebyshev weight to `1.0` (in `ssr.slang` alone for the specular
  one, which leaves the diffuse test green). The specular fixture subtracts a
  draw without the reflection pair so only the SSR pass's output is compared.

**What the capture costs, measured**
(`apps/lantern --headless --frames 400 --size 1920x1080`, radv on an RX 7900
XTX, median of three): **0.93 ms for 60 probes against 12 occluders** at load,
16 µs a probe against the 0.28 ms the old host ray cast took. The weighting does
not resolve on the diffuse path (`forward` 0.293 ms p50 with
`r_probe_visibility` on, 0.302 off); on the specular path it does, and that
price is in `docs/backlog.md` under _The SSR visibility weight costs the
software tier 11% of a frame_. A frame that follows without stepping is free
(1.476 against 1.475 ms on radv); a step's cost and its stall are in the
scrolling entry. A clipmap of a few thousand probes is a load of tens of
milliseconds, a load path rather than a redesign; each figure is measured on the
three tiers before a rung counts.

**What this amended.** The 2026-08-30 GI decision said the tier below ray
tracing has no bounce term; it has this one, because it is leak-free and costs
one compute pass and one extra render pass of three small targets. The DDGI
rejection stands on its temporal half and falls on its ray-tracing half (the
traced updater); the light-field-probe rejection ("no leaking defect yet") is
withdrawn — leaking is the defect the decision is about.

## What the deleted 51-volumetrics plan left behind (2026-09-24)

Record; rungs 1a to 2 are built — `crcbl_render::volumetric`'s
`volumetric-scatter`, `volumetric-integrate` and `volumetric-composite` passes
(`volumetric.slang`, `volumetric_composite.slang`), switched by
`RenderEffects::VOLUMETRIC_FOG`. What is left is in `docs/backlog.md` under _The
froxel column casts its shaft_, _The two-media rule has no frame-level test_ and
_Froxel rungs 3 and 4: a filtered 3D target and a density field_.

Code cites the plan's **rungs** by number, so the numbering is kept:

- **Rung 1a — the column.** The froxel buffer, the scatter, the prefix scan and
  the composite, proved against the closed form.
- **Rung 1b-i — the sun in the medium.** The Henyey-Greenstein phase copied into
  both shaders, and the sun's direction in the params block.
- **Rung 1b-ii — the shaft.** The cascade lookup copied once into `scatterMain`,
  a visibility buffer, and the drift guard over the copy.
- **Rung 2 — punctual lights.** The froxel's cluster list walked at the slice
  midpoint with `mesh.slang`'s falloff and cone, occluded by the light's own
  shadow tiles.
- **Rung 3 (unbuilt) — a 3D target, a coarser grid and a depth-aware lookup.**
- **Rung 4 (unbuilt) — a density field rather than a constant medium.**

The rules:

- **The scattering target is a storage buffer on the existing froxel grid, not a
  3D texture.** `crcbl_render::transient` has no volume — `TransientImageDesc`
  has no depth field and the pool hard-codes `ImageType::D2` — so a 3D target is
  the engine's first 3D image on four backends at once, the shape of gap that
  let a read-only depth attachment pass three cross-target clippy runs and reach
  `crcbl-dx12` as a refusal. And the grid already exists as a buffer:
  `crcbl_render::light_grid`'s froxels, addressed by `froxel_of`, filled by
  `light_cluster.slang`. Four floats per froxel, in-scattered radiance in `xyz`
  and extinction in `w`. **The price is named, not hidden**: the composite reads
  the nearest froxel, so a slow pan across a shaft steps rather than slides.
  Rung 3 is where that is bought back.
- **The composite is its own fullscreen pass, not a term in `mesh.slang`.**
  `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE` (the WebGPU guarantee) is a
  sum over a whole pipeline layout, and the mesh layout's fragment-visible
  storage buffers are `VERTEX`-visible on the non-mesh-shader path, where there
  is no headroom. The plan also wanted the pass after the reflection resolve so
  reflections are fogged; **as built it runs before the march**, where the
  analytic fog runs, so the two paths stay comparable (`ForwardRenderer`'s
  `add_passes` in `crates/crcbl-render/src/forward/view.rs`). Moving both after
  `ssr_blur.slang` is in the backlog.
- **The two media are one medium, and only one charges the transmittance.**
  Height fog's optical depth and the column's transmittance are the same air
  integrated twice; compositing both darkens the frame by the square of what the
  medium does, which reads as "the fog got thicker when volumetrics were
  enabled". When the column is present it owns the medium: `ForwardRenderer`
  zeroes the frame block's fog density on a frame with `VOLUMETRIC_FOG`, and the
  column's extinction is seeded from the same `Fog` rows, so switching paths
  changes the sampling, never the medium.
- **Slice thickness is computed, never assumed.** The split is exponential, so
  slices differ by four orders of magnitude and `integrate_slice` takes the
  thickness as an argument — a constant makes the near field vanish and the far
  field glow. **The thickness is along the view ray, not along `z`** (the secant
  factor; dropping it brightens the frame toward its edges at a wide field of
  view). **The last slice is bounded**: `light_cluster.slang` leaves its far
  side at `FLT_MAX` so distant lights are listed, and an unbounded length is an
  infinite optical depth, so `scatterMain` ends it at `CLUSTER_FAR` and
  `crcbl_shaders::fog::MAX_OPTICAL_DEPTH` is the ceiling either way. Slice zero
  starts at the eye, not at `CLUSTER_NEAR`, or the column has a gap the closed
  form does not.
- **Every copied light or cascade walk is guarded letter for letter.** There is
  no `#include`, so `mesh.slang`'s lighting exists twice, and the rule for a
  second copy is a drift guard that splits on the function's own signature and
  fails on the host with no GPU: `crcbl_shaders::volumetric`'s
  `both_shaders_spell_the_same_atlas_walk` and
  `both_shaders_spell_the_same_punctual_light`, on `crcbl_shaders::sky`'s
  `the_shader_spells_the_same_gradient` pattern. `grass.slang` and `water.slang`
  copy under the same arrangement. **The copy drops `shadow_slope`, both biases
  and the `n_dot_l` early return**: they are about a receiving facet, and
  biasing a froxel pushes its scattering out of the shadow it stands in — a lit
  rim along every shaft. It keeps the `w` test and the far-plane test a
  perspective map needs.
- **The froxel's lighting leaves the scatter pass in a buffer of its own.**
  `crcbl_shaders::volumetric::LIGHTING_STRIDE`: punctual glow in `rgb`, the
  sun's visibility in `w`, a row the scan does not touch. The composite's
  partial slice needs the same source `scatterMain` used; re-walking the atlas
  per pixel is a second opaque shadow pass and a second copy, and recovering the
  source from the column divides by `1 - T`, which is zero exactly where a thin
  slice makes the answer meaningless.
- **Temporal reprojection is refused (2026-08-30).** It is the industry's route
  to a filtered, coarser froxel volume, and a history buffer makes a frame a
  function of how many frames preceded it, which every golden in the tree is
  built not to be. Rung 3 buys the filtering back with a 3D target, a
  depth-aware upsample and a sample count along the slice as the quality tier —
  never with history or a per-frame jitter.
- **What the host already pins.** `crcbl_shaders::volumetric`'s tests hold the
  phase function to one over the sphere at every anisotropy, mirror its lobe
  with the sign of `g`, and demand a homogeneous column cut into 1, 2, 7, 64 and
  512 slices composite to the same radiance; the naive `source * thickness` a
  froxel pass reaches for first fails exactly one of them. The frame-level
  checks are in `crates/crcbl/tests/mesh_e2e/hdr.rs` and
  `crates/crcbl/tests/mesh_e2e/froxels.rs`.

## What the alpha-mask and double-sided material modes shipped without (2026-09-05)

Decision record; the decision is in `docs/backlog.md`.

- **The bucket table is twinned per scene, not per mesh, and the empty twins
  cost measurable time on the raster tier.** `ForwardRenderer::build` walks
  `SceneDesc::materials` for the set of modes present and emits every resident
  mesh's levels once per mode in it, because which mesh will be drawn by which
  material is not knowable there: instances arrive later through `add_instance`,
  and the description pairs meshes with materials nowhere. So a scene with one
  cutout in it doubles its bucket table, and every pass records one indirect
  call per bucket whatever is in it — topic 03 §3.3's own invariant.

  **Measured, not feared.** §3 of the plan carries the run: lantern with an
  unconditional empty twin per mesh reads `shadow` 0.222 ms on radv against
  0.138 ms with one bucket per mesh, which is the same number the genuinely
  masked build reads — so the cost is the twelve extra dispatches per shadow
  view rather than the `discard`. On lavapipe the split still wins by a wide
  margin, and on radv lantern's shape (twelve tiny meshes, two shadow views)
  loses.

  **A twin per mode the scene's materials carry, not per mode that exists.**
  `DEPTH_MODES` grew to four entries when `doubleSided` landed on 2026-09-05 and
  the table did not grow with it: `ForwardRenderer::build` filters that list by
  the mode _values_ its own `SceneDesc::materials` hold, so two materials of two
  modes are two twins whichever two they are, and only a description holding all
  four modes pays for four.
  `an_all_opaque_scene_keeps_one_bucket_per_mesh_level` states both halves — a
  scene whose three rows carry two mode values between them, setting both mode
  bits, still gets two.

  A finer rule needs something the description does not currently say: which
  `(mesh, material)` pairs a scene will actually instance. The two shapes worth
  weighing are a declared pair list on `SceneDesc`, checked against
  `add_instance` the way `check_scene` checks page layers; and rebuilding the
  bucket table when an `add_instance` introduces a pair no bucket covers, which
  is a device allocation inside a frame-adjacent call and is why it was not
  taken here. A third shape keeps that allocation out of `add_instance`: rebuild
  the table in `begin_frame` when the set of live `(mesh, mode)` pairs has
  changed since the last build — the pool's host mirror knows the set — so the
  stream still depends only on which pairs exist and never on how many instances
  there are, which is the property topic 03 opens with and
  `the_frame_records_one_indirect_call_per_bucket_whatever_the_scene_holds`
  pins. Skipping empty buckets at record time was considered and declined for
  that property's sake: it would make the recorded stream a function of the
  instance count. **Which of the three, if any, is the user's call**; none is
  worth doing before a scene in this tree actually has foliage in it — every
  demo is still all opaque, so every one of them emits exactly the table it
  emitted before modes existed.

## What the atmosphere shipped without (2026-09-05)

Decision record; the decision is in `docs/backlog.md`.

- **No demo has been switched to it, and that re-bless is owed.** Every shipped
  demo still draws no sky at all — nothing in `apps/` calls `set_sky` or
  `set_atmosphere` — so the whole rung is exercised by
  `crcbl::screenshot::atmosphere_forward` and `Scene::AtmosphereMirror`, through
  `render_e2e`'s `an_atmosphere_frame_is_the_host_lut` and
  `an_atmosphere_mirror_reflects_the_luts_limb`, and by nothing else. Giving
  `lantern`, `sundial` or `alcove` an atmosphere moves that demo's golden
  images, which is its own slice: pick the demo, set the sun to the one its
  `DirectionalLight` already uses, re-bless on both local adapters, and check
  the browser gate, which has its own copies.

- **No aerial perspective.** The paper's third LUT — the froxel volume that puts
  the air in _front_ of a surface — is not built. `crcbl_render::volumetric`'s
  column is the tree's froxel pass and does not read the atmosphere's medium.
  Doing it means the medium's extinction and in-scattering per froxel, which is
  the same march this module already has, and a decision about whether the two
  passes merge or compose.

- **The ground below the horizon is black, deliberately.** A view ray that meets
  the planet returns only the air in front of it, so the atmosphere's own lower
  hemisphere adds nothing to `SkyView::irradiance`. What bounces off a scene's
  floor is the irradiance-probe volume's, and an idealised sphere's albedo here
  would count it twice. `GROUND_ALBEDO` is still used by the multiple-scattering
  cook, where it belongs. Revisit only if a scene wants a sky with no floor
  under it.

- **A rough lobe still reflects the atmosphere as three bands.** The mirror half
  shipped: `ssr.slang`'s `sky_environment` reads the sky-view LUT along the
  reflected direction and weights it by `sharpness_of`'s ramp, so a surface at
  `ROUGHNESS_CUTOFF` and above takes `sky_prefiltered`'s three bands exactly as
  it did. **What that leaves** is the azimuth of a wide lobe: a rough conductor
  beside the sun gathers the horizon's azimuthal mean rather than a cone of the
  aureole. The upgrade is a convolution of the LUT itself — a second cooked
  table, or a mip chain over `SkyView::rows` — and it is not a committed cook
  like `sky_prefilter.bin`, because the field it convolves changes whenever the
  sun moves. Declined here on those terms and because the error shrinks as the
  lobe widens, which is the direction that matters: the widest lobes are the
  ones the azimuthal mean is closest to.

- **The demo cube's faces carry vertex colours, and `mesh.slang` multiplies them
  into the albedo.** `crcbl_shaders::mesh::FACES` gives the `+Y` face
  `[0.25, 0.80, 0.30]`, and `albedo = input.color * material.base_color * texel`
  — so a floor placed as the demo cube through a white conductor row has a green
  `F0` and reflects a green sky. Not a bug and not new; it cost an afternoon to
  find because the tint is plausible. `Scene::AtmosphereMirror` authors its own
  plate (`atmosphere_mirror_mesh`) for that reason, and `Scene::Ssr`'s
  `SSR_CHANNEL` doc is the one place the tree already said so.

- **The presented sky lags a moving sun by up to one whole build.** The striped
  march shipped: `SkyViewBuild` in `crcbl_shaders::atmosphere` and
  `ForwardRenderer::refresh_sky_view` stepping it `SKY_VIEW_BUILD_ROWS` rows per
  frame. What it leaves is the trade that choice makes — a sun that moves again
  mid-march restarts the build, so a sun that moves _every_ frame never
  completes one and the frame goes on drawing the LUT of the last sun the march
  caught up with. Deliberate, and written down in `refresh_sky_view`'s doc: the
  alternative is a sky that stutters at whatever the march last passed. If a
  scene ever wants the sky to track a continuous sweep rather than lag it, the
  two options left are the ones this rung declined — a coarser LUT while the sun
  is moving, or the march on a compute pass, which gives up "no transcendental
  reaches a colour". Neither is needed until an app sweeps a sun.

- **No measured band's normal has more than one non-zero component.** Every band
  either fixture reads faces an axis — `+Y` on the floor, `+Z` on the wall — so
  what is pinned is each lane on its own rather than a normal that mixes them.
  L1 is linear in the normal, so an oblique band is a combination of directions
  already read and there is no new arithmetic in it; recorded because "the row
  is evaluated at an arbitrary normal" is a stronger claim than anything here
  makes, not because a fix is planned.

- **`sky_ambient_forward` and `sky_ambient_wall_forward` are builders and not
  `Scene`s.** They are reached only from `render_e2e`, so they have no golden,
  are absent from `crcbl-cli`'s scene list and never reach the render-harness
  browser gate. Deliberate on two counts: the picture is a flat lit floor or a
  flat lit wall, which a golden could not tell from any other flat lit surface,
  and each fixture needs the same room drawn twice — with an atmosphere and
  without — which the `Scene` enum has no way to ask for. If the browser leg or
  the Metal and D3D12 legs ever need a sky-lit _surface_ rather than a sky-lit
  background, these become variants and gain goldens.

- **The LUT parameterisation is this tree's, not the paper's.** Hillaire indexes
  the sky-view LUT by angles; this one indexes it by the direction's `y` through
  `sign(s)·s²` and by the cosine of the azimuth away from the sun through
  `1 − 2u²`, because both of those and both inverses are algebraic and the
  paper's are not. The consequence to watch is resolution on the anti-solar
  side, where `sky_view_cosine_of` coarsens: the field is smooth there, and no
  artefact has been seen, but nothing measures it. `SKY_VIEW_WIDTH` is where to
  spend if one appears.

- **`SkyView::irradiance` is a quadrature where `SkyGradient::irradiance` is
  closed form**, and it is checked against a brute-force integral rather than
  against an analytic answer, because the field it integrates is a march's
  output and has no analytic answer. What that leaves is the shared error: both
  the projection and its oracle read the same LUT, so an error _in the LUT_
  cancels between them. `the_vertical_transmittance_matches_its_closed_form` is
  the one test in the module that reaches outside it, and it covers the
  transmittance integrator alone — the multiple-scattering cook and the sky-view
  march are checked structurally and against themselves.

## The shadow filter selector leaves three things owed (2026-09-04)

Decision record; the decision is in `docs/backlog.md`.

- **Decision needed: which rung each tier's shadow filter is.** The three are
  priced now — measured 2026-09-04 and recorded with topic 45's fifteenth
  decision (_What the deleted 45-shadows plan left behind_), off five
  `apps/sundial` runs per filter per adapter at the goldens' own pose with the
  seam off. Median `forward` p50: 0.228 / 0.199 / 0.180 ms for `pcss` / `disc` /
  `box` on an RX 7900 XTX (radv, Mesa 26.2.2) at 1920x1080, and 8.586 / 7.349 /
  6.914 ms on that machine's llvmpipe (LLVM 22.1.8) at 960x720. The ladder's own
  order on both adapters, no two ranges overlapping, and the ladder end to end
  is 0.068 ms of a 0.649 ms radv frame; the `shadow` row is flat across all
  three, as the selector claims. What was left was the assignment, and it was
  the user's: `r_shadow_filter` had no tier row, so every tier ran the shipped
  `pcss`. It got one on 2026-09-09 — `low` writes `box`, `medium` `disc` and
  `high` `pcss` (`crcbl::settings::presets`, and the tier table in
  `docs/backlog.md` under _The tier table the quality presets are built from_).
  Same shape of question as the SSR visibility weight and the AO knobs below.
- **Considered and declined: drawing the scene twice to compare.** The occlusion
  chain's seam records its gather twice under a scissor, and that shape is
  available to a full-screen pass because each recording pays for half a target
  of fragments. The forward pass is a _scene_ draw: a second recording is every
  triangle, every cull and every vertex fetch again, so a comparison would cost
  two frames rather than one. The per-side `PassTimers` row it would have bought
  measures the scene rather than the filter and would be dominated by whatever
  each half contains, so it buys nothing the per-filter runs above do not.
  `crcbl_render::split`'s header carries both shapes and which client takes
  which.

## What sundial still owes (2026-09-04)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading. The fixture's own rules and the readings its deleted plan recorded for
the bias pair, the cross-fade, the penumbra and the seam are in
`docs/notes/samples.md` under _What the deleted sample plans 05, 14, 18, 19 and
20 left behind_.

- **Subdivision is the coverage ladder's doing, not the allocator running out.**
  Recorded because the entry this replaces said the opposite: a scene with more
  shadowed lights than the atlas has root cells subdivides nothing.
  `Selection::lay_out` spends the coarsest requests first and its allocation
  cannot run out — which is why the failure there is `unreachable!` rather than
  a fallback. What hands out a halving is `shadow::tile_level` on a light's
  `coverage`, so the way to reach a subdivided cell from a test is one light
  whose map covers little of the frame, not a crowd of them.
- **A frame at a render scale below one draws the readout through the upscale.**
  The pass writes the internal target, which the upscale then filters to the
  caller's extent — so at `set_render_scale(0.5)` the atlas is a Catmull-Rom
  reconstruction of itself and its one-pixel borders are soft. Considered and
  left: drawing after the upscale would want a second pipeline at the caller's
  format for a case a reviewer can avoid by putting the scale back.
  - **What surprised us, and is not a bug (2026-09-05).** The pavement claims
    cannot be read from a pose high enough to look down on the whole plaza, and
    two independent reasons stop it. A raised eye puts `plaza::PLINTH_CONTACT`
    **past the cascade split** — 6.69 m from an eye at `(0, 3.5, 8.5)` against a
    split at 6.100 — and `r_shadow_bias` and `r_shadow_normal_offset` are counts
    of texels _of the cascade the fragment landed in_, so `PETER_PAN_BIAS`'s 96
    texels stop being the shipped station and wipe the shadow outright (contact
    `0.00`, beyond `19.88`, and `HELD_OFFSET` takes the contact from `69.11` to
    `0.00`). And `speckle_percent` is a **screen-space** statistic: from an eye
    at 2.5 m the constant bias's rise over the acne block is `1.4857%`, under
    the `1.5%` floor, where at 2.0 m it is `3.3629%` and at the fixture pose
    `3.2575%`. Both are why `plaza::PAVEMENT_EYE` is a modest 2.0 m rather than
    an overlook.

- **What surprised us, and is not a bug (2026-09-05).** Peter-panning at the
  plinth's contact needs a very large bias — 88 texels of `r_shadow_bias`,
  against the 1.5 that ships — and the plinth's own thickness is why. The depth
  pass keeps front faces, so what the shadow map stores along the ray from
  `PLINTH_CONTACT` to the sun is the plinth's _far_ face, and a bias towards the
  light has to cross the whole 1.2 m depth of the block before the contact
  compares as lit. A thin caster loses its contact at a small count, which is
  what `apps/lantern`'s 0.15 m shell showed topic 45's seventh decision (_What
  the deleted 45-shadows plan left behind_). The consequence for this fixture is
  that its peter-panning reading is a claim about a _thick_ caster, and a thin
  one in the plaza would make the same claim at a count near the shipped value —
  which is the version worth building if the pair is ever wanted as a regression
  guard rather than as a comparison.

**What surprised us, and is not a bug.** The first cascade's extent is a
function of the camera's **near plane** — `Cascades::splits` blends a
logarithmic division of `near .. DISTANCE`, and the logarithmic half is
`near * (DISTANCE / near).powf(ratio)` — so the two-centimetre near plane every
other sample opens with puts the first split about four metres from the eye and
makes the whole penumbra ladder unmeasurable: at cascade 1's texel, every
separation in a scene this size clamps to the same lower bound and `pcss` draws
exactly what `disc` draws. `apps/sundial/src/plaza.rs`'s `NEAR` is half a metre
for that reason and says so, and the split is read back out of `Cascades` by
`the_colonnade_straddles_the_cascade_split` rather than assumed.

## What the AO default change of 2026-09-03 did not cover (2026-09-03)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**The sweep the test's thresholds are chosen off**, read from
`forward_e2e::occlusion::the_tangential_occlusion_line_does_not_step`, sharp
edges over `DIAGONAL_LENGTH` samples, identical in all seven runs of each row:

```text
slices  blurs     radv   lavapipe
     2      1       37         38   the clamp floor
     4      1      100         89
     2      2        8          9
     4      2        0          2   what ships
anti-baseline: one plane orientation across the whole tile
     2      1      118        113
```

Kept because `MAX_SPARSE_SHARP_EDGES` and `MAX_SHIPPED_SHARP_EDGES` are picked
off it, and because it is re-taken whenever the pass changes rather than carried
forward. It was carried once — across the half-resolution change — read
`1 / 1 / 2 / 0` on radv for weeks, and produced a conclusion that was backwards
in both this file and the ambient-occlusion plan of the time.

## The SSR visibility weight costs the software tier 11% of a frame (2026-09-04)

Decision record; the decision is in `docs/backlog.md`.

`b83ae75` gave `ssr.slang`'s probe fallback the Chebyshev visibility weight
`mesh.slang` already had, closing a specular leak through walls. It was landed
unpriced; the two native tiers have now been measured and the answer is not
free.

`apps/lantern --headless --frames 400 --size 1920x1080`, p50 of the `ssr` row of
the app's own pass table, three runs each, before and after. "Before" is a
worktree at `55d0eba` — the commit `b83ae75` sits on — so the two binaries
differ by this change and nothing else:

```text
                    ssr p50          the whole frame's p50
              before    after        before    after
radv           0.115    0.169         1.141    1.109
lavapipe       7.072   15.421        72.042   80.152
```

- **On radv it is invisible at frame level.** The pass grows 47%, 54 us, and the
  frame total moves by less than the spread between runs — the two totals above
  bracket each other, which is the honest reading of a 3% instrument.
- **On lavapipe it more than doubles: +118%, +8.35 ms**, and the frame follows
  it almost exactly, +8.11 ms or **+11.3%**. The `ssr` pass goes from 9.9% of
  the frame to 19.1%. A software rasteriser pays for the sixteen extra `Load`s
  per pixel in a way a discrete GPU does not.

**The browser tier was measured on hardware and behaves like radv, not like
lavapipe.** quarry through Chrome on an RDNA-3 adapter at the gate's 959x463,
`web/run-browser-e2e.sh` on the `hardware` adapter, three runs each side: `ssr`
**0.036 ms p50 before against 0.053 after**, identical to the printed digit in
all three runs of each, and the frame **0.720 to 0.737 ms** — +47% on the pass,
exactly the +0.017 ms the pass grew, and **+2.4% of the frame**. So both GPU
tiers pay 47% and only the software rasteriser pays 118%.

**The candidate that would pay for it, not yet costed.** `ssr.slang`'s
`probe_environment` is evaluated for every non-far pixel including fully rough
ones, exactly as it was before the weight landed — so the weight's sixteen loads
apply to the whole screen rather than to the pixels whose ray actually missed
and actually reflects. Restricting the fallback to those pixels is a change to
the pass's shape rather than to the weight, it was already true before this
commit, and it is where the software tier's eight milliseconds are. Whether that
is worth building, or whether the leak fix should carry a console variable so a
tier can decline it, is the user's call — the same shape of decision as the AO
knobs, and the same missing route: neither `r_probe_visibility` nor anything
else has an `[engine.video]` key or a tier row.

## What the RSM probe updater shipped without (2026-09-04)

Decision record; the decision is in `docs/backlog.md`.

**What it costs.** p50 of three,
`lantern --headless --frames 400 --size 1920x1080`, the sun-only updater on
against off:

```text
                radv on   radv off   lavapipe on   lavapipe off
rsm               0.060          —         1.136              —
probe-gather      0.047          —         0.052              —
frame total       1.281      1.161        81.964         80.092
```

**+0.120 ms on radv, which is 10% of that frame**, and +1.87 ms on lavapipe,
which is 2.3% of that one. The discrete tier pays the larger share because its
frame is small; the software tier's `rsm` is nineteen times its radv cost and
still disappears into an 80 ms frame. `docs/backlog.md`'s survey constraint C3
budgets the whole frame at 0.990 ms on this adapter and it was already at 1.161
before the updater, so this is a rung spent over budget rather than into it. The
punctual half's own price is in `crcbl_shaders::probe_gather`'s
`PUNCTUAL_RSM_SIDE`, which carries its extent sweep.

**A finding worth keeping, because it cost most of the debugging time.** A probe
standing above a floor gathers nearly all its flux from below, so its L1 lobe
cancels at the floor's own `+Y` normal and a floor pixel shows the bounce barely
at all. The first fixture written for this measured a floor and read exactly
zero difference between the walled and open arms — a perfect false negative.
**Any future updater fixture must measure a surface facing the way the flux
travels**, which is why the one that shipped measures a wall face the sun never
touches.

**A producer is only as good as the light it is given.** `apps/shard`'s doused
zone was measured on 2026-09-04 with `r_probe_bounce` off and on and moved by
0.01 of a luma level, because the one light left burning there is faint and
stands in a corner — see "shard's doused zone was never lifted" in
`docs/notes/samples.md`. The gather is not at fault and no engine change
addresses it; it is what a scene with nothing to bounce looks like.

## Two price fixtures print a `forward` that bundles its fused clears (2026-09-05)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

`forward`'s full-extent attachment clears — scene colour, reflectivity and
motion — are `LoadOp::Clear`s fused into the pass's begin, so giving them their
own timestamp means giving them their own pass and a second full-target write —
**not worth doing, and the reason is the measurement's, not the renderer's.**
What attributes them honestly is a zero-geometry configuration timed beside the
loaded one, and `mesh_e2e/depth_only.rs`'s `the_price_of_the_depth_only_passes`
now has one: `PRICED_FIELDS`' second row draws an empty list at the same extent
through the same effect stack, interleaved a frame at a time with the field, and
the helper holds the two rows apart by their `FrameCounters` instance counts
rather than by a duration. Measured 2026-09-05 at 640x480 over 48 recorded
frames, the floor is a `forward` p50 of 0.009 ms on an RX 7900 XTX and 0.258 ms
on lavapipe — medians of three runs each, spread 0.009–0.010 and 0.256–0.268 —
and it is written into _What the deleted 43-render-standards plan left behind_
above.

- **The SSR row's `forward` shares are split now (_What the deleted
  47-reflections plan left behind_), and no lantern-side floor row was added,
  because one would measure nothing new.** There is no lantern price fixture to
  add a row to: those figures come from headless runs of the `lantern`
  **binary** (`c0917d6`, `b87a1ab`) reading the engine's `PassStats` report, and
  the only fixtures in the tree that read `PassStats` are `mesh_e2e`'s four. A
  lantern-side row would take the same number as `depth_only.rs`'s:
  `PassBuilder::clear_color` is `LoadOp::Clear` plus `StoreOp::Store`
  unconditionally, `ForwardRenderer::add_passes` creates `reflectivity` and
  `motion` whatever the effect stack is doing, and `RenderGraph::execute` emits
  a pass's barriers outside its timestamp bracket — so an empty-draw-list
  `forward` is the same quantity at a given extent whatever else the frame
  draws, and `price_frame`'s `CRCBL_PRICE_SIZE` already takes it at any extent.
  What lantern adds is only that its `forward` line is two passes summed, the
  room's and the monitor's, which `pass_stats.rs` documents and the report's
  occurrence column shows. Measured 2026-09-05 with
  `CRCBL_GPU=vk CRCBL_PRICE_SIZE=<extent> crates/crcbl/tests/run-mesh-e2e.sh the_price_of_the_depth_only_passes`,
  medians of three runs: 0.011 ms at 960×720 plus 0.005 ms at
  `room::MONITOR_EXTENT` on an RX 7900 XTX, 0.463 plus 0.098 on lavapipe — about
  a twenty-fifth of the `forward` row on both drivers.

## The multi-bounce tint narrowed AO's contrast, and two claims lost margin (2026-09-01)

**DECIDED 2026-09-06 —** the narrowed margin is accepted as measured, and this
entry is a record rather than an open question. `r_ssao_intensity` is the
industry control (Unreal's `r.AmbientOcclusion.Intensity`), and it defaults to
identity. The guards' contract is that the pass reaches shading, both go red
under sabotage, and the margin is what a correct curve leaves. Nothing is
scheduled by it.

`mesh.slang`'s `multi_bounce_occlusion` shipped 2026-09-01 and does what it is
for: it lifts an occluded fragment by the colour of what occludes it. On a
bright surface that lift is large, and it comes straight out of the occlusion
contrast a scene shows.

Measured on radv, before and after:

| claim                                      | scene                                    | was                     | is      |
| ------------------------------------------ | ---------------------------------------- | ----------------------- | ------- |
| `AO_RATIO` (`crcbl/tests/render_e2e.rs`)   | wall bands against open floor            | `1.198`                 | `1.058` |
| `AO_LIFT` (`apps/lantern/tests/golden.rs`) | contact corner, occlusion on against off | comfortably over `1.08` | `1.038` |

Both figures match the published curve at those scenes' albedos, so this is the
fit working rather than the occlusion weakening — checked against the polynomial
directly before either constant was touched. Both constants were re-measured and
both still land at exactly `1.00` for a pass that never reached the shading
line, which is the failure they exist to catch; that was verified by sabotage,
feeding `multi_bounce_occlusion` a visibility of one so the occlusion never
reaches the ambient term, and both went red.

**What is left open is the margin.** `AO_RATIO` guards a separation of 5.8%
where it used to guard 20%, and `AO_LIFT` 3.8% where it used to guard more than
8%. Both are still far above the rasteriser drift
`crcbl_golden::Tolerance::RASTERISER` was measured for, so neither is fragile
today, but the headroom for a future change to eat is a third of what it was.

**Re-checked after half-resolution AO landed, 2026-09-02, and neither margin
moved.** The halving was the obvious candidate for eating the headroom this
entry is about, so both were read again on radv: the AO scene measures its walls
at 70.8 and 70.5 against an open floor of 74.7, a ratio of 1.055, and lantern's
contact corner goes 57.5 to 59.7 with occlusion on, a lift of 1.038. Both sit
where they sat before the gather was halved, so the reconstruction is carrying
the contrast the full-resolution pass had. The margin is unchanged, and so is
the case for an intensity control below.

**Re-checked after the AO defaults moved, 2026-09-04, and both margins gave a
little back.** On radv the AO scene now measures its walls at 71.2 and 70.6
against an open floor of 74.7 — a ratio of **1.049** where the halving left it
at 1.055 — and lantern's contact corner goes 54.3 to 56.2 with occlusion off, a
lift of **1.035** against 1.038. So four horizon planes and a second blur cost
about half a point of separation on each, against thresholds of `AO_RATIO` 1.03
and `AO_LIFT` 1.02. Both still clear, and both clear by less: the AO scene now
guards 4.9% where this entry recorded 5.8%, and the corner 3.5% where it
recorded 3.8%.

**Re-read against the tree 2026-09-05, and the lift's figures above are a day
old.** `AO_LIFT` is **1.008**, not 1.02, and the corner reads 66.7 against 67.6
on radv (1.0135) and 66.8 against 67.6 on lavapipe (1.0120): the punctual RSM
producer narrowed it again on 2026-09-04 (`5874479`), and the constant's own doc
in `apps/lantern/tests/golden.rs` says it has run out of room to narrow a third
time. `AO_RATIO` and its 1.03 are as written.

The lift is measured on a frame whose bounce is the RSM updater's rather than
lantern's old CPU bake, since both landed before this reading. The ratio is not:
`Scene::Ao` is `ProbeUpdate::Authored`, so nothing about the updater reaches it
and its half-point is the AO defaults alone.

**The industry answer is an AO intensity control, and it was built on
2026-09-02** — `r_ssao_intensity`, a console variable in
`crates/crcbl-render/src/ssao.rs` that `ssao_upsample.slang` raises its
reconstructed visibility to, applied to the scalar occlusion before `mesh.slang`
tints it. It is a power rather than a blend towards one, because a blend can
only lift the answer back towards unoccluded and this knob exists to ask for
more occlusion than the horizons found. Range `0.25 ..= 4.0`, argued from the
curve's slope at an unoccluded surface.

**What that does _not_ do is restore the margin, and this entry stays open for
that reason.** The default is 1.0 and is exactly identity, so every figure above
still describes the frame that ships: `AO_RATIO` still guards 5.8% where it once
guarded 20%, and `AO_LIFT` 3.8% against more than 8%. The knob makes the
contrast _recoverable_ — it does not recover it. **Whether the shipped default
moves off 1.0 is the user's call**, and it is the same shape of decision the
`r_ssao_slices` and `r_ssao_blur_passes` defaults were until they moved on
2026-09-03: three occlusion console variables now exist, none has an
`[engine.video]` key, and no tier row spends any of them.

**This is also the honest answer to "the AO looks weak"** if that comes up: the
tint makes it weaker on purpose, and the fix is turning the intensity up rather
than removing the tint.

## The bent normals weakened one cross-path guard, and it is recorded (2026-09-02)

Decision record; the decision is in `docs/backlog.md`.

**Why it moved, and it is the feature rather than a defect.**
`path_lsb_channels`' own entry already recorded that a last-bit depth difference
can flip which tap wins a horizon in `ssao.slang`'s integral. Before the
bent-normal rung a flip could only move the occlusion **scalar**, which scales
the ambient — second order in the pixel. The direction is downstream of the same
max, and `mesh.slang` now samples the probe irradiance _along_ it, so a flipped
tap turns the lookup rather than dimming it. Measured at **9 channels, worst by
2** out of 196608, identical on the runner's Mesa 25.2.8 / LLVM 20.1.2 and on
Arch's Mesa 26.2.1 / LLVM 22.1.8; radv answers zero. The differing channels are
a contiguous cluster along the top edge rather than the pre-rung pair, which is
what one flipped tap looks like — `path_lsb_channels`' own entry carries the
coordinates.

**What is owed:**

- **Whether the damping is worth taking.** The remedy recorded under the
  bent-normal slice for a different reason — slerp the bent direction back
  towards the shading normal by the occlusion scalar, in `mesh.slang`'s
  `bent_normal_at` — would also damp this, by making a lightly-occluded pixel
  depend on the horizon direction not at all. Whether it takes `Probes` back to
  one level is unmeasured, and it is a picture change, so it is not a thing to
  do purely to restore a guard.
- **The user's call on whether that trade is acceptable at all.** Two levels out
  of 256 on nine channels is three orders of magnitude under the failure this
  guard exists for — a cluster that did not draw — so its teeth are intact. What
  is gone is this scene's ability to catch a one-to-two-level regression in the
  probe term specifically.

## What the bent-normal slice left owed (2026-09-02)

Decision record; the decision is in `docs/backlog.md`.

- **Specular occlusion, which is the other half the plan asks for.** It needs a
  cone angle beside the direction and the channel does not carry one:
  `Rgba8Unorm` is spent, so the angle wants either a second image or a swap to
  an octahedral pair in `.gb` with the angle in `.a` — the encoding the slice
  turned down for a three-channel direction with no seam and no fold. Until it
  exists, the SSR row's refusal of specular occlusion (_What the deleted
  47-reflections plan left behind_) stands and is correct.

- **The tier split the 2026-08-30 decision asked for.** The user's call was
  scalar-only on low and the widened target on medium and high. What landed is
  the widening on _every_ tier, because
  `crcbl_render::TransientImageDesc::ambient_occlusion` is one format and a
  per-tier format means a second pipeline, a second bind-group layout and a
  second `mesh.slang` binding type. `r_ssao_bent_normals` in
  `crates/crcbl-render/src/ssao.rs` turns off the _arithmetic_ and nothing turns
  off the bandwidth. Whether low should set it — and through what, since a
  console variable is not reachable from a preset — was the same open question
  as the two knobs in this file's HIGH PRIORITY entry and the contact-shadow
  entry. **Answered since:** `crcbl::settings`' `SSAO_BENT_NORMALS_KEY` is the
  `[engine.video]` key, and `crcbl::settings::presets` writes it `false` for Low
  and `true` for Medium and High, so low pays the bandwidth and not the
  arithmetic.

- **The direction is written in world space, not the view space the brief asked
  for.** Every other part of the encoding decision is as specified. The reason
  is that `mesh.slang`'s consumers — `sky_irradiance` and `probe_irradiance` —
  evaluate world-space L1 environments, and `crcbl_shaders::mesh::FrameUniforms`
  carries `view_proj` and no view matrix, so a view-space channel would need
  that struct to grow a member. It cannot grow one from inside `crcbl-shaders`
  alone: `crcbl-dx12`'s device builds `FrameUniforms` field by field with no
  `..Default::default()` spread, so every backend's construction site has to
  move with it. Instead `SsaoParams` gained `inv_view` and `ssao.slang` rotates
  once per half-resolution gathered pixel, which is also cheaper than rotating
  once per shaded fragment. Deriving the camera basis from `view_proj` was
  considered and declined: it depends on the projection matrix's sparsity and
  breaks under TAA jitter. If a later reader wants view space, the cost is that
  `FrameUniforms` change, not a redesign.

- **`apps/lantern`'s `SSR_HIT_TOLERANCE` widened from 0.10 to 0.15 rather than
  being blessed.** `zero_probes_only_remove_the_ssr_and_rough_fallbacks`
  measures, so a golden bless is not available to it. Brighter ambient on the
  reflected surface means zeroing the probe rows removes more of it: the hit
  moved 53.7 to 47.1 on radv (12.3%) and 54.1 to 47.7 on llvmpipe (11.8%), and
  the A/B against `r_ssao_bent_normals` showed the _zeroed_ reading unmoved, so
  the rung widened the remainder rather than changing the hit. The failure the
  constant guards — a fallback substituted for the hit, which reads at or below
  1.0 — is still an order of magnitude away. Not a defect, recorded because a
  widened tolerance is a thing a later reader should be able to check rather
  than trust. **It widened again to 0.28 on 2026-09-04** (`5874479`), when the
  punctual producer put the lamp's bounce behind the hit as well — 61.2 to 47.7
  on radv, 22.1% — and the constant's doc in `apps/lantern/tests/golden.rs`
  carries each step; the teeth are unchanged, the miss still pinned at 0.0.

## What the half-resolution occlusion harness does not cover (2026-09-02)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

- **`SILHOUETTE_SKIP` narrowing: considered 2026-09-02, declined.** Only the
  edge column itself is disturbed — it reads 188 against its neighbour's 164 —
  so the skip looks wider than the observation needs. Two things say leave it.
  The value is the blur kernel's own width (`-1..=2`, four pixels), which is a
  _reason_ the next reader can check; a fitted number is a measurement that
  silently stops being true when the kernel changes. And the gain is at most one
  column of falloff: the sweep note says the rise is monotonic "from the fourth
  column", which does not settle whether that means `edge + 3` or `edge + 4`,
  and settling it costs a GPU sweep to buy a single column. Revisit only if the
  blur footprint changes, which moves the principled value anyway.

## The depth-aware upsample has one reader, not three (2026-09-02)

The ambient-occlusion plan designed the bilateral upsample as **one shader with
three readers** — the AO pass, the reflection march, and the froxel composite,
which already samples a froxel grid far below the frame's resolution and would
trade its nearest-froxel lookup for a depth-aware one. Only the AO reader was
built. The volumetrics plan's rung 3 stopped citing a shared pass — the
generalisation is that rung's own work, now in `docs/backlog.md` under _Froxel
rungs 3 and 4: a filtered 3D target and a density field_ — and the reflections
plan never cited it (checked whole-file for "upsample", "bilateral" and
"depth-aware" before it was deleted on 2026-09-24).

What actually exists is `crates/crcbl-shaders/shaders/ssao_upsample.slang`, and
it is **AO-specific, not a shared pass**: it reads an `Rgba8Unorm` occlusion
image carrying one visibility channel and a bent direction, hard-codes
`RESOLUTION_DIVISOR` as its own scale factor, and returns `1.0` where nothing
was drawn because unoccluded is the identity for the value it carries. A
reflection colour and a froxel lookup share neither that channel layout nor the
far-plane fallback, so making it serve three readers is a generalisation with
real design in it — a rung, not a binding somebody forgot to add.

(This entry said `R8Unorm` until 2026-09-04, and so did
`crates/crcbl-render/src/ssao.rs`'s own module header. The target widened to
four channels when bent normals landed 2026-09-02 — `ssao.rs`'s pipeline builds
`ColorTargetState::opaque(Format::Rgba8Unorm)` and the shader's `fragmentMain`
returns a `float4`. The argument survives the correction and is stronger for it:
a reflection colour is four channels too, and still not these four.)

## SSAO reads no depth pyramid and the banding is bought (2026-09-01)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**The number that did not reconcile has been measured, and the suspect was
cleared.** This entry first recorded the 0.255 ms in `docs/backlog.md`'s _What
GTAO left owed_ against the 582 µs in the sweep above — the same two-slice pass,
same resolution, same driver, 2.3x apart — and named the tangential rung as the
only AO change between the two dates. Re-measured 2026-09-01 on the same
command, `lantern --headless --frames 400 --size 1920x1080` on radv: `ssao` is
**0.488 ms p50 / 0.505 ms p95**, 22.8% of a 2.143 ms frame, and `forward` is
ahead of it at 0.531 ms.

The rung did not cause it. Compiling the pre-rung `ssao.slang` against today's
tree and running the same command measures **0.518 ms** — _slower_ than the
0.488 ms the current shader takes — so the tangential rung made the pass
slightly faster. A second guess was also tested and discarded: bounding the
slice loop by the dynamic count instead of `SLICE_COUNT_MAX`, on the theory that
unrolling four slices while two ship costs occupancy, measured 0.481 ms against
0.488 ms.

**None of the absolute figures above reproduce, and the discrepancy is not
explained — 2026-09-02.** The same command on the same driver, `--release`, now
measures a **0.890 ms** frame with `ssao` at **0.105 ms** (11.8%), against the
2.143 ms and 0.488 ms recorded the day before. Four consecutive runs landed
between 0.890 and 0.905 ms with `ssao` at 0.105 in every one, so the new figure
is not the noisy one.

The drop is not confined to AO, which is what makes it a measurement question
rather than a rendering result: `shadow` 0.350 to 0.135 ms, `forward` 0.531 to
0.256 ms, `ssr` 0.218 to 0.110 ms — a ratio near 2.05 on three passes that no
commit in the window touches. AO's own 4.6x is that same factor times the
halving.

Three candidates were tested and none of them is it:

- **GPU contention during the old run.** Refuted by reproducing the conditions:
  with **three concurrent lantern runs** drawing at 1080p on the same adapter,
  the measured run still came in at 0.957 ms with `ssao` at 0.105 — a 6% cost on
  the frame total and none at all on the pass in question, nowhere near 2.4x.

  **The first attempt at this check was vacuous and is worth recording as
  such.** It launched `run-forward-e2e.sh` as the load and measured 25 seconds
  later; that suite runs its 21 checks in **1.268 s**, so it had exited long
  before the measurement began and the "contention" run was against an idle GPU.
  It reported 0.904 ms — a plausible number, indistinguishable from a real
  result, and evidence of nothing. The redone check asserts the load is alive
  immediately before _and_ immediately after the measured run, which is the
  thing the first one never established.

- **The report's meaning changed.** It did not. `PassStats` has one commit since
  2026-08-28 in `crcbl-render/src/pass_stats.rs`, and that is the commit that
  introduced this format — before both measurements. A label is summed within a
  frame and the occurrence count sits on the row, in both.
- **Something landed that made the frame cheaper.** The only render commits in
  the window are the albedo tint, the half-resolution gather, its test lock and
  the intensity control. All four are AO; none can move `shadow`, `forward` or
  `ssr`.

So what remains is how the old run was taken — most likely a build profile or an
environment difference that was not recorded with the number, which is the
lesson worth keeping. **Treat every absolute figure in this entry dated
2026-09-01 as unreproducible**, and re-measure rather than diffing against them.

**That also dissolves this entry's open half rather than answering it.** The
question was why `ssao` had grown from 0.255 to 0.488 ms and whether a denser
room explained it. There is no growth to explain: the pass measures 0.105 ms
today, below both. The scene-density hypothesis is withdrawn — it was reasoning
from a number that does not stand — and `MIN_RADIUS_PIXELS` making the pass
scene-dependent remains true and remains untested.

## Per-face granularity inside a point light's cube: declined (2026-08-31)

The cadence's unit is the cull, so a point light's `POINT_FACES` faces are
redrawn or held together. Splitting them would need six culls where topic 45's
fourth decision (_What the deleted 45-shadows plan left behind_ above) gives one
— the six faces' union is the light's sphere, which is what the cull tests
against — so the saving would be six `DrawGen`s of device-local memory against
half a light's draws. Declined; revisit only if a scene is measured spending
most of a frame on one cube.

### What the LTC area-light rung left (2026-08-31)

Decision record; the decision is in `docs/backlog.md`.

- **`fill` is drawn on all three kinds, and no sample sets it.** The rectangle's
  frame is `Scene::AreaLight`; the two punctual kinds got theirs on 2026-09-02 —
  `Scene::FillLight` for the picture and `mesh_e2e/fill_light.rs` for the exact
  comparison, two frames of one light with one boolean changed, so its
  assertions are about bits rather than a ratio under a tolerance. Each kind
  fails on its own: pinning `Light::is_fill`'s `Point` arm to `false` reddens
  the point test and leaves the spot's green, which is the failure mode a
  kind-agnostic flag actually has.

- **A rectangle is culled as a sphere — measured 2026-09-02, and the waste is
  not what this entry claimed.** It said the sphere reaches more froxels than
  the rectangle lights, wastefully in proportion to the aspect ratio. The
  aspect-ratio half is false: `mesh.slang`'s rect arm applies
  `range_window(distance to the centre, position.w)`, which reaches exactly zero
  at the same radius the cull tests, so **the sphere is the shading's support
  rather than a loose box around it**. `mesh_e2e/rect_bound.rs` reads the
  cluster grid back and finds byte-identical froxel sets from aspect ratio 1 to
  64 at a fixed radius, on radv and lavapipe alike. Where the sphere does grow
  with the shape it grows because `RectLight::radius` tells the caller to put it
  past the half-diagonal so the panel does not fade before its own edge — the
  shading model's reach, not the cull's slack.

  What is genuinely removable is the half-space **behind** the panel, which is
  what `light_cluster.slang`'s own `KIND_RECT` comment already names: 5.8% of a
  rectangle's froxels at a fixed radius and 12–20% once the radius follows the
  half-diagonal, and that is an over-estimate, since a bound in the pass would
  test the froxel's AABB and that straddles the plane more often than the froxel
  does. A wasted assignment does cost full price — 99.7% of a useful one on
  radv, 96.3% on lavapipe, because there is no early out for a back-facing
  receiver — but scaled against rectangles' share of a forward pass that ceiling
  is single-digit per cent of a frame, in a fixture where sixteen rectangles
  fill every froxel facing away.

  **Not built, and the trigger is a scene rather than a rung**: one dot product
  per froxel-light pair, the same shape as the existing spot-cone arm, worth
  adding if a scene ever appears with many panels facing out of the frustum and
  lit geometry behind them. Today the froxels behind a panel are usually inside
  a wall and outside the frustum, which is what that shader comment says.

- **The fit's grazing shoulder is tens of per cent off the real lobe, and that
  is the paper's trade rather than a defect.** `crcbl_shaders::ltc`'s
  `PUNCTUAL_SHARE` records the measurement against a punctual GGX lobe: 0.037 of
  the answer head on, 0.156 at `N·V` 0.7 and 0.394 at 0.4. Raising `LTC_SAMPLES`
  to 64 and `LTC_FIT_STEPS` to 160 did not move the worst case at all, which is
  what says it is model error and not an unconverged simplex — a three-parameter
  linear transform of a cosine cannot follow the asymmetric tail a grazing GGX
  lobe grows, and the paper's error norm weights the peak instead. Improving it
  means a richer transform (the full five-parameter matrix, or an anisotropic
  fit), which is a bigger table and a different shader.

### The shadow filter costs 48 taps and it timed out the browser gate (2026-08-28)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

Topic 45's ninth decision (_What the deleted 45-shadows plan left behind_)
replaced the 3×3 box filter with a 32-tap rotated disc, in `tile_pcf` in both
`mesh.slang` and `volumetric.slang`. The tenth put a 16-tap blocker search in
front of it, in `sun_penumbra_texels`, on the fragment path and for the sun
alone. So a sun-lit fragment reads 48 texels of the atlas where it read 9, and a
froxel reads 32; and **both counts were chosen entirely on the picture**. The
grain table in the ninth decision says what 16, 24 and 32 filter taps leave on a
smooth shadowed surface, and the wobble table in the tenth says what the search
buys on a quantised edge. Neither says what any of it costs.

**That sentence used to continue "and there is no shadow-pass timing in the tree
to measure it with", and it was wrong.** `crcbl_render::PassTimers` brackets
every pass in the graph with a GPU timestamp pair, `apps/lantern` builds one,
and `crcbl::engine`'s `finish` logs the whole per-pass report at `info`. The
instrument was there for both decisions; nobody ran it. What was true is that
the filter shipped on quality evidence alone.

**Something timed it anyway, and it went red.** The Pages workflow's browser
gate runs each demo against SwiftShader with a `timeout-minutes: 10` cap per
demo, and the per-step durations across four runs are the price of the two
decisions:

| demo    | 00c92e3 | cec27b3 | 713da9d (rotated disc) | c5bdf25 (PCSS) |
| ------- | ------- | ------- | ---------------------- | -------------- |
| quarry  | 480s    | 491s    | 586s                   | 602s — timeout |
| lantern | 471s    | 479s    | 612s — timeout         | skipped        |

Both runs failed on the cap with **every check inside them passing** — quarry
reported 42/42 before the step was killed — because `web/tools/browser-e2e.mjs`
scales its own per-check budgets to the machine it is on, so a slower frame
stretches the run instead of failing it. So the gate cannot say "too slow"; it
can only run out of wall clock, which is what it did. The Pages workflow's own
header already says the per-step caps bound a _hanging_ demo and cannot bound a
total; this is the first time the total was the thing that moved.

Two commits of main did not deploy on that; the caps for quarry and lantern were
raised to 20 minutes on 2026-08-28 and the gate has run inside them since. The
cap change bought the site back and priced nothing.

**Two of the four answers below are now taken.** The first — topic 45's eleventh
decision, 2026-08-28 — put a five-tap probe in front of the disc, so a fragment
away from a shadow edge costs 5 taps rather than 32 and a sun-lit one 21 rather
than 48, moving no golden on either adapter. The second was to run the timer
that already existed, and it did: `lantern --headless --frames N --size WxH`
under `RUST_LOG=info` prints the per-pass report, and the filter's cost is in
the **`forward`** row rather than the `shadow` one — `shadow` is the atlas draw
and did not move.

| Adapter           | `forward`, disc only | `forward`, with the probe | Cut |
| ----------------- | -------------------- | ------------------------- | --- |
| radv, 1920×1080   | 0.303 ms             | 0.221 ms                  | 27% |
| llvmpipe, 960×720 | 11.281 ms            | 8.135 ms                  | 28% |

Medians of five runs and of three. **This also settles what was not yet known**:
a SIMD GPU and a scalar software rasteriser cut by the same share, so the 48
taps cost what they cost because they are taps, not because of the branch
divergence a SwiftShader lane adds by running every tap the widest fragment in
its group takes.

### DECIDED — the vertex and material strides widen once, into the compact split-stream layout (2026-08-30)

Decision record; the decision is in `docs/backlog.md`.

**Two calls this leaves open, both the user's:**

- **MikkTSpace.** A tangent for a mesh that ships none has to be generated at
  import, and MikkTSpace is what every tool and engine agrees on — but it is a
  new dependency (`mikktspace` on crates.io) or a transcription of the reference
  implementation into `crcbl-scene`. New dependencies are the user's call; a
  transcription is the bigger review. The layout lands with the derivative frame
  as the no-tangent fallback either way, so this does not block it.
- **The BC encoder** for the block-compressed pages rung — a crate in the bake
  tool or a pinned external `basisu` — which is the same shape of choice and is
  the bandwidth rung's gate; see §2's filtering subsection.

### DECIDED — GI is hardware ray tracing only; the raster stack carries every other tier (2026-08-30)

**The user's decision:** no GI on hardware without ray tracing. The browser
(WebGPU has no ray tracing), lavapipe and every device that lacks the feature
run the traditional raster stack — direct lighting under forward+, cascades and
the shadow atlas, sky IBL, GTAO, SSR — and nothing there approximates a bounce.
On `crcbl-vk` (`VK_KHR_ray_query`), `crcbl-dx12` (DXR 1.1 inline) and
`crcbl-mtl` (Metal ray tracing, `intersection_query`) GI is the runtime-traced
probe volume below, tracing on the hardware through **inline ray queries in
compute** — the one shape all three expose and Slang targets with one source —
behind a `Capability` the device reports and the quality presets read. The
standing rules hold: no bake step, every light dynamic, shadows from the same
maps, and the tracer is priced on the desktop adapter before it counts. What
this buys: the ray budget stops being the question (hardware traversal is two
orders cheaper than a compute BVH), the browser tier pays nothing, and the
raster stack is one stack on four backends rather than two.

**Amended later the same day (2026-08-30):** the tier below ray tracing carries
one bounce after all — the probe decision recorded under _What the deleted
50-irradiance-probes plan left behind_ above: the existing `GpuProbe` grid gains
a per-probe octahedral depth map (rendered from static geometry on load,
re-rendered on demand; a capture of geometry, not of light), `probe_irradiance`
weights probes by a Chebyshev test against it so nothing leaks through a wall,
and a compute pass fills the rows every frame from the sun's reflective shadow
map, each sample gated by the same map. The user chose it on "best-looking for
decent performance, and above all no light leaking": it is the one cheap option
that is leak-free, and it is the same volume the RT tier fills by ray queries.
The lantern and shard bakes leave with the slice that lands it. Order among the
raster items: LTC area lights, the shadow atlas, the AO tint, **this**, then the
atmosphere, then anything else.

**Answered 2026-08-30 on the user's "best-looking for the performance": (i) no
temporal blend — fixed pattern, every probe every frame, on both tiers; (iii)
yes, ray-traced shadows and reflections join the high tier as presets once the
queries exist, priced then. (ii) is a design task for foundation (c), not a
call.** The original wording follows for the record: (i) whether the GI term may
carry a temporal blend now that it never runs on a golden's tier — C2 stands
until this is answered, and the fixed-pattern every-probe-every-frame update is
the default; (ii) what the seam adds — an acceleration-structure build and
refit, a ray-query capability, and the storage the hit shading reads — which is
foundation (c), now `docs/backlog.md`'s _Foundation (c): the
acceleration-structure seam_; (iii) whether ray-traced shadows and reflections
join the RT tier as a preset above the atlas and SSR, which is a pricing
question once the queries exist.

The survey that led here stays below for the record.

### The raster lighting stack: what its twelve calls left (2026-08-30)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

- **Runtime reflection captures — DECLINED.** The rebuilt probe volume is the
  interior environment on every tier and RT reflections are the exact one. The
  SSR row's refusals (_What the deleted 47-reflections plan left behind_).
- **Whether SSGI counts as GI — WITHDRAWN.** The probe volume is the bounce on
  every tier; SSGI would be a second, view-dependent estimate of it for a pass
  of its own. Struck from the GI candidates below and from the rendering-gap
  survey's §5 ordering.
- **Burley diffuse — DECLINED.** Lambert stays, improved by the terms around it
  (multi-scatter compensation, the AO tint and bent normals, LTC area lights,
  the probe bounce). _What the deleted 44-lighting plan left behind_ records it.

#### The survey (2026-08-30, superseded by the decision above)

A survey of what Frostbite, Unreal, Godot and Unity ship for GI, scored against
this tree's five standing constraints rather than in the abstract (the full
report lives outside the tree; this entry is its durable part):

- **C1 — no ninth storage buffer.** `PORTABLE_STORAGE_BUFFERS_PER_STAGE` in
  `crcbl_hal::pipeline` is what a WebGPU device promises, and the mesh
  bind-group layout in `crcbl_render::forward` is at it. GI arrives as a sampled
  image or as rows in the probe buffer `mesh.slang` already reads, or it does
  not arrive on the browser tier.
- **C2 — a frame is a function of its own inputs.** `Tolerance::RASTERISER`, the
  SSR row's history refusal (_What the deleted 47-reflections plan left behind_)
  and the probe volume's DDGI refusal all say so.
- **C3 — the budget.** The whole frame is 0.990 ms p50 at 1920×1080 on an RX
  7900 XTX (`docs/backlog.md`'s _What GTAO left owed_, the 2026-08-28
  distribution).
- **C4 — the software and browser tiers pay for every pass** at ~40× the desktop
  cost.
- **C5 — what exists.** L1 SH probes (`GpuProbe`, `probe_irradiance`), a Hi-Z
  pyramid, GTAO and its blur, SSR; no SDF, no 3D image path (`transient.rs` is
  `ImageType::D2`), no triangle intersector, no triangle BVH — `crcbl_phys::Bvh`
  is SAH over AABBs with ray-vs-sphere and ray-vs-AABB leaves.

**What the four engines say.** Every _runtime_ GI they ship carries state across
frames — Lumen (TSR + probe history), SDFGI/VoxelGI/DFAO/Brixelizer
(incrementally updated cascades), Enlighten (frame-rate decoupled), GIBS
(amortised) — so every one fails C2, and the RT-hardware ones (Lumen HW, Unity
RTGI, DDGI) have no WebGPU path at all; GIBS could trace on a compute BVH but is
the amortisation, see below. Every _baked_ one — Frostbite Flux, Unreal
Lightmass / Volumetric Lightmaps, Godot LightmapGI, Unity Adaptive Probe Volumes
— is exactly deterministic, costs zero passes, and two of the four engines name
it the recommended default for the hardware tier this engine targets. All three
probe formats converged on 4×4×4 bricks of low-order SH: the format `GpuProbe`
already is. What they have that this tree lacks is the thing that fills it — an
offline path tracer.

**Rule from the user (2026-08-30): lighting is not baked.** The sun and every
scene light are dynamic — direction, position, colour, on and off — and no bake
step writes a lighting result into the tree. That excludes candidate 1 (a cooked
irradiance volume _is_ baked lighting) and, for scene lights, candidate 2 (it
bakes the transport of a fixed light set); both stay below for the record of why
they were the survey's answer, and question 4 below is answered "no" by the
rule. It also settles what the tracer is for: **not a bake tool — a runtime.**

**And shadows must work with it (the user, the same day):** every dynamic light
shadows — the sun through the cascades that exist, the scene lights through the
shadow atlas topic 45 pulled forward — and the GI's hit shading reads those same
maps, so a bounce is occluded by the same shadow the eye sees. A BVH shadow ray
at the hit is the desktop-preset upgrade, not the baseline.

**The candidate the rule points at — runtime-traced probes, no history.** The
probe volume `GpuProbe` already is, filled every frame by a compute pass that
traces a _fixed_ ray pattern per probe against a triangle BVH on the GPU —
DDGI's tracer (Majercik et al. 2019) with its temporal blend removed. Every
probe, every frame, from this frame's lights and this frame's geometry, so it
clears C2 the way GTAO does: a fixed pattern and a smooth target (L1 SH is
low-frequency, and needs far fewer rays than DDGI's octahedral maps do). It
needs no RT hardware — traversal is a compute shader over a flattened BVH in a
storage buffer, its own pipeline and bind group, so C1's ceiling on the
_forward_ pass does not bind it. Hits shade with this frame's direct light (the
cascades and the light list the forward pass already reads), which gives one
bounce; a second is a second explicit trace, not a feedback read of the previous
volume. Dynamic geometry bounces too, through a refit of the BVH
(`crcbl_phys::Bvh` refits; the triangle-leaf variant is the same first slice
candidate 1 wanted, and its CPU form becomes the _reference_ the GPU pass is
held to). What it costs is the open number: probes × rays × traversal — the same
8192 × 64–128 rays is of the order of a million rays a frame, which on the
desktop adapter is plausibly inside a millisecond and on lavapipe and the
browser is not; it is priced on all three tiers before it counts, per plan 43,
and the web tier runs it reduced or off behind the quality preset. The
alternatives that also satisfy the rule — SDFGI/Brixelizer (SDF generator, 3D
images, cascade state), voxel cone tracing (leaks), LPV (single low-frequency
bounce, one RSM per light) — each carry more machinery for less; SSGI stays
candidate 3, the desktop-only contact term on top.

**The candidates, in order.**

1. **A cooked irradiance-probe volume** (the APV / Flux shape). An offline CPU
   gather that writes the probe rows, committed with a `--check` like `dfg.bin`
   and `sky_prefilter.bin`. Clears C1–C5 by construction: zero passes,
   byte-compared as an artifact rather than blessed as a golden, identical on
   every tier; ~384 KB for an 8192-probe volume. First slice touches no renderer
   code: `ray_vs_triangle` (Möller–Trumbore, tested against literature values)
   and a triangle-leaf `Bvh`, red-checked by sabotaging the intersector. Second:
   `cook-probes` on `cook-dfg`'s terms. Third: `apps/lantern` swaps its analytic
   `bounce` for the general bake — the two must agree on a box. Static geometry
   and, alone, static lights. Overturns the rendering-gap survey's "no baked GI"
   as a decision, closes the probe plan's deferred bake, and makes P7C's
   ray-traced GI row worth re-arguing as reflections and shadows only.
2. **Bake the transport, not the answer** (the Enlighten reduction). Per probe,
   the L1 response to a small basis of sun directions plus a sky term; the host
   folds the live sun into a weighted sum of rows before the upload `ProbeTable`
   already does. No shader change, no binding, zero passes; the basis multiplies
   committed bytes (six directions ≈ 2.3 MB for 8192 probes), not device memory.
   Anti-vacuity: a basis of one reproduces candidate 1 bit for bit. Strictly
   after candidate 1.
3. ~~**Non-temporal SSGI over the Hi-Z pyramid, delivered as an image.**~~
   **Withdrawn 2026-08-30** — the probe volume is the bounce on every tier. The
   plan's SSGI row, with one correction: the rendering-gap survey's §9 and the
   antialiasing plan filed it behind motion vectors for temporal accumulation,
   and that is a choice — GTAO's fixed-pattern-plus-blur determinism argument
   transfers to a cosine gather. Costs: it needs an albedo the tree does not
   expose (the scene target is shaded colour; the SSR row refuses a G-buffer),
   so it opens with a third attachment or a visible approximation; without
   accumulation the only quieting tool is a wider blur, which confines it to
   contact scale; and two GTAO-class passes is roughly a doubling of the frame.
   The only candidate that gives dynamic objects any indirect response.

**Considered and rejected — do not re-propose without answering the reason:**
Lumen software (8 ms at 1080p against a 0.990 ms frame; TSR-dependent; per-mesh
SDFs, card atlas, virtual texturing); every RT-hardware family (no WebGPU ray
tracing in 2026; all temporal); Godot SDFGI and AMD Brixelizer (the best RT-free
runtime answers, but SDF generator + 3D images + cascade state — reconsider only
if question 1 below is "fully dynamic"); Godot HDDAGI (unmerged, re-check in a
year); voxel cone tracing (thin-wall leaks, already "largely superseded" in the
rendering-gap survey); radiance cascades in 3D (right philosophy, "remains an
open problem" per radiance.wiki, the 0.3 ms figure is a 2023 demo); NRC
(training signal is live path-traced rays); Frostbite/SEED surfel GI, "GIBS"
(read in depth after the survey, and the survey's one-liner was wrong twice:
surfels spawn from the G-buffer in 16×16 screen tiles, not at ray hits, and only
6 of its >50 dispatches trace rays, so a compute BVH can stand in for RT
hardware — but the point of the cache is temporal amortisation, 1–2 rays per
surfel per frame over a multi-scale mean estimator, with six frame-carried
states of which surfel placement cannot be shed without collapsing into
screen-probe SSGI; shipped at 2.5 ms on XBSX at 1440p and frozen during gameplay
in College Football 25 to fit 2 ms at 60 Hz; the WebGPU port's integrate pass
binds 10 storage buffers against the 8 this tree pins, and needs a storage image
no `.slang` here uses yet); full Enlighten (custom clustering pipeline;
candidate 2 is the part worth having); lightmaps before probes (needs UV unwrap,
atlas and a denoiser on top of everything probes need); parallax-corrected
cubemap probes (cube arrays, mip chains, `SampleLevel` — all refused in the
probe design; the specular twin of candidate 1, re-open after it); SH L2 (priced
in the probe design; a contained escalation later).

**The questions, and the one that decides the rest:**

1. **Must indirect light respond to geometry that moves** — doors, destruction,
   player-built structures? _Lights_ are answered: dynamic, by the rule above.
   Geometry is still the user's call, though the runtime-traced candidate makes
   it a refit rather than a different design. Yes means candidates 1 and 2 are
   insufficient and the answer is a cascaded SDF, at the cost of an SDF
   generator, a 3D-image path across four backends and per-frame determinism for
   the GI term. Static geometry with dynamic lights means 1 + 2 beat anything in
   the plan for zero milliseconds. Every engine surveyed recommends the baked
   answer for this hardware tier.
2. May a golden ever carry a temporal component? If the answer is a permanent
   no, it belongs in the rendering-gap survey's rules as one constraint rather
   than three scattered refusals.
3. What is the GI budget in milliseconds, on which tier — is candidate 3's
   doubling acceptable on desktop if off by default on the web tier?
4. ~~Is a bake step acceptable in the content pipeline?~~ **No** (the rule
   above). A scene acquires a committed, `--check`ed artifact and a scene edit
   means a rebake. Everything above depends on this being yes.
5. Do `ray_vs_triangle` and the triangle BVH live in `crcbl-phys` (available to
   gameplay queries too) or bake-only beside the `cook-*` tools? No new
   dependency either way.
6. Does `apps/lantern` stay the GI acceptance fixture, or does GI want its own
   demo (which joins every list `tools/check-browser-gate-demos.sh` enforces)?
7. The lantern downlight question below becomes moot under candidate 1 — every
   static source is in the gather — so it can close with that slice.

Nothing here is started until question 1 is answered.

**Where it sits in the schedule (2026-08-30):** the bake tool itself is
foundation (c) (`docs/backlog.md`, _Foundation (c): the acceleration-structure
seam_), and candidate 1's probe volume is its first output — so the tool is
scheduled, and its first output waits on question 1, not the other way round.

### The mesh goldens absorb a whole-frame darkening of a few per cent (2026-08-27)

Measured while red-checking the fog composite, and worth keeping because it was
a surprise. Flooring the fog density at `0.01` — which darkens every lit texel
of the demo cube by roughly three per cent — leaves all four goldens in
`crates/crcbl/tests/mesh_e2e/goldens.rs` **green** under
`crcbl_golden::Tolerance::RASTERISER`.

Two reasons, both structural rather than a slack tolerance: the default key
light is bright enough that much of the cube tonemaps to saturation, where a
three per cent cut changes nothing at all, and the swapchain is sRGB, so a
linear cut of three per cent is a good deal less than three per cent of an
encoded channel.

**What this means for anyone reasoning about coverage:** "the goldens would have
caught it" is not a safe assumption for a small _uniform_ change to the whole
frame. They catch structure — a moved edge, a missing pass, a wrong colour — far
better than they catch gain. A term that scales the whole picture slightly needs
an assertion of its own, which is why the fog slice's zero-density claim is
carried by exact arithmetic (`crcbl_shaders::fog`'s
`the_exponential_is_exactly_one_at_zero` and the composite the shader guard
pins) rather than by a golden.

**Not measured:** where the threshold actually is. Nobody swept the darkening
until a golden reddened; the one figure above is the one data point.

### The Hi-Z march trusts a genuine crossing less than the strided one did (2026-08-27)

`ssr.slang`'s `bound` — `saturate(1 - behind / thickness)`, the soft half of the
thickness rejection — reads lower under the hierarchical march for the same hit,
so a little more probe environment is blended behind a valid screen reflection.
It is why `apps/lantern`'s `SSR_HIT_TOLERANCE` moved from 0.06 to 0.10. Not a
defect that was found and left: it is a measured difference nobody has
explained, and it is recorded so the next person does not repeat the search
below.

**What was measured** (the probe point in
`zero_probes_only_remove_the_ssr_and_rough_fallbacks`, both adapters agreeing to
a tenth of a percent):

- Strided march: 51.8 with probes, 49.2 without. Hierarchical: 46.8 and 43.4.
- With `bound`, `border` and `distance` all forced to one, **both marches read
  53** — so the hit texel and its colour are the same and the whole difference
  is the weight.
- Forcing `distance` alone to one moves nothing, and `border` alone moves 0.1,
  so `bound` is the entire term.
- Thresholding `behind / thickness` instead of ramping it: the strided march has
  almost every ray under 0.1, the hierarchical one spreads to 0.4.

**Three explanations tested and rejected**, each by measurement:

1. **A per-pixel-rate thickness** — dividing the ray's depth advance by the cell
   span before `thickness_at`. Byte-identical output. `THICKNESS_FLOOR`
   dominates at this pixel, so the advance term is not what is being compared
   against.
2. **Measuring `behind` at the cell entry** rather than at the exit, so the ray
   depth and the sampled texel are the same point. 43.5 — worse, because a
   smaller `behind` also relaxes the `behind <= thickness` gate and the march
   then accepts an earlier, darker crossing.
3. **Measuring `behind` at the midpoint of the cell traversal**, both as the
   gate and as the fade alone. 44.9 and 43.0 — worse both ways, which also says
   the sign of the ray's depth change along these rays is not what the
   entry/exit argument assumed.

**Not tested:** a minimum-travel gate before a hit is allowed was swept at 0,
1.5, 3 and 6 pixels and changes nothing, so near-origin self-intersection is
ruled out. What has _not_ been tried is instrumenting `behind` and `thickness`
per pixel into a debug attachment; every measurement above is the one blurred
pixel the lantern test reads, which aggregates a neighbourhood through
`ssr-blur` and cannot separate one ray from another. That is the next step if
this is picked up.

**Why it was left:** the reflection is better by every other measure — the
`Scene::Ssr` golden re-blessed to a smooth gradient where the strided march
stepped, the other six lantern claims and both lantern goldens are unchanged,
and the residual is a fade constant rather than a wrong pixel.

## Surprises worth keeping — not bugs

### Two geometry paths agree to the last bit only by luck (2026-08-27)

`a_multi_cluster_mesh_draws_the_same_frame_through_both_geometry_paths` in
`crates/crcbl-vk/tests/vk_e2e/mesh.rs` compared the mesh-shader and
indirect-count frames byte for byte, and had done since it was written. The
multi-scatter compensation term reddened it on lavapipe — one pixel of 49 152,
red 156 against 157, in the interior of a wall rather than on an edge. radv
still draws the two paths identically.

The cause is not the term. The two paths run the same fragment stage, but the
interpolated position and normal reach it from `mesh.slang`'s vertex stage and
`mesh_cluster.slang`'s mesh stage — two separately compiled modules, which a
driver may contract or reassociate differently. Any shading change moves which
pixels sit on an 8-bit rounding boundary, so the byte claim was one edit away
from failing whatever the edit was. The comparison now allows one 8-bit step and
no pixel beyond it (`PATHS_AGREE` in that file).

**Not tried: `precise` on both position outputs.** HLSL/Slang's `precise`
forbids the reassociation and contraction that would explain the drift, and
would restore byte equality if that is the whole cause. It was not attempted
because it constrains the hot vertex path on every backend to fix one pixel on
one software rasteriser, and because the mesh path also assembles its triangles
from cluster corners rather than from the index buffer — so a corner ordering
that differs from the index buffer's would move the barycentric evaluation and
`precise` would not touch it. Whether the two orderings agree was not checked.

### No transparent pass, and therefore no depth sort (2026-08-27)

Decision record; the decision is in `docs/backlog.md`.

**Scoped 2026-09-06, and held on the decisions below.** The whole rung was read
against the tree before anything was written, because the rendering-gap survey's
§3 said the order-independent question "should decide about before it is built,
not after", and the sort's shape turns out to be a decision too. What the
reading settled:

- **The blend state and every backend are already there.**
  `crcbl_hal::pipeline::BlendState::alpha` is the match — `mesh.slang`'s
  `fragmentMain` already writes `float4(lit, albedo.a)` in straight alpha, so
  the value a blend would consume is in the target today — and `crcbl-vk`,
  `crcbl-dx12`, `crcbl-mtl` and `crcbl-webgpu` each translate it for the overlay
  passes. The only hal work is a constructor beside `ColorTargetState::opaque`
  that carries a blend, plus one with an empty write mask for the reflectivity
  and motion targets, which a blended surface must not write (the SSR row's
  refusal, recorded under _What the deleted 47-reflections plan left behind_,
  and motion has no opaque history to be consistent with).
- **Where it sits.** After `"sky"` (`crcbl_render::sky_pass` is `LoadOp::Load`
  and fills only far-depth pixels, so a pass before it blends over clear), with
  `depth_read(scene_depth)` and no depth write, exactly the attachment shape
  `sky` already has; `debug_draw` is the precedent for the blend state over HDR.
  Whether it lands before or after `"volumetric-composite"` and `"ssr"` decides
  whether a blended surface is fogged and whether it can be a reflection source;
  both answers are defensible and neither is chosen.
- **The mode bits have room, and the room is a layout change.** Nothing reads
  `GpuInstance::flags` above `MATERIAL_MODE_MASK`'s two bits, so widening it to
  three is byte-neutral for every instance record; the cost is the twin
  constants (`INSTANCE_MATERIAL_MODE_MASK` in `draw_gen.slang` and
  `mesh_cluster.slang`), `DEPTH_MODES` and `ForwardRenderer::depth_partitions`
  moving together as their docs demand, and a bucket per mode the scene holds in
  `DrawGen::bucket_start_word`'s region. An all-opaque scene keeps one bucket
  per mesh level, which is what should keep every golden untouched, and
  `an_all_opaque_scene_keeps_one_bucket_per_mesh_level` is the assertion to read
  before spending anything else on the claim.
- **The scatter's order is arbitrary by design.** `draw_gen.slang` says "nothing
  here depends on the order" of the slots its atomic hands out, so a blended run
  is nondeterministic today; the sort is what would fix that, and
  `draw_scene_on_every_geometry_path`'s byte equality across `EmitTail` arms is
  the test that would prove it.
- **One key per instance is not enough.** A bucket is one indirect call whose
  instance count is the whole run (`BucketDraws::record`), so sorting inside a
  bucket orders one mesh's instances and two blended meshes still interleave in
  bucket-table order. A global order needs one call per blended instance (breaks
  §3.3's fixed CPU-side record), or one bucket per blended mesh (only if they
  share a mesh), or an argument array the sort writes with the count from GPU
  memory — which only `EmitTail::Count` can consume; `PerBatch` and `Mesh` have
  no such tail. This is the rung's largest open question.

**Decisions only the user can make, in the order they gate the work:**

1. Is order-independent transparency refused now, or left open? Weighted-blended
   OIT cannot be blessed against a reference; deciding this first is what stops
   the sort being built and then discarded.
2. Which of the three indirect shapes above carries the global order, given two
   geometry paths cannot do the third.
3. Do blended surfaces cast shadows? glTF says nothing — `alphaMode` governs
   only the base colour's alpha. Excluding the mode from `depth_partitions`
   gives "no", keeps the prepass honest and makes one sabotage serve two claims;
   an opaque shadow is free and wrong-looking; a partial shadow needs a
   mechanism the atlas has not got.
4. Before or after the volumetric composite and the SSR march.
5. Is `BLEND` exclusive with `MASK` (six modes, as glTF's one `alphaMode`) or
   orthogonal to it (eight, more buckets)?

### The atlas re-tiling's leftovers: resolution, and one option declined (2026-08-26)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

The budget question — how many point lights may cast — was answered by widening
the atlas to `SHADOW_ATLAS_COLUMNS` × `SHADOW_ATLAS_ROWS` tiles of
`SHADOW_TILE`, which ships. What that decision left behind:

**A quarter of the linear shadow resolution went with it**, deliberately: the
grid gained a column and a row while the tile shrank so `shadow::atlas_extent`
would not move, so every map is now 768 texels a side rather than 1024. The
visible cost is measured — `crates/crcbl/tests/golden/dunes.png` had 8.61% of
its pixels differ at all and 4.03% past the comparison's tolerance, worst
channel delta 40, and the diff image puts every one of them on a shadow edge —
but two things moved with it and are worth knowing before reading either as a
regression:

- `apps/lantern`'s `MIRROR_FRACTION_OF_PLASTER` was re-measured from 0.20 to
  0.14. The mirror itself did **not** move (20.3 before and after; it has no
  direct light on it); the directly-lit plaster it is compared against went from
  83.3 to 119.9 as the coarser maps' larger world-space bias let more of the
  lamp reach it. Measured on lavapipe, both sides, at 256×192.
- `room.png` and `live.png` were re-blessed for the same reason.

**The other two options, and why neither was taken.** Recorded so they are not
re-argued:

- **4×4 at 1024** buys the same capability and costs +28 MiB of `D32Float`,
  which matters because milestone 1's peak wasm memory is still unmeasured (see
  the milestone-1 figures entry). Revisit this if shadow resolution ever becomes
  the thing that is failing; it is a one-constant change now that the grid is
  already four wide.
- **Dual-paraboloid point shadows** — 2 tiles per point light instead of
  `SHADOW_POINT_FACES` — is **declined** unless someone overrides it. The
  paraboloid warp is nonlinear across a triangle, so it is wrong in proportion
  to how large the triangles are, and every `crcbl::greybox` scene is large flat
  quads: its worst case. Cube maps have no such dependence on tessellation.

**No budget makes every light cast, and that is the design.** `shard`'s zone has
more lights than `shadow::LIGHT_SLOTS`, so the renderer still ranks them and
shadows the ones that win. What was wrong before was the budget being one point
light, not the ranking.

### A tile's border needs no explicit locking — the decimator already holds it

Found on 2026-08-20 by a red-check that came back green, while building quarry's
tiling case for "border locking on a tiling mesh".

`crcbl_quarry::tile` first called `simplify_with_locked_edges` with every one of
the tile's border edges named. Replacing that list with `&[]` **passed
identically**: the two tiles' shared seam survived decimation bit-for-bit either
way.

The reason is in `crcbl_scene::simplify`'s own module docs and was there all
along — "an edge used by any number of faces other than two is a border (or a
non-manifold seam)… an open mesh keeps its boundary loop exactly". A tile is an
open mesh and its outer border is a mesh border, so the decimator locks it
unconditionally.

**So `simplify_with_locked_edges` is for boundaries interior to the mesh**,
which no rule over the two arrays can find — a cluster group's outer edge, which
is what `crcbl_scene::cluster_dag` passes it and remains its only caller. Worth
recording because the sample's own exit criterion is phrased as though a caller
must do the locking, and the next reader will otherwise write the same redundant
call.

**What this does not say.** It says nothing about UV or normal seams, which are
`crcbl_scene::simplify`'s other stated limitation and which quarry's single
untextured material cannot exercise: a seam in an attribute the decimator does
not carry is invisible to a position-only comparison like this one.

## Normal maps: what the tangent and page rungs left (2026-08-30)

Record; the open work is in docs/backlog.md under the same heading.

- **Surprise, not a bug: only the browser gate sees a uniformity break.**
  `shading_normal_of` was first written with its `layer == 0` early return above
  the derivatives and an implicit-LOD `Sample` below them. SPIR-V, MSL and DXIL
  all compiled that, every Vulkan suite passed on both ICDs, and the browser
  refused the module outright —
  `'textureSample' must only be called from uniform control flow`, with the
  fragment input named as the possibly non-uniform value. WGSL's analysis is
  static and cannot use the fact that the material row and the frame word are
  `nointerpolation`. The shape that satisfies it is in the function's own doc;
  the point for next time is that a native-only run says nothing at all about
  whether a fragment stage will parse, and `web/build.sh` plus
  `run-browser-e2e.sh` is the only thing that does. `fxaa.slang`'s `tap` learned
  the same lesson earlier and its doc says so too.

## The material lookup moved to the fragment stage, and what that probe learned

Record; the gap it leaves — nothing below the GPU seam checks a bind-group
layout's visibility against the module bound to it — is in docs/backlog.md under
the same heading.

### What the probe found, which is not what it was pointed at

**Every one of the four targets emits the flat qualifier**, read out of this
crate's own regenerated artifacts with slangc 2026.14: SPIR-V decorates both
sides `Flat`, WGSL writes `@interpolate(flat) @location(3)`, MSL puts `[[flat]]`
on the fragment's `[[stage_in]]` struct — which is where Metal reads it, not the
vertex output struct — and DXIL's input signature lists `TEXCOORD 0` as
`nointerpolation`. No divergence to report.

**Dropping `nointerpolation` does not make a golden go red, and cannot.** Tried
it, on both backends that run here:

- **SPIR-V repairs it.** Slang drops `Flat` from the vertex _output_ but keeps
  it on the fragment _input_, which is the decoration that decides
  interpolation, so `vk` draws a bit-identical frame:
  `golden cube on vulkan — 256x192: 0 pixel(s) differ at all (0.0000%)`.
- **WGSL refuses it**, and does so before any frame — naga rejects the module
  with "`@interpolate(flat)` must be explicitly specified for integer I/O". That
  is caught by `crcbl-shaders`' own
  `wgsl_validation::every_committed_wgsl_artifact_validates` on a machine with
  no GPU, which is a better gate than a golden anyway.

**And the cube scene could not detect a wrong interpolation _mode_ even if one
existed**, which is worth knowing before trusting it for the next varying. The
material id is constant across every primitive — all three vertices of a
triangle belong to one instance — so flat and linear interpolation of it agree
by construction, and there is no "fragment between two vertices" that could
resolve a third row. What `nointerpolation` actually buys here is what
`sprite.slang`'s `sheet.z` note says it buys: an exact integer instead of one
that arrived through a float unit and truncates a row early.

**What the golden does detect is a fragment resolving the wrong row**, which is
the failure a texture fetch would produce and the reason the scene's two
pyramids are in unlike colour families. Pinned by making the fragment stage read
a fixed `materials[0]` and rendering:
`256x192: 4105 pixel(s) differ at all (8.3516%), max channel delta 105, 4105 over tolerance (8.3516%), mean abs error 2.0736, rmse 11.1112, ssim 0.991305 — failed: TooManyDifferingPixels`,
the same line on `vk` and on `wgpu`.

**`msl` and `dxil` were not rendered.** Nothing here runs Metal or D3D12, and
they are the two whose lowering this probe least exercises. Their artifacts were
read and carry the right qualifier; CI is the only thing that can say the frame
does too.

### Two scene constants the new lobe invalidated

Both were calibrated against a Blinn lobe at a fixed strength of 0.35, which is
several times brighter than a four-per-cent dielectric GGX lobe at the same
angles. Neither assertion was weakened; the scenes were recalibrated.

- **`crcbl::screenshot`'s green point light.** `scene_lights`' own rule is that
  each light's colour is chosen against the material under it — and "the
  material" turns out to be the mesh's vertex colour as much as the row's
  factor. Every pyramid shows the same purple `+Z` face
  (`PYRAMID_SIDE_COLORS[2]`, whose blue is nearly three times its green), so the
  green light was the one fighting its own surface, and the Blinn highlight was
  what carried it. Its blue is now 0.1, the same as the red light's weakest
  channel. **This file is outside the slice's brief and the edit is one
  constant**; the alternative was to change what
  `each_point_light_pools_where_it_was_put_and_nowhere_else` asserts, which
  would have been weakening a test to fit the code.
- **`render_e2e`'s two-geometry-path comparison is no longer an exact byte
  compare.** It is now "at most `path_lsb_channels` channels differ, and never
  by more than 1". The mesh arm and the indirect arm transform a vertex through
  two different shaders (`mesh_cluster.slang`'s mesh stage, `mesh.slang`'s
  vertex stage) and are not obliged to contract their multiply-adds alike; a
  sharper highlight turns that last bit into a pixel where a broad one absorbed
  it. Measured: llvmpipe disagrees on **one** channel of the dunes frame, by
  one, out of 196608; radv and wgpu are still byte-for- byte identical on every
  scene. The budget is 16, two orders of magnitude under anything a level that
  failed to draw would produce.

### The render-e2e observable, and what it does not say

`the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one` in
`crates/crcbl/tests/render_e2e.rs` is the check that the lobe actually responds
to the column: `Scene::Cube`'s two top pyramids are the same mesh at the same
orientation under the same sun, and `crcbl_render::forward`'s
`PYRAMID_ROUGHNESS` is the only shading difference between their rows. It
measures the **falloff across one face** — inner block over outer block — on
both, and requires the smooth one's to exceed the rough one's by
`HIGHLIGHT_FALLOFF_RATIO`. Proven red: with both rows at one roughness it
measures 1.057 against 1.003 and fails; as the renderer writes them, 1.357
against 1.003.

- **It is a falloff and not a width, and the brief that asked for it wanted a
  width.** "Brighter at its centre and narrower across it" needs the lobe's
  centre to sit inside the measured surface with room either side. Under a
  _directional_ light on a _flat_ face the half-vector sweeps monotonically
  across the face, so the highlight's centre is at the face's inner edge and the
  frame shows one flank of the lobe. The falloff across that flank is the same
  claim by the only statistic this geometry supports. A scene with a curved
  surface, or `Scene::Spot`'s floor with two materials on it, is what would let
  the width be measured directly — and neither exists.
- **The right-hand pyramid is the only surface in the frame at the mirror
  direction.** `DirectionalLight::default`'s sun comes from `+X` and that
  pyramid stands at `+X`; the left-hand one's face never reaches the reflection
  angle, which is why the same pair of roughnesses leaves it flat and why it
  works as the control. The consequence is that the roughness edit had to go on
  the _tinted_ row, so `material_rows`' "one row and two single-column edits"
  invariant is now "two edits, neither of which can be mistaken for the other".
- **A metal is covered by `apps/lantern` alone.** Its brass block and mirror
  panel are fully metallic rows under a golden, so the `F0` interpolation and
  the `1 - metallic` on the diffuse albedo are exercised there; no
  `crcbl::screenshot` scene sets `metallic` above zero.
- **Not covered locally: Metal, D3D12 and wasm.** The lobe was run on lavapipe,
  radv and wgpu-on-radv. `msl/mesh.metal` and `dxil/mesh.fragmentMain.dxil` were
  regenerated and compile, and CI is the only thing that can say the frame they
  draw matches.

## The AO tuning constants, measured against a real frame (2026-08-13)

Two entries above — under "What screen-space AO left owed" and "What the
depth-weighted blur left owed" — say `r_ssao_radius`, the kernel's lateral reach
and `DEPTH_TOLERANCE_RADII` were tuned against `Scene::Ao` alone and that
`lantern` is what would tune them. This is what `Scene::Ao` could say on its
own; the section after it is the same three questions asked of lantern's room,
which now exists. **Nothing has been retuned in either**; the numbers are here
so the retune has a starting point.

Measured on this box — AMD RX 7900 XTX, radv, Mesa 26.1.6,
`MeshShader / Bindless / Rasterised` — from
`crcbl screenshot --scene ao --size 1280x960`, and from the render-e2e suite's
own printed numbers at its 256×192.

- **The trough is already room-scale in one axis, which the entries do not
  say.** `AO_RUN` is 6.0 and `AO_WALL` is 2.0 world units: a six-metre run
  between two-metre walls. What `Scene::Ao` is missing is not scale, it is a
  camera at eye height inside the box, a corner where three surfaces meet, and
  anything to cast a silhouette. The straight-down camera is load-bearing for
  the measurement it exists for and should not be changed to get those.
- **The occlusion reaches 0.38–0.40 world units from the wall**, against a
  kernel whose stated lateral reach is `7/8 × r_ssao_radius` = 0.4375. Floor
  luma down the middle of the run, averaged over a 3-unit-wide strip: 74.66 on
  open floor, falling to 63.31 at 0.10 from the wall, back within one percent of
  open floor by 0.38. So the term is **bounded by the kernel and not by the
  geometry** — `r_ssao_radius` is doing exactly what it says, and the trough is
  wide enough not to clip it.
- **So it reads as a broad ambient wash, not as contact occlusion.** A 0.4-unit
  gradient against a 2-unit wall is a fifth of the wall's height. Contact
  occlusion in a room wants a tighter band; 0.5 is at the top of the usual
  range. **This is the finding a retune would act on**, and it is a judgement
  about looks, so it wants goldens and a human, which is what the deferral said.
- **The gradient terraced, and GTAO 2026-08-28 is what stopped it.** It was
  about eleven sRGB levels over roughly 150 pixels at 1280×960 — a band every
  fourteen pixels — invisible at the goldens' 256×192 and plain in a
  contrast-stretched crop. Re-measured after the horizon integral shipped, over
  the same 0.40 units of floor approaching the wall and the same strip: the
  hemisphere put 13 distinct levels in 16 steps, so it repeated values; the
  integral puts 19 in 19, which is monotone. The rest of that profile barely
  moved — open floor 75.14 either way, the foot 53.14 against 51.68, the reach
  0.390 against 0.370 — because `Scene::Ao` is a closed trough and the wash the
  integral removes needs an open surface to appear on. `probes` and `lights` are
  where it showed.
- **The bilateral blur holds at a silhouette, with about a fortieth left.** The
  render-e2e's own reading on this GPU:
  `cube — the pyramid's underside measures 67.3 along its silhouette and 66.0 two rows in, against a clear of 37.0`
  — a residual halo of 2.0%, matching what the blur entry claimed. Against a
  _real_ silhouette rather than a pyramid's underside, nothing has been
  measured, because no scene in the tree has one.
- **`DEPTH_TOLERANCE_RADII` remains unmeasured and that is not fixable here.**
  It weights the blur by view-space depth difference, so what exercises it is
  two surfaces at different depths sharing a kernel footprint. `Scene::Ao` has
  one flat floor and two walls at the same depth as it, and `Scene::Cube` has
  one silhouette against the clear. Neither separates the tolerance from the
  far-plane test beside it. A room with furniture is still what would.
- **Where these came from**, so they can be reproduced: `r_ssao_radius` is a
  `convar!` in `crates/crcbl-render/src/ssao.rs` — reachable from the console
  and from an `autoexec.cfg`, which is what makes a sweep cheap now — the
  lateral reach is the `KERNEL` table in
  `crates/crcbl-shaders/shaders/ssao_hemisphere.slang`, which is where that
  kernel lives now that `r_ssao_technique` defaults to the GTAO march, and
  `DEPTH_TOLERANCE_RADII` is a `static const` in `ssao_blur.slang` reachable
  from nothing. So a tolerance retune is still an engine edit and a re-bless.

## The AO constants against lantern's room (2026-08-14)

Record; the measurement it leaves takeable is in docs/backlog.md under the same
heading. The measurement the entry above could not make, now that there is an
eye-height camera inside a real room. Read off `apps/lantern/tests/golden.rs`'s
own projection at 1280×960 on AMD RX 7900 XTX, radv, Mesa 26.1.6,
`MeshShader / Bindless / Rasterised`, averaging a 21×21 block about each world
point. **Nothing was retuned.**

- **AO reads as contact occlusion in a real room, and its reach is the kernel's
  rather than the geometry's.** Floor luma approaching the back wall, in the
  ambient-only part of the room: flat at 77–80 from 1.4 m out down to 0.5 m,
  then 71.5 at 0.35 m, 60.5 at 0.20 m, 61.5 at 0.05 m. A 25% darkening confined
  to the last 0.35 m. The same shape on a wall going up from the floor — 53.8 at
  0.12 m, recovering to 63.7 by 0.6 m — and at the metal block's contact with a
  _sunlit_ floor, where AO touches the ambient alone and still cuts 172 to 130
  over the last 0.3 m.
- **So `r_ssao_radius = 0.5` is sane at room scale**, and the "broad ambient
  wash" reading the `Scene::Ao` entry above reports is a property of that scene
  rather than of the constant: 0.4 units against a 2-metre trough wall is a
  fifth of it, and the same 0.4 units against a 3-metre room wall with an
  eye-height camera reads as the band under a skirting board. **The finding the
  earlier entry proposed a retune on does not survive the room it asked for.**
- **The three-surface corner is measurably darker and is the weakest of the
  three.** Down the diagonal into the floor–back-wall–coloured-wall corner: 99.5
  at 0.40 m, 95.6 at 0.30 m, 83.4 at 0.20 m. A 16% cut where two walls close
  most of the hemisphere, against the 25% a single wall gives. Not chased: it is
  in the sunlit part of the floor, so ambient is a smaller share of the pixel
  there, and separating the two needs an AO-off frame of the same view, which
  `--no-ao` can now produce.
- **`DEPTH_TOLERANCE_RADII` finally has something to be measured against, and
  shows no halo at it.** The room puts two surfaces one to two metres apart in
  view depth inside one kernel footprint at three places: the metal block's
  vertical silhouette edge, the plinth's, and the mirror panel's against the
  back wall. Row profiles across each: the block's edge goes 132.3 to 93.2 in
  **one** pixel with no gradient on either side, and the panel's goes 0.0 to
  57.0 in two. So the depth-aware blur is not smearing a near surface's
  occlusion onto a far one at 2.0 radii.

- **A finding worth more than any of them: the sun's shadow bias contaminates
  the floor's occlusion profile near a wall.** The first profile taken —
  approaching the `-x` wall — read 49 at 0.8 m and 138 at 0.25 m, which is
  backwards. That is the peter-panning recorded in the lantern entry above, not
  AO. Anyone repeating this measurement must take it on a wall whose foot the
  sun does not reach, which is why the numbers above are from the back wall.

## Irradiance probes: the slice plan (designed 2026-08-14)

Record; the limits still deferred are in `docs/backlog.md` under this heading.

The design is recorded above under _What the deleted 50-irradiance-probes plan
left behind_ — a static grid of L1 spherical-harmonic probes in a read-only
storage buffer, adding no render pass, added to `frame.ambient` for diffuse and
returned by an SSR miss for specular. All slices are built and both of its open
questions are taken, so what stays here is the record and the limits.

The seam still permits a read-only storage binding of a host-visible buffer, and
appending the mesh binding after `AMBIENT_OCCLUSION_BINDING` needed no
`mesh_cluster.slang` mirror for the same reason occlusion did not.

### Decisions, both taken

- **Q1: does the probe half of the environment specular evaluate above
  `ROUGHNESS_CUTOFF`?** **Resolved yes.** A wide lobe is where the low-frequency
  probe is more honest than one screen-space ray. The cutoff therefore gates the
  march only; rough surfaces return probe environment with zero sharpness, and
  the blur composites that centre value without filtering. This keeps
  `UNTINTED`'s exact-zero march endpoint without leaving lantern's brass black.
- **Q2: does this get a `RenderEffects` bit?** **Resolved no.** The off-switch
  is the scene, and a zero volume is bit-identical, so there is nothing to
  resolve through four layers. `effects.rs`'s own rule is that an effect which
  is off is a frame with fewer passes and never a shader branch — a probe bit
  removes no pass. If lantern's milestone-4 matrix wants a row anyway, it should
  swap the bound table for the zero one, which is still data and still no
  branch. It is public API shape, so it is yours.

### Named limits, so they are not rediscovered

- **Light leaking was the grid's real weakness, and per-probe visibility closed
  it.** A probe inside a wall used to light the room beyond it.
  `crcbl_render::probe_visibility` renders each probe's octahedral depth map and
  `probe_gather.slang` weights the probe by a Chebyshev test against it, which
  is the first of the two answers the literature offers; DDGI's temporal depth
  moments remain out of scope. What is left is what the RSM updater's own entry
  above records.

- **With `REFLECTIONS` off, metals go black again**, because the reflection pair
  is what draws the environment specular. Coherent rather than a defect, and
  `--no-reflections` showing it is honest.

## The probes fixture is a full-frame gradient, and WARP will not have it (2026-08-14)

`a5f0e29` added `Scene::Probes` and `a88d671` reverted it. **The probe maths was
not what broke** — on the WARP run that failed, the shader and
`crcbl_shaders::probe`'s Rust mirror agreed to 0.07 levels and the two geometry
paths were bit-identical, so the evaluation is right on that device. What failed
was the golden: max channel delta 8, **1212 pixels (2.47%) over tolerance
against `Tolerance::RASTERISER`'s 1% budget**.

### Why, and it is a lesson about fixtures rather than about probes

The fixture was designed so that **every pixel is the probe term and nothing
else** — ambient exactly zero, the sun parallel to the floor so Lambert and the
specular lobe vanish, the measured bands twice the occlusion radius from any
wall. That makes the anti-vacuity argument airtight and it is why the shader
could be compared against the mirror absolutely rather than only as a ratio.

It also makes the whole frame one smooth gradient, which removes the margin an
8-bit golden lives on. Every other scene's cross-driver drift is confined to
edges, so a handful of pixels exceed tolerance and the _ratio_ stays tiny —
`point_shadow` on the very same WARP run has max channel delta **34** and
passes, because only 0.057% of its pixels are affected. A gradient spanning the
frame has no such confinement.

**The two properties are in tension and that was not seen when the design was
written.** "Every pixel is the effect" and "an 8-bit golden survives four
rasterisers" pull against each other, and this is the first fixture in the tree
where the effect covers the whole frame rather than a shape inside it.

### Rebuilding it: two options, and the numbers needed to choose

- **A scene-scoped budget**, the way `path_lsb_channels` in
  `crates/crcbl/tests/render_e2e.rs` already scopes an allowance to
  `Scene::Dunes`. Honest if the argument is written down — the 1% ratio was
  derived for localised edges, not for content that is gradient everywhere.
  Dishonest if it is picked to be whatever makes WARP pass, which is the trap.
- **A fixture that is not gradient-dominated**: keep the probe term as the only
  term but give the frame flat regions — facing quads at distinct normals rather
  than one floor across the interpolation. The ratio assertion survives; the
  golden regains its margin.

**Neither can be chosen from this machine.** The failure only appears on dx12
under WARP, which needs Windows, so any fix validated locally is a guess pushed
to CI. Get the WARP numbers for a candidate fixture before blessing anything —
the previous attempt passed radv, lavapipe, both geometry paths and a four-way
negative control, and still broke `main`.

### Not at issue

`ce253ad` (slice 1) was never implicated and stays. The design's determinism
argument — that probe evaluation has no comparison between fetched values to
diverge on — was _supported_ by this run, not contradicted: radv against
lavapipe is max delta 2, and WARP agrees with the host mirror to 0.07 levels.
The 8-bit golden is the fragile part, not the arithmetic.

**The first replacement preserved the semantics but not WARP's budget.** With a
`0.4`-unit interval, WARP still reported 1,216 pixels over tolerance — 2.4740%
of the frame, effectively the reverted fixture's result — even though every
semantic check passed and the shader agreed with the Rust mirror to 0.20 levels.
The gradient had been confined, but not enough to fit the global 1% budget.

Reducing the interval to `0.1` units disproved that diagnosis: WARP again
reported exactly 1,216 over-tolerance pixels. The uploaded actual/diff artifact
located every one on the thin oblique `±X` wall strips — 656 on the left and 560
on the right. The floor had 8,825 differing pixels but none over tolerance and a
maximum channel delta of 2. The wall strips reached 7 and 8 respectively.

The room is now wide enough to crop those `±X` strips while retaining the `±Z`
walls as context. That changes no measured floor point, probe row, tolerance, or
semantic assertion. The centre is still compared against both endpoints, and the
widened-to-room negative control still fails on an 11.60-level endpoint-region
change against its 0.5-level flatness budget. **The crop held**:
`dx12 e2e (software adapter)`'s ForwardRenderer step runs `render_e2e` on WARP,
and it has been green with `Scene::Probes` in it since.

## The effect toggles landed, and two things about them are owed (2026-08-14)

Record; the coverage gaps this slice left are in `docs/backlog.md` under this
heading.

`crcbl_render::effects` is topic 39's resolution point: `RenderEffects` is the
effect set, `EffectRequest` carries the three requested layers,
`EffectRequest::resolve` applies the order, and `ForwardRenderer::begin_frame`
resolves once per frame and freezes the answer. What follows is what that left.

### The device-capability clamp is real and its rule set is empty

`ForwardRenderer::device_effects` is `RenderEffects::all()`, and that is a
statement about these three effects rather than an unfinished clamp:

- AO has no device fact to gate on, which topic 18 says in as many words —
  "inventing a capability that is really a performance opinion is what topic 39
  exists to prevent".
- The reflection pair says the same of itself in `crcbl_render::ssr`'s module
  docs: every backend has a full-screen draw, a sampled `D32Float` and a sampled
  `Rgba8Unorm`.
- Shadows are a `D32Float` image and a depth-only pass. **Considered and
  declined:** a rule requiring `max_image_2d >= shadow::atlas_extent()`. It is
  true and it is unreachable — a device that fails it cannot create the atlas at
  `build`, so the renderer never exists to be clamped, and writing the rule
  would imply a degradation path that is actually a build failure.

The clamp _step_ is exercised:
`the_layers_resolve_in_the_order_topic_39_specifies` passes a reduced device set
to `EffectRequest::resolve` and checks it wins over an override forcing an
effect on. The first rule that fires arrives with the ray-traced variants, which
`LightingPath` already selects.

## The scene API: the slice plan (decided 2026-08-13)

Record; what the API still owes is in `docs/backlog.md` under this heading.

`apps/lantern` could not be built because an application cannot describe a
scene: `ForwardRenderer::begin_frame` takes the cube's transform as an argument,
five `set_*` methods place instances of meshes the renderer holds the ids of,
and there is no material call on the type. The roadmap already put P9's scene
work before S4B while P7B's deliverable named lantern. **Resolved by pulling the
scene work forward**, rather than by moving lantern.

The resident set is a description now — `crcbl_render::scene` and
`ForwardRenderer::with_scene`, with `new` as `with_scene(&scene::demo())`;
instances are a runtime API: `ForwardRenderer::add_instance` / `set_instance` /
`remove_instance` over a `scene::InstanceDesc`, and the five `set_*` demo
wrappers are gone; `begin_frame` no longer takes the cube's transform — the cube
is an ordinary instance every caller places for itself, by `scene::DEMO_CUBE`
and the other public demo indices. Materials and page layers are the caller's
too: `PageDesc` at the caller's own extent, `push_layer` per layer,
`SceneDesc::materials` row by row, refused at build when a row names a layer the
page has not got. The pools are sized by `Capacities`, and a description that
outgrows one of the four is refused up front rather than part way through
filling it. `apps/lantern` is the application that consumed it, and what still
binds a future caller is everything below.

### The shape

The resident set becomes a description the app hands to `new`; instances become
a runtime API. That split is where the seam already is: pools, the cluster pool,
the bucket table and the page are fixed at build and never grow, while
`MaterialTable::insert` and `InstancePool::insert`/`set`/`remove` are already
per-frame paths. A runtime `add_mesh` would mean recreating the camera's
`DrawGen` and the four shadow ones plus every bind group naming their buffers
mid-life, which is the streaming path `crcbl-render`'s own `mesh_pool` docs
already assign to P9.

### Flat meshes only, and why that is not negotiable yet

`build_meshlets` needs positions alone and emits vertex runs indexing the
original array, so attributes survive exactly — a flat app mesh is fine. **A
cluster DAG is not.** `crcbl_scene::simplify` is position-only and says so in
its own module docs: a coarse level has no normals and no UVs. The engine's one
DAG works because the dunes patch is analytic — `residents` synthesises each
coarse vertex through `crcbl_shaders::dunes::vertex_at`. An app-supplied DAG
needs attribute-aware simplification or nearest-source attribute transfer, which
is unbuilt topic 25 work listed in that plan's own risks. `Geometry::Dag`
carries that constraint in its own documentation, so the limitation is stated at
the type rather than discovered.

### Capacity: a documented cap the caller chooses, never growth

The `POOL_*` constants are fields of `Capacities` now, whose `Default` is the
numbers the engine shipped. Growth is out for the reason `mesh_pool` already
argues — every bind group names those buffers. A description that outgrows one
is refused by `ForwardRenderer::check_scene`, before the first device object
exists, naming the pool, the capacity and what the description needs; the plan's
"`MeshPoolError::PoolExhausted` must reach the caller un-flattened" turned out
to be the wrong answer to that, for the reason the `SceneError` entry below now
records. Worth knowing while sizing: raising the instance cap is not linear,
since the LOD hysteresis buffer is per instance per `DrawGen` and there are
five. `Capacities::instances` is the one number no description can be measured
against — objects are placed while the renderer runs — so filling it is
`InstancePoolError::PoolFull` from `add_instance` and nothing earlier.

### Where the refactor could silently change a frame

Ordered by how quietly each would fail. Row 0 of the material table is what
`GpuInstance::default` names, so a reordered description swaps the pyramids'
materials. Mesh table ids come from upload order and the cull pass reads a
bounding box out of the entry the instance names, which for a DAG is level 0's.
Page layer numbers are a producer's own and nothing checks that a row names the
layer its producer meant — a row pointed one layer along shades a surface with
somebody else's texture. (Row (d) closed the older version of this: layer 0 used
to have to be opaque white, and `GpuMaterial::NO_PAGE` is out of band now, so
layer 0 is an ordinary layer and no page burns one.) `draw_gen`'s scatter takes
the first bucket whose mesh id matches, so two buckets naming one mesh means the
second never draws. Instance index is the LOD hysteresis key, inert with one DAG
instance and not inert with two. And the rollback path gains new early-failure
points that must sit on the same side of the self-cleaning handover, or a
rejected description leaks two device-local buffers.

**Four of these are invisible to `cargo test`**: `crcbl-render`'s unit tests run
on the null backend and cannot tell a right frame from a wrong one. Every
remaining slice is verified by `run-render-e2e.sh` and `run-vk-e2e.sh` on a real
device or it is not verified.

What the landed slices did about each, so the next one does not re-derive it:
row order is `SceneDesc::materials` order and `material_rows` inserts in it,
asserted by `scene`'s
`the_demo_scene_shades_by_omission_through_an_untinted_row`; ids are description
order, asserted by `forward`'s
`the_description_resolves_to_the_ids_it_was_written_in`; a layer's length
against its kind's extent is `PageDesc::check`'s to verify and a row naming a
layer that kind has not got is `check_scene`'s; buckets are built by walking the
mesh list, so a duplicate is not refused but unspellable; and every description
check runs from the top of `ForwardRenderer::check_scene`, before the first
device object exists, which `a_refused_description_creates_nothing_at_all` reads
off the recorder's live object count — with one arm deliberately refused _after_
the pool exists, so that count is evidence about `build_geometry`'s rollback and
not only about `check_scene`.

### The materials-and-layers slice was almost entirely already done

Recorded because the next reader will otherwise re-derive it. Of the three
things that slice was scoped as, the first description slice had already
delivered all three:

- The constants a caller reads the pattern off — `PYRAMID_TINT`,
  `PYRAMID_ROUGHNESS`, `CHECKER_TEXELS`, `CHECKER_LAYER`, `PAGE_EXTENT` — are
  public on `crcbl_render::scene`, and `scene::demo` builds its page through
  `PageDesc::empty`, `set_extent` and `push_layer` like any other caller.
  `UNTEXTURED_TEXELS` does not exist any more, and neither does the burned white
  layer it described: a material that names no texture carries
  `GpuMaterial::NO_PAGE`.
- A row naming a layer the page has not got is already refused by
  `ForwardRenderer::check_scene`, naming the row, the layer and the page's layer
  count, before any device object exists — and
  `a_refused_description_creates_nothing_at_all` already has an arm for it. Not
  duplicated.
- `PageDesc` already lets a caller append layers and gives it no way to write
  one at no extent, its fields being private and `set_extent` the only way to
  size a kind.

What was actually missing was **evidence**, not mechanism: every scene built
anywhere in the tree was `scene::demo()` — three rows, two layers, one extent —
so a `with_scene` that uploaded the first two layers and stopped, or inserted
the first three rows and stopped, would have left all eleven goldens
byte-identical and passed everything else. `forward`'s
`an_app_page_and_table_reach_the_device_whole` is what closes that: a four-layer
page at an extent that is not `PAGE_EXTENT` and six material rows, checked
against the recorded `CopyBufferToImage` per layer and against the material
buffer's bytes per row. Shown red three ways — the page upload truncated, the
row insert truncated, and the row insert reversed.

**Considered and declined: a `PageDesc::layer_bytes` accessor.** `extent² × 4`
is computed in `check` and by any app producing texels for `push_layer`, so
there is a real second caller for it. Left out anyway: it is a convenience
rather than a sufficiency gap — an app has `extent(kind)` and the RGBA8 layout
is documented on `push_layer` — and this slice's whole obligation was not to
manufacture work. Re-checked when row (d) landed on 2026-09-06: still declined,
and the per-kind extent makes it `layer_bytes(kind)` rather than a constant, so
the accessor would now have to carry the kind too.

(The instance-index reuse this slice documented rather than removed is in
`docs/backlog.md` under this heading.)

### Instance order is the caller's now, and it is what keeps a golden still

`ForwardRenderer::new` used to insert the cube itself, so it was always instance
0 and every `set_*` object landed above it. Nothing is inserted at build any
more, so the pool's slot order is the order a caller places objects in — and the
eleven goldens stayed byte-identical across the `begin_frame` slice and again
across the setters' retirement because every caller places the cube **first**:
`screenshot.rs`'s `place_cube`, `vk_e2e`'s `mesh::place_cube` / `place_cube_at`,
and `forward`'s own test helper, each of which says so at the call site.

Retiring the setters made this the whole risk of that slice, and it is why every
converted call site is a straight `add_instance` in the setters' own order, why
the toggling ones hold a handle and `remove_instance` before placing again
rather than inserting twice, and why the three helpers that grew out of it —
`screenshot.rs`'s `place`, `vk_e2e::mesh::place` and `forward`'s `place_demo` —
each say the order is load-bearing where a reader will find it.

Whether a different order would actually move a frame is **not measured**. The
visible list is filled by an atomic, so the draw order is not the pool's order
to begin with; but the instance index is topic 25's hysteresis key (see the
entry above), and a slice whose whole obligation was that no golden moves was
not the place to find out.

### A renderer nobody placed anything in records fewer dispatches

Not a defect, and newly reachable. With an empty instance pool the cull dispatch
covers no workgroups, and `DrawGen::add_passes` records **no dispatch at all**
rather than one of zero — Metal rejects the empty dispatch, and the comment
there says so. Before the cube became a caller's instance the pool was never
empty, so this could not be reached from `ForwardRenderer` at all.

`forward`'s
`the_frame_records_one_indirect_call_per_bucket_whatever_the_scene_holds` is
what found it: its no-pyramid half recorded 7 dispatches against the other
half's 10. It places the cube in both halves now, so the two differ in the
pyramid alone, which is what the test was always about. Nothing else in the tree
draws a frame with an empty pool.

### Declined twice: a `SceneError`, and the second reason retires the condition

The description slice declined one as indirection with a single implementation,
and set a condition for revisiting it: `MeshPoolError::PoolExhausted` reaching
the caller un-flattened, because its `largest_free`-versus-`total_free` pair
tells fragmentation from a genuinely full pool and `HalError` cannot carry that
distinction. The capacity slice revisited it and **declined again, because the
condition is not reachable at `with_scene`**.

`build_geometry` creates the `MeshPool` and then fills it; nothing is ever freed
in between, and `FreeList::alloc` is first-fit over a list that starts as one
block, so every allocation comes off the front of a single trailing block and
`largest_free == total_free` at every failure. The refusal a build can actually
produce says so out loud — breaking the new check and letting the pool refuse
instead prints
`the largest free block holds 1 and 1 are free in total, out of a capacity of 1`.
So the only thing exhaustion can mean here is "too small", the only answer is
"raise the capacity", and both are known from the description before a device
object exists. `check_scene` says it there instead, naming the pool, the
capacity and the need.

The condition becomes real when meshes can be freed and re-uploaded during a
renderer's life — P9's streaming `add_mesh`, deliberately deferred — and that is
the slice where the type earns itself. Not before: today it would still be one
implementation, and it would be carrying a distinction that cannot arise.

### The demo setters are gone, and what indexed the description besides them

`set_pyramid`, `set_tinted_pyramid`, `set_textured_pyramid`, `set_open_box` and
`set_dunes` are deleted, with `ForwardRenderer::place` — the body they shared,
whose swallowed `InstancePoolError::PoolFull` this backlog kept as "it goes when
they go" — and the five `Option<InstanceHandle>` fields, and the
`REQUIRED_MESHES` / `REQUIRED_MATERIALS` floor `check_scene` enforced.

**The floor was not held up by the setters alone**, which is what the plan for
this slice assumed. `ForwardRenderer::build` also indexed the description at
`DEMO_DUNES` in two places — the cluster range it published as `dunes_clusters`
and the per-level bucket list it published as `dunes_level_buckets` — so with
the check simply deleted, a one-mesh description **panicked** out of `build`
rather than being refused. Both are per-description-mesh now: the fields are
`mesh_clusters` / `mesh_level_buckets` and the accessors are
`ForwardRenderer::cluster_range(mesh)` and
`ForwardRenderer::level_buckets(mesh)`, whose only callers are
`vk_e2e/mesh.rs`'s `read_cut` and `selected_dunes_level`. `forward`'s
`a_description_smaller_than_the_demo_is_a_scene` is the test, and it was shown
red both ways — against a restored floor, and against the positional indexing,
where it fails with `index out of bounds: the len is 1 but the index is 3`.

Nothing in `crcbl-render`'s non-test code names a `DEMO_*` constant any more.

### Where the capacity slice drew the line, and what it left to the pool

`check_scene` owns what only the whole description knows — the four totals
(vertices, indices, mesh table entries, material rows) against `Capacities`, and
the cross-references between page, rows and DAG levels. What one mesh's _bytes_
say stays the pool's: `MeshPoolError::VertexStrideMismatch` and `EmptyMesh` are
still raised from inside `build_geometry`, mesh by mesh, and arrive as
`HalError::Backend` carrying their numbers.

**Considered and declined: hoisting those two into `check_scene` as well.** It
would make every description refusal free, and it would also make the
self-cleaning branch of `build_geometry` unreachable from any description — dead
code with a test that could no longer drive it. Left where it is deliberately,
so `a_refused_description_creates_nothing_at_all`'s last arm is a real path: it
appends one byte to the open box's vertices, which is refused on the third of
four meshes with the pool created, its buffers live and two meshes already
staged into them, and asserts both that something _was_ created (or the arm
proves nothing about the rollback) and that the live-object count came back.
Shown red by removing `pool.destroy(device)` from `build_geometry` — 8 objects
leaked, and that arm was the **only** failure in the whole `crcbl-render` suite.

The four capacity refusals are `HalError::InvalidDescriptor` like every other
`check_scene` answer, each asserted against a fragment of its own message so an
arm cannot pass on another check's refusal, and each shown red by deleting its
row from the table — every one of them then reached the pool and came back as
`Backend`. The opposite mistake has its own test, because nothing else in the
tree could fail on it: every other scene reserves far more than it holds, so a
comparison written `>=` would pass the entire suite and refuse only the
application that had sized its pools exactly right.
`a_description_that_exactly_fits_its_capacities_is_built` is what fails there.

(`Capacities::lights`, which nothing drives, is in `docs/backlog.md` under this
heading.)

### What the cook slice actually had to move, and what was already there

`ClusterDag::cook` was **already** in `crcbl-scene` (landed with
`crcbl lod gen`), so the `cook`/`sphere` pair in
`crates/crcbl-shaders/tools/cook-clusters.rs` was a second copy of the same
transcription with nothing between them. The example calls `built.cook()` now
and its own copy is gone;
`cargo run -p crcbl-shaders --example cook-clusters -- --check` is what says the
move changed no byte, and it was shown red by perturbing one cooked vertex index
(`they first differ at byte 4972`).

New is `MeshletBuild::into_clusters`, which is the flat-mesh half an application
needs and had no spelling at all: `build_meshlets` produces three private `Vec`s
and `crcbl_render::scene::Geometry::Flat` takes a
`crcbl_shaders::meshlet::MeshClusters`. `ClusterDag::cook` goes through it now
too (`level.clusters.clone().into_clusters()`, the same three allocations the
three `to_vec`s cost), so the mapping lives in one place.

## Screen-space reflections: the slice plan (decided 2026-08-14)

Record; the one coverage gap left is in `docs/backlog.md` under this heading.

The design and its refusals are in _What the deleted 47-reflections plan left
behind_ above. This is the slice order and what each one's observable is.

**The attachment, march, blur, probe fallback and rough-surface integration have
landed.** The cutoff remains at 0.5 because it gates marching rather than probe
environment specular; the measured cutoff raise below remains a declined
alternative, not pending work.

What those slices found on the way, and what a reader of the design should know
before writing the next one:

- **The reach had to become a share of the frame.** The design said a fixed
  pixel stride and a fixed loop bound, which a first cut read as a fixed pixel
  _reach_ — and a reflection that shrinks as the window grows is the same defect
  the design refuses one level down. `ssr.slang`'s `REACH_FRACTION` is the fix
  and _What the deleted 47-reflections plan left behind_ above carries the
  amendment.
- **The forward pass stores its depth now.** `PassBuilder::clear_depth` is
  `StoreOp::Discard`, and a discarded attachment is undefined rather than
  "whatever was written": radv and llvmpipe handed the values back and wgpu
  handed back the clear, so the same build reflected on one backend and not the
  other with no error anywhere. Anything else that wants to read the depth
  _after_ the forward pass inherits this.
- **`Scene::PointShadow` earned a geometry-path budget.** Its caster carries the
  tinted row, the only demo material under the cutoff, so it is the first scene
  whose pixels come from a march rather than from shading the fragment the
  rasteriser handed over — which makes the depth buffer's last bits visible in
  the picture. One channel, off by one, on llvmpipe alone, stable across runs.
  See `path_lsb_channels` in `crates/crcbl/tests/render_e2e.rs`.
- **The cross-driver evidence, which the design asked for and had none of.**
  `ssr.png` blessed on llvmpipe compares on radv and on wgpu at **max channel
  delta 1, zero pixels over `Tolerance::RASTERISER`** — a _less_ divergent frame
  than `cube.png`, which has no reflection in it and differs on 60% of its
  pixels at the same delta. The structural ratio reads 92.8 against 64.0 on all
  three, to the decimal. `lantern`'s room is where the exposure is visible: of
  the nine pixels over tolerance between llvmpipe and radv, five are in the
  panel's reflecting band and two of those are gross (deltas 66 and 33). That is
  one fixture's worth of evidence, not a general argument, and the design's
  recorded resolution — flatten the reflected content or drop the golden and
  keep the ratio — has not had to be used.
- **The stepping is gone, and it is measured rather than eyeballed.**
  `the_reflection_does_not_step_down_the_band` in
  `crates/crcbl/tests/render_e2e.rs` takes the **second** difference of the
  reflection down single rows of `Scene::Ssr`, so a reflection that merely fades
  down the band scores zero and only the alternation counts: 17.7 levels per row
  with `ssr_blur.slang`'s kernel cut down to its centre tap, 2.8 with the real
  one, limit 8. A block average hides it, which is why every other claim on that
  scene cannot see it and why the review PNG was the only evidence before.
- **The blur reduced cross-driver divergence where it mattered.** On the 192
  pixels of `lantern`'s room the blur changed, llvmpipe and radv disagree by at
  most **8** and 27 are over `Tolerance::RASTERISER`; the unfiltered march's
  worst inside the panel's band was 66. The pixels over tolerance that remain
  gross (worst 134) are **bit-identical to the pre-blur frame** on llvmpipe, so
  they are triangle-edge divergence and not the reflection's. A sixteen-tap
  denominator turning one whole-pixel disagreement into a spread of small ones
  is exactly what the AO pair's design predicts.

### lantern's mirror panel is close to SSR's worst case

Worked out by hand before the slice and **measured** by it since. The panel
faces `+Z` at the camera, so its rays point back past the viewer and its centre
reflects a point on the front wall behind the camera — off screen, a miss. Only
where the panel point is below eye height do rays go downward, and the band that
reaches the floor _while still inside the frame_ is narrower than the hand
estimate: `y = 0.45` up to about `0.607`, an eighth of the face rather than two
thirds, because the vertical frustum edge binds before the geometry does.
`room.rs`'s `the_mirror_panel_reflects_at_its_foot_and_not_at_its_head` bisects
for that height rather than writing it down.

So the observable is a block hung on the panel's **bottom edge** against one
further up the same face — same material row, same normal, same `F0`, same
roughness, same absence of direct light, differing only in whether the ray finds
anything. It reads about 22/255 against exactly 0 at 256×192 and 18 against 0 at
1280×960.

**`METAL_DARKNESS` is gone, not kept** — this entry said it survived at
`MIRROR_MISSES` with its number unmoved, and that was wrong on every count;
`71ef3e2 feat(render): fill SSR misses from probes` deleted it, and it resolves
nowhere in the tree (checked 2026-08-23). What stands at that control point is
`MIRROR_FRACTION_OF_PLASTER` in `apps/lantern/tests/golden.rs`, which asserts
the opposite sense — a floor on the brightness a miss retains, rather than a
ratio by which it must be darker, because a miss is filled from the probes now
instead of going black. `MIRROR_GRADIENT` is the central claim beside it, and
`reflecting > LIT_FLOOR` is the floor that stops a ratio against zero from being
a check that cannot fail.

**Considered, and for the sample's owner rather than the SSR slice:** if lantern
wants a mirror showing the room, the panel wants angling or moving to a side
wall. That is a change to the sample's content and should not be done on the way
past.

### The cutoff raise: what it costs, measured before it was declined

The blur slice was written with `ROUGHNESS_CUTOFF` at **0.75** first, run
end-to-end, and then split back out — so the cost below is measured on this tree
rather than estimated, and slice 1 above starts from an answer instead of a
guess.

**Two questions to settle before writing it.**

- **Is 0.75 the smallest cutoff that gets `ROUGH_METAL` reflecting?** It was not
  derived; it was placed between `lantern`'s brass at 0.55 and its plaster at
  0.9. The ramp is `1 - roughness/cutoff`, so brass weighs 0.083 at a cutoff of
  0.6, 0.214 at 0.7 and 0.267 at 0.75 — and the falloff comparison needs the
  brass reflection to clear `LIT_FLOOR` and to turn that face's own downward
  gradient around, which at 0.083 it may not. **Unmeasured below 0.75**, and
  worth measuring: a lower cutoff buys nothing in blast radius (any value over
  0.5 takes `UNTINTED` in) but it does keep more of the frame's arithmetic near
  zero.
- **Does any cutoff over 0.5 cost the determinism claim?** Yes, and it should go
  on the record as a decision rather than arrive as a side effect.
  `GpuMaterial::UNTINTED`'s roughness is exactly 0.5, no monotone ramp passes
  0.55 and stops at 0.5, and the design's one _unconditional_ determinism
  statement is that a pixel shaded through that row weighs exactly zero on four
  rasterisers. Raising the cutoff at all trades that for "the rough end —
  plaster, a fully rough conductor, `crcbl_scene`'s imported glTF default —
  weighs exactly zero", which is a real claim and a narrower one.

**What it moved, at 0.75, on llvmpipe.** Nine goldens: `ao` 41.3% of its pixels
at delta 3, `dunes` 6.6% at 9, `spot_shadow` 5.5% at 16, `cube` 0.07% at 1,
`lights` 0.04% at 1, `crcbl-vk/mesh_clusters` 12% at 5 — all of those purely
because `UNTINTED` entered the ramp — plus `ssr` and `point_shadow` moving
further than the blur alone moved them (0.25 weighs 0.667 where it weighed 0.5)
and `lantern/room` gaining the brass block's reflection. `ui`, `sprite`, `spot`
and every sprite and UI golden in `crcbl-vk` stayed byte-identical.

**What it broke.** Two byte-exact geometry-path comparisons: `Scene::Ao` in
`render_e2e` (four channels, off by one, llvmpipe only, stable over three runs)
and `crcbl-vk`'s open box in
`a_multi_cluster_mesh_draws_the_same_frame_through_both_geometry_paths` (one
channel, off by one, llvmpipe only). Both for `Scene::PointShadow`'s recorded
reason — a marching pass makes the depth buffer's last bits visible in the
picture, and the two paths compute the same world position through different
arithmetic. **If that slice re-adds a budget to `crcbl-vk`, it should be the
measured value per comparison and not `render_e2e`'s 16**: that constant is
per-scene there and zero everywhere it holds, and handing a second suite a
blanket sixteen is slack nobody measured.

**What the observable would be.** A sixth claim in
`apps/lantern/tests/golden.rs`, in the shape of `render_e2e`'s
`the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one`: two blocks up
each conductor's face, hung off its bottom edge five half-extents apart, and the
brass block's falloff asserted above one while the panel's is not. It reads
1.211 at 256×192 and 1.156 at 1280×960 with the cutoff at 0.75, against 1.007
and 0.897 with no reflection on that row — so a threshold near 1.08 has about a
fifteenth of margin either side. **The block's own shading runs the other way**,
which is what makes the measurement a claim about the reflection: the sun
reaches that face at a glancing angle and it darkens towards the floor, so a
build with no reflection on it reads _under_ one. It needs `BLOCK_FOOT` in
`room.rs` — the middle of the block's bottom edge — and a no-GPU bisection
beside `the_mirror_panel_reflects_at_its_foot_and_not_at_its_head` showing that
the block reflects across most of its face where the panel reflects across an
eighth of its own.

### One shared-code hazard

`ssr.slang` re-declares `depth_at`, `view_position` and `normal_at` verbatim and
`ssr_blur.slang` re-declares `depth_at` and `view_z`, because this repo has no
include mechanism by design — the manifest hashes one source per artifact.
`crcbl_shaders::ssr`'s `the_shared_screen_space_helpers_have_not_drifted`
compares the bodies as text and holds all of them: four copies of `depth_at`,
two each of `view_z`, `view_position` and `normal_at`. (The plan said three
copies of `normal_at`; `ssao_blur.slang` carries `depth_at` and a `view_z` cut
down from `view_position`, not a normal.)

Two shader **constants** are copied as well, and each has a guard beside that
one: `DEPTH_FAR`, which every screen-space source declares and
`the_far_plane_matches_the_constant_the_reflection_pair_declares` checks against
`crcbl_shaders::ssao::DEPTH_FAR`; and `THICKNESS_FLOOR`, which the march and its
blur both declare and `the_thickness_floor_matches_the_one_the_march_declares`
holds together. The blur has no ray, so that floor is the only length the march
owns which it can still evaluate — which is why it is a copy rather than a
uniform field.

Making that an equality rather than a substitution cost one rename: all three
files bind the projection block as `camera` rather than as `ssao`, because
`view_position`'s body names it. No compiled instruction moved and no golden
did.

## Left over from packing draw args into eight storage buffers

Record; the `crcbl-dx12` register case list is in `docs/backlog.md` under this
heading.

- **Settled — `ssr` and `ui` are excused by name on SwiftShader, and widening
  the tolerance was declined.** The choice this entry posed was between widening
  `Tolerance::RASTERISER` for those scenes and recording them as known
  software-rasteriser differences. The second was taken: the `render-harness`
  job's Linux and Windows legs pass `--expect-fail ssr,ui`, both scenes still
  render and still print their numbers every run, and
  `web/tools/render-harness-verdict.mjs` fails the job the moment either starts
  matching or anything else stops. Widening instead would have quietly weakened
  all eleven scenes to excuse two. The macOS leg carries **no** excuse list and
  gates all eleven, which is what says these are a rasteriser limit rather than
  a backend defect. The reasoning is in `.github/workflows/pages.yml` above the
  `render-harness:` job, which is where someone re-opening it will be standing.

## Hardware line width is one pixel, and the layer takes that (2026-08-31)

**Considered and taken deliberately**, against the earlier prediction that
`crcbl_ui::draw_list`'s triangle expansion would be lifted and shared. The
pipeline is `PrimitiveTopology::LineList` with no line-width state: one
entry-point pair over a storage buffer and no CPU-side expansion, so a box is
twelve `line()` calls and 24 vertices rather than twelve quads and 144. All four
backends already map `LineList`, so nothing at the seam changed.

**What it costs, plainly.** No width control, no dashed or thick lines, and the
exact texels a diagonal covers differ between rasterisers — D3D12's diamond-exit
rule and Vulkan's parallelogram rule disagree at a line's last pixel.
`crates/crcbl/tests/mesh_e2e/debug_draw.rs` is written around that: the sharp
per-texel assertion is on a segment that is horizontal in screen space, and the
twelve-edge and six-face assertions allow one texel of slack and say why.

**When to revisit.** If a caller wants width, the expansion is a vertex-stage
change — two triangles per segment, expanded in NDC by a pixel width from a
constant — not a new pass. `push_stroke`'s bevel logic is a screen-space helper
that does not transfer to a world-space segment whose two ends have different
depths, so "lift `push_stroke`" is probably still not the answer.

## CMAA2's append lists made a dense frame a function of the schedule (2026-09-06)

The tier first shipped with five passes and two lists: `cmaa2-edges` appended
every edge pixel to a candidate list, `cmaa2-shapes` ran once per candidate and
appended each coverage share as a blend item, and `cmaa2-accumulate` ran once
per item. Each list was an atomic counter into a buffer sized as a fraction of
the frame (`CANDIDATE_DIVISOR` 8, `ITEM_DIVISOR` 4) and each **dropped** what
arrived past its capacity, with every count read back clamped so nothing was
ever written outside a list. Which entries won the room was the device's
scheduling of the work-groups that produced them, so the tier's determinism held
only while a frame's edges fitted. Reproduced twice: with CMAA2 temporarily in
`RenderEffects::DEFAULT_STACK`, four of six runs of
`the_specular_aa_scene_draws_the_same_frame_on_every_geometry_path` differed
between the mesh-shader and indirect-count paths (240, 186, 42 and 264 channels,
worst by 77); and `a_dense_edge_frame_resolves_to_the_same_bytes_every_time` — a
grid of small spun cubes covering a 256×192 frame — moved up to 2411 of its
pixels between runs of one frame on radv, on every one of three attempts.

Both lists left. The classification became one invocation per pixel testing the
predicate the append used to test, and every share now goes into the fixed-point
accumulation directly as four integer atomic adds. Both buffers the tier has
left hold one entry per pixel, so there is no capacity to exceed. The sabotage
that shows the observer's teeth on a different axis: zero the weight word's add
in `cmaa2_shapes.slang`'s `accumulate` and
`cmaa2_changes_a_band_along_the_edges_and_nothing_else` goes red on its "moved
pixels" claim.

**Dropping the two list passes cost nothing on radv and saved time on
lavapipe.** Against the five-pass build at `15d6bca`, measured by the
antialiasing cost protocol (_What the deleted 49-antialiasing plan left behind_)
in the same session: on radv the resolve slot goes **0.095 → 0.093 ms**
(`cmaa2-clear` 0.001, `cmaa2-edges` 0.043, `cmaa2-shapes` 0.026,
`cmaa2-accumulate` 0.002, `cmaa2-apply` 0.023 before) and the whole frame
**1.562 → 1.571 ms**, which is inside the run-to-run spread either way; on
lavapipe the slot goes **2.004 → 1.744 ms** (0.101, 0.112, 0.108, 0.108, 1.575
before) and the whole frame **102.307 → 101.344 ms**. The saving is the two
dispatches themselves: each cost about a tenth of a millisecond on lavapipe
whatever it was asked to do, which is that driver's fixed cost per dispatch, and
the work they were doing did not go away — it moved into `cmaa2-shapes`, whose
own row is unchanged.

## The CMAA2 default flip: what moved and where it was blessed (2026-09-06)

`RenderEffects::DEFAULT_STACK` took `CMAA2` in place of `ANTIALIASING`.
Twenty-nine goldens moved: seventeen under `crates/crcbl/tests/golden/`, four
under `apps/sundial/tests/golden/`, three each under `apps/alcove/tests/golden/`
and `apps/quarry/tests/golden/`, and two under `apps/lantern/tests/golden/`. The
`crcbl`, `lantern` and `quarry` sets were blessed on radv and the `alcove` and
`sundial` sets on lavapipe — each where its own previous bless was — and every
harness suite was then run green on both drivers. The largest moves the suites
reported: `aa` 83.83% of pixels differ at 0.78% gross, `cube_97x61` 74.41% at
5.73% gross, lantern's `room` 31.77% at 0.75% gross, sundial's plaza set 8.96%
at 2.44% gross, alcove's court 4.10% at 0.77% gross, quarry's dolly-start trio
about 6.7% at 0.42% gross with dolly-end unmoved.

Rather less of the tree moved than the bit count suggested. A dozen fixtures
across `crates/crcbl/tests/mesh_e2e/`, `gltf_e2e.rs` and `apps/viewer` had asked
for no resolve by forcing `RenderEffects::ANTIALIASING` off, which under the new
default left CMAA2 running and would have moved every one of their goldens;
`Antialiasing::SLOT` went public for them to name instead. Three checks had
become comparisons of the default against itself and were repointed at `fxaa`,
the rung the default no longer carries:
`the_players_antialiasing_tier_replaces_the_resolve_slot`,
`the_players_video_clamp_reaches_the_frames` and
`reset_takes_the_antialiasing_tier_back_to_the_games_own_rung`; and
`the_resolve_is_what_puts_the_soft_pixels_there` was comparing a frame with
itself and now clears both slot bits for its control. No non-golden threshold
moved.

Browser, on SwiftShader at load 9.3: the render-harness drive took 74.5 s of its
180 s budget (three runs, 74.0/74.5/75.7 s) and lantern's first HUD line came at
11.4 s against a 180 s cap. The earlier CMAA2-only measurement on `e665d61` read
45.8 s and 8.8 s; the tree moved between the two, so the delta is not the flip's
alone without an A/B nobody ran. The lantern demo gate's first run reddened on
`web/tools/browser-e2e.mjs`'s pinned effects row, not on load, and the row moved
to `shadows ao ssr vfog cmaa2`.

## The ACES default flip: what moved and where it was blessed (2026-09-06)

`ForwardRenderer::new` took `TonemapCurve::Aces` in place of
`TonemapCurve::Clamp`. **The seam is the renderer's own default, and no
`CameraStack` field was added**: the 2D samples do not build a `ForwardRenderer`
at all. `apps/asteroids`, `apps/breakout`, `apps/flappy`, `apps/horde` and
`apps/hud` name the type only for the associated helper
`ForwardRenderer::present_target`, and draw through `crcbl_render::sprite_pass`
and `crcbl_render::ui_pass`; the sprite and UI scenes in
`crcbl::screenshot::Scene` build their own `SpriteRenderer` and `UiRenderer`. So
"3D defaults to the fit, 2D keeps the clamp" is true of the renderer's own
default with nothing else said, and the five 2D golden suites came back green at
0.0000% before anything was blessed.

Thirty-nine goldens moved: twenty-four under `crates/crcbl/tests/golden/`, six
under `apps/quarry/tests/golden/`, four under `apps/sundial/tests/golden/`,
three under `apps/alcove/tests/golden/` and two under
`apps/lantern/tests/golden/`. The `crcbl`, `lantern` and `quarry` sets were
blessed on radv and the `alcove` and `sundial` sets on lavapipe — each where its
own previous bless was — and every harness script under `crates/crcbl/tests/`,
`apps/*/tests/` and `crates/crcbl-vk/tests/` was then run green on both drivers.
Every golden that moved moved on 100.00% of its pixels, the curve being a remap
of every channel; what separates them is the magnitude. On radv the largest
maximum channel deltas were `cube` 155, `dunes` 106, `point_shadow` and
`spot_shadow` 80, `lights` and quarry's dolly-end trio 77; the smallest were
`gradient_mirror` 30 and `area_light`, `atmosphere_mirror` and `ao` 34. Mean
absolute error sat between 15.4 (`bloom`) and 23.7 (`ao`) across the whole set,
and the share "grossly wrong" ran from `bloom`'s 8.63% and `ssr`'s 50.76% up to
100% on `ao`, `gradient_mirror` and `probes`. lavapipe reported the same maximum
delta on every `crcbl` golden and agreed on the rest to within a few tenths of a
point; the one figure that parted company is quarry's `mesh-shader-dolly-start`,
71 on radv against 53 on lavapipe, which is the two drivers disagreeing about
that frame rather than about the operator.

## The CMAA2 default cost one cross-path guard a budget (2026-09-06)

`crates/crcbl/tests/render_e2e.rs`'s `path_lsb_channels` gained a
`Scene::AlphaMask` row at 16 channels and one level, where that scene was held
to zero. `the_alpha_mask_scene_draws_the_same_frame_on_every_geometry_path`
reddened on CI run 34018142030, job "vk e2e (lavapipe)", on `6377ed1` — the
commit that moved `RenderEffects::DEFAULT_STACK`'s resolve slot onto CMAA2 —
with one channel differing by one out of the frame's 196608: the red of
`(90, 130)`, `59` against `60`. That runner is Ubuntu's llvmpipe, Mesa
25.2.8-0ubuntu0.24.04.2 / LLVM 20.1.2.

**The mechanism is a hypothesis and is recorded as one.** CMAA2 classifies an
edge by comparing lumas against a threshold, a discontinuous decision where
FXAA's blend was a smooth one, so a sub-level luma difference between the
mesh-shader and indirect-count arms can flip one pixel's classification where it
previously washed out. Nothing in this workspace can test that: Arch's Mesa
26.2.2 / LLVM 22.1.8 answers **zero** channels on the same comparison and so
does radv, before and after the ACES default flip — the whole render e2e suite
was run on both drivers at each state. The difference does not exist on either
driver here, with the resolve on, so there is no A/B to run against it.

The ACES default does not move this scene further:
`alpha_mask on MeshShader against IndirectCount` reports
`0 channel(s) differ, worst by 0` on Arch lavapipe and on radv with the fit in
force. If the runner's disagreement ever grows past one level the row's second
figure is what would have to move, and that is a measurement nobody here can
take.

## The ACES default cost one cross-path guard a budget (2026-09-06)

`crates/crcbl/tests/render_e2e.rs`'s `path_lsb_channels` gained a
`Scene::DoubleSided` row at 16 channels and one level, where that scene was held
to zero. Its cause is the tonemap operator, which makes it the only row there
that is about no pass at all. Exposure-and-clamp has unit slope on `0..=1`, so
an HDR difference in the last place between the mesh-shader and indirect-count
arms rounded to the same eight-bit level; the fit's toe is steeper than one, so
the same difference can land two levels apart in the encode. Measured at one
channel, off by one, out of the frame's 196608 — the red of `(93, 159)`, `83`
against `82` — over three consecutive runs on Arch's lavapipe, with radv
answering zero on the same comparison. The frames' own difference is unchanged;
what changed is whether the encode can still see it.

## Where the clamp is pinned, and why it is not a threshold move (2026-09-06)

A fixture that predicts a **code value** from a host model of the shading is not
a test of the tonemap operator, so it asks for the clamp on the renderer it
drives rather than having its budget re-derived. Three shapes of pin, one rule:

- `render_e2e.rs`'s `scene_referred` maps a `ForwardScene` on the way into
  `OffscreenSetup::open_forward`, and eight fixtures use it — the aa resolve's
  soft-pixel control, the probe clipmap, scroll, slab and sealed-cell scenes,
  the atmosphere LUT frame and the two sky-ambient rows.
- `crcbl::screenshot::OffscreenSetup::set_tonemap_curve` is new, for a built-in
  `Scene` whose renderer a fixture never sees. It returns whether it reached a
  renderer, because the sprite and UI scenes run no tonemap pass and a silent
  no-op is exactly the shape of a pin that never arrived.
- `Arm::scene_referred` in `apps/alcove`, `apps/sundial` and `apps/lantern`'s
  golden suites, which is a field on the arm each fixture already describes
  itself with.

The scenes that both bless a golden and make such a claim draw **two** frames:
the golden is what the engine draws by default, and the claim reads a second
frame under the clamp. `render_e2e.rs`'s `ClaimFrame` is that choice for
`Scene::Probes`, `Scene::SpecularAa` and `Scene::GradientMirror`;
`apps/lantern`'s `draw_scene_referred` is it for the room's fixed-camera golden,
its below-the-device path pair and its presentation-size claims; `apps/alcove`'s
presentation-size test redraws its four crease arms. `apps/sundial`'s speckle
counts and `apps/lantern`'s effect-toggle frames bless nothing, so those draw
one frame under the clamp and the saved review pictures are scene-referred with
them.

Found by the parent's own workspace run with `CRCBL_GPU=vk`, not by the slice's:
`apps/viewer/src/gpu.rs`'s
`the_exposure_scales_the_frame_the_way_an_exposure_does` and
`the_normals_view_paints_each_face_the_encoding_of_its_world_normal` skip unless
a driver is pinned, and CI's "Run the viewer's suite against lavapipe" step pins
one. Under the fit the first read a clear ratio of 14.47 where the exposures
differ by 4, and the second read a shaded background of `[29, 34, 48]` against
the normals view's `[5, 8, 17]`, because a debug view resolves to the clamp
whatever the renderer starts on. Both fixtures pin the clamp beside
`without_the_resolve`; both suites are green on radv and lavapipe.

## `apps/sundial` took the atmosphere (2026-09-06)

`docs/backlog.md`'s "What the atmosphere shipped without" scheduled the demo
switch; this is what it moved. `crcbl_sundial::sun::Sky::atmosphere` is the new
value — `sun_direction` and `sun_illuminance` are read off `Sky::light`, so
there is one spelling of the sun — and `Gpu::frame` and the golden suite's
`build` are its two callers. The plaza had no sky before this at all: it never
called `set_sky`, so its background was the scene target's clear colour and the
whole of its ambient was `DirectionalLight::ambient`.

**The guard was shown red first**, on radv, with `set_atmosphere` commented out
of `build`: all twenty-seven readings failed and the first was
`band (128, 7), red measures 29.00 and the host LUT predicts 59.20, a miss of 30.20 level(s) against a budget of 0.9`.
`(29, 34, 48)` is the clear colour under the clamp and it read the same at all
nine bands, which the spread clause would also have caught.

**The sweep behind `SKY_LEVELS`**, over nine bands and three channels on a
scene-referred `Arm::shipped()` frame: the worst miss is 0.06 levels on radv and
0.13 on lavapipe. `SKY_LEVELS` is `crcbl/tests/render_e2e.rs`'s own
`ATMOSPHERE_MIRROR_LEVELS`, 0.9, taken rather than invented. The two
anti-vacuity clauses measured 35.16 and 35.06 levels of spread across the nine
bands, and 17.04 and 17.02 levels of movement at the horizon band between
`FIXTURE_TICK` and `GRAZING_TICK`.

**Five goldens moved and one did not.** Against the old references, on radv:
`plaza-grazing` a mean absolute error of 18.36 with a structural similarity of
0.781, `plaza-cascades` 19.74 at 0.852, and `plaza-pcss`, `plaza-disc` and
`plaza-box` 20.80, 20.82 and 20.84 at 0.762, 0.761 and 0.761 — every one of them
100% of pixels differing. `plaza-atlas` is byte-identical: the shadow-atlas
viewer replaces the picture. Re-blessed on lavapipe, where sundial's set has
always been blessed, and green afterwards on both adapters.

**What the lighting quantities did**, before and after, radv then lavapipe.
Every assertion held on both; none was re-thresholded.

| reading                                     | before          | after           |
| ------------------------------------------- | --------------- | --------------- |
| the plinth contact's darkening              | 0.7088 / 0.7076 | 0.4943 / 0.4938 |
| open pavement, no shadow term, ACES         | 184.08 / 184.02 | 192.39 / 192.31 |
| the acne block at `GRAZING_TICK`, clamp     | 185.29 / 185.15 | 196.68 / 196.55 |
| the acne block at `NOON_TICK`, clamp        | 252.46 / 252.46 | 255.00 / 255.00 |
| speckle at either tick                      | 0.0000%         | 0.0000%         |
| the bias pair's contact, fixed pose         | 70.73 / 70.44   | 58.08 / 57.88   |
| the bias pair's contact, pavement pose      | 69.84 / 69.59   | 57.63 / 57.47   |
| `OPEN_PAVEMENT` at `NOON_TICK`, clamp, radv | 252.67          | 255.00          |

The PCSS ladder is unchanged on radv — penumbrae 0.0400, 0.0560 and 0.1000 m,
tallest over lowest 2.500 — and moved by one measurement step on lavapipe, to
0.1040 m and 2.600. The disc ladder is 0.0400, 0.0440, 0.0400 and 1.000 on both,
before and after. The cascade overlay's two readings led red over blue by 79.9
and −74.1 before and 75.0 and −87.4 after, over surfaces that went from 170.59
and 184.01 to 181.28 and 192.33.

The last row of that table is the one that is a problem rather than a
measurement, and `docs/backlog.md` carries it: `INTENSITY` exists to keep that
pavement clear of the top of the range under the clamp, and it no longer is.
**Zeroing `AMBIENT` was tried and does not recover it** — the block still reads
255.00 with the flat term gone, so the sky's ambient alone is over the top.

**The seam comparison had to change, and the sweep says why.** Its claim is that
every column outside a bleed band is byte-identical to its own whole-frame
reference. Measured at `CLAIM_EXTENT` as the furthest column from the seam that
differs, on the `box` rung:

| arm                                 | radv | lavapipe |
| ----------------------------------- | ---- | -------- |
| shipped stack, no atmosphere        | 25   | 15       |
| shipped stack, under the atmosphere | 0    | 33       |
| `REFLECTIONS` cleared, under it     | 33   | 33       |
| `CMAA2` cleared, under it           | none | none     |

So the antialiasing resolve is the whole of it, which is what `SEAM_BLEED`
always said — but its 32 columns were never a bound: `cmaa2_shapes.slang` walks
a run of like boundaries for up to `MAX_LINE_LENGTH` texels, and how far it
actually reaches depends on the contrast of the edges it finds. A sky bright
enough took it past the band. **Widening the band is not available**: `disc` and
the shipped filter draw a different left half only within 43 columns of the seam
from this pose, so a band sized for the resolve leaves that half's exactness
separating nothing — measured, at a band of 64 the anti-vacuity assertion fires.
The arm now clears `CMAA2` and `SEAM_BLEED` is one column, the one the split
lands on; 1023 of 1024 columns are then exact on both adapters, and the two
halves stand 9.234 and 228.562 apart on `disc` and 27.387 and 258.306 on `box`
(radv; lavapipe answers 9.234/228.438 and 27.438/258.201). Both of that test's
recorded sabotages were re-run on radv under the new arm and still fire — the
reference swap at column 469, and the shipped filter on both sides through the
anti-vacuity clause.

## The sun disc: the fit, what clamped, what moved (2026-09-06)

`docs/backlog.md`'s "What the atmosphere shipped without" decided the disc on
2026-09-06 and this is what building it found. `sky.slang`'s `sun_disc` and
`crcbl_shaders::atmosphere::SkyView::disc_radiance` are the two spellings.

**The limb-darkening fit.** Hillaire 2020's `SunLimbDarkening` is
`1 - u(1 - mu^a)` with `u = [1, 1, 1]`, so the factor collapses to `mu^a` with
`a = [0.397, 0.503, 0.652]`. The shading rule (_What the deleted 44-lighting
plan left behind_) lets no transcendental reach a colour, so the `pow` had to
go. A polynomial in `mu` is hopeless — the function has an infinite derivative
at `mu = 0`, and the best degree-seven polynomial in `mu` still misses
`mu^0.397` by 3.8e-2. The substitution is what fixes it: under `t = mu^{1/2^k}`
the target becomes `t^{2^k a}`, and each square root is IEEE-exact, so the
substitution costs nothing in determinism. The sweep, Lawson-weighted least
squares over a grid uniform in `t`, worst channel:

| `k` (square roots) | degree 4 | degree 5 | degree 6 | degree 7 |
| ------------------ | -------- | -------- | -------- | -------- |
| 1                  | 5.6e-3   | 3.9e-3   | 3.0e-3   | 2.3e-3   |
| 2                  | 9.1e-4   | 4.5e-4   | 2.5e-4   | 1.5e-4   |
| 3                  | 9.6e-5   | 5.1e-5   | 5.8e-6   | 2.1e-6   |
| 4                  | 3.6e-2   | 1.1e-2   | 2.8e-3   | 5.1e-4   |

`k = 3` (`t = mu^{1/8}`, four square roots from `mu²`) at degree six is what
ships, as `SUN_LIMB_FIT`. Measured in the `f32` arithmetic that actually runs,
against `mu.powf(a)` in `f64` over two hundred thousand nodes uniform in `t`:
**6.033e-6 on red, 6.999e-7 on green, 3.682e-6 on blue.** `k = 4` falls apart
again because the exponent `16a` climbs past what a low-degree polynomial holds
on `[0, 1]`. `the_limb_fit_tracks_the_papers_curve` is the standing statement.

**The normalisation is a departure from the paper, on purpose.** Hillaire
multiplies the limb factor into the sun's radiance as authored, so his disc
emits `2/(a + 2)` of the light it stands for — 0.834, 0.799, 0.754 of it. Limb
darkening redistributes energy across the disc rather than removing it, and this
engine has a `DirectionalLight` beside the disc that the forward pass shades
with, so the factor is divided by its own mean and the shape is the paper's
while the total is the light's. That mean is closed form and exactly so: the
disc's `dω` is `2π dc` in the ray's cosine and this module's radial coordinate
`r²` is linear in `1 - c`, so `dω = Ω·2r dr` with no small-angle approximation,
and `∫₀¹ 2r (1-r²)^{a/2} dr = 2/(a + 2)`.
`the_disc_integrates_back_to_the_suns_illuminance` integrates the shipped
function over the cap and recovers the illuminance to **2.035e-6** relative.

**Two cosines are compared as versines.** `1 - cos(0.2665°)` is 1.0817e-5, and
an `f32` cosine of 0.999_989 has already thrown away the digits that difference
is made of — computing the solid angle from an `f32` cosine reads `1/Ω` as 14752
where it is 14713, a quarter of a per cent. So `SUN_ANGULAR_RADIUS_VERSINE` is
stored as the versine and never as the cosine, and a ray's own angle from the
sun arrives the same way: `½|d − s|²`, which is `1 − d·s` formed where the two
vectors differ rather than where they agree. There is no `acos` anywhere in the
disc.

**What clamps.** Nothing, in anything this tree draws. The disc is the
illuminance over `SUN_SOLID_ANGLE` and the limb mean — 17634 times it on red for
a sun above the air — and `apps/sundial` is the brightest atmosphere on a
golden: its disc reads **21225/17641/12879** at `FIXTURE_TICK` and
**17557/11418/5502** at `GRAZING_TICK`, against `MAX_RADIANCE`'s 65504. The
clamp is there for a scene that scales its sun, and
`the_sky_over_the_plaza_is_the_host_lut` prints and asserts both figures rather
than leaving the question to a comment.

**What moved.** One golden in the workspace: `apps/sundial`'s `plaza-grazing`,
**eight pixels**, blessed on lavapipe. At the bottom of the sun's arc the
plaza's camera has the sun 10° up and 22° across, which is inside a 60° lens
looking 13.6° down — two saturated pixels at (200, 20) and (200, 21) and six of
CMAA2's own skirt around them, `max channel delta 197`. Every other golden in
the tree is byte-identical, including `atmosphere_mirror`: `ssr.slang` reads
`SkyView::radiance` and not the disc, the block's new row is zeroes on a
gradient frame, and `min(x, 65504)` is exact for every value either sky
produces.

**A two-degree lens is what made the disc testable.** At an ordinary field of
view the sun is one or two pixels — `plaza-grazing` shows two of them — so a
fixture there could only ask whether some pixels are bright.
`crcbl::screenshot::sun_disc_forward` frames `ATMOSPHERE_SUNS[2]` through
`SUN_DISC_FOV_Y` of two degrees, where the disc is about a quarter of the
frame's height and its profile is picture. Its illuminance is 6.5e-5 rather than
one, for the same reason: at one, every pixel of the disc is four orders of
magnitude past the top of an eight-bit frame and the fixture measures a white
circle, which is true of any disc of any profile. At 6.5e-5 the centre reads
233.00/194.75/143.00 on radv and the limb is inside the range, so the frame
carries Hillaire's curve and the transmittance's reddening both.

## Aerial perspective, drafted and not built (2026-09-07)

The decision's fork — where the third LUT is marched — is the backlog's **OPEN
2026-09-07** paragraph under "What the atmosphere shipped without". This is the
rest of the draft, common to both answers, so the slice starts from it rather
than from the paper.

**What is stored.** Hillaire's `float4(in-scatter, mean transmittance)`, with
the transmittance itself in `a` rather than `1 - T`, because
`volumetric_composite.slang` already composes `scene * a + rgb` and the two
volumes should be one arithmetic. The scalar alpha loses chromatic extinction —
the reddening of a far surface under a low sun — which is Hillaire's own
compromise and is worth saying rather than inheriting; the chromatic in-scatter
carries the blue haze, which dominates at scene scale.

**Units.** The engine's unit is the metre — `apps/sundial`'s plaza is 24 m
across, `crcbl_render::camera` talks in hundreds of metres — and every
coefficient in `crcbl_shaders::atmosphere` is per kilometre, with
`Atmosphere::altitude_km` already flagged as the one field not in engine units.
So `crcbl_render::Atmosphere` and its shader mirror gain a `km_per_unit`
(default 0.001; a public field, so a `Breaking` entry), and every distance the
march takes is scaled by it. The consequence decides the fixture: over the
plaza's 25 m the Rayleigh optical depth in blue is 0.0331/km × 0.025 km ≈ 8e-4,
under a level of an eight-bit encode, so **no `plaza-*` golden should move** — a
moved one is the term applied at the wrong scale, to be investigated before any
bless. `AERIAL_MAX_DISTANCE` defaults to `CLUSTER_FAR`, the far end the local
column already stops at, so the two volumes end at one number. Slices are linear
in distance — Hillaire's, unlike the local column's exponential split — because
extinction is near-constant over a kilometre at an 8 km scale height; inclusive
at the slice centre with a trilinear read; a surface past the last slice takes
the last slice's value and is under-fogged there.

**The composite.** A `StructuredBuffer<float4> aerial` appended past `lighting`
(never inserted, on `sky_pass`'s Metal ordering rule); `fog_params.w`, the pad
lane `volumetric.rs` writes as zero, becomes the local column's switch; a
`float4 aerial_params` is appended last in the block with the max distance in
`x` and the aerial switch in `w`, so a frame with no atmosphere composes the
bytes it composed before. Order: the air first, then the fog over it —
`((scene * T_A + S_A) * T_F) + S_F` — because the fog is the near medium and its
glow must not be attenuated by kilometres of air behind it. The aerial term is
skipped where the depth is still the clear value: `sky.slang`'s
`atmosphere_radiance` already integrates the whole ray through the sky-view LUT,
and charging its first kilometre twice is the double-fog this excludes. Three
asymmetries to write beside it: the local fog still fogs the sky and the aerial
does not, on purpose; `mesh.slang`'s analytic fog stays keyed off
`VOLUMETRIC_FOG` alone, so on an atmosphere-only frame the composite's local
half is the identity; and the composite now runs on every atmosphere frame
(`forward.rs`'s `scene-fogged` gate becomes "fog effect or atmosphere") with the
scatter and integrate dispatches still gated on the effect, which splits
`Volumetric::PASSES` and moves the full-screen pass count.

**The test.** Nothing in the tree draws an atmosphere and `VOLUMETRIC_FOG` in
one frame, so the seam has no coverage. A `render_e2e` fixture on the
`AtmosphereMirror` plate's arrangement with a Lambertian material, a low level
eye, every effect refused, and `km_per_unit` turned up so a 100-unit plate spans
tens of kilometres — a real field, said loudly, and what makes the assertion a
prediction against the host LUT rather than "looks hazier". Five assertions: the
far band gains at least a measured `MIN_AERIAL_LEVELS` over the no-atmosphere
arm; the near band moves less than a tenth of that (a uniform or depth-blind
term passes the first and fails this); far light-minus-dark contrast falls
(in-scatter without transmittance passes both and fails this); a sky band is
byte-identical with and without the composite (the double-fog guard); and a
`Cube` frame with the fog effect and no atmosphere is byte-identical before and
after (the off position). Slice and angular counts are swept before they are
fixed, on `the_march_has_converged_at_the_shipped_step_count`'s model.

**Pricing** is plan 43's protocol:
`apps/lantern --headless --frames 400 --size 1920x1080`, medians of three p50s
off `PassStats`, radv and lavapipe, with a temporary `set_atmosphere` in lantern
as the sky was priced; the composite before and after, the whole frame with and
without, the host build and one stripe if the host answer is taken. The browser
has no per-pass budget; what moves is the per-demo slowdown table in
`docs/notes/browser.md`.

**Unverified in the draft:** Unreal's composition order and Hillaire's alpha
convention are from memory; every cost is an estimate; the golden predictions
are arithmetic, not runs; whether the graph can import an image (it imports
buffers) is unread; per-backend `Rgba16Float` storage-image support is unread;
Slang has never lowered an `RWTexture` in this tree; the striped march's lag
behind a moving sun is unmeasured and the host answer doubles it.

## CMAA2 blended the wrong side of every edge for a day (2026-09-07)

The user reported the web demos drawing with no antialiasing. They were right,
and it was not the browser: `apps/sundial` drawn headless on radv at the demo
canvas's 959×463 was stair-stepped too, and so were the goldens themselves — the
`plaza-*` set the CMAA2 default flip re-blessed on 2026-09-06 is a staircase
next to the FXAA-era set it replaced, and `crates/crcbl/tests/golden/aa.png` had
lost its ramp in the same commit. Twenty-nine goldens were blessed with a broken
filter and nothing in the tree said so.

**The mechanism.** `cmaa2_shapes.slang`'s `blend_line` integrates the
reconstructed boundary's height across each element of a run into two shares —
`below`, the area on the `-offset` side of the aliased boundary, and `above`,
the area on the other side — and then handed `below` to _this_ pixel and `above`
to the pixel across. A negative height means the reconstruction sits inside the
`-offset` pixel, so that pixel holds area that belongs to this side and is the
one that must take this side's colour; the code did the reverse. On a `Z` of
length `n` the shares ramp from about 0.44 at one end to 0.44 at the other, so
the wrong side received a strong blend exactly where the right side needed one:
the fully covered pixel at the top of the run was darkened, the uncovered pixel
at the bottom was brightened, and the step between them stayed. A Python
transliteration of the three shaders reproduced the GPU frame to within 0.6 of a
level and, with the two `accumulate` targets swapped, drew a clean ramp on a
synthetic 1:8 and 1:2 edge — which is the fix, and it is the whole fix.

**Why the guard did not fire.** `the_resolve_is_what_puts_the_soft_pixels_there`
counts pixels strictly between the frame's two levels and holds the count over
256; both the right blend and the wrong one touch the same silhouette pixels,
and both put exactly 532 there. A count of touched pixels cannot see which way
they moved. `the_resolve_moves_the_silhouette_toward_a_supersampled_reference`
is the assertion that would have gone red: the same scene at four times the
extent with no resolve, box-filtered down, is the coverage an ideal edge filter
reconstructs; over the truth's soft pixels and one pixel of ring around them,
the unresolved frame's mean luma error is the staircase's whole height and the
resolved frame must sit under `AA_MAX_RESIDUAL_SHARE` of it. Measured on radv:
the corrected CMAA2 0.747, the wrong-side blend 1.650, no resolve 1.000 by
construction. **The ring is load-bearing:** scored over the truth's soft pixels
alone the wrong-side blend measured 0.578 against the right one's 0.680 — it
spends its blend on the fully covered pixel beside the silhouette, which that
band did not look at — so a band the frames cannot move was the first thing the
test needed. `CRCBL_AA_DUMP=<dir>` writes the three frames the test compared,
which is where this investigation's numbers came from.

**What moved.** Every golden the flip re-blessed moves back to a resolved edge —
twenty-eight are re-blessed, and `probes.png` moves under
`Tolerance::RASTERISER` on both adapters — and the sets are re-blessed where
each is compared: `crcbl`, `lantern` and `quarry` on radv, `alcove` and
`sundial` on lavapipe, with the other adapter holding each within
`Tolerance::RASTERISER`. The browser gate compares the same files, so the demo
site draws the corrected filter as soon as the Pages run after this lands.
