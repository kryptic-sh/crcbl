# Reading nextest's summary line, for the PowerShell harnesses that have to
# prove their suite ran.
#
# **Dot-sourced, never run.** It defines two functions in the caller's scope:
#
#   . (Join-Path $repoRoot 'tools/nextest-summary.ps1')
#   $plain = ConvertTo-CrcblNextestPlain -Text (Get-Content -Raw -Path $log)
#   $ran = Get-CrcblNextestTestsRun -Plain $plain -Label 'crcbl e2e' `
#       -ZeroReason 'The win32-e2e feature or the ignore attribute stopped matching the tests.'
#   if ($null -eq $ran) { exit 1 }
#
# The PowerShell twin of `tools/nextest-summary.sh`, which every bash harness
# sources; `run-vk-e2e.ps1` and `run-win32-e2e.ps1` cannot source a bash file.
# The two are one piece of knowledge in two languages, so they are held together
# the only way that survives an edit: `tools/nextest-summary-test.sh` feeds the
# same fixture logs through both and asserts the same outcome from each. A
# change to one that is not made to the other goes red there. The reasoning for
# every rule below is in the bash file and is not repeated here.

# Strip ANSI colour from a nextest log, returning the plain text.
#
# CI sets `CARGO_TERM_COLOR: always`, so nextest emits the count as
# `\e[1m15\e[0m tests run` and a plain-text match sees no digits next to "tests
# run" at all. It returns the text rather than writing a file, unlike the bash
# helper, because both callers hold the log in a variable and match their
# adapter and validation lines against the same plain copy afterwards.
function ConvertTo-CrcblNextestPlain {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    $Text -replace "$([char]27)\[[0-9;]*[a-zA-Z]", ''
}

# Read the test count out of a colour-stripped nextest log, and refuse anything
# that is not a complete run of at least one test.
#
# Returns the count when the log ends in a complete run; otherwise writes why
# to stderr and returns nothing, so every caller spells the failure
# `if ($null -eq $ran) { exit 1 }` — returning rather than exiting for the
# reason the bash helper gives: the caller may have more to print before it
# goes. `-ZeroReason` is the caller's own account of why its suite might have
# selected nothing, indented under the zero message.
function Get-CrcblNextestTestsRun {
    param(
        [Parameter(Mandatory)][AllowEmptyString()][string]$Plain,
        [Parameter(Mandatory)][string]$Label,
        [string[]]$ZeroReason = @()
    )

    # Anchored on nextest's own summary line, as the bash helper is: these
    # suites print their own output into the log, and "12 tests run" in a
    # test's output is not a summary. The last one counts.
    $hits = [regex]::Matches($Plain, 'Summary \[[^\]]*\] +(?:(\d+)/)?(\d+) tests? run')
    if ($hits.Count -eq 0) {
        [Console]::Error.WriteLine("${Label}: nextest printed no test count at all — the gate is not gating")
        return
    }
    $summary = $hits[$hits.Count - 1]

    # A `<ran>/<total>` pair is the cancelled shape.
    if ($summary.Groups[1].Success) {
        $ran, $total = $summary.Groups[1].Value, $summary.Groups[2].Value
        [Console]::Error.WriteLine("${Label}: the run was cancelled after $ran of $total tests —")
        [Console]::Error.WriteLine('  the rest never executed, so a green count here would be a lie')
        return
    }

    $ran = [int]$summary.Groups[2].Value
    if ($ran -eq 0) {
        [Console]::Error.WriteLine("${Label}: the suite reported no tests run — the gate is not gating")
        foreach ($line in $ZeroReason) {
            [Console]::Error.WriteLine("  $line")
        }
        return
    }
    $ran
}
