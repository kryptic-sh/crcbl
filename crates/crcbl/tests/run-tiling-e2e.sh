#!/usr/bin/env bash
# Count the grid cells physical tiling puts on a 1 m and a 2 m surface, on the
# backend `CRCBL_GPU` names.
#
#   CRCBL_GPU=vk crates/crcbl/tests/run-tiling-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `tests/tiling_e2e.rs` asserts the win condition for
# `GpuMaterial::TILING_PHYSICAL`: a 2 m surface shows about twice the grid cells
# of a 1 m one, where authored-UV tiling stretches one tile across a face of any
# size and shows the same count at both.
#
# **Nothing ran it.** Its own header said `tests/run-render-e2e.sh` supplied the
# backend, and that script passes `--test render_e2e` — so the binary was
# compiled by `--all-features` builds, never selected by a runner, and the
# assertion had only ever been executed by hand. This script is what makes it a
# check.
#
# # Its own script rather than a second `--test` in `run-render-e2e.sh`
#
# The same shape `run-hal-seam-e2e.sh` has: one runner per test binary. Both of
# the checks below — nextest's own summary, and the adapter line grepped out of
# the suite's output — are written against a single run's single summary, and
# folding a second `--test` into one of those scripts would leave one parse
# covering two runs. The two also answer different questions: `render_e2e`
# compares frames against checked-in goldens, this counts a property off a frame
# and commits no image.
#
# Unlike the other two suites this one is **not** feature-gated — `#[ignore]` is
# the only thing holding it back — so there is no `--features` flag here and
# `--run-ignored all` is what selects it.
#
# # Why the backend must be named
#
# Every backend tiles this quad identically by construction, so a run that fell
# back to another backend counts the same cells and proves nothing about the one
# that was wanted. The suite asserts the opened backend against `CRCBL_GPU`, and
# this script refuses to run without it, because `crcbl::backend::open`'s
# automatic order would otherwise silently answer the question for you.
#
# # Pinning a driver, pinning an adapter
#
# Identical to `run-render-e2e.sh` and `run-hal-seam-e2e.sh`, through the same
# shared helpers rather than a third copy that drifts: `CRCBL_VK_ICD` resolves
# the Vulkan ICD via `crcbl_pin_vk_icd`, and `CRCBL_ADAPTER` names a device
# *class* that `crcbl::adapter` resolves — refusing rather than falling back when
# this machine has no adapter of that class. Set, and this script checks it
# *arrived*, off the suite's own output rather than off the variable it exported:
# a variable that never reached the test process and a pin that was honoured look
# identical from outside, because in both cases the suite is green.
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
# this suite is the trap that already sprang: a runner naming the wrong `--test`
# reports success having run nothing. `--no-tests fail` catches an empty
# selection; parsing nextest's own summary catches a filter that matched nothing
# inside a selection that was not empty.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${REPO_ROOT}/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "crcbl tiling e2e"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl tiling e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
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
echo "crcbl tiling e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-tiling-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the adapter
# it opened, and the two cell counts the claim is made of — are only interesting
# on a green run, which is exactly the run nextest captures them on.
set +e
cargo nextest run --locked -p crcbl --test tiling_e2e \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl tiling e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in
# `run-render-e2e.sh` and `run-hal-seam-e2e.sh`: CI sets `CARGO_TERM_COLOR:
# always`, so nextest wraps its counts in escapes and the match below sees no
# digits next to "tests run" — a check that then fires on a suite which ran
# everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# What the validation layer said, which nothing here read until a forward_e2e
# run went green on radv with an `ERROR … vk validation:` line in its log: a
# violation reaches `crcbl_core::log::error!` and the test still passes, because
# the fixture's `finish` checks only the seam's out-of-band channel.
# `tools/vk-validation-log.sh` asks both halves — the layer announced itself,
# and then said nothing — and `CRCBL_VK_VALIDATION_SELF_TEST=1` is how to watch
# this go red.
if [ "$CRCBL_GPU" = vk ] \
    && ! crcbl_validation_saw_nothing "${LOG}.plain" "the crcbl tiling e2e suite"; then
    exit 1
fi

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl tiling e2e" \
    "The ignore attribute or the test binary's name stopped matching the test."; then
    exit 1
fi

# Which adapter the quads were drawn on, from the suite rather than from the
# variable this script exported. The test prints this line each time it opens a
# device — once per surface size — and both are the same answer, so the first is
# read. If the test stops printing it, this is what says so rather than a green
# run quietly losing the check.
ADAPTER="$(grep -F 'crcbl tiling e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl tiling e2e: the suite never named the adapter it drew on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl tiling e2e: ${ADAPTER#*crcbl tiling e2e: }"

# The pin the test process actually saw is printed on that same line, so this
# compares two strings rather than re-deriving the class vocabulary in bash. A
# mismatch means the variable did not reach the test process — the one failure
# `crcbl::adapter` cannot diagnose for itself, because from inside an unset pin
# and no pin are the same thing.
if [ -n "${CRCBL_ADAPTER:-}" ]; then
    case "$ADAPTER" in
        *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
        *)
            echo "crcbl tiling e2e: #####################################################" >&2
            echo "crcbl tiling e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS.   #" >&2
            echo "crcbl tiling e2e: #####################################################" >&2
            echo "crcbl tiling e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported and the suite" >&2
            echo "  reported the line above instead. The variable did not reach the test" >&2
            echo "  process, so the quads were drawn on whatever was enumerated first and" >&2
            echo "  every result above is evidence about a device nobody chose." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl tiling e2e: a 2 m surface tiled by its size on $CRCBL_GPU"
