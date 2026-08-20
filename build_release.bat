@echo off
setlocal
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build_release.ps1"
if %ERRORLEVEL% neq 0 (
    echo.
    echo [PolyDeck] Build process exited with code %ERRORLEVEL%.
    pause
)