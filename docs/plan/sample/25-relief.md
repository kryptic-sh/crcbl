# Sample 25 — relief (S4J, gates P7H)

Tessellation acceptance test, and the fixture that proves
[59-tessellation.md](../59-tessellation.md): smooth silhouettes and real
displacement on every geometry path, natively and in a browser tab, with the
density the engine chose drawn where you can see it.

**This is the sample that shows tessellation adds geometry only where it earns
it, and never opens a crack.** A tessellation demo that looks good at one
distance proves nothing; the failures have names — cracks at a level boundary,
swimming as a factor changes, a silhouette that stays flat, a budget that
overflows silently, a displaced surface culled while on screen — and each scene
here is built to make one of them visible.

## Proves

- **Every rung the engine ships is reachable**: flat, R0-baked displacement, R1
  Phong tessellation, R1 run-time displacement and PN triangles, on a selector,
  with a seam putting any two side by side on sundial's split pattern
  (`crcbl_render::split`).
- **Density follows the screen**: a heatmap view colours each triangle by its
  tessellation factor or cluster level, and a dolly shows density rising as the
  camera closes and falling with it, with no swim.
- **No cracks**: a wireframe view over a contrasting background, at every dice
  rate and across a cut that mixes levels.
- **The budget is honest**: record count, overflow count, tessellation compute
  cost and extra vertex cost on the panel, the headless summary and the page.
- **Every geometry path**, the browser's included.

## Scope

- **Scene 1, silhouettes.** Avocado and Suzanne (both CC0, both already on the
  viewer's asset shelf and Suzanne in the repository) flat, Phong and PN side by
  side, wireframe toggle, a dice-rate slider. Suzanne needs its positions welded
  first — its glTF stores 11,808 corners over 2,012 positions.
- **Scene 2, a displaced patch.** A cobblestone or paving patch driven by a CC0
  16-bit height map from Poly Haven (`cobblestone_floor_04`, 1k displacement
  PNG) or ambientCG (PavingStones070), fetched by a pinned per-file URL and a
  sha256 list on `tools/fetch-shelf.sh`'s pattern, with a licence row per asset;
  flat normal map, R0 and R1 on the selector, the density heatmap on a key.
- **Scene 3, a rock field dolly.** A CC0 rock height map (ambientCG Rock030 or
  Poly Haven `rocky_terrain_02`) over a field of instances, a scripted dolly for
  determinism and goldens, and the record budget's overflow driven on purpose by
  a slider.
- **Debug views**: wireframe, factor heatmap, cluster level, bounds.
- **Pages web demo** at `/demos/relief/`, every control on the page, with a
  stated megabyte budget for the browser asset subset.

## Non-goals (hard cap)

Terrain authoring. Concurrent binary trees. Parallax occlusion mapping — it is
refused in [44-lighting.md](../44-lighting.md), so there is no parallax column
in the comparison unless that refusal is lifted. Hardware tessellation stages.
Assets under anything but CC0 without a recorded decision: Khronos's
`terrain_heightmap_r16.ktx` is Apache-2.0 and is not used.

**Exempt from sample rule 11**, on lantern's ground. **Exempt from rules 2 and
10**, on sundial's: no game state.

## Status: planned 2026-09-15, nothing built

## Milestones

1. **Baked relief.** R0: the height page, the import-time bake, scene 2 flat
   against R0, the heatmap of cluster levels, the sample skeleton, the asset
   fetch, the web demo and the CI golden step.
2. **Silhouettes.** R1a: scene 1 with Phong tessellation, wireframe, the
   dice-rate slider, the record and overflow counters, every geometry path.
3. **Run-time displacement.** R1b: scene 2's R1 column and scene 3's dolly,
   per-cascade dice rates for shadows.
4. **PN triangles.** R1c on scene 1.

## Exit criteria

- Every tessellation rung the engine ships is reachable, and the sample names
  any the device removed.
- No background pixel shows through a tessellated surface at any dice rate or
  across a mixed-level cut, as a golden claim shown red by sabotage.
- The scripted dolly is a determinism check: the same record counts on two runs.
- Record count, overflow count and tessellation cost on the panel, the headless
  summary and the page.
- A golden per scene and rung, on every geometry path.
- Web demo deployed, running on the WebGPU backend.
- Rule 12: selected paths reported, a flag forces a lesser one.
