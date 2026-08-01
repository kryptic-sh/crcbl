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
- **The changelog starts mid-project.** `CHANGELOG.md` covers this session's
  changes onward; everything before it is in `git log` only, and nobody has gone
  back to reconstruct it. Worth doing at the first tagged release, or not at all
  — there are no releases yet for a reader to be missing entries from.
