#!/usr/bin/env bash
# Offscreen native object/mesh proofs. Requires a real Metal 3 GPU; the
# paravirtual CI device is intentionally covered by run-mtl-e2e.sh instead.
# Run from the repository root. No runtime skip or zero-test success is allowed.
set -euo pipefail

export MTL_DEBUG_LAYER=1
export MTL_DEBUG_LAYER_ERROR_MODE=assert
export MTL_DEBUG_LAYER_WARNING_MODE=assert
export MTL_SHADER_VALIDATION=1
export MTL_SHADER_VALIDATION_ENABLE_ERROR_REPORTING=1
export MTL_SHADER_VALIDATION_REPORT_TO_STDERR=1
export MTL_SHADER_VALIDATION_ABORT_ON_FAULT=1
export CRCBL_MTL_VALIDATION=1

cargo nextest run --locked -p crcbl-mtl --features mtl-mesh-e2e \
    --run-ignored only --no-tests fail --test-threads 1 \
    --success-output immediate -E 'test(native_mesh_proof::)' "$@"
