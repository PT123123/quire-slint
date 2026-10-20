# Memory and leak gates. One command, four arms, each with the control that
# proves the arm could still fail.
#
#   just leakgate
#   powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\scripts\leak_gate.ps1
#   ... -SkipDrift          the two cheap arms only (no app launch, ~5 s)
#   ... -Seconds 150         how long the drift arm watches the app
#   ... -Exe path -Library path
#
# Why each arm exists (docs/PERFORMANCE.md ADR-0169):
#
#   1. unit      - the two bounds the shell now keeps by type: the notice queue
#                  collapses repeats and keeps a tail, and a page delete retires
#                  the per-block caches. Reading "running 2 tests" is the point:
#                  a filter that matches nothing exits 0 (`cargo test --bin` is
#                  the same trap, recorded in the project's own notes).
#   2. ratchet   - a Timer minted inside an event handler is a handle nobody can
#                  ever free, because keeping a Slint timer alive means leaking it.
#                  The count is frozen at today's 8: adding a *shared* handle in
#                  `wire` is allowed and says so by moving the baseline, minting
#                  one per keystroke is not. Control: the same counter over a
#                  fixture that carries nine occurrences has to report FAIL, which
#                  is what says the comparison is live rather than always-true.
#   3. drift     - the 6.7 GB shape: committed memory whose *floor* rises while a
#                  timer fires. Samples an app on a copy of this machine's real
#                  library and requires the Private-bytes floor not to climb more
#                  than 5 MB.
#   4. control   - a balloon process that allocates 64 MB *inside the sampling
#                  window* and holds it, read by the same Measure-Floor, must show
#                  >= 40 MB of drift. A sampler that reads it flat is broken, and a
#                  flat read from a broken sampler is how a leak gate passes
#                  vacuously - the first run of this arm read -0.3 MB precisely
#                  because the balloon had finished growing before sampling began.
#
# PowerShell 5.1: ASCII only in this file, and never round-trip a file through
# Get-Content + Set-Content.

[CmdletBinding()]
param(
    [int]$Seconds = 150,
    [string]$Exe = "",
    [string]$Library = "",
    [switch]$SkipDrift
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ($Exe -eq "") { $Exe = Join-Path $repoRoot "target\debug\quire-slint.exe" }

$rows = @()
function Record([string]$gate, [bool]$ok, [string]$note) {
    $script:rows += [pscustomobject]@{ gate = $gate; ok = $ok; note = $note }
    Write-Output ("{0,-7} {1,-14} {2}" -f ($(if ($ok) { "PASS" } else { "FAIL" })), $gate, $note)
}

# ── arm 1: the bounds, as tests ──────────────────────────────────────────────
# cargo writes its progress to stderr, and under EAP=Stop a native command's
# stderr throws before the arm can read anything (PERFORMANCE.md's PowerShell
# note). Continue for this call, and judge the run by what it printed.
$ErrorActionPreference = "Continue"
$unit = (& cargo test --lib -- notice_queue retires_its_database 2>&1) | Out-String
$ErrorActionPreference = "Stop"
$ran = [regex]::Match($unit, "running (\d+) tests").Groups[1].Value
$failed = [regex]::Match($unit, "test result: ok\. (\d+) passed; (\d+) failed").Groups[2].Value
$ok1 = ($ran -eq "2") -and ($failed -eq "0")
Record "unit" $ok1 "running=$ran failed=$failed (want 2 tests, 0 failed)"

# ── arm 2: the per-event Timer leak, as a ratchet ────────────────────────────
$LEAK_PATTERN = 'Box::leak\(Box::new\(slint::Timer::default\(\)\)\)'
$BASELINE = 8
function Count-Leaks([string]$Path) {
    (Select-String -Path $Path -Pattern $LEAK_PATTERN -AllMatches |
        ForEach-Object { $_.Matches.Count } | Measure-Object -Sum).Sum
}
$controller = Join-Path $repoRoot "src\app\controller.rs"
$found = Count-Leaks $controller
if ($null -eq $found) { $found = 0 }
# The control: the very same counter, over a fixture that carries one more than
# the baseline, must read nine.
$fixture = Join-Path $env:TEMP ("leakgate-fixture-" + [guid]::NewGuid().ToString("N") + ".rs")
(1..9 | ForEach-Object { "    let t = Box::leak(Box::new(slint::Timer::default()));" }) -join "`r`n" |
    Set-Content -Path $fixture -Encoding ASCII
$controlFound = Count-Leaks $fixture
Remove-Item $fixture -Force
$ok2 = ($found -le $BASELINE) -and ($controlFound -eq 9)
Record "ratchet" $ok2 "controller.rs=$found baseline=$BASELINE control=$controlFound(want 9)"

# ── arm 3: the floor drift, with a balloon control ───────────────────────────
function Measure-Floor([scriptblock]$Sample, [int]$Span, [int]$Every) {
    # returns the drift of the *minimum* of the first third against the minimum
    # of the last third. The mean hides churn; the floor is retention.
    $values = @()
    $end = (Get-Date).AddSeconds($Span)
    while ((Get-Date) -lt $end) {
        $v = & $Sample
        if ($null -ne $v) { $values += $v }
        Start-Sleep -Seconds $Every
    }
    if ($values.Count -lt 6) { return $null }
    $third = [math]::Max(1, [int]($values.Count / 3))
    $head = ($values[0..($third - 1)] | Measure-Object -Minimum).Minimum
    $tail = ($values[-$third..-1] | Measure-Object -Minimum).Minimum
    return [pscustomobject]@{
        n = $values.Count
        head = [math]::Round($head, 1)
        tail = [math]::Round($tail, 1)
        drift = [math]::Round($tail - $head, 1)
    }
}

if ($SkipDrift) {
    Record "drift" $true "skipped (-SkipDrift)"
} else {
    if (-not (Test-Path $Exe)) { throw "no binary at $Exe - run 'just build' first" }
    if ($Library -eq "") {
        $live = Join-Path $env:APPDATA "quire\quire.db"
        if (-not (Test-Path $live)) { throw "no live library at $live - pass -Library" }
        $source = $live
        $Library = Join-Path $env:TEMP "leakgate-arm.db"
        Copy-Item -LiteralPath $live -Destination $Library -Force
        foreach ($side in @("-wal", "-shm")) {
            if (Test-Path ($Library + $side)) { Remove-Item -LiteralPath ($Library + $side) -Force }
        }
    } else {
        $source = $Library
    }
    Write-Output ("drift arm: {0} on a copy of {1} for {2}s" -f (Split-Path -Leaf $Exe), $source, $Seconds)
    # Paired flags, and the app relies on the pairing: `--auto-exit` with an
    # explicit `--db` is what makes a launch a probe rather than a session, so
    # it leaves `\\.\pipe\quire-desktop-quit` alone instead of colliding with the
    # developer's running instance. That collision was not cosmetic - the probe
    # showed the 8 s notice, exited, and the arm read one sample (issue 14).
    $app = Start-Process -FilePath $Exe -ArgumentList @("--db", "`"$Library`"", "--auto-exit", "$Seconds") -PassThru
    Start-Sleep -Seconds 8
    $pid0 = $app.Id
    # stop short of the app's own --auto-exit, so every sample is a live process
    $driftApp = Measure-Floor {
        $q = Get-Process -Id $pid0 -ErrorAction SilentlyContinue
        if (-not $q) { $null } else { $q.PrivateMemorySize64 / 1MB }
    } ($Seconds - 15) 5
    Stop-Process -Id $pid0 -Force -ErrorAction SilentlyContinue
    if ($null -eq $driftApp) {
        Record "drift" $false "the app gave fewer than six samples (pid $pid0 exited?)"
    } else {
        Record "drift" ($driftApp.drift -le 5.0) ("Private floor {0} -> {1} MB, drift {2} MB over {3}s n={4} (want <= 5)" -f `
            $driftApp.head, $driftApp.tail, $driftApp.drift, $Seconds, $driftApp.n)
    }

    # The control, sampled by the same Measure-Floor. The balloon allocates
    # *inside* the window (after 10 s of holding nothing), because the first run
    # of this gate read -0.3 MB and that was not a flat app: the child filled
    # its 64 MB during the 8 s the arm spent waiting for its own window to come
    # up, so every sample of the control was already the high one and no sampler
    # could have shown a rise. 64 MB held in a byte array is not a guess at the
    # threshold: it is above 5 MB and above the 40 MB the control must show.
    $balloon = Start-Process -FilePath "powershell" -ArgumentList @(
        "-NoProfile", "-Command",
        'Start-Sleep -Seconds 10; $b = New-Object byte[] 67108864; for($i=0;$i -lt 67108864;$i+=4096){$b[$i]=1}; Start-Sleep -Seconds 70; $b = $null'
    ) -PassThru
    $pidB = $balloon.Id
    $driftCtl = Measure-Floor {
        $q = Get-Process -Id $pidB -ErrorAction SilentlyContinue
        if (-not $q) { $null } else { $q.PrivateMemorySize64 / 1MB }
    } 40 5
    Stop-Process -Id $pidB -Force -ErrorAction SilentlyContinue
    if ($null -eq $driftCtl) {
        Record "control" $false "the balloon gave fewer than six samples - sampler is not reading"
    } else {
        Record "control" ($driftCtl.drift -ge 40.0) ("balloon floor {0} -> {1} MB, drift {2} MB (want >= 40, else the sampler is blind) n={3}" -f `
            $driftCtl.head, $driftCtl.tail, $driftCtl.drift, $driftCtl.n)
    }
}

$bad = @($rows | Where-Object { -not $_.ok })
Write-Output ""
Write-Output ("leakgate: {0} arms, {1} failed" -f $rows.Count, $bad.Count)
if ($bad.Count -gt 0) { exit 1 }
exit 0
