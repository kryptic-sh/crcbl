#!/usr/bin/env bash
# Run the asteroids binary on a named backend, ask it for its last frame, and
# compare that frame against its checked-in golden.
#
#   CRCBL_GPU=vk apps/asteroids/tests/run-asteroids-golden.sh [extra nextest args…]
#
# # What this is for
#
# `docs/plan/12-testing.md` asks every sample for a determinism check **and** a
# golden frame. The determinism half is the crate's own unit tests and the
# `Run asteroids headless against lavapipe` step in `.github/workflows/ci.yml`, and
# neither of them contains a pixel — they pass unchanged whether the frame is
# correct, black or wrongly tonemapped. This is the other half.
#
# The suite is `apps/asteroids/tests/golden.rs`, and what it drives is the
# **compiled binary** rather than a scene it built itself: `--screenshot <PATH>`
# is an engine flag on `crcbl::args::Common`, so the frame it writes is the frame
# a player would have seen, rocks, HUD band and title menu included.
#
# # Why the backend must be named
#
# Every backend draws this frame identically by construction — that is the point
# of the seam — so a run that fell back to another backend produces a frame that
# passes and proves nothing about the one that was wanted. `crcbl::backend::open`
# would otherwise answer the question for you, so the suite itself refuses to run
# without `CRCBL_GPU` and this script refuses to start without it.
#
# # Pinning a driver
#
# `CRCBL_VK_ICD` picks the Vulkan ICD, through the same `crcbl_pin_vk_icd` the
# other harnesses source. There is no `CRCBL_ADAPTER` here: a sample binary picks
# the first adapter that can serve its surface, which is `crcbl::engine`'s rule
# and not `crcbl::adapter`'s. The suite prints the adapter the binary opened —
# read out of the binary's own log rather than out of this script's environment —
# and this script reads that back, so a run always says which device drew.
#
# # ENVIRONMENT
#
#   CRCBL_GPU       Which backend draws. Required; there is no default.
#   CRCBL_VK_ICD    Which Vulkan driver, when `CRCBL_GPU=vk`.
#   CRCBL_BLESS     Rewrite the reference instead of comparing. Never a pass —
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
crcbl_pin_vk_icd "asteroids golden"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
asteroids golden: CRCBL_GPU is not set, so nothing would pin the backend and a
  fallback would pass. Name one:

    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=wgpu $0     # wgpu
    CRCBL_GPU=mtl  $0     # Metal, macOS
    CRCBL_GPU=dx12 $0     # Direct3D 12, Windows
NOBACKEND
    exit 1
fi

cd "$REPO_ROOT"

LOG="$(mktemp -t crcbl-asteroids-golden.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because the lines this suite prints — the
# adapter, the ratios it measured and the golden's own numbers — are only
# interesting on a green run, which is exactly the run nextest captures them on.
set +e
cargo nextest run --locked -p asteroids --features golden-e2e --test golden \
    --run-ignored all --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "asteroids golden: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# CI sets `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and
# a match on digits next to "tests run" sees none — a check that then fires on a
# suite which ran everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

if ! crcbl_nextest_summary "${LOG}.plain" "asteroids golden" \
    "The golden-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi

# Which adapter the frame was drawn on, from the binary's own log rather than
# from the variable this script exported.
ADAPTER="$(grep -F 'asteroids golden: device on ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "asteroids golden: the suite never named the adapter it drew on." >&2
    echo "  The test must print it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
echo "asteroids golden: ${ADAPTER#*asteroids golden: }"

echo "asteroids golden: the field drew, made its claims and matched on $CRCBL_GPU"
