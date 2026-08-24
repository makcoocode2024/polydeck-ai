# Pre-flight check before the Agnes diagnostic run.
#
# Verifies the preconditions that, if missed, waste a whole run:
#   1. gateway is listening on 18888
#   2. upstream is Agnes (not sotamodel or another provider)
#   3. RUST_LOG debug is active, so the index-tracking instrumentation reaches
#      the log
#
# Precondition 2 can only be confirmed once at least one request has gone
# through, so this sends a probe request itself rather than waiting for traffic.

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$log = "$env:USERPROFILE\.ai-deck\logs\gateway.2026-08-24.log"
$ok = $true

Write-Host ""
Write-Host "=== Agnes 运行前自检 ===" -ForegroundColor Cyan
Write-Host ""

# 1. process + port
$proc = Get-Process polydeck -ErrorAction SilentlyContinue
if (-not $proc) {
    Write-Host "[X] polydeck.exe 没在运行" -ForegroundColor Red
    $ok = $false
} else {
    Write-Host ("[OK] polydeck.exe 运行中 (PID {0})" -f $proc.Id) -ForegroundColor Green
}

$listen = (Get-NetTCPConnection -LocalPort 18888 -State Listen -ErrorAction SilentlyContinue | Measure-Object).Count
if ($listen -lt 1) {
    Write-Host "[X] 18888 没有监听 -> 在 UI 里启用 gateway" -ForegroundColor Red
    $ok = $false
} else {
    Write-Host "[OK] gateway 监听 18888" -ForegroundColor Green
}

# 2 + 3. probe request, then read what the log recorded about it
if ($ok) {
    $before = 0
    if (Test-Path $log) { $before = (Get-Content $log -ErrorAction SilentlyContinue | Measure-Object -Line).Lines }

    $body = @{
        model      = "claude-sonnet-5"
        max_tokens = 2048
        stream     = $true
        messages   = @(@{ role = "user"; content = "hi" })
    } | ConvertTo-Json -Depth 5 -Compress

    Write-Host "[..] 发一个探针请求" -ForegroundColor Yellow
    try {
        Invoke-WebRequest -Uri "http://127.0.0.1:18888/v1/messages" -Method Post `
            -ContentType "application/json" -Body $body -TimeoutSec 45 `
            -UseBasicParsing -ErrorAction Stop | Out-Null
        Write-Host "[OK] 探针请求返回" -ForegroundColor Green
    } catch {
        Write-Host ("[!] 探针请求报错: {0}" -f $_.Exception.Message) -ForegroundColor Yellow
        Write-Host "    (不一定是问题, 继续看日志记了什么)" -ForegroundColor DarkGray
    }

    Start-Sleep -Seconds 2
    $new = @()
    if (Test-Path $log) {
        $all = Get-Content $log -ErrorAction SilentlyContinue
        if ($all.Count -gt $before) { $new = $all[$before..($all.Count - 1)] }
    }

    $rewrite = $new | Where-Object { $_ -match "Rewrote model" } | Select-Object -Last 1
    $upstream = $null
    if ($rewrite -match "->\s*([A-Za-z0-9._-]+)") { $upstream = $Matches[1] }

    if (-not $upstream) {
        # No rewrite rule fired; fall back to whatever model reached the log.
        Write-Host "[!] 日志里没有 'Rewrote model' 行" -ForegroundColor Yellow
        Write-Host "    可能该 profile 没配模型改写规则, 无法据此确认上游" -ForegroundColor DarkGray
    } elseif ($upstream -match "agnes") {
        Write-Host ("[OK] 上游是 Agnes ({0})" -f $upstream) -ForegroundColor Green
    } else {
        Write-Host ("[X] 上游不是 Agnes, 是 {0} -> profile 切错了" -f $upstream) -ForegroundColor Red
        $ok = $false
    }

    $dbg = ($new | Where-Object { $_ -match '"level":"DEBUG"' } | Measure-Object).Count
    if ($dbg -lt 1) {
        Write-Host "[X] 新日志里没有 DEBUG 行 -> RUST_LOG 没生效, 插桩不会记录" -ForegroundColor Red
        $ok = $false
    } else {
        Write-Host ("[OK] DEBUG 级别生效 (新增 {0} 行)" -f $dbg) -ForegroundColor Green
    }

    $trace = ($new | Where-Object { $_ -match "content_block indices" } | Measure-Object).Count
    if ($trace -lt 1) {
        Write-Host "[X] 没有 content_block indices 行 -> 插桩没生效或跑的是旧二进制" -ForegroundColor Red
        $ok = $false
    } else {
        Write-Host ("[OK] content_block 索引追踪在输出 ({0} 行)" -f $trace) -ForegroundColor Green
    }
}

Write-Host ""
if ($ok) {
    Write-Host "==> 全部通过, 可以跑诊断任务了" -ForegroundColor Green
} else {
    Write-Host "==> 有项目没通过, 先修上面标 [X] 的, 否则这一遍会白跑" -ForegroundColor Red
}
Write-Host ""
Read-Host "按回车关闭"
