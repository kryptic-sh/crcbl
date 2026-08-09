# Sample 14 — quarry (S4C, gates P7)

Geometry acceptance test and the living fixture for the meshlet path (topic 3
§3.5) and cluster LOD (topic 25). One dense scene, rendered on every
`GeometryPath`, with the cluster hierarchy made visible. Not a game — the
geometry is the content.

Where lumen proves the two lighting paths agree, quarry proves the three
geometry paths do. It is also the only place the QEM generator's output is
looked at rather than measured: an error metric can be within budget and the
mesh can still be visibly wrong at a seam.

## Proves

- **The meshlet path draws what the fallbacks draw.** Same scene on
  `MeshShader`, `IndirectCount` and `IndirectPerBatch`, forced by flag,
  compared. The paths differ in selection granularity — per cluster on the
  first, per instance on the other two — so they are not expected to match pixel
  for pixel; they are expected to be the same scene at the same quality budget.
- **Cluster LOD is per cluster, and you can see it.** LOD-level tint overlay,
  freeze-selection-from-here camera, screen-error heatmap: one mesh spanning
  several levels across its own surface is the claim, and the tint is what makes
  it a claim anyone can check.
- **The QEM generator survives real content**: UV and normal seams held,
  material boundaries preserved, border locking on a tiling mesh, skinned
  weights carried through collapses. Golden meshes for determinism, and a human
  looking at the seams for the part determinism cannot catch.
- **Hysteresis kills flicker.** A slow dolly past the switch distance shows no
  boundary popping, on every path.
- **Amplification-stage culling is doing work**: per-cluster frustum and
  normal-cone rejection counts on the debug panel, and a camera position where
  turning them off is measurable.

## Scope

- One quarry-face scene: high-polygon rock and machinery content with a wide
  depth range, chosen so that per-cluster selection has something to select
  differently across a single mesh. A tiling modular wall piece for border
  locking, and one skinned prop for weight-aware collapse.
- Free-fly camera plus a fixed dolly for goldens and the hysteresis check.
- Path forcing, LOD bias control, and the topic 25 debug overlays.
- Pages web demo, which runs `IndirectPerBatch` with per-instance LOD — the
  honest picture of what a browser visitor gets, at a recorded budget.

## Non-goals (hard cap)

Gameplay, streaming or HLOD (topic 25 schedules those later and this sample must
not smuggle them in), impostors, a second scene, an authoring tool for meshlet
parameters. Automatic simplification quality research beyond what topic 25
ships.

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the
subject is 3D geometry density. Rules 4 and 12 apply in full — this is the
sample where path reporting matters most, since three paths is the widest
selector in the engine.

## Milestones

1. Meshlet bake + `MeshShader` path rendering the scene (topic 3 §3.5 proof).
2. QEM cluster hierarchy generation; per-cluster selection + hysteresis; LOD
   tint and heatmap overlays (topic 25 proof).
3. `IndirectCount` and `IndirectPerBatch` paths on the same content, forced-path
   comparison.
4. Skinned and tiling cases; Pages demo with recorded browser budget.

## Exit criteria

- Golden frames per `GeometryPath` from the fixed dolly, and the human-reviewed
  three-way comparison recorded here.
- Golden meshes prove the generator is deterministic; the seam review is
  recorded with the content it was done against.
- Triangle count and draw count per path, at a stated camera position, recorded
  — including how much of the reduction is instance culling and how much is
  cluster culling, because a single total hides which one is working.
- Dolly run shows no LOD popping on any path.
- Web demo renders the scene on `IndirectPerBatch` with no missing geometry, at
  a recorded budget, and the summary line names the path it took.
