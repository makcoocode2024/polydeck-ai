@echo off
setlocal
cd /d "%~dp0"
taskkill /f /im ai-deck.exe >nul 2>&1
echo [PolyDeck] 正在启动最新生产构建版本 (v2.0.2)...
start "" "%~dp0target\release\ai-deck.exe"
