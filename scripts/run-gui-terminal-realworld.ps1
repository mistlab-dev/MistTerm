# 真实场景终端 GUI 测试（默认读 MISTTERM_TEST_SSH_*；远程 Linux 时跳过本地 sshd/lrzsz）
# 用法: .\scripts\run-gui-terminal-realworld.ps1

param(
    [switch]$Release,
    [int]$TimeoutSec = 10
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Get-EnvOrUser([string]$Name) {
    $proc = [Environment]::GetEnvironmentVariable($Name, 'Process')
    if ($proc) { return $proc }
    $user = [Environment]::GetEnvironmentVariable($Name, 'User')
    if ($user) { return $user }
    return [Environment]::GetEnvironmentVariable($Name, 'Machine')
}

$hostName = Get-EnvOrUser 'MISTTERM_TEST_SSH_HOST'
if ($hostName) { $env:MISTTERM_TEST_SSH_HOST = $hostName }
$pass = Get-EnvOrUser 'MISTTERM_TEST_SSH_PASSWORD'
if ($pass) { $env:MISTTERM_TEST_SSH_PASSWORD = $pass }
$user = Get-EnvOrUser 'MISTTERM_TEST_SSH_USER'
if ($user) { $env:MISTTERM_TEST_SSH_USER = $user }
$port = Get-EnvOrUser 'MISTTERM_TEST_SSH_PORT'
if ($port) { $env:MISTTERM_TEST_SSH_PORT = $port }
$sftpRoot = Get-EnvOrUser 'MISTTERM_TEST_SSH_SFTP_ROOT'
if ($sftpRoot) { $env:MISTTERM_TEST_SSH_SFTP_ROOT = $sftpRoot }

$isLocal = (-not $hostName) -or ($hostName -in @('127.0.0.1', 'localhost', '::1'))
if (-not $isLocal) {
    if (-not $env:MISTTERM_TEST_SSH_USER) { $env:MISTTERM_TEST_SSH_USER = 'root' }
    if (-not $env:MISTTERM_TEST_SSH_SFTP_ROOT) { $env:MISTTERM_TEST_SSH_SFTP_ROOT = '/tmp/mistterm_sftp' }
    Write-Host "==> Remote SSH test target: $($env:MISTTERM_TEST_SSH_USER)@$hostName"
} else {
    Write-Host "==> Local SSH test target (127.0.0.1)"
}

$profile = if ($Release) { "release" } else { "debug" }
$exe = Join-Path $Root "target\$profile\Mist.exe"
if (-not (Test-Path $exe)) {
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    if ($Release) { cargo build --release --bin Mist }
    else { cargo build --bin Mist }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

if ($isLocal) {
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
} else {
    Write-Host "==> Skip local sshd/lrzsz (remote Linux target)"
}

Write-Host "==> Seed MistTerm test session"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { throw "seed failed" }

python -c "import pywinauto, paramiko" 2>$null
if ($LASTEXITCODE -ne 0) { pip install pywinauto paramiko --quiet }

Write-Host "==> Terminal real-world GUI tests"
python (Join-Path $Root "scripts\gui_terminal_realworld.py") $exe --timeout $TimeoutSec
if ($LASTEXITCODE -ne 0) { throw "terminal real-world tests failed (exit $LASTEXITCODE)" }

if (-not $isLocal) {
    Write-Host ""
    Write-Host "==> Rust ZMODEM integration (remote)"
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    cargo test --test zmodem_integration_test -- --test-threads=1 --nocapture
    if ($LASTEXITCODE -ne 0) { throw "zmodem_integration_test failed" }
} else {
    Write-Host ""
    Write-Host "==> ZMODEM rz over SSH PTY (local rz_upload_smoke)"
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    $env:RZ_SMOKE_SSH = "mistterm_test@127.0.0.1"
    $env:RZ_SMOKE_SSH_IDENTITY = "$env:USERPROFILE\.ssh\id_rsa_mistterm_test"
    $env:RZ_SMOKE_REMOTE_CMD = "cd /d C:\Users\mistterm_test\mistterm_sftp && rz -bye"
    $env:RZ_SMOKE_FILE_COUNT = "1"
    cargo run --bin rz_upload_smoke
    if ($LASTEXITCODE -ne 0) { throw "rz_upload_smoke failed" }

    Write-Host ""
    Write-Host "==> Rust ZMODEM / ssh integration (local)"
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    cargo test --test zmodem_integration_test -- --test-threads=1 --nocapture
    if ($LASTEXITCODE -ne 0) { throw "zmodem_integration_test failed" }
}

Write-Host ""
Write-Host "OK: terminal real-world GUI tests finished"
