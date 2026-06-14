# Build MistTerm Windows setup.exe — **CI only** (see .github/workflows/build.yml).
# End users: download MistTerm-*-windows-x86_64-setup.exe from GitHub Releases.
param(
    [switch]$SkipBuild,
    [string]$Target = "",
    [string]$IsccPath = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$BinName = "Mist"
$Iss = Join-Path $Root "scripts\MistTerm.iss"

function Get-AppVersion {
    $line = Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1
    if (-not $line) { throw "Could not read version from Cargo.toml" }
    return $line.Matches[0].Groups[1].Value
}

function Find-Iscc {
    param([string]$Override)
    if ($Override -and (Test-Path $Override)) { return $Override }
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        (Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
    ) | Where-Object { $_ -and (Test-Path $_) }
    if ($candidates.Count -eq 0) {
        throw @"
Inno Setup not found. Install Inno Setup 6, then rerun:
  winget install --id JRSoftware.InnoSetup
  choco install innosetup -y
Or pass -IsccPath 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
"@
    }
    return @($candidates)[0]
}

function Find-BuiltExe {
    param([string]$Triple)
    $paths = @()
    if ($Triple) {
        $paths += Join-Path $Root "target\$Triple\release\$BinName.exe"
    }
    $paths += Join-Path $Root "target\release\$BinName.exe"
    foreach ($p in $paths) {
        if (Test-Path $p) { return (Resolve-Path $p).Path }
    }
    throw "Built binary not found. Run without -SkipBuild or build with: cargo build --release --bin $BinName --features vendored-openssl"
}

$version = Get-AppVersion
Write-Host "==> MistTerm Windows installer v$version"

if (-not $SkipBuild) {
    Write-Host "==> Building release binary..."
    $buildArgs = @("build", "--release", "--bin", $BinName, "--features", "vendored-openssl")
    if ($Target) { $buildArgs += @("--target", $Target) }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$exe = Find-BuiltExe -Triple $Target
$stage = Join-Path $Root "dist\installer-stage"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -Force $exe (Join-Path $stage "$BinName.exe")

$iscc = Find-Iscc -Override $IsccPath
Write-Host "==> Compiling installer with: $iscc"
$isccArgs = @(
    "/DAppVersion=$version",
    "/DSourceDir=$stage",
    $Iss
)
& "$iscc" @isccArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$setup = Join-Path $Root "dist\MistTerm-$version-windows-x86_64-setup.exe"
if (-not (Test-Path $setup)) {
    throw "Installer not produced: $setup"
}

Write-Host ""
Write-Host "Installer ready: $setup"
Write-Host "Double-click to install (Start menu shortcut, optional desktop icon, uninstaller)."
