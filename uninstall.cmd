@echo off
chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-Process DSHLauncher -ErrorAction SilentlyContinue | Where-Object { $_.Path -like 'D:\DSHLauncher\*' } | ForEach-Object { & taskkill /PID $_.Id /T /F }" >nul 2>&1
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher" /f >nul 2>&1
del "%USERPROFILE%\Desktop\DSH Harness *.lnk" >nul 2>&1
del "%USERPROFILE%\Desktop\DeepSeek Harness Launcher*.lnk" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-ChildItem (Join-Path ([Environment]::GetFolderPath('Desktop')) '*.lnk') -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'DSH Harness *.lnk' -or $_.Name -like 'DeepSeek Harness Launcher*.lnk' } | Remove-Item -Force" >nul 2>&1
start "" /min powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Start-Sleep 2; $p='D:\DSHLauncher'; if (-not ($p -match '^[A-Za-z]:\\?$') -and $p -ne $env:WINDIR -and $p -ne $env:USERPROFILE) { Remove-Item -LiteralPath $p -Recurse -Force }"
exit
