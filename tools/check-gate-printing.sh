#!/usr/bin/env bash
# Every Node tool under `web/tools/` prints through `say` and `warn`, never
# through `console`.
#
#   tools/check-gate-printing.sh [file…]   # default: every tracked web/tools/*.mjs
#
# WHY THIS EXISTS
#   CI runs these tools as `node … 2>&1 | tee` under a step or job cap, and
#   Node's stdout is asynchronous when it is a pipe: a line `console.log`
#   "printed" can still be queued when the cap kills the process, and then it
#   is never written at all. A gate that hangs is read backwards from its last
#   line, and that reading is worthless when the last line in the log is not
#   the last line the run reached. `say` and `warn` in
#   `web/tools/browser-launch.mjs` write synchronously; that file's `emit` says
#   how.
#
#   The conversion was mechanical, and nothing stopped a `console.log` coming
#   back into any of the files afterwards: it reads as fine right up to the
#   moment a job is killed.
#
# WHAT IT CHECKS
#   No line outside a comment calls `console.log`, `console.error`,
#   `console.warn`, `console.info` or `console.debug`. Reassigning one is not a
#   call and is allowed — `gpu-replay.mjs` swaps `console.warn` out to capture
#   what the engine code under test warns about, which is that code's console,
#   not the tool's printing.
#
# It fails when it scans nothing, rather than passing on an empty set: a guard
# whose scope silently matches nothing is the trap this family of scripts
# exists to avoid.

set -euo pipefail

cd "$(dirname "$0")/.."

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
  mapfile -t files < <(git ls-files 'web/tools/*.mjs')
fi

if [ ${#files[@]} -eq 0 ]; then
  echo "check-gate-printing: no web/tools/*.mjs to check, so this check" >&2
  echo "  would pass on anything. Has the layout changed?" >&2
  exit 1
fi

# `-H` so a single file argument still prints as `file:line:text`, which is
# what the comment filter below splits on.
calls="$(grep -HnE '\bconsole\.(log|error|warn|info|debug)\(' "${files[@]}" |
  grep -vE '^[^:]+:[0-9]+:[[:space:]]*(//|\*)' || true)"

if [ -n "$calls" ]; then
  echo "$calls" >&2
  echo >&2
  echo "Each line above prints through console, which a killed run can lose." >&2
  echo "Use say or warn from web/tools/browser-launch.mjs instead." >&2
  exit 1
fi

echo "check-gate-printing: ${#files[@]} file(s) checked, none print through console"
