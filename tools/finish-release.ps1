# tools\finish-release.ps1 — 一键完成收尾：恢复环境 → 发布 → 全量验证 → 基线测量
#
# 版本与发布标签**从 Cargo.toml / cli.rs 读取**，不在本脚本里手写（报告标题、发布标签
# 都随单一来源变化；发布工程轮把版本统一为 5.0.0 LTS 后，脚本不再制造第二处版本事实）。
#
# 背景：上一轮审计会话中，反复强杀 GUI 进程（WebView2 子进程频繁创建/销毁）之后
# Windows 无法再创建进程（0xC0000142 / STATUS_DLL_INIT_FAILED），导致
# 「最终发布 + 优化后内存/启动基线 + 端到端自检 + cargo audit」四项未能完成。
# 本脚本把剩余全部步骤串起来，可在重启 DSH / 注销后一次性跑完。
#
# 用法（建议在**新开的** PowerShell 窗口里跑，避免复用已损坏的作业对象）：
#   pwsh -NoProfile -ExecutionPolicy Bypass -File tools\finish-release.ps1
#
# 可选参数：
#   -SkipAuditInstalls   跳过 cargo-audit / cargo-machete 安装（离线或不想编译时）
#   -SkipE2E             跳过 selftest.ps1 端到端（A1/A2/B）
#   -MeasureSeconds N    内存基线采样时长（默认 10 秒）
#   -Report <path>       追加写入的报告文件（默认 docs\FINISH-REPORT.md）

param(
    [switch]$SkipAuditInstalls,
    [switch]$SkipE2E,
    [int]$MeasureSeconds = 10,
    [string]$Report = 'docs\FINISH-REPORT.md'
)

$ErrorActionPreference = 'Continue'
# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
Set-Location $root

$stamp = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
$reportPath = if ([System.IO.Path]::IsPathRooted($Report)) { $Report } else { Join-Path $root $Report }
$results = New-Object System.Collections.Generic.List[string]

# ---------------------------------------------------------------------------
# 版本与发布标签的唯一来源
#   * `$AppVersion`：Cargo.toml 的 [workspace.package] version（合法 semver）
#   * `$ReleaseChannel`：crates/dsh-app/src/cli.rs 的 RELEASE_CHANNEL（发布标签，如 LTS）
# 二者**分离**：LTS 绝不写进版本号（`5.0.0-LTS` 在 semver 里表示预发布版本）。
# ---------------------------------------------------------------------------
$AppVersion = $null
$vm = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?ms)^\[workspace\.package\].*?^\s*version\s*=\s*"([^"]+)"')
if ($vm.Success) { $AppVersion = $vm.Groups[1].Value }
if ([string]::IsNullOrWhiteSpace($AppVersion)) { throw '无法从 Cargo.toml 的 [workspace.package] 解析版本' }
$ReleaseChannel = $null
$cliText = Get-Content (Join-Path $root 'crates\dsh-app\src\cli.rs') -Raw
$cm = [regex]::Match($cliText, 'RELEASE_CHANNEL\s*:\s*&str\s*=\s*"([^"]+)"')
if ($cm.Success) { $ReleaseChannel = $cm.Groups[1].Value }
$VersionLabel = if ([string]::IsNullOrWhiteSpace($ReleaseChannel)) { $AppVersion } else { "$AppVersion $ReleaseChannel" }

# ---------------------------------------------------------------------------
# 构建缓存纪律（自清洁）
#
# 本脚本一轮要跑多次 cargo（build / clippy / test / 探针），而 `target\debug\incremental`
# 是**单项最大**的可再生垃圾（实测 1,030 MB）；收尾验证不需要增量加速（每轮都是
# "改完再全量验证"，命中率低）。因此这里显式关闭，把一轮验证的增量产物压到 0。
# 需要增量加速的**日常开发**直接在终端 `cargo build` 即可（本设置只作用于本脚本进程）。
# ---------------------------------------------------------------------------
$env:CARGO_INCREMENTAL = '0'

function Section($t) {
    Write-Host ''
    Write-Host ('=' * 70) -ForegroundColor DarkGray
    Write-Host " $t" -ForegroundColor Cyan
    Write-Host ('=' * 70) -ForegroundColor DarkGray
}
function Record($line) {
    Write-Host $line
    $results.Add($line)
}

# ---------------------------------------------------------------------------
# 失败聚合（P1-2）
#
# 为什么不能依赖异常：`Step` 捕获异常后脚本会继续跑，而 `cargo`/`pwsh` 这类外部
# 命令失败**根本不抛异常**（只设 $LASTEXITCODE）。旧实现只让第 2 步影响退出码，
# 所以「cargo test 编译失败」会被记成「0 个套件通过 / 0 个失败」并最终 exit 0。
# 现在：任何一步都可以把失败写入 $script:failures，汇总时统一决定退出码。
# ---------------------------------------------------------------------------
$script:failures = New-Object System.Collections.Generic.List[string]
function Add-Failure($what) {
    $script:failures.Add($what)
    Write-Host "  ✗ 记录失败：$what" -ForegroundColor Red
}
# 判定一次外部命令的退出码；非 0 即记为失败并返回 $false
function Test-ExitCode($stepName, [int]$code) {
    if ($code -ne 0) { Add-Failure ("$stepName 退出码 $code"); return $false }
    return $true
}

# ---------------------------------------------------------------------------
# Step：执行一步；异常被记录为失败（不再静默吞掉）
#
# 注意：`& $body` 让步骤体运行在**子作用域**。因此步骤内若要向汇总传递状态，
# 必须使用 `$script:` 前缀（见下面的作用域自检）。
# ---------------------------------------------------------------------------
function Step($name, [scriptblock]$body) {
    Section $name
    try { & $body }
    catch {
        Record ("[{0}] EXCEPTION: {1}" -f $name, $_.Exception.Message)
        Add-Failure ("$name 抛出异常：$($_.Exception.Message)")
    }
}

# ---------------------------------------------------------------------------
# 作用域自检（P1-1）
#
# 血泪教训：`Step` 用 `& $body` 调用脚本块 → 体在**子作用域**执行，体内对普通
# 变量（`$published = $true`）的赋值**不会**逃逸到外层。旧实现因此让 $published
# 永远为 $false，脚本无论成败都 exit 1。
# 下面这段在脚本启动时**自证**机制正确：Step 体里写 $script: 前缀的变量，外层
# 必须看得到。如果机制被改坏（例如有人把 Step 换成 Start-Job / 再套一层 & { }），
# 这里会立刻抛出，而不是等到发布结束才以「永远失败」的形式暴露。
# ---------------------------------------------------------------------------
function Test-StepScopeMechanism {
    $script:__scopeProbe = $null
    Step 'self-check: Step 作用域机制' {
        $script:__scopeProbe = 'visible'
    }
    if ($script:__scopeProbe -ne 'visible') {
        throw 'Step 作用域的机制失效：Step 体内对 $script: 变量的赋值未能被外层看到（P1-1 回归）'
    }
    Write-Host '  self-check: Step 体写入 $script: 变量对外层可见 OK' -ForegroundColor Green
}
Test-StepScopeMechanism

$results.Add("# DSHLauncher v$VersionLabel 收尾报告")
$results.Add("")
$results.Add("- 版本 / Version：$AppVersion（发布标签 / label：$(if ([string]::IsNullOrWhiteSpace($ReleaseChannel)) { '—' } else { $ReleaseChannel })）")
$results.Add("- 执行时间：$stamp")
$results.Add("- 工作目录：$root")
$results.Add("- PowerShell：$($PSVersionTable.PSVersion)")
$results.Add("")

# ---------------------------------------------------------------------------
# 结束启动器：**先优雅、后强杀**，且只作用于本仓库的产物（P1-10）
#
# 为什么不能 `Get-Process DSHLauncher | Stop-Process -Force`：
#   1) 那会杀掉**装在本机任何位置**的启动器实例（用户另一份安装、绿色版…）；
#   2) docs/OPS-RUNBOOK.md §1 明确记录：反复强杀 GUI 进程会耗尽桌面堆，
#      最终导致本机无法创建任何新进程（0xC0000142）。
# 正确做法：路径必须属于本仓库 → 用项目自带的优雅入口 `--quit` → 有界等待 →
# 仍未退出才回退强杀。每一步都记录走了哪条路径。
# ---------------------------------------------------------------------------
$script:repokillLog = New-Object System.Collections.Generic.List[string]
function Stop-RepoLauncher([int]$TimeoutSeconds = 15) {
    $rootFull = [System.IO.Path]::GetFullPath($root).TrimEnd('\')
    $targets = @(Get-Process DSHLauncher -ErrorAction SilentlyContinue | Where-Object {
            $p = $_.Path
            if (-not $p) { return $false }
            # 归一化 + 大小写无关：$_.Path 与 $root 的盘符/大小写可能不同
            $pf = [System.IO.Path]::GetFullPath($p)
            $pf.StartsWith($rootFull + '\', [System.StringComparison]::OrdinalIgnoreCase)
        })
    if ($targets.Count -eq 0) {
        Record '   无本仓库路径下的启动器进程（不触碰其他位置的实例）'
        return
    }

    # 1) 优先优雅退出：项目自带 --quit（不会切断 dsh 会话，也不触发 0xC0000142）
    $quitExe = Join-Path $root 'DSHLauncher.exe'
    if (Test-Path $quitExe) {
        Record ("   优雅退出：{0} --quit（等待最多 {1}s）" -f $quitExe, $TimeoutSeconds)
        try { Start-Process -FilePath $quitExe -ArgumentList '--quit' -WindowStyle Hidden | Out-Null }
        catch { Record ("   优雅退出入口调用失败：{0}" -f $_.Exception.Message) }
    } else {
        Record '   根目录无 DSHLauncher.exe，无法使用 --quit 优雅入口'
    }

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $still = @($targets | Where-Object { Get-Process -Id $_.Id -ErrorAction SilentlyContinue })
        if ($still.Count -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }

    # 2) 回退：仍未退出的才强杀（并按整棵进程树处理，避免残留 WebView2）
    $still = @($targets | Where-Object { Get-Process -Id $_.Id -ErrorAction SilentlyContinue })
    foreach ($p in $still) {
        Record ("   回退强杀 PID {0}（{1}）—— 优雅退出后在 {2}s 内未退出" -f $p.Id, $p.Path, $TimeoutSeconds)
        & taskkill.exe /PID $p.Id /T /F 2>&1 | Out-Null
    }
    if ($still.Count -eq 0) { Record '   优雅退出成功（未使用强杀）' }
}


# ---------------------------------------------------------------------------
# 0. 环境健康检查（进程创建是否已恢复）
# ---------------------------------------------------------------------------
Step '0. 环境健康检查' {
    $probe = & cmd.exe /c 'echo env-probe-ok' 2>&1
    if ($probe -match 'env-probe-ok') {
        Record '0. 进程创建: OK'
    } else {
        Record '0. 进程创建: FAIL —— Windows 仍无法创建进程（0xC0000142）'
        Record '   → 请注销或重启 Windows 后重新运行本脚本。已中止。'
        $results | Set-Content -Path $reportPath -Encoding utf8
        Write-Host "报告已写入: $reportPath" -ForegroundColor Yellow
        exit 1
    }
    Record ("0. 残留进程: DSHLauncher={0}, msedgewebview2={1}, node={2}" -f `
        (@(Get-Process DSHLauncher -ErrorAction SilentlyContinue).Count), `
        (@(Get-Process msedgewebview2 -ErrorAction SilentlyContinue).Count), `
        (@(Get-Process node -ErrorAction SilentlyContinue).Count))
}

# ---------------------------------------------------------------------------
# 1. 清理残留（启动器 + WebView2 子进程），释放桌面堆 / 句柄
#    —— 这是恢复 0xC0000142 的关键一步
# ---------------------------------------------------------------------------
Step '1. 清理残留进程' {
    Stop-RepoLauncher -TimeoutSeconds 15
    # WebView2 宿主：仅结束属于本项目的（user-data-dir 指向 %LOCALAPPDATA%\DSHLauncher）
    Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*DSHLauncher*' } |
        ForEach-Object {
            Record ("   结束 WebView2 PID {0}" -f $_.ProcessId)
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
    Start-Sleep -Seconds 2
    Record ("1. 清理后: DSHLauncher={0}, 本项目 WebView2={1}" -f `
        (@(Get-Process DSHLauncher -ErrorAction SilentlyContinue).Count), `
        (@(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.CommandLine -like '*DSHLauncher*' }).Count))
}

# ---------------------------------------------------------------------------
# 2. 发布构建（含版本资源校验；exe 不再被占用后应能通过）
# ---------------------------------------------------------------------------
$published = $false
Step '2. build.ps1 release（构建 + 版本资源校验 + 发布到根目录）' {
    & pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'build.ps1') release 2>&1 |
        ForEach-Object { Write-Host $_ }
    $code = $LASTEXITCODE
    if ($code -eq 0) {
        # 必须用 $script: 前缀：Step 体运行在子作用域，普通赋值不会逃逸（P1-1）
        $script:published = $true
        $exe = Join-Path $root 'DSHLauncher.exe'
        $vi = (Get-Item $exe).VersionInfo
        Record ("2. PASS: DSHLauncher.exe {0:N0} B, FileVersion={1}, ProductVersion={2}" -f `
            (Get-Item $exe).Length, $vi.FileVersion, $vi.ProductVersion)
        Record ("   SHA256 = {0}" -f (Get-FileHash $exe -Algorithm SHA256).Hash)
    } else {
        Record ("2. FAIL: build.ps1 退出码 {0}" -f $code)
        Add-Failure ("2. build.ps1 退出码 $code")
    }
}

# ---------------------------------------------------------------------------
# 3. 安装包构建（内嵌启动器 + 注入版本资源 + 同源校验）
# ---------------------------------------------------------------------------
Step '3. build-setup.ps1（安装包）' {
    & pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'build-setup.ps1') 2>&1 |
        ForEach-Object { Write-Host $_ }
    $code = $LASTEXITCODE
    $setup = Join-Path $root 'DSHLauncherSetup.exe'
    if ($code -eq 0 -and (Test-Path $setup)) {
        $vi = (Get-Item $setup).VersionInfo
        Record ("3. PASS: DSHLauncherSetup.exe {0:N0} B, FileVersion={1}, ProductName={2}" -f `
            (Get-Item $setup).Length, $vi.FileVersion, $vi.ProductName)
        Record ("   SHA256 = {0}" -f (Get-FileHash $setup -Algorithm SHA256).Hash)
    } else {
        Record ("3. FAIL: build-setup.ps1 退出码 {0}" -f $code)
        Add-Failure ("3. build-setup.ps1 退出码 $code")
    }
}

# ---------------------------------------------------------------------------
# 3b. 安装包内嵌一致性（**逐字节**；必须用 PowerShell 5.1 运行）
#
# 为什么单独一步（本轮实测盲区）：`verify-version.ps1` 只校验版本资源与"内嵌了卸载器"这类
# **存在性**事实，`check-consistency.ps1` 只看源码文本 —— 没有任何既有校验能发现
# "安装包内嵌的 README 是旧的"。实测踩到：给 README 加完「文档地图」没有再打包，
# 上面的校验**全绿**，而用户装出来的 README 是旧的。
# 因此这里把 6 项内嵌资源逐个与磁盘产物做 SHA256 比对。
# 必须用 `powershell`（5.1）：安装包是 .NET Framework 程序集，pwsh（.NET 8）加载不了它。
# ---------------------------------------------------------------------------
Step '3b. verify-embedded.ps1（安装包内嵌资源 == 当前产物）' {
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here 'verify-embedded.ps1') 2>&1 |
        ForEach-Object { Write-Host $_ }
    $code = $LASTEXITCODE
    if ($code -eq 0) {
        Record '3b. PASS: 安装包内嵌 6 项资源与当前产物逐字节一致'
    } elseif ($code -eq 2) {
        # 明确区分"没验"与"验过"：前置条件缺失按失败记账
        Record '3b. FAIL: 前置条件缺失（校验未执行，不等于通过）'
        Add-Failure '3b. verify-embedded.ps1 退出码 2（校验未执行）'
    } else {
        Record ("3b. FAIL: verify-embedded.ps1 退出码 {0}" -f $code)
        Add-Failure ("3b. verify-embedded.ps1 退出码 $code")
    }
}

# ---------------------------------------------------------------------------
# 4. 静态与测试验收
# ---------------------------------------------------------------------------
Step '4. cargo fmt --check' {
    & cargo fmt --check 2>&1 | ForEach-Object { Write-Host $_ }
    $c = $LASTEXITCODE
    Record ("4a. fmt: {0}" -f $(if ($c -eq 0) { 'PASS' } else { "FAIL($c)" }))
    if ($c -ne 0) { Add-Failure "4. cargo fmt 退出码 $c" }
}
Step '5. cargo clippy --all-targets --all-features -- -D warnings' {
    & cargo clippy --all-targets --all-features -- -D warnings 2>&1 | Select-Object -Last 5 |
        ForEach-Object { Write-Host $_ }
    $c = $LASTEXITCODE
    Record ("5. clippy: {0}" -f $(if ($c -eq 0) { 'PASS' } else { "FAIL($c)" }))
    if ($c -ne 0) { Add-Failure "5. cargo clippy 退出码 $c" }
}
Step '6. 全量测试' {
    $out = & cargo test --release --all-features 2>&1
    $c = $LASTEXITCODE                 # 必须紧跟 cargo test：任何后续命令都会覆盖它
    $out | Select-String -Pattern 'test result:' | ForEach-Object { Write-Host $_ }
    $suitesOk = ($out | Select-String -Pattern 'test result: ok' | Measure-Object).Count
    $suitesFailed = ($out | Select-String -Pattern 'test result: FAILED' | Measure-Object).Count
    # `^error(|:)` 覆盖 `error:` / `error[E0308]:` 这类**编译错误**。
    # 旧实现只看 "test result: FAILED"，而编译失败根本不产生该行 →
    # 显示成「0 个套件通过 / 0 个失败」并被判定为通过（假绿）。
    $errLines = @($out | Select-String -Pattern '^error(\[|:)')
    Record ("6. tests: {0} 个套件通过 / {1} 个失败（cargo 退出码 {2}，编译错误行 {3}）" -f `
        $suitesOk, $suitesFailed, $c, $errLines.Count)
    if ($c -ne 0) { Add-Failure "6. cargo test 退出码 $c" }
    if ($suitesOk -eq 0) { Add-Failure '6. cargo test 没有任何套件通过（构建失败或测试未被编译）' }
    if ($suitesFailed -gt 0) { Add-Failure ("6. cargo test 有 {0} 个套件失败" -f $suitesFailed) }
    if ($errLines.Count -gt 0) {
        $errLines | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
        Add-Failure ("6. cargo test 输出含 {0} 行错误（error/error[...]）" -f $errLines.Count)
    }
}

# ---------------------------------------------------------------------------
# 6b. 构建缓存体积（target\ + 生效构建目录，report-only）
# ---------------------------------------------------------------------------
Step '6b. 构建缓存体积（target\ + 生效构建目录，report-only）' {
    # 直接复用 clean.ps1 的报告：它是体积口径的唯一实现，且已会解析被搬迁的构建目录。
    $before = & pwsh -NoProfile -File (Join-Path $here 'clean.ps1') 2>&1
    $before | Where-Object { $_ -match 'target\\|构建目录|构建缓存合计|个文件' } | ForEach-Object { Write-Host $_ }

    function Get-TreeMb([string]$path) {
        if (-not (Test-Path -LiteralPath $path)) { return 0.0 }
        $s = Get-ChildItem -LiteralPath $path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum
        if ($null -eq $s.Sum) { return 0.0 }
        return [math]::Round(($s.Sum / 1MB), 1)
    }

    $defaultTarget = Join-Path $root 'target'
    $targetMb = Get-TreeMb $defaultTarget
    # 生效的构建目录：Cargo 1.91+ 允许把中间产物搬出 target\
    # （CARGO_BUILD_BUILD_DIR / [build] build-dir）。旧版只统计 target\，搬迁后会少报最大的一块（实测 538 MB）。
    $buildDirValue = $env:CARGO_BUILD_BUILD_DIR
    if (-not [string]::IsNullOrWhiteSpace($buildDirValue) -and -not [System.IO.Path]::IsPathRooted($buildDirValue)) {
        $buildDirValue = Join-Path $root $buildDirValue
    }
    if ([string]::IsNullOrWhiteSpace($buildDirValue)) { $buildDirValue = $defaultTarget }
    $buildMb = if ($buildDirValue.TrimEnd('\') -ieq $defaultTarget.TrimEnd('\')) { $targetMb } else { Get-TreeMb $buildDirValue }
    Record ("6b. target 体积: {0:N1} MB；构建目录 {1}: {2:N1} MB（回收用 tools\clean.ps1 -Cache）" -f $targetMb, $buildDirValue, $buildMb)
}

# ---------------------------------------------------------------------------
# 7. 一致性 / 版本链路 / 文档数字漂移校验
# ---------------------------------------------------------------------------
Step '7. check-consistency.ps1' {
    & pwsh -NoProfile -File (Join-Path $here 'check-consistency.ps1') 2>&1 |
        Select-String -Pattern 'FAIL|结果:|CHECK_SUMMARY' | ForEach-Object { Write-Host $_ }
    $c = $LASTEXITCODE
    Record ("7. consistency: {0}" -f $(if ($c -eq 0) { 'PASS' } else { "FAIL($c)" }))
    if ($c -ne 0) { Add-Failure "7. check-consistency.ps1 退出码 $c" }
}
Step '8. verify-version.ps1' {
    & pwsh -NoProfile -File (Join-Path $here 'verify-version.ps1') 2>&1 |
        Select-String -Pattern 'FAIL|结果:' | ForEach-Object { Write-Host $_ }
    $c = $LASTEXITCODE
    Record ("8. version-chain: {0}" -f $(if ($c -eq 0) { 'PASS' } else { "FAIL($c)" }))
    if ($c -ne 0) { Add-Failure "8. verify-version.ps1 退出码 $c" }
}
# gen-facts -Check 是「文档数字漂移」的门禁：docs/*.md 里的指标数字必须与
# docs/FACTS.json（唯一来源）一致。不跑它，文档数字就会静默过期（P1-4）。
Step '8b. gen-facts.ps1 -Check（文档数字漂移门禁）' {
    & pwsh -NoProfile -File (Join-Path $here 'gen-facts.ps1') -Check 2>&1 |
        ForEach-Object { Write-Host $_ }
    $c = $LASTEXITCODE
    Record ("8b. doc-facts: {0}" -f $(if ($c -eq 0) { 'PASS' } else { "FAIL($c)" }))
    if ($c -ne 0) { Add-Failure "8b. gen-facts.ps1 -Check 退出码 $c（文档数字与 FACTS.json 漂移）" }
}

# ---------------------------------------------------------------------------
# 9. 优化后基线：内存 / CPU / 启动阶段耗时
# ---------------------------------------------------------------------------
Step "9. 优化后基线（内存 / CPU / 启动耗时）" {
    $exe = Join-Path $root 'DSHLauncher.exe'
    if (-not (Test-Path $exe)) { Record '9. SKIP: 无 DSHLauncher.exe'; return }

    $log = Join-Path $env:LOCALAPPDATA 'DSHLauncher\logs\launcher.log'
    $logMark = if (Test-Path $log) { (Get-Item $log).Length } else { 0 }
    $t0 = Get-Date
    $p = Start-Process -FilePath $exe -PassThru
    Start-Sleep -Seconds $MeasureSeconds
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if (-not $proc) {
        Record '9. FAIL: 启动器提前退出（检查 launcher.log）'
        return
    }
    $cpu1 = $proc.TotalProcessorTime.TotalMilliseconds
    Start-Sleep -Seconds 3
    $proc2 = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    $cpu2 = $proc2.TotalProcessorTime.TotalMilliseconds
    Record ("9. 工作集       : {0:N0} KB" -f ($proc2.WorkingSet64 / 1KB))
    Record ("   专用内存     : {0:N0} KB" -f ($proc2.PrivateMemorySize64 / 1KB))
    Record ("   峰值工作集   : {0:N0} KB" -f ($proc2.PeakWorkingSet64 / 1KB))
    Record ("   线程数       : {0}" -f $proc2.Threads.Count)
    Record ("   CPU(空闲3s)  : {0:N0} ms" -f ($cpu2 - $cpu1))

    # 启动阶段耗时：由 main.rs 的 BootTimer 写入日志的 [boot] 行
    Start-Sleep -Seconds 2
    if (Test-Path $log) {
        $boot = Get-Content $log | Select-String -Pattern '\[boot\]' | Select-Object -Last 8
        if ($boot) { foreach ($b in $boot) { Record ("   " + $b.Line.Trim()) } }
        else { Record '   (日志中暂无 [boot] 行：该版本启动早于埋点加入？)' }
    }

    # 恢复用户可见形态：保留启动器运行（用户可继续用托盘）
    Record '   （启动器保持运行，便于直接观察）'
}

# ---------------------------------------------------------------------------
# 10. 端到端自检（A1 / A2 / B）
#    注意：--selftest 需要独占单实例互斥体；已有实例在跑时必须先退出
# ---------------------------------------------------------------------------
if ($SkipE2E) {
    Record '10. e2e: SKIPPED（-SkipE2E）'
} else {
    Step '10. selftest.ps1（Job Object A1/A2 + 端到端 B）' {
        # --selftest 需要独占单实例互斥体；用优雅 --quit 让位，必要时才回退强杀（P1-10）
        Stop-RepoLauncher -TimeoutSeconds 15
        Start-Sleep -Seconds 2
        & pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'selftest.ps1') 2>&1 |
            ForEach-Object { Write-Host $_ }
        $c = $LASTEXITCODE
        Record ("10. e2e: {0}" -f $(if ($c -eq 0) { 'PASS' } else { "FAIL($c)" }))
        if ($c -ne 0) {
            if ($c -eq 2) { Add-Failure '10. selftest 退出码 2（已有另一个启动器实例在运行，互斥体被占用）' }
            else { Add-Failure "10. selftest.ps1 退出码 $c" }
        }
    }
}

# ---------------------------------------------------------------------------
# 11. 依赖安全审计（cargo-audit / cargo-machete）
#    这两个工具在上轮未能安装，这里补做；离线时请加 -SkipAuditInstalls
# ---------------------------------------------------------------------------
if ($SkipAuditInstalls) {
    Record '11. audit: SKIPPED（-SkipAuditInstalls）'
} else {
    Step '11. 依赖安全审计（cargo-audit 优先，OSV 兜底）' {
        $auditOk = $false
        if (-not (Get-Command cargo-audit -ErrorAction SilentlyContinue)) {
            Write-Host '安装 cargo-audit（首次编译较慢）…'
            & cargo install cargo-audit --locked 2>&1 | Select-Object -Last 3 | ForEach-Object { Write-Host $_ }
        }
        if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
            $out = & cargo audit 2>&1
            $out | Select-Object -Last 25 | ForEach-Object { Write-Host $_ }
            if ($LASTEXITCODE -eq 0) {
                $auditOk = $true
                Record '11a. cargo-audit: PASS（无漏洞）'
            } elseif ($out -match 'couldn''t fetch advisory database|git operation failed') {
                # 本机网络不达 github.com：advisory-db 无法拉取，改用 OSV（同一份 RustSec 数据）
                Record '11a. cargo-audit: UNAVAILABLE（advisory-db 需 github.com，本机不可达）→ 转 OSV'
            } else {
                Record "11a. cargo-audit: FOUND（退出码 $LASTEXITCODE，见上方输出）"
                $auditOk = $true   # 已成功取数，结论见上游输出
            }
        } else {
            Record '11a. cargo-audit: NOT AVAILABLE（安装失败）→ 转 OSV'
        }

        if (-not $auditOk) {
            # OSV 兜底：逐包查 api.osv.dev，并对 Windows 目标构建图单独扫描
            & pwsh -NoProfile -File (Join-Path $here 'osv-audit.ps1') 2>&1 |
                Select-Object -Last 15 | ForEach-Object { Write-Host $_ }
            $allCode = $LASTEXITCODE

            $built = & cargo tree --offline --target x86_64-pc-windows-msvc -e normal,build,dev --prefix none 2>&1 |
                Where-Object { $_ -match '^([A-Za-z0-9_.-]+) v([0-9][^\s]*)' } |
                ForEach-Object { if ($_ -match '^([A-Za-z0-9_.-]+) v([0-9][^\s]*)') { "$($Matches[1]) $($Matches[2])" } } |
                Sort-Object -Unique
            $lock = Get-Content (Join-Path $root 'Cargo.lock')
            $subset = New-Object System.Collections.Generic.List[string]
            for ($i = 0; $i -lt $lock.Count; $i++) {
                if ($lock[$i] -match '^name = "([^"]+)"$') {
                    $n = $Matches[1]
                    if ($i + 1 -lt $lock.Count -and $lock[$i + 1] -match '^version = "([^"]+)"$') {
                        $v = $Matches[1]
                        if ($built -contains "$n $v") {
                            $subset.Add("name = ""$n""")
                            $subset.Add("version = ""$v""")
                        }
                    }
                }
            }
            $winLock = Join-Path $env:TEMP 'dsh-win-graph.lock'
            $subset | Set-Content $winLock -Encoding utf8
            Write-Host ''
            Write-Host "仅扫 Windows 构建图（$((($subset | Where-Object { $_ -like 'name = *' }).Count)) 个包，真正进 exe 的那部分）：" -ForegroundColor Cyan
            & pwsh -NoProfile -File (Join-Path $here 'osv-audit.ps1') -LockFile $winLock 2>&1 |
                Select-Object -Last 8 | ForEach-Object { Write-Host $_ }
            $winCode = $LASTEXITCODE
            Record ("11b. OSV 全量 lockfile : {0}" -f $(if ($allCode -eq 0) { 'PASS' } else { 'FOUND（见上方；若仅 Linux 专用包则不影响 Windows 产物）' }))
            Record ("11c. OSV Windows 构建图: {0}" -f $(if ($winCode -eq 0) { 'PASS（无漏洞）' } else { 'FOUND — 需处理' }))
        }

        if (-not (Get-Command cargo-machete -ErrorAction SilentlyContinue)) {
            Write-Host '安装 cargo-machete …'
            & cargo install cargo-machete --locked 2>&1 | Select-Object -Last 3 | ForEach-Object { Write-Host $_ }
        }
        if (Get-Command cargo-machete -ErrorAction SilentlyContinue) {
            $out2 = & cargo machete 2>&1
            $out2 | Select-Object -Last 20 | ForEach-Object { Write-Host $_ }
            Record ("11d. cargo-machete: {0}" -f $(if ($LASTEXITCODE -eq 0) { 'PASS（无未使用依赖）' } else { 'FOUND（有未使用依赖，见上方）' }))
        } else {
            Record '11d. cargo-machete: NOT AVAILABLE（安装失败；等价手段：对每个依赖做全项目标识符检索）'
        }
    }
}

# ---------------------------------------------------------------------------
# 12. 汇总
# ---------------------------------------------------------------------------
Section '汇总'
$results.Add('')
$results.Add('## 结果汇总')
$results.Add('')
$results.Add('```')
# 先做快照再 AddRange：直接在 ForEach-Object 里向 $results 追加会触发
# 「Collection was modified」非终止错误，导致汇总只剩一行（P1-3）。
$summary = @($results | Where-Object { $_ -match '^\d+[a-z]?\.' })
$results.AddRange([string[]]$summary)
$results.Add('```')

# 失败清单（P1-2）：让报告本身能回答「为什么退出码非 0」
$results.Add('')
$results.Add('## 失败清单')
$results.Add('')
if ($script:failures.Count -eq 0) {
    $results.Add('- 无')
} else {
    foreach ($f in $script:failures) { $results.Add("- $f") }
}
$results.Add('')
$results.Add(("- 汇总：失败 {0} 项（cargo/脚本退出码、异常、编译错误均计入）" -f $script:failures.Count))

$results | Set-Content -Path $reportPath -Encoding utf8
Write-Host ''
Write-Host "报告已写入: $reportPath" -ForegroundColor Green

$exitCode = 0
if (-not $script:published) {
    Write-Host '注意：根目录 DSHLauncher.exe 未更新（见报告第 2 节）。' -ForegroundColor Yellow
    $exitCode = 1
}
if ($script:failures.Count -gt 0) {
    Write-Host ("失败 {0} 项：" -f $script:failures.Count) -ForegroundColor Red
    foreach ($f in $script:failures) { Write-Host "  - $f" -ForegroundColor Red }
    $exitCode = 1
}
if ($exitCode -eq 0) { Write-Host '全部步骤通过 OK' -ForegroundColor Green }
exit $exitCode
