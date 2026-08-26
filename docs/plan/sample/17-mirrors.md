# Sample 17 — mirrors (S4D, gates P7B–P7C)

Reflection acceptance test, and the fixture that makes the reflection ladder in
[18-render-features.md](../18-render-features.md) comparable rather than merely
implemented. One scene, one camera, several reflection techniques drawn from the
same frame — and a human has looked at them next to each other.

**This is the sample that stops a technique upgrade being a claim.**
Screen-space reflections have a failure mode nobody notices from a single
screenshot: they are right where the reflected surface is on screen and wrong
the moment it leaves, and every technique on the ladder fails differently at
that boundary. A ladder whose rungs were each blessed alone is a ladder nobody
compared.

## Proves

- **Every reflection technique the engine ships draws the same scene**,
  selectable live and comparable side by side: the fixed-stride screen-space
  march that ships today, the Hi-Z march, roughness by cone tracing over a
  colour mip chain, the planar reflection the render-to-texture camera gives,
  and — where the device offers it — the ray-traced path.
- **The screen-space failure boundary is visible on purpose.** The scene puts a
  reflected object where a camera move takes it off screen, so the moment
  screen-space information runs out is a thing the demo shows rather than a
  thing a bug report discovers. A technique that hides its own boundary is not
  being compared honestly.
- **Roughness is a dimension, not a checkbox.** A row of surfaces from mirror to
  near-diffuse, so the cone-traced rung has somewhere to be obviously better
  than the mirror-only march and the mirror-only march has somewhere to be
  obviously cheaper.
- **Cost is reported per technique, per frame**, off the same timing seam the
  debug panel already reads. "Better looking" and "affordable" are two claims
  and this sample makes both or neither.
- **Degradation is monotonic across the ladder.** Forcing a lesser rung never
  produces a frame that is wrong rather than merely coarser, which is the
  property that lets `[engine.video]` clamp a player's machine downward without
  shipping them a defect.

## Scope

- One interior scene built for reflection and nothing else: a still water or
  polished floor plane, a curved chrome object the planar rung cannot serve, a
  graded roughness row, and one object placed to leave the frustum on a scripted
  camera move.
- A technique selector — live, one key per rung — plus a split-screen mode
  holding two rungs at once, on lantern's A/B precedent.
- A fixed camera set for goldens and a scripted move for the boundary
  demonstration.
- Debug panel modules for the selected technique, its per-frame cost, and the
  device clamp that removed any rung the machine cannot run.
- Pages web demo. The browser runs the screen-space rungs and no ray tracing,
  which is exactly rule 12's point.

## Non-goals (hard cap)

Gameplay. A second scene. Authoring tools. Any reflection technique topic 18 has
not decided on — this sample compares what the engine ships and is not a place
to smuggle in a technique by building its demo first. No denoiser research, and
no physically-measured validation against a reference renderer; the claim here
is comparative, not absolute.

**Exempt from sample rule 11** (`.crpix` art through the sprite pass), on
lantern's ground: the subject is a 3D lighting term and pixel art in front of it
would be showing the wrong system.

**Exempt from sample rules 2 and 10** (server authority, gameplay through
`GameModule`), on the viewer's and lantern's ground: there is no game state. The
scene is fixed, the camera is the viewer's, and this crate opens no `World`.

## Status: unbuilt, and the blocker is the ladder itself

Nothing exists. The gate is not scaffolding — it is that the ladder has one rung
built. `crates/crcbl-render/src/ssr.rs` and
`crates/crcbl-shaders/shaders/ssr.slang` ship the fixed-stride march; Hi-Z, cone
tracing and planar reflection are all owed by
[18-render-features.md](../18-render-features.md), and a comparison demo with
one technique in it is `apps/lantern` with extra steps.

So this sample lands **after the second rung**, and its first milestone compares
exactly two techniques. That is deliberate: a two-way comparison is where the
harness — the selector, the split screen, the per-technique timer — gets built
and proven, and every later rung is then a row added to a thing that works.

## Milestones

1. **Two rungs and the harness.** The shipped march against whichever rung lands
   first, split-screen, per-technique timing, golden coverage for both.
2. **The roughness row and the cone-traced rung.**
3. **Planar reflection through the render-to-texture camera**, which is the rung
   that is not screen-space at all and therefore the one that shows what the
   screen-space rungs are approximating.
4. **The ray-traced rung**, gated on P7C, and the device clamp that removes it.

## Exit criteria

- Every reflection technique the engine ships is reachable from this sample, and
  the sample names any the device removed.
- Split-screen comparison of any two rungs, from one frame's data.
- Per-technique cost in the debug panel and in the headless summary line.
- A golden per rung, plus the scripted camera move as a determinism check.
- Web demo deployed, running the screen-space rungs.
- Rule 12: the selected paths are reported, and a flag forces a lesser one.
