#!/usr/bin/env bash
# Run `crcbl-vk`'s end-to-end suite against a real Vulkan implementation.
#
#   crates/crcbl-vk/tests/run-vk-e2e.sh [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` on a machine with no Vulkan
# loader stays green. This script is the only thing that turns them on, and CI
# runs this script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a
# known trap, so the script fails when the suite reports zero tests run.
#
# Everything here is headless: the suite renders into an offscreen image ring
# (`SurfaceTarget::Offscreen`), so no compositor and no display are needed. The
# *windowed* Vulkan paths are covered by the sandbox runs inside
# `crates/crcbl-shell/tests/run-wayland-e2e.sh` and `run-x11-e2e.sh`.
#
# Exits non-zero if there is no Vulkan loader, if no ICD is visible, if no tests
# ran, or if any test fails.
#
# ENVIRONMENT
#   CRCBL_VK_ICD              Pin an ICD manifest, e.g. lavapipe's `lvp_icd.json`.
#                             CI sets this so a runner that grows a GPU does not
#                             silently stop testing the software path.
#   CRCBL_VK_SYNC_VALIDATION  `1` adds synchronisation validation. CI sets it;
#                             `docs/plan/02-vulkan-backend.md` names sync bugs
#                             as this stage's headline risk and this as the
#                             mitigation.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# Validation is the point of this suite: `ValidationReport::assert_clean` fails
# when the layer was never loaded, so a run without it fails loudly rather than
# passing vacuously. Setting it explicitly means a release-profile run works too.
export CRCBL_VK_VALIDATION="${CRCBL_VK_VALIDATION:-1}"
export CRCBL_VK_SYNC_VALIDATION="${CRCBL_VK_SYNC_VALIDATION:-1}"

if [ -n "${CRCBL_VK_ICD:-}" ]; then
    if [ ! -f "$CRCBL_VK_ICD" ]; then
        # Distributions disagree about the suffix: Debian ships
        # `lvp_icd.x86_64.json` and Arch ships `lvp_icd.json`. Rather than
        # encode one, look next to the name that was asked for. A miss is still
        # a hard failure — a pinned ICD that silently fell back to whatever the
        # loader found would defeat the point of pinning it.
        ICD_DIR="$(dirname "$CRCBL_VK_ICD")"
        # `lvp_icd.x86_64.json` and `lvp_icd.json` share the stem before the
        # first dot of the *basename*; anything matching it in the same
        # directory is the same driver under a different packaging convention.
        ICD_STEM="$(basename "$CRCBL_VK_ICD")"
        ICD_STEM="${ICD_STEM%%.*}"
        FOUND=""
        for CANDIDATE in "${ICD_DIR}/${ICD_STEM}".json "${ICD_DIR}/${ICD_STEM}".*.json; do
            if [ -f "$CANDIDATE" ]; then
                FOUND="$CANDIDATE"
                break
            fi
        done
        if [ -z "$FOUND" ]; then
            echo "crcbl vk e2e: CRCBL_VK_ICD=$CRCBL_VK_ICD does not exist, and no sibling matched" >&2
            ls -la "$(dirname "$CRCBL_VK_ICD")" >&2 || true
            exit 1
        fi
        echo "crcbl vk e2e: $CRCBL_VK_ICD is absent; using $FOUND"
        CRCBL_VK_ICD="$FOUND"
    fi
    # Both spellings: `VK_DRIVER_FILES` is the current one and
    # `VK_ICD_FILENAMES` is what older loaders read.
    export VK_DRIVER_FILES="$CRCBL_VK_ICD"
    export VK_ICD_FILENAMES="$CRCBL_VK_ICD"
    echo "crcbl vk e2e: pinned ICD $CRCBL_VK_ICD"
fi

# Fail early and legibly rather than letting every test panic with the same
# message. `ldconfig -p` is not universal, so this is a best-effort probe: the
# suite's own `NoLoader` panic is the real gate.
if ! ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1' \
    && [ ! -e /usr/lib/libvulkan.so.1 ] \
    && [ ! -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ]; then
    echo "crcbl vk e2e: no libvulkan.so.1 found; install a Vulkan loader" >&2
    echo "  Debian/Ubuntu: libvulkan1 mesa-vulkan-drivers vulkan-validationlayers" >&2
    echo "  Arch:          vulkan-icd-loader vulkan-swrast vulkan-validation-layers" >&2
    exit 1
fi

if command -v vulkaninfo >/dev/null 2>&1; then
    echo "crcbl vk e2e: --- vulkaninfo --summary ---"
    vulkaninfo --summary 2>&1 | sed -n '1,80p' || true
    echo "crcbl vk e2e: --- end vulkaninfo ---"
fi

cd "$REPO_ROOT"
OUTPUT="$(mktemp -t crcbl-vk-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$OUTPUT" "${OUTPUT}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

set +e
cargo nextest run \
    --locked \
    --package crcbl-vk \
    --features vk-e2e \
    --test vk_e2e \
    --run-ignored all \
    --test-threads 1 \
    --no-capture \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl vk e2e: the suite failed" >&2
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job. The colour-stripped copy is
# load-bearing — CI sets `CARGO_TERM_COLOR: always`, so nextest emits the count
# wrapped in escapes and a plain-text match sees no digits next to "tests run".
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"${OUTPUT}.plain"
RAN="$(grep -Eo '[0-9]+ tests? run' "${OUTPUT}.plain" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl vk e2e: the suite reported no tests run — the gate is not gating" >&2
    exit 1
fi
echo "crcbl vk e2e: $RAN tests ran against a real Vulkan implementation"

# The sandbox's own frame, headless, against the same implementation. This is
# the thing `docs/plan/02-vulkan-backend.md`'s milestone 1 is measured by — a
# clear reaching the screen through the whole shell→HAL→swapchain join — and it
# is the only part of that path a windowless CI runner can exercise.
echo "crcbl vk e2e: running the sandbox headless against Vulkan"
SANDBOX_LOG="$(mktemp -t crcbl-vk-sandbox.XXXXXX.log)"
set +e
cargo run --locked --quiet --package sandbox -- \
    --headless --backend vk --frames 30 2>&1 | tee "$SANDBOX_LOG"
SANDBOX_STATUS=${PIPESTATUS[0]}
set -e
if [ "$SANDBOX_STATUS" -ne 0 ]; then
    echo "crcbl vk e2e: the sandbox failed against Vulkan (exit $SANDBOX_STATUS)" >&2
    rm -f "$SANDBOX_LOG"
    exit "$SANDBOX_STATUS"
fi
if ! grep -q "30 frames" "$SANDBOX_LOG"; then
    echo "crcbl vk e2e: the sandbox did not report 30 frames" >&2
    cat "$SANDBOX_LOG" >&2
    rm -f "$SANDBOX_LOG"
    exit 1
fi
rm -f "$SANDBOX_LOG"
echo "crcbl vk e2e: the sandbox presented 30 frames through the Vulkan backend"

# And the null backend still works, on the same binary, selected at runtime.
# `docs/plan/10-wasm-webgpu.md`'s "does it repro on the other backend?" triage
# only exists if both are reachable without a rebuild.
echo "crcbl vk e2e: running the sandbox headless against the null backend"
if ! cargo run --locked --quiet --package sandbox -- \
    --headless --backend null --frames 10 | grep -q "10 frames"; then
    echo "crcbl vk e2e: the null backend is no longer reachable at runtime" >&2
    exit 1
fi
echo "crcbl vk e2e: both backends are selectable from one binary"
