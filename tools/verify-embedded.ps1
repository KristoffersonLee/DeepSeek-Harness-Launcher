# tools\verify-embedded.ps1 —— 校验「安装包内嵌资源 == 当前产物」（逐字节）
#
# ## 为什么需要它（本轮实测踩到的盲区）
#
# 安装包 `DSHLauncherSetup.exe` 把 6 个文件作为 **.NET 内嵌资源**打包进去：启动器、卸载器、
# 图标、README、中英维护手册（见 `build-setup.ps1` 的 `/resource:` 列表）。
# 此前**没有任何校验覆盖"内嵌的是不是当前产物"** —— 实测踩到：给 `README.md` 加了「文档地图」
# 之后没有再打包，安装包内嵌的仍是旧 README（41,763 B vs 磁盘 46,086 B），而当时所有既有校验
# （`verify-version.ps1` / `check-consistency.ps1` / `finish-release.ps1`）**全绿**。
# 后果：用户装出来的 README 是旧的，而发布流程报"通过"。
#
# ## 为什么必须用 Windows PowerShell 5.1 运行
#
# 安装包是 **.NET Framework (WinForms) 程序集**，PowerShell 7 跑在 .NET 8 上、加载不了它。
# 5.1 用 `[Reflection.Assembly]::LoadFile` 读元数据与资源即可（**只读，不执行其中代码**）。
# 因此 `finish-release.ps1` 调用本脚本时显式用 `powershell`（5.1），而不是 `pwsh`。
#
# ## 期望映射（与 build-setup.ps1 的 /resource: 列表逐项对应）
#
#   DSHLauncher.exe    -> <root>\DSHLauncher.exe
#   dsh-uninstall.exe  -> <root>\target\release\dsh-uninstall.exe   ← 注意取自 target，不是根副本
#   app.ico            -> <root>\app.ico
#   README.md          -> <root>\README.md
#   MAINTENANCE.zh.md  -> <root>\docs\MAINTENANCE.zh.md
#   MAINTENANCE.en.md  -> <root>\docs\MAINTENANCE.en.md
#
# ## 用法与退出码
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\verify-embedded.ps1
#   0 = 全部逐字节一致；1 = 有不一致（安装包内嵌的是旧产物）；2 = 前置条件缺失（**跳过≠通过**）
#
# ## 正负用例（证明这个校验不是"永远为真"）
#
#   负例：把"磁盘产物"换成一份改过的副本（安装包不动），本脚本必须报 FAIL：
#
#     $fx = Join-Path $env:TEMP 'dsh-embedded-fixture'
#     New-Item -ItemType Directory -Force -Path (Join-Path $fx 'docs'), (Join-Path $fx 'target\release') | Out-Null
#     Copy-Item DSHLauncher.exe,dsh-uninstall.exe,app.ico,README.md $fx -Force
#     Copy-Item target\release\dsh-uninstall.exe (Join-Path $fx 'target\release') -Force
#     Copy-Item docs\MAINTENANCE.zh.md,docs\MAINTENANCE.en.md (Join-Path $fx 'docs') -Force
#     Add-Content (Join-Path $fx 'README.md') "`n<!-- tampered -->"     # 只改夹具里的副本
#     powershell -NoProfile -ExecutionPolicy Bypass -File tools\verify-embedded.ps1 -Root $fx   # 期望 exit 1

param(
    # 留空 = 本仓库根；显式指定可对夹具做正负用例（见上）
    [string]$Root = '',
    # 留空 = <Root>\DSHLauncherSetup.exe
    [string]$Setup = ''
)

$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'
$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($Root)) { $Root = Split-Path -Parent $here }
$root = $Root
if ([string]::IsNullOrWhiteSpace($Setup)) { $setup = Join-Path $root 'DSHLauncherSetup.exe' } else { $setup = $Setup }

$expect = [ordered]@{
    'DSHLauncher.exe'   = (Join-Path $root 'DSHLauncher.exe')
    'dsh-uninstall.exe' = (Join-Path $root 'target\release\dsh-uninstall.exe')
    'app.ico'           = (Join-Path $root 'app.ico')
    'README.md'         = (Join-Path $root 'README.md')
    'MAINTENANCE.zh.md' = (Join-Path $root 'docs\MAINTENANCE.zh.md')
    'MAINTENANCE.en.md' = (Join-Path $root 'docs\MAINTENANCE.en.md')
}

function Pass($m) { Write-Host ('  [PASS] ' + $m) -ForegroundColor Green }
function Fail($m) { Write-Host ('  [FAIL] ' + $m) -ForegroundColor Red }
function Info($m) { Write-Host ('  ' + $m) -ForegroundColor DarkGray }

Write-Host '===== 安装包内嵌资源一致性校验 =====' -ForegroundColor Cyan

# ---- 前置条件（缺失时返回 2：明确区分"没验"与"验过"）----
if (-not (Test-Path -LiteralPath $setup)) {
    Write-Host ("SKIPPED: 找不到安装包 $setup（先跑 build-setup.ps1）") -ForegroundColor Yellow
    Write-Host '（SKIPPED 不等于通过：请先构建安装包后重跑）' -ForegroundColor Yellow
    exit 2
}
$missingOnDisk = @()
foreach ($k in $expect.Keys) { if (-not (Test-Path -LiteralPath $expect[$k])) { $missingOnDisk += $k } }
if ($missingOnDisk.Count -gt 0) {
    Write-Host ('SKIPPED: 磁盘上缺少待比对产物：' + ($missingOnDisk -join ', ')) -ForegroundColor Yellow
    Write-Host '（SKIPPED 不等于通过：请先跑 build.ps1 release 后重跑）' -ForegroundColor Yellow
    exit 2
}

# ---- 加载安装包并枚举内嵌资源 ----
$asm = [Reflection.Assembly]::LoadFile($setup)
$names = @($asm.GetManifestResourceNames() | Sort-Object)
Info ('安装包: ' + $setup)
Info ('内嵌资源: ' + $names.Count + ' 个 -> ' + ($names -join ', '))

$sha = [Security.Cryptography.SHA256]::Create()
$mismatch = @()
$unmapped = @()
foreach ($n in $names) {
    $st = $asm.GetManifestResourceStream($n)
    if ($null -eq $st) { Fail ("无法读取内嵌资源 $n"); $mismatch += $n; continue }
    $ms = New-Object IO.MemoryStream
    $st.CopyTo($ms)
    $bytes = $ms.ToArray()
    $embedded = ([BitConverter]::ToString($sha.ComputeHash($bytes))) -replace '-', ''
    $ms.Dispose(); $st.Dispose()

    $key = @($expect.Keys | Where-Object { $_ -ieq $n })
    if ($key.Count -eq 0) { $unmapped += $n; continue }
    $diskPath = $expect[$key[0]]
    $diskHash = (Get-FileHash -LiteralPath $diskPath -Algorithm SHA256).Hash
    $diskLen = (Get-Item -LiteralPath $diskPath).Length
    if ($embedded -eq $diskHash) {
        Pass ("$n 与磁盘产物逐字节一致（{0:N0} B）" -f $diskLen)
    } else {
        Fail ("$n 内嵌 {0:N0} B / 磁盘 {1:N0} B —— 安装包内嵌的是**旧产物**，需重新打包" -f $bytes.Length, $diskLen)
        $mismatch += $n
    }
    if ($expect[$key[0]] -like '*target\release\dsh-uninstall.exe') {
        # 根目录的副本也必须与 target 里的一致（build.ps1 负责拷贝；不一致说明根副本陈旧）
        $rootCopy = Join-Path $root 'dsh-uninstall.exe'
        if ((Get-FileHash -LiteralPath $rootCopy -Algorithm SHA256).Hash -eq $diskHash) {
            Pass 'dsh-uninstall.exe 的根目录副本与 target\release 一致'
        } else {
            Fail 'dsh-uninstall.exe 的根目录副本与 target\release **不一致**（根副本陈旧）'
            $mismatch += 'root:dsh-uninstall.exe'
        }
    }
}
if ($unmapped.Count -gt 0) { Fail ('安装包里有未纳入比对清单的资源：' + ($unmapped -join ', ')); }

Write-Host ''
if ($mismatch.Count -eq 0 -and $unmapped.Count -eq 0) {
    Write-Host ('结果: 安装包内嵌的 ' + $names.Count + ' 项资源与当前产物**逐字节一致** —— 一致 OK') -ForegroundColor Green
    exit 0
}
Write-Host ('结果: ' + ($mismatch.Count + $unmapped.Count) + ' 项不一致 —— 安装包需要重新打包') -ForegroundColor Red
exit 1
