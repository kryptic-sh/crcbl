#!/usr/bin/env bash
# Draw one frame through the engine's own renderer on a named backend and
# compare it against the checked-in golden.
#
#   CRCBL_GPU=mtl crates/crcbl/tests/run-render-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `docs/backlog.md`'s "The render layer has only ever run on Vulkan and wgpu".
# `crcbl-mtl`'s own suite covers the Metal HAL and has never constructed a
# `ForwardRenderer`; `crcbl-vk`'s `vk_e2e/mesh.rs` covers the renderer and only
# on Vulkan. `tests/render_e2e.rs` is the same frame on whichever backend
# `CRCBL_GPU` names, so the renderer stops being a Vulkan-only claim.
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

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl render e2e: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=wgpu $0     # wgpu
NOBACKEND
    exit 1
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO"

LOG="$(mktemp -t crcbl-render-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the two lines this suite prints — the
# selected `GeometryPath`/`BindingModel`/`LightingPath`, and the golden's own
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
sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$LOG" >"${LOG}.plain"

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! grep -qE "Summary \[[^]]*\] +[1-9][0-9]* tests? run" "${LOG}.plain"; then
    echo "crcbl render e2e: the suite reported zero tests run, which is the trap" >&2
    echo "  this script exists to catch — the feature gate or the ignore" >&2
    echo "  attribute stopped matching the test." >&2
    exit 1
fi

echo "crcbl render e2e: the forward renderer drew a frame on $CRCBL_GPU"
