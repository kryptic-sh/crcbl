# Sample 12 — flappy

One-button side-scroller: a bird under constant gravity, a flap impulse, an
endless procession of pipe gaps, a score, and instant death on contact.
Deliberately the _smallest game the engine can ship_ — smaller than breakout.

> **Numbered 12, built at S1B.** The file number is allocation order; the ladder
> in [00-samples-overview.md](00-samples-overview.md) is build order. This one
> needs nothing that did not already exist at P4A, so it is placed immediately
> after breakout rather than at the end.

## Why this one, and why now

Breakout proved the engine can host a game. Flappy asks a different question:
**can it host a _second_ one without the first one's shape leaking into the
engine?**

That is the same argument this project already paid for twice and both times
found real bugs — the seam that looked complete until `crcbl-wgpu` implemented
it, and the shader that compiled until Dawn read it. A single consumer cannot
tell an API from an accident. Breakout is currently the only game, so every
convenience that happens to suit a paddle and a brick grid reads as good design.
A second sample with a genuinely different shape — continuous scrolling instead
of a fixed field, procedural spawning instead of a static grid, one input
instead of two axes, instant loss instead of three lives — is the cheapest way
to find out which of those are which.

It is also the cheapest possible second demo for the site: the demo page, the
build and the browser gate are all generic already, so this exercises "does the
demo site carry N games" for the price of one small game.

## Proves

- **The engine has no breakout-shaped assumptions.** Any place the API only fits
  a paddle-and-bricks game is a finding, and the finding is the point.
- **Procedural spawn/despawn on a treadmill**: pipes are created ahead of the
  player and destroyed behind it, forever. Slot recycling and generational ids
  under steady churn, at a much gentler rate than asteroids will apply — a
  first, legible workload for the same machinery.
- **A game whose difficulty is a function of time**, so the fixed-timestep
  guarantee is directly observable: at a given seed the same input script must
  reach the same score, and a frame-rate change must not alter difficulty. This
  is the sample where a per-frame integration bug would be _obvious_ rather than
  subtle, which is exactly what makes it worth having.
- **Single-input latency**, felt rather than measured. A flap must land on the
  tick the key arrived; breakout's paddle can hide a tick of delay, a flap
  cannot.
- **Deterministic procedural generation**: gap positions come from a seeded
  generator that is part of the replicated simulation, not from the renderer, so
  client and server agree without sending the pipe list.
- **A second consumer of the UI**: score, best score, and a start/dead overlay —
  the same `crcbl-ui` widgets breakout uses, driven by a different state
  machine.

## Non-goals

- No new engine subsystem. If flappy appears to need one, that is a finding to
  record, not a reason to grow the engine here.
- ~~No art. The same untextured quads breakout draws.~~ **Withdrawn at P4B.**
  This held while it was written and it was the wrong cap: untextured quads were
  what forced both games through the UI pass, which became S1B finding 1, which
  bought the sprite system. Flappy is now `.crpix` art under `assets/` — a bird
  whose three-frame flap restarts on a rising vertical velocity, a three-sliced
  pipe, hills and a ground band on parallax layers — baked by `build.rs` and
  drawn through `SpriteRenderer`. Nothing about how the game _plays_ changed,
  which is the part the cap was actually protecting. See sample rule 11.
- No netcode beyond what breakout already does (in-memory transport,
  server-authoritative tick, interpolated render).

## Milestones

1. Bird under gravity with a flap impulse; the fixed-timestep integration and
   the input edge, and nothing else.
2. Pipes: seeded generation, the treadmill, spawn ahead and despawn behind.
3. Collision and death, score on gap passed, restart.
4. HUD, audio cue on flap and on death, high score through `crcbl-store` — OPFS
   in the browser, as breakout does.
5. Published to the demo site as the second entry in the bar.

## Status: done (2026-08-02)

Built in seven slices; the phase table in [../ROADMAP.md](../ROADMAP.md) lists
them and the findings note there is the deliverable this sample existed for.
Kept rather than deleted because the _argument_ below — why a second game, and
what a second game is evidence of — is what the next sample will be judged
against.

Two things ended up differently from the sketch below. The ceiling stops the
bird rather than killing it, because a lid that kills punishes the safest answer
to a low gap. And a restart advances the course seed rather than replaying the
same hand, deterministically, so both "a new run is a new course" and "a
recorded script replays exactly" hold at once.

**Retrofitted at P4B**, along with breakout: the art non-goal was withdrawn and
the game is drawn as sprites.

**And the debug panel landed, which this section used to record as owed.** Rule
4's panel is on, `F3` toggles it, and `HostedGame::debug_sections` in
`apps/flappy/src/app.rs` contributes two modules and no third: the course, which
is the treadmill's entity churn, and the audio, whose emission counter existed
before the section that reads it. There is deliberately **no network module** —
flappy runs over `InMemoryTransport` and has no connection to report on — and
that absence is the point rather than a gap: it is the check that the panel is
modular rather than assembled around a connection. Breakout is the other half of
that check.

## Exit criteria

- Playable, losable, restartable, native and in a browser.
- **A findings note in the roadmap**, listing every place the engine's API
  resisted a game that was not breakout — even if the list is empty, because
  "empty" is itself the result this sample exists to produce.
- Deterministic: a recorded input script replays to the same score, and the same
  seed produces the same pipes across runs and across native/browser.
- Frame-rate independence proven the way breakout's was — the same script at 20,
  60 and 240 fps reaches the same score, which is the assertion that caught
  three real bugs in breakout.
- Small enough to read in one sitting, and smaller than breakout.
