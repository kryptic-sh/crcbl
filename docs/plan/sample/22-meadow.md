# Sample 22 — meadow (S4G, gates P7D and P7F)

Grass and vegetation-wind acceptance test, and the fixture that proves
[57-grass.md](../57-grass.md) and the field half of [56-wind.md](../56-wind.md):
one hillside drawn three ways — card grass, mesh blades and stylised shells —
under one wind, natively and in a browser tab.

**This is the sample that shows the three looks are one system.** Switching the
field from cards to blades to shells changes the blade description and nothing
else: the same placement, the same gust rolling across the same slope at the
same moment, the same trail behind the same walker. A look that needs its own
wind or its own placement to look right is this sample's failure.

## Proves

- **Every look is selectable live** — cards, blades and shells — and a seam puts
  any two side by side on one frame, on sundial's split pattern
  (`crcbl_render::split`), so realistic-billboard, realistic-mesh and stylised
  read as a comparison rather than three screenshots.
- **The wind is one field.** Its two authored layers are visible as debug views
  — the intensity map with its calm patches and the direction map bending around
  the hill — and a gust visibly travels across grass, trees and a hanging banner
  together.
- **Wind reaches physics.** Light crates and a tumbleweed on the slope are
  pushed by the same field through `crcbl-phys`, and come to rest in the calm
  patches the intensity map paints.
- **Interaction is local and recovers.** A walker leaves a trail that bends
  exactly the grass it crossed and springs back over the fade time.
- **Trees sway on every rung** of the vegetation ladder, from vertex-colour
  bending to a hero tree on bones.
- **Placement is deterministic** and no frame reads the last one, so every look
  is a golden.

## Scope

- **One hillside**: a heightfield the sample builds, on quarry's lattice
  pattern, with a type and density map painting a meadow, a mown lawn, a mossy
  rock and a path.
- **The three looks** on a key and page buttons, and the seam between any two.
- **Stylised presets**: base-to-tip gradient, root occlusion colour, tip colour,
  clump colour, ground-facing normals.
- **Wind controls**: the weather speed on the Beaufort presets, the base
  direction, the two layers' debug views, a gust front toggle and a motor the
  player can drop (a fan, a helicopter's downwash).
- **Physics bodies**: crates and a tumbleweed pushed by the wind.
- **Trees**: a grove carrying T1 and T2 data, and one hero tree on T3 bones.
- **A walker** on a scripted path with a capsule collider for the trail.
- **Per-pass cost** for generation, the grass pass and the tree vertex work in
  the debug panel and the headless summary.
- **Pages web demo** at `/demos/meadow/`, every control on the page.
- **Spatial audio**: a wind bed whose gain follows the field at the listener.

## Non-goals (hard cap)

A game. Terrain authoring. Flowers, bushes and props beyond the grove. Seasons
and snow. Hair and fur — [23-mane.md](23-mane.md) owns those, and shares only
the shell technique.

**Exempt from sample rule 11**, on lantern's ground. **Not exempt from rule 2**:
the wind-pushed bodies are server state.

## Status: planned 2026-09-15, nothing built

## Milestones

1. **Cards in the wind.** 56's W1 and W2, 57's G1: the field, the hillside, card
   grass with cooked coverage mips, travelling gusts, the layer debug views. The
   sample skeleton, the web demo and the CI golden step.
2. **Blades.** G2 and G6: mesh blades, LODs, clumps, rings, the far-field
   texture, the shadow impostor; the cards-versus-blades seam.
3. **Shells.** G3: shell lawn and moss with fins; stylised presets across all
   three looks; the three-way selector.
4. **Trees.** T1 and T2 on the grove; the `_WIND_*` import.
5. **Wind in physics.** W3 and W4: motors and wind drag on the crates and the
   tumbleweed, with the rotation and per-body medium prerequisites
   [55-water.md](../55-water.md) names.
6. **Interaction.** G4, then G5: the walker's trail, then per-blade simulation.
7. **The hero tree.** T3: bone wind on compute skinning.

## Exit criteria

- Every look and every tree rung the engine ships is reachable, and the sample
  names any the device removed.
- A gust crossing the hillside reaches two marked blades, a tree and a crate in
  the order and at the separation their positions predict.
- A scripted run is a determinism check: the same placement and the same body
  poses on two runs.
- A golden per look, per stylised preset family, and one each of the intensity
  and direction debug views.
- Per-pass cost in the debug panel and the headless summary.
- Web demo deployed, every look included, running on the WebGPU backend.
- Rule 12: selected paths reported, a flag forces a lesser one.
