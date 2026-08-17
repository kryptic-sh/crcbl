#!/usr/bin/env bash
# Hold every backend to the same lit-mesh frame, on the one `CRCBL_GPU` names.
#
#   CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `tests/mesh_e2e/` draws the demo scene through the real `ForwardRenderer` and
# the real render graph and then measures what came out: four checked-in
# goldens, the `Rgba16Float` scene target's linear values, the transient pool's
# bound under a resize storm, and the level `docs/plan/25-lod.md`'s uniform cut
# selected. Every one of those lived in `crates/crcbl-vk/tests/vk_e2e/mesh.rs`
# and ran on Vulkan alone.
#
# That file was the last of the five clusters to move and the only one whose
# migration was a redesign: the half of it that opens a device demanding
# `MESH_SHADER | TASK_SHADER` and asserts `GeometryPath::MeshShader` stayed
# behind, because no other backend has a mesh stage to select. Two tests were
# split so that each half asserts something true everywhere it runs; see
# `tests/mesh_e2e/main.rs`.
#
# The suite names no backend type, so this script running it once per backend is
# the whole matrix — the same shape as `run-draw-gen-e2e.sh`.
#
# # Why the backend must be named
#
# Every backend is meant to draw these frames identically, so a run that fell
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
crcbl_pin_vk_icd "crcbl mesh e2e"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl mesh e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
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
echo "crcbl mesh e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-mesh-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the adapter
# it opened, each golden's diff summary, the peak linear value in the HDR target
# and the levels the uniform cut chose — are only interesting on a green run,
# which is exactly the run nextest captures them on.
set +e
cargo nextest run --locked -p crcbl --features mesh-e2e --test mesh_e2e \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl mesh e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in
# `run-hal-seam-e2e.sh`: CI sets `CARGO_TERM_COLOR: always`, so nextest wraps its
# counts in escapes and the match below sees no digits next to "tests run" — a
# check that then fires on a suite which ran everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl mesh e2e" \
    "The mesh-e2e feature or the ignore attribute stopped matching the test."; then
    exit 1
fi

# Which adapter the frames were drawn on, from the suite rather than from the
# variable this script exported. Every test prints this line when it opens a
# device; the first is read and the rest are the same answer. If the tests stop
# printing it, this is what says so rather than a green run quietly losing the
# check.
ADAPTER="$(grep -F 'crcbl mesh e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl mesh e2e: the suite never named the adapter it ran on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl mesh e2e: ${ADAPTER#*crcbl mesh e2e: }"

# The pin the test process actually saw is printed on that same line, so this
# compares two strings rather than re-deriving the class vocabulary in bash. A
# mismatch means the variable did not reach the test process — the one failure
# `crcbl::adapter` cannot diagnose for itself, because from inside an unset pin
# and no pin are the same thing.
if [ -n "${CRCBL_ADAPTER:-}" ]; then
    case "$ADAPTER" in
        *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
        *)
            echo "crcbl mesh e2e: ################################################" >&2
            echo "crcbl mesh e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS. #" >&2
            echo "crcbl mesh e2e: ################################################" >&2
            echo "crcbl mesh e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported and the suite" >&2
            echo "  reported the line above instead. The variable did not reach the test" >&2
            echo "  process, so the frames were drawn on whatever was enumerated first" >&2
            echo "  and every result above is evidence about a device nobody chose." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl mesh e2e: ${CRCBL_NEXTEST_TESTS_RUN} mesh-frame checks held on $CRCBL_GPU"
