#!/usr/bin/env bash
# Draw the quarry face on the backend `CRCBL_GPU` names, and measure it.
#
#   CRCBL_GPU=vk apps/quarry/tests/run-quarry-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `apps/quarry/tests/device/` is `docs/plan/sample/14-quarry.md`'s milestones
# turned into assertions: the face is resident, it draws, its cut mixes levels,
# the fixed dolly brings detail down the hierarchy without popping, all three
# `GeometryPath` values draw it, the reduction is attributed between the two
# culls, and the surface is shaded by the light it is given.
#
# # Why the backend must be named
#
# The suite runs on the `Null` backend by default so that `cargo test` covers it
# everywhere, and `Null` draws nothing — every assertion about pixels, about the
# per-cluster cut and about the level buckets reports that it was skipped and
# returns. That is the honest behaviour for a recording backend and it is
# useless as a gate, so this script refuses to run without a backend named: a
# green run on `Null` is not evidence that the face draws.
#
# # It is not a golden suite
#
# Nothing here is compared against a committed image, and `docs/backlog.md`
# carries the open question of whether it should be. What it compares instead is
# coverage, cluster counts, drawn levels and triangle counts — all of which are
# stable across drivers in a way pixels are not. Measured: the cuts this suite
# reads are **identical** on radv and on lavapipe, while the rasterised pixel
# counts differ by a handful, which is why the assertions are written on the
# former and not the latter.

set -euo pipefail

TESTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${TESTS_DIR}/../../.." && pwd)"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${REPO_ROOT}/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "crcbl quarry e2e"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

if [ -z "${CRCBL_GPU:-}" ]; then
    cat >&2 <<'NOBACKEND'
crcbl quarry e2e: CRCBL_GPU is not set, so this would run on the Null backend
  and every assertion about a picture would report itself skipped. Name one:

    CRCBL_GPU=vk   $0     # Vulkan
    CRCBL_GPU=mtl  $0     # Metal, macOS
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
echo "crcbl quarry e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER:-<unset>}"

LOG="$(mktemp -t crcbl-quarry-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` because every number this suite exists to record
# — the cut per level, the mean level down the dolly, the triangle count per
# path — is printed by a passing test.
set +e
cargo nextest run --locked -p quarry --test device \
    --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl quarry e2e: the suite failed on $CRCBL_GPU" >&2
    exit "$STATUS"
fi

# CI sets `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and
# a match on "tests run" sees no digits beside it — a check that then fires on a
# suite which ran everything and passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# What the validation layer said, which nothing here read until a forward_e2e
# run went green on radv with an `ERROR … vk validation:` line in its log: a
# violation reaches `crcbl_core::log::error!` and the test still passes, because
# the fixture's `finish` checks only the seam's out-of-band channel.
# `tools/vk-validation-log.sh` asks both halves — the layer announced itself,
# and then said nothing — and `CRCBL_VK_VALIDATION_SELF_TEST=1` is how to watch
# this go red.
if [ "$CRCBL_GPU" = vk ] \
    && ! crcbl_validation_saw_nothing "${LOG}.plain" "the crcbl quarry e2e suite"; then
    exit 1
fi

if ! crcbl_nextest_summary "${LOG}.plain" "crcbl quarry e2e" \
    "The device suite's test names or its filter stopped matching."; then
    exit 1
fi

# Which adapter drew, from the suite rather than from the variable this script
# exported. Every fixture prints it and they all open the same backend, so the
# first is read; if the tests stop printing it, this says so rather than a green
# run quietly losing the check.
ADAPTER="$(grep -F 'crcbl quarry e2e: device on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl quarry e2e: the suite never named the adapter it drew on." >&2
    echo "  The fixture must print it and this script must be able to find it, or" >&2
    echo "  a green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
echo "crcbl quarry e2e: ${ADAPTER#*crcbl quarry e2e: }"

# **The substance ran, rather than reporting itself skipped.** Every assertion
# about a picture answers `Null` by printing that it cannot run and returning,
# which is honest and is also indistinguishable from a pass in the summary
# above. The per-cluster cut is the one line only a device that drew can print,
# so its absence is what says the suite went through the motions.
if ! grep -qF 'crcbl quarry e2e: cut drew ' "${LOG}.plain"; then
    echo "crcbl quarry e2e: no frame reported a per-cluster cut on $CRCBL_GPU." >&2
    echo "  Either no device with an amplification stage opened, or every test" >&2
    echo "  that reads one took its skip path — both of which pass the summary" >&2
    echo "  above while proving nothing about this backend." >&2
    exit 1
fi
