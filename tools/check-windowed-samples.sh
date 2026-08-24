#!/usr/bin/env bash
# Every sample that opens a window must be run by the windowed gate.
#
# WHY THIS EXISTS
#   `tools/run-samples-windowed.sh` is the only check that a sample brings up a
#   real surface — a window, a swapchain, a present loop — rather than the
#   headless offscreen ring every other job uses. Its `SAMPLES` list is written
#   by hand, and a list written by hand is a list that falls behind. It did:
#   `orbit` and `sandbox` both open windows and neither was ever added, so the
#   windowed path of each went ungated for as long as they have existed.
#
#   This is the same seam `tools/check-browser-gate-demos.sh` closes for the
#   browser gate, asked of a different list.
#
# WHAT IT CHECKS
#   Every directory under `apps/` that has a `src/main.rs` — that is, every one
#   that builds a binary and could open a window — appears in `SAMPLES`, unless
#   it is named in EXEMPT below with a reason.
#
# It fails when it parses nothing, rather than passing on an empty set: a guard
# whose scope silently matches nothing is the trap this family of scripts exists
# to avoid.

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
harness="$repo/tools/run-samples-windowed.sh"

if [ ! -f "$harness" ]; then
  echo "check-windowed-samples: $harness is missing" >&2
  exit 1
fi

# Samples that build a binary and still have no window to open. Each needs a
# reason, because "it was failing" is not one — a sample that cannot open a
# window is a bug in the sample, not an exemption.
declare -A EXEMPT=(
  # `sandbox` opens a window and this gate still cannot run it: the harness
  # gives every sample an extent, and `sandbox` has no `--size` flag, so it
  # opens at its own default and fails the extent assertion. Giving it one
  # would close this, and is a change to a crate no demo depends on — see
  # `docs/backlog.md`, "sandbox is not in the windowed gate".
  ["sandbox"]="takes no --size, so the harness cannot ask it for an extent"
)

listed=()
while IFS= read -r name; do
  listed+=("$name")
done < <(sed -n '/^SAMPLES=(/,/^)/p' "$harness" |
  sed -n 's/^[[:space:]]*"\([a-z0-9_-]*\)[[:space:]].*/\1/p')

if [ "${#listed[@]}" -eq 0 ]; then
  echo "check-windowed-samples: parsed no samples out of the SAMPLES array in" >&2
  echo "  tools/run-samples-windowed.sh, so this check would pass on anything." >&2
  echo "  Has its shape changed?" >&2
  exit 1
fi

binaries=()
for main in "$repo"/apps/*/src/main.rs; do
  [ -e "$main" ] || continue
  app="$(basename "$(dirname "$(dirname "$main")")")"
  binaries+=("$app")
done

if [ "${#binaries[@]}" -eq 0 ]; then
  echo "check-windowed-samples: found no apps/*/src/main.rs at all, so this" >&2
  echo "  check would pass on anything. Has the layout changed?" >&2
  exit 1
fi

missing=0
for app in "${binaries[@]}"; do
  if [ -n "${EXEMPT[$app]:-}" ]; then
    continue
  fi
  found=0
  for name in "${listed[@]}"; do
    if [ "$name" = "$app" ]; then
      found=1
      break
    fi
  done
  if [ "$found" -eq 0 ]; then
    echo "check-windowed-samples: apps/$app builds a binary but the windowed" >&2
    echo "  gate never runs it." >&2
    missing=1
  fi
done

if [ "$missing" -ne 0 ]; then
  echo >&2
  echo "Add it to SAMPLES in tools/run-samples-windowed.sh, or name it in this" >&2
  echo "script's EXEMPT list with the reason it has no window to open." >&2
  exit 1
fi

exempt_count=0
for app in "${binaries[@]}"; do
  if [ -n "${EXEMPT[$app]:-}" ]; then
    exempt_count=$((exempt_count + 1))
  fi
done
echo "check-windowed-samples: ${#binaries[@]} binaries, ${exempt_count} exempt," \
  "the rest run by the windowed gate"
