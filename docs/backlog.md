# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

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

## CI on main is red, and was before these changes

Runs 30716427236 (commit 3ab92a9, a docs-only change) and 30720929583 (this
work) fail the **same nine jobs**: test/clippy/coverage/rustdoc/vk e2e/wgpu
e2e/decoder fuzz on linux, plus macOS and Windows. Two distinct causes in the
logs:

- **Linux and macOS**: `alsa-sys 0.4.0`'s build script panics — the runner has
  no `libasound2-dev`, so `pkg-config` finds no `alsa`. Every linux job that
  compiles the workspace dies there, which is why the failure looks total.
- **Windows**: `crates/crcbl-shaders/build.rs:369` panics during
  `cargo build --workspace --locked`.

Neither is caused by the breakout fixes: the same jobs failed identically on the
previous commit. The Pages workflow is green and did deploy the demo. Not
diagnosed further; the fix is presumably an apt step in the linux jobs and a
look at what the shader build expects to find on Windows.

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
