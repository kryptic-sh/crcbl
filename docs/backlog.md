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
