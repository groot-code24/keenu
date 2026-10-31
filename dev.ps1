# SPDX-License-Identifier: GPL-3.0-or-later
# Sets up to work on Keenu on Windows, and starts it. Installs nothing: it says what is missing and how to get it.
#
#   .\dev.ps1            check, build, and start the island with a separate data folder
#   .\dev.ps1 -Check     only check
param([switch]$Check)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
. (Join-Path $PSScriptRoot 'scripts\windows\common.ps1')

$tools = Find-KeenuTools
Show-KeenuTools $tools
if ($Check) { exit 0 }
if (-not $tools.CanBuild) { exit 1 }

Write-Step 'Building'
$exe = Build-Keenu $tools -Profile debug
$data = Join-Path $PSScriptRoot '.dev-data'
$env:KEENU_DATA_DIR = $data
$env:KEENU_LOG = 'debug'
Write-Step "Starting Keenu, with its data in $data and its log there as keenu.log"
Start-Process -FilePath $exe
