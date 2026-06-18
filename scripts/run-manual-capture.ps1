# 为操作手册采集 GUI 截图（需本地 sshd + seed 会话）
# 用法: .\scripts\run-manual-capture.ps1

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"

Write-Host "==> Seed local test session"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $Root "target\debug\Mist.exe"
if (-not (Test-Path $exe)) {
    cargo build --bin Mist
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

python -c "import pywinauto, paramiko, PIL" 2>$null
if ($LASTEXITCODE -ne 0) {
    pip install pywinauto paramiko Pillow --quiet
}

python (Join-Path $Root "scripts\gui_capture_manual.py") $exe --timeout 150
exit $LASTEXITCODE
