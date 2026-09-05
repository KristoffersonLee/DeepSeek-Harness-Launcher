# build-setup.ps1 — 构建 DSHLauncherSetup.exe（一键安装包，单文件）
# 1) 先构建 DSHLauncher.exe
# 2) 编译安装包并把 DSHLauncher.exe / WebView2 运行库 / app.ico 内嵌为资源
# 用法: powershell -ExecutionPolicy Bypass -File build-setup.ps1

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

# ---------- 2. 构建启动器（build.ps1 第 10 行已无条件调用 make-icon.ps1 生成 app.ico） ----------
& "$here\build.ps1"
if ($LASTEXITCODE -ne 0) { throw "启动器构建失败" }

# ---------- 3. 定位 csc ----------
$csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path $csc)) { $csc = "$env:WINDIR\Microsoft.NET\Framework\v4.0.30319\csc.exe" }
if (-not (Test-Path $csc)) { throw "未找到 csc.exe" }

# ---------- 4. 编译安装包（内嵌启动器、WebView2 运行库与图标） ----------
# 注意：资源参数必须整体加引号（"/resource:<路径>,<标识符>"）。
# 若写成 /resource:"<路径>",<标识符>，PowerShell 7 传递原生参数时引号处理与
# csc 的 /resource: 解析不兼容（csc 会把引号当文件名的一部分），导致 CS1566。
Write-Host "编译 DSHLauncherSetup.exe ..."
& $csc /nologo /target:winexe /optimize+ /codepage:65001 `
    "/win32icon:$here\app.ico" `
    "/out:$here\DSHLauncherSetup.exe" `
    "/resource:$here\DSHLauncher.exe,DSHLauncher.exe" `
    "/resource:$here\app.ico,app.ico" `
    "/resource:$here\lib\Microsoft.Web.WebView2.Core.dll,Microsoft.Web.WebView2.Core.dll" `
    "/resource:$here\lib\Microsoft.Web.WebView2.WinForms.dll,Microsoft.Web.WebView2.WinForms.dll" `
    "/resource:$here\lib\WebView2Loader.dll,WebView2Loader.dll" `
    "/resource:$here\README.md,README.md" `
    "/resource:$here\docs\MAINTENANCE.zh.md,MAINTENANCE.zh.md" `
    "/resource:$here\docs\MAINTENANCE.en.md,MAINTENANCE.en.md" `
    /r:System.dll /r:System.Core.dll /r:System.Drawing.dll /r:System.Windows.Forms.dll `
    "$here\DSHLauncherSetup.cs"

if ($LASTEXITCODE -ne 0) { throw "安装包编译失败 (csc exit code $LASTEXITCODE)" }
Write-Host "安装包构建完成: $here\DSHLauncherSetup.exe"
