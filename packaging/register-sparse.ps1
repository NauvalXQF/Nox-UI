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

if (-not (Test-Path (Join-Path $external "abra.exe"))) {
    Write-Error "abra.exe tidak ditemukan di $external. Jalankan 'cargo build --release --workspace' terlebih dahulu."
    exit 1
}

Write-Host "Mendaftarkan Sparse Package Identity ABRA..."
Write-Host "  Manifest : $manifest"
Write-Host "  External : $external"

Add-AppxPackage -Register $manifest -ExternalLocation $external
Write-Host "Selesai! Jalankan ulang ABRA melalui Start Menu ('ABRA') atau 'shell:AppsFolder\NauvalXQF.AbraShell_*!Abra' agar UserNotificationListener aktif."
