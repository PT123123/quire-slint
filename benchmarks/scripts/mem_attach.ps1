param(
    # sample an already-running instance instead of launching one
    [Parameter(Mandatory = $true)][int]$TargetPid,
    [int]$Minutes = 25,
    [int]$Every = 5,
    [string]$OutFile = ""
)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Gdi {
    [DllImport("user32.dll")]
    public static extern uint GetGuiResources(IntPtr hProcess, uint flags);
}
'@
if ($OutFile -eq "") {
    $OutFile = ".scratch/ctl/attach-" + $TargetPid + "-" + (Get-Date -Format 'yyyyMMdd-HHmmss') + ".log"
}
$rows = @()
$start = (Get-Process -Id $TargetPid -ErrorAction Stop)
$rows += ("label=attach pid={0} exe={1} started={2}" -f `
    $TargetPid, $start.Path, $start.StartTime.ToString('yyyy-MM-dd HH:mm:ss'))
$end = (Get-Date).AddMinutes($Minutes)
while ((Get-Date) -lt $end) {
    $q = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
    if (-not $q) { $rows += ("process exited at {0}" -f (Get-Date -Format 'HH:mm:ss')); break }
    $rows += ("{0} WS={1:N0}MB PM={2:N0}MB handles={3} threads={4} gdi={5} user={6} cpuMs={7:N0}" -f `
        (Get-Date -Format 'HH:mm:ss'), ($q.WorkingSet64 / 1MB), ($q.PrivateMemorySize64 / 1MB), `
        $q.HandleCount, $q.Threads.Count, [Gdi]::GetGuiResources($q.Handle, 0), `
        [Gdi]::GetGuiResources($q.Handle, 1), $q.TotalProcessorTime.TotalMilliseconds)
    $rows | Set-Content -Path $OutFile -Encoding UTF8
    Start-Sleep -Seconds $Every
}
Write-Output "wrote $OutFile ($($rows.Count) lines)"
