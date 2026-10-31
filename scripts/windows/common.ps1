# SPDX-License-Identifier: GPL-3.0-or-later
# What dev.ps1, build.ps1 and test.ps1 share: finding the tools and building with what is there.

# The oldest Rust this workspace is built with; see rust-version in Cargo.toml.
$script:MinimumRust = [version]'1.88.0'
$script:CrossImage = 'keenu-windows-cross'

function Write-Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }
function Stop-Keenu($text) { Write-Host "Keenu: $text" -ForegroundColor Red; exit 1 }

function Find-KeenuTools {
    $cargoHome = Join-Path $env:USERPROFILE '.cargo\bin'
    if ((-not (Get-Command cargo -ErrorAction SilentlyContinue)) -and (Test-Path (Join-Path $cargoHome 'cargo.exe'))) {
        $env:PATH = "$cargoHome;$env:PATH"
    }
    $docker = $null -ne (Get-Command docker -ErrorAction SilentlyContinue)
    if (-not $docker -and (Test-Path 'C:\Program Files\Docker\Docker\resources\bin\docker.exe')) {
        $env:PATH = "C:\Program Files\Docker\Docker\resources\bin;$env:PATH"
        $docker = $true
    }
    if ($docker) {
        & docker version --format '{{.Server.Version}}' *> $null
        $docker = $LASTEXITCODE -eq 0
    }
    $cargo = $null -ne (Get-Command cargo -ErrorAction SilentlyContinue)
    $rust = $null
    if ($cargo) {
        $text = (& rustc --version 2>$null) -join ' '
        if ($text -match '(\d+\.\d+\.\d+)') { $rust = [version]$Matches[1] }
    }
    # Rust's own Windows target links with Visual Studio's linker.
    $vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    $msvc = $false
    if (Test-Path $vswhere) {
        $found = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        $msvc = -not [string]::IsNullOrWhiteSpace($found)
    }
    if (Get-Command link.exe -ErrorAction SilentlyContinue) { $msvc = $true }
    $native = $cargo -and $rust -and ($rust -ge $script:MinimumRust) -and $msvc
    [pscustomobject]@{
        Cargo = $cargo
        Rust = $rust
        Msvc = $msvc
        Docker = $docker
        Native = $native
        CanBuild = $native -or $docker
    }
}

function Show-KeenuTools($tools) {
    $mark = { param($ok) if ($ok) { 'found' } else { 'MISSING' } }
    Write-Host "Windows $([Environment]::OSVersion.Version) on $env:PROCESSOR_ARCHITECTURE"
    Write-Host ("Rust {0} or newer: {1}" -f $script:MinimumRust, $(if ($tools.Rust) { "$($tools.Rust)" } else { 'MISSING' }))
    Write-Host ("Visual Studio C++ tools: {0}" -f (& $mark $tools.Msvc))
    Write-Host ("Docker (an alternative to both, and needed for the installers): {0}" -f (& $mark $tools.Docker))
    if ($tools.CanBuild) { return }
    Write-Host ''
    Write-Host 'To build Keenu you need either of these. Nothing has been installed for you.' -ForegroundColor Yellow
    Write-Host '  1. Rust from https://rustup.rs and Visual Studio Build Tools with the "Desktop development with C++" workload'
    Write-Host '  2. Docker Desktop, which builds it in a Linux container without either'
}

function Invoke-InCrossImage([string[]]$command) {
    & docker image inspect $script:CrossImage *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Step "Preparing the build image $script:CrossImage (first time only)"
        & docker build -q -t $script:CrossImage -f packaging\docker\windows-cross.Dockerfile packaging\docker | Out-Null
        if ($LASTEXITCODE -ne 0) { Stop-Keenu 'Could not prepare the Docker build image.' }
    }
    $arguments = @('run', '--rm', '-v', "${PWD}:/src", '-v', 'keenu-cargo:/usr/local/cargo/registry', '-v', 'keenu-cross-target:/target', '-w', '/src', $script:CrossImage) + $command
    & docker @arguments
}

# Builds keenu.exe and returns its path.
function Build-Keenu($tools, [ValidateSet('debug', 'release')][string]$Profile) {
    $flag = if ($Profile -eq 'release') { @('--release') } else { @() }
    if ($tools.Native) {
        & cargo build @flag --locked -p keenu-windows | Out-Host
        if ($LASTEXITCODE -ne 0) { Stop-Keenu 'The build failed.' }
        return (Resolve-Path "target\$Profile\keenu.exe").Path
    }
    New-Item -ItemType Directory -Force dist | Out-Null
    $copy = "cp /target/x86_64-pc-windows-gnu/$Profile/keenu.exe dist/keenu.exe"
    $command = "cargo build $flag --locked --target x86_64-pc-windows-gnu -p keenu-windows && $copy"
    Invoke-InCrossImage @('bash', '-c', $command) | Out-Host
    if ($LASTEXITCODE -ne 0) { Stop-Keenu 'The build failed.' }
    return (Resolve-Path 'dist\keenu.exe').Path
}

# Runs packages' tests; true if any failed. Through Docker the test programs are built there and run here.
function Test-KeenuPackages($tools, [string[]]$packages) {
    $selection = $packages | ForEach-Object { '-p', $_ }
    if ($tools.Native) {
        & cargo test --locked @selection | Out-Host
        return $LASTEXITCODE -ne 0
    }
    $out = Join-Path $PWD 'dist\tests'
    New-Item -ItemType Directory -Force $out | Out-Null
    Get-ChildItem $out -Filter *.exe -ErrorAction SilentlyContinue | Remove-Item -Force
    $list = $selection -join ' '
    $command = "cargo test --no-run --locked --target x86_64-pc-windows-gnu $list --message-format=json 2>/dev/null | python3 -c `"import json,shutil,sys`nfor l in sys.stdin:`n  m=json.loads(l)`n  if m.get('reason')=='compiler-artifact' and m.get('profile',{}).get('test') and m.get('executable'): shutil.copy(m['executable'],'dist/tests/'+m['target']['name'].replace('-','_')+'-'+m['target']['kind'][0]+'.exe')`""
    Invoke-InCrossImage @('bash', '-c', $command) | Out-Host
    $failed = $LASTEXITCODE -ne 0
    foreach ($test in Get-ChildItem $out -Filter *.exe) {
        Write-Host "--- $($test.Name)"
        & $test.FullName | Select-String -Pattern '^test result|FAILED' | Out-Host
        if ($LASTEXITCODE -ne 0) { $failed = $true }
    }
    return $failed
}
