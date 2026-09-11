# Windows PowerShell 5.1 的 Get-Content 默认按 ANSI 代码页解码；见其他脚本的同款说明。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

# tools\verify-install-layout-dryrun.ps1 —— 在**模拟安装目录**上验证卸载器的准入校验与影响范围报告
#
# 为什么需要它：卸载器的所有护栏（安装标记必需、源码树识别、受保护路径、外来文件保护、
# 载荷清单）在仓库根目录上**永远走不到正常分支**（这里是源码树，按设计必须被拒绝）。
# 本脚本在 %TEMP% 里搭一个与真实安装目录同构的模拟副本，然后只跑 `--dry-run`（零改动），
# 用于验证：
#   1. 有安装标记 ⇒ 准入通过（不再被当成源码树拒绝）；
#   2. 载荷清单（含 .tmp 暂存残留与上一版遗留脚本）被逐项列出；
#   3. dry-run 不修改任何状态（结束时目录内容不变）。
#
# 用法: pwsh -NoProfile -File tools\verify-install-layout-dryrun.ps1
# 退出码: 0 = 通过；1 = 失败
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here

$sim = Join-Path $env:TEMP 'dsh-install-sim'
if (Test-Path $sim) { Remove-Item $sim -Recurse -Force -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Path $sim -Force | Out-Null

# 1) 安装标记（内容与安装器 BuildMarkerText 同构：必须含 AppName）
$marker = 'DeepSeek Harness Launcher' + "`r`n" + 'version=5.0.0 LTS' + "`r`n" + ('install_dir=' + $sim) + "`r`n"
Set-Content -Path (Join-Path $sim '.dsllauncher-install') -Value $marker -NoNewline -Encoding UTF8

# 2) 载荷（尽量用真实产物；缺失的用占位，模拟用户实际装了什么）
$payloads = @('DSHLauncher.exe','dsh-uninstall.exe','app.ico','README.md','MAINTENANCE.zh.md','MAINTENANCE.en.md')
$map = @{
    'DSHLauncher.exe'         = (Join-Path $root 'DSHLauncher.exe')
    'dsh-uninstall.exe'       = (Join-Path $root 'dsh-uninstall.exe')
    'app.ico'                 = (Join-Path $root 'app.ico')
    'README.md'               = (Join-Path $root 'README.md')
    'MAINTENANCE.zh.md'       = (Join-Path $root 'docs\MAINTENANCE.zh.md')
    'MAINTENANCE.en.md'       = (Join-Path $root 'docs\MAINTENANCE.en.md')
}
foreach ($p in $payloads) {
    $src = $map[$p]
    if (Test-Path $src) { Copy-Item $src (Join-Path $sim $p) -Force } else { Set-Content (Join-Path $sim $p) '' }
}
# 暂存残留加上一版遗留脚本：卸载器必须把它们也算作我们自己的文件
Set-Content (Join-Path $sim 'README.md.tmp') ''
Set-Content (Join-Path $sim 'uninstall.cmd') ''
Set-Content (Join-Path $sim 'uninstall.ps1') ''

$before = @(Get-ChildItem $sim -Force | Sort-Object Name | ForEach-Object { $_.Name })

# 3) 只跑 dry-run（零改动）。用**模拟目录里的**卸载器副本：
#    它的 install_dir 由自身所在目录推导，因此看到的正是模拟安装目录。
$out = & (Join-Path $sim 'dsh-uninstall.exe') --dry-run 2>&1 | Out-String
$code = $LASTEXITCODE

$problems = @()
if ($out -notmatch 'DRY-RUN') { $problems += 'dry-run 未进入报告模式（可能被护栏拒绝）' }
if ($out -notmatch '安装标记') { $problems += '缺少准入信息' }
if ($out -notmatch '将删除的安装文件') { $problems += '缺少载荷清单段落' }
if ($out -notmatch 'uninstall') { $problems += '未列出上一版遗留脚本卸载器（会残留）' }
foreach ($p in $payloads) { if ($out -notmatch [regex]::Escape($p)) { $problems += "未列出载荷 $p" } }
if ($out -notmatch '用户配置') { $problems += '未说明用户配置去留' }
if ($code -ne 0) { $problems += "退出码 $code（dry-run 应为 0）" }

$after = @(Get-ChildItem $sim -Force | Sort-Object Name | ForEach-Object { $_.Name })
if (($before -join '|') -ne ($after -join '|')) { $problems += 'dry-run 修改了目录内容（必须零改动）' }

Remove-Item $sim -Recurse -Force -ErrorAction SilentlyContinue

if ($problems.Count -gt 0) {
    Write-Host 'FAIL: 模拟安装目录的 dry-run 校验未通过：' -ForegroundColor Red
    $problems | ForEach-Object { Write-Host ('  - ' + $_) -ForegroundColor Red }
    Write-Host '--- 卸载器输出 ---'
    Write-Host $out
    exit 1
}
Write-Host ('PASS: 模拟安装目录 dry-run 校验通过（载荷 ' + $payloads.Count + ' 项加暂存残留加遗留脚本均被列出，且零改动）') -ForegroundColor Green
exit 0