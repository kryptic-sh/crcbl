#!/usr/bin/env pwsh
# Run `crcbl-shell`'s Win32 end-to-end suite against the desktop this machine
# already has.
#
#   crates/crcbl-shell/tests/run-win32-e2e.ps1 [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` stays green on the ordinary
# Windows job. This script is the only thing that turns them on, and CI runs
# this script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a
# known trap, so the script fails when the suite reports zero tests run.
#
# Exits non-zero if nextest fails, if no tests ran, or if the count cannot be
# read out of the output at all.
#
# # Decision: there is nothing to start, so nothing is started
#
# `run-wayland-e2e.sh` and `run-x11-e2e.sh` are two hundred lines each because
# each has to launch a window system, poll for its socket with a deadline, watch
# the child for an early exit, and tear it all down. **Windows needs none of
# that**: a `windows-latest` runner boots into a session with a window station, a
# desktop and a cursor on it, which is precisely why P5C could move out of P14 in
# the first place (`docs/plan/ROADMAP.md`, 2026-08-04).
#
# So this script is the two things those two scripts do that are not about
# starting a compositor — run the suite, and assert that it ran — and porting the
# rest would be ceremony for a problem this platform does not have. What is left
# is deliberately in PowerShell rather than bash: `windows-latest` has `pwsh` and
# does not have a shell where `mkfifo` and `trap EXIT` mean anything.
#
# # Why the suite is serialised
#
# `--test-threads 1`, and it is not about speed. Everything this suite touches is
# **session-global**: the foreground window, the cursor position, the cursor's
# clip rectangle and the desktop clipboard are one of each per session, not one
# per process. Two tests running at once would take the foreground from each
# other, and the one that lost would report that injected input never arrived.

$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

# tests/ -> crcbl-shell/ -> crates/ -> the repository root.
$repoRoot = Split-Path -Parent (
    Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSCommandPath)))
. (Join-Path $repoRoot 'tools/nextest-summary.ps1')

$runtimeDir = Join-Path ([System.IO.Path]::GetTempPath()) "crcbl-win32-e2e-$PID"
New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
$log = Join-Path $runtimeDir 'nextest.log'

try {
    Write-Host "crcbl e2e: running the Win32 suite against this session's desktop"

    Push-Location $repoRoot
    try {
        # `2>&1 | Tee-Object` rather than a plain redirect, so the run is visible
        # while it happens *and* readable afterwards. `$ErrorActionPreference` is
        # `Continue` above precisely so that a native command writing to stderr —
        # which cargo does constantly, for progress — is not turned into a
        # terminating error by the merge.
        # `--no-fail-fast`, and it is not a preference. nextest stops at the
        # first failure by default, and the first CI run of this suite reported
        # `2/15 tests run` — thirteen tests never executed, and each round trip
        # on a suite nobody can run locally costs half an hour. A remote suite
        # has to report everything it knows in one round; the zero-count gate
        # below is what keeps "no failures" from meaning "nothing ran", and the
        # partial-run check beside it is what keeps a cut-short run from
        # reading as a whole one.
        # The filter takes the in-crate `win32::shell::tests` as well as the
        # e2e binary: the pointer-clip tests there are `#[ignore]`d out of the
        # ordinary sweep on the grounds that this script runs them, and a
        # `--test win32_e2e` selection left them running nowhere at all.
        cargo nextest run `
            --locked `
            --package crcbl-shell `
            --features win32-e2e `
            -E 'binary(win32_e2e) | test(/^win32::shell::tests::/)' `
            --run-ignored all `
            --test-threads 1 `
            --no-fail-fast `
            @args 2>&1 | Tee-Object -FilePath $log
        $status = $LASTEXITCODE
    } finally {
        Pop-Location
    }

    if ($status -ne 0) {
        Write-Error "crcbl e2e: the suite failed (exit $status)"
        exit $status
    }

    # The trap `docs/plan/12-testing.md` names by name: a job that skips
    # everything and reports success is worse than no job — and so is one
    # nextest cancelled after two tests, whose summary still ends in the total
    # it never reached. `tools/nextest-summary.ps1` is the one PowerShell copy of
    # that guard, and `tools/nextest-summary-test.sh` holds it to the bash one.
    $plain = ConvertTo-CrcblNextestPlain -Text (Get-Content -Raw -Path $log)
    $ran = Get-CrcblNextestTestsRun -Plain $plain -Label 'crcbl e2e' `
        -ZeroReason 'The win32-e2e feature or the ignore attribute stopped matching the tests.'
    if ($null -eq $ran) {
        exit 1
    }
    Write-Host "crcbl e2e: $ran tests ran against this session's desktop"
} finally {
    Remove-Item -Recurse -Force -Path $runtimeDir -ErrorAction SilentlyContinue
}
