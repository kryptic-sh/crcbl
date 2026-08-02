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

## The sprite system is an asset pipeline, not a renderer

`crcbl-sprite` describes sheets and converts art; **nothing draws a sprite**.
The crate is referenced by no line outside itself, its only dependency is an
optional PNG codec, and every public function takes text or bytes and returns
text or bytes — none takes a device, a camera, a transform or a draw list. Both
samples still draw solid quads and will until the list below is done.

Stated bluntly because the crate has 52 passing tests, and a green suite over
the half that does not render reads as a working feature.

What exists: the `Sheet` model (frames, clips with direction and loop,
nine-slice insets, sample mode), the `.crpix` format and parser
(`docs/specs/crcbl/pix.md`), baking to PNG + Aseprite JSON, and an
images-to-`.crpix` converter.

In dependency order, each item blocked by the one above it:

1. **RGBA texture upload through the HAL.** The engine has never uploaded one.
   `crcbl-render`'s `upload_texture_r8` is single-channel and written for the
   glyph atlas; a sprite needs RGBA8 and more than one texture bound at a time.
2. **Loading.** PNG decode, and the Aseprite JSON _reader_. §7 of the spec
   specifies what is written; nothing reads it back, so a baked sidecar is
   currently write-only.
3. **`SpriteRenderer`** — an instanced orthographic pass with its own
   `sprite.slang`, alpha blending and both sample modes in the shader. The
   largest single piece. It is also the fix for S1B finding 1: both games push
   their worlds through the UI pass because `ForwardRenderer` draws one
   instance, and a quad-instance pass is the instance path the roadmap's locked
   decision asks for rather than a second 2D renderer.
   - `SampleMode::Pixel` is **not** nearest-neighbour. At non-integer scale
     nearest makes some art pixels four screen pixels across and their
     neighbours five, and the unevenness crawls as the sprite moves. The
     intended implementation samples linearly with the UV bent so the blend
     happens only inside a one-fragment band at each texel boundary, plus
     snapping the quad's screen rect to whole device pixels. Neither exists.
   - The pinned `slangc 2026.14` is not installed here but does reproduce every
     committed artifact byte-for-byte, so authoring a new shader is possible —
     see `crates/crcbl-shaders/tools/compile-shaders.sh` for the install line.
4. **Clip playback.** Advancing over ticks, what ping-pong does at the ends,
   one-shots holding their last frame. `Clip` has no `advance`.
5. **Layers and parallax.** Nothing is designed. Note that Aseprite's
   `meta.layers` is about _compositing layers within one sheet_, which is not
   the same thing as a parallax layer a sprite is drawn on; §9 of the spec flags
   that they have been conflated nowhere yet and should not be.
6. **Nine-slice geometry.** The insets are stored and validated; turning them
   into nine quads with the corners fixed and the edges stretched on one axis is
   not written. This is what the flappy pipes need.
7. **Nine-slice buttons in `crcbl-ui`.** Blocked on 6, and on the UI pass being
   able to sample a second texture.
8. **Retrofit flappy, then breakout**, so the samples stop being solid blocks.

Also owed, and small: the **`crcbl crpix` CLI subcommand** wrapping
`crcbl_sprite::trace`. The library half is done and tested; the argument parsing
in `crcbl-cli/src/args.rs` is not written.

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
  browser gate reads the canvas back and asserts it is neither blank nor still,
  and `app::WorldToScreen` is asserted to keep the field on screen at five
  aspect ratios. Neither is a pixel check that would catch the framing drifting.
- **Nothing in `crcbl-sprite` is tested against a real image or a real
  renderer.** Every test is text in, bytes out, or a round trip between the two.
  The PNG tests decode what they just encoded, which proves the encoder agrees
  with the decoder and nothing about whether a GPU can sample the result. The
  first honest check of that is a golden image through the sprite pass, which
  does not exist.
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
