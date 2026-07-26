# Sample 01 — breakout

First playable game. 2D breakout: paddle, ball, brick grid, score, lives.
Deliberately the smallest thing that is a _game_ and not a demo.

## Proves

- The 2D story: ortho camera, z as z-index, sprites/quads through the same
  instance path as 3D meshes (stage 3 locked decision).
- Minimal ECS in anger: `PaddleSystem`, `BallSystem`, `BrickSystem`,
  `ScoreSystem` — smallest real demonstration of system-owned arrays.
- Server-authoritative shape at its most awkward: even pong-likes run
  client+server over in-memory transport. Input → server tick → snapshot →
  interpolated render. If this feels heavy here, the API needs sugar — that
  finding is the point.
- Game UI minimum: score, lives, start/game-over states via `crcbl-ui`.

## Scope

- One screen, fixed brick layout (hardcoded pre-stage-5, `.scn.ron` after).
- Ball/paddle/brick collision = AABB + reflection. Speed ramps per hit.
- 3 lives, score, restart. Keyboard + mouse paddle input.
- Sound: none (audio is out of engine MVP).

## Non-goals (hard cap)

Power-ups, levels, menus beyond start/game-over, juice (particles/screenshake),
local multiplayer. Any of these appearing = scope violation.

## Milestones

1. Paddle + ball bouncing (stage 4 exit demo candidate).
2. Bricks + scoring + lives + states.
3. (After stage 5) layout from scene file. (After stage 7) layout edited in
   editor — breakout becomes the smallest editor round-trip test.

## Exit criteria

- Winnable, losable, restartable. Feels responsive despite interpolation-only
  netcode (paddle latency acceptable at 60 Hz tick; if not, that's a finding to
  document, not hide).
- Deterministic input-script run: same script → same final score hash (rides the
  stage 4 determinism harness).
- Total game code small enough to read in one sitting (~500 lines target) — this
  sample is the engine's "hello world" documentation.
