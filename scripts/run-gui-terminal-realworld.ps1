# 真实场景终端测试：正常/异常命令 + ZMODEM rz（SSH 校验远端结果）
# 用法: .\scripts\run-gui-terminal-realworld.ps1

param(
    [switch]$Release,
    [int]$TimeoutSec = 90
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$profile = if ($Release) { "release" } else { "debug" }
$exe = Join-Path $Root "target\$profile\Mist.exe"
if (-not (Test-Path $exe)) {
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    if ($Release) { cargo build --release --bin Mist }
    else { cargo build --bin Mist }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

Write-Host "==> Ensure local OpenSSH + lrzsz"
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root "scripts\ensure-windows-test-sshd.ps1")
if ($LASTEXITCODE -ne 0) { throw "ensure-windows-test-sshd failed" }
$lrzSetup = Join-Path $Root "scripts\setup-windows-test-lrzsz.ps1"
if (Test-Path $lrzSetup) {
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator
    )
    if ($isAdmin) {
        & $lrzSetup
    } else {
        Write-Host "    (skip lrzsz setup: not admin; ZMODEM tests may skip)"
    }
}

Write-Host "==> Seed local test session"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { throw "seed failed" }

python -c "import pywinauto, paramiko" 2>$null
if ($LASTEXITCODE -ne 0) { pip install pywinauto paramiko --quiet }

Write-Host "==> Terminal real-world GUI tests"
python (Join-Path $Root "scripts\gui_terminal_realworld.py") $exe --timeout $TimeoutSec
if ($LASTEXITCODE -ne 0) { throw "terminal real-world tests failed (exit $LASTEXITCODE)" }

Write-Host ""
Write-Host "==> ZMODEM rz over SSH PTY (rz_upload_smoke)"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
$env:RZ_SMOKE_SSH = "mistterm_test@127.0.0.1"
$env:RZ_SMOKE_SSH_IDENTITY = "$env:USERPROFILE\.ssh\id_rsa_mistterm_test"
$env:RZ_SMOKE_REMOTE_CMD = "cd /d C:\Users\mistterm_test\mistterm_sftp && rz -bye"
$env:RZ_SMOKE_FILE_COUNT = "1"
cargo run --bin rz_upload_smoke
if ($LASTEXITCODE -ne 0) { throw "rz_upload_smoke failed" }

Write-Host ""
Write-Host "==> Rust ZMODEM / ssh integration (narrow)"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
cargo test --test zmodem_integration_test -- --test-threads=1 --nocapture
if ($LASTEXITCODE -ne 0) { throw "zmodem_integration_test failed" }

Write-Host ""
Write-Host "OK: terminal real-world + zmodem integration finished"
