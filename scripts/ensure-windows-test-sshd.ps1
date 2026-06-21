# 确保本地 OpenSSH 测试环境可用（mistterm_test / mistterm123 @ 127.0.0.1:22）
# 已就绪则快速通过；否则尝试调用 setup-windows-test-sshd.ps1（需管理员）。
#
# 用法:
#   .\scripts\ensure-windows-test-sshd.ps1
#   .\scripts\ensure-windows-test-sshd.ps1 -Quiet

param([switch]$Quiet)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$User = if ($env:MISTTERM_TEST_SSH_USER) { $env:MISTTERM_TEST_SSH_USER } else { "mistterm_test" }
$Pass = if ($env:MISTTERM_TEST_SSH_PASSWORD) { $env:MISTTERM_TEST_SSH_PASSWORD } else { "mistterm123" }

function Write-Step([string]$Msg) {
    if (-not $Quiet) { Write-Host $Msg }
}

function Test-SshLogin {
    python -c @"
import sys
import paramiko
try:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect('127.0.0.1', 22, '$User', '$Pass', timeout=8, allow_agent=False, look_for_keys=False)
    i, o, e = c.exec_command('echo ok')
    out = o.read().decode('utf-8', errors='replace').strip()
    err = e.read().decode('utf-8', errors='replace').strip()
    code = o.channel.recv_exit_status()
    c.close()
    print(out)
    if err:
        print('stderr:', err, file=sys.stderr)
    sys.exit(0 if code == 0 and out == 'ok' else 1)
except Exception as ex:
    print(str(ex), file=sys.stderr)
    sys.exit(1)
"@ 2>$null
    return $LASTEXITCODE -eq 0
}

Write-Step "==> Checking local OpenSSH test account ($User@127.0.0.1)"

python -c "import paramiko" 2>$null
if ($LASTEXITCODE -ne 0) {
    Write-Step "==> Installing paramiko..."
    pip install paramiko --quiet
    if ($LASTEXITCODE -ne 0) { throw "pip install paramiko failed" }
}

if (Test-SshLogin) {
    Write-Step "OK: sshd ready ($User@127.0.0.1, password auth verified, echo ok)"
    exit 0
}

Write-Step "==> SSH preflight failed; running setup-windows-test-sshd.ps1 (admin required)"
$setup = Join-Path $Root "scripts\setup-windows-test-sshd.ps1"
if (-not (Test-Path $setup)) { throw "Missing $setup" }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)
if ($isAdmin) {
    & $setup
} else {
    Write-Step "    Elevating to administrator..."
    Start-Process powershell -Verb RunAs -Wait -ArgumentList @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$setup`""
    )
}

if (-not (Test-SshLogin)) {
    throw "SSH still unavailable after setup. Run as admin: .\scripts\setup-windows-test-sshd.ps1"
}

Write-Step "OK: sshd configured and verified"
