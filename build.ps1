# build.ps1 — 构建 DSHLauncher.exe (Rust v5)
# 使用 cargo build（离线模式）。
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File build.ps1          # debug 构建
#   powershell -ExecutionPolicy Bypass -File build.ps1 release  # release 构建

param(
    [ValidateSet("debug", "release")]
    [string]$Mode = "debug"
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

# ---------- 1. 生成应用图标 app.ico（官方 DeepSeek 鲸鱼 LOGO） ----------
& "$here\make-icon.ps1"

# ---------- 2. 构建 Rust 项目 ----------
# 注意：不要把 "--release" 放进数组再 splat（`@args`）——PowerShell 会把
# 前导 "--" 拆坏，导致 cargo 收到裸 "-" 并报 unexpected argument。
# 这里用显式分支，参数原样传递。
#
# 重要：`debug` 模式也**必须**用 `--release` 构建才能产出带版本资源的产物。
# 原因：Windows 资源通过 `cargo:rustc-link-arg=<res>` 注入，而该指令**只对
# 与被链接产物匹配的 profile 生效**；debug profile 下不会链接该资源，于是
# `target\debug\dsh-app.exe` 没有 FileVersion/ProductVersion，随后的版本校验
# 必然失败。debug 与 release 的差别只在于「是否拷贝到仓库根」。
Write-Host "构建 DSHLauncher (Rust v5, profile=release, mode=$Mode) ..." -ForegroundColor Cyan
cargo build --offline --release
if ($LASTEXITCODE -ne 0) { throw "cargo build 失败 (exit code $LASTEXITCODE)" }

# ---------- 3. 校验 release 产物版本资源 ----------
# 必须在拷贝之前校验：根目录 DSHLauncher.exe 可能正被运行中的旧实例占用，
# 此时 Copy-Item 会失败、根目录留的是上一版产物——若先拷贝再校验就会假通过。
# 背景：v5.0.0 之前 rc.exe 找不到时只打 warning，发布出去的 exe 既无图标资源
# 也无 FileVersion/ProductVersion，安装包 DisplayIcon 与属性页全是空白。
# 注意：cargo 以【包名】命名产物，包名为 dsh-app，故产物是 dsh-app.exe
$src = Join-Path $here "target\release\dsh-app.exe"
if (-not (Test-Path $src)) { throw "未找到构建产物: $src" }

& "$here\tools\verify-version.ps1" -Exe $src
if ($LASTEXITCODE -ne 0) {
    throw "产物版本资源校验失败（未发布不合格产物）"
}

if ($Mode -ne "release") {
    Write-Host "debug 模式：已构建并通过版本资源校验，未拷贝到仓库根（$src）" -ForegroundColor Yellow
    exit 0
}

# ---------- 4. 拷贝产物到根目录 ----------
# 直接覆盖会失败（Windows 不允许覆盖**正在运行**的 exe）。但 Windows 允许对运行中的 exe
# **改名**（改目录项，不是文件内容），因此先改名让位、再放入新产物 ——
# **不需要退出启动器**，也就不会中断正在进行的 dsh 会话。
# 详见 docs/OPS-RUNBOOK.md §1c。
$dst = Join-Path $here "DSHLauncher.exe"
try {
    Copy-Item $src $dst -Force -ErrorAction Stop
} catch {
    $bak = Join-Path $here ("DSHLauncher.exe.bak-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
    Write-Host "  $dst 被占用（启动器正在运行）→ 走热替换：先改名让位" -ForegroundColor Yellow
    try {
        Rename-Item -LiteralPath $dst -NewName (Split-Path $bak -Leaf) -ErrorAction Stop
        Copy-Item $src $dst -Force -ErrorAction Stop
        Write-Host "  热替换完成（旧产物保留为 $(Split-Path $bak -Leaf)）" -ForegroundColor Yellow
        Write-Host "  ⚠ 正在运行的实例仍是旧映像；下次启动启动器时新版生效（退出不会中断会话）" -ForegroundColor Yellow
    } catch {
        throw ("无法更新 $dst：$($_.Exception.Message)`n" +
            "已尝试改名热替换仍失败。请确认 DSHLauncher 未在运行，或改用 " +
            "`.\DSHLauncher.exe --quit` 优雅退出后重试；已校验通过的产物仍在 $src。")
    }
}

$size = (Get-Item $dst).Length
$sizeStr = if ($size -gt 1MB) { "$([math]::Round($size / 1MB, 2)) MB" } else { "$([math]::Round($size / 1024, 1)) KB" }
Write-Host "构建完成: $dst ($sizeStr, v$((Get-Item $dst).VersionInfo.ProductVersion) LTS)" -ForegroundColor Green

# ---------- 4b. 一并发布卸载器（原生 exe 取代脚本卸载器） ----------
# 安装包内嵌的是 target\release\dsh-uninstall.exe；仓库根同样放一份，保持"便携/源码树"
# 场景下也能直接卸载（它自己带源码树护栏，在仓库根运行会被拒绝）。
$unSrc = Join-Path $here "target\release\dsh-uninstall.exe"
$unDst = Join-Path $here "dsh-uninstall.exe"
if (Test-Path $unSrc) {
    try {
        Copy-Item $unSrc $unDst -Force -ErrorAction Stop
        Write-Host "已发布卸载器: $unDst" -ForegroundColor Green
    } catch {
        Write-Host "  ⚠ 无法更新 $unDst（可能正在运行）：$($_.Exception.Message)" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ⚠ 未找到卸载器产物 $unSrc（cargo build --release 应构建 workspace 全部成员）" -ForegroundColor Yellow
}

# ---------- 5. 清理历史产物 ----------
# 只保留最近一个 .bak 作为回退点；.old-* / *.pubtmp / *.replaced-old 一律删除，
# 避免根目录堆积多个历史 exe（热替换与旧发布流程的遗留）。
$stale = Get-ChildItem $here -File -ErrorAction SilentlyContinue | Where-Object {
    $_.Name -like 'DSHLauncher.exe.bak-*' -or
    $_.Name -like 'DSHLauncher.exe.old-*' -or
    $_.Name -like '*.pubtmp' -or
    $_.Name -like '*.replaced-old'
}
$keep = $stale | Where-Object { $_.Name -like '*.bak-*' } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
$removed = 0
foreach ($f in $stale) {
    if ($keep -and $f.FullName -eq $keep.FullName) { continue }
    Remove-Item $f.FullName -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path $f.FullName)) {
        Write-Host "  已清理历史产物: $($f.Name)" -ForegroundColor DarkGray
        $removed++
    }
}
if ($removed -eq 0) { Write-Host "  无历史产物需要清理" -ForegroundColor DarkGray }
