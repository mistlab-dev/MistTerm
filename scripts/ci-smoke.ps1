# Low-memory local check mirroring GitHub Actions "test" job (tag builds).
# Usage:
#   .\scripts\ci-smoke.ps1              # compile + lib tests + zmodem test
#   .\scripts\ci-smoke.ps1 -CompileOnly   # compile only (fastest, lowest RAM)
#   .\scripts\ci-smoke.ps1 -WindowsCi     # add vendored-openssl (needs Perl on Windows)

param(
    [switch]$CompileOnly,
    [switch]$WindowsCi
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_INCREMENTAL = "0"

function Get-FeatureArgs {
    if ($WindowsCi) {
        return @("--features", "vendored-openssl")
    }
    return @()
}

function Invoke-CargoStep {
    param([string]$Label, [string[]]$CargoArgs)
    Write-Host ""
    Write-Host "== $Label =="
    $allArgs = $CargoArgs + (Get-FeatureArgs)
    & cargo @allArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo failed: cargo $($allArgs -join ' ')"
    }
}

Invoke-CargoStep "1/3 compile lib and integration tests" @(
    "test", "--lib", "--tests", "--no-run"
)

Invoke-CargoStep "2/3 compile examples" @(
    "test", "--examples", "--no-run"
)

Invoke-CargoStep "3/3 build bins" @(
    "build", "--bins"
)

if ($CompileOnly) {
    Write-Host ""
    Write-Host "OK: compile-only CI smoke passed"
    exit 0
}

Invoke-CargoStep "4/4 run lib unit tests" @(
    "test", "--lib", "-j", "1"
)

Invoke-CargoStep "5/5 zmodem integration test" @(
    "test", "--test", "zmodem_integration_test", "-j", "1"
)

Write-Host ""
Write-Host "OK: CI smoke passed (see docs/tech/TESTING.md)"
