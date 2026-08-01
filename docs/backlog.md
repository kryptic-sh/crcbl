# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

## crcbl-phys: `sweep_sphere` misses contacts by up to one radius

`PhysicsWorld::sweep_sphere` builds its candidate list with
`Bvh::traverse_segment`, which walks the BVH with the sphere's **centre line**
as a ray (`broadphase.rs`, `traverse_segment` → `traverse_ray`). The narrow
phase (`query::swept_sphere_vs_aabb`) does inflate the target by the radius and
handles a sweep that starts overlapping — but it is only ever called for
colliders the centre line already reached, so a sphere that grazes a box, or
stops short of it by less than its radius, is never offered to it at all.

Verified while writing breakout's paddle-steering test: a ball placed 0.05 units
clear of the paddle's top face and moving down at 11 u/s reported **no hit** on
the tick its surface passed through the face, and kept its velocity exactly. It
only registered once its centre crossed into the paddle's AABB, a tick and a
half later.

Consequences today: a contact resolves up to `radius` late, so breakout's ball
is drawn slightly inside a wall or a brick on the tick before it bounces, and a
genuinely grazing sweep is missed outright.

The fix is small and local — traverse with `Bvh::traverse_aabb` over the
segment's bounds inflated by the radius, then keep the existing narrow phase,
which already rejects the extra candidates. Not done here because `sweep_sphere`
is a shared engine query with consumers beyond this game, and changing what it
reports deserves its own change with its own tests. `apps/breakout` works around
it with `gpu::VIEW_MARGIN`, which keeps the sliver of ball that overshoots a
wall on screen.

## breakout: the ball's speed never ramps

`docs/plan/sample/01-breakout.md` lists "Speed ramps per hit" under scope. The
ball runs at a constant `game::BALL_SPEED` from launch to death — `keep_speed`
puts the magnitude back after every bounce, so a ramp is a single multiplier in
that function plus somewhere to keep the current speed. Deliberately left out of
the bounce-physics fix, which was about removing gravity, not about pacing.

## breakout: the paddle deviates from "reflection from contact normal"

The same plan doc says collision response is "reflection from contact normal"
and that there is "no game-code collision math". The paddle is now the
exception: `game::paddle_bounce` picks the outgoing angle from **where** across
the paddle the ball was caught, plus which way the paddle was moving. That is
deliberate and is the whole of the player's control over the ball — a mirror
reflection returns every ball at the angle it arrived — but the plan doc has not
been updated to say so, and someone reading the two together will notice.

Nothing else does its own intersection math; the sweep is still
`PhysicsSystem::sweep_sphere` and every other collider still mirrors.

## Coverage gaps

- **The camera fix was never seen on a screen.** The "whole play field is on
  screen" property is asserted against `app::WorldToScreen`, which derives its
  half-extents from `gpu::camera_half_height` — the same function the
  `Projection::Orthographic` the forward pass uses is built from. That makes it
  a real check of the mapping, and not a check of pixels: no windowed run, no
  browser run and no golden image were taken for it. `web/run-browser-e2e.sh`
  was not run.
- **No changelog exists in this repo**, so the bounce-physics and camera changes
  are recorded only in `git log`. Worth starting one at the first tagged
  release; there are no tags yet.
