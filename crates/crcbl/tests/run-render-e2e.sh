#!/usr/bin/env bash
# Draw every scene through the engine's own renderers on a named backend and
# compare each frame against its checked-in golden.
#
#   CRCBL_GPU=mtl crates/crcbl/tests/run-render-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `docs/backlog.md`'s "The render layer has only ever run on Vulkan and wgpu".
# `crcbl-mtl`'s own suite covers the Metal HAL and has never constructed a
# `ForwardRenderer`; `crcbl-vk`'s `vk_e2e/mesh.rs` covers the renderer and only
# on Vulkan. `tests/render_e2e.rs` is the same frames on whichever backend
# `CRCBL_GPU` names, so the renderer stops being a Vulkan-only claim.
#
# One test per scene — cube, sprite and UI — because `sprite.slang` and
# `ui.slang` are the two shaders that have diverged per target in this repo, and
# before those scenes were here they were compared across targets only by the
# vk-against-wgpu gate that went with `crcbl-wgpu`. See
# `docs/backlog.md`'s "the four-backend compare is more scenes in `render_e2e`".
#
# # Why the backend must be named
#
# Every backend draws this scene identically by construction — that is the whole
# point of the seam — so a run that fell back to another backend produces a
# frame that passes and proves nothing about the one that was wanted. The test
# asserts the opened backend against `CRCBL_GPU`, and this script refuses to run
# without it, because `crcbl::backend::open`'s automatic order would otherwise
# silently answer the question for you.
#
# # Pinning a driver
#
# Vulkan's ICD is chosen by `VK_DRIVER_FILES` / `VK_ICD_FILENAMES` at instance
# creation. Set `CRCBL_VK_ICD` and this script resolves both, through the same
# `crcbl_pin_vk_icd` `run-vk-e2e.sh` uses — one copy, sourced, rather than a
# second that drifts.
#
# It did not always: the ICD was the caller's job and the CI step wrote Debian's
# `lvp_icd.x86_64.json` straight into the environment. On a runner whose file
# was Arch's `lvp_icd.json` the loader answered `ERROR_INCOMPATIBLE_DRIVER`,
# which is also what it says for a manifest that names an incompatible driver —
# so the failure named neither the file nor the mistake. Resolving it here is
# what stops the next caller repeating that.
#
# # Pinning an adapter
#
# `CRCBL_ADAPTER` names a device *class* — `cpu`, `integrated`, `discrete` or
# `virtual` — and `crcbl::adapter` resolves it, refusing rather than falling back
# when this machine has no adapter of that class. Leave it unset and the frame is
# drawn on whatever the backend enumerated first, which is what every run before
# this pin existed did.
#
# It is not optional on a machine whose first adapter is not a working device.
# That is `windows-latest`: the D3D12 HAL suite passes 155/155 on WARP and the
# frame that followed died on its first buffer with `DXGI_ERROR_DEVICE_REMOVED`,
# before drawing anything, because nothing above the seam could say which adapter
# it meant.
#
# Set, and this script checks it *arrived* — off the suite's own output rather
# than off the variable it exported, exactly as `run-dx12-e2e.sh` does. A
# variable that never reached the test process and a pin that was honoured look
# identical from outside: in both cases the suite is green.
#
# # ENVIRONMENT
#
#   CRCBL_GPU       Which backend draws. Required; there is no default.
#   CRCBL_ADAPTER   Which adapter class inside it. Optional; unset takes the
#                   first one enumerated.
#   CRCBL_VK_ICD    Which Vulkan driver, when `CRCBL_GPU=vk`. See above.
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
crcbl_pin_vk_icd "crcbl render e2e"

# And reading nextest's own summary is `tools/nextest-summary.sh`'s, for the
# same reason. This harness rejected a cancelled run only by accident — its
# anchored pattern happened to break on the `/` in `2/15 tests run` — and so
# reported "zero tests run" about a run that had run two.
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl render e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=dx12 $0     # Direct3D 12, Windows
    CRCBL_GPU=wgpu $0     # wgpu
NOBACKEND
    exit 1
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO"

# Echoed rather than defaulted: unset means "whatever this machine enumerated
# first", which is a legitimate thing to run deliberately and is what every
# caller before the pin existed got.
echo "crcbl render e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-render-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the
# selected `GeometryPath`/`BindingModel`/`LightingPath`, and each golden's own
# numbers — are only interesting on a green run, which is exactly the run
# nextest captures them on.
set +e
cargo nextest run --locked -p crcbl --features render-e2e --test render_e2e \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl render e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing, exactly as it is in
# `run-vk-e2e.sh` and `run-mtl-e2e.sh`: CI sets `CARGO_TERM_COLOR: always`, so
# nextest wraps its counts in escapes and the match below sees no digits next to
# "tests run" — a check that then fires on a suite which ran everything and
# passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl render e2e" \
    "The render-e2e feature or the ignore attribute stopped matching the test."; then
    exit 1
fi

# Which adapter the frames were drawn on, from the suite rather than from the
# variable this script exported. Every scene's test prints this line and they
# all open the same device, so the first one is read and the rest are the same
# answer; if the tests stop printing it, this is what says so rather than a
# green run quietly losing the check.
ADAPTER="$(grep -F 'crcbl render e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl render e2e: the suite never named the adapter it drew on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl render e2e: ${ADAPTER#*crcbl render e2e: }"

# The pin the test process actually saw is printed on that same line, so this
# compares two strings rather than re-deriving the class vocabulary in bash. A
# mismatch means the variable did not reach the test process — the one failure
# `crcbl::adapter` cannot diagnose for itself, because from inside an unset pin
# and no pin are the same thing.
if [ -n "${CRCBL_ADAPTER:-}" ]; then
    case "$ADAPTER" in
        *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
        *)
            echo "crcbl render e2e: ######################################################" >&2
            echo "crcbl render e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS.   #" >&2
            echo "crcbl render e2e: ######################################################" >&2
            echo "crcbl render e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported and the suite" >&2
            echo "  reported the line above instead. The variable did not reach the test" >&2
            echo "  process, so the frame was drawn on whatever was enumerated first and" >&2
            echo "  every result above is evidence about a device nobody chose." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl render e2e: every scene drew and matched its golden on $CRCBL_GPU"
