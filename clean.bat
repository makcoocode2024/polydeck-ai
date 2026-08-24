@echo off
setlocal
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\clean.ps1"
if %ERRORLEVEL% neq 0 (
    echo.
    echo [PolyDeck] Clean exited with code %ERRORLEVEL%.
    pause
)
