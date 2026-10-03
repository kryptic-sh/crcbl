#!/usr/bin/env bash
# Neither `crcbl-ui` nor `crcbl-server` depends on the renderer or a GPU
# backend, under any feature set or target.
#
#   tools/check-no-renderer-deps.sh
#
# WHY THIS EXISTS
#   Two of the plan's exit criteria are a crate that names no renderer:
#   `crcbl-ui` produces draw lists and `crcbl-render` owns the pass
#   (`docs/notes/tooling.md`'s UI rules), and `crcbl-server` is the
#   authoritative simulation a headless box runs with no GPU at all
#   (`docs/notes/simulation.md`). Both held, and only the manifests' comments
#   said so. A dependency that pulls the HAL in two hops reads as harmless in
#   the one manifest a reviewer is shown.
#
# WHAT IT CHECKS
#   The normal and build dependency closure of each guarded crate, with its
#   default features and with `--all-features`, on every target
#   (`--target all`, so a Windows-only or wasm-only edge is seen from any
#   host), contains none of:
#
#   * `crcbl-hal` and every crate that depends on it directly — the backends,
#     `crcbl-render` and the `crcbl` facade. Derived from the graph rather than
#     written down, so a backend crate is forbidden the day it names the HAL.
#   * `crcbl-shaders`, the renderer's compiled shaders, which has no HAL edge
#     to be found by.
#
#   Dev-dependencies are not checked: a test may drive a client or a
#   renderer, and nothing it links reaches the shipped crate.
#
#   On a hit it names the crate, the feature set, and prints the path that
#   pulls it in (`cargo tree --invert`).
#
# It fails when it derives no forbidden set or reads an empty closure, rather
# than passing on an empty set: a guard whose scope silently matches nothing is
# the trap this family of scripts exists to avoid. `--locked` throughout, so a
# stale `Cargo.lock` is an error here rather than a graph resolved afresh.

set -euo pipefail

cd "$(dirname "$0")/.."

GUARDED=(crcbl-ui crcbl-server)
FEATURE_SETS=("" "--all-features")
EDGES=(--edges "normal,build" --target all)

# The crates in a `cargo tree --prefix none --format {p}` listing, one name per
# line: the first field of `name vX.Y.Z (path)`.
names() {
  awk '{ print $1 }' | sort -u
}

hal_dependents="$(cargo tree --locked "${EDGES[@]}" --invert crcbl-hal \
  --depth 1 --prefix none --format '{p}' | names)"

mapfile -t forbidden < <(printf '%s\ncrcbl-shaders\n' "$hal_dependents" | sort -u)

# Spot-check the derivation's shape: if `--invert` stopped listing dependents,
# the set would shrink to the HAL alone and every closure would pass.
for expected in crcbl-hal crcbl-render crcbl-vk; do
  if ! printf '%s\n' "${forbidden[@]}" | grep -qx "$expected"; then
    echo "check-no-renderer-deps: the derived forbidden set lacks $expected," >&2
    echo "  so this check would pass on graphs it should refuse. Derived:" >&2
    printf '    %s\n' "${forbidden[@]}" >&2
    exit 1
  fi
done

failed=0
checked=0
for crate in "${GUARDED[@]}"; do
  for features in "${FEATURE_SETS[@]}"; do
    label="${features:-default features}"
    # Unquoted on purpose: `$features` is either nothing or one flag, and a
    # quoted empty string would reach cargo as an argument.
    # shellcheck disable=SC2086
    closure="$(cargo tree --locked "${EDGES[@]}" --package "$crate" $features \
      --prefix none --format '{p}' | names)"
    if ! printf '%s\n' "$closure" | grep -qx "$crate"; then
      echo "check-no-renderer-deps: the closure of $crate ($label) does not" >&2
      echo "  list $crate itself, so it was not read. Got:" >&2
      printf '%s\n' "$closure" >&2
      exit 1
    fi
    checked=$((checked + 1))
    for name in "${forbidden[@]}"; do
      if printf '%s\n' "$closure" | grep -qx "$name"; then
        echo "check-no-renderer-deps: $crate ($label) depends on $name," >&2
        echo "  which it must not. The path that pulls it in:" >&2
        # shellcheck disable=SC2086
        cargo tree --locked "${EDGES[@]}" --package "$crate" $features \
          --invert "$name" >&2
        echo >&2
        failed=1
      fi
    done
  done
done

if [ "$failed" -ne 0 ]; then
  echo "A renderer-free crate reached the renderer. Move the code that needs" >&2
  echo "it up a layer, or depend on a crate below the HAL instead." >&2
  exit 1
fi

echo "check-no-renderer-deps: ${checked} closures of ${GUARDED[*]} checked," \
  "none reach any of ${#forbidden[@]} renderer crates"
