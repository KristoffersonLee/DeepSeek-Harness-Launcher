# build-setup.ps1 — 构建 DSHLauncherSetup.exe（一键安装包，单文件）
# 1) 先构建 DSHLauncher.exe（Rust，单文件、图标已内嵌）
# 2) 编译安装包并把 DSHLauncher.exe / dsh-uninstall.exe / app.ico / 文档内嵌为资源
# 3) 注入版本资源并校验版本与 Cargo.toml 同源
# 用法: powershell -ExecutionPolicy Bypass -File build-setup.ps1

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

# ---------- 1. 构建启动器 ----------
& "$here\build.ps1" release
if ($LASTEXITCODE -ne 0) { throw "启动器构建失败" }

# ---------- 1b. 版本唯一来源：Cargo.toml 的 [workspace.package] version ----------
# 不能全局搜 version，否则会命中依赖版本；必须锚定 [workspace.package] 段。
$cargoText = Get-Content (Join-Path $here 'Cargo.toml') -Raw
$expect = [regex]::Match($cargoText, '(?ms)^\[workspace\.package\].*?^\s*version\s*=\s*"([^"]+)"').Groups[1].Value
if ([string]::IsNullOrWhiteSpace($expect)) { throw '无法从 Cargo.toml 解析 workspace 版本' }

# ---------- 1c. 校验**被内嵌的启动器本体**（P2-10）----------
# 旧实现只校验安装包自身的版本资源，从不看它内嵌的 DSHLauncher.exe：只要根目录下的
# 启动器是旧版（版本漂移），就会被打进安装包，用户装到的是「文件是旧版、属性页是新版」
# 的混合体。这里在调用 csc **之前**把启动器钉死。
$psHost = if (Get-Command pwsh -ErrorAction SilentlyContinue) { 'pwsh' } else { 'powershell' }
$launcherExe = Join-Path $here 'DSHLauncher.exe'
if (-not (Test-Path $launcherExe)) { throw "未找到待内嵌的启动器：$launcherExe（build.ps1 未产出？）" }
Write-Host "校验被内嵌的启动器（期望版本 $expect）..." -ForegroundColor Cyan
& $psHost -NoProfile -ExecutionPolicy Bypass -File "$here\tools\verify-version.ps1" -Exe $launcherExe
if ($LASTEXITCODE -ne 0) {
    throw "启动器版本校验失败（tools/verify-version.ps1 退出码 $LASTEXITCODE）：Cargo.toml=$expect，产物=$launcherExe；请先修复版本链路再打包"
}
# 再独立比一次 FileVersion：只接受 "x.y.z" 与 "x.y.z.0" 两种写法，避免 -like "5.0.0*"
# 把 5.0.01 这种伪匹配放过。
$lv = (Get-Item $launcherExe).VersionInfo
if (@($expect, "$expect.0") -notcontains $lv.FileVersion) {
    throw "内嵌启动器 FileVersion = $($lv.FileVersion)，与 Cargo.toml 的 $expect 不一致（版本漂移）"
}
Write-Host "  OK 启动器 FileVersion = $($lv.FileVersion)，ProductName = $($lv.ProductName)" -ForegroundColor DarkGray

# ---------- 2. 确认 csc 可用 ----------
$csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path $csc)) { $csc = "$env:WINDIR\Microsoft.NET\Framework\v4.0.30319\csc.exe" }
if (-not (Test-Path $csc)) { throw "未找到 csc.exe" }

# ---------- 2b. 校验被内嵌的卸载器本体（v5.0.0 LTS 起：原生 exe 取代脚本） ----------
# 与启动器同样的纪律：先钉死"要内嵌的那份产物"，再交给 csc，避免把旧版卸载器打进去。
$uninstaller = Join-Path $here 'target\release\dsh-uninstall.exe'
if (-not (Test-Path $uninstaller)) {
    throw "未找到卸载器产物：$uninstaller（cargo build --release 会构建 workspace 全部成员）"
}
$uvi = (Get-Item $uninstaller).VersionInfo
if (@($expect, "$expect.0") -notcontains $uvi.FileVersion) {
    throw "卸载器 FileVersion = $($uvi.FileVersion)，与 Cargo.toml 的 $expect 不一致"
}
if ($uvi.OriginalFilename -ne 'dsh-uninstall.exe') {
    throw "卸载器 OriginalFilename = $($uvi.OriginalFilename)，应为 dsh-uninstall.exe"
}
Write-Host "  OK 卸载器 FileVersion = $($uvi.FileVersion)，ProductName = $($uvi.ProductName)" -ForegroundColor DarkGray

# ---------- 3. 生成安装包版本资源（版本取自 Cargo.toml，单一来源） ----------
# 否则 DSHLauncherSetup.exe 的属性页是 FileVersion 0.0.0.0、产品名为空
$asmInfo = Join-Path $here 'target\setup-assemblyinfo.cs'
& "$here\tools\gen-setup-version.ps1" -Out $asmInfo
if ($LASTEXITCODE -ne 0) { throw "生成安装包版本资源失败" }

# ---------- 4. 编译安装包（内嵌启动器与图标） ----------
# 说明：v5 启动器为单文件 exe，WebView2 loader 静态链接，不再内嵌/释放旁挂 DLL。
# 注意：资源参数必须整体加引号（"/resource:<路径>,<标识符>"）。
Write-Host "编译 DSHLauncherSetup.exe ..."
& $csc /nologo /target:winexe /optimize+ /codepage:65001 `
    "/win32icon:$here\app.ico" `
    "/out:$here\DSHLauncherSetup.exe" `
    "/resource:$here\DSHLauncher.exe,DSHLauncher.exe" `
    "/resource:$here\target\release\dsh-uninstall.exe,dsh-uninstall.exe" `
    "/resource:$here\app.ico,app.ico" `
    "/resource:$here\README.md,README.md" `
    "/resource:$here\docs\MAINTENANCE.zh.md,MAINTENANCE.zh.md" `
    "/resource:$here\docs\MAINTENANCE.en.md,MAINTENANCE.en.md" `
    /r:System.dll /r:System.Core.dll /r:System.Drawing.dll /r:System.Windows.Forms.dll `
    "$asmInfo" `
    "$here\DSHLauncherSetup.cs"

if ($LASTEXITCODE -ne 0) { throw "安装包编译失败 (csc exit code $LASTEXITCODE)" }

# ---------- 5. 校验安装包版本与启动器/注册表同源 ----------
# 版本号已在 1b 段从 [workspace.package] 解析（不能全局搜 version，否则会命中依赖版本）
$setupExe = Join-Path $here 'DSHLauncherSetup.exe'
$vi = (Get-Item $setupExe).VersionInfo
if ($vi.FileVersion -notlike "$expect*") {
    throw "安装包 FileVersion = $($vi.FileVersion)，应为 $expect.*（版本资源未注入）"
}
if ($vi.ProductName -ne 'DeepSeek Harness Launcher') {
    throw "安装包 ProductName = $($vi.ProductName)，应为 DeepSeek Harness Launcher"
}
$vi | Format-List FileVersion,ProductVersion,ProductName,CompanyName,LegalCopyright

# ---------- 6. 打包完成后重跑一次全链路版本校验（P2-10）----------
# -RequireInstaller：把「安装包产物缺失」从 SKIP 变成硬失败（verify-version.ps1 的
# 口径里跳过绝不算通过）。这样版本链路（Cargo.toml → exe → 安装包 → 卸载器）在
# 打包结束时必须整条绿，才允许打印"构建完成"。
& $psHost -NoProfile -ExecutionPolicy Bypass -File "$here\tools\verify-version.ps1" -Exe $launcherExe -RequireInstaller
if ($LASTEXITCODE -ne 0) { throw "打包后版本一致性校验失败（tools/verify-version.ps1 退出码 $LASTEXITCODE）" }

Write-Host "安装包构建完成: $setupExe (v$($vi.FileVersion)，发布标签 LTS)" -ForegroundColor Green
