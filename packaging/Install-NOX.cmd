@echo off
setlocal
title NOX Installer - Native Windows Setup

set "SCRIPT_DIR=%~dp0"
if exist "%SCRIPT_DIR%packaging\register-sparse.ps1" (
    set "PS_SCRIPT=%SCRIPT_DIR%packaging\register-sparse.ps1"
    set "INSTALL_FLAG=-InstallToAppData"
) else (
    set "PS_SCRIPT=%SCRIPT_DIR%register-sparse.ps1"
    set "INSTALL_FLAG="
)

if not exist "%PS_SCRIPT%" (
    echo [ERROR] register-sparse.ps1 tidak ditemukan.
    pause
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %INSTALL_FLAG%
if errorlevel 1 (
    echo.
    echo Instalasi mengalami kendala. Lihat pesan di atas.
    pause
    exit /b 1
)

timeout /t 2 /nobreak >nul
endlocal
