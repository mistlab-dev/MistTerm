# 配置 Windows OpenSSH 本地测试账号（mistterm_test / mistterm123）
# 需管理员权限。用法: 以管理员运行 PowerShell:
#   .\scripts\setup-windows-test-sshd.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

$User = "mistterm_test"
$Pass = "mistterm123"
$SftpRoot = "C:\Users\$User\mistterm_sftp"
$SshdConfig = "C:\ProgramData\ssh\sshd_config"

Write-Host "==> Ensuring OpenSSH sshd is running"
Set-Service sshd -StartupType Automatic
Start-Service sshd

Write-Host "==> Creating local user $User"
if (-not (Get-LocalUser -Name $User -ErrorAction SilentlyContinue)) {
    $sec = ConvertTo-SecureString $Pass -AsPlainText -Force
    New-LocalUser -Name $User -Password $sec -FullName "MistTerm SSH Test" -PasswordNeverExpires | Out-Null
    Write-Host "    Created user $User"
} else {
    Write-Host "    User $User already exists"
}

Write-Host "==> SFTP workspace: $SftpRoot"
New-Item -ItemType Directory -Force -Path $SftpRoot | Out-Null
icacls $SftpRoot /grant "${User}:(OI)(CI)F" /T | Out-Null

Write-Host "==> sshd_config: enable password auth"
$cfg = Get-Content $SshdConfig -Raw
if ($cfg -notmatch '(?m)^PasswordAuthentication\s+yes') {
    Add-Content $SshdConfig "`n# MistTerm local test`nPasswordAuthentication yes`n"
    Write-Host "    Appended PasswordAuthentication yes"
}
Restart-Service sshd
Start-Sleep -Seconds 2

Write-Host "==> Verifying SSH password login"
$env:MISTTERM_TEST_SSH_USER = $User
$env:MISTTERM_TEST_SSH_PASSWORD = $Pass
$testPy = @"
import sys, socket
import paramiko
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect('127.0.0.1', 22, '$User', '$Pass', timeout=8, allow_agent=False, look_for_keys=False)
stdin, stdout, stderr = client.exec_command('echo ok')
out = stdout.read().decode().strip()
client.close()
print('SSH:', out)
sys.exit(0 if out == 'ok' else 1)
"@
$pyTmp = Join-Path $env:TEMP "mistterm_ssh_verify.py"
Set-Content $pyTmp $testPy -Encoding UTF8
python -c "import paramiko" 2>$null
if ($LASTEXITCODE -ne 0) { pip install paramiko --quiet }
python $pyTmp
if ($LASTEXITCODE -ne 0) {
    Write-Warning "paramiko verify failed; try: ssh ${User}@127.0.0.1"
} else {
    Write-Host "OK: local sshd ready ($User@127.0.0.1, password $Pass)"
}

Write-Host "==> SSH key for integration tests (rz_upload_smoke)"
$adminKey = Join-Path $env:USERPROFILE ".ssh\id_rsa_mistterm_test"
if (-not (Test-Path $adminKey)) {
    ssh-keygen -t rsa -b 2048 -f $adminKey -N '""' -q
}
$sshDir = "C:\Users\$User\.ssh"
New-Item -ItemType Directory -Force -Path $sshDir | Out-Null
Set-Content -Path "$sshDir\authorized_keys" -Value (Get-Content "$adminKey.pub") -Encoding ascii
icacls $sshDir /inheritance:r /grant "${User}:(OI)(CI)F" "SYSTEM:(OI)(CI)F" "Administrators:(OI)(CI)F" | Out-Null
Write-Host "    $adminKey -> ${User}@127.0.0.1"

Write-Host ""
Write-Host "Next:"
Write-Host "  .\scripts\run-transfer-tests.ps1"
Write-Host "  (includes GUI connect + SFTP panel smoke)"
