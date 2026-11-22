@echo off
setlocal
title Uninstall NOX

set "SCRIPT_DIR=%~dp0"
if exist "%SCRIPT_DIR%packaging\register-sparse.ps1" (
    set "PS_SCRIPT=%SCRIPT_DIR%packaging\register-sparse.ps1"
) else (
    set "PS_SCRIPT=%SCRIPT_DIR%register-sparse.ps1"
)

if not exist "%PS_SCRIPT%" (
    echo [ERROR] register-sparse.ps1 tidak ditemukan.
    pause
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" -Uninstall
echo.
pause
endlocal
