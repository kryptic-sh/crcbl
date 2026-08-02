# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

## Owed, with a phase attached

The S1B findings in `docs/plan/ROADMAP.md` are the substantive list — six places
two unrelated games were pushed into the same workaround, each with the phase
that owes the fix. **Finding 1 is closed** (P4B bought `SpriteRenderer` and both
samples now draw through it); five stand. They are not repeated here; this file
carries what has no phase yet.

- **The browser entry point should be written once, before S2.** Finding 2 in
  that list. `apps/flappy/src/web.rs` and `apps/breakout/src/web.rs` are the
  same file with a different symbol prefix, and asteroids will be the third.
  What it would take: a crate (or a `crcbl` module) owning the `Stage` state
  machine, the log queue and the `prepare`/`boot`/`frame`/`status`/`shutdown`
  protocol, with the sample supplying only its prefix and its two loop types.
  The prefix has to stay per-sample — two demos can be open in one browser and
  the symbols must not collide — so the shape is probably a macro over a generic
  core rather than a plain function.

## The sprite system, and what is left of the retrofit

The pipeline is joined up end to end for both games: each of `apps/flappy` and
`apps/breakout` authors `.crpix` text under `assets/`, a `build.rs` bakes it,
and an `art::Scene` draws it through `SpriteRenderer` on a layer stack. What is
left:

- **`NineSliceSource` has no texels → units scale, and there are now two callers
  working around it.** `expand` takes its insets as target units directly, so a
  6-texel cap is 6 units tall whatever the caller's world is. Flappy scaled its
  whole sprite plane by `art::TEXELS_PER_UNIT` = 20 so its pipe's cap would
  survive a 12-unit playable band; breakout hit the identical wall on
  `assets/field.crpix`, whose 10-texel walls would otherwise be ten world units
  thick inside a court 28 across, and reached the same convention at a scale of
  10 — chosen independently, from the ball rather than from the pipe, which is
  the only part that did not copy.

  **This was the "if a second game hits this" condition, and it has been hit.**
  It was not fixed in that slice because the slice was scoped to `apps/*` and
  the change is in `crcbl-render`. The fix: a scale on `NineSliceSource` — a
  `texels_per_unit: f32` field set at `from_sheet` time, or an
  `expand_scaled(target, scale)` beside `expand` — so `minimum_size` and the
  fixed bands come back in the caller's units and a game whose world is not one
  unit per texel does not have to scale its camera to compensate. Both
  `art::TEXELS_PER_UNIT` constants and both `gpu::projection` multiplications
  come out when it lands; the sprite rectangles stay as they are, because those
  were never the problem. Nothing else in the workspace calls `expand` —
  `crcbl-render`'s own `button_skin` does, and would take the same scale of 1.

- **The bake half of `build.rs` is written twice.** `apps/flappy/build.rs` and
  `apps/breakout/build.rs` differ in their `ASSETS` array and in nothing else:
  the same parse → bake → write → generate-a-table loop, the same `ART_TICK_HZ`,
  the same `cargo::error` reporting. Asteroids will be the third. The fix is a
  real entry point in `crcbl-sprite` — something like
  `bake::bake_dir(manifest_dir, out_dir, &stems, tick_hz)` returning the table
  text — because a build script can depend on a workspace library and that is
  the only shape that removes the copy rather than moving it.

- **The tick rate the art is baked at is written twice per game** —
  `ART_TICK_HZ` in `apps/*/build.rs` and again in `apps/*/src/art.rs`. A build
  script cannot `use` the crate it builds, and the sidecar's durations are
  milliseconds, so the two conversions have to agree. Guarded rather than
  solved: each game's `the_art_bakes_to_the_sheets_it_declares` asserts an
  authored hold in ticks survives the round trip. **Breakout's guard is weaker
  than flappy's**, because nothing breakout draws is animated: it can only
  assert the default hold of 1 tick, which survives a fairly wide range of wrong
  rates. It gets real the moment breakout has a clip. Folding it into the
  `bake_dir` entry point above would close it outright.

- **Breakout's paddle is a plain frame, not a nine-slice.**
  `game::PADDLE_HALF_WIDTH` is a `const` and nothing shadows it, so the paddle
  is 10 world units across on every tick of every run and a stretch would have
  had no caller. If a widening power-up is ever added, `assets/paddle.crpix`
  wants `nine: 12 12 0 0` and `art::paddle_rect` already produces the target
  rectangle `expand` would take.

## Coverage gaps

- **Flappy's swept-sphere collision is exercised, not demonstrated.**
  `game::fatal` sweeps the bird's path with `PhysicsSystem::sweep_sphere`
  because that is the correct query, but at this game's speeds a point test at
  the end of the tick catches every pipe the swept one does — measured, down to
  a tick rate of 3 Hz, where the bird still ends each tick inside the pipe it
  would have tunnelled through. Tunnelling needs a step wider than a pipe plus a
  bird (2.3 units, so under about 2.6 Hz), which is not a rate this game is
  coherent at. Closing it honestly means a faster consumer, not a contrived test
  here.
- **No golden image covers the play field's framing, in either sample.** The
  browser gate reads the canvas back and asserts it is neither blank nor still.
  Flappy's `gpu::the_playable_band_is_on_screen_at_every_aspect_ratio` now puts
  the world through the real view-projection at five aspect ratios, which is
  stronger than the hand-written mapping it replaced, and is still not a pixel
  check that would catch the framing drifting.
- **Nothing has looked at either sample's art come out of a GPU.** Every test
  over it is `Sheet` data, sprite rectangles and layer membership; the pictures
  in both retrofit reports were composited in software from the same sprite
  lists, so they say the scenes are assembled correctly and nothing about the
  shader, the sampler or the blend. `crcbl screenshot` cannot help — it renders
  the sandbox cube through `ForwardRenderer`, which **neither** sample uses now.
  Closing this means either a golden through the sprite pass with a sample's own
  sheets, or an offscreen path the samples can drive. It is the same gap for
  breakout as for flappy and is not worth two entries.
- **The bird sprite is not checked against the bird collider.** It is drawn 0.8
  world units across against a `2 * BIRD_RADIUS` of 0.7, deliberately, and
  nothing asserts the relationship — so art that grew to twice the collider
  would look wrong and pass. Breakout closed the equivalent gap
  (`every_sprite_covers_the_collider_it_stands_for`, which holds all forty
  bricks, the paddle and the ball to their colliders exactly); flappy's is
  harder only because the bird is deliberately _not_ exact, so the assertion has
  to be a stated ratio rather than an equality.

- **Nothing checks breakout's clear colour against the court's interior.**
  `art::SURROUND` and `field.crpix`'s `f` are two hand-written sRGB values that
  have to read as different surfaces; a change to one is invisible to the tests.
  Low stakes — it is a look, not a behaviour — but it is the one number in
  breakout's art with nothing holding it.
- **The changelog starts mid-project.** `CHANGELOG.md` covers changes from
  2026-08-01 onward; everything before it is in `git log` only. Worth doing at
  the first tagged release, or not at all — there are no releases yet for a
  reader to be missing entries from.

## Deferred decisions

Questions that came up mid-slice and were answered by judgement rather than by
asking. Each is the question, the answer taken, and **what would change it** —
because the point is that a later reader can reopen one cheaply instead of
rediscovering that it was ever a question. An entry here is not a complaint
about the answer; most of these are probably right.

Distinct from _Considered and declined_ below, which is for ideas rejected on
their merits and expected to stay rejected. These are answers taken under
uncertainty.

- **Does a looping ping-pong replay its end frames?** Taken: **no.** A looping
  ping-pong's period is `2n - 2` — four frames run `0 1 2 3 2 1` and then `0`
  again — while a one-shot is `2n - 1`, precisely because that trailing `0` is
  no longer the next cycle's first, and an out-and-back that stopped on frame 1
  would look truncated. `Clip::steps` and `Clip::step` in `crcbl-sprite`.
  _Changes it_: art that wants a beat held at an end — a wing pausing at the top
  of its stroke. Today that is spelled by giving the end frame a longer `hold`,
  which works and is per-frame; if it turns out to be the common case rather
  than the exception, the answer is a hold on the return leg, not a global flag.

- **Does `reverse` reverse a clip's holds too?** Taken: **no** — a hold belongs
  to the frame it holds, not to the position in the list, so a reversed clip's
  tick pattern is the forward one read backwards. Documented on `Clip::step`.
  _Changes it_: a consumer wanting the reversed clip to have the same
  tick-by-tick timing _profile_ as the forward one (slow-then-fast staying
  slow-then-fast). Nothing has asked, and the current rule is the one that makes
  a frame's timing a property of the frame, which is easier to author against.

- **What does a nine-slice do when the target is smaller than its corners?**
  Taken: **the fixed bands shrink in proportion and the stretched band
  vanishes.** `NineSliceSource::expand`. The two alternatives were both worse:
  _refusing_ — emitting nothing, or clamping the target up to the minimum —
  makes a pipe squeezed below its caps either disappear at one size and not
  another or spill outside the rectangle it was handed; _letting the corners
  overlap_ inverts the middle band, which with no backface culling rasterises a
  mirrored quad rather than nothing, and double-blends the overlap. Shrinking
  keeps the three properties that matter more than corners staying literally
  fixed at a size where they arithmetically cannot: the quads still tile the
  target exactly, nothing is drawn outside it, and the picture is continuous —
  at exactly the minimum size this path and the ordinary one agree. _Changes
  it_: a caller that would genuinely rather draw nothing than draw squashed
  corners. That is `NineSlice::fits_in` at the call site, not a change here.

- **Should nine-slice edges tile instead of stretching?** Taken: **no tiling
  mode at all.** Two concrete costs: a tiled band is `ceil(extent / inset)`
  quads rather than one, so the instance count stops being bounded by nine and
  starts depending on how big the thing was drawn — a pipe stretched to a tall
  gap would quietly become hundreds of instances — and doing it in UV space by
  letting `u1` run past 1 needs a repeating sampler, while `SpriteRenderer` has
  exactly one sampler, `ClampToEdge`, shared by every sheet. _Changes it_: art
  whose edge is a repeating motif that stretching visibly smears — a chain, a
  rope, a brick course. Then it is a new mode with its own quad emitter, not a
  flag on `expand`.

- **Teach the UI pass a second texture, or draw button skins as sprites?**
  Taken: **sprites**, and an older backlog entry that called this "blocked on
  the UI pass sampling a second texture" was deleted as wrong rather than
  satisfied. The UI atlas is a single-channel `R8Unorm` glyph _coverage mask_
  sampled into alpha only — every fragment's RGB comes from the vertex colour —
  and a button skin is RGBA colour art. Routing it through would need a second
  bound image in a second format, a per-quad branch between two samplers, a
  UV-carrying draw command `DrawList` does not have, and an RGB path added by
  hand to both tier permutations of `ui.slang`. `SpriteRenderer` already is an
  instanced RGBA pass with alpha blending, and a skinned button is nine sprites.
  _The cost paid_: the caller owns the ordering. `RenderGraph` runs passes in
  declaration order with no topological sort, and both passes load rather than
  clear, so `SpriteRenderer::add_pass` must precede `UiRenderer::add_pass` or a
  skin paints over its own label — enforced by nothing but the order of two
  lines. _Changes it_: a UI element needing colour art _interleaved_ with text
  rather than behind it, which two passes cannot express at any ordering.

- **A fixed backdrop for breakout, or a parallax band?** Taken: **fixed.**
  _(Moved here from Considered and declined — it is a judgement about this
  game's camera, not an idea rejected on its merits.)_ Breakout's camera never
  moves — the field is fixed and the whole of it is on screen — and `Parallax`
  is `(1 − factor) × camera`, so with a camera at the origin every factor
  produces the same offset of zero. A "distant" layer and a world-locked one
  would be the same picture, and a band that scrolled anyway would be motion the
  player has no reason for. `art::Scene`'s two layers are both `Parallax::WORLD`
  and exist for depth ordering, which is the half of a `LayerStack` that still
  means something here. _Changes it_: breakout gaining a camera that moves — a
  screen-shake on a brick break would be the obvious one, and is currently a
  scope violation under the sample's "no juice" cap.

- **Texels-per-unit: fix the engine, or let each sample pick a scale?** Taken:
  **let the convention stand through P4B, fix it before the third sample.**
  `NineSliceSource::expand` takes its insets as target units directly, so a
  6-texel cap is 6 units tall whatever the caller's world is; flappy scaled its
  sprite plane by 20 and breakout by 10, chosen independently from the pipe and
  from the ball. The reason for deferring was scope — the slice was `apps/*` and
  the fix is in `crcbl-render` — not doubt about the fix. _Changes it_: nothing;
  this one is already owed. The work item, with the shape the fix should take,
  is under **The sprite system** above. Recorded here so the deferral itself is
  on the record and not just its consequence.

- **Commit the baked PNGs beside the `.crpix` text?** Taken: **no.** _(Moved
  here from Considered and declined.)_ It would make the build faster and the
  art reviewable in an ordinary diff, and it would create two sources of truth
  for one picture — the one a reviewer reads being the one that is not loaded.
  `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a build input, and both
  samples' `build.rs` keep it that way. _Changes it_: a build where baking is
  slow enough to be felt, or a review workflow that genuinely cannot read
  `.crpix`. Neither is true today — the baker is a strip blit — and the honest
  fix for the second would be rendering `.crpix` in review, not committing PNGs.

- **What phase are the eleven sprite slices?** Taken: **P4B**, by analogy with
  P4A audio, which was the same shape — a subsystem that was not in the original
  phase table, delivered between numbered phases. It is written into both the
  roadmap's status table and its phase table. _Changes it_: a preference for a
  different label. Nothing depends on the string except the roadmap's own
  cross-references and this file.

- **Where does the debug panel's frame-timing core live in the plan?** Taken:
  **pulled forward out of P10 and built as the next slice**, rather than given a
  new phase number or left at P10. P10 keeps the rest of `07-ui-debug.md`'s
  debug suite and `23-netcode.md`'s netgraph module. The reason is that both
  existing samples want it now and two more are planned before P10, so leaving
  it there would guarantee a third and fourth per-sample HUD — the shape
  `web.rs` already took twice. _Changes it_: a decision to run the whole of P10
  earlier instead, which would make the pull-forward unnecessary.

- **Which samples are exempt from the pixel-art rule?** Taken: **hud, viewer and
  sparks**, on the ground that each one's _subject_ is something other than
  pictures — a widget gallery, the user's own glTF, and a particle workbench —
  so authored sprite art in front of it would be showing the wrong system. hud
  still authors its button skins as `.crpix` because a skinned widget is a
  widget. Every other sample on the ladder is in scope. _Changes it_: a sample
  arguing itself out, which sample rule 11 requires it to do in its own doc with
  a reason.

- **Is `docs/code-review.md` a record or a description of current state?**
  Taken: **a record**, and left unedited except for a line in its header saying
  so. It is dated 2026-08-01, was added in one commit and never amended, and the
  roadmap already says its findings were fixed across eight commits. Several of
  its findings now describe code that no longer exists — the `paddle_model`
  finding is the clearest, since breakout has no forward pass at all. _Changes
  it_: a decision to keep it live, which would mean re-running the review rather
  than patching the findings that happen to have been noticed.

## Considered and declined

- **Sharing `apps/*/src/audio.rs` and the best-score file between the two
  samples directly.** The duplication is real (findings 4 and 5) and the fix is
  in the engine, not in a crate the samples share between themselves: a
  `flappy-and-breakout-utils` would be a third place for the same code to rot,
  and it would hide the evidence that `crcbl-audio` and `crcbl-store` are
  missing a layer.
- **Tinting one brick sprite four ways instead of authoring four frames.** It is
  the cheaper sheet and it is what `app.rs`'s colour table used to do. Four
  frames is what lets the rows differ in their _shading_ — a lit top edge and a
  shaded bottom in each row's own hue — which a single tinted rectangle cannot
  express, and it is what a sprite sheet is for. The cost is 96 × 8 texels
  instead of 24 × 8.
- **Re-randomising flappy's course from a clock.** A restart advances the seed
  deterministically (`course_seed(seed, runs)`) instead. A clock would make the
  course unreproducible, and the sample's exit criterion is that a recorded
  script replays to the same score.
- **Authoring flappy's background bands at one texel per sprite unit.** They are
  drawn at `art::BACKGROUND_SCALE` = 2 instead. At `TEXELS_PER_UNIT` = 20 a hill
  wide enough to read as a hill is a couple of hundred texels of hand-written
  rows for a silhouette with two bumps in it; the pipe is deliberately **not**
  scaled, because its caps are measured in texels and scaling would stretch
  them. If the bands ever gain detail that the doubling makes obvious, redraw
  them rather than adding a second scale knob.
