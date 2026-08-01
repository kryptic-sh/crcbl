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
- **Physics slice it drives** (interleaved build): first `crcbl-phys` L0
  vertical — box/sphere colliders, swept-sphere TOI, contact normal response.
  Smallest possible physics consumer; the API is designed against this game.
- **First audio consumer**: bounce/brick-break cues through the spatial grammar
  — ball position pans audibly left/right (rule 2 in its simplest form). Also
  the first Pages web demo once P5 lands.

## Scope

- One screen, fixed brick layout (hardcoded pre-stage-6, `.scn/` dir after).
- Ball/paddle/brick collision through `crcbl-phys` L0: ball = swept-sphere CCD
  vs box colliders (never tunnels at high speed — first CCD consumer),
  reflection from contact normal. Paddle = kinematic body. No game-code
  collision math. Speed ramps per hit.
- **The paddle mirrors like everything else; a paddle in motion drags.**
  `game::bounce` reflects off the contact normal, and that is the whole
  behaviour of a paddle standing still. A paddle being driven left or right
  decides which way the ball goes next instead — including turning a ball back
  the way it came, which a rebound off a moving surface would not do. That is
  response policy, not intersection math: the contact still comes from
  `PhysicsSystem::sweep_sphere`.

  The player's control is therefore entirely in _moving_ the paddle, and where
  the ball lands across its width means nothing. An earlier pass had it the
  other way round — outgoing angle from the contact offset — which reads as
  aiming with a bat rather than steering with one.

- **No gravity.** The ball is a dynamic body with no force providers at all — a
  ball that arcs cannot be aimed either, and breakout has never had one. The
  speed ramp is the only thing that changes its speed after a launch.
- 3 lives, score, restart. Keyboard + mouse paddle input.
- Sound: bounce/brick-break cues through the spatial grammar (P4A lands audio
  before this sample — the earlier "no sound" line predated that decision).

## Non-goals (hard cap)

Power-ups, levels, menus beyond start/game-over, juice (particles/screenshake),
local multiplayer. Any of these appearing = scope violation.

## Milestones

1. Paddle + ball bouncing (first playable: stage 4 loop + first stage 5 L0
   slice).
2. Bricks + scoring + lives + states.
3. (After stage 6) layout from scene file. (After stage 8) layout edited in
   editor — breakout becomes the smallest editor round-trip test.

## Exit criteria

- Winnable, losable, restartable. Feels responsive despite interpolation-only
  netcode (paddle latency acceptable at 60 Hz tick; if not, that's a finding to
  document, not hide).
- Deterministic input-script run: same script → same final score hash (rides the
  stage 4 determinism harness).
- High score + settings persist across restarts via `crcbl-store` (topic 14) —
  first persistence consumer, native + browser (OPFS once P5 lands).
- Total game code small enough to read in one sitting (~500 lines target) — this
  sample is the engine's "hello world" documentation.
