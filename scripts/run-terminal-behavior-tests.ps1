# 终端行为回归：选区随滚动、输入回到底部、平台剪贴板快捷键
# Usage: .\scripts\run-terminal-behavior-tests.ps1

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"

function Invoke-CargoTest {
    param([string]$Label, [string[]]$CargoArgs)
    Write-Host ""
    Write-Host "== $Label =="
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "failed: cargo $($CargoArgs -join ' ')"
    }
}

Write-Host "Terminal behavior tests (low-memory, single-threaded)"

Invoke-CargoTest "alacritty adapter (scroll / text_in_point_range)" @(
    "test", "--lib", "alacritty::tests", "--", "--test-threads=1"
)

Invoke-CargoTest "terminal_keys unit tests" @(
    "test", "--lib", "terminal_keys", "--", "--test-threads=1"
)

Invoke-CargoTest "terminal_behavior integration" @(
    "test", "--test", "terminal_behavior_test", "--", "--test-threads=1"
)

Invoke-CargoTest "keyboard_shortcuts integration" @(
    "test", "--test", "keyboard_shortcuts_integration_test", "--", "--test-threads=1"
)

Invoke-CargoTest "terminal_focus" @(
    "test", "--test", "terminal_focus_test", "--", "--test-threads=1"
)

Write-Host ""
Write-Host "OK: all terminal behavior tests passed"
