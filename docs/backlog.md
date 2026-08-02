# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

## Owed, with a phase attached

The S1B findings in `docs/plan/ROADMAP.md` are the substantive list — six places
two unrelated games were pushed into the same workaround, each with the phase
that owes the fix. They are not repeated here; this file carries what has no
phase yet.

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

The pipeline is joined up end to end for one game: `apps/flappy` authors
`.crpix` text under `assets/`, a `build.rs` bakes it, and `art::Scene` draws it
through `SpriteRenderer` on three parallax layers. What is left:

- **Breakout is still solid quads.** Its bricks, paddle and ball go through the
  UI pass for the reason flappy's pipes did, and the whole path they need now
  exists. The pattern to copy is `apps/flappy/src/art.rs` plus its `build.rs`;
  the one thing that will not copy verbatim is the sprite-space scale, because
  breakout's field is measured differently.
- **Nine-slice geometry is in world units, at one unit per texel, and there is
  no way to say otherwise.** `NineSliceSource::expand` takes its insets as
  target units directly, so a 6-texel cap is 6 units tall whatever the caller's
  world is. Flappy's playable band is 12 units, so it had to scale the whole
  sprite plane by `art::TEXELS_PER_UNIT` and give the sprite pass a camera in
  those units — which works and is documented, but it is a convention every
  caller now has to reinvent. If a second game hits this, the fix is a texels →
  units scale on `NineSliceSource` (or on `expand`) rather than a third copy of
  the convention. Not done here because one caller is not a pattern.
- **The tick rate the art is baked at is written twice** — `ART_TICK_HZ` in
  `apps/flappy/build.rs` and again in `apps/flappy/src/art.rs`. A build script
  cannot `use` the crate it builds, and the sidecar's durations are
  milliseconds, so the two conversions have to agree. Guarded rather than
  solved: `the_art_bakes_to_the_sheets_it_declares` asserts the authored hold in
  ticks survives the round trip, which is red the moment they drift.
- **Flappy's flap is a free-running loop.** It advances with ticks and never
  looks at the bird's velocity, so the wing does not beat when the player flaps.
  A `Playback::restart` on the flap edge would do it; left out because this
  slice was about the art existing, not about tuning it.

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
- **Nothing has looked at flappy's art come out of a GPU.** Every test over it
  is `Sheet` data, sprite rectangles and layer membership; the picture in this
  slice's report was composited in software from the same sprite list, so it
  says the scene is assembled correctly and nothing about the shader, the
  sampler or the blend. `crcbl screenshot` cannot help — it renders the sandbox
  cube through `ForwardRenderer`, which flappy no longer uses. Closing this
  means either a golden through the sprite pass with flappy's own sheets, or an
  offscreen path the samples can drive.
- **The bird sprite is not checked against the bird collider.** It is drawn 0.8
  world units across against a `2 * BIRD_RADIUS` of 0.7, deliberately, and
  nothing asserts the relationship — so art that grew to twice the collider
  would look wrong and pass.
- **The changelog starts mid-project.** `CHANGELOG.md` covers changes from
  2026-08-01 onward; everything before it is in `git log` only. Worth doing at
  the first tagged release, or not at all — there are no releases yet for a
  reader to be missing entries from.

## Considered and declined

- **Sharing `apps/*/src/audio.rs` and the best-score file between the two
  samples directly.** The duplication is real (findings 4 and 5) and the fix is
  in the engine, not in a crate the samples share between themselves: a
  `flappy-and-breakout-utils` would be a third place for the same code to rot,
  and it would hide the evidence that `crcbl-audio` and `crcbl-store` are
  missing a layer.
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
- **Committing the baked PNGs beside the `.crpix` text.** It would make the
  build faster and reviewable in a diff, and it would create two sources of
  truth for one picture — the one a reviewer reads being the one that is not
  loaded. `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a build input;
  `apps/flappy/build.rs` keeps it that way.
