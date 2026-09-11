# tools\verify-service-lifecycle.ps1 —— 验证「服务独立于启动器」是否真的成立
#
# 这是一个**破坏性**验证脚本：它会结束启动器（但不结束 dsh 服务），然后断言服务存活。
# 因此默认只做「只读检查」；真正做实验需要显式 -Force。
#
# 背景（v5.0.0 审计的核心缺陷）：
#   旧实现用 Job Object 的 KILL_ON_JOB_CLOSE 把 dsh 绑在启动器上，只有走托盘
#   「退出→否」这一条路径才会 disarm。于是强杀 / 崩溃 / 被安装包升级覆盖
#   都会切断正在进行的会话。默认已改为 independent（dsh 不挂 Job）。
#
# 用法:
#   pwsh -NoProfile -File tools\verify-service-lifecycle.ps1            # 只读检查
#   pwsh -NoProfile -File tools\verify-service-lifecycle.ps1 -Force      # 真跑实验
#
# 退出码: 0 = 通过；1 = 不通过；2 = SKIPPED（决定性实验未运行，只做了只读检查）
#
# 为什么 SKIPPED 必须是独立的退出码 2：本脚本唯一能**证伪**「服务独立于启动器」的
# 手段就是 C 段那次破坏性实验。跳过它却打印「结果：全部通过」并 exit 0，等于在
# 最关键的一项上给出假绿（P2-6）。只读检查全过也仍然只是 SKIPPED。

param(
    [switch]$Force,
    [int]$Port = 3080,
    [int]$QuitTimeoutSeconds = 15
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$AppExe = Join-Path $root 'target\release\dsh-app.exe'
$RootExe = Join-Path $root 'DSHLauncher.exe'
$Log = Join-Path $env:LOCALAPPDATA 'DSHLauncher\logs\launcher.log'
$Record = Join-Path $env:LOCALAPPDATA 'DSHLauncher\service.json'

$failed = New-Object System.Collections.Generic.List[string]
function Pass($m) { Write-Host "  [PASS] $m" -ForegroundColor Green }
function Fail($m) { Write-Host "  [FAIL] $m" -ForegroundColor Red; $script:failed.Add($m) }
function Info($m) { Write-Host "  $m" -ForegroundColor DarkGray }

# SKIPPED 追踪（P2-6）：决定性实验没跑 → 退出码 2，绝不报「全部通过」
$script:experimentRan = $false
$script:skipReasons = New-Object System.Collections.Generic.List[string]

# ---------------------------------------------------------------------------
# 优雅结束启动器（P1-10 + docs/OPS-RUNBOOK.md §1）
#
# 旧实现 `Get-Process DSHLauncher | Stop-Process -Force` 有两个问题：
#   1) 会杀掉装在本机**任何位置**的启动器实例；
#   2) 反复强杀 GUI 进程会耗尽桌面堆，最终让本机无法创建任何新进程（0xC0000142）。
# 因此：先按「可执行文件路径属于本仓库」过滤，再走项目自带的 `--quit` 优雅入口，
# 有界等待后才回退强杀，并记录实际走了哪条路径。
# ---------------------------------------------------------------------------
function Get-RepoLauncherProcesses {
    $rootFull = [System.IO.Path]::GetFullPath($root).TrimEnd('\')
    return @(Get-Process DSHLauncher -ErrorAction SilentlyContinue | Where-Object {
            if (-not $_.Path) { return $false }
            $pf = [System.IO.Path]::GetFullPath($_.Path)
            $pf.StartsWith($rootFull + '\', [System.StringComparison]::OrdinalIgnoreCase)
        })
}

function Stop-RepoLauncher {
    $targets = Get-RepoLauncherProcesses
    if ($targets.Count -eq 0) {
        Info '没有本仓库路径下的启动器进程（不触碰其他位置的实例）'
        return $false
    }
    Info ("待结束的启动器（均为本仓库路径）：" + (($targets | ForEach-Object { "$($_.Id)=$($_.Path)" }) -join ', '))

    # 1) 优先优雅退出
    if (Test-Path $RootExe) {
        Info ("优雅退出：{0} --quit（最多等待 {1}s）" -f $RootExe, $QuitTimeoutSeconds)
        try { Start-Process -FilePath $RootExe -ArgumentList '--quit' -WindowStyle Hidden | Out-Null }
        catch { Info ("优雅退出入口调用失败：{0}（将直接回退强杀）" -f $_.Exception.Message) }
    } else {
        Info "根目录无 DSHLauncher.exe，无法使用 --quit 优雅入口"
    }

    $ids = @($targets | ForEach-Object { $_.Id })
    $deadline = (Get-Date).AddSeconds($QuitTimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $alive = @($ids | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
        if ($alive.Count -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }

    # 2) 回退：仍未退出的才强杀（按整棵进程树）
    $alive = @($ids | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
    foreach ($victimPid in $alive) {
        Info ("回退强杀 PID {0}（优雅退出后在 {1}s 内未退出）" -f $victimPid, $QuitTimeoutSeconds)
        & taskkill.exe /PID $victimPid /T /F 2>&1 | Out-Null
    }
    if ($alive.Count -eq 0) { Info '优雅退出成功（未使用强杀）' }
    return $true
}

function Get-PortOwner([int]$p) {
    $c = Get-NetTCPConnection -State Listen -LocalPort $p -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($c) { return [int]$c.OwningProcess }
    return 0
}

Write-Host ''
Write-Host ('=' * 70) -ForegroundColor DarkGray
Write-Host ' 服务生命周期验证（independent 语义）' -ForegroundColor Cyan
Write-Host ('=' * 70) -ForegroundColor DarkGray

# ---------------------------------------------------------------------------
# A. 静态检查：产物与配置
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[A] 静态检查' -ForegroundColor Cyan

if (Test-Path $AppExe) { Pass "构建产物存在：$AppExe" } else { Fail "缺少构建产物 $AppExe（先跑 build.ps1 release）" }

$viOk = $false
if (Test-Path $AppExe) {
    $vi = (Get-Item $AppExe).VersionInfo
    $viOk = -not [string]::IsNullOrWhiteSpace($vi.FileVersion)
    if ($viOk) { Pass "产物含版本资源（FileVersion=$($vi.FileVersion)）" } else { Fail '产物缺少版本资源' }
}

# 关键：源码里必须不再无条件把子进程挂到带 KILL_ON_JOB_CLOSE 的 Job 上
$procRs = Join-Path $root 'crates\dsh-core\src\process.rs'
if (Test-Path $procRs) {
    $t = Get-Content $procRs -Raw
    if ($t -match 'CREATE_BREAKAWAY_FROM_JOB') { Pass 'process.rs 使用 CREATE_BREAKAWAY_FROM_JOB' }
    else { Fail 'process.rs 未使用 CREATE_BREAKAWAY_FROM_JOB（independent 语义不成立）' }
    if ($t -match 'ChildLifecycle::Independent' -and $t -match 'ChildLifecycle::Tied') {
        Pass 'process.rs 同时保留 independent / tied 两种语义'
    } else {
        Fail 'process.rs 缺少两种生命周期语义之一'
    }
} else {
    Fail "缺少 $procRs"
}

# 默认配置必须是 independent
$cfgRs = Join-Path $root 'crates\dsh-core\src\config.rs'
if (Test-Path $cfgRs) {
    $t = Get-Content $cfgRs -Raw
    if ($t -match '(?s)enum ServiceLifecycle\b.*?#\[default\]\s*\r?\n\s*Independent') {
        Pass 'ServiceLifecycle 默认值为 Independent'
    } else {
        Fail 'ServiceLifecycle 默认值不是 Independent（退出启动器会切断会话）'
    }
} else {
    Fail "缺少 $cfgRs"
}

# ---------------------------------------------------------------------------
# B. 运行期：诊断当前实例
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[B] 当前运行状态' -ForegroundColor Cyan
$owner = Get-PortOwner $Port
if ($owner -eq 0) {
    Info "端口 $Port 上当前没有服务（跳过运行期检查）"
} else {
    $p = Get-CimInstance Win32_Process -Filter "ProcessId=$owner" -ErrorAction SilentlyContinue
    Info "端口 $Port 占用者：PID $owner（$($p.Name)）"
    if (Test-Path $Record) {
        $rec = Get-Content $Record -Raw | ConvertFrom-Json
        if ($rec.pid -eq $owner) { Pass "service.json 记录的 PID 与端口占用者一致（$owner）" }
        else { Info "service.json 记录 PID=$($rec.pid)，端口占用者 PID=$owner（可能已被接管/重启）" }
    } else {
        Info '尚无 service.json（由改版后的启动器首次成功启动/接管时写入）'
    }
}

# ---------------------------------------------------------------------------
# C. 破坏性实验（需 -Force）
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[C] 破坏性实验：结束启动器后服务是否存活' -ForegroundColor Cyan

if (-not $Force) {
    Info '未指定 -Force：跳过。指定后会结束启动器（dsh 服务应存活）。'
    Info "命令：pwsh -NoProfile -File tools\verify-service-lifecycle.ps1 -Force"
    $script:skipReasons.Add('未指定 -Force（只做了只读检查）')
} elseif ($owner -eq 0) {
    Info "端口 $Port 上没有服务，实验无意义：请先启动服务再重试。"
    $script:skipReasons.Add("端口 $Port 上没有服务")
} else {
    $launcher = Get-RepoLauncherProcesses
    if ($launcher.Count -eq 0) {
        # 这里不能只看「有没有 DSHLauncher 进程」：还要求进程确实来自本仓库路径，
        # 否则会误杀其他安装，也无法证明实验针对的是本次构建的产物。
        Info '当前没有运行中的、属于本仓库的启动器：无法做「结束启动器」实验。'
        Info "请先启动启动器（$RootExe）并等它拉起 dsh，然后重跑本脚本 -Force。"
        $script:skipReasons.Add('没有属于本仓库的启动器进程在运行')
    } else {
        $ownerBefore = Get-PortOwner $Port
        Info "实验前：启动器 PID $(($launcher | ForEach-Object { $_.Id }) -join ',')；服务 PID $ownerBefore"
        # 优雅优先、路径过滤、逐条记录实际路径（P1-10）
        Stop-RepoLauncher | Out-Null
        Start-Sleep -Seconds 3
        $ownerAfter = Get-PortOwner $Port
        $alive = $ownerAfter -ne 0
        $script:experimentRan = $true
        if ($alive) {
            Pass "结束启动器后服务仍存活（PID $ownerAfter）—— independent 语义生效 ✓"
        } else {
            Fail '结束启动器后服务消失：仍是 Job 绑死语义（或运行的是旧版产物）'
        }
    }
}

Write-Host ''
Write-Host ('=' * 70) -ForegroundColor DarkGray
if ($failed.Count -gt 0) {
    Write-Host " 结果：$($failed.Count) 项不通过" -ForegroundColor Red
    foreach ($f in $failed) { Write-Host "   - $f" -ForegroundColor Red }
    exit 1
}
# P2-6：决定性实验没真正运行 → 只读检查全过也只能是 SKIPPED（退出码 2），
# 绝不能打印「全部通过」并 exit 0。
if (-not $script:experimentRan) {
    Write-Host ' 结果：SKIPPED —— 只读检查通过，但决定性的破坏性实验未运行' -ForegroundColor Yellow
    Write-Host '       （SKIPPED 不等于通过：请按下列原因准备环境后重跑本脚本）' -ForegroundColor Yellow
    foreach ($r in $script:skipReasons) { Write-Host "       - $r" -ForegroundColor Yellow }
    Write-Host "       命令：pwsh -NoProfile -File tools\verify-service-lifecycle.ps1 -Force" -ForegroundColor Yellow
    exit 2
}
Write-Host ' 结果：全部通过' -ForegroundColor Green
exit 0
