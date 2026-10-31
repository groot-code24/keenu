# SPDX-License-Identifier: GPL-3.0-or-later
# Builds Keenu for Windows.
#
#   .\build.ps1            target\release\keenu.exe, or dist\keenu.exe when built through Docker
#   .\build.ps1 -Package   also the MSI, setup program and portable ZIP in dist\ (needs Docker)
param([switch]$Package)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
. (Join-Path $PSScriptRoot 'scripts\windows\common.ps1')

$tools = Find-KeenuTools
Show-KeenuTools $tools
if (-not $tools.CanBuild) { exit 1 }

Write-Step 'Building'
$exe = Build-Keenu $tools -Profile release
Write-Host "Built $exe"
if ($Package) {
    if (-not $tools.Docker) { Stop-Keenu 'Making the installers needs Docker (for msitools and NSIS), or run scripts/windows/package.sh on Linux.' }
    Write-Step 'Making the installers'
    New-Item -ItemType Directory -Force dist | Out-Null
    if ((Resolve-Path $exe).Path -ne (Join-Path $PSScriptRoot 'dist\keenu.exe')) { Copy-Item $exe dist\keenu.exe -Force }
    Invoke-InCrossImage @('bash', 'scripts/windows/package.sh', 'dist/keenu.exe')
    if ($LASTEXITCODE -ne 0) { Stop-Keenu 'Making the installers failed.' }
}
