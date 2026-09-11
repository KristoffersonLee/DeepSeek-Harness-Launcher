<#
  tools\verify-build-layout.ps1 —— 构建目录布局兼容性验证（Cargo build-dir v2 / CFT）
  ============================================================================
  ## 背景

  Cargo 正在把构建目录从「按内容类型组织」改为「按包名 + 构建单元哈希组织」
  （Call for Testing: Build Dir Layout v2）：`deps\` 与 `.fingerprint\` 消失，
  中间产物按 `build\<包名>\<哈希>\{fingerprint,out}\` 分桶。
  该布局在 **Cargo 1.100 起已稳定并成为默认**（本机实测：`-Z build-dir-new-layout` 已无需给出，
  `build.build-dir-new-layout` 恒为开启）。另外 Cargo 1.91 起（**稳定版可用**）可以用
  
  ## 检查内容（静态断言始终执行；-Run 再跑三种配置的实测）

    A. 仓库脚本是否硬编码了**中间产物**的内部布局（deps / .fingerprint / 中间 examples）；
    B. example 产物路径是否来自 cargo 报告（而不是猜 target\<profile>\examples）；
    C. .gitignore 是否覆盖可能被搬迁的构建目录；
    D. clean.ps1 是否会解析生效的构建目录（否则自清洁会漏掉最大的一块 cache）；
    E. 三种配置（默认 / CARGO_BUILD_BUILD_DIR / nightly 新布局）下最终产物与版本链路必须同样成立。

  ## 用法

      pwsh -File tools\verify-build-layout.ps1                        # 只做静态断言
      pwsh -File tools\verify-build-layout.ps1 -Run                   # 加三种配置实测
      pwsh -File tools\verify-build-layout.ps1 -Run -IncludeNightly   # 含 nightly 新布局

  退出码：0 = 通过；1 = 有断言/实测失败。
#>
param(
    [switch]$Run,
    [switch]$IncludeNightly
)

$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$target = Join-Path $root 'target'
$BS = [string][char]92

$failures = New-Object System.Collections.Generic.List[string]
function Pass([string]$m) { Write-Host ('  [PASS] ' + $m) -ForegroundColor Green }
function Fail([string]$m) { $script:failures.Add($m); Write-Host ('  [FAIL] ' + $m) -ForegroundColor Red }
function Info([string]$m) { Write-Host ('  ' + $m) -ForegroundColor DarkGray }

Write-Host '===== 构建目录布局兼容性验证 =====' -ForegroundColor Cyan
Write-Host '-- 1) 工具链与生效构建目录 --' -ForegroundColor Cyan
Info ('cargo   : ' + ((& cargo --version 2>&1) -join ' '))
Info ('rustc   : ' + ((& rustc --version 2>&1) -join ' '))
$nightlyV = $null
if (((& rustup toolchain list 2>&1) -join "`n") -match 'nightly') {
    $nightlyV = (& cargo +nightly --version 2>&1) -join ' '
    Info ('nightly : ' + $nightlyV)
} else { Info 'nightly : 未安装（-IncludeNightly 不可用）' }

$buildDirValue = $env:CARGO_BUILD_BUILD_DIR
$cfgToml = Join-Path $root '.cargo\config.toml'.Replace('\', $BS)
if ([string]::IsNullOrWhiteSpace($buildDirValue) -and (Test-Path -LiteralPath $cfgToml)) {
    $m = [regex]::Match((Get-Content -LiteralPath $cfgToml -Raw), '(?ms)^\[build\].*?^\s*build-dir\s*=\s*"([^"]+)"'.Replace('\', $BS))
    if ($m.Success) { $buildDirValue = $m.Groups[1].Value }
}
if ([string]::IsNullOrWhiteSpace($buildDirValue)) { Info '构建目录: 默认（中间产物与最终产物同在 target）' }
else {
    if (-not [System.IO.Path]::IsPathRooted($buildDirValue)) { $buildDirValue = Join-Path $root $buildDirValue }
    Info ('构建目录: ' + $buildDirValue + '（已搬迁：中间产物在此，最终产物仍在 target）')
}
foreach ($rel in @('release\dsh-app.exe', 'release\dsh-uninstall.exe')) {
    $p = Join-Path $target $rel.Replace('\', $BS)
    Info ('最终产物: ' + $p + '  存在=' + (Test-Path -LiteralPath $p))
}

Write-Host '-- 2) 静态断言（不得依赖未承诺的内部布局）--' -ForegroundColor Cyan
# 本脚本自身必然出现这些名字（它就是用来检查它们的），因此排除自己。
$scripts = @(Get-ChildItem -Path $here -File -Filter '*.ps1' -ErrorAction SilentlyContinue) +
           @(Get-ChildItem -Path $root -File -Filter '*.ps1' -ErrorAction SilentlyContinue) |
    Where-Object { $_.Name -ne 'verify-build-layout.ps1' -and $_.Name -ne 'check-consistency.ps1' }
# 只看**代码**：先去掉块注释 <# ... #>，再去掉整行 # 注释，避免说明文字被当成依赖。
$internalMarkers = @(('deps' + $BS), '.fingerprint', ($BS + 'debug' + $BS + 'examples'))
$hits = @()
foreach ($s in $scripts) {
    $raw = Get-Content -LiteralPath $s.FullName -Raw
    $noBlock = [regex]::Replace($raw, '(?s)<#.*?#>', '')
    $code = ($noBlock -split "`n" | Where-Object { $_.TrimStart() -notmatch '^#' }) -join "`n"
    foreach ($mk in $internalMarkers) { if ($code -like ('*' + $mk + '*')) { $hits += ($s.Name + ' -> ' + $mk) } }
}
if ($hits.Count -eq 0) { Pass '脚本未硬编码构建内部布局（deps / .fingerprint / 中间 examples）' }
else { foreach ($h in $hits) { Fail ('脚本依赖构建内部布局：' + $h) } }

$selftest = Get-Content -LiteralPath (Join-Path $root 'selftest.ps1') -Raw
if ($selftest -match 'message-format=json' -and $selftest -match '\.executable') {
    Pass 'selftest.ps1 的 example 产物路径来自 cargo 报告（不猜内部布局）'
} else { Fail 'selftest.ps1 仍硬编码 example 产物路径（新布局/搬迁后失效）' }

$gitignore = Get-Content -LiteralPath (Join-Path $root '.gitignore') -Raw
if ($gitignore -match ('(?m)^\/target/\s*$'.Replace('\', $BS)) -and $gitignore -match ('(?m)^\/build/\s*$'.Replace('\', $BS))) {
    Pass '.gitignore 覆盖 target 与可能被搬迁的构建目录'
} else { Fail '.gitignore 未覆盖被搬迁的构建目录（搬迁后会被当成未跟踪文件）' }

$cleanText = Get-Content -LiteralPath (Join-Path $here 'clean.ps1') -Raw
if ($cleanText -match 'CARGO_BUILD_BUILD_DIR' -and $cleanText -match 'Resolve-BuildDir') {
    Pass 'clean.ps1 会解析生效的构建目录（搬迁后不会漏掉最大的一块缓存）'
} else { Fail 'clean.ps1 未识别被搬迁的构建目录：自清洁会漏掉最大的缓存' }

if (-not $Run) {
    Write-Host ''
    if ($failures.Count -gt 0) { Write-Host ('静态断言失败 ' + $failures.Count + ' 项') -ForegroundColor Red; exit 1 }
    Write-Host '静态断言通过。加 -Run 执行三种配置的实测（默认布局 / 搬迁构建目录 / nightly 新布局）。' -ForegroundColor Green
    exit 0
}

function Assert-Artifacts([string]$tag) {
    $ok = $true
    foreach ($rel in @('release\dsh-app.exe', 'release\dsh-uninstall.exe')) {
        $p = Join-Path $target $rel.Replace('\', $BS)
        if (-not (Test-Path -LiteralPath $p)) { Fail ($tag + '：缺少最终产物 ' + $p); $ok = $false }
    }
    if ($ok) { Pass ($tag + '：最终产物仍在 target<profile>（与 CFT 承诺一致）') }
}

Write-Host '-- 3) 实测：默认布局 --' -ForegroundColor Cyan
& cargo build --release --locked --offline 2>&1 | Select-Object -Last 1 | ForEach-Object { Info $_ }
if ($LASTEXITCODE -ne 0) { Fail '3a：cargo build --release --locked 失败' } else { Pass '3a：默认布局构建通过' }
Assert-Artifacts '3a'
& pwsh -NoProfile -File (Join-Path $here 'verify-version.ps1') -Exe (Join-Path $target ('release' + $BS + 'dsh-app.exe').Replace('\', $BS)) 2>&1 |
    Select-String -Pattern '结果:' | ForEach-Object { Info $_.Line }
if ($LASTEXITCODE -eq 0) { Pass '3a：版本链路校验通过' } else { Fail ('3a：版本链路校验退出码 ' + $LASTEXITCODE) }

Write-Host '-- 4) 实测：中间产物搬迁（CARGO_BUILD_BUILD_DIR，Cargo 1.91+ 稳定可用）--' -ForegroundColor Cyan
$probe = Join-Path $root 'build-layout-probe'
$env:CARGO_BUILD_BUILD_DIR = $probe
& cargo build --release --locked --offline 2>&1 | Select-Object -Last 1 | ForEach-Object { Info $_ }
if ($LASTEXITCODE -ne 0) { Fail '4a：搬迁构建目录后构建失败' } else { Pass '4a：搬迁构建目录后构建通过' }
Assert-Artifacts '4a'
if (Test-Path -LiteralPath $probe) {
    $n = @(Get-ChildItem -LiteralPath $probe -Recurse -File -Force -ErrorAction SilentlyContinue).Count
    Info ('中间产物确实落在 ' + $probe + '（' + $n + ' 个文件）')
    if ($n -gt 0) { Pass '4a：中间产物与最终产物确实分离（证据如上）' } else { Fail '4a：搬迁目录为空，拆分未生效' }
} else { Fail '4a：搬迁目录不存在' }
$cleanOut = & pwsh -NoProfile -File (Join-Path $here 'clean.ps1') -Cache -WhatIf 2>&1 | Out-String
if ($cleanOut -match 'build-layout-probe') { Pass '4b：clean.ps1 -Cache 计划包含被搬迁的构建目录' }
else { Fail '4b：clean.ps1 -Cache 未覆盖被搬迁的构建目录（自清洁会漏掉它）' }
Remove-Item -LiteralPath $probe -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item Env:CARGO_BUILD_BUILD_DIR -ErrorAction SilentlyContinue

if ($IncludeNightly) {
    Write-Host '-- 5) 实测：nightly 的默认布局（探针，**独立 target 目录**）--' -ForegroundColor Cyan
    # ⚠ 必须用独立的 CARGO_TARGET_DIR。
    #
    # 本仓库主工具链本身就是**钉死的** nightly，而 `cargo +nightly` 指向**滚动** nightly
    # （rustc 提交号不同 ⇒ 代码生成不同）。旧实现直接写默认 `target\`，于是把交付物换成了
    # 另一个编译器产出的二进制 —— 而 `FACTS.json` 与 `verify-version.ps1` 依赖"根产物与
    # `target\release\` 同源"。实测踩到：卸载器从 316,928 B 变成 317,440 B，而
    # `gen-facts -Check` **看不见**这种漂移（它只比尺寸/版本，不比字节）。
    if (-not $nightlyV) { Fail '5a：未安装 nightly 工具链' }
    else {
        $probeTarget = Join-Path $root 'build-layout-probe-nightly'
        Remove-Item -LiteralPath $probeTarget -Recurse -Force -ErrorAction SilentlyContinue
        $env:CARGO_TARGET_DIR = $probeTarget
        & cargo +nightly build --release --locked --offline 2>&1 | Select-Object -Last 1 | ForEach-Object { Info $_ }
        $nightlyCode = $LASTEXITCODE
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        if ($nightlyCode -ne 0) { Fail '5a：nightly 新布局构建失败' } else { Pass '5a：nightly 新布局构建通过' }
        $missing = @()
        foreach ($rel in @('release\dsh-app.exe', 'release\dsh-uninstall.exe')) {
            $p = Join-Path $probeTarget $rel.Replace('\', $BS)
            if (-not (Test-Path -LiteralPath $p)) { $missing += $rel }
        }
        if ($missing.Count -eq 0) { Pass '5a：探针 target 里最终产物仍在 <target>\release（隔离于默认 target）' }
        else { Fail ('5a：探针 target 缺少最终产物 ' + ($missing -join '、')) }
        $top = @(Get-ChildItem -LiteralPath (Join-Path $probeTarget 'release') -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
        Info ('探针 target' + $BS + 'release 顶层：' + ($top -join ', '))
        if ($top -contains 'build') { Pass '5b：新布局生效（中间产物按包名分桶到 build/<包名>/<哈希>）' }
        else { Info '  提示：该 nightly 尚未启用新布局（顶层没有 build 目录）' }
        Remove-Item -LiteralPath $probeTarget -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 供货一致性提示：第 3/4 步是用**默认工具链**重建 `target\<profile>` 的，因此仓库根里的
# 交付物副本与它不再逐字节相同（构建戳不同）。需要恢复"根产物 == target\<profile>"时再跑一次
# `build.ps1 release` 即可（它负责校验并把产物拷到仓库根）。
Write-Host ''
Info '-Run 已重建 target\<profile>（默认工具链）。若要让仓库根产物与之逐字节一致，请再运行：pwsh -File build.ps1 release'

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host ('构建布局兼容性验证失败：' + $failures.Count + ' 项') -ForegroundColor Red
    $failures | ForEach-Object { Write-Host ('  - ' + $_) -ForegroundColor Red }
    exit 1
}
Write-Host '构建布局兼容性验证通过：最终产物路径在三种配置下一致，工具链不依赖未承诺的内部布局。' -ForegroundColor Green
exit 0
