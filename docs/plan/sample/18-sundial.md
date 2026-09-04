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
  nothing else in the tree was going to build. **Built 2026-09-04** as
  `crcbl_render::DebugView::Cascades`, and bound here to `C`, to the pause
  panel's `CASCADES` row and — **2026-09-05** — to `__crcbl_sundial_cascades`, a
  button on the page beside the atlas viewer's, and `tests/golden.rs`'s
  `plaza-cascades` frame, which is what draws it in CI.
- An atlas viewer: the shadow atlas drawn to screen, so tile assignment is a
  thing you can look at. **Built 2026-09-04** as
  `crcbl_render::DebugView::ShadowAtlas` — an engine slice, because drawing the
  atlas is a pass in `crcbl-render` rather than anything a sample can do — and
  bound here to `T`, to the pause panel's `ATLAS` row and to a button on the
  page, with `tests/golden.rs`'s `plaza-atlas` frame drawing it in CI.
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
  [19-alcove.md](19-alcove.md)'s pattern: the filter, the seam, the atlas viewer
  and the sun's clock as HTML controls rather than keys, because a seam walked
  with `,` and `.`, an atlas put up with `T` and a sun stopped with `P` are
  controls a phone cannot reach. `apps/sundial/src/web.rs` exports one symbol
  per knob and each answers with what the engine holds after the write, so
  nothing on the page keeps a copy. What is new against alcove is the clock: a
  tick is game state rather than a console cell, so the sun's two controls go
  through `crate::sun`'s channel and are adopted on the next fixed step.

## Non-goals (hard cap)

Gameplay. A second scene. Any shadow technique topic 18 has not decided on —
virtual shadow maps are refused there with a reason and this sample is not the
place to reopen it. No authoring tools.

**Exempt from sample rule 11**, on lantern's ground. **Exempt from rules 2 and
10**, on the viewer's: no game state, no `World`, no `GameModule`.

## Status: built 2026-09-04, milestones 1–4 complete

**`apps/sundial` exists and CI draws it.** The engine held exactly one shadow
filter until 2026-09-04; it now holds three, selectable at runtime, with a
per-fragment seam that puts any of them beside the one that ships — topic 45's
fifteenth decision — and the sample followed the same day: the plaza
(`src/plaza.rs`), the sun on its scripted clock (`src/sun.rs`), the filter and
seam bindings and panel rows (`src/filter.rs`, `src/menu.rs`), and the golden
suite (`tests/golden.rs`) CI runs on lavapipe. **The browser demo followed the
same day**: `apps/sundial/src/web.rs`, `web/pages/sundial.html` and
`web/demos/sundial/main.js` put the filter, the seam, the atlas viewer and the
sun's clock on the page as HTML controls, and `web/tools/browser-e2e.mjs`'s
`sundial` row presses each of them and reads what the `[HUD]` line says they
did. **Milestone 1's two diagnostics landed the same day**: the cascade overlay
on `C`, and the atlas viewer on `T`; **2026-09-05** gave the overlay the golden
and the page button the viewer already had, and closed milestones 2, 3 and 4 —
the acne/peter-panning pair on the two bias counts, the colonnade's shadow
across the cascade split, and the whole filter ladder beside the shipped rung.
What is left is milestone 5, which is gated on P7C.

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
   `CASCADES` row, and **2026-09-05** gave it the other two routes the atlas
   viewer has: `__crcbl_sundial_cascades` and a button on the page, so a visitor
   with no keyboard can put it up, and `tests/golden.rs`'s `plaza-cascades`
   frame, so CI draws it. Two readings stand beside that golden — one inside
   cascade 0 and clear of the cross-fade band, one past the split, placed from
   `crcbl::render::Cascades`' own split — because a golden alone cannot say
   which of the tints it is looking at, and the same two places with the overlay
   off are the control that says the ordering is the overlay's and not the
   plaza's own colour.

   The **atlas viewer** landed the same day as
   `crcbl_render::DebugView::ShadowAtlas`: the `D32Float` atlas letterboxed over
   the finished frame, each texel's stored depth as a grey and an amber border
   round every slot holding a map. It is a full-screen pass in `crcbl-render`
   rather than a branch in `mesh.slang`, because the atlas is one image the
   whole frame shares rather than a function of any fragment, and it draws in
   display space after the tonemap so its greys do not move with the exposure.
   `T`, the panel's `ATLAS` row and a button on the page all show it, and
   `tests/golden.rs`'s `plaza-atlas` frame is what draws it in CI — with the
   amber border round the near cascade's cell and the black letterbox asserted
   beside the picture, because a golden alone cannot say which of the greys it
   is looking at.

2. **Normal-offset bias and the acne/peter-panning pair**, checkable at the
   contact points the scene was built for. **Complete 2026-09-05.**

   The bias itself **shipped 2026-08-28** — `docs/plan/45-shadows.md`'s seventh
   decision — so what was left here was the comparison, not the rung: a scene
   that shows the pair moving against each other as the two counts change, where
   that decision could only measure one fixture's strip and one patch's dots.
   **2026-09-04** gave the acne half a reading of its own, where before only
   peter-panning had one. **2026-09-05** made the two counts movable and put the
   two artefacts on one frame.

   The counts are console variables now — `crcbl_render::shadow::r_shadow_bias`
   and `r_shadow_normal_offset`, floats in texels of the cascade a fragment
   landed in, each declaring the constant it replaced as its own default, so no
   golden moved. `Cascades::params` reads the cells rather than the constants,
   which is the whole of the engine half. This sample binds them to `[` `]` and
   `;` `'`, to the pause panel's `BIAS` and `NORMAL OFFSET` rows and to the
   debug overlay's `shadow filter` section. They are **on the page** as well —
   two sliders on `/demos/sundial/` over `__crcbl_sundial_bias` and
   `__crcbl_sundial_normal_offset`, each track spanning the ceiling the engine
   declares, and both counts on the `[HUD]` line, so the browser gate reads a
   drag off the console's cell rather than off the canvas.

   `tests/golden.rs`'s
   `the_two_bias_counts_trade_acne_against_the_plinths_own_contact` is the
   claim. Five arms of one frame at `sun::GRAZING_TICK` — what ships, each count
   at zero, each count pushed — and two readings off each: what share of
   `ACNE_CENTRE`'s block of open pavement is a self-shadowing dot, and the
   **shadow term** at `plaza::PLINTH_CONTACT` and at five stations further along
   the plinth's shadow. What comes out is three claims:
   - **Zero either count and the pavement roughens; the contact does not move.**
     The normal offset at zero takes the block to `41.53%` dots on radv and
     `41.53%` on lavapipe, the constant bias at zero to `3.26%` and `3.23%`,
     against `0.00%` on both as the sample ships — and the contact's term is
     `70.73` on radv and `70.44` on lavapipe on all three arms, to a hundredth.
   - **Push the constant bias and the shadow comes off the plinth.** At 96
     texels the contact's term falls to `6.37` while the pavement past it still
     carries `67.01`, which is peter-panning — a lit gap between a caster and
     its shadow — rather than a shadow that has gone. Under 88 texels the
     contact keeps its shadow outright and past 104 the shadow has left the
     whole visible strip; the fixture's own constants carry the sweep.
     **Eighty-eight is a large count and the plinth is why**: the depth pass
     keeps front faces, so what the map stores along the ray from that contact
     to the sun is the plinth's far face, and a bias has to cross the block's
     whole 1.2 m depth. A thin caster loses its contact at a small count, which
     is `apps/lantern`'s wall and the seventh decision's own fixture.
   - **Push the normal offset twenty times as far and the contact does not
     move.** At 40 texels its term is the shipped one to a hundredth, on both
     adapters, though the frame is a different picture and the shadow's far end
     has begun to go — which is the seventh decision's claim, that a sideways
     move keeps a contact, measured rather than argued. At 44 the contact and
     the pavement beyond it go together, which is a shadow that has gone.

   The sabotage that says the reading has teeth is `Cascades::params` handing
   the shader the constants again instead of the two cells: the four moved arms
   then draw the shipped arm's frame byte for byte and the anti-vacuity
   comparison fires first.

3. **Cascade cross-fade**, against the seam the colonnade crosses. **Complete
   2026-09-05.**

   [45-shadows.md](../45-shadows.md)'s eighth decision made the cascade switch a
   band rather than an edge and measured it on `apps/lantern`'s floor; what was
   missing was a claim on the fixture the colonnade was laid out for.
   `tests/golden.rs`'s
   `the_colonnades_shadow_crosses_the_cascade_split_without_a_step` is it.

   What it measures is the **shadow term** — the frame with the shadow passes
   off, less the frame with them on, so the pavement's own Lambert falloff
   cancels and what is left is what the sun's shadow map did there. Every column
   of the colonnade's shadow is walked at offsets either side of its own edge,
   where the two cascades disagree because their filters are denominated in
   texels of very different sizes, and each walk's readings are binned into
   shells of **distance from the eye**, which is the quantity `sun_visibility`
   selects a cascade by. The claim is then local and scale-free: each walk's
   step between the two shells either side of the split is held to the steepest
   step the same walk shows clear of the band. With the band it reads at worst
   `2.24` against `1.43` on radv and `2.33` against `1.16` on lavapipe; with
   `CASCADE_FADE_FRACTION` set to zero and every artifact regenerated — the band
   collapsed to an edge — the same walks read `17.49` against `1.24` and `17.55`
   against `1.41`, which is the sabotage that says the reading has teeth.

   **2026-09-05 widened it from one arm to three**, each drawn against its own
   split and its own shadow direction. The `disc` rung is read the same way,
   because a filter's width is what differs between the two cascades at a
   vertical caster's shadow and a band held under one filter is not a band held
   under the ladder: `2.98` against `1.43` on radv with the band, `39.24`
   against `4.02` with it collapsed. The grazing sun, `sun::GRAZING_TICK`, puts
   a shadow several times longer through the same window of distance and comes
   back on the other side of the shadow's axis: `0.63` against `1.30` with the
   band, `3.32` against `0.09` without. The `box` rung was measured and is
   **not** read — its walk clear of the band is flat enough (`0.12`/255 on radv,
   `0.04` on lavapipe) that the ratio reads that denominator's noise and comes
   out higher with the band than with it collapsed, so no bound separates the
   two and the bound was left where the arms it does separate put it.
   `plaza::counter_camera` was measured and is not read either: every sample of
   every walk that lands in the shell window falls outside that pose's frame, so
   an arm there has no pair of shells either side of the split and would red the
   suite rather than widen the claim. Both refusals and what closing them would
   take are in [backlog.md](../../backlog.md).

   The walk reads only pavement the arm's camera can see and no lamp reaches,
   which `plaza::hidden_from` and `plaza::lamplit` answer off the plaza's own
   geometry: the colonnade hides much of the floor its shadows fall on from the
   fixed pose, and a walk that did not ask would be reading a column's lit face.

4. **The filter ladder side by side**, with the penumbra-versus-distance claim.
   **Complete 2026-09-05.**

   The filters themselves **shipped 2026-08-28 and 2026-09-04** — the ninth
   decision's rotated disc took the place of the Poisson set this line asked
   for, with the reason in that decision, and the fifteenth made all three rungs
   selectable — so what was left here was the same as milestone 2's: the
   comparison, not the rung.

   The **penumbra-versus-distance claim** is `tests/golden.rs`'s
   `the_penumbra_widens_with_its_casters_height_under_pcss_and_not_under_disc`,
   which was already the whole of it: three cubes of one size hanging at graded
   heights over one plane, so the only thing differing between their shadows is
   the distance from blocker to receiver, and the widths walked in **metres of
   pavement** rather than pixels so the three are comparable. Under `pcss` they
   read 0.0440 / 0.0640 / 0.1080 m, a ratio of 2.455; under `disc` 0.0440 /
   0.0480 / 0.0440, a ratio of 1.000. The `disc` arm is the half that says the
   widening came from the blocker search rather than from the scene.

   The **side by side** is the seam claim,
   `the_seam_runs_the_console_filter_on_the_left_and_the_shipped_one_on_the_right`,
   which held one pair — `disc` against `pcss` — until **2026-09-05**. A rung
   wired to its neighbour's branch is exactly the failure one pair cannot see,
   so it now walks every rung the engine declares: 961 of the 1024 columns exact
   on both adapters for each of them, with `disc` standing 3.110 and 324.498/255
   from `pcss` down the two halves and `box` 26.417 and 363.250.

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
