# SPDX-License-Identifier: GPL-3.0-or-later
# Runs Keenu's tests on Windows, and says plainly what ran, what passed and what was skipped.
#
#   .\test.ps1             formatting, then unit and integration tests, on this computer
#   .\test.ps1 -Desktop    also drives the real program on this desktop (scripts\windows\smoke-test.ps1)
param([switch]$Desktop)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
. (Join-Path $PSScriptRoot 'scripts\windows\common.ps1')

$tools = Find-KeenuTools
Show-KeenuTools $tools
if (-not $tools.CanBuild) { exit 1 }

$results = @()
function Add-Result($name, $status, $detail = '') { $script:results += [pscustomobject]@{ Check = $name; Status = $status; Detail = $detail } }

if ($tools.Cargo) {
    Write-Step 'Formatting'
    & cargo fmt --all -- --check 2>&1 | Out-Host
    Add-Result 'cargo fmt' $(if ($LASTEXITCODE -eq 0) { 'PASS' } else { 'FAIL' })
} else {
    Add-Result 'cargo fmt' 'SKIP' 'no Rust toolchain on this computer'
}

Write-Step 'Unit and integration tests'
$packages = 'keenu-core', 'keenu-security', 'keenu-integrations', 'keenu-platform', 'keenu-ui', 'keenu-app', 'keenu-release', 'keenu-windows'
$failed = Test-KeenuPackages $tools $packages
Add-Result 'unit and integration tests' $(if ($failed) { 'FAIL' } else { 'PASS' })

if ($Desktop) {
    Write-Step 'Driving the real program on this desktop'
    $exe = Build-Keenu $tools -Profile debug
    & (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -NoProfile -File scripts\windows\smoke-test.ps1 -Exe $exe | Out-Host
    Add-Result 'desktop smoke test' $(if ($LASTEXITCODE -eq 0) { 'PASS' } else { 'FAIL' })
} else {
    Add-Result 'desktop smoke test' 'SKIP' 'run with -Desktop; it uses your screen and clipboard'
}
Add-Result 'Linux and macOS tests' 'SKIP' 'these run on their own systems, or in CI'

Write-Host ''
$results | Format-Table -AutoSize | Out-Host
if ($results.Status -contains 'FAIL') { exit 1 }
