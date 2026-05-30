# Install Mist (release) on Windows.
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$BinName = "Mist"
$InstallRoot = Join-Path $env:LOCALAPPDATA "Programs\Mist"
$Dest = Join-Path $InstallRoot "$BinName.exe"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "Rust/cargo not found. Install from https://rustup.rs then reopen PowerShell."
}

Write-Host "==> Building $BinName (release)..."
cargo build --release --bin $BinName

$Src = Join-Path $Root "target\release\$BinName.exe"
if (-not (Test-Path $Src)) {
    throw "Build failed: $Src not found"
}

New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
Copy-Item -Force $Src $Dest

Write-Host ""
Write-Host "Installed: $Dest"
Write-Host "Add to PATH (User) example:"
Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$InstallRoot', 'User')"
Write-Host ""
Write-Host "Then run: $BinName"
Write-Host "See docs/en/INSTALL.md (or docs/zh/INSTALL.md) for libssh2 / vcpkg notes on Windows."
