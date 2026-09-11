# tools\osv-audit.ps1 — 用 OSV API 对 Cargo.lock 做漏洞扫描（cargo-audit 的替代路径）
#
# 为什么需要它：`cargo audit` 需要从 GitHub 拉取 RustSec advisory-db
# （`git clone https://github.com/RustSec/advisory-db.git`）。在**只放行部分域名**的
# 网络环境里该步骤会直接失败：
#     error: couldn't fetch advisory database: git operation failed: An IO error ...
#
# 本脚本改用 OSV（Open Source Vulnerabilities，Google 维护）的公开 REST API，
# 它对 crates.io 生态提供与 RustSec 同源（RustSec 已镜像进 OSV）的镜像数据：
#     POST https://api.osv.dev/v1/query
#     {"package":{"name":"<crate>","ecosystem":"crates.io"},"version":"<ver>"}
#
# 覆盖范围与 cargo-audit 等价（都是 RustSec 数据），差别只是取数通道。
#
# 用法:
#   pwsh -NoProfile -File tools\osv-audit.ps1                 # 扫全量 264 个包
#   pwsh -NoProfile -File tools\osv-audit.ps1 -OnlyDirect     # 只扫直接依赖
#   pwsh -NoProfile -File tools\osv-audit.ps1 -ThrottleMs 120
# 退出码: 0 = 未发现「进入 Windows 构建图」的漏洞；1 = Windows 构建图内确有漏洞或查询失败

param(
    [switch]$OnlyDirect,
    [int]$ThrottleMs = 60,
    [string]$LockFile
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
if (-not $LockFile) { $LockFile = Join-Path $root 'Cargo.lock' }
if (-not (Test-Path $LockFile)) { Write-Error "找不到 $LockFile"; exit 1 }

# ---- 解析 Cargo.lock（[[package]] 块内 name + version 相邻出现）----
$lines = Get-Content $LockFile
$pkgs = New-Object System.Collections.Generic.List[object]
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^name = "([^"]+)"$') {
        $n = $Matches[1]
        if ($i + 1 -lt $lines.Count -and $lines[$i + 1] -match '^version = "([^"]+)"$') {
            $pkgs.Add([pscustomobject]@{ Name = $n; Version = $Matches[1] })
        }
    }
}

$direct = @('dsh-core', 'dsh-ui', 'dsh-app', 'wry', 'tao', 'tray-icon', 'muda', 'windows',
    'serde', 'serde_json', 'toml', 'thiserror', 'anyhow', 'dirs')
if ($OnlyDirect) {
    $pkgs = @($pkgs | Where-Object { $direct -contains $_.Name })
}

Write-Host "OSV 扫描：$($pkgs.Count) 个包（ecosystem=crates.io）" -ForegroundColor Cyan

$findings = New-Object System.Collections.Generic.List[string]
$errors = New-Object System.Collections.Generic.List[string]
$checked = 0

foreach ($p in $pkgs) {
    $body = @{ package = @{ name = $p.Name; ecosystem = 'crates.io' }; version = $p.Version } |
        ConvertTo-Json -Compress -Depth 4
    try {
        $r = Invoke-RestMethod -Uri 'https://api.osv.dev/v1/query' -Method Post -Body $body `
            -ContentType 'application/json' -TimeoutSec 25
        $checked++
        if ($r.vulns) {
            foreach ($v in $r.vulns) {
                $sev = ''
                if ($v.database_specific -and $v.database_specific.severity) { $sev = $v.database_specific.severity }
                $fixed = ''
                foreach ($a in $v.affected) {
                    foreach ($rg in $a.ranges) {
                        foreach ($e in $rg.events) { if ($e.fixed) { $fixed = $e.fixed } }
                    }
                }
                $line = "{0} {1}  <- {2}{3}{4}" -f $p.Name, $p.Version, $v.id,
                    $(if ($sev) { " [$sev]" } else { '' }),
                    $(if ($fixed) { " (fixed in $fixed)" } else { '' })
                $findings.Add($line)
                Write-Host "  VULN  $line" -ForegroundColor Red
            }
        }
    } catch {
        $errors.Add("$($p.Name) $($p.Version): $($_.Exception.Message)")
    }
    if ($ThrottleMs -gt 0) { Start-Sleep -Milliseconds $ThrottleMs }
}

Write-Host ''
Write-Host "已查询 $checked / $($pkgs.Count) 个包" -ForegroundColor DarkGray
if ($errors.Count -gt 0) {
    Write-Host "查询失败 $($errors.Count) 个（网络/限流），未覆盖：" -ForegroundColor Yellow
    $errors | Select-Object -First 10 | ForEach-Object { Write-Host "  $_" }
}
if ($findings.Count -gt 0) {
    Write-Host ''
    Write-Host "Cargo.lock 全平台范围内发现 $($findings.Count) 条漏洞记录" -ForegroundColor Red
    $findings | ForEach-Object { Write-Host "  $_" }
    # ------------------------------------------------------------------
    # 关键判定：`Cargo.lock` 是 **universal** 的，它为所有平台记录依赖，不等于
    # 「本次构建用到的包」。本项目刻意用 `default-features = false` 关掉 GUI crate
    # 的 Linux 默认 feature，因此 `glib` / `proc-macro-error` 这类 advisory **只存在于
    # Linux 的 GTK 栈**，Windows 构建图里根本没有它们。
    #
    # 此前脚本只要发现记录就 `exit 1`，使这条门禁**永远是红的**——一个永远红的门禁
    # 等于没有门禁。现在按真实影响面判定：只有 advisory 落在 **Windows 构建图** 里才失败。
    # ------------------------------------------------------------------
    Write-Host ''
    Write-Host '按 Windows 构建图（x86_64-pc-windows-msvc）复核真实影响面…' -ForegroundColor Cyan
    $inGraph = New-Object System.Collections.Generic.List[string]
    $names = @($findings | ForEach-Object { ($_ -split ' ')[0] } | Select-Object -Unique)
    foreach ($name in $names) {
        $tree = & cargo tree --offline -i $name --target x86_64-pc-windows-msvc -e normal,build 2>&1
        $present = -not ($tree -match 'nothing to print|did not match')
        if ($present) {
            $inGraph.Add("$name : 存在于 Windows 构建图（真实风险）")
            Write-Host "  [IN-GRAPH]     $name" -ForegroundColor Red
        } else {
            Write-Host "  [NOT-IN-GRAPH] $name （仅存在于非 Windows 平台依赖，不参与本次构建）" -ForegroundColor DarkGray
        }
    }
    Write-Host ''
    if ($inGraph.Count -gt 0) {
        Write-Host "结论：Windows 构建图内 $($inGraph.Count) 个 —— 构成真实风险" -ForegroundColor Red
        $inGraph | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
        exit 1
    }
    Write-Host '结论：所有 advisory 均不在 Windows 构建图中 —— 不构成本项目运行风险' -ForegroundColor Green
    Write-Host '（改 GUI crate 的 feature 时必须重跑本脚本：一旦某个包进入 Windows 图，这里会立刻变红）' -ForegroundColor DarkGray
    exit 0
}
if ($errors.Count -gt 0) {
    Write-Host "结果：无漏洞记录，但存在未覆盖包（不可视为全绿）" -ForegroundColor Yellow
    exit 1
}
Write-Host "结果：未发现漏洞 ✓" -ForegroundColor Green
exit 0
