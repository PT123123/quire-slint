param(
    [Parameter(Mandatory = $true)][int]$TargetPid,
    [int]$Seconds = 30
)
# Per-thread CPU attribution for a running instance. Get-Process' Threads give
# TotalProcessorTime per OS thread; sampling twice and diffing says which thread
# is awake while the app looks idle. Rust thread names are not exposed here, so
# a thread is identified by its delta pattern: steady = a repeated timer, bursty
# = a round, and any StartTime inside the span is a thread that was minted by
# whatever this script was doing.
$ErrorActionPreference = "Stop"
$seen = @{}
$names = @{}
$p0 = Get-Process -Id $TargetPid -ErrorAction Stop
foreach ($th in $p0.Threads) {
    $seen[[int64]$th.Id] = [pscustomobject]@{ first = $th.TotalProcessorTime; last = $th.TotalProcessorTime; start = $th.StartTime }
}
$t0 = Get-Date
$end = $t0.AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
    Start-Sleep -Seconds 3
    $p = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
    if (-not $p) { Write-Output "process exited"; break }
    foreach ($th in $p.Threads) {
        $k = [int64]$th.Id
        if (-not $seen.ContainsKey($k)) {
            $seen[$k] = [pscustomobject]@{ first = $th.TotalProcessorTime; last = $th.TotalProcessorTime; start = $th.StartTime }
        }
        $seen[$k].last = $th.TotalProcessorTime
    }
}
$span = ((Get-Date) - $t0).TotalSeconds
$rows = foreach ($k in $seen.Keys) {
    $v = $seen[$k]
    $ms = ($v.last - $v.first).TotalMilliseconds
    [pscustomobject]@{
        tid = $k
        cpuMs = [math]::Round($ms, 0)
        pctOfCore = [math]::Round($ms / 1000 / $span * 100, 2)
        started = $v.start.ToString('yyyy-MM-dd HH:mm:ss')
        fresh = ($v.start -ge $t0)
    }
}
$sum = ($rows | Measure-Object -Sum cpuMs).Sum
Write-Output ("span = {0:N0}s  pid = {1}  threads seen = {2}  total = {3:N0} ms cpu = {4:N2} % of one core" -f `
    $span, $TargetPid, $seen.Count, $sum, ($sum / 1000 / $span * 100))
$rows | Where-Object { $_.cpuMs -gt 0 } | Sort-Object cpuMs -Descending | Format-Table -AutoSize
Write-Output ("threads created inside the span = " + @($rows | Where-Object { $_.fresh }).Count)
