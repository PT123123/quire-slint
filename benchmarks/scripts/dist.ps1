# Package the Slint release exe into dist/quire-windows-x64.zip (M8-lite).
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item .\target\release\quire-slint.exe .\dist\quire-slint.exe -Force
Compress-Archive -Path .\dist\quire-slint.exe -DestinationPath .\dist\quire-windows-x64.zip -Force
Remove-Item .\dist\quire-slint.exe
Write-Output "dist/quire-windows-x64.zip ready"
