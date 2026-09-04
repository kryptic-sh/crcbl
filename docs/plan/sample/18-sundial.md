# Sample 18 — sundial (S4D, gates P7B–P7C)

Shadow acceptance test, and the fixture that makes the shadow ladder in
[18-render-features.md](../18-render-features.md) comparable. One scene, a sun
that moves, and every filtering technique the engine ships drawn from the same
frame.

**This is the sample that catches the artefacts a still frame hides.** Shadow
quality is mostly a set of failure modes with names — acne, peter-panning,
cascade seams, swimming edges, a penumbra that is the same width at every
distance — and every one of them is either invisible in a screenshot or
invisible until the light moves. A shadow implementation reviewed from stills is
a shadow implementation whose artefacts ship.

## Proves

- **Every shadow filter the engine ships is selectable live**: the 3×3 hardware
  PCF that ships today, the rotated Poisson kernel, contact-hardening PCSS, and
  — where the device offers it — ray-traced shadows.
- **Each named artefact has somewhere to appear.** The scene is built so that
  acne, peter-panning and a cascade seam each have a surface that would show
  them: a large ground plane at a grazing sun angle, an object resting _on_ that
  plane so its contact point is checkable, and geometry crossing a cascade
  boundary. A demo where the artefacts cannot appear proves nothing about the
  bias.
- **The sun moves, on a clock, and the edges do not swim.** Cascade stability is
  a claim `crates/crcbl-render/src/shadow.rs` makes in prose — a sphere-fitted
  cascade snapped to whole texels — and a moving sun is the only thing that
  checks it.
- **Contact hardening is visibly a function of distance.** A penumbra that
  widens with the gap between caster and receiver is the whole point of PCSS, so
  the scene carries objects at several heights above one plane.
- **The tile budget is observable.** The atlas is a fixed grid, a light that
  gets no tile still lights and does not occlude, and a scene with more
  shadow-worthy lights than the budget is what makes that a quality knob rather
  than a cliff. This sample can exceed the budget on purpose and show what
  happens.
- **Cost per technique, per frame**, from the same timing seam the debug panel
  reads.

## Scope

- One exterior scene: a ground plane, a colonnade or similar repeated caster for
  the cascade seam to cross, objects at graded heights above the plane for the
  penumbra claim, and a handful of punctual lights — at least one spot and two
  point lights, because two point lights is exactly what the 2026-08-26
  re-tiling bought and what a third would exceed.
- A sun on a scripted clock, pausable, scrubbable.
- A cascade debug overlay — tinting each cascade — which
  [18-render-features.md](../18-render-features.md) has owed since P7 and which
  nothing else in the tree is going to build.
- An atlas viewer: the shadow atlas drawn to screen, so tile assignment is a
  thing you can look at. **Built 2026-09-04** as
  `crcbl_render::DebugView::ShadowAtlas` — an engine slice, because drawing the
  atlas is a pass in `crcbl-render` rather than anything a sample can do — and
  bound here to `T` and to the pause panel's `ATLAS` row.
- Technique selector and split screen, on sample 17's harness pattern. **Both
  are built as of 2026-09-04** — topic 45's fifteenth decision:
  `r_shadow_filter` selects between `pcss`, `disc` and `box`, and
  `r_shadow_split` puts the console's choice either side of a seam
  `crcbl_render::split::halves` counts, resolved per fragment out of
  `FrameUniforms::shadow_filter` because a scene pass cannot be recorded twice
  under a scissor. The **binding and the panel** landed with `apps/sundial` on
  2026-09-04: `F` cycles the filter, `X` raises and drops the seam, `,` and `.`
  walk it, and the pause panel and the debug overlay both name the filter on
  each side of the line.
- Pages web demo. **Built 2026-09-04** at `/demos/sundial/`, on
  [19-alcove.md](19-alcove.md)'s pattern: the filter, the seam and the sun's
  clock as HTML controls rather than keys, because a seam walked with `,` and
  `.` and a sun stopped with `P` are controls a phone cannot reach.
  `apps/sundial/src/web.rs` exports one symbol per knob and each answers with
  what the engine holds after the write, so nothing on the page keeps a copy.
  What is new against alcove is the clock: a tick is game state rather than a
  console cell, so the sun's two controls go through `crate::sun`'s channel and
  are adopted on the next fixed step.

## Non-goals (hard cap)

Gameplay. A second scene. Any shadow technique topic 18 has not decided on —
virtual shadow maps are refused there with a reason and this sample is not the
place to reopen it. No authoring tools.

**Exempt from sample rule 11**, on lantern's ground. **Exempt from rules 2 and
10**, on the viewer's: no game state, no `World`, no `GameModule`.

## Status: built and published 2026-09-04, milestone 1 complete

**`apps/sundial` exists and CI draws it.** The engine held exactly one shadow
filter until 2026-09-04; it now holds three, selectable at runtime, with a
per-fragment seam that puts any of them beside the one that ships — topic 45's
fifteenth decision — and the sample followed the same day: the plaza
(`src/plaza.rs`), the sun on its scripted clock (`src/sun.rs`), the filter and
seam bindings and panel rows (`src/filter.rs`, `src/menu.rs`), and the golden
suite (`tests/golden.rs`) CI runs on lavapipe. **The browser demo followed the
same day**: `apps/sundial/src/web.rs`, `web/pages/sundial.html` and
`web/demos/sundial/main.js` put the filter, the seam and the sun's clock on the
page as HTML controls, and `web/tools/browser-e2e.mjs`'s `sundial` row presses
each of them and reads what the `[HUD]` line says they did. **Milestone 1's two
diagnostics landed the same day**: the cascade overlay on `C`, and the atlas
viewer on `T`.

What also exists is evidence that the shadow implementation is at the edge of
its current budget: since the 2026-08-26 re-tiling that bought a second shadowed
point light by shrinking every tile, the `cube` browser-path golden fails on
linux and windows — 64 grossly-wrong pixels against a 49-pixel budget, a maximum
channel delta of 216, an ssim of 0.998945 — and the diff is scattered noise
along shadow edges. macOS passes. The trade was taken knowingly and the cost
landed where a demo would have shown it first.

That is the argument for this sample rather than a footnote to it: the tile
resolution, the filter and the bias are three knobs whose interaction nobody can
hold in their head, and the only honest way to set them is to look.

## Milestones

1. **The scene, the moving sun and the shipped filter.** Cascade overlay and
   atlas viewer land here — they are the diagnostic half and they are worth more
   than a second filter. **Complete 2026-09-04.**

   **The scene, the clock and the filter selector were built on 2026-09-04**, as
   `apps/sundial`: the plaza (`src/plaza.rs`), the scripted sun (`src/sun.rs`),
   the filter and seam bindings (`src/filter.rs`) and the golden suite
   (`tests/golden.rs`), which measures the penumbra under three casters at
   graded heights, holds the seam exact to the column, reads the pavement the
   plinth stands on and replays a tick of the clock byte for byte.

   The **cascade debug overlay** landed 2026-09-04 as
   `crcbl_render::DebugView::Cascades`: the shaded picture multiplied by a tint
   per cascade, blended across [45-shadows.md](../45-shadows.md)'s eighth
   decision's band. This sample binds it to `C` and to the pause panel's
   `CASCADES` row.

   The **atlas viewer** landed the same day as
   `crcbl_render::DebugView::ShadowAtlas`: the `D32Float` atlas letterboxed over
   the finished frame, each texel's stored depth as a grey and an amber border
   round every slot holding a map. It is a full-screen pass in `crcbl-render`
   rather than a branch in `mesh.slang`, because the atlas is one image the
   whole frame shares rather than a function of any fragment, and it draws in
   display space after the tonemap so its greys do not move with the exposure.
   `T` and the panel's `ATLAS` row show it.

2. **Normal-offset bias and the acne/peter-panning pair**, checkable at the
   contact points the scene was built for. The bias itself **shipped
   2026-08-28** — `docs/plan/45-shadows.md`'s seventh decision — so what is left
   here is the comparison, not the rung: a scene that shows the pair moving
   against each other as the two counts change, where that decision could only
   measure one fixture's strip and one patch's dots.
3. **Cascade cross-fade**, against the seam the colonnade crosses.
4. **The filter ladder side by side**, with the penumbra-versus-distance claim.
   The filters themselves **shipped 2026-08-28 and 2026-09-04** — the ninth
   decision's rotated disc took the place of the Poisson set this line asked
   for, with the reason in that decision, and the fifteenth made all three rungs
   selectable — so what is left here is the same as milestone 2's: the
   comparison, not the rung. `r_shadow_filter` and `r_shadow_split` are what it
   drives.
5. **Ray-traced shadows**, gated on P7C, and the device clamp.

## Exit criteria

- Every shadow filter the engine ships is reachable, and the sample names any
  the device removed.
- The cascade overlay and the atlas viewer both work, and the cascade overlay is
  the one topic 18 has been owed.
- A scripted sun sweep runs as a determinism check, not merely as a demo.
- A golden per filter, plus one at a grazing sun angle where acne would show.
- Per-technique cost in the debug panel and the headless summary.
- Web demo deployed.
- Rule 12: selected paths reported, a flag forces a lesser one.
