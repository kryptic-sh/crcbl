# Sample 19 — alcove (S4D, gates P7B–P7C)

Ambient-occlusion acceptance test, and the fixture that makes the AO ladder in
[18-render-features.md](../18-render-features.md) comparable. One scene of
corners, creases and contact points, with every occlusion technique the engine
ships drawn from the same frame.

**This is the sample that shows AO is an approximation with a shape.** Ambient
occlusion is the one term in the stack where the wrong answer looks like a style
choice: too much reads as dirt, too little reads as a scene lit by nothing, and
a haloed silhouette reads as a rendering bug in whatever is behind it. Those are
three different defects and none of them announces itself.

## Proves

- **Every occlusion technique the engine ships is selectable live**: no AO, the
  hemisphere SSAO that ships today, GTAO, and — where the device offers it —
  ray-traced AO. A no-AO rung is not a formality; it is the only way to see how
  much of the image the term is responsible for.
- **AO darkens the ambient term and nothing else.** The scene carries a surface
  in direct light inside a crease, so an implementation that darkened direct
  light or a highlight would be visible rather than merely wrong. This is the
  refusal written into topic 18 and this is where it is checked.
- **The silhouette halo is visible or absent on purpose.** Normals are
  reconstructed from depth, which is exact on a plane and wrong on a one-pixel
  rim at every silhouette — the escalation clause topic 18 wrote before it was
  needed. The scene puts a curved object against a far background so that rim
  has somewhere to appear, and the demo is how anyone would know the clause had
  fired.
- **Bent normals are a direction, not a scalar.** Once GTAO lands, the sample
  visualises the bent normal directly, because a term that steers where ambient
  is sampled from cannot be reviewed as a grey image.
- **Radius and intensity are legible knobs.** Both live, both shown, because
  almost every AO complaint in practice is one of the two set wrong rather than
  the technique being wrong.
- **Cost per technique, per frame**, from the timing seam the debug panel reads.

## Scope

- One interior scene of nothing but occlusion geometry: an alcove, a stair
  underside, boxes resting on a floor for the contact-shadow claim, a deep
  crease lit directly, and a curved object silhouetted against distance.
- Flat, untextured or near-untextured surfaces by choice — texture detail is
  exactly what hides an AO artefact, and this is the one sample that should not
  have any.
- An AO-only view mode, drawing the occlusion buffer alone, which is how the
  technique is actually compared. The composited view is how it is judged.
- Radius and intensity controls, live.
- Technique selector and split screen, on sample 17's harness pattern.
- Pages web demo.

## Non-goals (hard cap)

Gameplay. A second scene. Any AO technique topic 18 has not decided on — HBAO is
refused there with a reason. No specular occlusion until topic 18 decides it is
a term of its own; the plan is explicit that a scalar AO is the wrong quantity
for it, and this sample must not quietly imply otherwise.

**Exempt from sample rule 11**, on lantern's ground — and more strongly, since
flat untextured surfaces are the point. **Exempt from rules 2 and 10**, on the
viewer's: no game state, no `World`, no `GameModule`.

## Status: built and published

**`apps/alcove` exists and milestones 1, 2 and 3 are done, 2026-09-05.** The
court is one interior of nothing but occlusion geometry — an alcove with a
recess, a cantilevered stair, boxes and a post on the floor, a slot the sun runs
down, and a sphere on a pedestal against the far wall — and the sun's azimuth,
the fixed camera's eye ray and the slot's axis are deliberately one line, so the
floor at the bottom of that slot is in full sun at any depth and the crease
claim is a claim about a directly lit surface rather than about a shaded one.

The engine half of the first two milestones landed the same day and is described
where it lives: `crcbl_render::ssao`'s `r_ssao_technique`, `r_ssao_radius`,
`r_ssao_intensity`, `r_ssao_split` and `r_ssao_bent_normals`, and
`ForwardRenderer::set_occlusion_view`. The sample drives every one of them by
name through `crcbl::render::console_table()`, which is the same seam a person
typing a console line goes through — so a pause-panel row and a typed line
cannot disagree.

**Milestone 3 is the picture that switch was missing, 2026-09-05.**
`ForwardRenderer::set_bent_normal_view` draws the direction the gather reported
as `n * 0.5 + 0.5`, and the sample reaches it through `crcbl::debug_view` — one
cell, written by `N`, by the panel's `BENT VIEW` row, by the page's own button,
by `--bent-view` on the command line and by a typed `debug_view bent normal`, so
the panel reports which picture is in force rather than a flag of its own. The
flag is a `DebugView` beside `--ao-view` rather than a `bool` of its own, for
the same reason the panel's two rows are: there is one cell, so the last flag on
the line is the one that draws. What holds it is
`the_bent_direction_is_the_normal_on_open_floor_and_leans_out_of_an_enclosure`:
on open floor nothing stands inside the occlusion radius, so the average
unblocked direction is the floor's own normal and the frame draws exactly that;
in the slot, in the recess and on the floor beside a box it leans out towards
the opening, which is the whole of what a direction says and a grey image
cannot. `bent-normal.png` is behind those readings rather than in front of them.

Its anti-vacuity is
`the_bent_direction_view_draws_the_sentinel_grey_where_no_direction_was_gathered`,
which draws the picture there is no direction in: with `r_ssao_bent_normals`
off, and again with the occlusion pass out of the stack, every pixel of the
frame is the grey `crcbl_shaders::ssao::BENT_NORMAL_NONE` encodes to — and the
four blocks above are not that grey on the arm that ships.

What holds it up is `apps/alcove/tests/golden.rs`, run by
`apps/alcove/tests/run-alcove-golden.sh` and by CI's "Draw alcove on lavapipe"
step. Its claims are the ones this document asked for: that occlusion scales the
ambient term and leaves direct light alone, measured as a difference of
differences with the sun switched off rather than as a ratio; that the alcove's
back corner and the contact band under a box darken while open floor does not
move at all; that the two gathers draw different occlusion and both darken the
same corners; that the comparison seam runs the console's gather on the left and
the shipped one on the right **to the column**, outside the blur's own bleed
band; that a wider radius deepens the occlusion; and that a silhouette does not
print onto the wall two metres behind it. Four goldens sit behind them. Every
one was measured on lavapipe and on radv, and every one was watched to fail.

**The browser demo landed the same day**, at `/demos/alcove/`. It is the first
page on the site whose controls are HTML rather than keys, and that is this
sample's own reason rather than a flourish: the seam is the interesting knob to
drive and natively it is walked with `,` and `.`, which a phone does not have.
Each control drives the same `r_ssao_*` variable the key writes, through the
exports `apps/alcove/src/web.rs` adds on top of the shared boot protocol, and
reads back what the console holds afterwards — so there is no second copy of the
state on the page. What a browser cannot carry is the ray-traced rung, and it
cannot for the same reason it is owed everywhere else: WebGPU exposes no ray
query, so the page draws through `LightingPath::Rasterised` by construction and
compares two screen-space gathers.

## What is owed

- **Milestone 4, ray-traced AO**, gated on P7C. `Paths::ray_tracing_note` says
  "raster only (P7C)" on the panel rather than leaving the row out, so the rung
  that is missing is named rather than absent.
- **Per-technique cost, on the second gather.** `OcclusionCost` reads the
  frame's `ssao` and `ssao-shipped` timing rows and prints both on the panel and
  in the headless summary, so the seam gives a cost per technique **when it is
  up**. A cost for a technique the frame did not draw is not something a timing
  seam can report, and the sample does not pretend otherwise.

## Milestones

1. **The scene, the AO-only view mode, and the shipped technique.** Done.
2. **GTAO**, side by side with SSAO, with the silhouette-rim comparison the
   escalation clause cares about. Done, and the rim is a golden of its own from
   a second camera pose: at the fixed camera the sphere is a few dozen pixels
   across and a one-pixel halo is not something a person or a block average can
   see.
3. **Bent-normal visualisation**, once GTAO produces one. Done: `N`, the pause
   panel's `BENT VIEW` row, the page's own button, `--bent-view` and
   `__crcbl_alcove_bent_view` all put `crcbl_render::DebugView::BentNormal` up,
   and `bent-normal.png` is a golden with four readings in front of it — the
   direction is the floor's own normal on open floor and leans out towards the
   opening in the slot, the recess and the box's contact band — with the
   sentinel frame behind them as their control.
4. **Ray-traced AO**, gated on P7C, and the device clamp — and this is the rung
   that gives the screen-space rungs a reference to be judged against, which
   samples 17 and 18 do not get as cleanly.

## Exit criteria

- Every AO technique the engine ships is reachable, and the sample names any the
  device removed.
- AO-only and composited views, switchable.
- A golden per technique, plus one framing the silhouette rim.
- The direct-light-inside-a-crease surface reads correctly on every rung — the
  check that AO scales ambient alone.
- Per-technique cost in the debug panel and the headless summary.
- Web demo deployed.
- Rule 12: selected paths reported, a flag forces a lesser one.
