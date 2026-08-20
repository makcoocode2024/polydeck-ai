[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
    $Host.UI.RawUI.WindowTitle = "PolyDeck - 开发者模式启动"
} catch {}

Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "       PolyDeck v2.0 - 开发者模式一键启动器" -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Cyan
Write-Host ""

# 1. 检查 Node.js 与 Cargo
$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) {
    Write-Host "[错误] 未检测到 Node.js，请先安装 Node.js 18+ 环境！" -ForegroundColor Red
    Read-Host "按回车键退出..."
    exit 1
}

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    Write-Host "[错误] 未检测到 Rust/Cargo，请先安装 Rust 环境 (rustup)！" -ForegroundColor Red
    Read-Host "按回车键退出..."
    exit 1
}

# 2. 清理残留进程与端口 (1420, 18888, polydeck.exe)
Get-Process -Name '*polydeck*' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Write-Host "[1/3] 检查端口 1420 状态..." -ForegroundColor Yellow
$conn = Get-NetTCPConnection -LocalPort 1420 -ErrorAction SilentlyContinue
if ($conn) {
    $pids = $conn | Select-Object -ExpandProperty OwningProcess -Unique
    foreach ($p in $pids) {
        if ($p -gt 0) {
            try {
                Stop-Process -Id $p -Force -ErrorAction SilentlyContinue
                Write-Host "  -> 已清理占用 1420 端口的残留进程 (PID: $p)" -ForegroundColor Yellow
            } catch {}
        }
    }
} else {
    Write-Host "  -> 端口 1420 处于空闲状态。" -ForegroundColor Green
}

# 3. 检查依赖
if (-not (Test-Path "node_modules")) {
    Write-Host "[2/3] 检测到首次运行，正在安装 npm 前端依赖..." -ForegroundColor Yellow
    npm.cmd install
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[错误] npm install 安装依赖失败！" -ForegroundColor Red
        Read-Host "按回车键退出..."
        exit 1
    }
} else {
    Write-Host "[2/3] 前端依赖已就绪。" -ForegroundColor Green
}

# 4. 启动 Tauri 开发模式
Write-Host "[3/3] 正在启动 PolyDeck 开发者模式 (Tauri 2 + Vite HMR)..." -ForegroundColor Cyan
Write-Host "--------------------------------------------------------" -ForegroundColor DarkGray

npm.cmd run tauri dev

if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "[提示] 应用程序已退出 (退出码: $LASTEXITCODE)。" -ForegroundColor Yellow
    Read-Host "按回车键退出..."
}
