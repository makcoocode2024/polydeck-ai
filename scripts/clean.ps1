[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
    $Host.UI.RawUI.WindowTitle = "PolyDeck - 构建产物清理"
} catch {}

$Root = Split-Path -Parent $PSScriptRoot
$Target = Join-Path $Root 'target'

function Get-DirSizeGB {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return 0 }
    # rust-analyzer runs cargo check in the background, so files can vanish
    # mid-walk. That surfaces as a Win32Exception which -ErrorAction does not
    # suppress, hence the redirect and the catch.
    $sum = 0
    try {
        $sum = (Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction SilentlyContinue 2>$null |
                Measure-Object -Sum Length).Sum
    } catch {
        $sum = 0
    }
    if (-not $sum) { return 0 }
    return [math]::Round($sum / 1GB, 2)
}

function Show-Breakdown {
    $total = Get-DirSizeGB $Target
    Write-Host ""
    Write-Host "  target/ 合计: $total GB" -ForegroundColor Cyan
    if ($total -eq 0) { return }
    foreach ($sub in @('debug\incremental', 'debug\deps', 'debug\build', 'debug', 'release')) {
        $p = Join-Path $Target $sub
        if (Test-Path $p) {
            $g = Get-DirSizeGB $p
            Write-Host ("    {0,-20} {1,6} GB" -f $sub, $g) -ForegroundColor DarkGray
        }
    }
}

function Stop-PolyDeck {
    $procs = Get-Process -ErrorAction SilentlyContinue |
             Where-Object { $_.Name -match '^(polydeck|ai-deck)$' }
    if (-not $procs) { return $true }
    Write-Host ""
    Write-Host "  [警告] PolyDeck 正在运行，会锁住 target 里的 .exe，导致清理失败:" -ForegroundColor Yellow
    foreach ($p in $procs) {
        Write-Host ("    PID {0}  {1}" -f $p.Id, $p.Path) -ForegroundColor Yellow
    }
    Write-Host "  强制结束会丢失应用内未保存的状态（代码和配置不受影响）。" -ForegroundColor Yellow
    $ans = Read-Host "  结束这些进程? 输入 y 确认，其他键取消"
    if ($ans -ne 'y') {
        Write-Host "  已取消。请手动退出应用后重新运行。" -ForegroundColor DarkGray
        return $false
    }
    $procs | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800
    return $true
}

Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "       PolyDeck - 构建产物清理" -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Cyan

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    Write-Host "[错误] 未检测到 Rust/Cargo，请先安装 Rust 环境 (rustup)！" -ForegroundColor Red
    Read-Host "按回车键退出..."
    exit 1
}

$before = Get-DirSizeGB $Target
if ($before -eq 0) {
    Write-Host ""
    Write-Host "  target/ 已是空的，无需清理。" -ForegroundColor Green
    Read-Host "按回车键退出..."
    exit 0
}

Show-Breakdown

$gInc = Get-DirSizeGB (Join-Path $Target 'debug\incremental')
$gDbg = Get-DirSizeGB (Join-Path $Target 'debug')

Write-Host ""
Write-Host ("  [1] 只删增量缓存 incremental/   回收 {0} GB  —— 保留已编译依赖，下次全量编译一次" -f $gInc) -ForegroundColor White
Write-Host ("  [2] 清 dev 产物 (整个 debug/)   回收 {0} GB  —— 保留 release/ 里可运行的成品" -f $gDbg) -ForegroundColor White
Write-Host ("  [3] 全清 (cargo clean)          回收 {0} GB  —— debug + release 都要重新编译" -f $before) -ForegroundColor White
Write-Host "  [0] 退出" -ForegroundColor DarkGray
Write-Host ""
$choice = Read-Host "  选择"

if ($choice -eq '0' -or $choice -eq '') {
    Write-Host "  已退出。" -ForegroundColor DarkGray
    exit 0
}
if ($choice -notin @('1', '2', '3')) {
    Write-Host "  无效选择，已退出。" -ForegroundColor Red
    Read-Host "按回车键退出..."
    exit 1
}

if (-not (Stop-PolyDeck)) {
    Read-Host "按回车键退出..."
    exit 1
}

Write-Host ""
Push-Location $Root
try {
    switch ($choice) {
        '1' {
            $inc = Join-Path $Target 'debug\incremental'
            if (Test-Path $inc) {
                Write-Host "  正在删除 debug\incremental ..." -ForegroundColor Yellow
                Remove-Item -Recurse -Force $inc -ErrorAction SilentlyContinue
            } else {
                Write-Host "  debug\incremental 不存在，跳过。" -ForegroundColor DarkGray
            }
        }
        '2' {
            Write-Host "  正在执行 cargo clean --profile dev ..." -ForegroundColor Yellow
            cargo clean --profile dev
        }
        '3' {
            Write-Host "  正在执行 cargo clean ..." -ForegroundColor Yellow
            cargo clean
        }
    }
} finally {
    Pop-Location
}

$after = Get-DirSizeGB $Target
$freed = [math]::Round($before - $after, 2)

Write-Host ""
Write-Host "--------------------------------------------------------" -ForegroundColor DarkGray
Write-Host ("  清理前: {0} GB   清理后: {1} GB   回收: {2} GB" -f $before, $after, $freed) -ForegroundColor Green
Show-Breakdown

$stale = @()
if (Test-Path $Target) {
    try {
        $stale = Get-ChildItem -LiteralPath $Target -Recurse -Force -File -ErrorAction SilentlyContinue 2>$null |
                 Where-Object { $_.Name -match 'ai[_-]deck' }
    } catch {
        $stale = @()
    }
}
if ($stale.Count -gt 0) {
    $sgb = [math]::Round(($stale | Measure-Object -Sum Length).Sum / 1MB, 1)
    Write-Host ""
    Write-Host ("  另有 {0} 个改名前的 ai_deck 残留文件 ({1} MB)，已不会被复用。" -f $stale.Count, $sgb) -ForegroundColor Yellow
    $ans = Read-Host "  删除它们? 输入 y 确认，其他键跳过"
    if ($ans -eq 'y') {
        $stale | Remove-Item -Force -ErrorAction SilentlyContinue
        Write-Host ("  已删除，target/ 现为 {0} GB" -f (Get-DirSizeGB $Target)) -ForegroundColor Green
    }
}

Write-Host ""
Read-Host "按回车键退出..."

