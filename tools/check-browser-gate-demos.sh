#!/usr/bin/env bash
# Every demo the site builds must be run through the browser gate, in both jobs.
#
# WHY THIS EXISTS
#   `web/run-browser-e2e.sh` is the only check that a demo boots, opens a WebGPU
#   device and puts moving pixels on a canvas. It runs one demo per invocation,
#   so `.github/workflows/pages.yml` names each demo — in the `demos` job's
#   matrix on Linux, and in a step of its own in the macOS job — and a list
#   written by hand is a list that falls behind. It did: `viewer` shipped and
#   its browser gate was never added, so the demo was green in CI for a week
#   without that gate ever running on it once.
#
#   `web/tools/build-pages.mjs` already holds `web/build.sh` and its own DEMOS
#   list to each other. Neither of them knows about the workflow, which is the
#   seam this closes.
#
# WHAT IT CHECKS
#   For every demo in `web/build.sh`'s DEMOS array, `pages.yml` must run the
#   browser gate on it twice: once without `--headless` (the Linux job, inside
#   Xvfb) and once with it (the macOS job, which has no X server). Those are the
#   two browser-gate jobs; a demo missing from either is a demo one of them
#   never looks at.
#
#   The two are read differently because the workflow spells them differently.
#   Since 2026-08-28 the Linux gates are a matrix — one job per demo, so the
#   wall clock is the slowest demo rather than the sum — and the demo names live
#   in that job's `matrix.demo` list rather than in fourteen `run:` lines. The
#   macOS job is still a list of steps and is still grepped for one.
#
# It fails when it parses nothing, rather than passing on an empty set — a
# guard whose scope silently matches nothing is the trap this whole family of
# scripts exists to avoid.

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_sh="$repo/web/build.sh"
workflow="$repo/.github/workflows/pages.yml"

for required in "$build_sh" "$workflow"; do
  if [ ! -f "$required" ]; then
    echo "check-browser-gate-demos: $required is missing" >&2
    exit 1
  fi
done

# The DEMOS array's rows are `slug:crate:dir`; the slug is what the gate takes.
demos=()
while IFS= read -r slug; do
  demos+=("$slug")
done < <(sed -n '/^DEMOS=(/,/^)/p' "$build_sh" |
  sed -n 's/^[[:space:]]*"\([a-z0-9_-]*\):.*/\1/p')

if [ "${#demos[@]}" -eq 0 ]; then
  echo "check-browser-gate-demos: parsed no demos out of web/build.sh's DEMOS" >&2
  echo "  array, so this check would pass on anything. Has its shape changed?" >&2
  exit 1
fi

# The `demos` job's matrix list: the rows between its `demo:` key and the next
# key at the same indent. Extracted once rather than grepped per demo, so an
# empty list is caught here — where it can say so — rather than reported
# fourteen times over as a missing demo.
matrix=()
while IFS= read -r slug; do
  matrix+=("$slug")
done < <(sed -n '/^        demo:$/,/^        [a-z]/p' "$workflow" |
  sed -n 's/^          - \([a-z0-9_-]*\)$/\1/p')

if [ "${#matrix[@]}" -eq 0 ]; then
  echo "check-browser-gate-demos: parsed no demos out of the demos job's" >&2
  echo "  matrix in pages.yml, so every demo below would report missing." >&2
  echo "  Has the job's shape changed?" >&2
  exit 1
fi

missing=0
for demo in "${demos[@]}"; do
  # The windowed job and the headless one. Counted separately rather than
  # together, so a demo named twice in one job cannot stand in for the other.
  windowed=0
  for named in "${matrix[@]}"; do
    if [ "$named" = "$demo" ]; then
      windowed=$((windowed + 1))
    fi
  done
  headless=$(grep -cE \
    "CRCBL_WEB_E2E_DEMO=$demo \./web/run-browser-e2e\.sh --headless" \
    "$workflow" || true)

  if [ "$windowed" -eq 0 ]; then
    echo "check-browser-gate-demos: $demo is built by web/build.sh but the" >&2
    echo "  demos job's matrix in pages.yml does not name it." >&2
    missing=1
  fi
  if [ "$headless" -eq 0 ]; then
    echo "check-browser-gate-demos: $demo is built by web/build.sh but no" >&2
    echo "  --headless browser-gate step in pages.yml runs it." >&2
    missing=1
  fi
done

if [ "$missing" -ne 0 ]; then
  echo >&2
  echo "Add the demo to the demos job's matrix and a step to the macOS job in" >&2
echo ".github/workflows/pages.yml. A demo the" >&2
  echo "browser gate never runs is a demo nothing has checked draws anything." >&2
  exit 1
fi

echo "check-browser-gate-demos: ${#demos[@]} demos, each run by both browser-gate jobs"
