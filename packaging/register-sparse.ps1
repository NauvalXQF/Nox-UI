# Registers NOX as a Windows 10/11 Sparse Package so WinRT UserNotificationListener
# works natively (Jalur #1: Aktif (WinRT)) and launches NOX immediately.
# Works both from a distributed ZIP folder (via Install-NOX.cmd) and from this dev repo.

param(
    [string]$BinDir = "",
    [switch]$InstallToAppData,
    [switch]$NoLaunch,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$PkgName = "Opankcs.NoxShell"
$LegacyPkgName = "NauvalXQF.AbraShell"
$AppId = "Nox"
$InstallRoot = Join-Path $env:LOCALAPPDATA "NOX"

function Stop-NoxProcesses {
    Get-Process nox, nox-guard, abra, abra-guard -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
}

function Remove-NoxPackage {
    foreach ($name in @($PkgName, $LegacyPkgName)) {
        $existing = Get-AppxPackage -Name $name -ErrorAction SilentlyContinue
        if ($existing) {
            Write-Host "Menghapus registrasi paket lama ($name)..." -ForegroundColor DarkGray
            $existing | Remove-AppxPackage -ErrorAction SilentlyContinue
        }
    }
}

if ($Uninstall) {
    Write-Host ""
    Write-Host "=== Uninstall NOX ===" -ForegroundColor Cyan
    Stop-NoxProcesses
    Remove-NoxPackage
    if (Test-Path $InstallRoot) {
        Write-Host "Menghapus folder aplikasi: $InstallRoot" -ForegroundColor DarkGray
        Remove-Item -Path $InstallRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Host ""
    Write-Host "NOX berhasil dihapus dari Windows." -ForegroundColor Green
    Write-Host "(Pengaturan & kunci tersimpan di %APPDATA%\NOX tidak dihapus.)" -ForegroundColor DarkGray
    exit 0
}

function Resolve-SourceBinDir {
    param([string]$Requested)
    if ($Requested -and (Test-Path (Join-Path $Requested "nox.exe"))) {
        return (Resolve-Path $Requested).Path
    }
    $candidates = @(
        (Split-Path -Parent $PSScriptRoot),
        $PSScriptRoot,
        (Join-Path $PSScriptRoot "..\target\release")
    )
    foreach ($dir in $candidates) {
        if ($dir -and (Test-Path (Join-Path $dir "nox.exe"))) {
            return (Resolve-Path $dir).Path
        }
    }
    throw "nox.exe tidak ditemukan. Pastikan file nox.exe berada di folder rilis atau jalankan 'cargo build --release --workspace'."
}

Write-Host ""
Write-Host "=== NOX Installer (WinRT Native Notifications) ===" -ForegroundColor Cyan
$srcBin = Resolve-SourceBinDir -Requested $BinDir
$srcPkgDir = $PSScriptRoot

$targetBin = $srcBin
$targetPkgDir = $srcPkgDir

if ($InstallToAppData) {
    Write-Host "Menyalin NOX ke $InstallRoot ..." -ForegroundColor DarkGray
    Stop-NoxProcesses
    $targetPkgDir = Join-Path $InstallRoot "packaging"
    New-Item -ItemType Directory -Path $targetPkgDir -Force | Out-Null

    # Clean up legacy abra binaries if upgrading in place
    foreach ($old in @("abra.exe", "abra-guard.exe", "packaging\abra.png")) {
        $p = Join-Path $InstallRoot $old
        if (Test-Path $p) { Remove-Item $p -Force -ErrorAction SilentlyContinue }
    }

    Copy-Item (Join-Path $srcBin "nox.exe") (Join-Path $InstallRoot "nox.exe") -Force
    if (Test-Path (Join-Path $srcBin "nox-guard.exe")) {
        Copy-Item (Join-Path $srcBin "nox-guard.exe") (Join-Path $InstallRoot "nox-guard.exe") -Force
    }
    foreach ($f in @("AppxManifest.xml", "nox.png", "register-sparse.ps1", "NOX.msix", "NOX.cer")) {
        $srcFile = Join-Path $srcPkgDir $f
        if (Test-Path $srcFile) {
            Copy-Item $srcFile (Join-Path $targetPkgDir $f) -Force
        }
    }
    foreach ($cmd in @("Install-NOX.cmd", "Uninstall-NOX.cmd")) {
        $srcCmd = Join-Path $srcBin $cmd
        if (-not (Test-Path $srcCmd)) {
            $srcCmd = Join-Path $srcPkgDir $cmd
        }
        if (Test-Path $srcCmd) {
            Copy-Item $srcCmd (Join-Path $InstallRoot $cmd) -Force
        }
    }
    $targetBin = $InstallRoot
}

$manifest = Join-Path $targetPkgDir "AppxManifest.xml"
$msixPath = Join-Path $targetPkgDir "NOX.msix"
$cerPath = Join-Path $targetPkgDir "NOX.cer"

if (-not (Test-Path (Join-Path $targetPkgDir "nox.png"))) {
    $fallbackLogo = Join-Path $PSScriptRoot "..\assets\logo-rounded.png"
    if (Test-Path $fallbackLogo) {
        Copy-Item $fallbackLogo (Join-Path $targetPkgDir "nox.png") -Force
    }
}

$registered = $false

# Path A: Signed .msix package (no Developer Mode required once certificate is trusted)
if ((Test-Path $msixPath) -and (Test-Path $cerPath)) {
    Write-Host "Mendeteksi paket MSIX bertanda tangan (NOX.msix)..." -ForegroundColor Cyan
    $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($cerPath)
    $alreadyTrusted = Get-ChildItem Cert:\LocalMachine\Root -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $cert.Thumbprint }

    if (-not $alreadyTrusted) {
        Write-Host "Memasang sertifikat NOX agar Windows mempercayai paket (1x konfirmasi UAC)..." -ForegroundColor Yellow
        $importCmd = "Import-Certificate -FilePath '$cerPath' -CertStoreLocation Cert:\LocalMachine\Root | Out-Null"
        $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList "-NoProfile -ExecutionPolicy Bypass -Command `"$importCmd`""
        if ($p.ExitCode -ne 0) {
            Write-Host "Pemasangan sertifikat dibatalkan." -ForegroundColor Yellow
        }
    }

    try {
        Stop-NoxProcesses
        Remove-NoxPackage
        Add-AppxPackage -Path $msixPath -ExternalLocation $targetBin
        $registered = $true
    } catch {
        Write-Host "Registrasi NOX.msix gagal: $($_.Exception.Message)" -ForegroundColor Yellow
        Write-Host "Mencoba fallback registrasi manifest..." -ForegroundColor DarkGray
    }
}

# Path B: Loose AppxManifest.xml registration (requires Developer Mode)
if (-not $registered) {
    $devKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock"
    $devMode = (Get-ItemProperty -Path $devKey -Name "AllowDevelopmentWithoutDevLicense" -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense

    if ($devMode -ne 1) {
        Write-Host ""
        Write-Host "Windows Developer Mode belum aktif (dibutuhkan untuk mendaftarkan paket unsigned)." -ForegroundColor Yellow
        Write-Host "Pilih opsi:"
        Write-Host "  [1] Aktifkan Developer Mode otomatis & daftarkan NOX WinRT Native (1x UAC)"
        Write-Host "  [2] Buka Settings -> For developers secara manual"
        Write-Host "  [3] Langsung jalankan NOX tanpa registrasi (Mode Zero-Config UIA)"
        Write-Host ""
        $choice = Read-Host "Pilihan kamu [1/2/3] (default 1)"
        if ([string]::IsNullOrWhiteSpace($choice)) { $choice = "1" }

        if ($choice -eq "1") {
            Write-Host "Meminta izin Administrator (UAC) untuk mengaktifkan Developer Mode..." -ForegroundColor Cyan
            $enableCmd = "New-Item -Path '$devKey' -Force | Out-Null; Set-ItemProperty -Path '$devKey' -Name 'AllowDevelopmentWithoutDevLicense' -Type DWord -Value 1"
            $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList "-NoProfile -ExecutionPolicy Bypass -Command `"$enableCmd`""
            $devMode = (Get-ItemProperty -Path $devKey -Name "AllowDevelopmentWithoutDevLicense" -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense
            if ($devMode -ne 1) {
                Write-Host "Developer Mode belum aktif. Menjalankan NOX dalam Mode Zero-Config..." -ForegroundColor Yellow
            }
        } elseif ($choice -eq "2") {
            Start-Process "ms-settings:developers"
            Write-Host "Aktifkan 'Developer Mode', lalu jalankan ulang Install-NOX.cmd." -ForegroundColor Yellow
            exit 0
        }
    }

    if ($devMode -eq 1) {
        Write-Host "Mendaftarkan Sparse Package Identity NOX..." -ForegroundColor Cyan
        Write-Host "  Manifest : $manifest" -ForegroundColor DarkGray
        Write-Host "  Folder   : $targetBin" -ForegroundColor DarkGray
        Stop-NoxProcesses
        Remove-NoxPackage
        Add-AppxPackage -Register $manifest -ExternalLocation $targetBin
        $registered = $true
    }
}

if ($registered) {
    $pkg = Get-AppxPackage -Name $PkgName | Select-Object -First 1
    Write-Host ""
    Write-Host "Berhasil! NOX terdaftar sebagai aplikasi native Windows (WinRT)." -ForegroundColor Green
    if ($pkg) {
        Write-Host "  Package : $($pkg.PackageFamilyName)" -ForegroundColor DarkGray
    }
    if (-not $NoLaunch) {
        Write-Host "Menjalankan NOX..." -ForegroundColor Cyan
        if ($pkg) {
            Start-Process "explorer.exe" -ArgumentList "shell:AppsFolder\$($pkg.PackageFamilyName)!$AppId"
        } else {
            Start-Process (Join-Path $targetBin "nox.exe")
        }
    }
} else {
    Write-Host ""
    Write-Host "Menjalankan NOX dalam Mode Zero-Config (UI Automation)..." -ForegroundColor Cyan
    if (-not $NoLaunch) {
        Start-Process (Join-Path $targetBin "nox.exe")
    }
}
