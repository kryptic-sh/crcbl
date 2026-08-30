# Topic 18 — Render Features: Lighting, Shadows + Post-Processing

The visual-credibility layer on top of the stage 2/3 renderer: **two complete
lighting paths** (ray traced and rasterised), shadow maps, and the
post-processing stack (HDR, tonemapping, AA, bloom). Ray-traced lighting,
rasterised lighting, shadows and the HDR/tonemap/FXAA core are all **MVP**;
bloom follows at P10; TAA is post-MVP.

## Where each technique now lives

**This topic was split on 2026-08-27.** It had reached a hundred kilobytes and
eight independent techniques, which meant that a reader who came for the shadow
bias had to page through screen-space reflections to reach it, and that any
change to one technique produced a diff nobody could review against the rest.
Each technique now owns a document; what stayed here is the part that belongs to
none of them — how the techniques interact, when each slice lands, and what is
at risk across all of them.

Nothing was rewritten in the move. **A citation that names this file still lands
somewhere useful**: the table below is the one hop from here to the section it
meant, and the code and test comments that name
`docs/plan/18-render-features.md` were deliberately left alone rather than
churned in a move commit.

| Technique                                                               | Document                                           |
| ----------------------------------------------------------------------- | -------------------------------------------------- |
| Lighting paths, the light list, clustered forward, BRDF, the PBR ladder | [44-lighting.md](44-lighting.md)                   |
| Shadows: cascades, atlas tiles, bias, filter ladder                     | [45-shadows.md](45-shadows.md)                     |
| Ambient occlusion: SSAO, its blur, GTAO                                 | [46-ambient-occlusion.md](46-ambient-occlusion.md) |
| Screen-space reflections: the march, roughness                          | [47-reflections.md](47-reflections.md)             |
| The post-processing stack: order, HDR, tonemap, bloom                   | [48-post-processing.md](48-post-processing.md)     |
| Antialiasing: FXAA, SMAA, CMAA2, TAA, MSAA                              | [49-antialiasing.md](49-antialiasing.md)           |
| Irradiance probes: the L1 grid                                          | [50-irradiance-probes.md](50-irradiance-probes.md) |
| Volumetrics: height fog, the froxel column, light shafts                | [51-volumetrics.md](51-volumetrics.md)             |

**What this engine does not do at all** is a different question from how well it
does these, and it is answered in one place:
[43-render-standards.md](43-render-standards.md).

## Interactions (kept honest)

- Render-scale upscale (topic 15) happens **after** tonemap+AA: post chain costs
  scale with internal res (the whole point of render scale). The UI bullet below
  binds whoever composites at native resolution.
- UI renders after upscale at native res (crisp text regardless of 3D scale) —
  this ordering is the reason the UI pass was kept separate in stage 7.
- Debug overlays (debug draw, gizmos) render pre-tonemap in HDR (they're in the
  world) except UI-space panels. §3.6's debug draw layer is itself unbuilt, so
  this rule binds whoever builds it.
- Golden-image tests (topic 12): shadows and each post pass get dedicated golden
  frames; tonemap changes are the classic "everything shifted" diff — the
  `--bless` flow exists for exactly this.

## Delivery

| Slice                                                                                                                                         | Phase                                                                                                                                                                                                                                                                    |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Sun CSM (culling-integrated; the 3×3 PCF became the disc below)                                                                               | P7 — the **cascade debug overlay** is not built                                                                                                                                                                                                                          |
| Acceleration structures: BLAS bake/load, TLAS refit, `crcbl as stats`                                                                         | P7C                                                                                                                                                                                                                                                                      |
| Ray-traced shadows + AO                                                                                                                       | P7C                                                                                                                                                                                                                                                                      |
| Ray-traced reflections                                                                                                                        | P7C                                                                                                                                                                                                                                                                      |
| Ray-traced global illumination                                                                                                                | P7C                                                                                                                                                                                                                                                                      |
| The render quality pass: **SMAA 1x**, ~~GTAO~~ + bent normals, **Hi-Z + cone-traced SSR**, ~~shadow cross-fade → rotated Poisson PCF → PCSS~~ | P10, with the bloom chain, because the profiler HUD is what shows a quality rung's cost honestly. Each rung's section above says what it costs and what it refuses. **The Hi-Z half of the SSR rung is built (2026-08-27)**; the cone trace over a colour pyramid is not |
| MSAA                                                                                                                                          | **No phase, and not a rejection** — viable and priced by the seventh decision, and not the default for exactly as long as SSAO and SSR read a single-sample depth                                                                                                        |
| TAA (jitter, motion vectors); temporal SSR; ~~shadow atlases~~ (pulled forward 2026-08-30, [45-shadows.md](45-shadows.md))                    | post-MVP. Auto-exposure left this row on 2026-08-29 (post stack)                                                                                                                                                                                                         |

**P7B and P7C are new phases** carrying the raster twin and the ray-traced path
respectively; the roadmap's phase table is authoritative for their ordering. The
raster twin lands **first**, deliberately: it is the path macOS, iOS and every
browser use, so it is the one whose absence would block more platforms than it
unblocked, and it is the reference the ray-traced path is reviewed against.

Sample impact: horde (S3) onward renders shadowed + tonemapped; orbit's planet
terminator and towers' map lighting are the showcase beneficiaries; **`lantern`
(sample 13) is the dedicated lighting sample** and exists to show the same scene
under both paths side by side. Exit criteria of the other samples inherit
"shadows on, stack on" implicitly via the phase gates.

## Risks

- **CSM artifact whack-a-mole** (peter-panning, acne, cascade seams): this risk
  arrived, and the fifth through eighth decisions above are the record of
  fighting it — bias denominated in tile texels, slope read off the geometric
  normal rather than the shading one, the slope term moved off the light's axis
  onto the receiver's own normal, the cascade switch itself blended over a band
  rather than taken as a step, the box filter under all of it replaced by a
  rotated disc, and that disc's width taken from a blocker search rather than
  held constant. Stable snapping and the golden frames did the work; the debug
  overlay that was supposed to help is still unbuilt. What is left is the fringe
  the seventh decision traded lantern's wall-foot strip for, at one silhouette,
  which `docs/backlog.md` still carries.
- **Post-stack perf in a browser**: each pass is simple, but measure — the horde
  web demo budget (S3) includes the stack. The quality pass adds passes to it:
  SMAA 1x is three where FXAA is one, and a Hi-Z march builds a pyramid before
  it walks one.
- **An AA rung re-blesses the suite, and there is no additive-zero form of it.**
  The probe and bloom slices could land switched off and move nothing; FXAA
  moves every edge in every frame the bit is on for. So the risk is not the
  filter, it is deciding once which goldens carry it and re-blessing them once
  rather than a scene at a time.
- **A Hi-Z pyramid puts a reduction underneath the reflection goldens.** Every
  level is float arithmetic four rasterisers perform independently, where the
  fixed-stride march read the prepass directly. The SSR section's standing rule
  — structural ratios rather than tolerances, and never a per-driver re-bless —
  is what had to absorb it, and it was written before this rung was scheduled.
