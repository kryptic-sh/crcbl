# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

## Coverage gaps

- **No golden image covers the play field's framing.** The camera fix was
  confirmed on a real build by eye — the field is no longer cut off — and the
  "whole play field is on screen" property is asserted against
  `app::WorldToScreen`. Neither is a pixel check that would catch the framing
  drifting again: `web/run-browser-e2e.sh` has not been run and there is no
  golden image for a breakout frame.
- **No changelog exists in this repo**, so the bounce-physics, speed-ramp and
  camera changes are recorded only in `git log`. Worth starting one at the first
  tagged release; there are no tags yet.
