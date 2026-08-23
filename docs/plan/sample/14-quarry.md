# Sample 14 — quarry (S4C, gates P7)

Geometry acceptance test and the living fixture for the meshlet path (topic 3
§3.5) and cluster LOD (topic 25). One dense scene, rendered on every
`GeometryPath`, with the cluster hierarchy made visible. Not a game — the
geometry is the content.

Where lantern proves the two lighting paths agree, quarry proves the three
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

**Exempt from sample rules 2 and 10** (client/server authority, gameplay through
`GameModule`), on the same ground the viewer is: both rules exist so a _game_'s
state lives on the server and its logic lives in module code, and this fixture
has no game state to place anywhere. One face, one instance, a camera and a
debug view selector — nothing simulates. So this crate opens no `World`,
registers no system and implements no `GameModule`, and their absence is the
charter's answer rather than an oversight.

## Milestones

1. Meshlet bake + `MeshShader` path rendering the scene (topic 3 §3.5 proof).
2. QEM cluster hierarchy generation; per-cluster selection + hysteresis; LOD
   tint and heatmap overlays (topic 25 proof).
3. `IndirectCount` and `IndirectPerBatch` paths on the same content, forced-path
   comparison.
4. Skinned and tiling cases; Pages demo with recorded browser budget.

## Measured

Taken 2026-08-20 on an AMD Radeon RX 7900 XTX (RADV NAVI31, Mesa 26.1.7-arch1.1)
by `apps/quarry/tests/device/`, which is where each number can be reproduced.
The exit criteria below ask for them here rather than in a commit message, so
here they are — and where a criterion asks for a **human** to look, this section
says so instead of standing in for one.

**The face.** 8192 triangles at level 0, one instance of one mesh. The uniform
cut at a 16 px budget draws level 1, 4096 triangles.

**Where the reduction comes from — `all_of_the_reduction_is_cluster_culling`.**
Over seven reported frames down the fixed dolly the camera's instance cull kept
**1 of 1 every time**, and the amplification stage kept 26 to 34 clusters. All
of the reduction is cluster culling, and the reason is the scene rather than the
renderer: quarry places one instance, so the instance cull has one thing to
decide about. A scene wanting both numbers to be interesting needs more
instances, which is horde's job and not this sample's.

**Which test does the rejecting —
`the_three_cluster_counts_add_up_to_the_cut_they_were_taken_over`.** Standing at
the dolly's far end at a 256 px budget, the descent chose a cut of **58**
clusters (`[15, 31, 12, …]` finest level first), and the amplification stage
answered: **30 kept, 28 rejected by the frustum, 0 by the normal cone.** The
three partition the cut, which is what that test asserts.

**The cone rejects nothing on this face, and that is the correct answer.**
Measured separately by pinning the eye underneath the surface so every cluster
faces away from its viewer: clusters kept moved from 44 to 42, and covered
pixels not at all. A rough surface gives clusters cones wider than a hemisphere,
which `crcbl_shaders::meshlet::ClusterBounds::cone_cutoff` records as a cutoff
at or below zero and `cluster_survives` skips outright. The value of the split
is that the panel can now _say_ the cone did nothing; before it could only say
30 of 58 survived, which is equally consistent with the cone doing all of the
work.

**The three paths against each other**, from the committed goldens rather than
from a device, so any reader can reproduce it:

| dolly stop            | mesh-shader against either indirect path     |
| --------------------- | -------------------------------------------- |
| start (standing back) | 233 px differ (0.47%), max channel delta 118 |
| end (inside the face) | 0 px differ                                  |

The two indirect paths are identical to each other at both stops, which is
expected: they run the same per-instance selection through different draw
machinery. The difference is at the **far** stop, not the near one — standing
back, screen-space error varies most across a face that recedes 180 m, so
per-cluster selection has the most to disagree with per-instance selection
about; inside the quarry everything is at the finest level and all three draw
the same triangles.

**What no measurement can close, stated as owed rather than met:** whether those
233 pixels read as "the same scene at the same budget" is a judgement, and the
seam review — UV and normal seams held, material boundaries preserved through
collapses — asks for a human to look at `quarry_face(CELLS)` and `quarry_tile`.
Neither has been done. `docs/backlog.md` carries both.

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
