#!/usr/bin/env pwsh
# Run every sample in a real Win32 window, on a chosen GPU backend, and read
# back what it says it did.
#
#   tools/run-samples-windowed.ps1 -Backend vk|dx12 [-Frames N] [-NoValidation]
#
# The Windows twin of `tools/run-samples-windowed.sh`, which stays the Linux
# one. That script's header argues the gate itself — why a sample has to run
# without `--headless` for its windowed present to be covered at all, and why
# the one summary line it prints as it exits is the assertion — and none of it
# is repeated here. What is here is what Windows changes.
#
# # One table, read out of the bash script
#
# `SAMPLES`, `SAMPLE_FRAMES`, `VIEWER_MODEL` and the `AUTOEXEC_*` constants are
# read out of `tools/run-samples-windowed.sh` rather than written again, so a
# sample added there — which `tools/check-windowed-samples.sh` forces for every
# binary under `apps/` — is run here too, at the same extent, with no second
# edit to forget. The parse fails when it finds nothing, or a line in the table
# it does not understand, rather than running a shorter list.
#
# # What it asserts, per sample
#
# Exit 0, then off the one summary line: the frames it was asked for, `on the
# win32 shell at`, the requested extent and the *effective* `windowed` mode.
# Then no `object(s) still alive at device teardown` line, which `crcbl-vk` and
# `crcbl-dx12` both print. `CRCBL_SHELL=win32` is set for the reason the bash
# script names `x11`: stating the intent keeps a silent fallback from passing.
#
# # Validation
#
# **vk**: `CRCBL_VK_VALIDATION=1`, graded by `tools/vk-validation-log.sh`
# itself, run through Git Bash — that file is the one statement of what a
# validation record looks like in a log, and `tools/vk-validation-log-test.sh`
# is what holds it to that; a PowerShell copy would be a second statement
# nothing holds to the first. After the loop, the bash script's self-test pass
# runs too: the first sample again with `CRCBL_VK_VALIDATION_SELF_TEST=1` and
# `CRCBL_VK_VALIDATION_PROVOKE=1`, which must come back red on the validation
# check. `-NoValidation` turns all of that off, loudly, for a machine without
# `VK_LAYER_KHRONOS_validation`.
#
# **dx12**: nothing is checked, and the run says so. The D3D12 debug layer's
# messages are read only by `crcbl-dx12`'s own device tests and by the
# diagnosis a device-removed failure carries; a sample that runs to the end
# never writes them to its log, so there is nothing here to grade.
# `docs/backlog.md` carries it.
#
# # The autoexec, through `USERPROFILE`
#
# The bash script ends by running `lantern` against a seeded config root and an
# empty one, the only place `Loop::new` running `autoexec.cfg` is observed. It
# moves the root with `XDG_CONFIG_HOME`, which Windows does not read.
# `NativeStorage::config_root` is `dirs::config_dir()`, which on Windows is
# `SHGetKnownFolderPath(FOLDERID_RoamingAppData)`, and measured on Windows 11
# 26200 with `dirs` 7.0.0:
#
#   * `APPDATA` in the child's environment moves nothing, alone or beside
#     `LOCALAPPDATA`.
#   * `USERPROFILE` does. The known folder is stored in the registry as
#     `%USERPROFILE%\AppData\Roaming` (`HKCU\...\Explorer\User Shell Folders`,
#     value `AppData`) and is expanded against the calling process's own
#     environment — provided the expanded directory exists; if it does not,
#     `config_dir()` answers `None` and the sample logs that it has nowhere to
#     read `autoexec.cfg` from.
#
# So each autoexec run gets a scratch profile with the registry value's
# expansion created inside it. The value is read, not assumed: a machine whose
# AppData is redirected somewhere that does not start with `%USERPROFILE%` fails
# here by name. And the control run refuses both "ran an autoexec" and "ran no
# autoexec because there was nowhere to look", so a root that did not move, or
# moved to nothing, cannot pass as an empty directory.
#
# `USERPROFILE` is also where `cargo` and `rustup` find their homes when
# `CARGO_HOME` and `RUSTUP_HOME` are unset, so it must never reach them. That is
# why every sample is built once up front and each run starts the built binary
# directly, where the bash script says `cargo run` per sample.
#
# # ENVIRONMENT
#
#   CRCBL_E2E_SAMPLE_LOG   `CRCBL_LOG` for the sample runs, `info` by default.
#
# Everything else is the caller's: which Vulkan driver the loader offers, and
# `CRCBL_DX12_VALIDATION`. `CRCBL_ADAPTER` does not reach a windowed run — only
# the offscreen setup reads it — so a windowed sample opens whatever adapter the
# backend enumerates first, and the pass line names it.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('vk', 'dx12')]
    [string]$Backend,
    # Zero takes `SAMPLE_FRAMES` from the bash script.
    [int]$Frames = 0,
    [switch]$NoValidation
)

# `Continue`, as `run-win32-e2e.ps1` argues: a native command writing progress
# to stderr must not become a terminating error through the `2>&1` merge.
$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

if (-not $IsWindows) {
    [Console]::Error.WriteLine('crcbl e2e: this is the Win32 gate; tools/run-samples-windowed.sh is the Linux one')
    exit 1
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$harness = Join-Path $repoRoot 'tools/run-samples-windowed.sh'
$validationHelper = Join-Path $repoRoot 'tools/vk-validation-log.sh'
$runtimeDir = Join-Path ([System.IO.Path]::GetTempPath()) "crcbl-samples-windowed-$PID"
$sampleLog = if ($env:CRCBL_E2E_SAMPLE_LOG) { $env:CRCBL_E2E_SAMPLE_LOG } else { 'info' }
$validating = $Backend -eq 'vk' -and -not $NoValidation

# The whole log of the run that just failed, printed under its diagnosis — the
# bash script's `cat "$log"`.
$script:failedLog = $null

# ── Reading the bash script ────────────────────────────────────────────────

$harnessText = Get-Content -Raw -Path $harness

# `NAME="value"` at the start of a line, as the bash script assigns its
# constants. Missing is a failure: a constant renamed there must not turn into
# an empty string here.
function Get-HarnessValue {
    param([Parameter(Mandatory)][string]$Name)
    $m = [regex]::Match($harnessText, "(?m)^$Name=`"?([^`"\r\n]*)`"?\s*$")
    if (-not $m.Success) {
        throw "crcbl e2e: tools/run-samples-windowed.sh assigns no $Name at the start of a line, so this script cannot read it. Has its shape changed?"
    }
    $m.Groups[1].Value
}

# Replace each `${NAME}` in one word with its value here, and refuse a name
# this script does not know rather than handing a sample a literal `${...}`.
function Expand-HarnessWord {
    param([Parameter(Mandatory)][string]$Word, [Parameter(Mandatory)][hashtable]$Values)
    $pattern = '\$\{([A-Z_]+)\}'
    foreach ($m in [regex]::Matches($Word, $pattern)) {
        $name = $m.Groups[1].Value
        if (-not $Values.ContainsKey($name)) {
            throw "crcbl e2e: tools/run-samples-windowed.sh's SAMPLES uses `${$name}, which this script does not define. Add it to the values it expands."
        }
    }
    $expanded = [regex]::Replace($Word, $pattern, { param($m) $Values[$m.Groups[1].Value] })
    if ($expanded -match '\$') {
        throw "crcbl e2e: '$Word' in SAMPLES still holds a `$ after expansion; this script only understands `${NAME}."
    }
    $expanded
}

# The `SAMPLES=( ... )` block, one quoted entry per line, the same lines
# `tools/check-windowed-samples.sh` reads. Each entry becomes a name, an extent
# and whatever it hands the sample, split into words *before* expansion so a
# path with a space in it stays one argument.
function Get-HarnessSamples {
    param([Parameter(Mandatory)][hashtable]$Values)
    $block = [regex]::Match($harnessText, '(?ms)^SAMPLES=\(\s*$(.*?)^\)')
    if (-not $block.Success) {
        throw 'crcbl e2e: found no SAMPLES=( ... ) block in tools/run-samples-windowed.sh. Has its shape changed?'
    }
    $samples = @()
    foreach ($line in ($block.Groups[1].Value -split "`r?`n")) {
        $trimmed = $line.Trim()
        if ($trimmed -eq '' -or $trimmed.StartsWith('#')) {
            continue
        }
        $entry = [regex]::Match($trimmed, '^"([^"]+)"$')
        if (-not $entry.Success) {
            throw "crcbl e2e: SAMPLES holds a line this script does not understand: $trimmed"
        }
        $words = @($entry.Groups[1].Value -split '\s+' | ForEach-Object { Expand-HarnessWord -Word $_ -Values $Values })
        if ($words.Count -lt 2 -or $words[1] -notmatch '^\d+x\d+$') {
            throw "crcbl e2e: SAMPLES entry '$trimmed' is not '<name> <W>x<H> [args...]'"
        }
        $samples += [pscustomobject]@{
            Name   = $words[0]
            Extent = $words[1]
            Args   = @($words | Select-Object -Skip 2)
        }
    }
    if ($samples.Count -eq 0) {
        throw 'crcbl e2e: parsed no samples out of SAMPLES, so this gate would pass on nothing. Has its shape changed?'
    }
    $samples
}

# ── Running one sample ─────────────────────────────────────────────────────

# Run `$Script` with `$Environment` set in this process, and put every variable
# back afterwards — the child inherits it, and nothing after the call does. A
# `$null` value removes the variable.
function Invoke-WithEnvironment {
    param([Parameter(Mandatory)][hashtable]$Environment, [Parameter(Mandatory)][scriptblock]$Script)
    $saved = @{}
    foreach ($name in $Environment.Keys) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name)
        [Environment]::SetEnvironmentVariable($name, $Environment[$name])
    }
    try {
        & $Script
    } finally {
        foreach ($name in $saved.Keys) {
            [Environment]::SetEnvironmentVariable($name, $saved[$name])
        }
    }
}

# Git for Windows' own bash, found through `git` rather than `PATH`: from a
# native shell `bash` is as likely to be the WSL launcher in `System32` or
# `WindowsApps`, which cannot read these paths and is not what anyone meant.
function Resolve-GitBash {
    if (-not (Get-Command git -CommandType Application -ErrorAction SilentlyContinue)) {
        throw 'crcbl e2e: no git on PATH, so there is no Git Bash to run tools/vk-validation-log.sh with. Install Git for Windows, or pass -NoValidation to run without the validation check.'
    }
    # `<root>/mingw64/libexec/git-core` -> `<root>`.
    $execPath = (& git --exec-path).Trim()
    $root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $execPath))
    $bash = Join-Path $root 'bin/bash.exe'
    if (-not (Test-Path -LiteralPath $bash)) {
        throw "crcbl e2e: git's exec path is $execPath and there is no $bash beside it, so tools/vk-validation-log.sh cannot be run. Pass -NoValidation to run without the validation check."
    }
    $bash
}

# Call one function out of `tools/vk-validation-log.sh` on a log. Returns the
# diagnosis it printed, or `$null` when it returned 0.
function Invoke-ValidationHelper {
    param([Parameter(Mandatory)][string]$Function, [Parameter(Mandatory)][string]$Log, [Parameter(Mandatory)][string]$What)
    # Forward slashes, which MSYS reads as the same path and which cannot end
    # an argument in a backslash that escapes its closing quote.
    $out = & $script:gitBash -c 'source "$1" && "$2" "$3" "$4"' crcbl-e2e `
        ($validationHelper -replace '\\', '/') $Function ($Log -replace '\\', '/') $What 2>&1
    if ($LASTEXITCODE -eq 0) {
        return $null
    }
    ($out | ForEach-Object { "$_" }) -join [Environment]::NewLine
}

# `Invoke-Sample` — the bash script's `run_sample`. Throws the diagnosis on any
# failure, having recorded the log for the caller to print; returns the pass
# line otherwise. `-Quiet` keeps the sample's output off the console, for the
# self-test run that is expected to fail.
function Invoke-Sample {
    param(
        [Parameter(Mandatory)][pscustomobject]$Sample,
        [hashtable]$Environment = @{},
        [switch]$Quiet
    )
    $name = $Sample.Name
    $extent = $Sample.Extent
    $log = Join-Path $runtimeDir "$name.log"
    $script:failedLog = $log

    $childEnv = @{
        CRCBL_SHELL = 'win32'
        CRCBL_LOG   = $sampleLog
    }
    if ($validating) {
        $childEnv['CRCBL_VK_VALIDATION'] = '1'
    }
    foreach ($key in $Environment.Keys) {
        $childEnv[$key] = $Environment[$key]
    }

    if (-not $Quiet) {
        Write-Host "crcbl e2e: running $name windowed on the win32 shell on the $Backend GPU backend"
    }
    $exe = Join-Path $script:binDir "$name.exe"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    # Streamed to the console as it runs, like the bash script's `tee`.
    $sink = if ($Quiet) { 'Out-Null' } else { 'Out-Host' }
    Invoke-WithEnvironment -Environment $childEnv -Script {
        & $exe @($Sample.Args) --backend $Backend --frames $script:frames --size $extent 2>&1 |
            ForEach-Object { "$_" } | Tee-Object -FilePath $log | & $sink
    }
    $status = $LASTEXITCODE
    $clock.Stop()
    $seconds = [math]::Round($clock.Elapsed.TotalSeconds, 1)
    if ($status -ne 0) {
        throw "crcbl e2e: $name failed on win32/$Backend (exit $status)"
    }

    # Everything below off the one summary line, for the bash script's reason:
    # four greps over a whole log can each be answered by a different line.
    $summary = Select-String -LiteralPath $log -Pattern "^$([regex]::Escape($name)): " |
        Select-Object -First 1 | ForEach-Object { $_.Line }
    if (-not $summary) {
        throw "crcbl e2e: $name exited 0 and printed no summary line"
    }
    if (-not $summary.StartsWith("${name}: $($script:frames) frames, ")) {
        throw "crcbl e2e: $name did not present $($script:frames) frames: $summary"
    }
    if (-not $summary.Contains(' on the win32 shell at ')) {
        throw "crcbl e2e: $name did not run on the win32 shell: $summary"
    }
    # The trailing space stops "windowed" matching a longer mode name.
    if (-not $summary.Contains(" at $extent, windowed ")) {
        throw "crcbl e2e: $name did not come up at $extent windowed: $summary"
    }

    $leaks = @(Select-String -LiteralPath $log -SimpleMatch 'object(s) still alive at device teardown' |
            ForEach-Object { "               $($_.Line)" })
    if ($leaks.Count -gt 0) {
        throw (@("crcbl e2e: $name destroyed its device with objects still alive:") + $leaks + @(
                "           The sample's own teardown reporter wrote that. Destroy them",
                '           where they were made rather than leaving the line in a log.'
            ) -join [Environment]::NewLine)
    }

    $checked = 'validation not checked'
    if ($validating) {
        $complaint = Invoke-ValidationHelper -Function crcbl_validation_saw_nothing -Log $log -What $name
        if ($null -ne $complaint) {
            throw $complaint
        }
        $checked = 'validation silent'
    }

    $adapter = Select-String -LiteralPath $log -Pattern 'hal: \S+ adapter "([^"]+)"' |
        Select-Object -First 1 | ForEach-Object { $_.Matches[0].Groups[1].Value }
    if (-not $adapter) {
        $adapter = 'an adapter it did not name'
    }
    $script:failedLog = $null
    "crcbl e2e: $name presented $($script:frames) frames at $extent windowed on win32/$Backend ($adapter) in ${seconds}s, nothing left alive, $checked"
}

# Print a failure the way the bash script does — the diagnosis, then the log it
# came from — and stop.
function Stop-Gate {
    param([Parameter(Mandatory)][string]$Message)
    [Console]::Error.WriteLine($Message)
    if ($script:failedLog -and (Test-Path -LiteralPath $script:failedLog)) {
        Get-Content -LiteralPath $script:failedLog | ForEach-Object { [Console]::Error.WriteLine($_) }
    }
    exit 1
}

# ── The validation self-test ───────────────────────────────────────────────

# The bash script's `self_test_validation`, which argues why it exists: it is
# what proves the validation check the other runs are graded by can go red.
# `Invoke-Sample` itself is run, not a copy of its checks.
function Test-ValidationSelfTest {
    param([Parameter(Mandatory)][pscustomobject]$Sample)
    $name = $Sample.Name
    $log = Join-Path $runtimeDir "$name.log"
    $id = 'CRCBL-VALIDATION-SELF-TEST'
    $line = "crcbl_vk::debug\] vk validation: $id"

    Write-Host "crcbl e2e: re-running $name with CRCBL_VK_VALIDATION_SELF_TEST=1, which must fail"
    $refusal = $null
    try {
        Invoke-Sample -Sample $Sample -Quiet -Environment @{
            CRCBL_VK_VALIDATION_SELF_TEST = '1'
            CRCBL_VK_VALIDATION_PROVOKE   = '1'
        } | Out-Null
    } catch {
        $refusal = $_.Exception.Message
    }
    $script:failedLog = $log

    if (-not (Select-String -LiteralPath $log -Pattern $line -Quiet)) {
        Stop-Gate ("crcbl e2e: $name ran with CRCBL_VK_VALIDATION_SELF_TEST=1 and no $id line reached its log, " +
            'so the validation check the other runs are graded by has never been seen to fail. Either the ' +
            'debug messenger is not calling back, or the callback no longer reaches crcbl_core::log::error!, ' +
            "or CRCBL_LOG dropped it.`n$refusal")
    }
    if ($null -eq $refusal) {
        Stop-Gate "crcbl e2e: $name logged the injected $id message and still passed, so the validation check is not reading what the messenger writes."
    }
    # ...and it failed on the validation check rather than somewhere else.
    if (-not $refusal.Contains("the validation layer complained about $name") -or $refusal -notmatch $line) {
        Stop-Gate "crcbl e2e: $name failed with the self-test injected, but not on the validation check, so that check still has not been shown to fail. What it did fail on:`n$refusal"
    }
    $unchecked = Invoke-ValidationHelper -Function crcbl_validation_layer_checked -Log $log -What $name
    if ($null -ne $unchecked) {
        Stop-Gate $unchecked
    }
    $script:failedLog = $null
    Write-Host "crcbl e2e: $name went red on the injected $id and the layer answered a"
    Write-Host '           provoked violation, so the validation check is live and checking'
}

# ── The autoexec ───────────────────────────────────────────────────────────

# Where `SHGetKnownFolderPath(FOLDERID_RoamingAppData)` lands under a profile
# directory, read from the registry value it expands. See the header.
function Get-RoamingUnderProfile {
    $key = Get-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders'
    $raw = $key.GetValue('AppData', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $prefix = '%USERPROFILE%\'
    if (-not $raw -or -not $raw.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw ("crcbl e2e: this machine's AppData known folder is '$raw', which does not start with " +
            "%USERPROFILE%, so moving USERPROFILE cannot move dirs::config_dir and the autoexec check has " +
            'no way to point NativeStorage at a scratch directory here.')
    }
    $raw.Substring($prefix.Length)
}

# One windowed run of the autoexec sample with `USERPROFILE` pointed at
# `<root>\<label>`, graded by `Invoke-Sample` like every other run, its log
# kept under `<label>`.
function Invoke-AutoexecRun {
    param([Parameter(Mandatory)][string]$Label, [Parameter(Mandatory)][pscustomobject]$Sample, [Parameter(Mandatory)][string]$Root)
    $userProfile = Join-Path $Root $Label
    Write-Host "crcbl e2e: running $($Sample.Name) against USERPROFILE=$userProfile"
    Write-Host (Invoke-Sample -Sample $Sample -Environment @{ USERPROFILE = $userProfile })
    Copy-Item -LiteralPath (Join-Path $runtimeDir "$($Sample.Name).log") -Destination (Join-Path $runtimeDir "autoexec-$Label.log")
}

# Fail unless (`-Wants`) or if (`-Refuses`) the run labelled `$Label` printed a
# line matching `$Pattern`.
function Assert-AutoexecLog {
    param(
        [Parameter(Mandatory)][string]$Label,
        [Parameter(Mandatory)][string]$Pattern,
        [Parameter(Mandatory)][string]$Meaning,
        [switch]$Refuses
    )
    $log = Join-Path $runtimeDir "autoexec-$Label.log"
    $hit = Select-String -LiteralPath $log -Pattern $Pattern -Quiet
    if ($Refuses -and $hit) {
        $script:failedLog = $log
        Stop-Gate "crcbl e2e: the $Label $($script:autoexecSample) run printed a line matching /$Pattern/, so $Meaning"
    }
    if (-not $Refuses -and -not $hit) {
        $script:failedLog = $log
        Stop-Gate "crcbl e2e: the $Label $($script:autoexecSample) run printed no line matching /$Pattern/, so $Meaning"
    }
}

# The bash script's `check_autoexec`, whose comments argue every assertion
# below — the control run included, and why the pass rows are matched by their
# log target. Only the way the config root moves differs.
function Test-Autoexec {
    param([Parameter(Mandatory)][object[]]$Samples)
    $sampleName = $script:autoexecSample
    $file = Get-HarnessValue AUTOEXEC_FILE
    $var = Get-HarnessValue AUTOEXEC_VAR
    $value = Get-HarnessValue AUTOEXEC_VALUE
    $basePass = Get-HarnessValue AUTOEXEC_BASE_PASS
    $pass = Get-HarnessValue AUTOEXEC_PASS

    $sample = $Samples | Where-Object { $_.Name -eq $sampleName } | Select-Object -First 1
    if (-not $sample) {
        Stop-Gate "crcbl e2e: $sampleName is not in SAMPLES, so this check would be driving a sample the gate no longer covers."
    }

    # Two profiles rather than one reused, for the bash script's reason: a
    # sample saves its settings under the config root as it exits.
    $roaming = Get-RoamingUnderProfile
    $root = Join-Path $runtimeDir 'autoexec'
    $seededDir = Join-Path (Join-Path (Join-Path $root 'seeded') $roaming) $sampleName
    New-Item -ItemType Directory -Force -Path $seededDir | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path (Join-Path $root 'control') $roaming) | Out-Null
    # No byte-order mark: the console would read one as part of the first
    # variable's name.
    [System.IO.File]::WriteAllText((Join-Path $seededDir $file), "$var $value`n", [System.Text.UTF8Encoding]::new($false))
    Write-Host "crcbl e2e: seeded $sampleName\$file under the seeded profile's $roaming with '$var $value'; the control's is empty"

    Invoke-AutoexecRun -Label seeded -Sample $sample -Root $root
    Invoke-AutoexecRun -Label control -Sample $sample -Root $root

    $f = [regex]::Escape($file)
    Assert-AutoexecLog seeded "console\] ${f}: 1 line$" `
        "it ran no $file at boot. Either Loop::new no longer calls run_autoexec, or the file it reads is not the one seeded here, or a line in it failed and the count says so."
    Assert-AutoexecLog seeded "console\] $var = $value$" `
        "the file ran and $var did not take the value it set."
    Assert-AutoexecLog seeded "crcbl::engine\]\s+$basePass\s" `
        "it reported no $basePass pass at all, so the absence of $pass below would be a missing report rather than a missing pass."
    Assert-AutoexecLog seeded "crcbl::engine\]\s+$pass\s" -Refuses `
        'the value never reached the renderer before the frames were timed.'

    Assert-AutoexecLog control "crcbl::engine\]\s+$pass\s" `
        "$pass is absent with nothing having asked for its absence, so the seeded run proves nothing about what read the file."
    Assert-AutoexecLog control "console\] ${f}:" -Refuses `
        "it ran an $file out of a profile this gate left empty. USERPROFILE did not move the config root, and these two runs have been reading the machine's own."
    # Windows only: a root that expanded to a directory that does not exist is
    # `None`, which would otherwise pass as an empty one.
    Assert-AutoexecLog control "no $f was run:" -Refuses `
        "the control run found no config root to look in, so it is not the empty directory it is supposed to be. Did the scratch profile's $roaming stop being where dirs::config_dir looks?"
    Assert-AutoexecLog control "console\] $var = " -Refuses `
        "something other than the autoexec sets $var."

    Write-Host "crcbl e2e: $sampleName ran $file at boot and left $pass out;"
    Write-Host '           the same run against an empty config directory did neither'
}

# ── The gate ───────────────────────────────────────────────────────────────

New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
Push-Location $repoRoot
try {
    try {
        $script:frames = if ($Frames -gt 0) { $Frames } else { [int](Get-HarnessValue SAMPLE_FRAMES) }
        $script:autoexecSample = Get-HarnessValue AUTOEXEC_SAMPLE
        $viewerModel = (Get-HarnessValue VIEWER_MODEL).Replace('${RUNTIME_DIR}', $runtimeDir) -replace '/', '\'
        $samples = @(Get-HarnessSamples -Values @{ VIEWER_MODEL = $viewerModel; RUNTIME_DIR = $runtimeDir })
        if ($validating) {
            $script:gitBash = Resolve-GitBash
        }
    } catch {
        Stop-Gate $_.Exception.Message
    }

    if ($Backend -eq 'dx12') {
        Write-Warning ('crcbl e2e: dx12 runs check NO validation. The D3D12 debug layer''s messages never reach a ' +
            'sample''s log, so nothing here can read them. docs/backlog.md, "No sample-level pass in CI", has it.')
    } elseif ($NoValidation) {
        Write-Warning ('crcbl e2e: -NoValidation: CRCBL_VK_VALIDATION is not set, and neither the validation check nor ' +
            'its self-test runs. A green run here says nothing about the Vulkan validation layer.')
    }

    # Built once, before any environment is moved; see the header.
    $names = @($samples | ForEach-Object { $_.Name })
    Write-Host "crcbl e2e: building $($names.Count) samples"
    cargo build --locked --quiet @($names | ForEach-Object { '--package'; $_ }) 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Stop-Gate "crcbl e2e: the samples failed to build (exit $LASTEXITCODE)"
    }
    $targetDir = (cargo metadata --locked --format-version 1 --no-deps | ConvertFrom-Json).target_directory
    $script:binDir = Join-Path $targetDir 'debug'

    Write-Host "crcbl e2e: writing $viewerModel from crcbl-scene's gltf-fixture triangle"
    cargo run --locked --quiet --package crcbl-scene --features gltf-fixture `
        --example write-triangle-glb -- $viewerModel 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Stop-Gate "crcbl e2e: writing the viewer's model failed (exit $LASTEXITCODE)"
    }

    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    foreach ($sample in $samples) {
        try {
            Write-Host (Invoke-Sample -Sample $sample)
        } catch {
            Stop-Gate $_.Exception.Message
        }
    }
    $clock.Stop()
    Write-Host ("crcbl e2e: $($samples.Count) samples ran windowed on win32/$Backend in " +
        "$([math]::Round($clock.Elapsed.TotalSeconds, 1))s")

    if ($validating) {
        Test-ValidationSelfTest -Sample $samples[0]
    } else {
        Write-Warning 'crcbl e2e: the validation self-test did not run, because validation is off for this run.'
    }

    try {
        Test-Autoexec -Samples $samples
    } catch {
        Stop-Gate $_.Exception.Message
    }
} finally {
    Pop-Location
    Remove-Item -Recurse -Force -LiteralPath $runtimeDir -ErrorAction SilentlyContinue
}
