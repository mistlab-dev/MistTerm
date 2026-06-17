# Full MistTerm validation: cargo tests + Windows GUI feature walkthrough.
# Usage:
#   .\scripts\run-full-validation.ps1
#   .\scripts\run-full-validation.ps1 -GuiOnly
#   .\scripts\run-full-validation.ps1 -TestsOnly

param(
    [switch]$GuiOnly,
    [switch]$TestsOnly,
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"
$env:RUST_TEST_THREADS = "1"

function Import-VsDevShell {
    $devShell = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
    if (Test-Path $devShell) {
        Import-Module $devShell
        Enter-VsDevShell -VsInstallPath "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools" -SkipAutomaticLocation -DevCmdArguments "-arch=amd64"
    }
}

Import-VsDevShell

$failures = @()

if (-not $GuiOnly) {
    Write-Host ""
    Write-Host "========== 1/2 Cargo tests =========="
    Write-Host "==> lib unit tests"
    cargo test --lib -j 1 -- --test-threads=1
    if ($LASTEXITCODE -ne 0) { $failures += "lib unit tests" }

    Write-Host ""
    Write-Host "==> integration tests (all tests/*)"
    cargo test --tests -j 1 -- --test-threads=1
    if ($LASTEXITCODE -ne 0) { $failures += "integration tests" }
}

if (-not $TestsOnly) {
    Write-Host ""
    Write-Host "========== 2/2 GUI feature walkthrough =========="
    $profile = if ($Release) { "release" } else { "debug" }
    $exe = Join-Path $Root "target\$profile\Mist.exe"
    if (-not (Test-Path $exe)) {
        Write-Host "==> Building Mist ($profile)..."
        if ($Release) { cargo build --release --bin Mist }
        else { cargo build --bin Mist }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }

    python -c "import pywinauto" 2>$null
    if ($LASTEXITCODE -ne 0) {
        pip install pywinauto --quiet
    }

    python (Join-Path $Root "scripts\smoke_gui_interact.py") $exe --timeout 25
    if ($LASTEXITCODE -ne 0) { $failures += "GUI walkthrough" }
}

Write-Host ""
if ($failures.Count -eq 0) {
    Write-Host "OK: full validation passed"
    exit 0
}

Write-Host "FAILED sections:"
foreach ($f in $failures) { Write-Host "  - $f" }
exit 1
