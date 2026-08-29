#!/usr/bin/env bash
# Draw an imported glTF file and check its own texture landed where its own node
# hierarchy puts it, on the backend `CRCBL_GPU` names.
#
#   CRCBL_GPU=vk crates/crcbl/tests/run-gltf-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `tests/gltf_e2e.rs` is the win condition for `crcbl_scene::gltf_render`: a
# `.glb` written to disk, imported through the real `DirSource`, converted to a
# `SceneDesc`, and drawn — with the four texels of its base-colour image asserted
# in the four quadrants the composed transform puts them in. Before that bridge
# existed no glTF in this workspace had ever reached a pixel, so a conversion
# test could only have compared one host-side structure against another.
#
# # Its own script rather than a second `--test` in another runner
#
# The same shape `run-hal-seam-e2e.sh` and `run-tiling-e2e.sh` have: one runner
# per test binary. Both of the checks below — nextest's own summary, and the
# adapter line grepped out of the suite's output — are written against a single
# run's single summary, and folding a second `--test` into one of them would
# leave one parse covering two runs.
#
# # The feature flag is not optional here
#
# `tests/gltf_e2e.rs` is `#![cfg(feature = "scene")]` because the glTF importer
# is: `crcbl`'s `scene` feature is what links `crcbl-scene` at all, and a browser
# build that describes no scene of its own should not link a glTF parser. Without
# `--features scene` the binary compiles to nothing at all and nextest reports a
# suite of zero tests — which is exactly the failure the zero-tests check below
# exists to catch, and why this script passes the flag rather than trusting a
# caller to.
#
# # Why the backend must be named
#
# Every backend samples this quad identically by construction, so a run that fell
# back to another backend reads the same quadrants and proves nothing about the
# one that was wanted. The suite asserts the opened backend against `CRCBL_GPU`,
# and this script refuses to run without it, because `crcbl::backend::open`'s
# automatic order would otherwise silently answer the question for you.
#
# # Pinning a driver, pinning an adapter
#
# Identical to `run-tiling-e2e.sh`, through the same shared helpers rather than a
# fourth copy that drifts: `CRCBL_VK_ICD` resolves the Vulkan ICD via
# `crcbl_pin_vk_icd`, and `CRCBL_ADAPTER` names a device *class* that
# `crcbl::adapter` resolves — refusing rather than falling back when this machine
# has no adapter of that class. Set, and this script checks it *arrived*, off the
# suite's own output rather than off the variable it exported: a variable that
# never reached the test process and a pin that was honoured look identical from
# outside, because in both cases the suite is green.
#
# # ENVIRONMENT
#
#   CRCBL_GPU       Which backend draws. Required; there is no default.
#   CRCBL_ADAPTER   Which adapter class inside it. Optional; unset takes the
#                   first one enumerated.
#   CRCBL_VK_ICD    Which Vulkan driver, when `CRCBL_GPU=vk`.
#
# # The zero-tests check is the point
#
# `docs/plan/12-testing.md` calls a silently-skipped e2e suite a known trap, and
# `run-tiling-e2e.sh` records the time it sprang here: a runner naming the wrong
# `--test` reports success having run nothing. `--no-tests fail` catches an empty
# selection; parsing nextest's own summary catches a filter — or a missing
# feature flag — that matched nothing inside a selection that was not empty.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${REPO_ROOT}/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "crcbl gltf e2e"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl gltf e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=dx12 $0     # Direct3D 12, Windows
NOBACKEND
    exit 1
fi

# A vk run validates whatever the shell says: the check after the run reads
# what the layer said, and a `CRCBL_VK_VALIDATION=0` left over from profiling
# would hand it a log with no messenger in it — which it rejects, for the
# wrong reason.
if [ "$CRCBL_GPU" = vk ]; then
    export CRCBL_VK_VALIDATION=1
fi

cd "$REPO_ROOT"

# Echoed rather than defaulted: unset means "whatever this machine enumerated
# first", which is a legitimate thing to run deliberately.
echo "crcbl gltf e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-gltf-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the adapter
# it opened, and the four quadrant colours the claim is made of — are only
# interesting on a green run, which is exactly the run nextest captures them on.
set +e
cargo nextest run --locked -p crcbl --features scene --test gltf_e2e \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl gltf e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in the other
# runners: CI sets `CARGO_TERM_COLOR: always`, so nextest wraps its counts in
# escapes and the match below sees no digits next to "tests run" — a check that
# then fires on a suite which ran everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# What the validation layer said, which nothing here read until a forward_e2e
# run went green on radv with an `ERROR … vk validation:` line in its log: a
# violation reaches `crcbl_core::log::error!` and the test still passes, because
# the fixture's `finish` checks only the seam's out-of-band channel.
# `tools/vk-validation-log.sh` asks both halves — the layer announced itself,
# and then said nothing — and `CRCBL_VK_VALIDATION_SELF_TEST=1` is how to watch
# this go red.
if [ "$CRCBL_GPU" = vk ] \
    && ! crcbl_validation_saw_nothing "${LOG}.plain" "the crcbl gltf e2e suite"; then
    exit 1
fi

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl gltf e2e" \
    "The ignore attribute, the scene feature or the test binary's name stopped matching the test."; then
    exit 1
fi

# Which adapter the quad was drawn on, from the suite rather than from the
# variable this script exported. If the test stops printing it, this is what says
# so rather than a green run quietly losing the check.
ADAPTER="$(grep -F 'crcbl gltf e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl gltf e2e: the suite never named the adapter it drew on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl gltf e2e: ${ADAPTER#*crcbl gltf e2e: }"

# The pin the test process actually saw is printed on that same line, so this
# compares two strings rather than re-deriving the class vocabulary in bash. A
# mismatch means the variable did not reach the test process — the one failure
# `crcbl::adapter` cannot diagnose for itself, because from inside an unset pin
# and no pin are the same thing.
if [ -n "${CRCBL_ADAPTER:-}" ]; then
    case "$ADAPTER" in
        *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
        *)
            echo "crcbl gltf e2e: #####################################################" >&2
            echo "crcbl gltf e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS.   #" >&2
            echo "crcbl gltf e2e: #####################################################" >&2
            echo "crcbl gltf e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported and the suite" >&2
            echo "  reported the line above instead. The variable did not reach the test" >&2
            echo "  process, so the quad was drawn on whatever was enumerated first and" >&2
            echo "  every result above is evidence about a device nobody chose." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl gltf e2e: an imported glTF drew its own texture on $CRCBL_GPU"
