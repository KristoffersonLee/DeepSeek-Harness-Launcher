# build.ps1 — 构建 DSHLauncher.exe
# 使用 Windows 自带的 .NET Framework C# 编译器 (csc.exe)，无需安装任何东西。
# 用法: powershell -ExecutionPolicy Bypass -File build.ps1

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

# ---------- 1. 生成应用图标 app.ico（官方 DeepSeek 鲸鱼 LOGO） ----------
& "$here\make-icon.ps1"

# ---------- 2. 若启动器正在运行，先结束（否则 exe 被占用，csc 报 CS0016 无法覆盖） ----------
$running = Get-Process -Name 'DSHLauncher' -ErrorAction SilentlyContinue
if ($running) {
    Write-Host "检测到 DeepSeek Harness Launcher 正在运行，正在结束以便重新编译 ..." -ForegroundColor Yellow
    foreach ($proc in $running) {
        & "$env:WINDIR\System32\taskkill.exe" /PID $proc.Id /T /F | Out-Null
    }
    Start-Sleep -Milliseconds 500
}

# ---------- 3. 定位 csc.exe ----------
$csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path $csc)) { $csc = "$env:WINDIR\Microsoft.NET\Framework\v4.0.30319\csc.exe" }
if (-not (Test-Path $csc)) { throw "未找到 csc.exe（需要 .NET Framework 4.x）" }

# ---------- 4. 编译 ----------
Write-Host "编译 DSHLauncher.exe ..."
& $csc /nologo /target:winexe /optimize+ /codepage:65001 `
    /win32icon:"$here\app.ico" `
    /out:"$here\DSHLauncher.exe" `
    /r:System.dll /r:System.Core.dll /r:System.Drawing.dll /r:System.Windows.Forms.dll /r:System.Management.dll `
    /r:"$here\lib\Microsoft.Web.WebView2.Core.dll" /r:"$here\lib\Microsoft.Web.WebView2.WinForms.dll" `
    "$here\DSHLauncher.cs"

if ($LASTEXITCODE -ne 0) { throw "编译失败 (csc exit code $LASTEXITCODE)" }

# ---------- 5. 拷贝 WebView2 运行所需 DLL 到 exe 旁 ----------
Write-Host "拷贝 WebView2 运行库 ..."
Copy-Item "$here\lib\Microsoft.Web.WebView2.Core.dll" "$here\" -Force
Copy-Item "$here\lib\Microsoft.Web.WebView2.WinForms.dll" "$here\" -Force
Copy-Item "$here\lib\WebView2Loader.dll" "$here\" -Force

Write-Host "构建完成: $here\DSHLauncher.exe"
