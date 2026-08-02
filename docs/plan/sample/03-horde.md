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

**Slices 18a and 18b have landed** (`apps/horde`). One arena, one player with
WASD movement and a gun that aims itself, three enemy kinds with seek plus
separation, contact damage, hit points, death and restart; `.crpix` art through
`SpriteRenderer` with `SampleMode::Pixel`; XP gems that drop where an enemy died
and a "pick 1 of 3" level-up from a fixed pool of six upgrades; pause, level-up
and death menus over the shared `crcbl_render::menu` art, with the debug panel
on. 90 tests.

**The art is two sheets and that is the sample's own decision.** Everything
numerous — the player, all three enemy kinds and the gems — is in one
`assets/actors.crpix` at one frame size (34 texels, which is the brute's
collider box at 20 texels a unit), so the whole field is a single
`SpriteRenderer` batch **whatever order it is emitted in** and `art::Scene`
needs no grouping pass over the crowd. Asteroids has three rock sheets and has
to emit largest-first to hold three batches; a field of ten thousand walked in
the order the game holds it would be ten thousand. The shot is the only second
sheet, because it is 8 texels and would otherwise be drawn in a quad twenty
times its own area. The price is the transparent margin round the small kinds —
a runner is 13 texels of art in a 34-texel quad — and it is bounded by the
screen rather than by the horde. 18c measures both halves.

**The level-up freezes the field**, and the freeze is simulation state rather
than a loop pause: the choice changes what the simulation does, so a seeded
script has to replay it. `GameState::LevelUp` short-circuits `run_tick` and
`freeze_field` writes a zero velocity to the player, every enemy and every bolt
**once**, on the tick the screen opens — so nothing moves for as long as it is
up and no branch is added to the hot path. Bolts keep their velocity so it can
be handed back; enemies do not need to, because `steer_enemies` writes them a
fresh one on the first tick after.

Still owed:

- **18c — scale, measurement and the web demo.** The numbers below, done
  properly, now including batch count and the fill cost of the shared frame; the
  browser build and its Pages entry; the profiler capture the exit criteria ask
  for.

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
- **The render side is not in these numbers at all.** It is the instanced sprite
  path now rather than the `DrawList` placeholder, and the draw cap the
  placeholder needed is gone — what is left in front of it is a CPU view cull,
  which is itself `N` comparisons a frame. Nothing has drawn more than a few
  hundred enemies at once. The exit criteria's "60 fps render" is untouched by
  any of this.
- Two named, un-taken wins sit in front of the tick number, both recorded in
  `docs/backlog.md`: `PhysicsSystem::overlap_sphere` returns an owned `Vec`, so
  separation allocates `N` times a tick, and `PhysicsSystem` has no `body_mut`,
  so writing a velocity is a full `HashMap` insert.
