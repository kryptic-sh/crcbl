# Sample 04 — horde

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
