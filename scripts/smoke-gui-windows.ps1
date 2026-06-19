# Windows 桌面 GUI 冒烟测试：启动 Mist.exe，点击顶栏菜单并验证不崩溃。
# 依赖：target\debug\Mist.exe（或 -Release）、pip install pywinauto
#
# 用法:
#   .\scripts\smoke-gui-windows.ps1
#   .\scripts\smoke-gui-windows.ps1 -Release -TimeoutSec 20
#   .\scripts\smoke-gui-windows.ps1 -LaunchOnly   # 仅启动窗口，不自动关（手测）

param(
    [switch]$Release,
    [switch]$LaunchOnly,
    [int]$TimeoutSec = 20,
    [string]$WindowTitle = "Mist"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$profile = if ($Release) { "release" } else { "debug" }
$exe = Join-Path $Root "target\$profile\Mist.exe"
if (-not (Test-Path $exe)) {
    Write-Host "==> Building Mist ($profile)..."
    $env:CARGO_BUILD_JOBS = "1"
    $env:CARGO_INCREMENTAL = "0"
    if ($Release) {
        cargo build --release --bin Mist
    } else {
        cargo build --bin Mist
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

Write-Host "==> Seed local test session"
$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { throw "seed_local_test_session failed" }

if ($LaunchOnly) {
    Write-Host "==> Launching (manual): $exe"
    Start-Process -FilePath $exe
    Write-Host "OK: Mist started — close the window when done."
    exit 0
}

$pyScript = Join-Path $Root "scripts\smoke_gui_interact.py"
if (-not (Test-Path $pyScript)) {
    throw "Missing $pyScript"
}

python -c "import pywinauto, paramiko" 2>$null
if ($LASTEXITCODE -ne 0) {
    Write-Host "==> Installing pywinauto, paramiko..."
    pip install pywinauto paramiko --quiet
    if ($LASTEXITCODE -ne 0) { throw "pip install failed" }
}

Write-Host "==> GUI full feature walkthrough"
python $pyScript $exe --title $WindowTitle --timeout $TimeoutSec
if ($LASTEXITCODE -ne 0) {
    throw "GUI interaction smoke failed (exit $LASTEXITCODE)"
}

Write-Host ""
Write-Host "OK: GUI walkthrough finished (see summary above)"
