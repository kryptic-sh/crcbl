#!/usr/bin/env bash
# Hold every backend to the same draw-generation numbers, on the one `CRCBL_GPU`
# names.
#
#   CRCBL_GPU=vk crates/crcbl/tests/run-draw-gen-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `tests/draw_gen_e2e/` reads the buffers the GPU wrote — the compacted visible
# list, its counter, the per-bucket `draw_indexed_indirect` arguments, the draw
# counts, the instance runs and the culling-statistics ring — and compares them
# against `crcbl::render::cull`'s CPU oracle. Those checks lived in
# `crates/crcbl-vk/tests/vk_e2e/` and ran on Vulkan alone, so a divergence on
# Metal, D3D12 or wgpu could only ever show up as a picture that looked wrong.
#
# The suite names no backend type, so this script running it once per backend is
# the whole matrix — the same shape as `run-hal-seam-e2e.sh`, which the same
# migration produced for the HAL seam.
#
# # Why the backend must be named
#
# Every backend is meant to compute these buffers identically, so a run that fell
# back to another backend passes and proves nothing about the one that was
# wanted. The suite asserts the opened backend against `CRCBL_GPU`, and this
# script refuses to run without it, because `crcbl::backend::open`'s automatic
# order would otherwise silently answer the question for you.
#
# # Pinning a driver, pinning an adapter
#
# Identical to `run-hal-seam-e2e.sh`, through the same shared helper rather than
# a second copy that drifts: `CRCBL_VK_ICD` resolves the Vulkan ICD via
# `crcbl_pin_vk_icd`, and `CRCBL_ADAPTER` names a device *class* that
# `crcbl::adapter` resolves — refusing rather than falling back when this machine
# has no adapter of that class. Set, and this script checks it *arrived*, off the
# suite's own output rather than off the variable it exported: a variable that
# never reached the test process and a pin that was honoured look identical from
# outside, because in both cases the suite is green.
#
# # ENVIRONMENT
#
#   CRCBL_GPU       Which backend is under test. Required; there is no default.
#   CRCBL_ADAPTER   Which adapter class inside it. Optional; unset takes the
#                   first one enumerated.
#   CRCBL_VK_ICD    Which Vulkan driver, when `CRCBL_GPU=vk`.
#
# # The zero-tests check is the point
#
# `docs/plan/12-testing.md` calls a silently-skipped e2e suite a known trap, and
# a suite that is both feature-gated and `#[ignore]`d has two ways to run
# nothing. `--no-tests fail` catches an empty selection; parsing nextest's own
# summary catches a filter that matched nothing inside a selection that was not
# empty.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${REPO_ROOT}/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "crcbl draw gen e2e"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl draw gen e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
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
echo "crcbl draw gen e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-draw-gen-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the adapter
# it opened, the geometry path the renderer built, the counters it read back —
# are only interesting on a green run, which is exactly the run nextest captures
# them on.
set +e
cargo nextest run --locked -p crcbl --features draw-gen-e2e --test draw_gen_e2e \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl draw gen e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in
# `run-hal-seam-e2e.sh`: CI sets `CARGO_TERM_COLOR: always`, so nextest wraps its
# counts in escapes and the match below sees no digits next to "tests run" — a
# check that then fires on a suite which ran everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# What the validation layer said, which nothing here read until a forward_e2e
# run went green on radv with an `ERROR … vk validation:` line in its log: a
# violation reaches `crcbl_core::log::error!` and the test still passes, because
# the fixture's `finish` checks only the seam's out-of-band channel.
# `tools/vk-validation-log.sh` asks both halves — the layer announced itself,
# and then said nothing — and `CRCBL_VK_VALIDATION_SELF_TEST=1` is how to watch
# this go red.
if [ "$CRCBL_GPU" = vk ] \
    && ! crcbl_validation_saw_nothing "${LOG}.plain" "the crcbl draw gen e2e suite"; then
    exit 1
fi

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl draw gen e2e" \
    "The draw-gen-e2e feature or the ignore attribute stopped matching the test."; then
    exit 1
fi

# Which adapter the buffers were produced on, from the suite rather than from the
# variable this script exported. Every test prints this line when it opens a
# device; the first is read and the rest are the same answer. If the tests stop
# printing it, this is what says so rather than a green run quietly losing the
# check.
ADAPTER="$(grep -F 'crcbl draw gen e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl draw gen e2e: the suite never named the adapter it ran on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl draw gen e2e: ${ADAPTER#*crcbl draw gen e2e: }"

# The pin the test process actually saw is printed on that same line, so this
# compares two strings rather than re-deriving the class vocabulary in bash. A
# mismatch means the variable did not reach the test process — the one failure
# `crcbl::adapter` cannot diagnose for itself, because from inside an unset pin
# and no pin are the same thing.
if [ -n "${CRCBL_ADAPTER:-}" ]; then
    case "$ADAPTER" in
        *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
        *)
            echo "crcbl draw gen e2e: ################################################" >&2
            echo "crcbl draw gen e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS. #" >&2
            echo "crcbl draw gen e2e: ################################################" >&2
            echo "crcbl draw gen e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported and the suite" >&2
            echo "  reported the line above instead. The variable did not reach the test" >&2
            echo "  process, so the buffers were produced on whatever was enumerated first" >&2
            echo "  and every result above is evidence about a device nobody chose." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl draw gen e2e: ${CRCBL_NEXTEST_TESTS_RUN} draw-generation checks held on $CRCBL_GPU"
