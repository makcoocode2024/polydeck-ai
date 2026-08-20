@echo off
setlocal
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\dev.ps1"
if %ERRORLEVEL% neq 0 (
    echo.
    echo [PolyDeck] Process exited with code %ERRORLEVEL%.
    pause
)