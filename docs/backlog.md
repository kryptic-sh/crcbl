# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

## Coverage gaps

- **The camera fix was never seen on a screen.** The "whole play field is on
  screen" property is asserted against `app::WorldToScreen`, which derives its
  half-extents from `gpu::camera_half_height` — the same function the
  `Projection::Orthographic` the forward pass uses is built from. That makes it
  a real check of the mapping, and not a check of pixels: no windowed run, no
  browser run and no golden image were taken for it. `web/run-browser-e2e.sh`
  was not run.
- **No changelog exists in this repo**, so the bounce-physics, speed-ramp and
  camera changes are recorded only in `git log`. Worth starting one at the first
  tagged release; there are no tags yet.
