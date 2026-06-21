# 纯 GUI 端到端：新建连接 + SSH + SFTP 传文件（不跑 cargo test）
# 会自动 ensure 本地 sshd（必要时提权 setup-windows-test-sshd.ps1）
# 用法:
#   .\scripts\run-gui-e2e.ps1
#   .\scripts\run-gui-e2e.ps1 -KeepOpen

param(
    [switch]$KeepOpen,
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$devShell = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
if (Test-Path $devShell) {
    Import-Module $devShell
    Enter-VsDevShell -VsInstallPath "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools" -SkipAutomaticLocation -DevCmdArguments "-arch=amd64"
}

$profile = if ($Release) { "release" } else { "debug" }
$exe = Join-Path $Root "target\$profile\Mist.exe"
if (-not (Test-Path $exe)) {
    Write-Host "==> Building Mist ($profile)..."
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    if ($Release) { cargo build --release --bin Mist }
    else { cargo build --bin Mist }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

python -c "import pywinauto, paramiko" 2>$null
if ($LASTEXITCODE -ne 0) {
    pip install pywinauto paramiko --quiet
}

Write-Host "==> Ensure local OpenSSH test sshd"
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root "scripts\ensure-windows-test-sshd.ps1")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> Seed local test session"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$pyArgs = @($exe, "--timeout", "120")
if ($KeepOpen) { $pyArgs += "--keep-open" }

python (Join-Path $Root "scripts\gui_e2e_local_ssh.py") @pyArgs
exit $LASTEXITCODE
