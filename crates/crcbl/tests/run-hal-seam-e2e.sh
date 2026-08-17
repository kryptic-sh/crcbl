#!/usr/bin/env bash
# Hold every backend to the same `crcbl-hal` seam behaviour, on the one
# `CRCBL_GPU` names.
#
#   CRCBL_GPU=vk crates/crcbl/tests/run-hal-seam-e2e.sh [extra nextest args…]
#
# # What this is for
#
# The seam's own obligations used to be asserted three times over — once in
# `crcbl-vk`'s `tests/vk_e2e/`, once in `crcbl-wgpu`'s `tests/wgpu_e2e.rs`, and
# once inside `crcbl-mtl`'s and `crcbl-dx12`'s `src/` as `#[ignore]`d device
# tests — so each backend was held to its own copy and the copies drifted.
# `tests/hal_seam_e2e.rs` is the single owner: it names no backend type, so this
# script running it four times is the whole matrix.
#
# What stays in a backend's own suite is what is genuinely that backend's:
# validation- and debug-layer wiring, capability tiers, adapter enumeration, and
# refusals that exist because one API has a limit the others do not.
#
# # Why the backend must be named
#
# Every backend implements this seam identically by construction — that is the
# point of the seam — so a run that fell back to another backend passes and
# proves nothing about the one that was wanted. The suite asserts the opened
# backend against `CRCBL_GPU`, and this script refuses to run without it,
# because `crcbl::backend::open`'s automatic order would otherwise silently
# answer the question for you.
#
# # Pinning a driver, pinning an adapter
#
# Identical to `run-render-e2e.sh`, through the same shared helper rather than a
# second copy that drifts: `CRCBL_VK_ICD` resolves the Vulkan ICD via
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
crcbl_pin_vk_icd "crcbl hal seam e2e"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl hal seam e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=dx12 $0     # Direct3D 12, Windows
    CRCBL_GPU=wgpu $0     # wgpu
NOBACKEND
    exit 1
fi

cd "$REPO_ROOT"

# Echoed rather than defaulted: unset means "whatever this machine enumerated
# first", which is a legitimate thing to run deliberately.
echo "crcbl hal seam e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-hal-seam-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the adapter
# it opened, the clear colour that reached memory — are only interesting on a
# green run, which is exactly the run nextest captures them on.
set +e
cargo nextest run --locked -p crcbl --features hal-seam-e2e --test hal_seam_e2e \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl hal seam e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in
# `run-render-e2e.sh` and `run-vk-e2e.sh`: CI sets `CARGO_TERM_COLOR: always`, so
# nextest wraps its counts in escapes and the match below sees no digits next to
# "tests run" — a check that then fires on a suite which ran everything and
# passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl hal seam e2e" \
    "The hal-seam-e2e feature or the ignore attribute stopped matching the test."; then
    exit 1
fi

# Which adapter the seam was exercised on, from the suite rather than from the
# variable this script exported. Every test prints this line when it opens a
# device; the first is read and the rest are the same answer. If the tests stop
# printing it, this is what says so rather than a green run quietly losing the
# check.
ADAPTER="$(grep -F 'crcbl hal seam e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl hal seam e2e: the suite never named the adapter it ran on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl hal seam e2e: ${ADAPTER#*crcbl hal seam e2e: }"

# The pin the test process actually saw is printed on that same line, so this
# compares two strings rather than re-deriving the class vocabulary in bash. A
# mismatch means the variable did not reach the test process — the one failure
# `crcbl::adapter` cannot diagnose for itself, because from inside an unset pin
# and no pin are the same thing.
if [ -n "${CRCBL_ADAPTER:-}" ]; then
    case "$ADAPTER" in
        *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
        *)
            echo "crcbl hal seam e2e: ###################################################" >&2
            echo "crcbl hal seam e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS. #" >&2
            echo "crcbl hal seam e2e: ###################################################" >&2
            echo "crcbl hal seam e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported and the suite" >&2
            echo "  reported the line above instead. The variable did not reach the test" >&2
            echo "  process, so the seam was exercised on whatever was enumerated first and" >&2
            echo "  every result above is evidence about a device nobody chose." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl hal seam e2e: every seam obligation held on $CRCBL_GPU"
