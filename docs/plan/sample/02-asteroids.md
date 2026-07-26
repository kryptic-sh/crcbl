# Sample 02 — asteroids

2D asteroids: ship, thrust/rotate, shoot, rocks split, screen wrap, waves. The
churn sample — entities spawn and die constantly.

## Proves

- Entity lifecycle under pressure: bullets (short-lived, high rate), asteroid
  splits (1 → 2 + destroy), wave respawns. Generational ids, deferred
  destruction sweeps, pool slot recycling all get hammered — leaks and stale
  handle bugs surface here first.
- Replication churn: snapshots where a large fraction of entities changed or
  died every tick. Dirty-set replication (stage 4) gets a worst-case-ish
  workload while entity counts stay small.
- Continuous (not paddle-constrained) movement + rotation through the
  interpolation buffer — rotational interpolation correctness is visible at a
  glance here.
- Debug tools on a moving target: entity inspector on entities that keep dying
  (stale selection handling), debug draw of collision radii.

## Scope

- One ship, wrap-around playfield, sphere collision everywhere.
- Asteroids: 3 sizes, split twice, wave count scales.
- Score + lives + game-over/restart. Keyboard input.
- Rendering: flat-shaded meshes or sprites — whatever stage state provides; this
  sample is not about looks.

## Non-goals (hard cap)

UFOs, hyperspace, power-ups, particles, sound, two-player.

## Milestones

1. Ship flight + wrap + shooting (stage 4 exit ladder, after breakout).
2. Splits, waves, score/lives/states.
3. (After stage 5) tuning constants from a data file — first use of data-driven
   balance outside scenes.

## Exit criteria

- 10-minute soak with input script: entity/pool counts return to baseline
  between waves (leak check, asserted in the determinism harness run).
- No stale-handle panics across a full session with inspector open and entities
  selected as they die.
- Same one-sitting code-size bar as breakout (~700 lines target).
