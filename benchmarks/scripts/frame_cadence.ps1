# Frame-cadence harness: how many frames does a drawer tween actually paint?
#
# This is the evidence behind ADR-0168 and the M17 section of docs/PERFORMANCE.md.
# It runs the app's own `--frame-cadence` probe (src/main.rs), which mounts a scene,
# drives the sidebar open and closed on a 900 ms beat, and stamps every painted
# frame from Slint's rendering notifier. The output is the number that answers
# "does this feel smooth": how many frames land inside the 220 ms animation, per
# arm, plus the gap/paint split that says whether a slow frame is the renderer or
# the event loop.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\frame_cadence.ps1
#   ... -Runs 5                                  repeat the whole probe (default 3)
#   ... -Library C:\path\to\fixture.db          use a specific library, no copy
#   ... -Exe ..\..\target\release\quire-slint.exe   probe a release-profile build
#
# Three things this file exists to not get wrong:
#
#   1. Why a copy of the library. The probe reads real rows, and a tween measured
#      against an empty window measures nothing. The copy is thrown away; the live
#      file is only ever read to be copied, so no second process opens the library
#      that the running instance holds.
#   2. Why the probe runs more than once. Repeated runs on this machine spread: the
#      slowest arm of the same tween read 8, 11, 14 and 17 frames across four runs
#      on 2026-10-20, and 4 in a run while the machine was busy. Anything that
#      reports one number reports the machine's mood, so the arms are aggregated over
#      runs by median, and a run whose control arm is slow is reported as contended
#      rather than believed.
#   3. Why the control arm is what gates the verdict. Arm E is the docs area
#      mounted, which paints 1-3 ms a frame and sits at the monitor's throttle
#      ceiling (~10 ms here = 22 frames per tween). If E is slow, the tween being
#      measured is not the thing that was slow, and a FAIL would be a fixture
#      artefact.
#
# PowerShell 5.1 notes for anyone editing: ASCII only (a BOM-less script is read as
# this machine's ANSI codepage, and the Chinese arm labels come from the exe's
# stderr, which is decoded separately), and never round-trip a file through
# `Get-Content` + `Set-Content`.

[CmdletBinding()]
param(
    [string]$Library = "",
    [string]$Exe = "",
    [int]$Runs = 3
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ($Exe -eq "") { $Exe = Join-Path $repoRoot "target\debug\quire-slint.exe" }
if (-not (Test-Path $Exe)) { throw "no binary at $Exe - run 'just build' first (or pass -Exe)" }

# A tween is 220 ms (Theme.duration-normal); one 10 ms frame of slop is allowed.
$windowMs = 235.0
# Arm E's measured ceiling is 22 frames; half of it is the point at which the
# machine, not the app, is the one holding the frame.
$controlFloor = 12

# ── the fixture ──────────────────────────────────────────────────────────────
# A stale WAL replayed onto a fresh main file makes SQLite call the image
# `malformed`, and that failure lands in the fixture rather than the code
# (PERFORMANCE.md, ADR-0165's method note). So: copy the main file only.
if ($Library -eq "") {
    $live = Join-Path $env:APPDATA "quire\quire.db"
    if (-not (Test-Path $live)) { throw "no live library at $live - pass -Library" }
    $Library = Join-Path $env:TEMP "frame-cadence.db"
    Copy-Item -LiteralPath $live -Destination $Library -Force
    foreach ($side in @("-wal", "-shm")) {
        if (Test-Path ($Library + $side)) { Remove-Item -LiteralPath ($Library + $side) -Force }
    }
    Write-Output ("fixture: copied {0} -> {1}" -f $live, $Library)
} else {
    Write-Output ("fixture: {0}" -f $Library)
}

function Get-Median([double[]]$Values) {
    if ($Values.Count -eq 0) { return 0 }
    $s = @($Values | Sort-Object)
    $n = $s.Count
    if ($n % 2 -eq 1) { return $s[[int](($n - 1) / 2)] }
    return ($s[$n / 2 - 1] + $s[$n / 2]) / 2
}

# ── one run: start the probe, wait for it, parse what it painted ─────────────
function Invoke-CadenceRun([int]$Number) {
    $log = Join-Path $env:TEMP ("frame-cadence-{0}.err" -f $Number)
    # Write-Host, not Write-Output: this function's return value *is* the row set,
    # and a stray progress string in it makes Measure-Object -Property Frames throw.
    Write-Host ("run {0}: {1} --frame-cadence" -f $Number, $Exe)
    $proc = Start-Process -FilePath $Exe `
        -ArgumentList @("--frame-cadence", "--db", "`"$Library`"") `
        -RedirectStandardError $log -NoNewWindow -PassThru
    if (-not $proc.WaitForExit(120000)) {
        Stop-Process -Id $proc.Id -Force
        throw "probe did not exit; killed pid $($proc.Id)"
    }
    # Parameterless WaitForExit is what populates ExitCode on a -PassThru object.
    $proc.WaitForExit()

    $text = [System.IO.File]::ReadAllText($log)
    $lines = $text -split "`r?`n"

    $marksLine = $lines | Where-Object { $_ -like "*marks_ms=*" } | Select-Object -Last 1
    if ($null -eq $marksLine) { throw "run ${Number}: no marks_ms line in $log (window never painted?)" }
    $marks = @($marksLine.Substring($marksLine.IndexOf("marks_ms=") + 9).Split(",") |
        ForEach-Object { [double]$_ })

    $shown = ($lines | Where-Object { $_ -like "*stage=idle*" } | Select-Object -First 1)
    $display = ($lines | Where-Object { $_ -like "*slint throttle*" } | Select-Object -First 1)
    if ($display) { Write-Host ("  {0}" -f $display.Trim()) }
    elseif ($shown) { Write-Host ("  {0}" -f $shown.Trim()) }

    $out = @()
    foreach ($line in $lines) {
        if ($line -notmatch "stage=(\w+)-(close|open)\b") { continue }
        $arm = $Matches[1]
        $move = $Matches[2]
        if ($line -notmatch "cadence: t=([0-9.]+)") { continue }
        $t0 = [double]$Matches[1]
        # The stage callback runs the command, so the frame carrying the new state
        # is the first one after this timestamp.
        $inTween = @($marks | Where-Object { $_ -gt $t0 -and $_ -le ($t0 + $windowMs) })
        $gaps = @()
        for ($i = 1; $i -lt $inTween.Count; $i++) { $gaps += ($inTween[$i] - $inTween[$i - 1]) }
        $mean = 0.0
        if ($gaps.Count -gt 0) { $mean = ($gaps | Measure-Object -Average).Average }
        $out += [pscustomobject]@{
            Run    = $Number
            Arm    = $arm
            Move   = $move
            Frames = $inTween.Count
            MeanMs = [math]::Round($mean, 1)
        }
    }
    if ($out.Count -eq 0) { throw "run ${Number}: no close/open stages found in $log" }
    return $out
}

# ── the arms, in the order the probe mounts them ─────────────────────────────
$armNames = [ordered]@{
    A = "notes / all, long list"
    B = "notes / today, body open"
    C = "notes / done, empty list"
    D = "tasks"
    E = "docs area (control)"
}

$all = @()
for ($r = 1; $r -le $Runs; $r++) { $all += Invoke-CadenceRun $r }

$perRun = $all | Group-Object Run | ForEach-Object {
    $g = $_.Group
    $worstControl = ($g | Where-Object { $_.Arm -eq "E" } | Measure-Object -Property Frames -Minimum).Minimum
    [pscustomobject]@{
        Run       = $_.Name
        WorstArm  = ($g | Measure-Object -Property Frames -Minimum).Minimum
        ControlE  = $worstControl
        Contended = ($worstControl -lt $controlFloor)
    }
}
Write-Output ""
$perRun | Format-Table -AutoSize | Out-String -Width 120 | Write-Output

$rows = @()
foreach ($key in $armNames.Keys) {
    foreach ($move in @("close", "open")) {
        $frames = @($all | Where-Object { $_.Arm -eq $key -and $_.Move -eq $move } |
            ForEach-Object { [double]$_.Frames })
        $means = @($all | Where-Object { $_.Arm -eq $key -and $_.Move -eq $move } |
            ForEach-Object { $_.MeanMs } | Where-Object { $_ -gt 0 })
        $rows += [pscustomobject]@{
            Arm    = $key
            Scene  = $armNames[$key]
            Move   = $move
            Frames = [int](Get-Median $frames)
            MeanMs = [math]::Round((Get-Median $means), 1)
        }
    }
}
$rows | Format-Table -AutoSize | Out-String -Width 120 | Write-Output

# The 100 Hz ceiling: Slint 1.18 paces every animation repaint through
# i-slint-backend-winit's TimerBasedFrameThrottle, whose interval is the monitor's
# refresh rate, read here from inside the window as 10 ms. 220 ms / 10 ms = 22.
$control = @($rows | Where-Object { $_.Arm -eq "E" } | ForEach-Object { $_.Frames })
if ((Get-Median $control) -lt $controlFloor) {
    Write-Output ("INCONCLUSIVE: the control arm (docs area) painted {0} frames - the machine was " +
        "holding frames, so no number below is about the app" -f ($control -join "/"))
    exit 1
}

$worst = ($rows | Where-Object { $_.Arm -ne "E" } | Measure-Object -Property Frames -Minimum).Minimum
Write-Output ("worst arm over {0} runs: {1} frames per 220 ms tween (ceiling here is 22)" -f $Runs, $worst)
# The floor is a ratchet, not a target. Measured on 2026-10-20 with an optimized
# dependency tree, the slowest arm (notes/all, close) medians at 11-14 frames and read
# 8, 11, 11, 14, 14 and 17 across six runs; the same build read 4 when this machine was
# contended, and the unoptimized tree read 1. 6 sits under that spread on purpose -
# a floor inside the noise cries wolf, and a wolf that cries every Tuesday gets
# ignored. It still catches the failure mode this harness was built for: a tween
# that paints one frame is a teleport. Whoever makes the note row's measure cheaper
# should raise it in the same change, because a floor nobody raises is how a 6-frame
# snap starts looking normal.
if ($worst -lt 6) {
    Write-Output "FAIL: a tween painted in fewer than 6 frames is a snap, not a motion"
    exit 1
}
Write-Output "PASS"
