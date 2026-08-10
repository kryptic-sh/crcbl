#!/usr/bin/env bash
# The test for `tools/nextest-summary.sh`.
#
#   tools/nextest-summary-test.sh
#
# Every e2e harness in this repository decides whether its suite really ran by
# calling that helper, and a guard nothing exercises is worse than no guard —
# `docs/plan/12-testing.md` makes that the rule and this file is the helper's
# half of it. The five bugs the helper was extracted to fix were all in code that
# had never been fed anything but a healthy log.
#
# So the fixtures here are the shapes nextest actually emits, including the two
# that cost a gate: the cut-short `<ran>/<total>` summary, and a summary wrapped
# in the colour escapes CI's `CARGO_TERM_COLOR: always` produces. Prints one line
# per case and exits non-zero on the first failure.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

WORK="$(mktemp -d -t crcbl-nextest-summary-test.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT INT TERM

ERRLOG="${WORK}/stderr"
FAILURES=0

# Run the helper in *this* shell — it sets `CRCBL_NEXTEST_TESTS_RUN` and the
# point is to read it — with stderr captured rather than printed, since a
# rejected fixture is a pass here.
run_summary() {
    RC=0
    crcbl_nextest_summary "$@" 2>"$ERRLOG" || RC=$?
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

# --- 1. a healthy complete run ---------------------------------------------
cat >"${WORK}/complete.log" <<'LOG'
    Starting 15 tests across 3 binaries
        PASS [   0.011s] crcbl-vk vk_e2e::mesh
------------
     Summary [   0.014s] 15 tests run: 15 passed, 0 skipped
LOG
run_summary "${WORK}/complete.log" "crcbl test"
check "a complete run is accepted" 0 "$RC"
check "a complete run reports its count" 15 "$CRCBL_NEXTEST_TESTS_RUN"

# --- 2. a run nextest cancelled part-way ------------------------------------
# The bug this helper exists for: `15` sits immediately before "tests run" here
# too, so a pattern that reads only those digits calls this a healthy fifteen.
cat >"${WORK}/cancelled.log" <<'LOG'
    Starting 15 tests across 3 binaries
        FAIL [   0.011s] crcbl-vk vk_e2e::mesh
   Canceling due to test failure
------------
     Summary [   0.014s] 2/15 tests run: 1 passed, 1 failed, 13 skipped
LOG
run_summary "${WORK}/cancelled.log" "crcbl test"
check "a cancelled run is rejected" 1 "$RC"
check "a cancelled run reports no count" "" "$CRCBL_NEXTEST_TESTS_RUN"
check_contains "the cancelled message names how many of how many ran" \
    "cancelled after 2 of 15 tests" "$ERR"

# --- 3. a run that selected nothing -----------------------------------------
cat >"${WORK}/zero.log" <<'LOG'
    Starting 0 tests across 3 binaries
------------
     Summary [   0.001s] 0 tests run: 0 passed, 41 skipped
LOG
run_summary "${WORK}/zero.log" "crcbl test" "the feature gate stopped matching."
check "a zero-test run is rejected" 1 "$RC"
check_contains "the zero message names the trap" "reported no tests run" "$ERR"
check_contains "the zero message carries the caller's own reason" \
    "the feature gate stopped matching." "$ERR"

# --- 4. no summary at all ---------------------------------------------------
cat >"${WORK}/nosummary.log" <<'LOG'
    Starting 15 tests across 3 binaries
error: creating test list failed
LOG
run_summary "${WORK}/nosummary.log" "crcbl test"
check "a log with no summary is rejected" 1 "$RC"
check_contains "the absent-summary message says so, rather than claiming zero" \
    "printed no test count at all" "$ERR"

# --- 5. a summary wrapped in ANSI colour ------------------------------------
# Real escape bytes, via `printf '\033'` — CI sets `CARGO_TERM_COLOR: always`
# and this is the shape that made every harness grow a colour-stripped copy.
# A literal backslash-e would test nothing but this file's own quoting.
printf '\033[32m     Summary\033[0m [   0.014s] \033[1m15\033[0m tests run: \033[32m15 passed\033[0m, 0 skipped\n' \
    >"${WORK}/coloured.log"
if grep -q 'Summary \[' "${WORK}/coloured.log"; then
    echo "FAILED: the coloured fixture is not actually coloured" >&2
    FAILURES=$((FAILURES + 1))
else
    echo "ok: the coloured fixture defeats a match on the raw log"
fi
crcbl_nextest_plain "${WORK}/coloured.log" "${WORK}/coloured.plain.log"
run_summary "${WORK}/coloured.plain.log" "crcbl test"
check "a colour-stripped summary is accepted" 0 "$RC"
check "a colour-stripped summary reports its count" 15 "$CRCBL_NEXTEST_TESTS_RUN"

# --- 6. several summaries, the last one counts ------------------------------
# nextest prints one per run, and a harness that runs it twice into one log has
# two. The last is the one the harness is gating on, which is what every copy of
# this guard did with its `tail -1`.
cat >"${WORK}/multi.log" <<'LOG'
     Summary [   0.014s] 15 tests run: 15 passed, 0 skipped
     Summary [   0.002s] 7 tests run: 7 passed, 0 skipped
LOG
run_summary "${WORK}/multi.log" "crcbl test"
check "several summaries take the last" 7 "$CRCBL_NEXTEST_TESTS_RUN"

# --- 7. nextest's singular ---------------------------------------------------
cat >"${WORK}/one.log" <<'LOG'
     Summary [   0.014s] 1 test run: 1 passed, 0 skipped
LOG
run_summary "${WORK}/one.log" "crcbl test"
check "a one-test run is accepted" 0 "$RC"
check "a one-test run reports its count" 1 "$CRCBL_NEXTEST_TESTS_RUN"

if [ "$FAILURES" -ne 0 ]; then
    echo "crcbl nextest-summary test: ${FAILURES} assertion(s) failed" >&2
    exit 1
fi
echo "crcbl nextest-summary test: every case passed"
