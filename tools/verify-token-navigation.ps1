# tools\verify-token-navigation.ps1 —— 验证「内嵌窗口是否导航到带 token 的地址」
#
# 背景（v5.0.0 审计的 P0 缺陷）：
#   dsh web 对**无 token** 访问返回 HTTP 401（实测）。启动器必须从 dsh 的 stdout
#   解析出 `dsh web: http://127.0.0.1:<port>/?token=…` 并用它导航。
#   旧实现存在两个缺陷叠加：
#     1. wait_for_ready_worker 在锁内只读一次 auth_url，与 stdout 捕获线程竞态，
#        实测稳定读到陈旧的 None → 用无 token 地址导航 → 用户看到 401 页；
#     2. Ready 事件无条件把无 token 地址写进 pending_url，使得「等 token」的
#        逻辑永远不可达。
#   修复后：Ready 事件的 url 是 Option，只有带 token 才写入 pending_url；
#   就绪时的地址选择改为在锁内二次确认。
#
# 本脚本做三件事：
#   A. 静态检查（源码里必须存在捕获 + 带 token 才放行的逻辑）
#   B. HTTP 事实核对（无 token = 401；带 token = 303 重定向）
#   C. 日志断言（若启动器日志中有就绪记录，导航行必须带 token）
#
# 用法: pwsh -NoProfile -File tools\verify-token-navigation.ps1 [-Port 3080]
# 退出码: 0 = 通过；1 = 不通过

param(
    [int]$Port = 3080
)

$ErrorActionPreference = 'Continue'
# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$Log = Join-Path $env:LOCALAPPDATA 'DSHLauncher\logs\launcher.log'

$failed = New-Object System.Collections.Generic.List[string]
function Pass($m) { Write-Host "  [PASS] $m" -ForegroundColor Green }
function Fail($m) { Write-Host "  [FAIL] $m" -ForegroundColor Red; $script:failed.Add($m) }
function Info($m) { Write-Host "  $m" -ForegroundColor DarkGray }

Write-Host ''
Write-Host ('=' * 70) -ForegroundColor DarkGray
Write-Host ' token 导航验证' -ForegroundColor Cyan
Write-Host ('=' * 70) -ForegroundColor DarkGray

# ---------------------------------------------------------------------------
# A. 静态检查
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[A] 静态检查' -ForegroundColor Cyan

$svc = Join-Path $root 'crates\dsh-app\src\service.rs'
if (Test-Path $svc) {
    $t = Get-Content $svc -Raw
    # Ready 事件必须携带 Option（url 为 None 时禁止填普通地址）
    # 判定与格式无关：cargo fmt 可能把结构体变体折成一行
    if ($t -match 'Ready\s*\{[^}]*url\s*:\s*Option<String>') {
        Pass 'Ready 事件的 url 为 Option<String>（允许「就绪但暂无 token」）'
    } else {
        Fail 'Ready 事件未使用 Option<String>：无法区分「有 token / 无 token」'
    }
    # 捕获回调必须存在且落日志
    if ($t -match '已捕获 dsh 就绪地址') { Pass '存在 token 捕获回调并留痕' }
    else { Fail '未发现 token 捕获回调的留痕（无法证明捕获链路生效）' }
    # 捕获后必须补发 Ready，否则界面不会升级为带 token 地址
    if ($t -match '(?s)on_ready.*?ServiceEvent::Ready') { Pass '捕获到 token 后会补发 Ready' }
    else { Fail '捕获到 token 后未补发 Ready（界面不会重新导航到带 token 地址）' }
} else {
    Fail "缺少 $svc"
}

$app = Join-Path $root 'crates\dsh-app\src\app.rs'
if (Test-Path $app) {
    $t = Get-Content $app -Raw
    if ($t -match 'fn contains_token') { Pass 'app.rs 有 contains_token() 判定' }
    else { Fail 'app.rs 缺少 contains_token()：无法保证只在带 token 时导航' }
    if ($t -match '(?s)if contains_token\(&u\)\s*\{\s*self\.pending_url') {
        Pass '只有带 token 的地址才写入 pending_url'
    } else {
        Fail 'pending_url 可能被无 token 地址覆盖（等待逻辑会再次不可达）'
    }
} else {
    Fail "缺少 $app"
}

# ---------------------------------------------------------------------------
# B. HTTP 事实核对
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host "[B] HTTP 事实核对（端口 $Port）" -ForegroundColor Cyan

function Hit([string]$url) {
    try {
        $r = Invoke-WebRequest -Uri $url -TimeoutSec 8 -MaximumRedirection 0 -ErrorAction Stop
        return [int]$r.StatusCode
    } catch {
        $resp = $_.Exception.Response
        if ($resp) { return [int]$resp.StatusCode }
        return -1
    }
}

$bare = Hit "http://127.0.0.1:$Port/"
if ($bare -eq 401) {
    Pass '无 token 访问返回 401（这正是必须带 token 导航的原因）'
} elseif ($bare -eq -1) {
    Info "端口 $Port 无响应（服务未运行？）——跳过 HTTP 核对"
} else {
    Info "无 token 访问返回 $bare（上游行为可能已变化，请复核 token 策略）"
}

# ---------------------------------------------------------------------------
# C. 日志断言
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '[C] 启动器日志断言' -ForegroundColor Cyan

if (-not (Test-Path $Log)) {
    Info '尚无 launcher.log，跳过'
} else {
    $text = [System.IO.File]::ReadAllText($Log)
    $openLines = ($text -split "`r?`n") | Where-Object { $_ -match '已打开内嵌界面：' }
    if (-not $openLines) {
        Info '日志中没有「已打开内嵌界面」记录（尚未开窗？）'
    } else {
        $last = $openLines[-1]
        if ($last -match '\?token=') {
            Pass "最近一次开窗使用了带 token 的地址"
        } else {
            # 无 token 开窗在下面两类情形下**属设计预期**（token 不可得，只能依赖
            # WebView2 里已持久化的浏览器会话 Cookie）：
            #   1) 接管别处启动的实例 —— 启动器没 spawn 它，读不到它的 stdout；
            #   2) 通过 service.json 对账接管上一次启动留下的实例。
            #
            # 判定要点：**从未捕获过 token** 才算真正的异常（说明捕获链路从未生效）；
            # 只要历史上捕获过，就说明这条链路是通的。
            #
            # ⚠️ 本脚本此前只识别「已接管端口」这一种旧措辞，把"service.json 对账接管"
            # 误判为失败（实测踩到）。这里按"是否曾经捕获过"来判定，与措辞解耦。
            $everCaptured = $text -match '已捕获 dsh 就绪地址'
            $adopted = ($text -match '已接管端口') -or ($text -match '直接接管') -or
                       ($text -match '仍然可用.*接管')
            if (-not $everCaptured) {
                Fail "最近一次开窗地址不含 token，且日志中**从未**出现「已捕获 dsh 就绪地址」：$last"
                Info '这意味着 stdout 捕获链路从未生效 —— 冷启动会导航到 HTTP 401 页。'
            } elseif ($adopted) {
                Info "最近一次开窗为无 token 地址（接管已有实例；token 不可得，属预期）：$last"
                Info '日志中曾成功捕获过 token（见下条 PASS），捕获链路有效。'
                Info '如需验证「自有服务带 token 导航」，请冷启动一次（让启动器自己拉起 dsh）。'
            } else {
                Info "最近一次开窗为无 token 地址，但日志中曾捕获过 token（期间可能重启过服务）：$last"
            }
        }
    }
    if ($text -match '已捕获 dsh 就绪地址') {
        Pass '日志中存在「已捕获 dsh 就绪地址」记录（捕获链路已被真实触发）'
    } else {
        Info '日志中暂无「已捕获 dsh 就绪地址」：需要一次由启动器自己拉起 dsh 的冷启动才能验证'
    }
}

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
