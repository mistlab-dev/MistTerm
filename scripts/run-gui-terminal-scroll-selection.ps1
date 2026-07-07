# 终端选区/滚动/剪贴板 UI 回归（pywinauto + 本地 SSH）
# Usage: .\scripts\run-gui-terminal-scroll-selection.ps1

param(
    [switch]$Release,
    [int]$TimeoutSec = 30
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

# 本地 UI 测试固定走 127.0.0.1 + mistterm_test，避免用户级 MISTTERM_TEST_SSH_* 指向远端时 preflight 失败
$env:MISTTERM_TEST_SSH_HOST = "127.0.0.1"
$env:MISTTERM_TEST_SSH_USER = "mistterm_test"
$env:MISTTERM_TEST_SSH_PASSWORD = "mistterm123"
$env:MISTTERM_TEST_SSH_PORT = "22"

$profile = if ($Release) { "release" } else { "debug" }
$exe = Join-Path $Root "target\$profile\Mist.exe"
if (-not (Test-Path $exe)) {
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    if ($Release) { cargo build --release --bin Mist }
    else { cargo build --bin Mist }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

Write-Host "==> Ensure local OpenSSH test sshd"
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root "scripts\ensure-windows-test-sshd.ps1")
if ($LASTEXITCODE -ne 0) { throw "ensure-windows-test-sshd failed" }

Write-Host "==> Seed local test session"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { throw "seed_local_test_session failed" }

python -c "import pywinauto, paramiko" 2>$null
if ($LASTEXITCODE -ne 0) {
    pip install pywinauto paramiko pillow --quiet
    if ($LASTEXITCODE -ne 0) { throw "pip install failed" }
}

Write-Host "==> Stop lingering Mist.exe from prior GUI tests"
Get-Process -Name Mist -ErrorAction SilentlyContinue | ForEach-Object {
    try { [void]$_.CloseMainWindow() } catch {}
}
Start-Sleep -Seconds 2
Get-Process -Name Mist -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

Write-Host "==> Terminal scroll/selection UI tests ($exe)"
python (Join-Path $Root "scripts\gui_terminal_scroll_selection.py") $exe --timeout $TimeoutSec
if ($LASTEXITCODE -ne 0) { throw "GUI terminal scroll/selection tests failed (exit $LASTEXITCODE)" }

Write-Host ""
Write-Host "OK: terminal scroll/selection UI tests passed"
