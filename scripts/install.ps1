# Installs the built CordFast to %LOCALAPPDATA%\Programs\CordFast as the
# setup program does: the installer marker (updates come through the next
# setup program, not as a portable copy) and a Start menu shortcut.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)
$target = Join-Path $env:LOCALAPPDATA "Programs\CordFast"
New-Item -ItemType Directory -Force $target | Out-Null
Copy-Item target\release\cordfast.exe (Join-Path $target "cordfast.exe") -Force
# fastframe-update's marker: an installed copy, like the setup program's.
# A portable marker left by an older script goes.
Set-Content -Encoding ascii -NoNewline -Path (Join-Path $target "cordfast-installer.txt") -Value "cordfast-installer-v1"
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $target "cordfast-portable.txt")

$menu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\CordFast.lnk"
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($menu)
$link.TargetPath = Join-Path $target "cordfast.exe"
$link.WorkingDirectory = $target
$link.Save()
"Instalado em $target"
