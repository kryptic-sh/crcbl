#!/usr/bin/env bash
# The order the library crates publish to crates.io in, and the check that it
# is still right.
#
# crates.io resolves a crate's dependencies when it is uploaded, so each crate
# must follow every workspace crate it needs as a normal or build dependency.
# (Internal dev-dependencies are path-only and stripped on publish; the
# workspace `Cargo.toml` says why.) The list is spelled out rather than derived
# so a release publishes exactly what was reviewed, and this check fails, on
# every push, when the list and the workspace disagree:
#
# * a publishable member missing from ORDER, or a listed crate that is no
#   longer a publishable member — a new library would otherwise fail the
#   release halfway, with the crates ahead of it already uploaded;
# * a crate listed before one of its own dependencies.
#
# `--print` prints ORDER, one crate per line, for the jobs that walk it.
set -euo pipefail

ORDER=(
crcbl-rand
crcbl-console
crcbl-jobs
crcbl-vfx
crcbl-water
crcbl-sprite
crcbl-font
crcbl-image
crcbl-golden
crcbl-reflect-derive
crcbl-wl-scanner
crcbl-anim
crcbl-spawn
crcbl-core
crcbl-reflect
crcbl-shaders
crcbl-net
crcbl-store
crcbl-audio
crcbl-mixer
crcbl-hal
crcbl-shell
crcbl-input
crcbl-ui
crcbl-frame
crcbl-ecs
crcbl-phys
crcbl-nav
crcbl-render
crcbl-dx12
crcbl-mtl
crcbl-vk
crcbl-webgpu
crcbl-app
crcbl-server
crcbl-steam
crcbl-assets
crcbl-inventory
crcbl-greybox
crcbl-client
crcbl-scene
crcbl-ragdoll
crcbl-wind
crcbl
)

if [ "${1:-}" = "--print" ]; then
  printf '%s\n' "${ORDER[@]}"
  exit 0
fi

# jq writes CRLF on Windows, and a name ending in a carriage return matches
# nothing, so every list it produces goes through this.
names() {
  jq -r "$@" | tr -d '\r'
}

metadata=$(cargo metadata --no-deps --locked --format-version 1)
members=$(names '.packages[] | select(.publish != []) | .name' <<<"$metadata" | sort)
listed=$(printf '%s\n' "${ORDER[@]}" | sort)
if [ "$members" != "$listed" ]; then
  echo "::error::publish order drift: ORDER in $0 must list every publishable member"
  diff <(echo "$listed") <(echo "$members") | sed 's/^/  /' || true
  exit 1
fi

failed=0
seen=" "
for crate in "${ORDER[@]}"; do
  # The crate's workspace dependencies that reach crates.io with it: normal
  # and build, every target. A dev-dependency (`kind == "dev"`) is stripped.
  # shellcheck disable=SC2016 # `$crate`, `$members` and `$n` are jq's own.
  deps=$(names --arg crate "$crate" '
    [.packages[].name] as $members
    | .packages[] | select(.name == $crate) | .dependencies[]
    | select(.kind != "dev") | .name
    | select(. as $n | $members | index($n))' <<<"$metadata" | sort -u)
  for dep in $deps; do
    case "$seen" in
      *" $dep "*) ;;
      *)
        echo "::error::$crate is listed before its dependency $dep"
        failed=1
        ;;
    esac
  done
  seen="$seen$crate "
done
if [ "$failed" -ne 0 ]; then
  exit 1
fi
echo "publish order: every publishable member, each after its dependencies"
