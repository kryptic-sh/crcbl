#!/usr/bin/env bash
# Native Metal 3 regression: link the engine's actual cluster mesh and raster
# vertex outputs to the shared forward fragment, using fresh pinned Slang MSL.
# Requires macOS and a Metal 3 GPU; unavailable hardware is a failure, not a skip.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
SLANGC="${CRCBL_SLANGC:-slangc}"
"$SLANGC" -version 2>&1 | rg -q '^2026\.14$'
for shader in mesh mesh_cluster; do
    "$SLANGC" "$ROOT/shaders/$shader.slang" -target metal -profile spirv_1_5 \
        -D CRCBL_TARGET_MSL=1 -o "$WORK/$shader.metal"
done
xcrun clang -fobjc-arc -framework Foundation -framework Metal \
    "$ROOT/tests/metal/mesh-stage-link.m" -o "$WORK/mesh-stage-link"
"$WORK/mesh-stage-link" "$WORK/mesh_cluster.metal" "$WORK/mesh.metal"
"$WORK/mesh-stage-link" "$ROOT/msl/mesh_cluster.metal" "$ROOT/msl/mesh.metal"
