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

## Where this stands

**Milestones 1 and 2 are built.** `apps/asteroids` has the ship that turns,
thrusts and wraps, bullets that sweep, rocks in three sizes that split twice,
waves that grow, and score / lives / game over / restart — with `.crpix` art
baked by `build.rs` and drawn through `SpriteRenderer` with `SampleMode::Pixel`,
start / pause / game-over menus, rule 4's debug panel, three spatial cues (the
engine, the gun, a rock coming apart) with the listener at the camera, and a
best score in the platform config directory or the browser's OPFS.
`web/demos/asteroids/` is its page and `asteroids` is a row in `web/build.sh`'s
`DEMOS`.

**This was the first sample where a drawn thing turns**, and answering that took
a decision the earlier samples never had to make: an angle integrated per tick
and drawn per frame stutters, and an angle wraps, so the renderer interpolates
it the short way round. `lerp_angle` in `apps/asteroids/src/game.rs` carries the
argument.

**Milestone 3 — tuning constants from a data file — is not built.** The
constants are still in `apps/asteroids/src/game.rs`. That milestone is written
as "(After stage 6)" and there is no `.scn/` directory anywhere in this tree, so
it is waiting on the asset stage rather than on this sample.

The soak's half of the exit criteria has a home in the suite:
`hundreds_of_spawns_and_deaths_leak_nothing` asserts entity and pool counts
return to baseline. What has not been run is the **10-minute** scripted soak the
criterion actually names, and the inspector-open stale-handle session has no
inspector to open — that is a debug-tools gap, not this sample's.

## Exit criteria

- 10-minute soak with input script: entity/pool counts return to baseline
  between waves (leak check, asserted in the determinism harness run).
- No stale-handle panics across a full session with inspector open and entities
  selected as they die.
- Same one-sitting code-size bar as breakout (~700 lines target).
