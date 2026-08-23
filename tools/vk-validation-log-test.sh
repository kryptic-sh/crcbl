#!/usr/bin/env bash
# The test for `tools/vk-validation-log.sh`.
#
#   tools/vk-validation-log-test.sh
#
# Four e2e harnesses decide whether a Vulkan run was clean by calling that
# helper, and a guard nothing exercises is worse than no guard —
# `docs/plan/12-testing.md` makes that the rule and `tools/nextest-summary-test.sh`
# is the other half of it. The helper's whole reason for existing is that a
# *quiet* log and a *clean* log are different things, and only one of the two
# fixtures below can tell them apart, so both belong here.
#
# The lines are the shapes `crcbl_core::log`'s stderr logger actually emits:
# `[   0.0000s LEVEL  module] message`, with the level and the module the
# helper's pattern names. Prints one line per case and exits non-zero on the
# first failure.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/vk-validation-log.sh
source "${REPO_ROOT}/tools/vk-validation-log.sh"

WORK="$(mktemp -d -t crcbl-vk-validation-log-test.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT INT TERM

ERRLOG="${WORK}/stderr"
FAILURES=0

# The helper prints its diagnosis to stderr, and here a rejected fixture is a
# pass — so stderr is captured rather than shown.
run_check() {
    RC=0
    crcbl_validation_saw_nothing "$1" "$2" 2>"$ERRLOG" || RC=$?
    ERR="$(cat "$ERRLOG")"
}

check() {
    local what="$1" want="$2" got="$3"
    if [ "$want" = "$got" ]; then
        echo "ok: ${what}"
    else
        echo "FAILED: ${what}" >&2
        echo "  want: ${want}" >&2
        echo "  got:  ${got}" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

check_contains() {
    local what="$1" needle="$2" haystack="$3"
    case "$haystack" in
        *"$needle"*) echo "ok: ${what}" ;;
        *)
            echo "FAILED: ${what}" >&2
            echo "  expected to contain: ${needle}" >&2
            echo "  in: ${haystack}" >&2
            FAILURES=$((FAILURES + 1))
            ;;
    esac
}

ENABLED='[   0.0021s INFO  crcbl_vk::debug] crcbl-vk: validation enabled (VK_LAYER_KHRONOS_validation), messages go through the debug messenger'

# --- 1. the layer loaded and said nothing -----------------------------------
{
    echo "$ENABLED"
    echo '[   0.0400s INFO  crcbl::engine] shell: first configure at 960x720'
    echo 'sandbox: 120 frames, 600 ticks'
} >"${WORK}/clean.log"
run_check "${WORK}/clean.log" "the sandbox"
check "a clean run is accepted" 0 "$RC"

# --- 2. the layer was never there -------------------------------------------
# **The case the helper exists for.** This log has no validation errors in it
# for the same reason it has no validation: a scan for complaints alone calls it
# clean, which is a green light wired to nothing.
{
    echo '[   0.0018s WARN  crcbl_vk::instance] crcbl-vk: VK_LAYER_KHRONOS_validation is not installed'
    echo 'sandbox: 120 frames, 600 ticks'
} >"${WORK}/no-layer.log"
run_check "${WORK}/no-layer.log" "the sandbox"
check "a run with no layer is rejected" 1 "$RC"
check_contains "and says the layer never loaded" "never loaded the" "$ERR"

# --- 3. the layer complained ------------------------------------------------
{
    echo "$ENABLED"
    echo '[   0.0500s ERROR crcbl_vk::debug] vk validation: VUID-vkCmdCopyBuffer-size-00225: the region exceeds the buffer'
    echo 'sandbox: 120 frames, 600 ticks'
} >"${WORK}/error.log"
run_check "${WORK}/error.log" "the sandbox"
check "an error is rejected" 1 "$RC"
check_contains "and the complaint is quoted" "VUID-vkCmdCopyBuffer-size-00225" "$ERR"

# --- 4. a warning is a complaint too ----------------------------------------
# Where `ValidationReport::assert_clean` draws the line, and what
# `docs/plan/02-vulkan-backend.md`'s P1 exit criterion says.
{
    echo "$ENABLED"
    echo '[   0.0500s WARN  crcbl_vk::debug] vk best practices: this pipeline is not cached'
} >"${WORK}/warning.log"
run_check "${WORK}/warning.log" "the sandbox"
check "a warning is rejected" 1 "$RC"

# --- 5. the teardown leak warning is a different question -------------------
# It comes from `crcbl_vk::device`, and each harness asks it separately. A
# pattern that matched the module loosely would fail this run here instead, and
# the harness's own leak check would then never be the thing that spoke.
{
    echo "$ENABLED"
    echo '[   0.9000s WARN  crcbl_vk::device] crcbl-vk: 3 object(s) still alive at device teardown'
} >"${WORK}/leak.log"
run_check "${WORK}/leak.log" "the sandbox"
check "a teardown leak warning is not a validation complaint" 0 "$RC"

# --- 6. a messenger that panicked ------------------------------------------
# The callback swallowing a panic means the log below it is silent for a reason
# that has nothing to do with the run being clean.
{
    echo "$ENABLED"
    echo '[   0.0500s ERROR crcbl_vk::debug] a panic escaped the Vulkan debug messenger callback'
} >"${WORK}/panic.log"
run_check "${WORK}/panic.log" "the sandbox"
check "a panicking messenger is rejected" 1 "$RC"
check_contains "and says why the scan cannot be trusted" "panic escaped the" "$ERR"

# --- 7. the label reaches the message ---------------------------------------
# Each harness names what it ran, and a report that named nothing would send the
# reader to the wrong log.
run_check "${WORK}/error.log" "quarry"
check_contains "the caller's label is used" "complained about quarry" "$ERR"

# --- 8. the layer answered a provoked violation ------------------------------
COMPLAINT='[   0.0560s ERROR crcbl_vk::debug] vk validation: VUID-vkCmdCopyBuffer-size-00115: vkCmdCopyBuffer(): pRegions[0].size (4096) is greater than the source buffer size (64) minus srcOffset (0).'
{
    echo "$ENABLED"
    echo '[   0.0551s INFO  crcbl_vk::device] crcbl-vk: CRCBL_VK_VALIDATION_PROVOKE records a deliberate 4096-byte copy between two 64-byte buffers'
    echo "$COMPLAINT"
} >"${WORK}/provoked.log"
RC=0
crcbl_validation_layer_checked "${WORK}/provoked.log" "the sandbox" 2>"$ERRLOG" || RC=$?
check "a layer that answered the provocation is accepted" 0 "$RC"

# --- 9. the layer is loaded and checking nothing -----------------------------
# **The case this second question exists for**, and the one every other check
# in this file calls clean: the layer loads, announces itself, and reports
# nothing about a real specification violation. Measured with
# `VK_KHRONOS_VALIDATION_VALIDATE_CORE=false` on layer 1.4.357.
{
    echo "$ENABLED"
    echo '[   0.0551s INFO  crcbl_vk::device] crcbl-vk: CRCBL_VK_VALIDATION_PROVOKE records a deliberate 4096-byte copy between two 64-byte buffers'
} >"${WORK}/checking-nothing.log"
RC=0
crcbl_validation_layer_checked "${WORK}/checking-nothing.log" "the sandbox" 2>"$ERRLOG" || RC=$?
ERR="$(cat "$ERRLOG")"
check "a layer that checked nothing is rejected" 1 "$RC"
check_contains "and says so in those words" "loaded and checking nothing" "$ERR"
# And the log it rejected passes the other question, which is the whole point.
RC=0
crcbl_validation_saw_nothing "${WORK}/checking-nothing.log" "the sandbox" 2>"$ERRLOG" || RC=$?
check "...while the complaint scan calls that same log clean" 0 "$RC"

# --- 10. crcbl-vk's own line cannot answer the grep --------------------------
# The provocation's info line deliberately names neither the entry point nor
# the VUIDs. If it ever did, case 9 would pass on our own output.
check_contains "the provocation's own line is in the rejected log" \
    "CRCBL_VK_VALIDATION_PROVOKE records" "$(cat "${WORK}/checking-nothing.log")"

if [ "$FAILURES" -ne 0 ]; then
    echo "crcbl vk-validation-log test: ${FAILURES} assertion(s) failed" >&2
    exit 1
fi
echo "crcbl vk-validation-log test: every case passed"
