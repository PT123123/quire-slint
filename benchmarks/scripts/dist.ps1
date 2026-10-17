# Package the Slint release exe into dist/quire-windows-x64.zip (M8-lite).
$ErrorActionPreference = "Stop"
if (-not (Test-Path .\target\release\quire-slint.exe -PathType Leaf)) {
    throw "release executable is missing: target/release/quire-slint.exe"
}
if ((Get-Item .\target\release\quire-slint.exe).Length -le 0) {
    throw "release executable is empty: target/release/quire-slint.exe"
}
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item .\target\release\quire-slint.exe .\dist\quire-slint.exe -Force
Compress-Archive -Path .\dist\quire-slint.exe -DestinationPath .\dist\quire-windows-x64.zip -Force
Remove-Item .\dist\quire-slint.exe
if (-not (Test-Path .\dist\quire-windows-x64.zip -PathType Leaf) -or
    (Get-Item .\dist\quire-windows-x64.zip).Length -le 0) {
    throw "distribution archive is missing or empty: dist/quire-windows-x64.zip"
}
Write-Output "dist/quire-windows-x64.zip ready"
