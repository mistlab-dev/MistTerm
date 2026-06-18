# Full MistTerm validation: Cargo smoke + all Windows GUI scripts.
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
    Write-Host "========== Cargo CI smoke =========="
    & (Join-Path $Root "scripts\ci-smoke.ps1")
    if ($LASTEXITCODE -ne 0) { $failures += "ci-smoke" }
}

if (-not $TestsOnly) {
    Write-Host ""
    Write-Host "========== GUI validation =========="
    $profile = if ($Release) { "release" } else { "debug" }
    $exe = Join-Path $Root "target\$profile\Mist.exe"
    if (-not (Test-Path $exe)) {
        Write-Host "==> Building Mist ($profile)..."
        if ($Release) { cargo build --release --bin Mist }
        else { cargo build --bin Mist }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }

    Write-Host "==> Seed local test session"
    cargo run --bin seed_local_test_session
    if ($LASTEXITCODE -ne 0) { $failures += "seed session" }

    python -c "import pywinauto, paramiko" 2>$null
    if ($LASTEXITCODE -ne 0) {
        pip install pywinauto paramiko --quiet
    }

    $guiSteps = @(
        @{ Name = "menu walkthrough"; Script = "smoke_gui_interact.py"; Args = @($exe, "--timeout", "45") },
        @{ Name = "full workflow"; Script = "gui_full_workflow.py"; Args = @($exe, "--timeout", "150") },
        @{ Name = "local SSH SFTP E2E"; Script = "gui_e2e_local_ssh.py"; Args = @($exe, "--timeout", "120", "--skip-new-session") },
        @{ Name = "connect + panels"; Script = "gui_connect_transfer.py"; Args = @($exe, "--timeout", "30") }
    )

    foreach ($step in $guiSteps) {
        Write-Host ""
        Write-Host "==> GUI: $($step.Name)"
        python (Join-Path $Root "scripts\$($step.Script)") @($step.Args)
        if ($LASTEXITCODE -ne 0) { $failures += "GUI: $($step.Name)" }
    }
}

Write-Host ""
if ($failures.Count -eq 0) {
    Write-Host "OK: full validation passed"
    exit 0
}

Write-Host "FAILED sections:"
foreach ($f in $failures) { Write-Host "  - $f" }
exit 1
