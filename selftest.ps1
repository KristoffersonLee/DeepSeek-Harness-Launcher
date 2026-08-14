# selftest.ps1 — 运行 DSHLauncher 的隐藏自检模式并打印报告
$exe = Join-Path $PSScriptRoot 'DSHLauncher.exe'
if (-not (Test-Path $exe)) { throw "未找到 $exe，请先运行 build.ps1" }

Write-Host "运行 DSHLauncher --selftest ..." -ForegroundColor Cyan
$p = Start-Process -FilePath $exe -ArgumentList '--selftest' -Wait -PassThru
Write-Host ("自检退出码: " + $p.ExitCode) -ForegroundColor Cyan
Write-Host "---------------- 报告 ----------------"
$log = Join-Path $PSScriptRoot 'selftest.log'
if (Test-Path $log) {
    Get-Content $log -Encoding UTF8
} else {
    Write-Host "(未生成 selftest.log)"
}
