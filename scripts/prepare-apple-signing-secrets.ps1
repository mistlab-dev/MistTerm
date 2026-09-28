#Requires -Version 5.1
<#
.SYNOPSIS
  Prepare Developer ID CSR/p12 and push GitHub Actions secrets for macOS notarization.

.DESCRIPTION
  Does not log into Apple. After you upload the CSR and drop the issued .cer
  (and App Store Connect .p8) into secrets/apple-signing/, re-run this script.

.EXAMPLE
  powershell -File scripts/prepare-apple-signing-secrets.ps1 -Step csr
  powershell -File scripts/prepare-apple-signing-secrets.ps1 -Step p12
  powershell -File scripts/prepare-apple-signing-secrets.ps1 -Step secrets
#>
param(
    [ValidateSet('csr', 'p12', 'secrets', 'all')]
    [string]$Step = 'csr',

    [string]$P12Password,
    [string]$TeamId,
    [string]$CodesignIdentity,
    [string]$AppStoreIssuerId,
    [string]$AppStoreKeyId
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Dir = Join-Path $Root 'secrets\apple-signing'
$KeyPath = Join-Path $Dir 'mistterm-developer-id.key'
$CsrPath = Join-Path $Dir 'mistterm-developer-id.csr'
$P12Path = Join-Path $Dir 'mistterm-developer-id.p12'
$IssuedCer = Join-Path $Dir 'developerID_application.cer'
$IssuedPem = Join-Path $Dir 'developerID_application.pem'
$G2Cer = Join-Path $Dir 'DeveloperIDG2CA.cer'
$G2Pem = Join-Path $Dir 'DeveloperIDG2CA.pem'
$PwdFile = Join-Path $Dir 'p12.password.pwd'
$EnvFile = Join-Path $Dir 'github-secrets.env'
$EnvExample = Join-Path $Dir 'github-secrets.env.example'

function Find-OpenSsl {
    $candidates = @(
        'C:\Program Files\Git\usr\bin\openssl.exe',
        'C:\Program Files\Git\mingw64\bin\openssl.exe'
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    $cmd = Get-Command openssl -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    throw 'OpenSSL not found. Install Git for Windows (includes openssl) or add openssl to PATH.'
}

function Read-DotEnv([string]$Path) {
    $map = @{}
    if (-not (Test-Path $Path)) { return $map }
    Get-Content -LiteralPath $Path -Encoding UTF8 | ForEach-Object {
        $line = $_.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { return }
        $idx = $line.IndexOf('=')
        if ($idx -lt 1) { return }
        $name = $line.Substring(0, $idx).Trim()
        $value = $line.Substring($idx + 1)
        if ($value.StartsWith('"') -and $value.EndsWith('"') -and $value.Length -ge 2) {
            $value = $value.Substring(1, $value.Length - 2)
        }
        $map[$name] = $value
    }
    return $map
}

function Get-EnvOrFile([string]$Name, [string]$Fallback) {
    if ($Fallback) { return $Fallback }
    if ($env:GITHUB_SECRETS_ENV) {
        # unused hook
    }
    $fromFile = Read-DotEnv $EnvFile
    if ($fromFile.ContainsKey($Name) -and $fromFile[$Name]) { return $fromFile[$Name] }
    $fromExample = Read-DotEnv $EnvExample
    if ($fromExample.ContainsKey($Name) -and $fromExample[$Name] -and $fromExample[$Name] -notmatch '^REPLACE') {
        return $fromExample[$Name]
    }
    $envVal = [Environment]::GetEnvironmentVariable($Name)
    if ($envVal) { return $envVal }
    return $null
}

function Invoke-OpenSsl {
    param([Parameter(Mandatory = $true)][string[]]$Args)
    $openssl = Find-OpenSsl
    & $openssl @Args
    if ($LASTEXITCODE -ne 0) {
        throw "openssl failed ($LASTEXITCODE): $($Args -join ' ')"
    }
}

function Ensure-SigningDir {
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
}

function Step-Csr {
    Ensure-SigningDir
    if (-not (Test-Path $KeyPath)) {
        Write-Host "==> generating RSA key: $KeyPath"
        Invoke-OpenSsl -Args @('genrsa', '-out', $KeyPath, '2048')
    } else {
        Write-Host "==> reusing existing key: $KeyPath"
    }
    Write-Host "==> generating CSR: $CsrPath"
    Invoke-OpenSsl -Args @(
        'req', '-new',
        '-key', $KeyPath,
        '-out', $CsrPath,
        '-subj', '/CN=MistTerm Developer ID/C=CN'
    )
    Write-Host "==> downloading Apple Developer ID G2 intermediate"
    Invoke-WebRequest -Uri 'https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer' -OutFile $G2Cer -UseBasicParsing
    Write-Host @"

CSR ready. Next (Apple Developer, logged in as the paid team):

  1. Membership: copy Team ID
  2. Identifiers: App ID bundle id com.mist.term (Explicit)
  3. Certificates: Developer ID Application (G2) — upload:
     $CsrPath
  4. Save the issued certificate as:
     $IssuedCer

Then run:
  powershell -File scripts/prepare-apple-signing-secrets.ps1 -Step p12

App Store Connect API Key (notarization):
  https://appstoreconnect.apple.com → Users and Access → Integrations → Team Keys
  Save AuthKey_<KEYID>.p8 into:
     $Dir
  Fill secrets/apple-signing/github-secrets.env (copy from .example)

"@
}

function Convert-DerCerToPem([string]$CerPath, [string]$PemPath) {
    $text = Get-Content -LiteralPath $CerPath -Raw -ErrorAction SilentlyContinue
    if ($text -match '-----BEGIN CERTIFICATE-----') {
        Copy-Item -LiteralPath $CerPath -Destination $PemPath -Force
        return
    }
    Invoke-OpenSsl -Args @('x509', '-inform', 'DER', '-in', $CerPath, '-out', $PemPath)
}

function Step-P12 {
    Ensure-SigningDir
    if (-not (Test-Path $KeyPath)) { throw "missing private key: $KeyPath (run -Step csr first)" }
    if (-not (Test-Path $IssuedCer)) {
        throw @"
missing issued certificate: $IssuedCer

Upload $CsrPath at https://developer.apple.com/account/resources/certificates/list
then save the downloaded .cer to that path and re-run -Step p12.
"@
    }
    if (-not (Test-Path $G2Cer)) {
        Invoke-WebRequest -Uri 'https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer' -OutFile $G2Cer -UseBasicParsing
    }
    Convert-DerCerToPem $IssuedCer $IssuedPem
    Convert-DerCerToPem $G2Cer $G2Pem

    $pass = $P12Password
    if (-not $pass) { $pass = Get-EnvOrFile 'APPLE_CERTIFICATE_PASSWORD' $null }
    if (-not $pass -and (Test-Path $PwdFile)) { $pass = (Get-Content -LiteralPath $PwdFile -Raw).Trim() }
    if (-not $pass) {
        $sec = Read-Host -AsSecureString -Prompt 'p12 export password (saved only in secrets/apple-signing/p12.password.pwd)'
        $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
        try { $pass = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
    }
    if (-not $pass) { throw 'p12 password is empty' }
    Set-Content -LiteralPath $PwdFile -Value $pass -Encoding ascii -NoNewline

    $exportArgs = @(
        'pkcs12', '-export',
        '-inkey', $KeyPath,
        '-in', $IssuedPem,
        '-certfile', $G2Pem,
        '-out', $P12Path,
        '-name', 'Developer ID Application',
        '-passout', "pass:$pass"
    )
    try {
        Invoke-OpenSsl -Args ($exportArgs + '-legacy')
    } catch {
        Write-Host 'openssl -legacy not accepted, retrying without it'
        Invoke-OpenSsl -Args $exportArgs
    }

    Write-Host "==> reading codesign subject from issued cert"
    $openssl = Find-OpenSsl
    $subject = & $openssl x509 -in $IssuedPem -noout -subject -nameopt RFC2253
    Write-Host $subject

    $identity = $CodesignIdentity
    if (-not $identity) { $identity = Get-EnvOrFile 'APPLE_CODESIGN_IDENTITY' $null }
    if (-not $identity -and $subject) {
        # subject=CN=Developer ID Application: Name (TEAMID),OU=...,O=...,C=US
        if ($subject -match 'CN=([^,]+)') {
            $identity = $Matches[1]
            Write-Host "==> suggested APPLE_CODESIGN_IDENTITY: $identity"
        }
    }

    Write-Host "==> p12 written: $P12Path"
}

function Find-P8 {
    $named = Get-ChildItem -LiteralPath $Dir -Filter 'AuthKey_*.p8' -ErrorAction SilentlyContinue
    if ($named) { return $named[0].FullName }
    $any = Get-ChildItem -LiteralPath $Dir -Filter '*.p8' -ErrorAction SilentlyContinue
    if ($any) { return $any[0].FullName }
    return $null
}

function Step-Secrets {
    Ensure-SigningDir
    if (-not (Test-Path $P12Path)) { throw "missing p12: $P12Path (run -Step p12 first)" }
    $p8 = Find-P8
    if (-not $p8) {
        throw @"
missing App Store Connect API key .p8 in $Dir

Create a Team Key at App Store Connect → Users and Access → Integrations,
download the .p8 once, copy it here as AuthKey_<KEYID>.p8, fill github-secrets.env,
then re-run -Step secrets.
"@
    }

    $team = Get-EnvOrFile 'APPLE_TEAM_ID' $TeamId
    $identity = Get-EnvOrFile 'APPLE_CODESIGN_IDENTITY' $CodesignIdentity
    $issuer = Get-EnvOrFile 'APPSTORE_ISSUER_ID' $AppStoreIssuerId
    $keyId = Get-EnvOrFile 'APPSTORE_KEY_ID' $AppStoreKeyId
    $pass = Get-EnvOrFile 'APPLE_CERTIFICATE_PASSWORD' $P12Password
    if (-not $pass -and (Test-Path $PwdFile)) { $pass = (Get-Content -LiteralPath $PwdFile -Raw).Trim() }

    if (-not $keyId -and (Split-Path $p8 -Leaf) -match 'AuthKey_([A-Z0-9]+)\.p8') {
        $keyId = $Matches[1]
        Write-Host "==> APPSTORE_KEY_ID from filename: $keyId"
    }

    $missing = @()
    if (-not $team) { $missing += 'APPLE_TEAM_ID' }
    if (-not $identity) { $missing += 'APPLE_CODESIGN_IDENTITY' }
    if (-not $issuer) { $missing += 'APPSTORE_ISSUER_ID' }
    if (-not $keyId) { $missing += 'APPSTORE_KEY_ID' }
    if (-not $pass) { $missing += 'APPLE_CERTIFICATE_PASSWORD' }
    if ($missing.Count -gt 0) {
        throw "fill secrets/apple-signing/github-secrets.env (copy from .example). missing: $($missing -join ', ')"
    }

    $gh = Get-Command gh -ErrorAction SilentlyContinue
    if (-not $gh) { throw 'GitHub CLI (gh) not found. Install it and run gh auth login.' }

    $p12B64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($P12Path))
    $p8Body = Get-Content -LiteralPath $p8 -Raw

    $pairs = @{
        APPLE_CERTIFICATE_BASE64     = $p12B64
        APPLE_CERTIFICATE_PASSWORD   = $pass
        APPLE_TEAM_ID                = $team
        APPLE_CODESIGN_IDENTITY      = $identity
        APPSTORE_ISSUER_ID           = $issuer
        APPSTORE_KEY_ID              = $keyId
        APPSTORE_PRIVATE_KEY         = $p8Body
    }

    Push-Location $Root
    try {
        foreach ($name in $pairs.Keys) {
            Write-Host "==> gh secret set $name"
            $pairs[$name] | & gh secret set $name
            if ($LASTEXITCODE -ne 0) { throw "gh secret set $name failed" }
        }
    } finally {
        Pop-Location
    }
    Write-Host '==> all 7 repository secrets updated'
}

switch ($Step) {
    'csr' { Step-Csr }
    'p12' { Step-P12 }
    'secrets' { Step-Secrets }
    'all' {
        Step-Csr
        Step-P12
        Step-Secrets
    }
}
