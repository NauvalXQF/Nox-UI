# Registers ABRA as a Windows 10/11 Sparse Package so WinRT UserNotificationListener
# has Package Identity while running target\release\abra.exe directly from disk.
# Usage (run in PowerShell):
#   .\packaging\register-sparse.ps1
# To unregister later:
#   Get-AppxPackage NauvalXQF.AbraShell | Remove-AppxPackage

param(
    [string]$BinDir = (Join-Path $PSScriptRoot "..\target\release")
)

$ErrorActionPreference = "Stop"
$manifest = Resolve-Path (Join-Path $PSScriptRoot "AppxManifest.xml")
$external = Resolve-Path $BinDir

$devMode = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" -Name "AllowDevelopmentWithoutDevLicense" -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense
if ($devMode -ne 1) {
    Write-Host ""
    Write-Host "Windows Developer Mode belum aktif (dibutuhkan untuk mendaftarkan paket unsigned)." -ForegroundColor Yellow
    Write-Host "Cara mengaktifkan:"
    Write-Host "  1. Buka Settings -> System -> For developers (atau jalankan: start ms-settings:developers)"
    Write-Host "  2. Aktifkan 'Developer Mode' (Mode Pengembang)"
    Write-Host "  3. Jalankan ulang .\packaging\register-sparse.ps1"
    Write-Host ""
    exit 1
}

if (-not (Test-Path (Join-Path $external "abra.exe"))) {
    Write-Error "abra.exe tidak ditemukan di $external. Jalankan 'cargo build --release --workspace' terlebih dahulu."
    exit 1
}

if (-not (Test-Path (Join-Path $PSScriptRoot "abra.png"))) {
    $fallbackLogo = Join-Path $PSScriptRoot "..\assets\logo-rounded.png"
    if (Test-Path $fallbackLogo) {
        Copy-Item $fallbackLogo (Join-Path $PSScriptRoot "abra.png") -Force
    }
}

Write-Host "Mendaftarkan Sparse Package Identity ABRA..."
Write-Host "  Manifest : $manifest"
Write-Host "  External : $external"

Add-AppxPackage -Register $manifest -ExternalLocation $external
$pkg = Get-AppxPackage -Name "NauvalXQF.AbraShell" | Select-Object -First 1
if ($pkg) {
    Write-Host ""
    Write-Host "Selesai! PackageFamilyName: $($pkg.PackageFamilyName)" -ForegroundColor Green
    Write-Host "Jalankan ABRA dari Start Menu ('ABRA') atau dengan perintah:"
    Write-Host "  explorer.exe `"shell:AppsFolder\$($pkg.PackageFamilyName)!Abra`""
} else {
    Write-Host "Selesai! Jalankan ulang ABRA melalui Start Menu ('ABRA') agar UserNotificationListener aktif."
}
