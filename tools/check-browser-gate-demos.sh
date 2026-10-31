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
#   A demo heavy enough to be split is not in that list: it is two or more rows
#   of the matrix's `include`, each a `demo:` with the `groups:` it makes, and
#   counts as run by the Linux job only if those rows' groups add up to every
#   group the driver has after A and B. A split that dropped a letter is a demo
#   whose share of the gate nothing runs, and it would read here as covered.
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

# The `include` rows, as `demo groups` pairs: each `- demo:` line, then the
# `groups:` line under it. The groups each row of a split demo makes are
# gathered per demo and checked against the full set below.
split=()
while IFS= read -r row; do
  split+=("$row")
done < <(sed -n '/^        include:$/,/^    [a-z]/p' "$workflow" |
  sed -n -e 's/^          - demo: \([a-z0-9_-]*\)$/\1/p' \
    -e 's/^            groups: \([A-Z, ]*\)$/\1/p' |
  tr -d ' ' | paste -d' ' - -)

# The groups a split demo's rows must add up to: every one the driver defines
# after A and B, which each row makes whatever it names. Read from the driver's
# own `GROUPS`, so a group added there is one every split must cover.
driver="$repo/web/tools/browser-e2e.mjs"
all_groups=$(sed -n "s/^const GROUPS = \[\(.*\)\];$/\1/p" "$driver" |
  tr -d "' " | tr ',' '\n' | grep -vx -e A -e B | tr '\n' ' ')
if [ -z "$all_groups" ]; then
  echo "check-browser-gate-demos: could not read GROUPS from $driver" >&2
  exit 1
fi
split_bad=0
split_demos=()
for row in "${split[@]}"; do
  demo="${row%% *}"
  case " ${split_demos[*]} " in
    *" $demo "*) ;;
    *) split_demos+=("$demo") ;;
  esac
done
for demo in "${split_demos[@]}"; do
  made=""
  rows=0
  for row in "${split[@]}"; do
    if [ "${row%% *}" = "$demo" ]; then
      made="$made,${row#* }"
      rows=$((rows + 1))
    fi
  done
  for letter in $all_groups; do
    case ",$made," in
      *",$letter,"*) ;;
      *)
        echo "check-browser-gate-demos: $demo is split across $rows include row(s)" >&2
        echo "  of the demos job's matrix and none of them makes group $letter," >&2
        echo "  so that part of its browser gate runs nowhere." >&2
        split_bad=1
        ;;
    esac
  done
  if [ "$rows" -lt 2 ]; then
    echo "check-browser-gate-demos: $demo has a single include row; a demo that" >&2
    echo "  is not split belongs in the matrix's demo list." >&2
    split_bad=1
  fi
  for named in "${matrix[@]}"; do
    if [ "$named" = "$demo" ]; then
      echo "check-browser-gate-demos: $demo is both in the demos job's matrix" >&2
      echo "  list and split under its include, so its groups run twice." >&2
      split_bad=1
    fi
  done
done
if [ "$split_bad" -ne 0 ]; then
  exit 1
fi

missing=0
for demo in "${demos[@]}"; do
  # The windowed job and the headless one. Counted separately rather than
  # together, so a demo named twice in one job cannot stand in for the other.
  windowed=0
  for named in "${matrix[@]}" "${split_demos[@]}"; do
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

echo "check-browser-gate-demos: ${#demos[@]} demos, each run by both browser-gate jobs" \
  "(${#split_demos[@]} of them split across the Linux job's include rows)"
