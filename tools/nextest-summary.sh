#!/usr/bin/env bash
# Reading nextest's summary line, for every harness that has to prove its suite
# ran.
#
# **Sourced, never run.** It defines functions and sets one variable in the
# caller's shell, which is what each of the eight e2e harnesses did when they
# carried this inline:
#
#   source "${REPO_ROOT}/tools/nextest-summary.sh"
#   crcbl_nextest_plain "$OUTPUT" "$PLAIN"
#   if ! crcbl_nextest_summary "$PLAIN" "crcbl e2e"; then
#       exit 1
#   fi
#   echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN tests ran"
#
# It lives at the top level rather than under a crate because it is nobody's
# crate knowledge. `crates/crcbl-vk/tests/vulkan-icd.sh` is the model for the
# convention and sits where it does because *which manifest names a driver* is
# `crcbl-vk`'s subject; how nextest spells its own totals is the subject of the
# test runner, and the eight callers span seven crates.
#
# # Why this is a function and not the four lines it replaces
#
# Because it already was those four lines, eight times, and they drifted into
# three different behaviours — which is how the bug below survived in five of
# them:
#
#   Summary [ 0.1s] 15 tests run: 15 passed, 0 skipped     a complete run
#   Summary [ 0.1s] 2/15 tests run: 2 passed, 0 skipped    one nextest cancelled
#
# `grep -Eo '[0-9]+ tests? run' | grep -Eo '^[0-9]+'` reads the digits
# immediately before the words, which in the second shape is the **total**. So a
# run that stopped after two of fifteen reported a healthy fifteen and the gate
# passed a run in which thirteen tests never executed. `run-cli-e2e.sh`,
# `run-wgpu-e2e.sh`, `run-vk-e2e.sh`, `run-wayland-e2e.sh` and `run-x11-e2e.sh`
# all did that. `run-dx12-e2e.sh` had the fix; `run-mtl-e2e.sh` and
# `run-render-e2e.sh` rejected the cut-short shape only as a side effect of an
# anchored pattern the `/` happened to break, and so reported "zero tests run"
# about a run that had in fact run some.
#
# One copy is also the only way the shell test next to this file is worth
# anything: it feeds these functions the summary shapes nextest emits and
# asserts what each one does, which is a thing eight inline copies cannot have.

# Strip ANSI colour from a nextest log into a plain-text copy.
#
# CI sets `CARGO_TERM_COLOR: always`, so nextest emits the count as
# `\e[1m15\e[0m tests run` and a plain-text match sees no digits next to "tests
# run" at all. Every harness learnt that from a guard firing on a run where
# everything had passed.
#
# The destination is the caller's to name because several harnesses grep that
# same copy afterwards for their adapter, driver or validation lines.
crcbl_nextest_plain() {
    local raw="$1" plain="$2"
    sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$raw" >"$plain"
}

# Read the test count out of a colour-stripped nextest log, and refuse anything
# that is not a complete run of at least one test.
#
#   crcbl_nextest_summary <plain-log> <label> [extra line for the zero case…]
#
# Sets `CRCBL_NEXTEST_TESTS_RUN` and returns 0 when the log ends in a complete
# run; otherwise prints why to stderr and returns 1.
#
# **It returns rather than exiting**, which is where it parts company with
# `vulkan-icd.sh`'s convention: the compositor harnesses have a log tail to
# print before they go, and `crcbl-vk`'s has a golden-diff directory to name. A
# helper that exited the caller's shell would take those with it. Every caller
# therefore spells the failure `if ! crcbl_nextest_summary …; then … exit 1; fi`
# — a bare call under `set -e` would abort before the tail was printed.
#
# The trailing arguments are the caller's own account of *why* its suite might
# have run nothing, indented under the zero message: a feature gate and an
# `#[ignore]` are two different ways to select no tests and only the harness
# knows which ones it turned on.
crcbl_nextest_summary() {
    local plain="$1" label="$2"
    shift 2

    CRCBL_NEXTEST_TESTS_RUN=""

    # Anchored on nextest's own summary line rather than on the words alone.
    # These suites run with `--no-capture` and `--success-output immediate`, so
    # the log carries whatever the tests themselves printed, and "12 tests run"
    # in a test's output is not a summary. `run-mtl-e2e.sh` and
    # `run-render-e2e.sh` have anchored this way through every green CI run they
    # have had, so the anchor is known to match what nextest actually emits.
    local summary counts line
    summary="$(grep -Eo 'Summary \[[^]]*\] +([0-9]+/)?[0-9]+ tests? run' "$plain" \
        | tail -1 || true)"
    if [ -z "$summary" ]; then
        echo "${label}: nextest printed no test count at all — the gate is not gating" >&2
        return 1
    fi

    # `Summary [   0.014s] 2/15 tests run` -> `2/15`. The singular is nextest's
    # for a one-test run.
    counts="${summary% tests run}"
    counts="${counts% test run}"
    counts="${counts##* }"

    # A `<ran>/<total>` pair is the cancelled shape. It is read here rather than
    # left to the exit status because the two answer different questions: the
    # status says whether what ran passed, and this says whether what ran was
    # all of it.
    if [ "$counts" != "${counts#*/}" ]; then
        echo "${label}: the run was cancelled after ${counts%%/*} of ${counts#*/} tests —" >&2
        echo "  the rest never executed, so a green count here would be a lie" >&2
        return 1
    fi

    if [ "$counts" -eq 0 ]; then
        echo "${label}: the suite reported no tests run — the gate is not gating" >&2
        for line in "$@"; do
            echo "  $line" >&2
        done
        return 1
    fi

    # The out-parameter, and the only place it is set. Nothing in this file
    # reads it — every reader is in the harness that sourced this one, which is
    # what shellcheck cannot see from here and what `shellcheck -x` on any
    # caller does.
    # shellcheck disable=SC2034
    CRCBL_NEXTEST_TESTS_RUN="$counts"
}
