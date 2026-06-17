# 全套 GUI 流程：新建连接 + SFTP + AI + 面板（不跑 cargo test）
# 前置: 管理员运行一次 .\scripts\setup-windows-test-sshd.ps1
# 自动化快捷键（MISTTERM_GUI_AUTOMATION=1）见 scripts/gui_automation_keys.py
# 用法:
#   .\scripts\run-gui-full-workflow.ps1
#   .\scripts\run-gui-full-workflow.ps1 -KeepOpen

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

$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"

Write-Host "==> 更新本地测试会话 (seed)"
cargo run --bin seed_local_test_session
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$profile = if ($Release) { "release" } else { "debug" }
$exe = Join-Path $Root "target\$profile\Mist.exe"
if (-not (Test-Path $exe)) {
    Write-Host "==> Building Mist ($profile)..."
    if ($Release) { cargo build --release --bin Mist }
    else { cargo build --bin Mist }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

python -c "import pywinauto, paramiko" 2>$null
if ($LASTEXITCODE -ne 0) {
    pip install pywinauto paramiko --quiet
}

$pyArgs = @($exe, "--timeout", "60")
if ($KeepOpen) { $pyArgs += "--keep-open" }

python (Join-Path $Root "scripts\gui_full_workflow.py") @pyArgs
exit $LASTEXITCODE
