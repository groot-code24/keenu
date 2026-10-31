# SPDX-License-Identifier: GPL-3.0-or-later
# Installs Keenu on Windows from an official release, with no development tools needed.
#
#   irm https://YOUR-KEENU-DOMAIN/install.ps1 | iex
#
# To read it first, which is a good idea with any script from the internet:
#
#   irm https://YOUR-KEENU-DOMAIN/install.ps1 -OutFile install.ps1
#   notepad install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1
#
# Environment: KEENU_VERSION picks a version, KEENU_CHANNEL=prerelease allows test versions, KEENU_YES=1 asks
# nothing, KEENU_MANIFEST_URL points at another manifest. -Uninstall takes Keenu off again; your data stays.
param([switch]$Uninstall)

$ErrorActionPreference = 'Stop'
# Set by the release workflow when it publishes this script.
$DefaultManifest = 'https://YOUR-KEENU-DOMAIN/keenu-release.json'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Keenu'

function Say($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Stop-With($text) { Write-Host "Keenu: $text" -ForegroundColor Red; exit 1 }

function Test-SecureUrl([string]$url) {
    if ($url -match '^https://') { return $true }
    return $url -match '^http://(127\.0\.0\.1|localhost)(:|/)'
}

function Stop-RunningKeenu {
    Get-Process keenu -ErrorAction SilentlyContinue | ForEach-Object {
        # Asked first: it saves what it holds.
        if ($_.CloseMainWindow()) { [void]$_.WaitForExit(3000) }
        if (-not $_.HasExited) { $_.Kill() }
    }
}

function Get-ExistingVersion {
    $exe = Join-Path $InstallDir 'keenu.exe'
    if (-not (Test-Path $exe)) { return $null }
    $out = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
    try {
        Start-Process -FilePath $exe -ArgumentList '--version' -Wait -NoNewWindow -RedirectStandardOutput $out
        return ([string](Get-Content $out -Raw)).Trim()
    } catch { return 'unknown' } finally { Remove-Item $out -ErrorAction SilentlyContinue }
}

# Windows files products under their GUID with each part's characters swapped about.
function Get-ProductCode([string]$packed) {
    $reverse = { param($text) -join ($text.ToCharArray() | ForEach-Object { $_ })[($text.Length - 1)..0] }
    $swap = { param($text) -join (0..($text.Length / 2 - 1) | ForEach-Object { $text.Substring($_ * 2 + 1, 1) + $text.Substring($_ * 2, 1) }) }
    '{{{0}-{1}-{2}-{3}-{4}}}' -f (& $reverse $packed.Substring(0, 8)), (& $reverse $packed.Substring(8, 4)), (& $reverse $packed.Substring(12, 4)),
        (& $swap $packed.Substring(16, 4)), (& $swap $packed.Substring(20, 12))
}

function Invoke-Uninstall {
    Say 'Removing Keenu (your data stays in %LOCALAPPDATA%\Keenu)'
    Stop-RunningKeenu
    $exe = Join-Path $InstallDir 'keenu.exe'
    if (Test-Path $exe) {
        Start-Process -FilePath $exe -ArgumentList '--agent-uninstall' -Wait -NoNewWindow
        Start-Process -FilePath $exe -ArgumentList '--disable-startup' -Wait -NoNewWindow
    }
    $product = Get-ChildItem 'HKCU:\Software\Microsoft\Installer\Products' -ErrorAction SilentlyContinue |
        Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).ProductName -eq 'Keenu' } | Select-Object -First 1
    $uninstaller = Join-Path $InstallDir 'uninstall.exe'
    if ($product) {
        $msiexec = Join-Path $env:SystemRoot 'System32\msiexec.exe'
        $code = (Start-Process -FilePath $msiexec -ArgumentList '/x', (Get-ProductCode $product.PSChildName), '/qn', '/norestart' -Wait -PassThru).ExitCode
        if ($code -notin 0, 3010) { Stop-With "Windows Installer could not remove it (error $code)." }
    } elseif (Test-Path $uninstaller) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait
        # It hands over to a copy of itself and returns before that has finished.
        for ($i = 0; $i -lt 50 -and (Test-Path $exe); $i++) { Start-Sleep -Milliseconds 200 }
    } elseif (Test-Path $exe) {
        Remove-Item -Recurse -Force $InstallDir
    }
    if (Test-Path $exe) { Stop-With "$exe is still there; it may be in use. Close Keenu and try again." }
    Say 'Keenu is removed.'
}

if ($Uninstall) { Invoke-Uninstall; exit 0 }

$manifestUrl = if ($env:KEENU_MANIFEST_URL) { $env:KEENU_MANIFEST_URL } else { $DefaultManifest }
if ($manifestUrl -match 'YOUR-KEENU-DOMAIN') {
    Stop-With 'this copy of the installer is not set up with a release address yet. Set KEENU_MANIFEST_URL, or install from a release page.'
}
if (-not (Test-SecureUrl $manifestUrl)) { Stop-With "$manifestUrl is not a secure address." }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x64' }
    'ARM64' { 'arm64' }
    default { Stop-With "this Windows ($env:PROCESSOR_ARCHITECTURE) has no Keenu build." }
}
$platform = "windows-$arch"
$build = [Environment]::OSVersion.Version.Build
if ($build -lt 19041) { Stop-With "Keenu needs Windows 10 version 2004 or newer; this is build $build." }

Say "Looking up the release for $platform"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
try { $manifest = Invoke-RestMethod -Uri $manifestUrl -UseBasicParsing }
catch { Stop-With "could not fetch the release list from $manifestUrl : $($_.Exception.Message)" }
if ($manifest.product -ne 'Keenu') { Stop-With 'that address does not hold a Keenu release list.' }

$allowPre = $env:KEENU_CHANNEL -eq 'prerelease'
$candidates = @($manifest.releases | Where-Object {
    if ($env:KEENU_VERSION) { $_.version -eq $env:KEENU_VERSION } else { $allowPre -or $_.channel -eq 'stable' }
})
if (-not $candidates) { Stop-With 'no matching release is published.' }
$release = $candidates[0]
$artifacts = @($release.artifacts | Where-Object { $_.platform -eq $platform })
$artifact = $null
foreach ($kind in 'msi', 'exe', 'zip') {
    $artifact = $artifacts | Where-Object { $_.kind -eq $kind } | Select-Object -First 1
    if ($artifact) { break }
}
if (-not $artifact) { Stop-With "release $($release.version) has nothing for $platform." }
if (-not (Test-SecureUrl $artifact.url)) { Stop-With "$($artifact.url) is not a secure address." }

$existing = Get-ExistingVersion
if ($existing) { Say "Keenu $existing is installed; your settings and data will be kept" }

if (-not $artifact.signature.signed) {
    Write-Host "Note: $($artifact.file_name) is not code signed, so Windows may warn about it. Its checksum is still checked." -ForegroundColor Yellow
}
if ($env:KEENU_YES -ne '1' -and [Environment]::UserInteractive) {
    $answer = Read-Host "Install Keenu $($release.version) ($($artifact.kind)) to $InstallDir? [Y/n]"
    if ($answer -and $answer -notmatch '^[Yy]') { Say 'Nothing was changed.'; exit 0 }
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("keenu-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $work | Out-Null
try {
    $file = Join-Path $work (Split-Path $artifact.file_name -Leaf)
    Say "Downloading $($artifact.file_name)"
    try { Invoke-WebRequest -Uri $artifact.url -OutFile $file -UseBasicParsing }
    catch { Stop-With "the download failed: $($_.Exception.Message)" }

    $actual = (Get-FileHash $file -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne ([string]$artifact.sha256).ToLowerInvariant()) {
        Stop-With 'the download does not match its published checksum, so it was not installed. Try again; if it keeps happening, report it.'
    }
    Say 'Checksum verified'
    if ($artifact.signature.signed) {
        $signature = Get-AuthenticodeSignature $file
        if ($signature.Status -ne 'Valid') {
            Stop-With "the release says it is signed but its signature is $($signature.Status), so it was not installed."
        }
        Say "Signature verified: $($signature.SignerCertificate.Subject)"
    }

    Stop-RunningKeenu
    switch ($artifact.kind) {
        'msi' {
            Say 'Installing'
            $msiexec = Join-Path $env:SystemRoot 'System32\msiexec.exe'
            $code = (Start-Process -FilePath $msiexec -ArgumentList '/i', "`"$file`"", '/qn', '/norestart' -Wait -PassThru).ExitCode
            if ($code -notin 0, 3010) { Stop-With "Windows Installer failed with error $code; the earlier version, if any, is as it was." }
        }
        'exe' {
            Say 'Installing'
            $code = (Start-Process -FilePath $file -ArgumentList '/S' -Wait -PassThru).ExitCode
            if ($code -ne 0) { Stop-With "the setup program failed with error $code." }
        }
        'zip' {
            Say "Unpacking to $InstallDir"
            $unpacked = Join-Path $work 'unpacked'
            Expand-Archive -Path $file -DestinationPath $unpacked -Force
            New-Item -ItemType Directory -Force $InstallDir | Out-Null
            Copy-Item (Join-Path $unpacked '*') $InstallDir -Recurse -Force
            $path = [Environment]::GetEnvironmentVariable('Path', 'User')
            if (($path -split ';') -notcontains $InstallDir) {
                [Environment]::SetEnvironmentVariable('Path', "$path;$InstallDir", 'User')
            }
        }
    }
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

Say "Keenu $($release.version) is installed"
Write-Host "Start it from the Start Menu, or run: $InstallDir\keenu.exe"
Write-Host 'To remove it: irm <this address> -OutFile install.ps1; powershell -ExecutionPolicy Bypass -File install.ps1 -Uninstall'
