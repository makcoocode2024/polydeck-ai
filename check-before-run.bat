@echo off
setlocal
rem UTF-8 so the Chinese output does not turn into mojibake on a zh-CN console.
chcp 65001 >nul
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\check-before-run.ps1"
rem Fallback pause: if the script itself fails to parse, its own Read-Host never
rem runs and the window would close before the error could be read.
if %ERRORLEVEL% neq 0 (
    echo.
    echo [自检脚本异常退出, code %ERRORLEVEL%] 上面的报错请截图给我
    pause
)
