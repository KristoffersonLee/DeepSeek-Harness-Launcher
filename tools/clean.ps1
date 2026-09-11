<#
  tools\clean.ps1 —— 工作区自清洁（**报告优先**、显式执行、清完自校验）
  ============================================================================
  ## 清理对象（四个互相独立的区域）

    1. target\            构建缓存。`target/` 被 .gitignore 排除，是**缓存不是交付物**；
                          cargo 从不回收旧产物（换 feature / profile / 工具链都只新增），
                          仓库会随构建与验证轮次膨胀到数 GB。
    2. 仓库根            热替换回退副本（DSHLauncher.exe.bak-*）、旧产物（*.old-* /
                          *.pubtmp / *.replaced-old）与门禁日志（setup*.log / selftest.log）。
    3. %TEMP%             自检与单测留下的残留（dsh-install-sim / dsh_log_* / dsh_lock_* …）。
                          **白名单制**：绝不触碰 DSH 运行时正在用的 dsh-spill-* / dsh-subprocess-*。
    4. 运行期数据           %LOCALAPPDATA%\DSHLauncher（日志 / WebView2 profile / service.json）
                          —— **只报告、永不删除**：那是卸载器的职责，误删会清掉用户登录态。

  ## 保留策略（-Cache 的核心不变式）

    `-Cache` 会删掉 target\ 里**除了** release 交付物之外的一切，保留：
        release\dsh-app.exe / dsh-uninstall.exe / 对应 .d / .pdb
        CACHEDIR.TAG（cargo 写的「这是缓存，备份工具请跳过」标记，见下）
    因为：
      * `docs/FACTS.json` 的 `artifacts.launcher_build` 记录的就是这两个路径与字节数，
        `tools/gen-facts.ps1 -Check` 会与实测比对（删掉 = 事实漂移）；
      * `tools/verify-version.ps1 -Exe target\release\dsh-app.exe` 直接读它们。

  ## 构建目录可能是**被搬迁过的**（Cargo 1.91+ / 新布局）

  Cargo 1.91 起可以把「中间产物」与「最终产物」分开放：
      CARGO_BUILD_BUILD_DIR=<dir>      或     .cargo/config.toml 的 [build] build-dir
  最终产物仍在 target\<profile>\（本脚本的保留项不受影响），但 deps/build/.fingerprint/incremental
  会整块搬到 <dir>。实测：搬走后 target\ 只剩 7.6 MB，而 <dir> 有 **538 MB** 可再生缓存。
  Cargo 1.100 起的新布局（build.build-dir-new-layout）也把中间产物按包名分桶到
  `<dir>\<profile>\build\<包名>\<哈希>\`，同样不在 target\ 下。
  因此本脚本会**解析生效的构建目录**并把缓存清理与体积统计都指向它 —— 否则「自清洁」会漏掉
  最大的一块（而且用户完全看不出来）。
    清完后本脚本会**复核 FACTS 记录与实测是否仍然一致**，并明确告诉你哪些门禁仍可跑。

  ## 与旧版的差别（本轮重写）

    * 旧的清理清单是**子目录白名单**（debug / release\deps / …）：新增的 `target\tmp`、
      `target\doc`、`target\package`、新 profile 都会漏掉；现在改为「除保留项外全部清理」，
      对未来 cargo 的新目录天然免疫；
    * 旧版删不掉就报失败：热替换副本 `DSHLauncher.exe.bak-*` **是运行中实例的映像**，
      Windows 语义下不可删除 —— 现在识别为「跳过」并说明何时可删，不再污染退出码；
    * 旧版体积报告对每个子目录各递归一遍（大 target 明显慢）：现在整棵树只枚举一次；
    * 旧版不管仓库根与 %TEMP%，也不校验清理是否破坏 FACTS —— 现在都管。

  ## 用法

      pwsh -File tools\clean.ps1                     # 只报告（默认，不删任何东西）
      pwsh -File tools\clean.ps1 -Cache              # 清 target 缓存，保留 release 交付物（推荐）
      pwsh -File tools\clean.ps1 -All                # 全清 target\（之后必须重新构建）
      pwsh -File tools\clean.ps1 -Repo               # 清仓库根残留（副本/旧产物/日志）
      pwsh -File tools\clean.ps1 -Temp               # 清 %TEMP% 自检残留（白名单）
      pwsh -File tools\clean.ps1 -Cache -Repo -Temp  # 可任意组合
      pwsh -File tools\clean.ps1 -Cache -WhatIf      # 只打印将删除的每一项，零改动

  退出码：0 = 成功（含「本来就干净」与「跳过被占用项」）；1 = 有删除失败；
          2 = -Cache 前置条件不满足（release 交付物缺失 ⇒ 先构建再清缓存）。
#>
param(
    # 清 target\ 可重建缓存，保留 release 交付物
    [switch]$Cache,
    # 全清 target\（含 release 交付物 ⇒ 之后必须重新构建）
    [switch]$All,
    # 清仓库根残留（热替换副本 / 旧产物 / 门禁日志）
    [switch]$Repo,
    # 清 %TEMP% 里的自检残留（白名单制）
    [switch]$Temp,
    # 只打印将删除的内容，不实际删除
    [switch]$WhatIf
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$target = Join-Path $root 'target'

# ---------------------------------------------------------------------------
# 0. 保留项（-Cache 的唯一不变式：这两个 exe 是 FACTS 与 verify-version 的输入）
# ---------------------------------------------------------------------------
$keepRel = @(
    'release\dsh-app.exe',
    'release\dsh-app.d',
    'release\dsh_app.pdb',
    'release\dsh-uninstall.exe',
    'release\dsh-uninstall.d',
    'release\dsh_uninstall.pdb',
    # cargo 在**构建目录根**写的「这是缓存，备份工具请跳过」标记（固定签名，43 字节）。
    #
    # 为什么必须保留：它不在清单里时 `-Cache` 会把它删掉，而 cargo **只在创建构建目录时**写它 ——
    # 实测（stable 旧布局 / 钉死的 nightly v2 / `CARGO_BUILD_BUILD_DIR` 搬迁，三种配置 cargo 都会写）：
    # 一旦被删，后续多次 `cargo build` **都不会补写**，于是 1.5 GB 缓存会被备份软件照单全收，
    # 而 `legacyBuildDirs` 的识别判据（见下）在遗留目录上也会失效。
    'CACHEDIR.TAG'
)
$keepFull = @($keepRel | ForEach-Object { (Join-Path $target $_).ToLowerInvariant() })

# ---------------------------------------------------------------------------
# 生效的**构建目录**（中间产物）：默认等于 target\，可被 CARGO_BUILD_BUILD_DIR 或
# `.cargo/config.toml` 的 [build] build-dir 搬走（Cargo 1.91+）。
# 自清洁必须跟着它走，否则最大的那块缓存会被漏掉。
# ---------------------------------------------------------------------------
function Resolve-BuildDir {
    $value = $env:CARGO_BUILD_BUILD_DIR
    if ([string]::IsNullOrWhiteSpace($value)) {
        $cfg = Join-Path $root '.cargo\config.toml'
        if (-not (Test-Path -LiteralPath $cfg)) { $cfg = Join-Path $root '.cargo\config' }
        if (Test-Path -LiteralPath $cfg) {
            $m = [regex]::Match((Get-Content -LiteralPath $cfg -Raw), '(?ms)^\[build\].*?^\s*build-dir\s*=\s*"([^"]+)"')
            if ($m.Success) { $value = $m.Groups[1].Value }
        }
    }
    if ([string]::IsNullOrWhiteSpace($value)) { return $target }
    # 相对路径按**工作区根**解析（与 Cargo 的语义一致）
    if ([System.IO.Path]::IsPathRooted($value)) { return $value }
    return (Join-Path $root $value)
}
$buildDir = Resolve-BuildDir
$buildDirIsTarget = $buildDir.TrimEnd('\') -ieq $target.TrimEnd('\')

# ---------------------------------------------------------------------------
# 0b. **历史遗留**的构建目录：曾经用 CARGO_BUILD_BUILD_DIR 搬迁过、但现在没有生效的那些。
#
# 为什么需要：只按「当前生效的构建目录」清理会漏掉搬迁实验留下的目录
# （本轮实测踩到：最早一次 CARGO_BUILD_BUILD_DIR=build 的实验留下 538 MB，而脚本看不到它）。
# 判据用 CACHEDIR.TAG —— cargo 会在自己的构建/目标目录里写这个标记文件，
# 因此不会误删同名但无关的普通目录。
# ---------------------------------------------------------------------------
$legacyBuildDirs = @()
foreach ($cand in @((Join-Path $root 'build'), (Join-Path $root 'build-layout-probe'))) {
    if ($cand.TrimEnd([char]92) -ieq $buildDir.TrimEnd([char]92)) { continue }
    if (-not (Test-Path -LiteralPath $cand)) { continue }
    if (-not (Test-Path -LiteralPath (Join-Path $cand 'CACHEDIR.TAG'))) { continue }
    $legacyBuildDirs += $cand
}

# ---------------------------------------------------------------------------
# 1. 工具函数
# ---------------------------------------------------------------------------
function Get-PathSize([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return [pscustomobject]@{ Files = 0; Bytes = [long]0 } }
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) { return [pscustomobject]@{ Files = 0; Bytes = [long]0 } }
    if (-not $item.PSIsContainer) { return [pscustomobject]@{ Files = 1; Bytes = [long]$item.Length } }
    $m = Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum
    $bytes = if ($null -eq $m.Sum) { [long]0 } else { [long]$m.Sum }
    return [pscustomobject]@{ Files = [int]$m.Count; Bytes = $bytes }
}

function Format-MB([long]$Bytes) { return ('{0,10:N1} MB' -f ($Bytes / 1MB)) }

function Get-Rel([string]$Path) {
    $p = $Path
    if ($p.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) { $p = $p.Substring($root.Length).TrimStart('\') }
    return $p
}

function New-Item2([string]$Kind, [string]$Path) {
    $s = Get-PathSize $Path
    # 标签：target 内的项相对仓库根（便于对照 cargo 目录结构）；仓库根与 %TEMP% 的项只显示名字。
    $label = if ($Kind -eq 'cache' -or $Kind -eq 'all') { Get-Rel $Path } else { Split-Path -Leaf $Path }
    return [pscustomobject]@{
        Kind  = $Kind
        Path  = $Path
        Label = $label
        Files = $s.Files
        Bytes = $s.Bytes
    }
}

# target\ 缓存计划：**除保留项外全部清理**（对 cargo 新增目录天然免疫）
function Add-CachePlan($plan, [string]$Dir) {
    foreach ($child in (Get-ChildItem -LiteralPath $Dir -Force -ErrorAction SilentlyContinue)) {
        $full = $child.FullName.ToLowerInvariant()
        if ($keepFull -contains $full) { continue }
        if ($child.PSIsContainer) {
            $inner = @($keepFull | Where-Object { $_.StartsWith($full + '\') })
            if ($inner.Count -gt 0) { Add-CachePlan $plan $child.FullName; continue }
        }
        $plan.Add((New-Item2 'cache' $child.FullName))
    }
}

function Add-PatternPlan($plan, [string]$Kind, [string]$Dir, [string[]]$Patterns) {
    if (-not (Test-Path -LiteralPath $Dir)) { return }
    foreach ($entry in (Get-ChildItem -LiteralPath $Dir -Force -ErrorAction SilentlyContinue)) {
        foreach ($pattern in $Patterns) {
            if ($entry.Name -like $pattern) { $plan.Add((New-Item2 $Kind $entry.FullName)); break }
        }
    }
}

# ---------------------------------------------------------------------------
# 2. 采集与报告
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '===== DSHLauncher 工作区自清洁报告 =====' -ForegroundColor Cyan

Write-Host ''
Write-Host '-- 1) target\（最终产物 + 构建缓存；-Cache / -All 管辖）--' -ForegroundColor Cyan
if (-not $buildDirIsTarget) {
    $bs = Get-PathSize $buildDir
    Write-Host ('  生效的构建目录（中间产物）: ' + $buildDir) -ForegroundColor Yellow
    Write-Host ('  {0,-42} {1,8} 个文件 {2}' -f '  ↳ 构建缓存合计', $bs.Files, (Format-MB $bs.Bytes)) -ForegroundColor Yellow
    if ($buildDir.StartsWith($target, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Host '  （位于 target\ 之下，已包含在上面的构成里）' -ForegroundColor DarkGray
    } else {
        Write-Host '  ⚠ 它**不在** target\ 之下：只清 target\ 的自清洁工具会漏掉这一整块。' -ForegroundColor Yellow
    }
}
if ($legacyBuildDirs.Count -gt 0) {
    foreach ($cand in $legacyBuildDirs) {
        $ls = Get-PathSize $cand
        Write-Host ('  历史搬迁的构建目录: ' + $cand + ' —— ' + $ls.Files + ' 个文件 ' + (Format-MB $ls.Bytes)) -ForegroundColor Yellow
    }
    Write-Host '  ⚠ 这些目录当前**未生效**（CARGO_BUILD_BUILD_DIR 未指向它们），但确实含 CACHEDIR.TAG（cargo 构建缓存）。' -ForegroundColor Yellow
}

$targetTotal = Get-PathSize $target
if ($targetTotal.Files -eq 0) {
    Write-Host '  （target\ 不存在 ⇒ 已是最干净状态）' -ForegroundColor Green
} else {
    Write-Host ('  {0,-30} {1,8} 个文件 {2}' -f 'target\（合计）', $targetTotal.Files, (Format-MB $targetTotal.Bytes))
    foreach ($sub in (Get-ChildItem -LiteralPath $target -Force -ErrorAction SilentlyContinue | Sort-Object Name)) {
        $s = Get-PathSize $sub.FullName
        Write-Host ('  {0,-30} {1,8} 个文件 {2}' -f ('  ' + $sub.Name), $s.Files, (Format-MB $s.Bytes))
    }
    $keepBytes = [long]0
    $missingKeep = @()
    foreach ($k in $keepRel) {
        $p = Join-Path $target $k
        if (Test-Path -LiteralPath $p) { $keepBytes += (Get-Item -LiteralPath $p).Length } else { $missingKeep += $k }
    }
    Write-Host ('  保留（交付/校验依赖）: ' + ($keepRel -join '、')) -ForegroundColor DarkGray
    Write-Host ('  {0,-30} {1,8}            {2}' -f '  ↳ 保留项合计', $keepRel.Count, (Format-MB $keepBytes)) -ForegroundColor DarkGray
    if ($missingKeep.Count -gt 0) {
        Write-Host ('  ⚠ release 交付物缺失：' + ($missingKeep -join '、') + ' ⇒ 先 cargo build --release --locked') -ForegroundColor Yellow
    }
}

Write-Host ''
Write-Host '-- 2) 仓库根残留（-Repo 管辖）--' -ForegroundColor Cyan
$repoPatterns = @('DSHLauncher.exe.bak-*', 'DSHLauncher.exe.old-*', '*.pubtmp', '*.replaced-old', 'setup.log', 'setup.detect.log', 'selftest.log')
$repoPlanPreview = New-Object System.Collections.Generic.List[object]
Add-PatternPlan $repoPlanPreview 'repo' $root $repoPatterns
if ($repoPlanPreview.Count -eq 0) { Write-Host '  （无残留）' -ForegroundColor Green }
foreach ($i in $repoPlanPreview) { Write-Host ('  {0,-42} {1,8} 个文件 {2}' -f $i.Label, $i.Files, (Format-MB $i.Bytes)) }

Write-Host ''
Write-Host '-- 3) %TEMP% 自检残留（-Temp 管辖；白名单制）--' -ForegroundColor Cyan
# 白名单覆盖仓库里**所有**会往 %TEMP% 写东西的路径（单测临时目录、卸载器副本、自检探针）。
# 新增临时目录时请一并登记，否则它只会出现在「未识别」清单里（不会被删）。
$tempWhitelist = @(
    'dsh-install-sim',           # tools/verify-install-layout-dryrun.ps1
    'dsh-start-*',               # crates/dsh-app/src/service.rs 的单测日志目录
    'dsh_log_*',                 # crates/dsh-core/src/log.rs 的单测目录
    'dsh_lock_*',                # maintenance.rs 的锁文件单测目录
    'dsh_ws_*',                  # maintenance.rs 的 workspace.json 单测目录
    'dsh_cfg_*',                 # config.rs 的单测临时文件
    'dsh_test_nodejs*',          # dsh.rs 的单测 node 目录
    'dsh-uninstall-*.exe',       # 卸载器两阶段自删除的 %TEMP% 副本（重启后由系统清理）
    'dsh-uninstall-probe*',      # 早期轮次的身份判定探针产物
    'dsh-setup-syntax-check.exe',
    'dsh-web-help*.txt'
)
# 绝不触碰：DSH 运行时正在使用的目录（删掉会破坏正在进行的会话/本工具的落盘文件）
$tempNever = @('dsh-spill-*', 'dsh-subprocess-*')
$tempPlanPreview = New-Object System.Collections.Generic.List[object]
Add-PatternPlan $tempPlanPreview 'temp' $env:TEMP $tempWhitelist
if ($tempPlanPreview.Count -eq 0) { Write-Host '  （无残留）' -ForegroundColor Green }
foreach ($i in $tempPlanPreview) { Write-Host ('  {0,-42} {1,8} 个文件 {2}' -f $i.Label, $i.Files, (Format-MB $i.Bytes)) }
$tempUnknown = @(Get-ChildItem -LiteralPath $env:TEMP -Force -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^dsh[-_]' } |
    Where-Object { $n = $_.Name; -not ($tempWhitelist | Where-Object { $n -like $_ }) } |
    Where-Object { $n = $_.Name; -not ($tempNever | Where-Object { $n -like $_ }) })
if ($tempUnknown.Count -gt 0) {
    Write-Host ('  未识别（**不动**，仅供留意）：' + (($tempUnknown | ForEach-Object { $_.Name }) -join '、')) -ForegroundColor DarkGray
}
Write-Host ('  永不触碰：' + ($tempNever -join '、') + '（DSH 运行时正在使用）') -ForegroundColor DarkGray

Write-Host ''
Write-Host '-- 4) 运行期数据（只报告，**永不删除**）--' -ForegroundColor Cyan
$runtime = Join-Path $env:LOCALAPPDATA 'DSHLauncher'
if (Test-Path -LiteralPath $runtime) {
    $rs = Get-PathSize $runtime
    Write-Host ('  {0,-42} {1,8} 个文件 {2}' -f $runtime, $rs.Files, (Format-MB $rs.Bytes))
    Write-Host '  说明：日志 / WebView2 profile / service.json —— 属卸载器职责（--purge 才连用户配置一起删）。' -ForegroundColor DarkGray
} else {
    Write-Host '  （不存在）' -ForegroundColor Green
}

# ---------------------------------------------------------------------------
# 3. 默认：只报告，不删任何东西
# ---------------------------------------------------------------------------
if (-not ($Cache -or $All -or $Repo -or $Temp)) {
    Write-Host ''
    Write-Host '只报告，未删除任何内容。要执行清理请显式选择：' -ForegroundColor Yellow
    Write-Host '  pwsh -File tools\clean.ps1 -Cache              # 清 target 缓存（保留 release 交付物，推荐）' -ForegroundColor Gray
    Write-Host '  pwsh -File tools\clean.ps1 -Repo               # 清仓库根残留（副本 / 旧产物 / 日志）' -ForegroundColor Gray
    Write-Host '  pwsh -File tools\clean.ps1 -Temp               # 清 %TEMP% 自检残留（白名单）' -ForegroundColor Gray
    Write-Host '  pwsh -File tools\clean.ps1 -All                # 全清 target\（之后必须重新构建）' -ForegroundColor Gray
    Write-Host '  pwsh -File tools\clean.ps1 -Cache -WhatIf      # 先看会删什么' -ForegroundColor Gray
    exit 0
}

# ---------------------------------------------------------------------------
# 4. 生成计划
# ---------------------------------------------------------------------------
$plan = New-Object System.Collections.Generic.List[object]

# 被搬迁的构建目录（Cargo 1.91+ 的 build-dir）：整块都是可重建的中间产物。
# 它若位于 target 之下，上面的递归已覆盖；若在 target 之外，必须单独列一项 ——
# 否则「自清洁」会漏掉最大的一块（实测：搬走后 <dir> 538 MB、target 只剩 7.6 MB）。
$buildDirSeparate = (-not $buildDirIsTarget) -and
    (-not $buildDir.StartsWith($target + '\', [System.StringComparison]::OrdinalIgnoreCase)) -and
    (-not $target.StartsWith($buildDir + '\', [System.StringComparison]::OrdinalIgnoreCase))

if ($All) {
    if (Test-Path -LiteralPath $target) { $plan.Add((New-Item2 'all' $target)) }
    if ($buildDirSeparate -and (Test-Path -LiteralPath $buildDir)) { $plan.Add((New-Item2 'all' $buildDir)) }
    foreach ($cand in $legacyBuildDirs) { $plan.Add((New-Item2 'all' $cand)) }
} elseif ($Cache) {
    if (-not (Test-Path -LiteralPath $target)) {
        Write-Host ''
        Write-Host 'target\ 不存在：没有缓存可清（也就无需 -Cache）。' -ForegroundColor Green
        exit 0
    }
    $missing = @($keepRel | Where-Object { -not (Test-Path -LiteralPath (Join-Path $target $_)) })
    if ($missing.Count -gt 0) {
        Write-Host ''
        Write-Host ('拒绝执行 -Cache：release 交付物缺失（' + ($missing -join '、') + '）。') -ForegroundColor Red
        Write-Host '  原因：docs/FACTS.json 的 artifacts.launcher_build 与 tools/verify-version.ps1' -ForegroundColor Red
        Write-Host '        都以它们为输入；先构建再清缓存，顺序反了会让下游门禁全部变红。' -ForegroundColor Red
        Write-Host '  正确顺序：cargo build --release --locked  →  pwsh -File tools\clean.ps1 -Cache' -ForegroundColor Yellow
        exit 2
    }
    Add-CachePlan $plan $target
    if ($buildDirSeparate -and (Test-Path -LiteralPath $buildDir)) { $plan.Add((New-Item2 'cache' $buildDir)) }
    foreach ($cand in $legacyBuildDirs) { $plan.Add((New-Item2 'cache' $cand)) }
}
if ($Repo) { Add-PatternPlan $plan 'repo' $root $repoPatterns }
if ($Temp) { Add-PatternPlan $plan 'temp' $env:TEMP $tempWhitelist }

if ($plan.Count -eq 0) {
    Write-Host ''
    Write-Host '没有可清理的内容（已是最干净状态）' -ForegroundColor Green
    exit 0
}

$reclaim = [long](($plan | Measure-Object -Property Bytes -Sum).Sum)
Write-Host ''
Write-Host ('将删除 {0} 项，可回收约 {1:N1} MB：' -f $plan.Count, ($reclaim / 1MB)) -ForegroundColor Yellow
foreach ($item in ($plan | Sort-Object Bytes -Descending)) {
    Write-Host ('  [{0,-5}] {1,-40} {2,8} 个文件 {3}' -f $item.Kind, $item.Label, $item.Files, (Format-MB $item.Bytes)) -ForegroundColor DarkGray
}
# 只有真的动了 target\ 才需要说明保留项（-Repo/-Temp 与它无关）
if (@($plan | Where-Object { $_.Kind -eq 'cache' -or $_.Kind -eq 'all' }).Count -gt 0) {
    Write-Host ('  保留：' + ($keepRel -join '、')) -ForegroundColor DarkGray
}

if ($WhatIf) {
    Write-Host ''
    Write-Host '-WhatIf：以上内容**未**被删除。' -ForegroundColor Yellow
    exit 0
}

# ---------------------------------------------------------------------------
# 5. 执行（区分「失败」与「被运行实例占用 ⇒ 跳过」）
# ---------------------------------------------------------------------------
$launcherRunning = @(Get-Process -Name 'DSHLauncher' -ErrorAction SilentlyContinue).Count -gt 0
$failed = New-Object System.Collections.Generic.List[string]
$skipped = New-Object System.Collections.Generic.List[string]
foreach ($item in $plan) {
    try {
        Remove-Item -LiteralPath $item.Path -Recurse -Force -ErrorAction Stop
    } catch {
        # 只有 target / 构建目录内的删除失败才算**失败**（那是本工具对「缓存已回收」的承诺）。
        # 仓库根与 %TEMP% 的锁定属平台语义：热替换副本是**运行中实例的映像**（已映射的镜像
        # 在 Windows 下删不掉），%TEMP% 的卸载器副本可能正被第二阶段卸载流程使用；
        # 这两类一律降级为「跳过」并说明何时可回收，避免污染退出码。
        if ($item.Kind -eq 'cache' -or $item.Kind -eq 'all') {
            $failed.Add($item.Label + '：' + $_.Exception.Message)
        } else {
            $skipped.Add($item.Label)
        }
    }
}

# ---------------------------------------------------------------------------
# 6. 后置校验（清完必须仍然可发布）
# ---------------------------------------------------------------------------
if ($Cache -and -not $All) {
    foreach ($k in $keepRel) {
        if (-not (Test-Path -LiteralPath (Join-Path $target $k))) {
            $failed.Add('保留项被误删：target\' + $k + '（FACTS / verify-version.ps1 会因此失败）')
        }
    }
}

Write-Host ''
Write-Host ('回收完成：约 {0:N1} MB；target\ 现为 {1:N1} MB' -f ($reclaim / 1MB), ((Get-PathSize $target).Bytes / 1MB)) -ForegroundColor Green

if ($skipped.Count -gt 0) {
    Write-Host ''
    Write-Host ('跳过 {0} 项（被占用：运行中实例的映像，或正在被使用的卸载器副本）：' -f $skipped.Count) -ForegroundColor Yellow
    $skipped | ForEach-Object { Write-Host ('  - ' + $_) -ForegroundColor Yellow }
    if ($launcherRunning) { Write-Host '  提示：退出该启动器后重跑本命令即可回收（或留待下次热替换时由 build.ps1 自动清理）。' -ForegroundColor Gray }
}

# FACTS 是否仍然成立：这是「清缓存有没有破坏事实文件」的直接答案
$factsPath = Join-Path $root 'docs\FACTS.json'
$exe = Join-Path $target 'release\dsh-app.exe'
if ((Test-Path -LiteralPath $factsPath) -and (Test-Path -LiteralPath $exe)) {
    try {
        $facts = Get-Content -LiteralPath $factsPath -Raw | ConvertFrom-Json
        $recorded = [long]$facts.artifacts.launcher_build.bytes
        $actual = [long](Get-Item -LiteralPath $exe).Length
        if ($recorded -eq $actual) {
            Write-Host ('FACTS 自校验：artifacts.launcher_build = {0} 字节，与实测一致 ✔' -f $actual) -ForegroundColor Green
        } else {
            Write-Host ('FACTS 自校验：记录 {0} 字节 / 实测 {1} 字节 —— 请跑 pwsh -File tools\gen-facts.ps1 重新实测' -f $recorded, $actual) -ForegroundColor Yellow
        }
    } catch {
        Write-Host ('FACTS 自校验跳过：' + $_.Exception.Message) -ForegroundColor DarkGray
    }
}

Write-Host ''
if ($failed.Count -gt 0) {
    Write-Host ('清理未完全成功：{0} 项失败' -f $failed.Count) -ForegroundColor Red
    $failed | ForEach-Object { Write-Host ('  - ' + $_) -ForegroundColor Red }
    exit 1
}

# 下一步指引：按模式给出「哪些门禁仍可跑、哪些需要先构建」
if ($All) {
    Write-Host '下一步：target\ 已全清 ⇒ 先 `cargo build --release --locked`，再 `pwsh tools\gen-facts.ps1`' -ForegroundColor Yellow
    Write-Host '        （FACTS 记录 release 产物，未构建时 gen-facts -Check 必然报漂移）。' -ForegroundColor Gray
} elseif ($Cache) {
    Write-Host '下一步：无需重建即可跑的通路 —— check-consistency / gen-facts -Check / verify-version /' -ForegroundColor Gray
    Write-Host '        模拟安装 dry-run 与 --version 核对；下次 cargo build|test 会重新编译依赖（首次较慢）。' -ForegroundColor Gray
    Write-Host '        提示：把 release 交付物保留在 target\ 是刻意的 —— 删掉它们会让上面两条门禁变红。' -ForegroundColor DarkGray
} else {
    Write-Host '下一步：无（本模式未触碰 target\，所有门禁不受影响）。' -ForegroundColor Gray
}
exit 0
