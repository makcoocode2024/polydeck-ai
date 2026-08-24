@echo off
setlocal
rem This file is UTF-8. Without 65001 the Chinese below renders as mojibake on a
rem default zh-CN console (code page 936).
chcp 65001 >nul
cd /d "%~dp0"

set "EXE=%~dp0target\release\polydeck.exe"

if not exist "%EXE%" (
    echo [PolyDeck] 未找到 release 构建: %EXE%
    echo [PolyDeck] 请先执行 build_release.bat 生成生产构建。
    pause
    exit /b 1
)

taskkill /f /im polydeck.exe >nul 2>&1
echo [PolyDeck] 正在启动最新生产构建版本...
start "" "%EXE%"
