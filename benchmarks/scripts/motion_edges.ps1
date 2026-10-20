param(
    # A motion's frames, as kept by `quire-shot --motion --seq-dir <dir>`.
    [string]$SeqDir = (Join-Path $env:TEMP "quire-clips-seq-sidebar-toggle"),
    # Rows to scan, as a fraction of the frame height: the sidebar's tree band,
    # where both of its faces are plain background. Fractions, because the same
    # script has to read a 320-wide clip and a 640-wide fine sample.
    [double]$FromFrac = 0.35,
    [double]$ToFrac = 0.85,
    # The two faces this reads are 13/255 apart in the midnight palette, so the
    # test is "closer to the sidebar's colour than to the page's", not equality.
    [int]$MinDiff = 6
)
$ErrorActionPreference = "Stop"

# The speed curve of a captured motion, measured rather than admired.
#
# `clips.ps1` answers "did it interpolate at all" with a distinct-frame count;
# that is the floor, not the shape. A linear tween passes it as happily as an
# eased one, and a linear *layout* tween is precisely what reads as 卡 at a
# solid 60 fps (ADR-0166). This prints the animated edge's position per frame
# and the delta between frames, so the easing is a number you can compare.
#
# The edge is the sidebar's right border, found per row as the first column that
# stops wearing the sidebar's face colour, and taken as the maximum over the
# scanned band. Read it only while the sidebar is open: at width 0 the page's own
# background is the same colour as the face (13/255 apart), so the detector has
# nothing left to separate and reports the page's first text pixel instead.

Add-Type -AssemblyName System.Drawing
$files = Get-ChildItem -Path $SeqDir -Filter *.png | Sort-Object Name
if (-not $files) { Write-Output "no PNGs under $SeqDir"; exit 1 }

function Get-Edges([string]$Path) {
    $bmp = New-Object System.Drawing.Bitmap $Path
    $y0 = [int]($bmp.Height * $FromFrac); $y1 = [int]($bmp.Height * $ToFrac)
    $rect = New-Object System.Drawing.Rectangle(0, $y0, $bmp.Width, ($y1 - $y0))
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = [Math]::Abs($data.Stride)
    $bytes = New-Object byte[] ($stride * ($y1 - $y0))
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
    $w = $data.Width
    $bmp.UnlockBits($data)
    $out = @()
    for ($y = 0; $y -lt ($y1 - $y0); $y++) {
        $row = $y * $stride
        # The sidebar's own face, read at x=2 (its padding, plain on every row of
        # the band). The edge is the *first* column from the left that stops
        # wearing it: text inside the sidebar can only make a row's edge too
        # small, and the maximum over the band is then the true one.
        $i0 = $row + 2 * 4
        $r0 = $bytes[$i0 + 2]; $g0 = $bytes[$i0 + 1]; $b0 = $bytes[$i0]
        $edge = 0
        for ($x = 1; $x -lt $w - 1; $x++) {
            $i = $row + $x * 4
            $d = [Math]::Abs($bytes[$i] - $b0) + [Math]::Abs($bytes[$i + 1] - $g0) + [Math]::Abs($bytes[$i + 2] - $r0)
            if ($d -ge $MinDiff * 3) { $edge = $x; break }
        }
        $out += $edge
    }
    $bmp.Dispose()
    $out
}

$prev = $null
foreach ($f in $files) {
    $edges = @(Get-Edges $f.FullName)
    $median = ($edges | Measure-Object -Maximum).Maximum
    $delta = if ($null -eq $prev) { 0 } else { $median - $prev }
    Write-Output ("{0}  edge={1,4}  delta={2,4}" -f $f.Name, $median, $delta)
    $prev = $median
}
