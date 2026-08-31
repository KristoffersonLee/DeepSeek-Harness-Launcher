@echo off
rem 注意：本文件是仓库模板（绿色版/仓库根自卸载用）；安装目录内的 uninstall.cmd 由
rem 安装包（DSHLauncherSetup.cs 的 WriteUninstallCmd）按安装路径动态生成，逻辑与本文件一致。
rem 若修改本文件，请同步更新 DSHLauncherSetup.cs 中的生成逻辑。
chcp 65001 >nul
rem 通用卸载脚本：基于自身所在目录（%~dp0），可在任意安装位置工作
set "APP_DIR=%~dp0"
if "%APP_DIR:~-1%"=="\" set "APP_DIR=%APP_DIR:~0,-1%"
rem 结束运行中的本目录启动器（仅限本安装目录，避免误杀其它位置实例）
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-Process DSHLauncher -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '%APP_DIR%\*' } | ForEach-Object { & taskkill /PID $_.Id /T /F }" >nul 2>&1
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher" /f >nul 2>&1
del "%USERPROFILE%\Desktop\DSH Harness *.lnk" >nul 2>&1
del "%USERPROFILE%\Desktop\DeepSeek Harness Launcher*.lnk" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-ChildItem (Join-Path ([Environment]::GetFolderPath('Desktop')) '*.lnk') -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'DSH Harness *.lnk' -or $_.Name -like 'DeepSeek Harness Launcher*.lnk' } | Remove-Item -Force" >nul 2>&1
start "" /min powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Start-Sleep 2; $p='%APP_DIR%'; if (-not ($p -match '^[A-Za-z]:\\?$') -and $p -ne $env:WINDIR -and $p -ne $env:USERPROFILE) { Remove-Item -LiteralPath $p -Recurse -Force }"
exit
