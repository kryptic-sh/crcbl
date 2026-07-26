# Topic 33 — Decals + Surface Carving

One decal system for both jobs: **authored decoration** (posters, grime, cracks,
signage placed in the editor) and **dynamic impacts** (bullet holes, scorch,
blood) spawned from `KineticContact` events (28). Three fidelity tiers on one
volume primitive — flat projected textures, parallax depth, and **slicer volumes
that carve negative space out of the surface they hit** for genuinely 3D bullet
holes. Projected decals land wave 1; carving is FPS-era.

## One primitive, three tiers

A decal is a **volume** (OBB) + material + tier. Everything else is a knob.

| Tier                    | What it does                                                                                                                                                                                                | Cost             | Used for                                           |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- | -------------------------------------------------- |
| **T0 — Projected**      | Albedo/normal/roughness projected onto surfaces inside the volume                                                                                                                                           | cheap            | decoration, blood pools, scorch, dirt              |
| **T1 — Parallax**       | T0 + heightmap ray-marched in the decal shader — apparent depth, flat silhouette                                                                                                                            | moderate         | small pits, tread marks, shallow holes             |
| **T2 — Slicer (carve)** | Declares a **negative SDF volume**: host surface fragments inside it are discarded, and the volume's interior walls render as cavity geometry — real silhouette change, see-through when it punches through | higher, budgeted | bullet holes with visible depth, punctures, gouges |

Tier is a property of the decal material, so the _same_ impact definition can
ship as T0 on low quality settings and T2 on high (a quality-settings knob,
topic 14) — one authored asset, three budgets.

## Carving (T2) — how the negative space works

**No mesh is modified.** The carve is evaluated in the host surface's shader:

- The decal declares an SDF primitive in decal space (sphere, capsule, cone,
  box, or a small 3D texture for authored shapes) marked _subtractive_.
- Active carve volumes are **clustered** alongside decals (below); a host
  material flagged `carveable` tests only the volumes in its froxel: sample the
  SDF at the fragment's world position → inside → `discard`.
- **Cavity interior**: the decal volume's own back-facing geometry renders with
  the crater material (rock/metal/wood interior), depth-tested against the host
  — you get a shaded pit with real parallax and a real silhouette, not a painted
  circle.
- **Through-holes**: when the volume passes fully through thin geometry, the
  discard punches a genuine hole — you see what's behind it, and light passes if
  the shadow pass applies the same test (it does, same cluster data).
- Cone/tapered volumes give the entry-wide/exit-narrow look for free;
  penetration direction from the impact event orients them.

**Graphics-only rule (LOCKED, same as LOD 25)**: carving never touches
colliders, navmesh, audio occlusion, or any sim query. It is a client-side
visual record — and it stays _consistent_ because the gameplay truth is the
ballistics system: a hole appears where topic 28 already decided a round
penetrated, so what you see through matches what you can shoot through. Holes
that genuinely change gameplay geometry (blow open a wall, create a new
sightline) are **destruction** — a separate future topic requiring
server-authoritative geometry state, collider rebuild, navmesh re-bake (24's
async tile path), and PVS invalidation (31). The hooks exist; the feature is
deliberately not here.

## Spawning + lifetime

- **Dynamic impacts**: client-side reactions to `KineticContact` (28) — the
  event already carries point, normal, material tag, deposited energy and
  collider id. Material tag picks the decal set (concrete → crater + dust, metal
  → scar + sparks, wood → splinter, flesh → blood); deposited energy scales
  size/tier. Zero new networking: decals are a _presentation_ of events the
  client already receives.
- **Authored decoration**: decal entities in scene chunks (topic 6),
  editor-placed with volume gizmos. Static decals may be **bake-merged** into
  the surface material at `crcbl bake` time for zero runtime cost when a map
  ships.
- **Pool + budget**: fixed-size ring for dynamic decals; oldest fade and retire.
  **Per-surface density cap** (a wall that has taken 500 rounds shows a
  plausible cluster, not 500 stacked quads — new decals replace nearest
  neighbors past the cap). Carve volumes have their own, tighter cap; excess
  impacts degrade T2 → T1 → T0 rather than dropping.
- **Skinned surfaces**: blood on characters = bone-attached decal volumes
  (projected in the skinned output space, 17) — they follow the animation.
  Carving on skinned meshes is not supported (stated, not discovered).

## Rendering

- **Clustered forward decals**: decal + carve volumes are binned into the view
  froxel grid (SSBO lists); host materials loop only their froxel's entries. No
  G-buffer requirement — works identically on Tier A and Tier B (topic 3),
  consistent with the forward HDR pipeline (18).
- Projection sanity: surface-normal-vs-decal-axis rejection threshold (the
  classic fix for decals smearing across perpendicular faces), plus per-decal
  angle fade.
- Decals write into the HDR pass before tonemap; emissive decals (glowing
  scorch, holo signage) feed bloom naturally.
- Shadow interaction: carve volumes apply in the shadow pass (light through a
  shot-out panel), projected decals do not.
- Sorting/blending: decals composite in cluster order with per-decal blend
  modes; deterministic ordering by decal id so goldens are stable.

## Tooling

- **Editor**: decal placement tool (drag a decal material into the scene, gizmo
  the volume, live projection preview), tier override per instance, bake-merge
  toggle for static decals.
- **Debug** (topic 7): decal cluster heatmap, carve volume wireframes, overdraw
  view, pool occupancy + per-surface density map, "why isn't my decal showing"
  inspector (angle rejection / cluster miss / budget drop).
- **CLI** (topic 11): `crcbl decal preview <material>` offscreen renders on test
  surfaces at a sweep of angles — the golden-frame source.

## Testing (topic 12)

- Golden frames per tier × material × incidence angle (flat wall, corner, curved
  surface, thin panel).
- **Carve silhouette goldens**: through-hole shows background; cavity interior
  shades correctly; shadow pass agrees with the color pass.
- Budget properties: density cap holds under sustained fire (fuzzed impact
  streams); tier degradation happens in order and never leaves a gap; pool churn
  leaks nothing.
- Projection property: no decal renders on a surface beyond the rejection angle
  (the smear regression).
- Determinism for goldens: decal spawn from seeded events reproduces
  identically.
- Gate interaction (31): impact decals from culled sources are filtered exactly
  like VFX — covered by the all-channel leak property, and decals on _your_
  geometry pass through (they are legitimate peek information).

## Delivery

| Slice                                                              | Phase                      |
| ------------------------------------------------------------------ | -------------------------- |
| Volume primitive + clustered forward projection (T0) + pool/budget | wave 1 (with sparks/VFX)   |
| Editor placement + bake-merge for static decals                    | wave 1                     |
| `KineticContact` → impact decal mapping (material sets)            | wave 1                     |
| T1 parallax                                                        | wave 1 tail                |
| **T2 carve volumes** (SDF discard, cavity interior, shadow pass)   | FPS-era (breach drives it) |
| Bone-attached decals on skinned meshes                             | FPS-era                    |
| Quality-tier mapping in settings; degradation ladder               | FPS-era                    |

## Risks

- **`discard` kills early-Z** — the classic carve performance trap. Mitigated
  by: only `carveable`-flagged materials take the branch, tight clustering so
  most fragments test zero volumes, and a hard carve budget. Measured in the
  breach frame budget, not assumed.
- **Decal overdraw** on heavily-fought surfaces: density cap + heatmap make it
  visible before it's a report.
- **Carve/collision mismatch expectations** ("I can see through it, why can't I
  walk through it") — answered by the graphics-only rule and by ballistics
  consistency; documented in the decal authoring guide so it's a design
  decision, not a bug report.
- **Scope pull toward destruction**: T2 is a shader trick with hard limits. Real
  geometry destruction is a separate topic with server-authoritative
  requirements; the line is stated here so it stays uncrossed by accident.
