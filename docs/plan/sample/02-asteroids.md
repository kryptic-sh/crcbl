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
- **Physics slice it drives** (interleaved build): dynamic broadphase (BVH
  insert/remove churn from bullets/splits), sphere overlap queries, segment CCD,
  ship thrust/inertia through the L1 integrator (first force-pipeline consumer:
  thrust + damping).

## Scope

- One ship, wrap-around playfield. All collision via `crcbl-phys`: bullets = L0
  segment CCD (prev→cur, never miss at any speed), ship/asteroid = sphere
  overlap queries against the broadphase. Wrap teleport exercises `WorldPos`
  rebase + broadphase re-insertion (sector-crossing machinery in miniature).
- Asteroids: 3 sizes, split twice, wave count scales.
- Score + lives + game-over/restart. Keyboard input.
- **Rendering: `.crpix` sprites** (sample rule 11). Ship, three asteroid sizes,
  bullets — authored as text under `assets/`, baked by `build.rs`, drawn through
  `SpriteRenderer` with `SampleMode::Pixel`. The earlier "flat-shaded meshes or
  sprites, whatever stage state provides" is superseded: the sprite system
  exists, and asteroids is the first sample that gets to start with it rather
  than be retrofitted onto it. This is also the first real test of rotation
  through the sprite pass — breakout and flappy draw nothing that turns.
- **The debug panel on from the first slice** (sample rule 4), rather than added
  at the end. Asteroids is the first sample built after the panel exists, so it
  is the evidence that "switch it on" is genuinely one thing; if it is not, that
  is a finding about the panel, in the same shape as the S1B findings.

## Non-goals (hard cap)

UFOs, hyperspace, power-ups, particles, two-player. (Sound is _not_ a non-goal —
sample rule 8 requires spatial audio after P4A; thrust/shot/ explosion cues ship
with it.)

## Milestones

1. Ship flight + wrap + shooting (stage 4 loop + stage 5 L0 slices, after
   breakout).
2. Splits, waves, score/lives/states.
3. (After stage 6) tuning constants from a data file — first use of data-driven
   balance outside scenes.

## Exit criteria

- 10-minute soak with input script: entity/pool counts return to baseline
  between waves (leak check, asserted in the determinism harness run).
- No stale-handle panics across a full session with inspector open and entities
  selected as they die.
- Same one-sitting code-size bar as breakout (~700 lines target).
