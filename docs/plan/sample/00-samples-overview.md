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

| #   | Sample                       | Playable after stage | Proves                                                                       |
| --- | ---------------------------- | -------------------- | ---------------------------------------------------------------------------- |
| 01  | [breakout](01-breakout.md)   | 5 (first L0 slice)   | 2D path, minimal ECS, in-memory server loop, game UI, swept-sphere CCD       |
| 02  | [asteroids](02-asteroids.md) | 5 (nicer after 6)    | Entity churn, generational ids, broadphase churn, segment CCD                |
| 03  | [viewer](03-viewer.md)       | 6 (UI after 7)       | Asset pipeline as a _usable tool_, camera, inspector                         |
| 04  | [horde](04-horde.md)         | 6                    | GPU-driven renderer at scale, flat CPU cost claim                            |
| 05  | [towers](05-towers.md)       | 8 (wasm after 10)    | **Flagship**: everything — editor content, co-op multiplayer, browser client |
| 06  | [arena](06-arena.md)         | post-MVP             | Client prediction driver — pulls netcode forward                             |
| 07  | [orbit](07-orbit.md)         | 5 (UI after 7)       | Physics acceptance test: sector space, orbits, drag, CCD, on-rails handoff   |

Order is dependency order, not build-one-then-next: 01–04 and 07 stay tiny
(days, not weeks, each). 05 is the real game and the long-lived dogfood. 06
exists to force post-MVP netcode work and is explicitly not started before MVP
ends. 07 is numbered late but lands with stage 5 — it is the physics stage's
exit-criteria artifact.

## Rules for all samples

1. **Engine API only.** Samples never link `crcbl-vk`/`crcbl-wgpu` directly,
   never reach around the facade. A sample needing a backdoor = engine API gap =
   engine work item, filed and fixed in the engine.
2. **Server-authoritative always.** Even breakout runs client+server over the
   in-memory transport. No sample gets a "simple mode" that bypasses the
   architecture — the architecture is what's being proven.
3. **Scenes from files** (once stage 6 exists): samples load `.scn.ron`, and
   after stage 8 their scenes are maintained _in the editor_. Hand-edited scene
   files after that point are a smell.
4. **Debug overlay on by default in dev builds.** Samples are also the test bed
   for the debug tools.
5. **CI-built, clippy-clean, same bar as engine crates.** Playtest scripts
   (input-script determinism runs from stage 4) where feasible.
6. **Scope charters are hard caps.** Each sample doc lists non-goals; feature
   ideas beyond them go to the flagship (05) or die.
7. **All collision and motion through `crcbl-phys`.** No game-code collision
   math, ever — even breakout's ball reflection is an L0 sweep + contact normal.
   Physics is built **interleaved with the samples, demand-driven**: each sample
   doc names the physics slice it drives (breakout → first L0 sweep vertical;
   asteroids → broadphase churn + segment CCD + first forces; horde → query
   scale; orbit → full L1; towers → CCD vs moving targets, triggers, character
   controller). A physics feature no sample demands is a feature built too
   early.
