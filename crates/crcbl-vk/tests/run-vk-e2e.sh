#!/usr/bin/env bash
# Run `crcbl-vk`'s end-to-end suite against a real Vulkan implementation.
#
#   crates/crcbl-vk/tests/run-vk-e2e.sh [--bless] [extra nextest args…]
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
# WHAT A GREEN RUN HERE IS AND IS NOT EVIDENCE OF
#   This script is what a developer runs to see what CI sees, so it has to say
#   what it actually checked rather than only how many tests passed. Two things
#   were silently inherited before and are now reported by name:
#
#   * **which driver ran**, taken from the suite's own adapter line rather than
#     from the manifest path that was asked for; and
#   * **how far this machine's validation layer can see**, because sync
#     validation is not one switch. A hazard inside one command buffer is caught
#     while it is recorded; a hazard that spans two *submissions* can only be
#     caught when the queue is submitted, and layer builds differ in whether
#     they model that. Every cross-frame hazard is of the second kind.
#
#   That difference is not hypothetical. A missing cross-frame barrier on the
#   render graph's depth transient was reported by the CI leg's layer and by
#   nothing at all on an Arch box running VK_LAYER_KHRONOS_validation 1.4.350 —
#   same driver, same flags, same tests, 26/26 green. A harness that reports
#   26/26 without saying that is worse than no harness, so it now prints the
#   layer's reach and shouts when the reach is short.
#
#   The gate for that *class* of bug is therefore deliberately not here: it is
#   `crcbl-render`'s graph-compile suite, which compiles two frames against one
#   `TransientPool` and asserts the second one's barriers name what the first
#   left behind. No layer, no ICD, no GPU, no packaging opinions.
#
# ENVIRONMENT
#   CRCBL_VK_ICD              Pin an ICD manifest, e.g. lavapipe's `lvp_icd.json`.
#                             CI sets this so a runner that grows a GPU does not
#                             silently stop testing the software path.
#   CRCBL_VK_SYNC_VALIDATION  `1` adds synchronisation validation. CI sets it;
#                             `docs/plan/02-vulkan-backend.md` names sync bugs
#                             as this stage's headline risk and this as the
#                             mitigation.
#   CRCBL_BLESS               `1` regenerates golden images instead of comparing
#                             against them. `--bless` sets it; see below.
#
# GOLDEN IMAGES
#   `--bless` regenerates `tests/golden/*.png` rather than comparing against
#   them, which is the spelling `docs/plan/12-testing.md` asks for. A blessed run
#   deliberately **fails**: it has not checked anything, and a gate that any
#   missing reference switches off is not a gate. Review the regenerated image,
#   commit it, and re-run without the flag.
#
#   Bless on the driver the reference is meant to represent. The tolerance in
#   `crcbl-golden` is calibrated for the radv/lavapipe difference (see that
#   crate's docs for the measurements), so either works — but re-blessing on a
#   third driver and committing it silently moves the reference.

set -euo pipefail

if [ "${1:-}" = "--bless" ]; then
    export CRCBL_BLESS=1
    shift
    echo "crcbl vk e2e: CRCBL_BLESS=1 — golden images will be regenerated, and the"
    echo "              golden tests will fail on purpose because a blessed run checks nothing."
fi

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
else
    # Say so. The header above claims this script is what a developer runs to
    # see what CI sees, and with no ICD pinned that claim is false: the loader
    # picks whatever is installed, which on a workstation is the discrete GPU
    # and never the software rasteriser CI runs. The suite prints the adapter it
    # got, but nobody reads an adapter line looking for an absence — this is the
    # line that names it. Not a hard failure: running against real hardware is a
    # thing worth doing deliberately, and this is exactly how.
    cat >&2 <<'NOICD'
crcbl vk e2e: CRCBL_VK_ICD is not set, so the loader will choose the driver.
              CI pins lavapipe and this run does not, so a green run here is
              NOT the run CI makes. To reproduce CI:
                CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.json $0
NOICD
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
    # Named, rather than left in eighty lines of dump: a run gated by a
    # validation layer is only evidence about the layer that gated it, and
    # `vulkaninfo --summary`'s layer table is the one place that says which.
    LAYER_LINE="$(vulkaninfo --summary 2>/dev/null | grep -E '^VK_LAYER_KHRONOS_validation' || true)"
    if [ -n "$LAYER_LINE" ]; then
        # `NAME  description…  <spec version>  version <impl>`, so the three
        # trailing fields are the two numbers that identify a build.
        echo "crcbl vk e2e: validation layer $(echo "$LAYER_LINE" | awk '{print $1, "spec", $(NF-2), $(NF-1), $NF}')"
    else
        echo "crcbl vk e2e: vulkaninfo does not list VK_LAYER_KHRONOS_validation" >&2
    fi
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

# The colour-stripped copy is load-bearing for every match below — CI sets
# `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and a
# plain-text match sees no digits next to "tests run".
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$OUTPUT" >"${OUTPUT}.plain"

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl vk e2e: the suite failed" >&2
    # `docs/plan/12-testing.md`: "diffs uploaded as CI artifacts on failure".
    # Naming the directory here is what makes the CI step's `if: failure()`
    # upload obvious rather than folklore.
    if [ -d "${REPO_ROOT}/target/golden-diff" ]; then
        echo "crcbl vk e2e: golden-image diffs are in target/golden-diff:" >&2
        ls -la "${REPO_ROOT}/target/golden-diff" >&2 || true
    fi
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job.
RAN="$(grep -Eo '[0-9]+ tests? run' "${OUTPUT}.plain" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl vk e2e: the suite reported no tests run — the gate is not gating" >&2
    exit 1
fi

# Which driver actually ran, from the suite rather than from the manifest that
# was asked for. A pinned ICD the loader quietly ignored, or a sibling manifest
# that turned out to be a different driver, both show up here and nowhere else.
DRIVER="$(grep -Eo 'vk e2e: adapter .*' "${OUTPUT}.plain" | head -1 || true)"
if [ -n "$DRIVER" ]; then
    echo "crcbl vk e2e: ${DRIVER#vk e2e: }"
else
    echo "crcbl vk e2e: the suite never named an adapter — it did not open a device" >&2
    exit 1
fi

echo "crcbl vk e2e: $RAN tests ran against a real Vulkan implementation"

# How far this machine's validation layer can see. The suite measures it; this
# is what turns the measurement into something a reader cannot miss.
REACH="$(grep -Eo 'sync-validation reach: .*' "${OUTPUT}.plain" | tail -1 || true)"
if [ -z "$REACH" ]; then
    # Not a soft warning: the suite is supposed to publish this, and a harness
    # that silently stopped measuring its own blind spot is the failure this
    # whole section exists to prevent.
    echo "crcbl vk e2e: the suite did not report its sync-validation reach." >&2
    echo "              crates/crcbl-vk/tests/vk_e2e.rs must print it, and this" >&2
    echo "              script must be able to find it, or a green run here" >&2
    echo "              claims evidence it does not have." >&2
    exit 1
fi
echo "crcbl vk e2e: $REACH"
case "$REACH" in
    *cross-submission=no*)
        echo "crcbl vk e2e: ############################################################" >&2
        echo "crcbl vk e2e: # THIS RUN IS WEAKER THAN THE CI JOB IT STANDS IN FOR.     #" >&2
        echo "crcbl vk e2e: ############################################################" >&2
        echo "crcbl vk e2e: This machine's validation layer reports hazards inside a" >&2
        echo "              submission and not hazards *between* submissions. Every" >&2
        echo "              missing cross-frame barrier is of the second kind, so the" >&2
        echo "              green result above says nothing about them — CI's layer" >&2
        echo "              has caught one that this configuration cannot see." >&2
        echo "              Rely on 'cargo nextest run -p crcbl-render' for that" >&2
        echo "              class: it compiles consecutive frames against one pool" >&2
        echo "              and needs no layer at all." >&2
        ;;
    *)
        echo "crcbl vk e2e: the layer sees across submissions, so cross-frame hazards \
were in scope"
        ;;
esac

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
