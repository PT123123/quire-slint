param(
    [string]$OutDir = "docs\clips",
    [string]$Shot = "target-shot\debug\quire-shot.exe",
    [int]$W = 1280,
    [int]$H = 800,
    [int]$GifW = 480,
    [int]$Frames = 20,
    [int]$FrameMs = 40,
    # Slint divides its animation clock by this, so a 80ms fade costs 8 frames
    # instead of two. Without it a GIF of a fast tween is a slide show of a jump.
    [int]$Slow = 4,
    # comma-separated subset of the motion names, e.g. -Only sidebar-toggle,theme-fade
    [string[]]$Only = @()
)
$ErrorActionPreference = "SilentlyContinue"

# One GIF per motion, captured headless by `quire-shot --motion`: the frames are
# the running app's own interpolation (Slint's animation clock, driven one
# --frame-ms step per frame), not a re-drawing of it. Everything is recorded at
# 1280x800 and written out $GifW wide — a README has to load on a phone, and the
# whole point of the downscale is that a clip of this shell costs a few hundred
# kilobytes.
#
# The scene column matters: a motion is staged *inside* a scene, and the scene
# is what puts the content there for the motion to move.
#
# `distinct` is the gate. A motion that does not interpolate renders the same
# pixels twice, and the only difference between a clip of an animation and a
# clip of a jump is whether the middle frames moved — so each motion has to
# prove it moved, in its own frames, or the run is not a pass.
$motions = @(
    @{ motion = "sidebar-toggle"; scene = "default";     file = "sidebar.gif";    min = 5 },
    @{ motion = "theme-fade";     scene = "default";     file = "theme.gif";      min = 4 },
    @{ motion = "board-drag";     scene = "tasks-board"; file = "board-drag.gif"; min = 5 },
    @{ motion = "mcp-toggle";     scene = "settings";    file = "mcp-toggle.gif"; min = 4 }
)

# a re-run of a subset must not silently skip the rest and look like success
$Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
if (-not (Test-Path $Shot)) {
    Write-Output "build it first: cargo build --features software --bin quire-shot"
    exit 1
}
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }
if ($Slow -gt 1) { $env:SLINT_SLOW_ANIMATIONS = "$Slow" } else { Remove-Item Env:SLINT_SLOW_ANIMATIONS -ErrorAction SilentlyContinue }

$fail = 0
foreach ($m in $motions) {
    if ($Only.Count -gt 0 -and ($Only -notcontains $m.motion)) { continue }
    $gif = Join-Path $OutDir $m.file
    # the frames are kept as PNGs so an interpolation can be *measured* rather
    # than admired; they are this script's own scratch, per motion
    $seq = Join-Path $env:TEMP ("quire-clips-seq-" + $m.motion)
    if (Test-Path $seq) { Remove-Item -Path $seq -Recurse -Force }
    $null = & $Shot --motion $m.motion --scene $m.scene --w $W --h $H `
        --frames $Frames --frame-ms $FrameMs --gif-w $GifW --gif $gif --seq-dir $seq 2>&1
    if (-not (Test-Path $gif)) {
        Write-Output ("{0,-15} MISSING" -f $m.motion)
        $fail = 1
        continue
    }
    $hashes = Get-ChildItem -Path $seq -Filter *.png | Sort-Object Name |
        ForEach-Object { (Get-FileHash $_.FullName -Algorithm MD5).Hash }
    $distinct = ($hashes | Select-Object -Unique).Count
    $kb = [math]::Round((Get-Item $gif).Length / 1KB, 1)
    $verdict = if ($distinct -lt $m.min) { $fail = 1; "JUMPED (no tween)" } else { "ok" }
    Write-Output ("{0,-15} {1,7} KB  frames={2}  distinct={3}  {4}" -f $m.motion, $kb, $hashes.Count, $distinct, $verdict)
}
exit $fail
