# tools\gen-facts.ps1 —— 生成「事实文件」，终结文档数字漂移
#
# ## 为什么需要它
#
# v5.0.0 审计发现同一组指标在文档里有 3–4 个互相矛盾的值：
#   单元测试    28 / 48 / 58         （实测 65+）
#   一致性项数  31 / 50+ / 53 / 60   （实测 88）
#   exe 体积    490 / 934.5 / 952.5 KB / 973,312 B（实测约 981 KB）
#   代码量      2,900 / 4,089 / 5,155 行
#
# 根因：这些数字是**手写**在 5 份文档里的，改代码时没人改文档。
# 本脚本把它们算出来并写入 `docs/FACTS.json`，作为**唯一来源**；
# 文档只引用该文件（并在文首注明「数字来自 FACTS.json，由 tools/gen-facts.ps1 生成」）。
#
# 用法: pwsh -NoProfile -File tools\gen-facts.ps1 [-Check]
#   -Check : 只比对，不写入；有漂移则退出码 1（供 CI / check-consistency 使用）
#
# ## -Check 的比对范围（P1-12）
#
# 旧实现只比对 4 项（unit_tests.total / code_lines.rust_production /
# dependencies.lockfile_packages / consistency_items），于是
# `artifacts.*`、`code_lines.html`、`code_lines.rust_build_rs`、
# `code_lines.rust_examples`、`code_lines.rust_total` **从来没有被比对过**：
# 一个陈旧的产物条目（甚至指向已不存在的 exe、或 installer: null）都能通过。
# 现在改为**递归比对整棵 FACTS 树**，任何一项不同都是漂移并逐项报出；
# 其中 `artifacts.*` 的 `null ↔ 对象` 变化（产物出现/消失）也算漂移。

param(
    [switch]$Check
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
Set-Location $root

$factsPath = Join-Path $root 'docs\FACTS.json'

# ---------------------------------------------------------------------------
# 1. 单元测试数量：直接数 `#[test]`
#
# 为什么要把 build.rs 单列（P1-13）：build.rs 里的 `#[test]` **永远不会被
# `cargo test` 编译或执行**（构建脚本不参与测试目标），旧实现把它们算进
# unit_tests.total，于是对外发布的"单元测试数"比真正可运行的套件多 4 个。
# 现在：per_crate 只统计真实可测的 `.rs`，total = 各 crate 之和（即实际会跑的），
# build.rs 的 `#[test]` 单独记为 unit_tests.build_script。
# ---------------------------------------------------------------------------
# crate 列表**从 Cargo.toml 的 workspace members 解析**，不写死：
# 写死会导致新增 crate（例如 dsh-uninstall / dsh-buildinfo）的测试被静默漏统计 ——
# 实测踩到：加了两个 crate（22 个测试）后 FACTS 仍报旧的 111，文档跟着失真。
$crateNames = @()
$memberBlock = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?ms)^\[workspace\](.*?)(^\[|\z)').Groups[1].Value
foreach ($m in [regex]::Matches($memberBlock, '"crates/([^"]+)"')) { $crateNames += $m.Groups[1].Value }
if ($crateNames.Count -eq 0) { Write-Host '无法从 Cargo.toml 解析 workspace members' -ForegroundColor Red; exit 1 }

$testCount = 0
$perCrate = [ordered]@{}
$buildScriptTests = 0
foreach ($crate in $crateNames) {
    $dir = Join-Path $root "crates\$crate"
    if (-not (Test-Path $dir)) { continue }
    $n = 0
    Get-ChildItem -Path $dir -Recurse -File -Filter *.rs |
        Where-Object { $_.Name -ne 'build.rs' -and $_.FullName -notmatch '\\target\\' } |
        ForEach-Object {
            $n += ([regex]::Matches((Get-Content $_.FullName -Raw), '(?m)^\s*#\[test\]')).Count
        }
    $perCrate[$crate] = $n
    $testCount += $n
}
# 构建脚本里的 `#[test]`：cargo test 不会编译它们，因此单列
Get-ChildItem -Path (Join-Path $root 'crates') -Recurse -File -Filter build.rs -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch '\\target\\' } |
    ForEach-Object {
        $buildScriptTests += ([regex]::Matches((Get-Content $_.FullName -Raw), '(?m)^\s*#\[test\]')).Count
    }

# ---------------------------------------------------------------------------
# 2. 代码量：只统计生产代码（排除 target / 示例 / 构建脚本单列）
# ---------------------------------------------------------------------------
function Count-Lines($paths) {
    $n = 0
    foreach ($p in $paths) { $n += (Get-Content $p | Measure-Object -Line).Lines }
    return $n
}
$srcFiles = Get-ChildItem -Path (Join-Path $root 'crates') -Recurse -File -Filter *.rs |
    Where-Object { $_.FullName -notmatch '\\target\\' -and $_.Name -ne 'build.rs' -and $_.FullName -notmatch '\\examples\\' }
$buildRs = Get-ChildItem -Path (Join-Path $root 'crates') -Recurse -File -Filter build.rs |
    Where-Object { $_.FullName -notmatch '\\target\\' }
$examples = Get-ChildItem -Path (Join-Path $root 'crates') -Recurse -File -Filter *.rs |
    Where-Object { $_.FullName -match '\\examples\\' }
$htmlFiles = Get-ChildItem -Path (Join-Path $root 'ui') -File -Filter *.html -ErrorAction SilentlyContinue

$rustSrc = Count-Lines $srcFiles.FullName
$rustBuild = Count-Lines $buildRs.FullName
$rustExample = Count-Lines $examples.FullName
$htmlLines = Count-Lines $htmlFiles.FullName

# ---------------------------------------------------------------------------
# 3. 产物体积与版本
# ---------------------------------------------------------------------------
function Exe-Info($p) {
    if (-not (Test-Path $p)) { return $null }
    $f = Get-Item $p
    $vi = $f.VersionInfo
    return [ordered]@{
        path          = $p.Replace("$root\", '')
        bytes         = $f.Length
        kib           = [math]::Round($f.Length / 1KB, 1)
        file_version  = $vi.FileVersion
        product_name  = $vi.ProductName
    }
}

# ---------------------------------------------------------------------------
# 4. 依赖规模
# ---------------------------------------------------------------------------
$lockText = Get-Content (Join-Path $root 'Cargo.lock') -Raw
$lockPkgs = ([regex]::Matches($lockText, '(?m)^\[\[package\]\]')).Count

# ---------------------------------------------------------------------------
# 5. 一致性校验项数（读取脚本自身的机器可读汇总行）
#
# 用 `CHECK_SUMMARY passed=<n> failed=<m> total=<n+m>` 这一行，而不是中文
# 「全部 N 项通过」：后者在某条检查失败时**不会出现**，于是这里会拿不到数字
# （旧实现写成 null / 静默跳过比对），记录到的"检查项总数"随失败而漂移（P2-10）。
# 注意：脚本本身返回非 0（有检查失败）**不是**本步骤的失败 —— 我们只是要那个总数；
# 但**拿不到总数**是明确失败（`consistency_items` 不可测），不再静默跳过（P1-12）。
# ---------------------------------------------------------------------------
$consistencyCount = $null
$consistencyScript = Join-Path $here 'check-consistency.ps1'
$consistencyError = $null
if (-not (Test-Path $consistencyScript)) {
    $consistencyError = "缺少 $consistencyScript"
} else {
    $out = & pwsh -NoProfile -File $consistencyScript 2>&1
    $joined = $out -join "`n"
    $m = [regex]::Match($joined, 'CHECK_SUMMARY passed=(\d+) failed=(\d+) total=(\d+)')
    if ($m.Success) {
        $consistencyCount = [int]$m.Groups[3].Value
    } else {
        # 兼容旧版脚本（只有中文汇总行）
        $m2 = [regex]::Match($joined, '全部 (\d+) 项通过')
        if ($m2.Success) { $consistencyCount = [int]$m2.Groups[1].Value }
        else { $consistencyError = 'check-consistency.ps1 未输出 CHECK_SUMMARY（无法测量检查项总数）' }
    }
}

# ---------------------------------------------------------------------------
# 6. 组装
# ---------------------------------------------------------------------------
$facts = [ordered]@{
    '_generated_by'  = 'tools/gen-facts.ps1（文档数字的唯一来源；请勿手改本文件）'
    '_note'          = 'docs/*.md 中的指标数字必须与本文件一致（tools/gen-facts.ps1 -Check 会强制校验）'
    unit_tests       = [ordered]@{
        total        = $testCount
        per_crate    = $perCrate
        # cargo test 不会编译/运行 build.rs 里的 #[test]，单列以免混入对外数字
        build_script = $buildScriptTests
    }
    code_lines       = [ordered]@{
        rust_production = $rustSrc
        rust_build_rs   = $rustBuild
        rust_examples   = $rustExample
        html            = $htmlLines
        rust_total      = $rustSrc + $rustBuild + $rustExample
    }
    artifacts        = [ordered]@{
        launcher_root   = Exe-Info (Join-Path $root 'DSHLauncher.exe')
        launcher_build  = Exe-Info (Join-Path $root 'target\release\dsh-app.exe')
        installer       = Exe-Info (Join-Path $root 'DSHLauncherSetup.exe')
    }
    dependencies     = [ordered]@{
        lockfile_packages = $lockPkgs
    }
    consistency_items = $consistencyCount
}

$json = $facts | ConvertTo-Json -Depth 6

# ---------------------------------------------------------------------------
# 7. 递归比对（P1-12）
#
# 把 FACTS.json 与本次实测值按同一套"归一化"规则压平后逐项比对。
# 先 `ConvertTo-Json | ConvertFrom-Json` 把两边都变成纯 PSCustomObject/数组/标量，
# 避免 `[ordered]` 字典与 PSCustomObject 混用时的遍历差异。
# 规则：
#   * 对象 → 递归到叶子，路径形如 artifacts.installer.bytes
#   * 数组 → 用 JSON 文本整体比较（本项目目前没有数组事实）
#   * $null → 记为 "<null>"，于是 `null ↔ 对象` 表现为键的出现/消失 = 漂移
#   * 标量 → 统一转字符串（避免反序列化后 Int64/Double 之类差异误报）
# ---------------------------------------------------------------------------
function ConvertTo-PlainObject($obj) {
    # 统一类型：字典/对象 → PSCustomObject；标量原样
    $json = $obj | ConvertTo-Json -Depth 12 -Compress
    return ($json | ConvertFrom-Json)
}

function Expand-Facts {
    param($obj, [string]$prefix, $bag, [int]$depth = 0)
    if ($depth -gt 12) { $bag[$prefix] = '<depth-limit>'; return }
    if ($null -eq $obj) {
        $bag[$prefix] = '<null>'
        return
    }
    if ($obj -is [string] -or $obj -is [bool] -or $obj -is [int] -or $obj -is [long] -or $obj -is [double] -or $obj -is [decimal]) {
        $bag[$prefix] = [string]$obj
        return
    }
    if ($obj -is [System.Array]) {
        $bag[$prefix] = ($obj | ConvertTo-Json -Compress -Depth 12)
        return
    }
    if ($obj -is [System.Collections.IDictionary]) {
        foreach ($k in @($obj.Keys)) {
            $p = if ([string]::IsNullOrEmpty($prefix)) { [string]$k } else { "$prefix.$k" }
            Expand-Facts -obj $obj[$k] -prefix $p -bag $bag -depth ($depth + 1)
        }
        return
    }
    # 其余一律按对象处理（PSCustomObject）
    $props = @($obj.PSObject.Properties)
    if ($props.Count -eq 0) {
        $bag[$prefix] = [string]$obj
        return
    }
    foreach ($prop in $props) {
        $p = if ([string]::IsNullOrEmpty($prefix)) { $prop.Name } else { "$prefix.$($prop.Name)" }
        Expand-Facts -obj $prop.Value -prefix $p -bag $bag -depth ($depth + 1)
    }
}

if ($Check) {
    if (-not (Test-Path $factsPath)) {
        Write-Host "FACTS.json 不存在，请先运行 pwsh -File tools\gen-facts.ps1" -ForegroundColor Red
        exit 1
    }
    if ($null -ne $consistencyError) {
        # 不可测量不能静默跳过（P1-12）：一致性项数是文档要引用的数字之一
        Write-Host "无法测量一致性项数：$consistencyError" -ForegroundColor Red
        exit 1
    }

    $oldObj = ConvertTo-PlainObject (Get-Content $factsPath -Raw | ConvertFrom-Json)
    $newObj = ConvertTo-PlainObject $facts
    $oldBag = [ordered]@{}
    $newBag = [ordered]@{}
    Expand-Facts -obj $oldObj -prefix '' -bag $oldBag
    Expand-Facts -obj $newObj -prefix '' -bag $newBag

    $drift = New-Object System.Collections.Generic.List[string]
    # 1) 旧有：值不同 / 已不存在 → 漂移
    foreach ($k in $oldBag.Keys) {
        if (-not $newBag.Contains($k)) {
            $drift.Add("$k ：FACTS.json=$($oldBag[$k])，实测=<已不存在>")
        } elseif ($oldBag[$k] -ne $newBag[$k]) {
            $drift.Add("$k ：FACTS.json=$($oldBag[$k])，实测=$($newBag[$k])")
        }
    }
    # 2) 新增（含 artifacts 从 null 变成对象）→ 漂移
    foreach ($k in $newBag.Keys) {
        if (-not $oldBag.Contains($k)) {
            $drift.Add("$k ：FACTS.json=<不存在>，实测=$($newBag[$k])")
        }
    }

    if ($drift.Count -gt 0) {
        Write-Host '检测到事实漂移（请重新运行 tools\gen-facts.ps1 并同步文档）：' -ForegroundColor Red
        foreach ($d in $drift) { Write-Host "  - $d" -ForegroundColor Red }
        exit 1
    }
    Write-Host ("FACTS.json 与实测一致 OK（已比对 {0} 项事实）" -f $newBag.Count) -ForegroundColor Green
    exit 0
}

$dir = Split-Path -Parent $factsPath
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
$json | Set-Content -Path $factsPath -Encoding utf8
Write-Host "已生成事实文件：$factsPath" -ForegroundColor Green
Write-Host "  单元测试       = $testCount（可运行；另有 $buildScriptTests 个 build.rs 内 #[test] 不计入）  $($perCrate | ConvertTo-Json -Compress)"
Write-Host "  生产 Rust 行数 = $rustSrc"
Write-Host "  HTML 行数      = $htmlLines"
Write-Host "  lockfile 包数  = $lockPkgs"
Write-Host "  一致性项数     = $consistencyCount"
exit 0
