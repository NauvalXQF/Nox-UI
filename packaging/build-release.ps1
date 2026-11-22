# Builds a ready-to-share NOX release package in dist\NOX-v<version>-win64\ and .zip.
# Users only need to extract the ZIP and double-click Install-NOX.cmd to register
# Windows Native WinRT Package Identity and launch NOX automatically.
#
# Bonus: If Windows 10/11 SDK (makeappx.exe & signtool.exe) is available on this machine
# (e.g., locally or on GitHub Actions windows-latest), this script automatically builds
# a self-signed NOX.msix + NOX.cer so end users do NOT even need Developer Mode!

param(
    [switch]$SkipBuild,
    [switch]$NoZip
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$CargoToml = Get-Content (Join-Path $RepoRoot "Cargo.toml") -Raw
$Version = if ($CargoToml -match 'version\s*=\s*"([^"]+)"') { $Matches[1] } else { "0.5.0" }

if (-not $SkipBuild) {
    Write-Host "Membangun NOX v$Version (release)..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        cargo build --release --workspace
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build gagal dengan exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

$ReleaseBin = Join-Path $RepoRoot "target\release"
$OutName = "NOX-v$Version-win64"
$StageDir = Join-Path $RepoRoot "dist\$OutName"
$StagePkg = Join-Path $StageDir "packaging"
$ZipPath = Join-Path $RepoRoot "dist\$OutName.zip"

if (Test-Path $StageDir) {
    Remove-Item $StageDir -Recurse -Force
}
New-Item -ItemType Directory -Path $StagePkg -Force | Out-Null

# Copy binaries
Copy-Item (Join-Path $ReleaseBin "nox.exe") (Join-Path $StageDir "nox.exe") -Force
if (Test-Path (Join-Path $ReleaseBin "nox-guard.exe")) {
    Copy-Item (Join-Path $ReleaseBin "nox-guard.exe") (Join-Path $StageDir "nox-guard.exe") -Force
}

# Copy 1-click launchers to root of release folder
Copy-Item (Join-Path $PSScriptRoot "Install-NOX.cmd") (Join-Path $StageDir "Install-NOX.cmd") -Force
Copy-Item (Join-Path $PSScriptRoot "Uninstall-NOX.cmd") (Join-Path $StageDir "Uninstall-NOX.cmd") -Force

# Copy packaging assets
Copy-Item (Join-Path $PSScriptRoot "AppxManifest.xml") (Join-Path $StagePkg "AppxManifest.xml") -Force
Copy-Item (Join-Path $PSScriptRoot "register-sparse.ps1") (Join-Path $StagePkg "register-sparse.ps1") -Force
if (Test-Path (Join-Path $PSScriptRoot "nox.png")) {
    Copy-Item (Join-Path $PSScriptRoot "nox.png") (Join-Path $StagePkg "nox.png") -Force
} elseif (Test-Path (Join-Path $RepoRoot "assets\logo-rounded.png")) {
    Copy-Item (Join-Path $RepoRoot "assets\logo-rounded.png") (Join-Path $StagePkg "nox.png") -Force
}

# Optional: Auto-build & self-sign NOX.msix if Windows SDK tools are installed
$kits = "C:\Program Files (x86)\Windows Kits\10\bin"
$makeAppx = $null
$signTool = $null
if (Test-Path $kits) {
    $makeAppx = Get-ChildItem -Path $kits -Recurse -Filter "makeappx.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -Last 1 -ExpandProperty FullName
    $signTool = Get-ChildItem -Path $kits -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -Last 1 -ExpandProperty FullName
}

if ($makeAppx -and $signTool) {
    Write-Host "Windows SDK terdeteksi — membuat paket self-signed NOX.msix..." -ForegroundColor Cyan
    $msixBuildDir = Join-Path $env:TEMP "nox-msix-stage"
    if (Test-Path $msixBuildDir) { Remove-Item $msixBuildDir -Recurse -Force }
    New-Item -ItemType Directory -Path $msixBuildDir -Force | Out-Null
    Copy-Item (Join-Path $StagePkg "AppxManifest.xml") $msixBuildDir -Force
    Copy-Item (Join-Path $StagePkg "nox.png") $msixBuildDir -Force

    $msixOut = Join-Path $StagePkg "NOX.msix"
    & $makeAppx pack /d $msixBuildDir /p $msixOut /nv /o | Out-Null

    $cert = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq "CN=Opankcs" -and $_.HasPrivateKey } | Select-Object -First 1
    if (-not $cert) {
        $cert = New-SelfSignedCertificate -Type Custom -Subject "CN=Opankcs" `
            -KeyUsage DigitalSignature -FriendlyName "NOX Self-Signed Dev Cert" `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
    }
    $cerOut = Join-Path $StagePkg "NOX.cer"
    Export-Certificate -Cert $cert -FilePath $cerOut -Force | Out-Null
    & $signTool sign /fd SHA256 /sha1 $cert.Thumbprint $msixOut | Out-Null
    Remove-Item $msixBuildDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "NOX.msix + NOX.cer berhasil dibuat (tanpa perlu Developer Mode!)." -ForegroundColor Green
} else {
    Write-Host "Windows SDK tidak terdeteksi — paket memakai AppxManifest.xml (Install-NOX.cmd tetap 1-klik)." -ForegroundColor DarkGray
}

# Write friendly quick-start instructions
$readme = @"
NOX v$Version - Built by opankcs.com
====================================

Cara Menjalankan (Disarankan - Notifikasi Native WinRT):
  1. Ekstrak seluruh isi ZIP ini ke folder mana saja.
  2. Klik ganda (double-click) file: Install-NOX.cmd
  3. Ikuti petunjuk singkat di layar -> NOX akan langsung terbuka otomatis!
     Saat Windows meminta izin notifikasi, klik 'Allow / Izinkan'.

Cara Cepat (Tanpa Instalasi - Mode Zero-Config):
  - Kamu juga bisa langsung klik ganda 'nox.exe'.
    Notifikasi tetap berjalan otomatis memakai UI Automation (Zero-Config).

Cara Uninstall:
  - Klik ganda 'Uninstall-NOX.cmd' (atau dari %LOCALAPPDATA%\NOX\Uninstall-NOX.cmd).
"@
Set-Content -Path (Join-Path $StageDir "README-INSTALL.txt") -Value $readme -Encoding UTF8

if (-not $NoZip) {
    if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
    Compress-Archive -Path "$StageDir\*" -DestinationPath $ZipPath -Force
    Write-Host ""
    Write-Host "Paket rilis siap dibagikan:" -ForegroundColor Green
    Write-Host "  Folder : $StageDir"
    Write-Host "  ZIP    : $ZipPath"
} else {
    Write-Host ""
    Write-Host "Folder rilis siap: $StageDir" -ForegroundColor Green
}
