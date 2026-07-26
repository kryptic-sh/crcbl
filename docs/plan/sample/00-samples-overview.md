# Sample Projects Overview

Small, complete, playable projects built on the engine. Each sample is the proof
artifact for one or more stage exits — a stage isn't "done" when its sandbox
demo runs; it's done when the sample that depends on it ships.

Samples live in `apps/` alongside `sandbox` and `editor`. They are kept in the
repo, kept building in CI, and kept small — a sample that grows features beyond
its charter gets its scope cut, not the engine bent around it.

## Why multiple small samples instead of one big one

- One big game exercises systems _together_ but hides which system is weak.
  Small samples isolate: if `asteroids` is hard to build, entity churn is the
  problem, not "the engine".
- Each sample doubles as living documentation: "how do I make a 2D game" has an
  answer that compiles.
- The ladder gives every stage a playable milestone — motivation and regression
  suite in one.

## The ladder

Sample numbering **is build order** (see [../ROADMAP.md](../ROADMAP.md) —
S-phases). Every sample starts only when all its engine dependencies exist,
ships with spatial audio, and (except viewer) publishes as a wasm demo on the
GitHub Pages site.

| #   | Sample                       | Roadmap gate           | Proves                                                                                           |
| --- | ---------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------ |
| 01  | [breakout](01-breakout.md)   | S1 (P0–P4A)            | 2D path, minimal ECS, in-memory server loop, game UI, swept-sphere CCD, panning audio            |
| 02  | [asteroids](02-asteroids.md) | S2 (P5–P6)             | Entity churn, generational ids, broadphase churn, segment CCD, first forces                      |
| 03  | [horde](03-horde.md)         | S3 (P7–P8)             | GPU-driven renderer at scale, flat CPU cost claim, 10k-body queries                              |
| 04  | [hud](04-hud.md)             | P4 skeleton → P10 done | Pure UI demo: CSS HUD + widget gallery + themes; the UI system's living fixture                  |
| 05  | [viewer](05-viewer.md)       | S4 (P9–P10)            | Asset pipeline as a _usable tool_, camera, inspector panels                                      |
| 06  | [orbit](06-orbit.md)         | S5 (P11)               | Physics acceptance test: sector space, orbits, drag, CCD, on-rails handoff                       |
| 07  | [towers](07-towers.md)       | S6 (P12–P13)           | **Flagship**: everything — editor content, co-op multiplayer, browser client, esports audio cues |
| 08  | [arena](08-arena.md)         | post-MVP               | Client prediction driver — pulls netcode forward; audio grammar under fire                       |
| 09  | [puppet](09-puppet.md)       | post-MVP wave 1        | Skeletal animation acceptance test; shadows + device-swap input showcase                         |

01–06 stay tiny (days, not weeks, each; hud is continuous — a P4 skeleton that
grows until P10). 07 is the real game and the long-lived dogfood. 08 exists to
force post-MVP netcode work and is explicitly not started before MVP ends.

## Rules for all samples

1. **Engine API only.** Samples never link `crcbl-vk`/`crcbl-wgpu` directly,
   never reach around the facade. A sample needing a backdoor = engine API gap =
   engine work item, filed and fixed in the engine.
2. **Server-authoritative always.** Even breakout runs client+server over the
   in-memory transport. No sample gets a "simple mode" that bypasses the
   architecture — the architecture is what's being proven.
3. **Scenes from files** (once stage 6 exists): samples load `.scn/` scene dirs,
   and after stage 8 their scenes are maintained _in the editor_. Hand-edited
   scene files after that point are a smell.
4. **Debug overlay on by default in dev builds.** Samples are also the test bed
   for the debug tools.
5. **CI-built, clippy-clean, same bar as engine crates.** Playtest scripts
   (input-script determinism runs from stage 4) where feasible.
6. **Scope charters are hard caps.** Each sample doc lists non-goals; feature
   ideas beyond them go to the flagship (07) or die.
7. **Web demo on the ladder.** Every game sample builds for wasm and deploys to
   the GitHub Pages demo site as part of its exit criteria (viewer exempt as a
   native tool; web build stretch). A sample that breaks the wasm build breaks
   CI.
8. **Sound through `crcbl-audio`, spatial by default.** Positional game events
   use the cue grammar (topic 13) — samples are how players (and we) learn the
   grammar. No sample ships silent after P4A.
9. **All collision and motion through `crcbl-phys`.** No game-code collision
   math, ever — even breakout's ball reflection is an L0 sweep + contact normal.
   Physics is built **interleaved with the samples, demand-driven**: each sample
   doc names the physics slice it drives (breakout → first L0 sweep vertical;
   asteroids → broadphase churn + segment CCD + first forces; horde → query
   scale; orbit → full L1; towers → CCD vs moving targets, triggers, character
   controller). A physics feature no sample demands is a feature built too
   early.
10. **Game logic through the module API** (topic 16). Sample gameplay code
    implements `GameModule` (static binding for dev; breakout also ships as
    `.wasm` for the P6A equivalence gate). Engine-internal systems stay native;
    sample code is module code — proving the API games and mods will live on.
