# Sample 03 — horde

Survivors-lite: one player, auto-firing weapon, thousands of enemies converge,
survive N minutes. The renderer-at-scale sample — the stage 3 "flat CPU cost"
claim, demonstrated as gameplay instead of a synthetic sandbox scene.

## Proves

- GPU-driven pipeline under gameplay conditions: 5–10k live enemies +
  projectiles, all through instance deltas + GPU culling + indirect draws.
  Synthetic sandbox scenes are static-ish; horde adds per-tick movement of
  everything — the dirty-range delta upload path gets its real workload.
- Server tick at scale: the ECS SoA claim (linear iteration, cache-friendly)
  measured with 10k-entity systems. If the server can't tick 10k simple agents
  at 60 Hz, that's a stage 4 finding.
- Replication at scale: snapshot size/bandwidth with 10k entities forces the
  question interest management answers post-MVP — this sample produces the
  numbers that justify (or defer) that work.
- Profiler HUD as the primary dev instrument: this sample is built _by watching
  the profiler_, and its doc records the measured budgets.
- **Physics slice it drives** (interleaved build): broadphase + overlap queries
  at 10k-body scale — batch query API, BVH refit cost, island/ sleeping
  pressure. Physics perf numbers recorded here alongside render numbers.

## Scope

- One arena (flat plane + props), one player, WASD + auto-aim weapon.
- Enemies: 2–3 types, dumb seek movement + separation (cheap flocking-lite, no
  pathfinding), contact damage, HP, on-death despawn. Separation neighbors +
  contact damage = `crcbl-phys` broadphase overlap queries; player weapon =
  segment/swept CCD.
- XP pickups + one "pick 1 of 3" level-up choice screen (exercises game UI
  mid-session) — but a small fixed upgrade pool.
- Timer, kill count, death screen. 5-minute survival target.
- **`.crpix` sprites for the player, the enemy types and the pickups** (sample
  rule 11), and **the debug panel on** (rule 4) — which this sample needs more
  than any other on the ladder, because its whole claim is a flat CPU cost at
  10k instances and the panel's frame-timing module is where that is read. The
  arena and props follow whatever P9's asset path provides; the actors are
  sprites. Ten thousand enemies drawn from a handful of sheets is also the first
  time `SpriteRenderer`'s per-sheet batching is under real pressure, which is a
  finding either way.

## Non-goals (hard cap)

Meta-progression, many weapons/characters, bosses, terrain, pathfinding,
particles beyond reused debug-draw-style primitives. This is a benchmark wearing
a game costume — keep the costume thin.

## Milestones

1. 1k enemies seeking player, culling stats on HUD (stage 6-ish start, scene
   from file).
2. Combat loop complete (damage, XP, level-up UI) — after stage 7.
3. Scale push: raise counts until a budget breaks; file engine findings; record
   the numbers in this doc.

## Exit criteria

- 10k enemies at 60 fps render / 60 Hz tick on the reference Linux machine
  (numbers recorded here, revisited per backend in stages 9–10 — Tier B/wasm
  gets its own smaller recorded budget).
- CPU frame time demonstrably flat 1k → 10k on the render side (profiler capture
  archived in the doc).
- Playable and mildly fun for 5 minutes — fun is not the goal but "not obviously
  broken as a game" is.

## Where this stands

**Slice 18a — the core loop — has landed** (`apps/horde`). One arena, one player
with WASD movement and a gun that aims itself, three enemy kinds with seek plus
separation, contact damage, hit points, death and restart, drawn as untextured
quads through the UI pass with the debug panel on. 59 tests.

Still owed, in the order the sub-slices take them:

- **18b — art and progression.** `.crpix` sprites for the player, the three
  enemy kinds and the pickups (rule 11); XP pickups and the "pick 1 of 3"
  level-up screen; the sprite pass replacing `app::draw_field`.
- **18c — scale, measurement and the web demo.** The numbers below, done
  properly; the browser build and its Pages entry; the profiler capture the exit
  criteria ask for.

## Early scale signal (provisional, not the exit measurement)

Taken during 18a because it was one command, **not** as the milestone-3 scale
push, which is 18c's. Read it as a bound on the shape of the problem rather than
as a result.

Conditions: `cargo test --release`, headless, **simulation only — nothing is
rendered**, single-threaded (there is no `crcbl-jobs` and no parallel schedule),
AMD Ryzen 9 9950X3D. `N` grunts staged on a 1.25-unit grid — which is the
spacing separation itself settles at, so the neighbourhoods are the ones the
real game produces rather than empty — then 60 ticks timed and averaged.

| enemies | ms/tick | µs/enemy |
| ------: | ------: | -------: |
|     500 |   0.418 |     0.84 |
|   1 000 |   0.619 |     0.62 |
|   2 000 |   1.307 |     0.65 |
|   5 000 |   3.848 |     0.77 |
|  10 000 |  18.433 |     1.84 |

What that says, and what it does not:

- **Per-enemy cost is flat from 1k to 5k** (0.62 → 0.77 µs) and then **triples
  by 10k**. The tick is `N` broadphase queries plus `N` `HashMap` writes, so a
  flat region and then a cliff is the shape of a working set leaving cache, not
  of an algorithmic change — but nothing here profiled it, so that is a
  hypothesis and 18c's job to confirm.
- **The 60 Hz tick budget is 16.67 ms**, so on this machine the simulation alone
  carries somewhere around 8–9k before it misses, and 10k misses by about 10%.
  That is the plan's target within striking distance **without P8** — which is a
  better position than the roadmap assumed, and the reason 18c is worth doing
  before `crcbl-jobs` rather than after.
- **The render side is not in these numbers at all.** The placeholder emits one
  `DrawList` quad per visible enemy through the UI pass' per-frame vertex
  upload, which is the opposite of the instanced path the 10k claim rests on;
  `app::MAX_DRAWN_ENEMIES` caps it at 2 000 so the two numbers stay separable.
  The exit criteria's "60 fps render" is untouched by any of this.
- Two named, un-taken wins sit in front of the tick number, both recorded in
  `docs/backlog.md`: `PhysicsSystem::overlap_sphere` returns an owned `Vec`, so
  separation allocates `N` times a tick, and `PhysicsSystem` has no `body_mut`,
  so writing a velocity is a full `HashMap` insert.
