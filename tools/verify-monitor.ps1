# verify-monitor.ps1 — 验证「服务失联检测 + 自动重新接管」
#
# 复现用户场景（用**自动挑选的空闲端口**，不干扰正在使用的会话）：
#   1. 手动启动外部 dsh web（模拟用户在 CMD 里启动）
#   2. 启动器接管它
#   3. 杀掉外部 dsh（模拟用户关掉 CMD → Windows 给控制台进程发 CTRL_CLOSE_EVENT）
#   4. 验证启动器在约 12 秒内检测到失联并记录
#   5. 重新手动启动 dsh
#   6. 验证启动器自动重新接管并让界面重连

param(
    # 0 = 自动挑一个空闲端口。
    #
    # 旧默认写死 3099，而 3099 可能**正是用户当前会话使用的端口**（本机实测踩到）：
    # 脚本会去杀那个端口的所有者、并把用户活会话卷进测试，同时自己拉起的 dsh
    # 因为绑不上端口而从未参与测试 —— 后续每一步都变成误导性的 FAIL。
    [int]$Port = 0
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
# 本脚本位于 tools\ 下，仓库根是其父目录
$here = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $here 'Cargo.toml'))) { $here = $PSScriptRoot }
Set-Location $here

$TASKKILL = Join-Path $env:WINDIR 'System32\taskkill.exe'
$node = (Get-Command node).Source
$binJs = Join-Path $env:APPDATA 'npm\node_modules\@deepseek-ai\dsh\lib\bin.js'
$log = Join-Path $env:LOCALAPPDATA 'DSHLauncher\logs\launcher.log'
$cfg = Join-Path $env:APPDATA 'DSHLauncher\settings.toml'

function Section($t) { Write-Host ""; Write-Host ("=" * 60) -ForegroundColor DarkGray; Write-Host " $t" -ForegroundColor Cyan; Write-Host ("=" * 60) -ForegroundColor DarkGray }
function Start-Dsh([int]$p) {
    Start-Process -FilePath $node -ArgumentList @($binJs, 'web', '--host', '127.0.0.1', '--port', $p) -WindowStyle Hidden -PassThru
}
function Wait-Port([int]$p, [int]$sec) {
    $deadline = (Get-Date).AddSeconds($sec)
    while ((Get-Date) -lt $deadline) {
        if (Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}
function Mark() { if (Test-Path $log) { (Get-Content $log | Measure-Object -Line).Lines } else { 0 } }
function NewLines($m) { if (Test-Path $log) { Get-Content $log | Select-Object -Skip $m } else { @() } }

# 挑一个真正空闲的端口：绑定 0 号端口由系统分配，读出后立刻释放。
function Get-FreePort {
    $listener = New-Object System.Net.Sockets.TcpListener -ArgumentList @([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $free = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    return $free
}

# 端口上监听的必须**正是**本测试刚启动的那个进程。
#
# 旧实现只检查"端口在听"：当外部 dsh 因端口被占而绑不上时，它会误判为"就绪"，
# 于是后续「杀外部 dsh → 端口仍被占用 → 检测不到失联 → 未重新接管」全是**误导性**结果
# （本机实测：三次 FAIL 其实都源自这个前提没校验）。
function Assert-Owner([int]$p, [int]$expectPid, [string]$what) {
    $rows = @(Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue)
    $owners = @($rows.OwningProcess | Select-Object -Unique)
    if ($owners.Count -eq 0) { throw "$what 未监听端口 $p（端口上没有监听者）" }
    if ($owners -notcontains $expectPid) {
        throw ("$what（PID $expectPid）不是端口 $p 的监听者（实际监听者 PID $($owners -join ',')）—— " +
               "本测试需要独占该端口，否则后续断言全部无效。")
    }
}

$failed = @()
$cleanup = @()

# --- 用户配置保护（P0）-----------------------------------------------------
# $cfg 是**用户的真实配置**（%APPDATA%\DSHLauncher\settings.toml），本脚本为了
# 在独立端口上做实验必须临时改写它。因此：
#   * 写入前先记录「原本是否存在」与「原始字节」；
#   * 无论成功、异常还是被取消，finally 里都必须还原；
#   * 原本不存在的文件必须被删除（本脚本只删自己创建的文件）。
$existed = Test-Path $cfg
$backup = if ($existed) { Get-Content $cfg -Raw } else { $null }
$cfgDir = Split-Path -Parent $cfg
$createdDir = $false
if (-not $existed -and $cfgDir -and -not (Test-Path $cfgDir)) {
    New-Item -ItemType Directory -Path $cfgDir -Force | Out-Null
    $createdDir = $true
}

try {
    if ($Port -le 0) { $Port = Get-FreePort }
    Section "准备：清理旧实例并写入测试配置（端口 $Port）"
    if ($existed) { Write-Host "  已备份用户配置（$($backup.Length) 字符），测试结束后原样还原" -ForegroundColor DarkGray }
    else { Write-Host "  用户配置原本不存在，测试结束后删除本脚本创建的文件" -ForegroundColor DarkGray }
    Get-Process -Name DSHLauncher, dsh-app -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host "  结束正在运行的启动器实例（PID $($_.Id)）—— dsh 服务是独立进程，不受影响" -ForegroundColor DarkGray
        & $TASKKILL /F /PID $_.Id 2>&1 | Out-Null
    }
    Start-Sleep -Seconds 1

    # 前置条件：端口必须空闲。
    #
    # 旧实现会 `taskkill` 掉**该端口当前的任意所有者** —— 如果那正是用户正在使用的
    # 会话（本机实测：默认端口 3099 就是），既可能打断用户，又会让测试永远测不到
    # 自己拉起的进程。现在的规则：被占用就明确报错退出，绝不结束不是本测试启动的进程。
    $busy = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    if ($busy.Count -gt 0) {
        $ownerPid = @($busy.OwningProcess | Select-Object -Unique)
        $names = ($ownerPid | ForEach-Object { (Get-Process -Id $_ -ErrorAction SilentlyContinue).ProcessName }) -join ', '
        throw ("端口 $Port 已被占用（PID $($ownerPid -join ',') $names）。本测试需要独占一个空闲端口，" +
               "且**不会**结束不是它启动的进程（那可能是用户正在使用的会话）。请改用 -Port <空闲端口>。")
    }
    Set-Content -Path $cfg -Value "port = $Port`n" -Encoding UTF8
    Write-Host "  端口 $Port 空闲；已写入 port = $Port"

    Section "步骤 1：手动启动外部 dsh web（模拟用户在 CMD 里启动）"
    $dsh1 = Start-Dsh $Port
    $cleanup += $dsh1.Id
    if (-not (Wait-Port $Port 120)) { throw "外部 dsh 未在 120s 内就绪" }
    Assert-Owner $Port $dsh1.Id '外部 dsh'
    Write-Host "  ✓ 外部 dsh 就绪（PID $($dsh1.Id)）"

    Section "步骤 2：启动启动器并接管"
    $m = Mark
    $app = Start-Process -FilePath (Join-Path $here 'target\release\dsh-app.exe') -PassThru
    $cleanup += $app.Id
    Start-Sleep -Seconds 8
    $t2 = (NewLines $m) -join "`n"
    # 判据必须与产品**实际**输出的措辞对齐：
    #   * 分支 2（service.json 对账接管）："…已直接接管（会话未中断）。"
    #   * 分支 3（端口上有 dsh 但无有效簿记）："端口 P 上已有 dsh 服务在运行（PID …），直接接管。"
    # 旧模式 '已接管端口' 在产品里**已不存在**，等于无论环境如何都判失败
    # （同类措辞误报此前已在 tools/verify-token-navigation.ps1 修过，本脚本漏了）。
    if ($t2 -match '直接接管') { Write-Host "  PASS: 已接管外部实例 ✓" -ForegroundColor Green } else { Write-Host "  FAIL: 未见接管日志" -ForegroundColor Red; $failed += 'adopt' }

    Section "步骤 3：杀掉外部 dsh（模拟关闭 CMD → CTRL_CLOSE_EVENT 使 dsh 退出）"
    & $TASKKILL /F /PID $dsh1.Id 2>&1 | Out-Null
    Start-Sleep -Seconds 2
    if (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue) { Write-Host "  ⚠ 端口仍被占用" } else { Write-Host "  ✓ 外部 dsh 已退出，端口释放" }

    Section "步骤 4：等待启动器检测到失联（阈值约 12 秒）"
    $wait = 0
    $detected = $false
    while ($wait -lt 30) {
        Start-Sleep -Seconds 1; $wait++
        if (((NewLines $m) -join "`n") -match '已失联') { $detected = $true; break }
        Write-Host "    ...已等待 $wait 秒"
    }
    if ($detected) { Write-Host "  PASS: 检测到失联（约 $wait 秒）✓" -ForegroundColor Green } else { Write-Host "  FAIL: 30 秒内未检测到失联" -ForegroundColor Red; $failed += 'detect' }

    Section "步骤 5：用户重新手动启动 dsh"
    $dsh2 = Start-Dsh $Port
    $cleanup += $dsh2.Id
    if (-not (Wait-Port $Port 120)) { throw "新 dsh 未就绪" }
    Assert-Owner $Port $dsh2.Id '新 dsh'
    Write-Host "  ✓ 新 dsh 就绪（PID $($dsh2.Id)）"

    Section "步骤 6：验证自动重新接管 + 界面重连"
    $wait = 0
    $readopted = $false
    while ($wait -lt 30) {
        Start-Sleep -Seconds 1; $wait++
        if (((NewLines $m) -join "`n") -match '已重新接管') { $readopted = $true; break }
    }
    if ($readopted) { Write-Host "  PASS: 自动重新接管（约 $wait 秒）✓" -ForegroundColor Green } else { Write-Host "  FAIL: 未自动重新接管" -ForegroundColor Red; $failed += 'readopt' }

    Section "本次完整日志"
    NewLines $m | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
}
finally {
    Section "清理"
    foreach ($p in $cleanup) { & $TASKKILL /F /PID $p 2>&1 | Out-Null }
    # 只结束**本测试启动过**的进程。端口上若还留着别人的监听者（用户自己的会话、
    # 别的启动器拉起的服务），一律不动 —— 旧实现会杀掉端口的所有者，
    # 而旧默认端口 3099 恰好就是用户会话端口（本机实测）。
    $left = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    if ($left.Count -gt 0) {
        $owners = @($left.OwningProcess | Select-Object -Unique)
        foreach ($p in @($owners | Where-Object { $cleanup -contains $_ })) { & $TASKKILL /F /PID $p 2>&1 | Out-Null }
        $others = @($owners | Where-Object { $cleanup -notcontains $_ })
        if ($others.Count -gt 0) {
            Write-Host "  ⚠ 端口 $Port 上仍有不属于本测试的监听进程（PID $($others -join ',')），已保留不动" -ForegroundColor Yellow
        }
    }

    # --- 还原用户配置（必须在本 finally 中，异常/取消路径同样会执行）---
    if ($existed) {
        try {
            # 写回精确的原始字节（-NoNewline 避免追加换行破坏内容）
            Set-Content -Path $cfg -Value $backup -NoNewline -Encoding UTF8
            $now = Get-Content $cfg -Raw
            if ($now -eq $backup) {
                Write-Host "  ✓ 用户配置已原样还原（$cfg）" -ForegroundColor Green
            } else {
                Write-Host "  ⚠ 用户配置还原后内容不一致，请检查 $cfg" -ForegroundColor Yellow
                $failed += 'config-restore-mismatch'
            }
        } catch {
            Write-Host "  ❌ 还原用户配置失败：$($_.Exception.Message)" -ForegroundColor Red
            Write-Host "     原始内容已丢失，备份如下（请手工写回 $cfg）：" -ForegroundColor Yellow
            Write-Host $backup -ForegroundColor DarkGray
            $failed += 'config-restore-failed'
        }
    } else {
        # 本脚本创建的文件必须由本脚本删除；不要删除别人的配置
        if (Test-Path $cfg) { Remove-Item $cfg -Force -ErrorAction SilentlyContinue }
        if ($createdDir -and (Test-Path $cfgDir) -and -not (Get-ChildItem $cfgDir -Force -ErrorAction SilentlyContinue)) {
            Remove-Item $cfgDir -Force -ErrorAction SilentlyContinue
        }
        Write-Host "  ✓ 已移除本脚本创建的测试配置（$cfg）" -ForegroundColor Green
    }
    Write-Host "  已清理测试进程与配置"
}

Section "结果"
if ($failed.Count -eq 0) { Write-Host "  ✅ 全部通过" -ForegroundColor Green; exit 0 }
else { Write-Host "  ❌ 失败项: $($failed -join ', ')" -ForegroundColor Red; exit 1 }
