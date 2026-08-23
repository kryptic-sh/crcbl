#!/usr/bin/env pwsh
# Run `crcbl-vk`'s end-to-end suite against a real Vulkan implementation, on
# Windows.
#
#   crates/crcbl-vk/tests/run-vk-e2e.ps1 [extra nextest args…]
#
# The Windows twin of `run-vk-e2e.sh`, which stays the Linux one. Same suite,
# same pin, same guards; what differs is the shell it is launched from, and that
# difference is the reason this file exists.
#
# # Why this is not `run-vk-e2e.sh` under Git Bash
#
# It was, for three CI runs, and the loader never saw its environment. The two
# real causes found on the way are fixed and still in `vulkan-icd.sh` — the
# manifest reached the loader in Git Bash's `C:/…` form, and exporting the
# loader's variables from Git Bash did not reach a native child, so the job sets
# them at job level instead. Neither was enough: the loader still reported
# `windows_read_data_files_in_registry: Registry lookup failed`.
#
# **A native process launching a native process is the only shape with no
# environment translation in it at all.** `pwsh` sets `$env:X` in its own process
# block and `CreateProcess` hands that block to `cargo`, which hands it to the
# test binary, which is where `vulkan-1.dll` reads it. There is no MSYS layer
# anywhere in that chain, so if the loader still does not see `VK_DRIVER_FILES`
# after this, the environment was never the cause and the next round should look
# at the loader and the manifest instead. That is what makes this worth a run
# either way: it is a measurement, not just a port.
#
# `run-win32-e2e.ps1` made the same choice for a different reason — `windows-
# latest` has `pwsh` and has no shell where `mkfifo` and `trap EXIT` mean
# anything — and this file follows its shape deliberately.
#
# # What it reimplements, and what it does not
#
# `vulkan-icd.sh`'s `crcbl_pin_vk_icd` does three things. Two of them are about
# the *loader* and are here: a `CRCBL_VK_ICD` that is set but does not resolve is
# a hard failure rather than a silent fallback, and an already-set
# `VK_DRIVER_FILES` wins, because a caller who set the loader's own variables
# meant it.
#
# The third — searching for a sibling manifest whose name differs only after the
# stem — is **not** here, and the omission is deliberate. That exists because
# Debian ships `lvp_icd.x86_64.json` and Arch ships `lvp_icd.json`; it is Linux
# packaging knowledge, and there is no second packager of lavapipe for Windows to
# disagree with the first. **The job resolves the manifest instead**, by globbing
# the extracted archive for `lvp_icd*.json` — which is where the knowledge
# belongs, since that step is the one that knows which asset was downloaded, and
# it fails with the whole file listing when the glob matches nothing.
#
# # Golden images
#
# There is no `--bless` flag here, unlike the Linux script. `crcbl-golden`'s
# tolerance was measured between radv and *one* lavapipe, and this runs a
# different Mesa build of lavapipe from the one the references were blessed on —
# so a bless here would silently move a reference to a driver it is not meant to
# represent. `CRCBL_BLESS=1` still works if that is genuinely what you want; it
# is read by the tests, not by this script.
#
# Exits non-zero if there is no Vulkan loader, if a pinned ICD does not resolve,
# if nextest fails, if no tests ran, if the count cannot be read, if the run was
# cancelled part-way, if the suite never opened a device, if the adapter it
# opened is not the one `CRCBL_VK_EXPECT_ADAPTER` asked for, if the suite stopped
# reporting its sync-validation reach, or if either sandbox run fails.

$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

# tests/ -> crcbl-vk/ -> crates/ -> the repository root.
$repoRoot = Split-Path -Parent (
    Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSCommandPath)))

# Validation is the point of this suite: `ValidationReport::assert_clean` fails
# when the layer was never loaded, so a run without it fails loudly rather than
# passing vacuously. Setting it explicitly means a release-profile run works too.
if (-not $env:CRCBL_VK_VALIDATION) { $env:CRCBL_VK_VALIDATION = '1' }
if (-not $env:CRCBL_VK_SYNC_VALIDATION) { $env:CRCBL_VK_SYNC_VALIDATION = '1' }

# The pin. `Resolve-Path`'s `ProviderPath` is the native `C:\…` form whatever
# spelling came in, which is the structural version of the `cygpath -w` the bash
# path needs — the difference being that nothing here ever produced the other
# form in the first place.
if ($env:CRCBL_VK_ICD) {
    if (-not (Test-Path -LiteralPath $env:CRCBL_VK_ICD -PathType Leaf)) {
        Write-Error ("crcbl vk e2e: CRCBL_VK_ICD=$($env:CRCBL_VK_ICD) does not exist. A pin that " +
            'quietly fell back to whatever driver is installed would defeat the pinning.')
        exit 1
    }
    $icd = (Resolve-Path -LiteralPath $env:CRCBL_VK_ICD).ProviderPath
    $env:CRCBL_VK_ICD = $icd
    # Both spellings: `VK_DRIVER_FILES` is the current one and `VK_ICD_FILENAMES`
    # is what older loaders read. An existing value wins — see the header.
    if (-not $env:VK_DRIVER_FILES) { $env:VK_DRIVER_FILES = $icd }
    if (-not $env:VK_ICD_FILENAMES) { $env:VK_ICD_FILENAMES = $icd }
    Write-Host "crcbl vk e2e: pinned ICD $icd"
} else {
    Write-Host ('crcbl vk e2e: CRCBL_VK_ICD is not set, so the loader will choose the driver. ' +
        'CI pins lavapipe and this run does not, so a green run here is NOT the run CI makes.')
}

# Fail early and legibly rather than letting every test panic with the same
# message. Windows resolves a bare DLL name against the process directory, the
# system directory and then `PATH`, and the system directory is on `PATH` — so
# walking it covers the search, and naming the file that was found is what tells
# a loader this job installed from one that was already there. There is no Vulkan
# loader in a stock `windows-latest` image, which is why an absence has to be a
# loud failure here.
$loader = $null
foreach ($dir in ($env:PATH -split [System.IO.Path]::PathSeparator)) {
    if (-not $dir) { continue }
    # String concatenation rather than `Join-Path`, which throws on a `PATH`
    # entry containing a character the path parser rejects — and one malformed
    # entry must not decide that there is no loader.
    $candidate = $dir.TrimEnd('\', '/') + '\vulkan-1.dll'
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        $loader = $candidate
        break
    }
}
if (-not $loader) {
    Write-Error ('crcbl vk e2e: no vulkan-1.dll anywhere on PATH; install a Vulkan loader. ' +
        'The LunarG SDK carries one, and so does the smaller Vulkan Runtime. ' +
        'A stock GitHub windows runner has neither.')
    exit 1
}
Write-Host "crcbl vk e2e: loader $loader"

# What this process actually holds, one process closer to the loader than the
# workflow step's own dump — which is the measurement three runs were missing.
# The step showed the variables set and the loader behaved as though they were
# not; these are the values `cargo` and then the test binary inherit.
foreach ($name in 'VK_DRIVER_FILES', 'VK_ICD_FILENAMES', 'VK_LAYER_PATH', 'VK_LOADER_DEBUG') {
    $value = [System.Environment]::GetEnvironmentVariable($name)
    if ($value) {
        Write-Host "crcbl vk e2e: $name=$value"
    } else {
        Write-Host "crcbl vk e2e: $name is unset"
    }
}

# The probe that let the loader name its own problem, and the reason it earned
# its place: `ERROR_INCOMPATIBLE_DRIVER` and `Registry lookup failed` are each
# ambiguous across several distinct causes — never read the manifest, read it and
# declined it, wrong path form, variable never arrived — and only the loader's
# own preceding lines separate them. Its line count is reported rather than left
# to be eyeballed: with `VK_LOADER_DEBUG=all` set, a handful of lines is itself
# the finding.
$vulkaninfo = Get-Command 'vulkaninfo' -CommandType Application -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($vulkaninfo) {
    # Captured whole and then trimmed, rather than `Select-Object -First` on a
    # live pipeline, which stops the native command mid-write.
    $info = @(& $vulkaninfo.Source --summary 2>&1 | Out-String -Stream)
    Write-Host 'crcbl vk e2e: --- vulkaninfo --summary ---'
    $info | Select-Object -First 80 | ForEach-Object { Write-Host $_ }
    Write-Host "crcbl vk e2e: --- end vulkaninfo ($($info.Count) line(s)) ---"
    # Named, rather than left in eighty lines of dump: a run gated by a
    # validation layer is only evidence about the layer that gated it, and
    # `vulkaninfo --summary`'s layer table is the one place that says which.
    $layer = $info | Where-Object { $_ -match '^VK_LAYER_KHRONOS_validation' } | Select-Object -First 1
    if ($layer) {
        Write-Host "crcbl vk e2e: validation layer $layer"
    } else {
        Write-Host 'crcbl vk e2e: vulkaninfo does not list VK_LAYER_KHRONOS_validation'
    }
} else {
    Write-Host 'crcbl vk e2e: no vulkaninfo on PATH, so the loader gets no chance to explain itself'
}

$runtimeDir = Join-Path ([System.IO.Path]::GetTempPath()) "crcbl-vk-e2e-$PID"
New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
$log = Join-Path $runtimeDir 'nextest.log'
$sandboxLog = Join-Path $runtimeDir 'sandbox.log'

try {
    $status = 1
    Push-Location $repoRoot
    try {
        # `2>&1 | Tee-Object` rather than a plain redirect, so the run is visible
        # while it happens *and* readable afterwards. `$ErrorActionPreference` is
        # `Continue` above precisely so that a native command writing to stderr —
        # which cargo does constantly, and which is where the suite prints its
        # adapter and reach lines — is not turned into a terminating error by the
        # merge.
        #
        # `--no-fail-fast`, which the Linux twin does not pass and does not need:
        # a developer with lavapipe reruns that one in two minutes, and nobody on
        # this team can run this one at all. Each round trip here is up to an
        # hour of CI, so the run has to report everything it knows in one. The
        # zero-count and cancelled-run gates below are what keep "no failures"
        # from meaning "nothing ran".
        #
        # `--success-output immediate` rather than `--no-capture`, which is what
        # this passed for most of its life. `--no-capture` hands the test binary
        # the real stdio, and nextest cannot then interleave two tests' output —
        # so it silently forces one thread and prints `warning: ignoring
        # --test-threads because --no-capture is specified` on every run. The
        # `--test-threads 1` that used to sit beside it was therefore dead, and
        # this leg ran serially whatever either flag said, which is where most
        # of its wall clock went. Capturing instead lets nextest use the
        # runner's cores, and `immediate` keeps every line the matches below
        # need — the adapter line, the sync-validation reach — in the log, ahead
        # of the summary, which is the ordering the count match relies on.
        # `run-vk-e2e.sh` is the twin and carries the same two flags.
        cargo nextest run `
            --locked `
            --package crcbl-vk `
            --features vk-e2e `
            --test vk_e2e `
            --run-ignored all `
            --success-output immediate `
            --no-fail-fast `
            @args 2>&1 | Tee-Object -FilePath $log
        $status = $LASTEXITCODE
    } finally {
        Pop-Location
    }

    if ($status -ne 0) {
        Write-Error "crcbl vk e2e: the suite failed (exit $status)"
        # `docs/plan/12-testing.md`: "diffs uploaded as CI artifacts on failure".
        # Naming the directory here is what makes the CI step's `if: failure()`
        # upload obvious rather than folklore.
        $diffDir = Join-Path $repoRoot 'target/golden-diff'
        if (Test-Path -LiteralPath $diffDir) {
            Write-Host 'crcbl vk e2e: golden-image diffs are in target/golden-diff:'
            Get-ChildItem -LiteralPath $diffDir | ForEach-Object { Write-Host "  $($_.Name)" }
        }
        exit $status
    }

    # Matched against a colour-stripped copy, exactly as the bash harnesses do it
    # and for the same reason: CI sets `CARGO_TERM_COLOR: always`, so nextest
    # emits the count as `\e[1m<n>\e[0m tests run` and a plain-text match sees no
    # digits next to "tests run".
    #
    # nextest prints `<n> tests run:` for a complete run and `<ran>/<total> tests
    # run:` for one it cancelled, and a pattern that reads only the digits
    # immediately before the words takes the *total* out of the second shape — so
    # a run that stopped after two of fifteen reports a healthy-looking fifteen.
    # The optional `<ran>/` group is what tells the two apart.
    $escape = [char]27
    $plain = (Get-Content -Raw -Path $log) -replace "$escape\[[0-9;]*[a-zA-Z]", ''
    $hits = [regex]::Matches($plain, '(?:(\d+)/)?(\d+) tests? run')
    if ($hits.Count -eq 0) {
        Write-Error 'crcbl vk e2e: nextest printed no test count at all — the gate is not gating'
        exit 1
    }
    $summary = $hits[$hits.Count - 1]
    if ($summary.Groups[1].Success) {
        $ran = [int]$summary.Groups[1].Value
        $total = [int]$summary.Groups[2].Value
        Write-Error ("crcbl vk e2e: the run was cancelled after $ran of $total tests — " +
            'the remaining ones never executed, so a green count here would be a lie')
        exit 1
    }
    $ran = [int]$summary.Groups[2].Value
    if ($ran -eq 0) {
        Write-Error 'crcbl vk e2e: the suite reported no tests run — the gate is not gating'
        exit 1
    }

    # Which driver actually ran, from the suite rather than from the manifest
    # that was asked for. A pinned ICD the loader quietly ignored, or a sibling
    # manifest that turned out to be a different driver, both show up here and
    # nowhere else. First match, as the bash harness takes it, so the two agree
    # about which line they are reading.
    $adapterMatch = [regex]::Match($plain, 'vk e2e: adapter .*')
    if (-not $adapterMatch.Success) {
        Write-Error 'crcbl vk e2e: the suite never named an adapter — it did not open a device'
        exit 1
    }
    $driver = $adapterMatch.Value -replace '^vk e2e: ', ''
    Write-Host "crcbl vk e2e: $driver"

    # And whether that is the implementation the caller came here for.
    # `CRCBL_VK_ICD` offers the loader a manifest; only this line says the loader
    # took it. The two come apart quietly — a manifest for an incompatible driver
    # is refused with the same `ERROR_INCOMPATIBLE_DRIVER` a missing one gets,
    # and a `VK_DRIVER_FILES` that never reached the test process leaves the
    # loader free to pick anything installed. Either way the tests still run,
    # still pass, and are evidence about a driver nobody chose. On this platform
    # that is not a hypothetical: it is the failure three runs of this job hit.
    if ($env:CRCBL_VK_EXPECT_ADAPTER) {
        if ($driver -like "*$($env:CRCBL_VK_EXPECT_ADAPTER)*") {
            Write-Host "crcbl vk e2e: the adapter contains '$($env:CRCBL_VK_EXPECT_ADAPTER)', as expected"
        } else {
            Write-Host 'crcbl vk e2e: ############################################################'
            Write-Host 'crcbl vk e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS IT IS.   #'
            Write-Host 'crcbl vk e2e: ############################################################'
            Write-Error ("crcbl vk e2e: expected an adapter containing " +
                "'$($env:CRCBL_VK_EXPECT_ADAPTER)', and the suite opened its device on: $driver. " +
                "CRCBL_VK_ICD=$($env:CRCBL_VK_ICD) was what the loader was offered, so either it " +
                'declined that manifest or the pin never reached the test process.')
            exit 1
        }
    }

    Write-Host "crcbl vk e2e: $ran tests ran against a real Vulkan implementation"

    # **The teardown leak report, read rather than left in the log.** `crcbl-vk`
    # warns when a device is destroyed with objects still alive, naming their
    # kinds and formats — and a warning fails nothing, so the leaks it found the
    # afternoon it learned to name them were found by a person reading a job
    # log. The expectation is zero lines: every test in this suite destroys what
    # it creates, so a line here names a test that stopped doing so. Same check
    # as `run-vk-e2e.sh`'s, and the two have to move together.
    $leaks = [regex]::Matches($plain, '.*object\(s\) still alive at device teardown.*')
    if ($leaks.Count -gt 0) {
        Write-Host 'crcbl vk e2e: a device was destroyed with objects still alive:'
        foreach ($leak in $leaks) { Write-Host "                $($leak.Value)" }
        Write-Error ('crcbl vk e2e: the suite''s own teardown reporter wrote those lines. ' +
            'Destroy the objects in the test that made them rather than leaving the line ' +
            'in the log for somebody to notice.')
        exit 1
    }
    Write-Host 'crcbl vk e2e: every device was destroyed with nothing left alive'

    # How far this machine's validation layer can see. A hazard inside one
    # command buffer is caught while it is recorded; a hazard spanning two
    # *submissions* can only be caught at submit, and layer builds differ in
    # whether they model that. Every cross-frame hazard is of the second kind, so
    # a harness that reports a green count without saying which kind it could see
    # is worse than no harness — and this leg's layer comes from LunarG rather
    # than from Debian, so it is a different build from the one the Linux job
    # measured.
    $reachMatch = [regex]::Match($plain, 'sync-validation reach: .*')
    if (-not $reachMatch.Success) {
        # Not a soft warning: the suite is supposed to publish this, and a
        # harness that silently stopped measuring its own blind spot is the
        # failure this whole section exists to prevent.
        Write-Error ('crcbl vk e2e: the suite did not report its sync-validation reach. ' +
            'crates/crcbl-vk/tests/vk_e2e/validation_gate.rs must print it, and this script ' +
            'must be able to find it, or a green run here claims evidence it does not have.')
        exit 1
    }
    $reach = $reachMatch.Value
    Write-Host "crcbl vk e2e: $reach"
    if ($reach -like '*cross-submission=no*') {
        Write-Host 'crcbl vk e2e: ############################################################'
        Write-Host 'crcbl vk e2e: # THIS RUN IS WEAKER THAN THE LINUX LEG IT STANDS BESIDE.  #'
        Write-Host 'crcbl vk e2e: ############################################################'
        Write-Host ("crcbl vk e2e: This layer build reports hazards inside a submission and not " +
            'hazards *between* submissions. Every missing cross-frame barrier is of the second ' +
            "kind, so the green result above says nothing about them. Rely on " +
            "'cargo nextest run -p crcbl-render' for that class: it compiles consecutive frames " +
            'against one pool and needs no layer at all.')
    } else {
        Write-Host 'crcbl vk e2e: the layer sees across submissions, so cross-frame hazards were in scope'
    }

    # The sandbox's own frame, headless, against the same implementation — the
    # thing `docs/plan/02-vulkan-backend.md`'s milestone 1 is measured by, a clear
    # reaching the screen through the whole shell->HAL->swapchain join, and the
    # only part of that path a windowless runner can exercise. The Linux harness
    # runs it too; dropping it here would make this leg quietly the weaker twin.
    Write-Host 'crcbl vk e2e: running the sandbox headless against Vulkan'
    Push-Location $repoRoot
    try {
        cargo run --locked --quiet --package sandbox -- `
            --headless --backend vk --frames 30 2>&1 | Tee-Object -FilePath $sandboxLog
        $sandboxStatus = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($sandboxStatus -ne 0) {
        Write-Error "crcbl vk e2e: the sandbox failed against Vulkan (exit $sandboxStatus)"
        exit $sandboxStatus
    }
    if (-not (Select-String -Path $sandboxLog -Pattern '30 frames' -SimpleMatch -Quiet)) {
        Write-Error 'crcbl vk e2e: the sandbox did not report 30 frames'
        exit 1
    }
    Write-Host 'crcbl vk e2e: the sandbox presented 30 frames through the Vulkan backend'

    # And the null backend still works, on the same binary, selected at runtime.
    # `docs/plan/10-wasm-webgpu.md`'s "does it repro on the other backend?" triage
    # only exists if both are reachable without a rebuild.
    Write-Host 'crcbl vk e2e: running the sandbox headless against the null backend'
    Push-Location $repoRoot
    try {
        cargo run --locked --quiet --package sandbox -- `
            --headless --backend null --frames 10 2>&1 | Tee-Object -FilePath $sandboxLog
        $nullStatus = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    if ($nullStatus -ne 0 -or
        -not (Select-String -Path $sandboxLog -Pattern '10 frames' -SimpleMatch -Quiet)) {
        Write-Error 'crcbl vk e2e: the null backend is no longer reachable at runtime'
        exit 1
    }
    Write-Host 'crcbl vk e2e: both backends are selectable from one binary'
} finally {
    Remove-Item -Recurse -Force -Path $runtimeDir -ErrorAction SilentlyContinue
}
