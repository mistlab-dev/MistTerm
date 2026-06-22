# 为本地 SSH 测试账号安装 lrzsz（rz/sz），供 ZMODEM GUI/Rust 集成测试使用。
# 需管理员权限（修改 Machine PATH）。用法:
#   .\scripts\setup-windows-test-lrzsz.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$LrzDir = "C:\ProgramData\mistterm\lrzsz"
$ZipUrl = "https://github.com/trzsz/lrzsz-win32/releases/download/v0.12.21rc/lrzsz_0.12.21rc_windows_x86_64.zip"
$ZipPath = Join-Path $env:TEMP "lrzsz_0.12.21rc_windows_x86_64.zip"

Write-Host "==> Installing lrzsz for local ZMODEM tests ($LrzDir)"

New-Item -ItemType Directory -Force -Path $LrzDir | Out-Null

$needDownload = -not (Test-Path (Join-Path $LrzDir "rz.exe"))
if ($needDownload) {
    Write-Host "    Downloading $ZipUrl"
    Invoke-WebRequest -Uri $ZipUrl -OutFile $ZipPath -UseBasicParsing
    $extract = Join-Path $env:TEMP "lrzsz_extract"
    if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
    Expand-Archive -Path $ZipPath -DestinationPath $extract -Force
    $inner = Get-ChildItem $extract -Recurse -Filter "rz.exe" | Select-Object -First 1
    if (-not $inner) { throw "rz.exe not found in archive" }
    $srcDir = $inner.Directory.FullName
    Copy-Item (Join-Path $srcDir "rz.exe") $LrzDir -Force
    Copy-Item (Join-Path $srcDir "sz.exe") $LrzDir -Force
    if (Test-Path (Join-Path $srcDir "msys-2.0.dll")) {
        Copy-Item (Join-Path $srcDir "msys-2.0.dll") $LrzDir -Force
    }
    Remove-Item $extract -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $ZipPath -Force -ErrorAction SilentlyContinue
    Write-Host "    Extracted rz.exe / sz.exe"
} else {
    Write-Host "    rz.exe already present"
}

$machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
if ($machinePath -notlike "*$LrzDir*") {
    $newPath = if ($machinePath) { "$machinePath;$LrzDir" } else { $LrzDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "Machine")
    $env:Path = "$env:Path;$LrzDir"
    Write-Host "    Added to Machine PATH"
} else {
    Write-Host "    Already on Machine PATH"
}

Write-Host "==> Verifying via SSH (mistterm_test@127.0.0.1)"
$verifyPy = @"
import sys
import paramiko
c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('127.0.0.1', 22, 'mistterm_test', 'mistterm123', timeout=10, allow_agent=False, look_for_keys=False)
for cmd in ['where rz', 'where sz']:
    i, o, e = c.exec_command(cmd)
    out = (o.read() + e.read()).decode('utf-8', errors='replace').strip()
    print(cmd + ':', out or '(missing)')
    if not out:
        sys.exit(1)
c.close()
print('OK')
"@
$pyTmp = Join-Path $env:TEMP "mistterm_lrzsz_verify.py"
Set-Content $pyTmp $verifyPy -Encoding UTF8
python -c "import paramiko" 2>$null
if ($LASTEXITCODE -ne 0) { pip install paramiko --quiet }
python $pyTmp
if ($LASTEXITCODE -ne 0) {
    Write-Warning "SSH verify failed; restart sshd or open a new SSH session and retry."
    Restart-Service sshd -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    python $pyTmp
    if ($LASTEXITCODE -ne 0) { throw "lrzsz not visible to mistterm_test SSH sessions" }
}

Write-Host "OK: lrzsz ready for ZMODEM tests ($LrzDir)"
