#!/usr/bin/env bash
# Run `crcbl-dx12`'s hardware suite against a D3D12 device that is definitely
# WARP.
#
#   crates/crcbl-dx12/tests/run-dx12-e2e.sh [extra nextest args…]
#
# # There is still no feature, and there should not be one
#
# D3D12 ships in Windows and WARP ships with it, so unlike Metal (no software
# rasteriser at all) and Vulkan (no loader on a bare machine) there is no Windows
# machine where this backend's tests *cannot* run. Hiding them behind a feature
# would remove working coverage from the ordinary Windows job in exchange for
# nothing, which is why `docs/plan/12-testing.md` records the absence as argued
# rather than overlooked.
#
# # Why `--run-ignored only`
#
# What *has* changed is that the device tests are now `#[ignore]`d — not to hide
# them, but so that they can be named. Some of the tests here open a real
# `ID3D12Device`; the rest are pure — the DXGI format tables, the root signature
# layout arithmetic, the DXIL container parse, the adapter pin's decision table —
# and pass on a machine with no GPU, which is what `docs/plan/12-testing.md`'s
# placement rule turns on.
#
# This script used to run the whole crate, so the count the guard below reads was
# unit tests plus device tests. A run in which **every device test had vanished**
# would still have reported a healthy total and cleared the zero check — the same
# "check that cannot fail" shape the guard exists to prevent, one level up.
# `--run-ignored only` selects exactly the tests that need the device, so the
# number this script prints is the number that matters. The pure ones are not
# lost: they run on Linux on every push, because the modules holding them are not
# behind `#[cfg(target_os = "windows")]`, and on Windows they run in the ordinary
# `--workspace --all-features` job.
#
# # What this harness adds over running the tests
#
# What this script is for is the thing a plain run does not give:
#
#   * **it names the adapter.** `adapters()[0]` is the discrete GPU on a
#     workstation and WARP only on a runner that has nothing else, so a green run
#     without a pin is evidence about a device nobody wrote down —
#     `crates/crcbl-vk/tests/vulkan-icd.sh` documents the same trap for Vulkan
#     ICDs. `CRCBL_DX12_ADAPTER` (below) is this backend's `CRCBL_VK_ICD`.
#   * **it checks the pin landed**, off the suite's own output rather than off
#     the variable it exported. `run-vk-e2e.sh` reads the driver out of the
#     adapter line for exactly this reason: a pin the loader ignored and a pin
#     that never reached the process both look like a pass from the outside.
#   * **it fails on a run that tested nothing**, including the cut-short shape
#     that reports a healthy-looking total. `docs/plan/12-testing.md` calls a
#     silently-skipped e2e job a known trap.
#
# Exits non-zero if this is not Windows, if the suite fails, if it ran no tests,
# if it was cancelled part-way, if it never named the adapter it opened, or if
# the adapter it named is not the one that was pinned.
#
# # ENVIRONMENT
#
#   CRCBL_DX12_ADAPTER   Which enumerated adapter every device this suite opens
#                        is opened on. Defaults to `warp` here, which is the
#                        only value `crcbl_dx12::pin` accepts; unset it and run
#                        nextest directly to use whatever DXGI listed first.
#                        A pin that matches no adapter is a hard failure and
#                        never a fallback — see that module.
#   CRCBL_DX12_VALIDATION
#                        Whether the D3D12 debug layer is on. Defaulted to `1`
#                        here rather than left to the build profile, so the
#                        suite states what it ran with instead of inheriting it
#                        — a test binary built `--release` would otherwise turn
#                        validation off and still look identical from outside.
#                        `crcbl-vk`'s harness sets `CRCBL_VK_VALIDATION` for the
#                        same reason.
#
# # Why the debug layer matters more here than a validation layer usually does
#
# `DXGI_ERROR_DEVICE_REMOVED` is reported at the **next** call, not the offending
# one, so without the layer a failure names a symptom and the call that caused it
# is somewhere earlier with nothing pointing at it. The layer reports at the
# call. It needs Windows' **Graphics Tools** optional feature; without it
# `crcbl_dx12::debug` warns, prints `debug layer=false` on the line this script
# checks, and the run still happens — a missing optional component is not a
# reason to refuse to test.
#
# # Why bash, when `run-win32-e2e.ps1` argued for PowerShell
#
# That script starts nothing and needs nothing a Windows shell lacks, and its
# header says so; it chose `pwsh` because `mkfifo` and `trap EXIT` mean nothing
# on Windows. **Neither appears here**: this harness needs `mktemp`, `tee`, `sed`
# and `grep`, all of which the Git Bash that `windows-latest` ships have, and
# GitHub Actions' `shell: bash` selects it. What bash buys in exchange is that
# the guards below are the same text `shellcheck` and a Linux developer can
# exercise — and nobody on this team has a Windows machine to find out on.

set -euo pipefail

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *)
        echo "crcbl dx12 e2e: D3D12 only exists on Windows; this is $(uname -s)" >&2
        echo "               the pin's own decision table is portable and does run here:" >&2
        echo "                 cargo nextest run -p crcbl-dx12 -E 'test(pin::)'" >&2
        exit 1
        ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

# The summary parsing below was this harness's own, and it was the only bash
# copy that told a cancelled run from a whole one. `tools/nextest-summary.sh` is
# that code, moved out to where the other seven can call it instead of carrying
# the version it was written to replace.
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

# The pin. Exported rather than passed as a flag because it is read by the test
# process, not by cargo — and defaulted rather than required so that running the
# script *is* running what CI runs.
export CRCBL_DX12_ADAPTER="${CRCBL_DX12_ADAPTER:-warp}"
echo "crcbl dx12 e2e: CRCBL_DX12_ADAPTER=${CRCBL_DX12_ADAPTER}"

# The debug layer, exported for the same reason and read by the same process.
export CRCBL_DX12_VALIDATION="${CRCBL_DX12_VALIDATION:-1}"
echo "crcbl dx12 e2e: CRCBL_DX12_VALIDATION=${CRCBL_DX12_VALIDATION}"

LOG="$(mktemp -t crcbl-dx12-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$LOG" "${LOG}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--no-fail-fast`, and it is not a preference: this is a suite nobody on this
# team can run locally, so a round trip costs a whole CI run and it has to report
# everything it knows in one. That is the lesson `run-win32-e2e.ps1` records from
# its first CI run, which stopped after two of fifteen tests.
# `--success-output immediate` publishes the adapter report lines, which nextest
# would otherwise capture on exactly the run a reader wants to read them on.
set +e
cargo nextest run \
    --locked \
    --package crcbl-dx12 \
    --run-ignored only \
    --no-fail-fast \
    --no-tests fail \
    --success-output immediate \
    "$@" 2>&1 | tee "$LOG"
STATUS=${PIPESTATUS[0]}
set -e

# The colour-stripped copy is load-bearing for every match below — CI sets
# `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and a
# plain-text match sees no digits next to "tests run". Both bash harnesses and
# the PowerShell one do this, and each of them learnt it from a guard firing on a
# run where everything had in fact passed.
crcbl_nextest_plain "$LOG" "${LOG}.plain"

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl dx12 e2e: the suite failed (exit $STATUS)" >&2
    exit "$STATUS"
fi

# nextest prints `<n> tests run:` for a complete run and `<ran>/<total> tests
# run:` for one it cancelled, and telling those apart is the whole job of the
# helper this now calls — this is where that code was written, moved out to
# `tools/nextest-summary.sh` so the other seven harnesses stop carrying the
# version it was written to replace.
if ! crcbl_nextest_summary "${LOG}.plain" "crcbl dx12 e2e" \
    "This suite is not feature-gated, so an empty selection means the ignore" \
    "attribute stopped matching the device tests, or a filter argument matched" \
    "nothing."; then
    exit 1
fi
COUNTS="$CRCBL_NEXTEST_TESTS_RUN"

# Which adapter the suite actually opened, from the suite rather than from the
# variable this script exported. `the_pinned_adapter_opens_a_device_and_names_itself`
# is what prints it; if that test is renamed or stops printing, this is what says
# so rather than a green run quietly losing the check.
ADAPTER="$(grep -F 'crcbl-dx12 e2e: device on ' "${LOG}.plain" | head -1 || true)"
if [ -z "$ADAPTER" ]; then
    echo "crcbl dx12 e2e: the suite never named the adapter it opened." >&2
    echo "                crcbl_dx12::instance's the_pinned_adapter_opens_a_device_and_names_itself" >&2
    echo "                must print it and this script must be able to find it, or a green" >&2
    echo "                run claims evidence it does not have." >&2
    exit 1
fi
ADAPTER="${ADAPTER#*crcbl-dx12 e2e: device on }"
echo "crcbl dx12 e2e: ${ADAPTER}"

# `warp` is the only value the pin accepts, and `DeviceType::Cpu` is how the seam
# spells a software rasteriser — so anything else on this line means the pin did
# not reach the test process, which is the one failure the pin cannot diagnose
# for itself.
case "$ADAPTER" in
    *type=Cpu*) ;;
    *)
        echo "crcbl dx12 e2e: ############################################################" >&2
        echo "crcbl dx12 e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS IT IS.   #" >&2
        echo "crcbl dx12 e2e: ############################################################" >&2
        echo "crcbl dx12 e2e: CRCBL_DX12_ADAPTER=${CRCBL_DX12_ADAPTER} was exported and the suite" >&2
        echo "                opened a device on an adapter that is not the software" >&2
        echo "                rasteriser. Either the variable did not reach the test" >&2
        echo "                process or crcbl_dx12::pin stopped honouring it; both make" >&2
        echo "                every result above evidence about a device nobody chose." >&2
        exit 1
        ;;
esac

# Whether validation was on, off the suite's own output — the same rule the
# adapter check above follows, and for the same reason: a variable that never
# reached the test process and a layer Windows does not have both look like a
# green run from outside. This is a *report*, not a gate: a machine without the
# Graphics Tools feature must still be able to run the suite, so the line being
# absent is the failure and `debug layer=false` is merely news.
VALIDATION="$(grep -F 'crcbl-dx12 e2e: debug layer=' "${LOG}.plain" | head -1 || true)"
if [ -z "$VALIDATION" ]; then
    echo "crcbl dx12 e2e: the suite never said whether validation was on." >&2
    echo "                crcbl_dx12::debug's" >&2
    echo "                a_fresh_device_says_whether_it_is_validated_and_is_not_already_removed" >&2
    echo "                must print it and this script must be able to find it, or a green" >&2
    echo "                run claims evidence it does not have." >&2
    exit 1
fi
echo "crcbl dx12 e2e: ${VALIDATION#*crcbl-dx12 e2e: }"
case "$VALIDATION" in
    *"debug layer=false"*)
        echo "crcbl dx12 e2e: the D3D12 debug layer is NOT on for this run. A validation" >&2
        echo "                error will be reported one call late, if at all. Install the" >&2
        echo "                Graphics Tools optional feature to fix that." >&2
        ;;
esac

echo "crcbl dx12 e2e: $COUNTS tests ran against WARP"
