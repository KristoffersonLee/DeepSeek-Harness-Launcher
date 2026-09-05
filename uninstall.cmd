@echo off
rem 注意：本文件是仓库模板（绿色版/仓库根自卸载用）；安装目录内的 uninstall.cmd 由
rem 安装包（DSHLauncherSetup.cs 的 WriteUninstallCmd）按安装路径动态生成，清理逻辑与本文件一致。
rem 两者的路径来源不同：模板用 %~dp0（动态），生成版用字面安装路径（静态）。
chcp 65001 >nul
rem 通用卸载脚本：基于自身所在目录（%~dp0），可在任意安装位置工作
set "APP_DIR=%~dp0"
if "%APP_DIR:~-1%"=="\" set "APP_DIR=%APP_DIR:~0,-1%"
rem 结束运行中的本目录启动器（仅限本安装目录，避免误杀其它位置实例）。
rem 注意：卸载是彻底清理场景，这里用 /T 按进程树强杀（连同其启动的 dsh web / 网关一并结束）；
rem 这与升级安装（build.ps1 / StopLauncherInDir 不带 /T、保留后台服务）的语义不同，属有意设计。
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-Process DSHLauncher -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '%APP_DIR%\*' } | ForEach-Object { & taskkill /PID $_.Id /T /F }" >nul 2>&1
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher" /f >nul 2>&1
del "%USERPROFILE%\Desktop\DSH Harness *.lnk" >nul 2>&1
del "%USERPROFILE%\Desktop\DeepSeek Harness Launcher*.lnk" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-ChildItem (Join-Path ([Environment]::GetFolderPath('Desktop')) '*.lnk') -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'DSH Harness *.lnk' -or $_.Name -like 'DeepSeek Harness Launcher*.lnk' } | Remove-Item -Force" >nul 2>&1
rem 清理防火墙规则（DSHLauncher LAN * 匹配所有端口；netsh delete rule name= 不支持通配符，改用 PowerShell）
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-NetFirewallRule -Name 'DSHLauncher LAN *' -ErrorAction SilentlyContinue | Remove-NetFirewallRule" >nul 2>&1
rem 清理 %APPDATA%\DSHLauncher\ 下的凭据与网关脚本；settings.ini 保留（重装后配置不丢失）
del /f /q "%APPDATA%\DSHLauncher\lan-pin.txt" >nul 2>&1
del /f /q "%APPDATA%\DSHLauncher\lan-token.txt" >nul 2>&1
del /f /q "%APPDATA%\DSHLauncher\lan-secret.txt" >nul 2>&1
del /f /q "%APPDATA%\DSHLauncher\.env" >nul 2>&1
del /f /q "%APPDATA%\DSHLauncher\lan-gateway.mjs" >nul 2>&1
rem 清理 %LOCALAPPDATA%\DSHLauncher\ 下的运行日志与内嵌浏览器缓存
rd /s /q "%LOCALAPPDATA%\DSHLauncher" >nul 2>&1
rem 通过环境变量传递路径，避免路径含单引号时 PowerShell 解析失败
set "UNINSTALL_DIR=%APP_DIR%"
start "" /min powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Start-Sleep 2; $p = $env:UNINSTALL_DIR; if (-not ($p -match '^[A-Za-z]:\\?$') -and $p -ne $env:WINDIR -and $p -ne $env:USERPROFILE) { Remove-Item -LiteralPath $p -Recurse -Force }"
exit
