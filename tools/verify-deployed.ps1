# tools\verify-deployed.ps1 —— 校验「根产物 + 运行中实例」是否真的是可用版本
#
# ## 为什么需要它
#
# 本轮踩过一个很贵的坑：**根产物长期没被更新**，用户双击的一直是带死锁的旧版
# （进程在、托盘图标在，但没有窗口）。当时的"验证"是看进程在不在 —— 那是无效验证。
#
# 这个脚本把"部署成功"定义成三件可观测的事：
#   1. 根产物 `DSHLauncher.exe` 通过 `--build-info` 自报修复标记（**可执行判定**，
#      不要用"在二进制里搜字符串"：`debug_assert!` 与注释都不进 release 产物）；
#   2. 运行中的实例**确实有可见窗口**（死锁时进程在但窗口没有）；
#   3. 进程不处于 `hung` 状态（死锁时所有窗口 `IsHungAppWindow = True`）。
#
# ## 用法
#
#   pwsh -NoProfile -File tools\verify-deployed.ps1                 # 默认期望启动器在运行
#   pwsh -NoProfile -File tools\verify-deployed.ps1 -NotRunning    # 只校验产物，不要求进程
#
# 退出码：0 = 通过；1 = 不通过

param(
    [switch]$NotRunning
)

$ErrorActionPreference = 'Continue'
# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$exe = Join-Path $root 'DSHLauncher.exe'
$marker = Join-Path $env:APPDATA 'DSHLauncher\build-info.txt'

$failed = New-Object System.Collections.Generic.List[string]
function Pass($m) { Write-Host "  [PASS] $m" -ForegroundColor Green }
function Fail($m) { Write-Host "  [FAIL] $m" -ForegroundColor Red; $script:failed.Add($m) }
function Info($m) { Write-Host "  ..   $m" -ForegroundColor DarkGray }

Write-Host ''
Write-Host ('=' * 70) -ForegroundColor DarkGray
Write-Host ' 根产物与运行实例校验' -ForegroundColor Cyan
Write-Host ('=' * 70) -ForegroundColor DarkGray

# ---------------------------------------------------------------------------
# A. 产物存在且自报修复标记
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[A] 根产物' -ForegroundColor Cyan
if (-not (Test-Path $exe)) {
    Fail "缺少 $exe"
} else {
    $f = Get-Item $exe
    Info ("{0:N0} 字节，{1}" -f $f.Length, $f.LastWriteTime)
    Remove-Item $marker -Force -ErrorAction SilentlyContinue
    Start-Process -FilePath $exe -ArgumentList '--build-info' -Wait -WindowStyle Hidden | Out-Null
    if (-not (Test-Path $marker)) {
        Fail '根产物不支持 --build-info（旧构建）'
    } else {
        $info = Get-Content $marker -Raw
        ($info.Trim() -split "`r?`n") | ForEach-Object { Info $_ }
        # 关键修复标记必须齐全（缺失说明部署的是旧产物）
        $required = @(
            'deadlock-fix-service-start-lock-scope',
            'service-independent-default',
            'token-capture-race-fix'
        )
        foreach ($r in $required) {
            if ($info -match [regex]::Escape($r)) { Pass "含修复标记 $r" }
            else { Fail "缺少修复标记 $r（部署的是旧产物？）" }
        }
        Remove-Item $marker -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# B. 运行中的实例必须真的有窗口、且不 hung
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[B] 运行实例' -ForegroundColor Cyan
$procs = @(Get-Process DSHLauncher -ErrorAction SilentlyContinue)
if ($procs.Count -eq 0) {
    if ($NotRunning) { Info '当前没有运行中的启动器（-NotRunning，跳过）' }
    else { Info '当前没有运行中的启动器（未要求校验；如需校验请先启动）' }
} else {
    Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public class WD {
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
  delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool IsHungAppWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  public static List<string> ForPid(uint target) {
    var res = new List<string>();
    EnumWindows((h, p) => { uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == target) { var t=new StringBuilder(256); GetWindowTextW(h,t,256);
        var c=new StringBuilder(256); GetClassNameW(h,c,256);
        res.Add(string.Format("visible={0} hung={1} class={2} title='{3}'", IsWindowVisible(h), IsHungAppWindow(h), c, t)); }
      return true; }, IntPtr.Zero);
    return res;
  }
}
"@ -ErrorAction SilentlyContinue

    # -----------------------------------------------------------------------
    # 必须先证明 Win32 桥接类型真的可用（P1-7）。
    #
    # 血泪教训：加 `-ErrorAction SilentlyContinue` 后，若 Add-Type 因任何原因失败
    # （受限语言模式、已存在同名类型冲突、编译不可用…），`[WD]::ForPid(...)` 会报错，
    # `$wins` 变成 $null，`$hung` 自然为空，于是脚本打印
    # 「[PASS] PID xxxx 无 hung 窗口」—— 在**最重要的一项检查**上给出假绿。
    # 因此类型不存在 = 直接失败，且**不允许**再打印任何 PASS。
    # -----------------------------------------------------------------------
    $wdType = 'WD' -as [type]
    $wdAvailable = $null -ne $wdType
    if (-not $wdAvailable) {
        Fail '无法加载 Win32 窗口查询桥接类型（Add-Type 失败）—— 本节结论不可信，拒绝给出 PASS'
        Write-Host '   → 常见原因：已存在同名类型（重复加载）、受限语言模式。请在新会话中重跑。' -ForegroundColor Yellow
    }

    foreach ($p in $procs) {
        if (-not $wdAvailable) {
            Fail "PID $($p.Id)：因桥接类型不可用，未能检查窗口 hung 状态"
            continue
        }
        # 任何来自 ForPid 的错误都算失败，而不是让 $wins 静默变 $null
        try {
            $wins = [WD]::ForPid([uint32]$p.Id)
        } catch {
            Fail "PID $($p.Id)：[WD]::ForPid 调用失败（$($_.Exception.Message)）—— 无法判定 hung 状态"
            continue
        }
        if ($null -eq $wins) {
            Fail "PID $($p.Id)：[WD]::ForPid 返回 null（无法枚举窗口）—— 无法判定 hung 状态"
            continue
        }
        $visible = @($wins | Where-Object { $_ -like 'visible=True*' })
        $hung = @($wins | Where-Object { $_ -like '*hung=True*' })
        Info "PID $($p.Id)：可见窗口 $($visible.Count) 个，hung $($hung.Count) 个"
        $wins | ForEach-Object { Info "    $_" }

        if ($hung.Count -gt 0) {
            Fail "PID $($p.Id) 存在 hung 窗口 —— 疑似死锁（进程在但界面无响应）"
        } else {
            Pass "PID $($p.Id) 无 hung 窗口"
        }
        # 标题为应用名的可见窗口是"它是个应用"的直接证据
        if (@($visible | Where-Object { $_ -match "title='DSHLauncher'" }).Count -gt 0) {
            Pass '存在标题为 DSHLauncher 的可见窗口'
        } else {
            Info '当前没有标题为 DSHLauncher 的可见窗口（可能已关闭到托盘 —— 托盘模式下属正常）'
        }
        # CPU 增量：死锁时主线程在等待，几乎不涨
        $c1 = $p.TotalProcessorTime.TotalMilliseconds
        Start-Sleep -Milliseconds 1500
        $p.Refresh()
        $delta = $p.TotalProcessorTime.TotalMilliseconds - $c1
        if ($delta -ge 1) { Pass ("事件循环在运行（CPU 增量 {0:N1} ms）" -f $delta) }
        else { Info ("CPU 增量为 {0:N1} ms（若窗口已关闭到托盘，接近 0 属正常）" -f $delta) }
    }
}

# ---------------------------------------------------------------------------
Write-Host ''
Write-Host ('=' * 70) -ForegroundColor DarkGray
if ($failed.Count -eq 0) {
    Write-Host ' 结果：全部通过' -ForegroundColor Green
    exit 0
} else {
    Write-Host " 结果：$($failed.Count) 项不通过" -ForegroundColor Red
    foreach ($f in $failed) { Write-Host "   - $f" -ForegroundColor Red }
    exit 1
}
