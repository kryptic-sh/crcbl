#!/usr/bin/env bash
# Run `crcbl-mtl`'s hardware suite — every test that opens a real Metal device,
# up to and including the ones that make the GPU execute a shader program.
#
#   crates/crcbl-mtl/tests/run-mtl-e2e.sh [extra nextest args…]
#
# # Who runs this
#
# **CI runs it, on `macos-latest`.** The device that image exposes is an
# `Apple Paravirtual device`, and a paravirtual device was long assumed unable
# to execute a shader — but that was generalised from macos-14, the one hosted
# image whose `MTLCreateSystemDefaultDevice()` returns nil. macos-15 and
# macos-26 both run a compute dispatch and a triangle draw correctly, and
# `macos-latest` resolves to macos-26. `docs/backlog.md` has the measurements.
#
# **A person on a real Mac runs it too, and that is still the only thing that
# covers a non-virtual GPU.** The CI job says the suite passes on Apple's
# paravirtual device; it says nothing about a discrete or an unvirtualised
# Apple GPU, and Metal has no software rasteriser to cross-check against the
# way Vulkan has lavapipe.
#
# The CI job holds one test out — the layer swapchain's drawable acquisition,
# which is gated on a headless container vending a `CAMetalLayer` drawable and
# not on shader execution. `.github/workflows/ci.yml` explains why there and
# passes the filter; this script excludes nothing on its own.
#
# # Running one test at a time
#
# Extra arguments go straight to nextest, so a person on a real Mac runs any
# one test by name:
#
#   crates/crcbl-mtl/tests/run-mtl-e2e.sh -E 'test(a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear)'
#
# # Why `--run-ignored only` and not `all`
#
# This harness used to pass `--run-ignored all`, which ran the whole crate — the
# hardware tests *and* the pure ones (format tables, handle tagging, the extent
# arithmetic, the present ledger) that pass on a machine with no GPU at all. The
# count the guard below reads was then unit tests plus device tests, and a run in
# which **every device test had vanished** would still report a healthy total and
# clear the zero check. That is the same "check that cannot fail" shape the guard
# exists to prevent, one level up.
#
# `docs/plan/12-testing.md`'s placement rule is what makes the narrower selection
# possible: a test lives ungated in `src/` iff it can pass with no GPU, and a
# test that needs a live device is `#[ignore]`d. So `--run-ignored only` selects
# exactly the device tests, and the number this script prints is the number that
# matters. The pure ones are not lost — they are what
# `cargo nextest run --workspace --all-features` runs on every push.
#
# `--features mtl-e2e` stays, and is not redundant with it: the handful of tests
# that make the GPU execute a shader are feature-gated *as well as* `#[ignore]`d,
# so without the feature they are not compiled and `--run-ignored only` cannot
# select what does not exist.
#
# # The zero-tests check is the point
#
# `docs/plan/12-testing.md` calls a silently-skipped e2e suite a known trap, and
# a suite that is both feature-gated and `#[ignore]`d has two ways to run
# nothing. Parsing the summary is what closes that.
#
# # ENVIRONMENT
#
#   MTL_DEBUG_LAYER      Metal's API-validation layer. Defaulted to `1` here
#                        rather than inherited, so this run states what it
#                        checked instead of depending on a shell nobody reads —
#                        `run-vk-e2e.sh` defaults `CRCBL_VK_VALIDATION` and
#                        `run-dx12-e2e.sh` defaults `CRCBL_DX12_VALIDATION` for
#                        the same reason. **It has to be set before the process
#                        starts**: Metal reads it when the framework loads, so
#                        no code in `crcbl-mtl` can turn it on for itself.
#   MTL_DEBUG_LAYER_ERROR_MODE
#   MTL_DEBUG_LAYER_WARNING_MODE
#                        What the layer does about a violation: `ignore`,
#                        `assert`, `abort` or `nslog`. Both default to `abort`
#                        here, and the warning one is not an oversight —
#                        `crcbl-vk`'s line is zero errors **and** zero warnings,
#                        and this backend is held to the same one.
#   MTL_SHADER_VALIDATION
#                        GPU-side bounds checking inside a running kernel.
#                        Defaulted to `1`. Whether the device supports it is not
#                        knowable from inside the process — Metal says so on
#                        stderr and carries on — so this is asked for and
#                        reported, never asserted.
#   CRCBL_MTL_VALIDATION Whether the suite *requires* the layer to have been
#                        interposed. Defaulted to `1`; set it to `0` to run
#                        against an unvalidated device and have the log say so.
#
# # What Metal can and cannot report, and what this suite therefore asserts
#
# Neither Vulkan's messenger callback nor D3D12's info queue has a Metal
# equivalent. An API misuse is **printed and then acted on** — at `abort`, the
# process dies — so there is no message list to count and no assertion this
# suite can make about one. What `crcbl_mtl::fault` asserts at every device
# test's teardown is the two things that *are* observable:
#
#   * Metal really did interpose its validation layer on the device, read off
#     the device object's Objective-C class, and
#   * no command buffer the device submitted ended in `MTLCommandBufferStatus`
#     `Error` — which is where shader validation's findings and every GPU fault
#     arrive, and which nothing else in this backend noticed for a submission
#     whose result no test waited on.
#
# **That is weaker than the other two backends' gates and is not claimed to be
# parity.** A violation caught by API validation reaches this script as a killed
# test process, not as a failed assertion. `crates/crcbl-mtl/src/fault.rs` argues
# the whole of it.

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
    echo "crcbl mtl e2e: Metal only exists on macOS; this is $(uname -s)" >&2
    exit 1
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO"

# Reading nextest's summary is `tools/nextest-summary.sh`'s job, in one copy
# rather than eight. This harness rejected a cancelled run only by accident —
# its anchored pattern happened to break on the `/` in `2/15 tests run` — and so
# reported "zero tests run" about a run that had run two.
# shellcheck source=tools/nextest-summary.sh
source "${REPO}/tools/nextest-summary.sh"

LOG="$(mktemp -t crcbl-mtl-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# Metal's own switches, exported rather than passed as flags because they are
# read by the test process — by the Metal framework inside it, in fact, before
# any of this crate's code runs. Defaulted rather than required so that running
# this script *is* running what CI runs.
export MTL_DEBUG_LAYER="${MTL_DEBUG_LAYER:-1}"
# `nslog`, not `abort`. **`abort` is not a value Metal accepts**, and it does
# not ignore one it does not know: `MTLGetEnvCase` asserts, so every device
# creation dies with `Assertion failed: (0) … MTLUtils_Internal.h, line 100` and
# the whole suite SIGABRTs before running. That is what run 31452339144 did, 71
# of 71. The accepted values are `ignore`, `assert` and `nslog`.
#
# `nslog` reports each finding to stderr and lets the process continue, which is
# what a first run wants: the suite has never executed under this layer, so the
# job now is to read what it says rather than to die on the first line of it.
# `assert` is the stricter setting and is where this should end up once the log
# is clean — `docs/backlog.md` carries that as the follow-up.
export MTL_DEBUG_LAYER_ERROR_MODE="${MTL_DEBUG_LAYER_ERROR_MODE:-nslog}"
export MTL_DEBUG_LAYER_WARNING_MODE="${MTL_DEBUG_LAYER_WARNING_MODE:-nslog}"
export MTL_SHADER_VALIDATION="${MTL_SHADER_VALIDATION:-1}"
# And this suite's own: whether a run that did not get the layer fails.
export CRCBL_MTL_VALIDATION="${CRCBL_MTL_VALIDATION:-1}"
echo "crcbl mtl e2e: MTL_DEBUG_LAYER=${MTL_DEBUG_LAYER}" \
    "MTL_DEBUG_LAYER_ERROR_MODE=${MTL_DEBUG_LAYER_ERROR_MODE}" \
    "MTL_DEBUG_LAYER_WARNING_MODE=${MTL_DEBUG_LAYER_WARNING_MODE}"
echo "crcbl mtl e2e: MTL_SHADER_VALIDATION=${MTL_SHADER_VALIDATION}" \
    "CRCBL_MTL_VALIDATION=${CRCBL_MTL_VALIDATION}"

# `--success-output immediate` publishes the validation report line, which
# nextest would otherwise capture on exactly the run a reader wants to read it
# on. `run-dx12-e2e.sh` passes it for the same line.
set +e
cargo nextest run --locked -p crcbl-mtl --features mtl-e2e \
    --run-ignored only --no-tests fail --success-output immediate "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

# The colour-stripped copy is load-bearing, exactly as it is in
# `crates/crcbl-vk/tests/run-vk-e2e.sh`: CI sets `CARGO_TERM_COLOR: always`, so
# nextest wraps its counts in escapes and the match below sees no digits next to
# "tests run". Without this the check fires on a suite that ran everything and
# passed — which is what run 31045734181 did, reporting the zero-tests trap at
# `102 tests run: 102 passed`.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

# What validation the suite actually ran under, off its own output rather than
# off the variables this script exported — the rule `run-dx12-e2e.sh` follows for
# its adapter and its debug layer, and for the same reason: a variable that never
# reached the test process and a layer Metal declined to install both look like a
# green run from outside.
#
# Read **before** the failure gate, because when the layer is missing and
# CRCBL_MTL_VALIDATION asked for it, this is the whole explanation for the wall
# of failures that follows.
VALIDATION="$(grep -F 'crcbl-mtl e2e: api validation=' "${LOG}.plain" | head -1 || true)"
case "$VALIDATION" in
    *"api validation=false"*)
        echo "crcbl mtl e2e: ############################################################" >&2
        echo "crcbl mtl e2e: # METAL DID NOT INTERPOSE ITS VALIDATION LAYER.            #" >&2
        echo "crcbl mtl e2e: ############################################################" >&2
        echo "               An API misuse would have gone unreported. The class Metal" >&2
        echo "               handed back is on the api-validation line this script prints" >&2
        echo "               at the end; if MTL_DEBUG_LAYER was set, macOS has renamed the" >&2
        echo "               wrapper and crcbl_mtl::fault's layer_wrapped_device needs" >&2
        echo "               updating." >&2
        case "$(printf '%s' "$CRCBL_MTL_VALIDATION" | tr '[:upper:]' '[:lower:]')" in
            0 | false | no | off)
                echo "               CRCBL_MTL_VALIDATION=${CRCBL_MTL_VALIDATION} asked for no validation, so the" >&2
                echo "               tests below passed against an unvalidated device." >&2
                ;;
            *)
                echo "               CRCBL_MTL_VALIDATION=${CRCBL_MTL_VALIDATION} asked for it, so every device test" >&2
                echo "               fails at teardown rather than passing while checking nothing." >&2
                ;;
        esac
        ;;
esac

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl mtl e2e: the suite failed" >&2
    exit "$STATUS"
fi

# nextest reports its own totals; counting lines of its output would silently
# pick up headers and land a number that is close and wrong.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl mtl e2e" \
    "The mtl-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi

# The line itself has to exist. `$VALIDATION` was read above, before the failure
# gate; what is left here is the check that the suite said anything at all, which
# only means something on a run that got this far.
if [ -z "$VALIDATION" ]; then
    echo "crcbl mtl e2e: the suite never said what validation it ran under." >&2
    echo "               crcbl_mtl::fault's" >&2
    echo "               a_fresh_device_says_what_validation_it_is_running_under" >&2
    echo "               must print it and this script must be able to find it, or a green" >&2
    echo "               run claims evidence it does not have." >&2
    exit 1
fi
echo "crcbl mtl e2e: ${VALIDATION#*crcbl-mtl e2e: }"

echo "crcbl mtl e2e: the hardware suite ran $CRCBL_NEXTEST_TESTS_RUN tests against a Metal device"
