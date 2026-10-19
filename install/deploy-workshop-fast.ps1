# Deploy a release into the workshop's archive, built in the *dev* profile.
# Same destination and same ceremony as deploy-workshop.ps1; the only step that
# changes is the build.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File install\deploy-workshop-fast.ps1
#
# Why this exists: `just deploy-workshop` builds `--release`, and `[profile.release]`'s
# `codegen-units = 1` + thin LTO spends ~198s of codegen on the single `quire` crate
# plus the link — a repeat deploy is measured at 9-11 minutes (docs/BUILD_PERFORMANCE.md,
# and the numbers PERFORMANCE.md / the release recipe still call "~2m30s"). When all a
# deploy needs is a current exe on the workshop shelf, that optimization cost is not what
# is being asked for. This recipe compiles the same version into the same archive in the
# dev profile instead.
#
# Dev, not a hybrid. `just check`'s comment records the trap: a one-off profile spliced
# with `--config` (dev + release's lto/codegen-units/strip) makes cargo see a new
# fingerprint and rebuild all 325 dependency crates. This script does the opposite — it
# asks for the plain `dev` profile, the very tree `just build` and `just run` already keep
# warm, with no override. So the only unit the bump invalidates is `quire` itself, and in
# dev that is a few seconds of codegen and an ~11s link. The release and shot trees are
# untouched; nothing here competes for `just build`'s incremental cache.
#
# Everything else is deploy-workshop.ps1 verbatim, because the fast deploy is still a
# *recorded release*, not a scratch drop:
#
#   1. [package] version's patch +1 in Cargo.toml — the same guard against bumping the
#      wrong table, the same read-back before anything expensive.
#   2. `cargo build --bin quire-slint` (dev). The bump must precede the build: build.rs
#      stamps the exe's FileVersion resource block from CARGO_PKG_VERSION, and that is
#      profile-independent, so the dev exe carries the number too.
#   3. The bump committed (Cargo.toml + Cargo.lock) and pushed, one commit, the way every
#      repository in this workspace is left.
#   4. target\debug\quire-slint.exe copied into C:\workshop\quire-slint-<version>\ under
#      the versioned name.
#
# One rule carried over from deploy-workshop.ps1: every character inside a Write-Output or
# a throw is ASCII. Windows PowerShell 5.1 reads a BOM-less script as ANSI (this machine's
# codepage is GBK), so a non-ASCII character in a *string literal* reaches the console as
# mojibake. Comments are free; messages are not.

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
Set-Location $root

# ── 1: the bump ─────────────────────────────────────────────────────────────
$manifest = Join-Path $root 'Cargo.toml'
$text = [System.IO.File]::ReadAllText($manifest)
$m = [regex]::Match($text, '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"')
if (-not $m.Success) { throw "no [package] version line in Cargo.toml" }
$deps = $text.IndexOf('[dependencies]')
if ($deps -ge 0 -and $m.Index -gt $deps) {
    throw "the first top-level 'version' key sits below [dependencies]; refusing to bump the wrong table"
}
$version = "{0}.{1}.{2}" -f $m.Groups[1].Value, $m.Groups[2].Value, ([int]$m.Groups[3].Value + 1)
$vStart = $m.Groups[1].Index
$vEnd = $m.Groups[3].Index + $m.Groups[3].Length
$text = $text.Substring(0, $vStart) + $version + $text.Substring($vEnd)
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($manifest, $text, $utf8)
$check = [regex]::Match([System.IO.File]::ReadAllText($manifest), '(?m)^version\s*=\s*"([^"]+)"')
if (-not $check.Success -or $check.Groups[1].Value -ne $version) {
    throw ("the bump wrote '{0}', expected '{1}'; aborting before the build" -f `
        $check.Groups[1].Value, $version)
}
Write-Output "==> version -> $version (dev profile)"

# ── 2: the build (dev, not --release) ───────────────────────────────────────
# Same default renderer as the release deploy (femtovg); the profile is the only
# difference. No --release, no --config: the plain dev tree `just build` uses.
cargo build --bin quire-slint
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $root 'target\debug\quire-slint.exe'
if (-not (Test-Path $exe)) { throw "no exe at $exe; the build reported success but left nothing" }

# Confirm the exe took the bump, exactly as the release deploy does — build.rs stamps
# FileVersion from CARGO_PKG_VERSION regardless of profile. `NotAttempted` (no rc.exe on
# this machine) leaves the block empty, so an empty value is a soft miss, not a failure.
$reported = (Get-Item $exe).VersionInfo.FileVersion
if ($reported -and $reported -notlike "$version.*") {
    throw "$exe reports version $reported, expected $version; the build did not take the bump"
}

# ── 3: the bump goes in as its own commit ───────────────────────────────────
git add Cargo.toml Cargo.lock
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
git commit -m "chore(release): $version"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
git push
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# ── 4: the deploy ───────────────────────────────────────────────────────────
$name = "quire-slint-$version"
$archive = Join-Path 'C:\workshop' $name
New-Item -ItemType Directory -Force -Path $archive | Out-Null
$deployed = Join-Path $archive "$name.exe"
Copy-Item $exe $deployed -Force
Write-Output ("==> archived v{0} (dev build, {1:N1} MiB) -> {2}" -f `
    $version, ((Get-Item $exe).Length / 1MB), $deployed)
