# Sample 21 — tide (S4F, gates P7E)

Water acceptance test, and the fixture that proves
[55-water.md](../55-water.md): every body kind the engine ships, drawn and
floated from the same data, natively and in a browser tab. One gallery of four
scenes you switch between with a key or a page button.

**This is the sample that shows water is a system and not a shader.** A water
demo that is only a pretty surface proves nothing the engine can reuse. Tide's
subject is the agreement between three readers of one field: the renderer
drawing a wave, the physics world lifting a crate on that wave, and a server
with no GPU answering where the wave is — so a crate that bobs out of step with
the surface it sits on is this sample's failure, however good the surface looks.

## Proves

- **Every body kind is reachable**: ocean, lake (with pond and swamp as medium
  presets), river, waterfall, pool, and a shoreline where the ocean meets a
  beach — each in a scene built to show it, with the sea state, the medium and
  the wind as live knobs.
- **Water is deterministic.** A scripted run of any scene produces the same wave
  heights and the same floating poses on every run, on every backend, and in the
  browser, because the surface is a function of the body, the seed and the tick.
- **The physics floats things on the water it draws.** Crates on pontoons and a
  boat on Kerner's hull model sit on the rendered surface within a stated
  tolerance, drift down the river by the flow the surface draws, and leave wakes
  and ripples the grid carries.
- **The surface reads the frame behind it**: refraction, depth absorption,
  shoreline fade, reflection with the surface's own normal, and a planar
  reflection on the flat pool.
- **Underwater works from both sides**: the camera dives, the medium closes in,
  the surface is seen from below through Snell's window, and caustics cross the
  floor.
- **No transcendental reaches a pixel and no frame reads the last one**, which
  is what lets every one of those scenes be a golden.

## Scope

- **Scene 1, open sea.** The FFT ocean to the horizon under a moving sun; a sea
  state selector (calm, normal, storm) and a wind knob that blends between them;
  a boat on the hull model and a spread of crates on pontoons; wake foam behind
  the boat; a dive camera that crosses the surface.
- **Scene 2, coast.** The same ocean meeting a sloped beach: shore waves phased
  by the cooked distance field, shallow attenuation, shore foam, and an
  underwater view in the shallows with caustics on the sand.
- **Scene 3, valley.** A river running into a waterfall, the waterfall into a
  plunge pool, the pool into a lake. Logs drift down the flow and over the fall;
  the lake carries the pond and swamp presets.
- **Scene 4, courtyard.** A tiled pool with a fountain: a clear medium, floor
  caustics, the planar reflection, and a ball the player drops to start the
  ripple grid.
- **Server-authoritative.** Floating bodies are game state and run in the server
  loop over `crcbl-phys`, which is how the sample proves `crcbl-water` links and
  answers queries without a renderer.
- **Debug views**: the wave field's height, the Jacobian and whitecap coverage,
  the flow map, the shore field, the underwater mask, and the CPU query points
  drawn over the rendered surface so disagreement is something you can look at.
- **Per-pass cost** for the water passes in the debug panel and the headless
  summary, per [43-render-standards.md](../43-render-standards.md).
- **Pages web demo** at `/demos/tide/`, with the scene, the sea state, the wind,
  the medium preset and the camera as page controls rather than keys, on
  alcove's pattern (`apps/alcove/src/web.rs`).
- **Spatial audio** on the ladder's rule: surf, the river and the fall as
  looping voices at points until line emitters exist.

## Non-goals (hard cap)

A game. Swimming characters. Terrain authoring or carving tools. A volumetric
fluid solver of any kind. Mist, spray and splash particles until blended
particles exist ([53-transparency.md](../53-transparency.md)). Wet sand and rain
until a material wetness hook exists. Weather beyond the wind knob.

**Exempt from sample rule 11**, on lantern's ground. **Not exempt from rule 2**:
the floating bodies are why this sample has a server.

## Status: milestone 1 built 2026-09-15

`apps/tide` exists and ships at `/demos/tide/`: the four-scene switch with the
courtyard built and three labelled rooms, four medium presets from one
bio-optical model, fixed, orbit and free cameras, the water passes' cost on the
panel, the summary and the heartbeat, a golden step in CI and a browser gate in
`pages.yml`. **Two departures from the scope above**: the sun is fixed rather
than on a clock (sundial's clock lives inside that sample, and reusing it would
mean copying it), and there is no server yet — milestone 1 has no game state, so
the server loop arrives with milestone 2's floating crates, which is when rule 2
starts to bind.

Milestones follow [55-water.md](../55-water.md)'s rungs, so a milestone cannot
close before its rung has landed in the engine.

## Milestones

1. **The courtyard pool, still.** Rung 1: the surface pass, refraction,
   absorption, shoreline fade, sky and probe reflection, received shadows. The
   sample skeleton, the four-scene switch with three scenes stubbed as empty
   rooms, the web demo and the CI golden step. **Done 2026-09-15**, held by
   three relations and four goldens on lavapipe and radv. Its browser price at a
   959×463 canvas, p50: `water-copy` 0.018 ms and `water` 0.055 ms on an RDNA-3
   adapter, 5.7 ms and 44.7 ms on SwiftShader — one boot's frames from the
   gate's run, not a dedicated measurement.
2. **Waves and floating.** Rung 2: trochoid waves on the pool and lake, rings,
   the query, pontoon crates in the server loop, immersion events, the
   query-points debug view and the agreement check.
3. **The open sea.** Rung 3: the FFT ocean, sea states, the colour model,
   whitecap foam, the CPU swell field, and the boat on pontoons.
4. **The valley.** Rung 4: river, waterfall, plunge pool and lake; flow-driven
   foam; drifting logs; body transitions.
5. **The coast.** Rung 5: the shore cook, shore waves, shallow foam.
6. **Reflection.** Rung 6: the surface march everywhere; planar on the pool.
7. **Underwater.** Rung 7: the dive camera in the sea and the coast; the mask,
   medium, meniscus, Snell's window and caustics.
8. **Interaction.** Rung 8: the ripple grid in the courtyard, wakes, and the
   boat moved onto Kerner's hull model.
9. **The fountain.** Rung 9: jets, floor caustics under the ripples.

## Exit criteria

- Every body kind the engine ships is reachable, and the sample names any the
  device removed.
- A scripted run of every scene is a determinism check: the same wave heights
  and floating poses on two runs, compared bit for bit on the CPU.
- The CPU query agrees with the rendered surface within the tolerance
  [55-water.md](../55-water.md) states, on every scene with a floating body.
- A box of known density floats at the waterline its density predicts.
- A golden per scene, plus one underwater, one at a grazing sun for glints, and
  one of the whitecap coverage view.
- Per-pass cost in the debug panel and the headless summary.
- Web demo deployed, every scene included, running on the WebGPU backend.
- Rule 12: selected paths reported, a flag forces a lesser one.
