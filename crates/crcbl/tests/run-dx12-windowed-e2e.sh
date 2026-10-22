#!/usr/bin/env bash
# Run the D3D12 windowed-presenter suite against this session's Win32 desktop,
# on a pinned adapter.
#
#   crates/crcbl/tests/run-dx12-windowed-e2e.sh [extra nextest args…]
#
# # What this is for
#
# `tests/dx12_windowed_e2e.rs` presents to a real Win32 window through
# `crcbl_app::present::Presenter` on D3D12, keeps frames in flight, resizes, and
# presents again — the path where lyr hit `DXGI_ERROR_INVALID_CALL` from
# `ResizeBuffers`. Every other D3D12 suite presents offscreen, where a
# reconfigure never reaches `ResizeBuffers`. This is the only thing that turns
# the suite on; the file's module docs say what it asserts and with what.
#
# Exits non-zero if this is not Windows, if the suite fails, if it ran no tests
# or was cancelled part-way, if it never named the adapter it presented on, or
# if that adapter is not the one that was pinned.
#
# # ENVIRONMENT
#
#   CRCBL_ADAPTER        The adapter class every presenter here is opened on, in
#                        `crcbl_app::adapter`'s vocabulary. Defaults to `cpu`,
#                        which on Windows is WARP — the same pin the dx12 job's
#                        `crcbl` suites use. A pin that matches nothing fails the
#                        open rather than falling back.
#   CRCBL_DX12_VALIDATION
#                        Whether the D3D12 debug layer is on. Defaulted to `1`
#                        here for `run-dx12-e2e.sh`'s reason: the suite states
#                        what it ran with instead of inheriting it from the build
#                        profile. Every test fails when the layer is not on, so
#                        a machine without Windows' Graphics Tools optional
#                        feature cannot run this suite green.
#   CRCBL_E2E_DX12_WINDOWED_LOG
#                        The `CRCBL_LOG` filter for the run; `info` by default.
#                        The suite's validation check reads the log on the test's
#                        own thread whatever this says.
#
# # Why bash on Windows
#
# `run-dx12-e2e.sh`'s argument: this needs `mktemp`, `tee` and `grep`, which the
# Git Bash on `windows-latest` has, and the guards below are then the one text
# `shellcheck` and a Linux developer can read.

set -euo pipefail

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *)
        echo "crcbl dx12 windowed e2e: D3D12 and the Win32 shell only exist on Windows; this is $(uname -s)" >&2
        exit 1
        ;;
esac

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"
cd "$REPO_ROOT"

# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

# Both exported rather than passed as flags, because the test process reads
# them, not cargo — and defaulted rather than required, so running the script
# *is* running what CI runs.
export CRCBL_ADAPTER="${CRCBL_ADAPTER:-cpu}"
echo "crcbl dx12 windowed e2e: CRCBL_ADAPTER=${CRCBL_ADAPTER}"
export CRCBL_DX12_VALIDATION="${CRCBL_DX12_VALIDATION:-1}"
echo "crcbl dx12 windowed e2e: CRCBL_DX12_VALIDATION=${CRCBL_DX12_VALIDATION}"

LOG="$(mktemp -t crcbl-dx12-windowed-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--test-threads 1` because each test opens a window on the one desktop, and
# the bar test moves a topmost window across it. `--no-fail-fast` for
# `run-dx12-e2e.sh`'s reason: nobody on this team can run this locally, so one
# CI round trip has to report everything it knows. `--success-output immediate`
# publishes the adapter line and the debug layer's teardown counts, which
# nextest would otherwise capture on exactly the runs a reader wants them from.
set +e
CRCBL_LOG="${CRCBL_E2E_DX12_WINDOWED_LOG:-info}" \
    cargo nextest run \
    --locked \
    --package crcbl \
    --features dx12-windowed-e2e \
    --test dx12_windowed_e2e \
    --run-ignored all \
    --no-fail-fast \
    --no-tests fail \
    --test-threads 1 \
    --success-output immediate \
    "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl dx12 windowed e2e: the suite failed (exit $STATUS)" >&2
    exit "$STATUS"
fi

# The colour-stripped copy is load-bearing: CI sets `CARGO_TERM_COLOR: always`,
# so nextest wraps its counts in escapes.
crcbl_nextest_plain "$LOG" "${LOG}.plain"
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl dx12 windowed e2e" \
    "The dx12-windowed-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi

# Which adapter actually presented, off the suite's own output rather than off
# the variable this script exported: a pin that never reached the test process
# and one that was honoured both end in a green summary.
ADAPTER="$(grep -F 'crcbl dx12 windowed e2e: presenting on adapter ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl dx12 windowed e2e: the suite never named the adapter it presented on." >&2
    echo "  Every test's fixture prints it and this script must be able to find it, or a" >&2
    echo "  green run claims evidence about a device nobody wrote down." >&2
    exit 1
fi
# `#*` rather than `#`, because the line arrives indented inside nextest's
# captured-output block.
ADAPTER="${ADAPTER#*crcbl dx12 windowed e2e: }"
echo "crcbl dx12 windowed e2e: ${ADAPTER}"

case "$ADAPTER" in
    *"(CRCBL_ADAPTER=${CRCBL_ADAPTER})"*) ;;
    *)
        echo "crcbl dx12 windowed e2e: THE PIN MISSED. CRCBL_ADAPTER=${CRCBL_ADAPTER} was exported" >&2
        echo "  and the suite reported the line above instead, so every result above is" >&2
        echo "  evidence about a device nobody chose." >&2
        exit 1
        ;;
esac
# `cpu` is the default, and on Windows the only adapter of that class is WARP,
# which the seam spells `DeviceType::Cpu`.
if [ "$CRCBL_ADAPTER" = cpu ]; then
    case "$ADAPTER" in
        *type=Cpu*) ;;
        *)
            echo "crcbl dx12 windowed e2e: CRCBL_ADAPTER=cpu and the adapter that presented is" >&2
            echo "  not the software rasteriser." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl dx12 windowed e2e: $CRCBL_NEXTEST_TESTS_RUN tests presented to a Win32 window on D3D12 and resized it"
