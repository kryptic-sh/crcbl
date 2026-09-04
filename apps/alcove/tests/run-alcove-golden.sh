#!/usr/bin/env bash
# Draw alcove's court on a named backend, check every occlusion claim, and
# compare the frames against their checked-in goldens.
#
#   CRCBL_GPU=vk apps/alcove/tests/run-alcove-golden.sh [extra nextest args…]
#
# # What this is for
#
# `docs/plan/sample/19-alcove.md`'s milestones 1 and 2. The suite is
# `apps/alcove/tests/golden.rs`, and what it is about is not whether a frame
# drew: an occlusion pass that never ran leaves a white channel, one whose
# intensity is stuck at zero leaves the same, and a technique selector wired to
# one pipeline draws two identical halves either side of the comparison seam.
# Every one of those produces a picture somebody would bless. So the goldens are
# the last thing this suite checks, and the assertions before them are about
# where the frame is dark, by how much, and which gather drew which column.
#
# # Why the backend must be named
#
# The court is drawn identically on every backend by construction, so a run that
# fell back to another one produces a frame that passes and proves nothing about
# the one that was wanted. `crcbl::backend::open` would otherwise answer the
# question for you.
#
# # Pinning a driver and an adapter
#
# `CRCBL_VK_ICD` picks the Vulkan ICD, through the same `crcbl_pin_vk_icd` the
# other harnesses source. `CRCBL_ADAPTER` names a device class — `cpu`,
# `integrated`, `discrete` or `virtual` — and `crcbl::adapter` refuses rather
# than falling back when this machine has none of it. The suite prints the
# adapter it opened and this script reads it back: a variable that never reached
# the test process and a pin that was honoured look identical from outside.
#
# # ENVIRONMENT
#
#   CRCBL_GPU       Which backend draws. Required; there is no default.
#   CRCBL_ADAPTER   Which adapter class inside it. Optional.
#   CRCBL_VK_ICD    Which Vulkan driver, when `CRCBL_GPU=vk`.
#   CRCBL_BLESS     Rewrite the references instead of comparing. Never a pass —
#                   `crcbl_golden::Outcome::into_result` reports a blessed run
#                   as a failure, which is what this script then reports too.
#
# # The zero-tests check is the point
#
# A suite that is both feature-gated and `#[ignore]`d has two ways to run
# nothing. `--no-tests fail` catches an empty selection; parsing nextest's own
# summary catches a filter that matched nothing inside one that was not empty.

set -euo pipefail

APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${APP_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${REPO_ROOT}/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "alcove golden"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
alcove golden: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=dx12 $0     # Direct3D 12, Windows
NOBACKEND
    exit 1
fi

# A vk run validates whatever the shell says: the check after the run reads what
# the layer said, and a `CRCBL_VK_VALIDATION=0` left over from profiling would
# hand it a log with no messenger in it — which it rejects, for the wrong reason.
if [ "$CRCBL_GPU" = vk ]; then
    export CRCBL_VK_VALIDATION=1
fi

cd "$REPO_ROOT"

# Echoed rather than defaulted: unset means "whatever this machine enumerated
# first", which is a legitimate thing to run deliberately.
echo "alcove golden: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-alcove-golden.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the
# selected paths, the adapter, and every reading the occlusion claims are made
# from — are only interesting on a green run, which is exactly the run nextest
# captures them on.
set +e
cargo nextest run --locked -p alcove --features golden-e2e --test golden \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "alcove golden: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# CI sets `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and
# a match on digits next to "tests run" sees none — a check that then fires on a
# suite which ran everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# What the validation layer said. A violation reaches `crcbl_core::log::error!`
# and the test still passes, because the fixture's `finish` checks only the
# seam's out-of-band channel. `tools/vk-validation-log.sh` asks both halves — the
# layer announced itself, and then said nothing — and
# `CRCBL_VK_VALIDATION_SELF_TEST=1` is how to watch this go red.
if [ "$CRCBL_GPU" = vk ] \
    && ! crcbl_validation_saw_nothing "${LOG}.plain" "the alcove golden suite"; then
    exit 1
fi

if ! crcbl_nextest_summary "${LOG}.plain" "alcove golden" \
    "The golden-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi

# Which adapter the frames were drawn on, from the suite rather than from the
# variable this script exported.
ADAPTER="$(grep -F 'alcove golden: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "alcove golden: the suite never named the adapter it drew on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
echo "alcove golden: ${ADAPTER#*alcove golden: }"

echo "alcove golden: the court drew, made its occlusion claims and matched on $CRCBL_GPU"
