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

## Status: unbuilt

Nothing exists. One rung ships: `crates/crcbl-shaders/shaders/ssao.slang` and
its depth-weighted blur, gated by a golden. GTAO is owed by
[18-render-features.md](../18-render-features.md), and as with sample 17 a
comparison demo with one technique in it is not a comparison.

The AO-only view mode is the part worth building first regardless of how many
rungs exist, because the occlusion buffer is currently a texture nothing outside
the forward pass ever displays — and a buffer nobody can look at is a buffer
whose defects are found by reading the shader.

## Milestones

1. **The scene, the AO-only view mode, and the shipped technique.** Radius and
   intensity controls land here.
2. **GTAO**, side by side with SSAO, with the silhouette-rim comparison the
   escalation clause cares about.
3. **Bent-normal visualisation**, once GTAO produces one.
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
