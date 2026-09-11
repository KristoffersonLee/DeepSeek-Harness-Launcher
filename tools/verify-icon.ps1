# verify-icon.ps1 — 验证窗口标题栏图标确实来自 app.ico
#
# 用法:
#   pwsh -NoProfile -File tools\verify-icon.ps1 [-IcoPath <路径>] [-WinIconPath <PNG>] [-MaxDiff 40]
#
# 退出码:
#   0 = 通过（差异像素数 <= MaxDiff）
#   1 = 不匹配（差异像素数 > MaxDiff）
#   2 = SKIPPED（必要输入缺失：基准 ico 或窗口导出的 PNG 不存在）—— 与「通过」严格区分
#
# 说明：本脚本需要一份**窗口图标导出 PNG**（由外部导出步骤产生，形如
# %TEMP%\dsh_icon_check\DSHLauncher.png）。该 PNG 不存在时**不能**算通过，
# 也**不能**抛异常伪装成崩溃，所以用独立的 SKIPPED 退出码 2。
param(
    [string]$IcoPath,
    [string]$WinIconPath = "$env:TEMP\dsh_icon_check\DSHLauncher.png",
    [double]$MaxDiff = 40
)
# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'

# 仓库根 = 本脚本所在目录（tools\）的上一级；默认基准图标取自仓库根，
# 不再硬编码 D:\DSHLauncher（换机器/换目录就必然误报）。
$root = Split-Path -Parent $PSScriptRoot
if (-not $IcoPath) { $IcoPath = Join-Path $root 'app.ico' }

function Skip([string]$why) {
    Write-Host "SKIPPED: $why" -ForegroundColor Yellow
    Write-Host '（SKIPPED 不等于通过：请先准备所需输入后重跑）' -ForegroundColor Yellow
    exit 2
}

if (-not (Test-Path $IcoPath)) {
    # 基准图标缺失属于「环境不完整」，同样按 SKIPPED 处理（不是不匹配）
    Skip "找不到基准图标 $IcoPath"
}
if (-not (Test-Path $WinIconPath)) {
    Skip "找不到窗口图标导出 $WinIconPath（需先用窗口图标导出步骤生成 32x32 PNG）"
}

Add-Type -AssemblyName System.Drawing

$refIcon = New-Object System.Drawing.Icon($IcoPath, 32, 32)
$ref = $refIcon.ToBitmap()
$win = [System.Drawing.Bitmap]::FromFile($WinIconPath)
Write-Host "基准 app.ico : $($ref.Width)x$($ref.Height)  ($IcoPath)"
Write-Host "窗口图标     : $($win.Width)x$($win.Height)  ($WinIconPath)"
Write-Host "差异阈值     : $MaxDiff 像素"

if ($ref.Width -lt 32 -or $ref.Height -lt 32) {
    $rw = $ref.Width; $rh = $ref.Height
    $ref.Dispose(); $refIcon.Dispose(); $win.Dispose()
    Skip "基准图标尺寸不足 32x32（实际 ${rw}x${rh}），无法逐像素比较"
}
if ($win.Width -lt 32 -or $win.Height -lt 32) {
    $w = $win.Width; $h = $win.Height
    $ref.Dispose(); $refIcon.Dispose(); $win.Dispose()
    Skip "窗口图标导出尺寸不足 32x32（实际 ${w}x${h}），无法逐像素比较"
}

$diff = 0; $opaque = 0
for ($y = 0; $y -lt 32; $y++) {
    for ($x = 0; $x -lt 32; $x++) {
        $a = $ref.GetPixel($x, $y)
        $b = $win.GetPixel($x, $y)
        if ($a.A -gt 8) { $opaque++ }
        if ($a.R -ne $b.R -or $a.G -ne $b.G -or $a.B -ne $b.B -or $a.A -ne $b.A) { $diff++ }
    }
}
Write-Host ""
Write-Host "不透明像素 : $opaque / 1024"
Write-Host "不一致像素 : $diff"

Write-Host ""
Write-Host "app.ico 32x32 形状（#=不透明 .=半透明 空=透明）:"
for ($y = 0; $y -lt 32; $y++) {
    $line = ""
    for ($x = 0; $x -lt 32; $x++) {
        $p = $ref.GetPixel($x, $y)
        if ($p.A -lt 16) { $line += " " }
        elseif ($p.A -lt 128) { $line += "." }
        else { $line += "#" }
    }
    Write-Host $line
}

$ref.Dispose(); $refIcon.Dispose(); $win.Dispose()

# 判定必须是**可失败的**：不匹配时给出非 0 退出码（旧实现只打印红字，永远 exit 0）
if ($diff -eq 0) {
    Write-Host "结论: 完全一致 —— 窗口标题栏图标就是 app.ico 的鲸鱼 logo" -ForegroundColor Green
    exit 0
} elseif ($diff -le $MaxDiff) {
    Write-Host "结论: 基本一致（$diff <= $MaxDiff 像素差异来自缩放/抗锯齿）" -ForegroundColor Green
    exit 0
} else {
    Write-Host "结论: 不匹配（$diff > $MaxDiff 像素差异）" -ForegroundColor Red
    exit 1
}
