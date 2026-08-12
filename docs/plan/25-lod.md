# Topic 25 — LOD System

Level-of-detail for meshes: **hand-authored LOD chains and automatic
simplification** (base high-quality mesh programmatically decimated), selected
**on the GPU** inside the existing culling pass — LOD with zero CPU cost, the
stage 3 discipline extended. Mechanism lands with the GPU-driven renderer (P7,
hand LODs usable immediately).

**QEM auto-generation is MVP, not wave 1** (moved 2026-08-09). Mesh shaders
became the primary geometry path in
[03-gpu-driven-rendering.md](03-gpu-driven-rendering.md) §3.5, and a
meshlet-clustered pipeline selects detail **per cluster** rather than per
instance — which needs a simplified cluster hierarchy to select between. A
cluster hierarchy with no generated levels is the culling win without the detail
win, i.e. most of the reason for the path. Hand-authored chains cannot fill that
role: an artist supplies whole-mesh levels, not per-cluster ones.

Two consequences that follow from the same move:

- **Selection is per cluster on the `MeshShader` path and per instance
  otherwise.** The error metric, the thresholds and the hysteresis are shared;
  only the granularity differs. See "Runtime selection" below.
- **The generator becomes a cluster-hierarchy builder**, not only a decimator:
  it emits the meshlet clusters, their bounds and normal cones, and the
  simplified parent levels — one bake step, one content hash, one cache entry.

## The cluster DAG (LOCKED 2026-08-12, replaces the chain)

**A mesh asset is a DAG of clusters, not a chain of levels.** The chain was the
original design and it cannot deliver per-cluster selection; the reasoning is
below, and `crcbl_scene::lod::build_lod_chain` is the chain implementation that
proved it.

### Why a chain cannot work

A chain simplifies each level from the base independently, then clusters each
level on its own. Two levels' cluster boundaries therefore have no relationship:
the coarser level's simplification collapsed vertices **along the boundary** the
finer level still has. Drawing one cluster at LOD0 beside its neighbour at LOD2
puts two differently-decimated versions of one shared edge next to each other,
and the surfaces no longer meet — a hole you can see the background through,
moving as the camera does. That is not an implementation defect; it is what
"simplify each level independently" means.

Locking every cluster boundary is not the fix either: cluster boundaries are
everywhere, so nothing would simplify and the mesh would freeze at LOD0
topology.

### The build

Group–lock–simplify–resplit, the shape Nanite uses (Karis, SIGGRAPH 2021):

1. **Cluster** the base mesh (`crcbl_scene::meshlet`). These are the DAG's
   leaves.
2. **Group** neighbouring clusters — a handful at a time — by partitioning the
   cluster adjacency graph, where two clusters are adjacent when they share an
   edge. Adjacency, not proximity: two clusters that nearly touch across a gap
   must not be grouped.
3. **Lock the group's outer boundary** and simplify its interior to roughly half
   its triangles. The internal cluster boundaries dissolve, so real
   simplification happens; the outer boundary is preserved exactly, so the group
   still meets its neighbours.
4. **Re-split** the simplified group into fresh clusters. These are the parents
   of every cluster that went into the group.
5. **Repeat, grouping differently each level**, so an edge locked at one level
   becomes interior at the next and finally gets to simplify.

A parent's clusters have several children, which is what makes it a DAG rather
than a tree.

### Why every cut is crack-free

Selection picks a **cut** through the DAG — a set of clusters covering the
surface exactly once. Wherever two detail levels meet across the cut, that
boundary was a **group boundary in the coarser level**, and step 3 preserved it
exactly. So the two sides share their boundary vertices by construction. This is
the whole reason for the grouping, and it is the property to test.

**Error is carried per group, not per cluster** (refined 2026-08-12 by the
implementation). A group simplifies as a unit, so every cluster it produced
stands or falls together: a cut that drew one of a group's parents while
descending into another would tear along a boundary that group never locked.
Each group's error is the worst vertex charge over its parents, raised to the
worst error of any cluster that went into it — monotone up the DAG by
construction, which is what makes a cut well-defined. Detail still varies across
a level, because different _groups_ differ; it does not vary within one.

### How a DAG reaches the renderer (decided 2026-08-12)

`crcbl-render` cannot see `crcbl-scene` — that would pull `gltf` into the
renderer, and it is a deliberate boundary — and `crcbl-shaders` has no
dependencies at all by design. So a DAG built by `build_cluster_dag` has no path
to `ClusterPool`, and the renderer's clusters are hand-written cooked constants
(`cube_clusters`, `pyramid_clusters`, `open_box_clusters`).

**The seam is a cooked artifact, mirroring the shader arrangement.** A tool
generates the DAG from the builder and writes it into `crcbl-shaders`; the
artifact is committed; a `--check` mode regenerates and compares, and CI runs it
the way it already runs "shaders (committed artifacts match their sources)".
That keeps `crcbl-shaders` dependency-free, makes the existing hand-written
constants a generated case of the same thing, and is a bake output in the sense
topic 6 means — when the real asset pipeline arrives it replaces the generator,
not the consumer.

Rejected: a dev-dependency on `crcbl-scene` (helps tests, leaves the shipping
path with no data), and a conversion in a crate that can see both (the renderer
still cannot reach it).

### What the fallback paths do

The DAG subsumes the chain. `IndirectCount` and `IndirectPerBatch` select a
**uniform cut** — every cluster at the same depth — which is exactly a chain
level, drawn as ordinary index ranges. Same hierarchy, same error metric, one
decision per instance instead of per cluster. So there is one builder, not two,
and the fallback is a restriction of the same structure rather than a parallel
implementation.

### Hand-authored levels keep their precedence

**Precedence rule (LOCKED): hand-authored first, auto as fallback**, unchanged
by this. Import resolves each level in order:

1. **Hand-authored level exists** (glTF node naming `name_LOD1`, `name_LOD2`, or
   the `MSFT_lod` extension) → used verbatim, always wins.
2. **Missing level** → generated by the DAG build above.

A hand-authored level is a whole-mesh level, so a mesh with hand LODs is
selected per instance at those levels even on the `MeshShader` path — an artist
supplies whole-mesh geometry, not a crack-free cluster hierarchy, and the engine
will not pretend otherwise. The import report says which levels came from where
(`crcbl lod stats`, editor LOD panel) — no silent substitution. A model with no
hand LODs gets a full generated DAG; a fully hand-authored chain is never
touched by the generator.

- Skinned meshes: the hierarchy applies to the bind-pose mesh; GPU skinning
  (topic 17) skins whichever clusters were selected — joint-weight-aware
  collapse is part of the auto-gen slice (weights carried through edge
  collapses).

## Auto-LOD: QEM simplification (from scratch, classic territory)

**Quadric error metrics** (Garland–Heckbert lineage — the well-published
standard under meshoptimizer/Simplygon-class tools). Implemented in
`crcbl_scene::simplify`, cited to the 1997 paper, with hand-derived values in
its tests rather than values recorded from its own output.

- Iterative edge collapse ordered by quadric error; **attribute-aware**:
  UV/normal seam edges constrained (seam drift is the classic auto-LOD
  artifact), material boundaries preserved.
- **A caller-supplied locked-edge set is the interface the DAG build needs**,
  and it is what separates this from a plain decimator. The simplifier infers
  topological borders — an edge used by exactly one face — on its own; a group's
  outer boundary is _interior_ to the mesh and can only come from the caller.
  Step 3 of the DAG build passes it, and without it there is no crack-free
  hierarchy.
- **The link condition is enforced alongside border locking**: an edge may
  collapse only when its endpoints share exactly two neighbours. Without it a
  closed mesh gains an edge with four faces and stops being closed. Not obvious,
  and the closed-mesh requirement silently depends on it.
- Per-level output records its **max geometric error** — that number drives
  runtime selection, so generation and selection share one metric. It is a
  quadric error and **not a certified Hausdorff bound**; the property test under
  Tooling is what would make it one.
- Deterministic: same input hash → identical output (bake cache by content hash,
  topic 12 golden-mesh tests). Needs a strict total order on collapse candidates
  — a cost keyed on `f32` has ties, and tie order decides the result.
- Quality guardrails: per-level triangle floor, degenerate/flip rejection,
  `crcbl lod stats` reports error curves; ugly results are an artist override
  away (that is why hand levels mix freely). Flip rejection is per-collapse and
  therefore local — a face can rotate all the way round under a sequence of
  individually-legal collapses, which only a global orientation check catches.

## Runtime selection: in the culling compute pass

- The stage 3 cull shader already reads per-instance bounds; it adds:
  **projected screen-space error** = level error scaled by distance/FOV → pick
  the coarsest level whose error stays under the pixel threshold → emit the
  indirect draw for that level's range. Per-frame, zero CPU, on every
  `GeometryPath` — it is the same maths and the same thresholds throughout.
- **Selection picks a cut through the DAG.** A cut is a set of clusters covering
  the surface exactly once: descend from the root while a cluster's projected
  error exceeds the pixel threshold, stop when it does not. Because a group's
  boundary was locked when it was simplified, any cut is crack-free by
  construction — that property is what the DAG exists for and is what its tests
  assert.
- **Granularity follows the path.** On `MeshShader` the descent runs in the
  amplification stage and is **per cluster**, so one mesh spans several levels
  across its own surface. On `IndirectCount` and `IndirectPerBatch` it runs in
  the cull compute pass and takes a **uniform cut** — every cluster at one
  depth, which is exactly a whole-mesh level — drawn as ordinary index ranges.
  Same hierarchy, same error metric, one decision per instance instead of per
  cluster: a visible quality difference on the fallback paths, and an honest
  one.
- **Monotonic error is what makes a cut well-defined.** A parent's error is at
  least its children's, so the descent has a single stopping point per branch
  and no cluster is ever drawn while an ancestor covering it is also drawn.
- **Hysteresis** on the threshold (switch-up and switch-down differ) kills
  boundary flicker.
- **Transitions**: instant swap MVP (correct thresholds make pops sub-pixel by
  definition — the error metric _is_ the pop size); dithered crossfade later if
  hero assets demand it (pairs with TAA era).
- **Shadow LOD bias**: shadow-pass culling (topic 18, same shader) selects +1/+2
  coarser levels — casters are cheap where it never shows.
- Global **LOD bias** = a quality setting (topic 14 settings UI; also the Tier
  B/web lever — web demos ship a default bias).
- **Graphics-only (LOCKED)**: LOD never touches simulation. Colliders are always
  full fidelity — physics, navmesh, audio occlusion, and every sim query run
  against the same geometry at every distance and every quality setting. This is
  structural, not policy: colliders live server-side and LOD selection is a
  client render concern — a client's LOD bias _cannot_ reach the sim by
  construction (fairness: two players with different quality settings play the
  identical physical world; determinism: the tick hash never depends on anyone's
  graphics).

## Far ranges (scheduled later, designed now)

- **HLOD**: per-sector merged proxy meshes (bake step: union of a sector's
  static content, aggressively decimated) — the mid-far tier between
  per-instance LODs and sector streaming; a sector's HLOD renders when the
  sector is loaded-but-distant. Slots into the same selection math at the sector
  granularity.
- **Impostors** (octahedral billboards) for the far-far tier — after HLOD proves
  insufficient, not before.
- Both are bake outputs riding existing sector machinery; neither is MVP.
  Orbit's planet gets by on LOD chains + on-rails distance (nothing walks a far
  planet — it's an equation with a texture).

## Tooling

- Debug (topic 7): LOD-level tint overlay, freeze-LOD-from-here camera (inspect
  selection from elsewhere), per-level instance counts + triangle totals in the
  render stats panel, screen-error heatmap.
- Editor (stage 8 growth): per-asset LOD panel — chain preview (step through
  levels), threshold/ratio overrides, regenerate button; viewer (sample 05)
  grows the same panel first (it's the asset inspection tool).
- CLI (topic 11): `crcbl lod gen|stats|preview` — headless generation,
  error-curve reports, offscreen renders per level for golden frames.
- Testing (topic 12): golden meshes (decimation determinism), error-bound
  property tests (reported error ≥ actual Hausdorff-approx sample), golden
  frames per level, selection unit table (distance/FOV/bias → expected level,
  hysteresis behavior).

## Delivery

| Slice                                                                                                                         | Phase                                                                         |
| ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| LOD table in mesh handles + cull-shader selection + hysteresis + hand-LOD import (naming/`MSFT_lod`)                          | P7 (with GPU-driven rendering — mechanism is a small delta on the cull pass)  |
| Shadow LOD bias                                                                                                               | P7 (with CSM, topic 18)                                                       |
| **QEM auto-generation + meshlet cluster hierarchy** (attribute/seam-aware, skinned-aware), sidecar overrides, `crcbl lod` CLI | **P7** (the `MeshShader` path selects per cluster and needs levels to select) |
| Debug overlay + stats; global bias setting; editor/viewer LOD panel                                                           | P10                                                                           |
| HLOD per-sector proxies                                                                                                       | wave 2                                                                        |
| Impostors, dithered crossfade                                                                                                 | later, on demonstrated need                                                   |

## Risks

- **QEM attribute handling** is where auto-LOD tools live or die (seams,
  skinning weights): constrained-collapse rules + golden meshes from the first
  slice; hand-override escape hatch means no asset is ever blocked.
- **Threshold tuning subjectivity**: the error metric makes it math-first
  (pixel-error budget), the debug tint makes it visible; per-asset overrides are
  data.
- **Selection-shader complexity creep**: it's ~20 lines on the cull pass;
  HLOD/impostors get their own passes rather than complicating this one.
