# @deprecated Use run-gui-e2e.ps1 (GUI-only). Wrapper for compatibility.
& (Join-Path $PSScriptRoot "run-gui-e2e.ps1") @args
exit $LASTEXITCODE
