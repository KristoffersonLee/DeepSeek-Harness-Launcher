# selftest.ps1 — DSHLauncher v5 (Rust) 自检
#
# 覆盖三项核心验证：
#   A1. Job Object 强杀回收：父进程被强杀 → 子进程由内核自动回收（回归清单 #1）
#   A2. 优雅退出保留服务：disarm 后父进程正常退出 → 子进程继续存活（v2.0 语义）
#   B.  端到端：启动 dsh web → 就绪探测 → 停止 全链路
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File selftest.ps1
#   powershell -ExecutionPolicy Bypass -File selftest.ps1 -SkipE2E            # 只跑 Job Object 两项（A1/A2）
#   powershell -ExecutionPolicy Bypass -File selftest.ps1 -SkipDistribution   # 只跳过 D 段
#
# 退出码:
#   0 = 全部通过
#   1 = 存在失败（非互斥体冲突）
#   2 = 唯一失败原因是「已有另一个启动器实例在运行」（互斥体冲突，与启动器
#       `--selftest` 的退出码 2 语义保持一致）
#
# 分区与开关：
#   A1 / A2        总是运行（Job Object 两项，不依赖 GUI）
#   B / C / D / E  端到端部分；`-SkipE2E` 全部跳过（等价于「只跑 Job Object 两项」）
#   D              也可由 `-SkipDistribution` 单独跳过

param(
    [switch]$SkipE2E,
    [switch]$SkipDistribution
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

$script:TASKKILL = Join-Path $env:WINDIR 'System32\taskkill.exe'

# ---------------------------------------------------------------------------
# 失败记录
#
# $script:runInstanceConflicts 只统计「另一个实例在运行」这一类失败：若全部失败
# 都属此类，脚本以退出码 2 结束（P2-3），与启动器 `--selftest` 的约定一致。
# ---------------------------------------------------------------------------
$script:failed = New-Object System.Collections.Generic.List[string]
$script:runInstanceConflicts = 0
function Add-Failure([string]$id, [string]$reason = '') {
    $line = if ($reason) { "$id（$reason）" } else { $id }
    $script:failed.Add($line)
    Write-Host "  FAIL: $line" -ForegroundColor Red
}
function Add-InstanceConflict([string]$id, [string]$detail) {
    $script:runInstanceConflicts++
    Add-Failure $id $detail
    Write-Host "  提示: 退出码 2 = 已有启动器实例在运行，请先退出它再自检" -ForegroundColor Yellow
}

function Write-Section($title) {
    Write-Host ""
    Write-Host ("=" * 60) -ForegroundColor DarkGray
    Write-Host " $title" -ForegroundColor Cyan
    Write-Host ("=" * 60) -ForegroundColor DarkGray
}

# 结束单个 PID（忽略错误）
function Kill-Quiet($pid_) {
    if ($pid_) { & $script:TASKKILL /F /PID $pid_ 2>&1 | Out-Null }
}

# 结束整棵进程树（父子一起），用于 demo 程序未按预期退出时兜底（P2-2）
function Kill-Tree($pid_) {
    if ($pid_) { & $script:TASKKILL /T /F /PID $pid_ 2>&1 | Out-Null }
}

# 有界等待 GUI 进程退出（P1-5）。
# 关键：exe 是 GUI 子系统，PowerShell 的 `&` 不会等待它，$LASTEXITCODE 会残留为
# 上一条命令的值（造成假通过）；而 `Start-Process -Wait` **没有超时**，死锁的
# 启动器会把脚本永远挂住（文案却写着「最长 180 秒」）。因此改为
# `-PassThru`（不 -Wait）+ `$proc.WaitForExit(ms)`，超时则杀掉整棵进程树并记失败。
function Wait-ProcessWithTimeout($proc, [int]$timeoutMs, [string]$failId) {
    $exited = $proc.WaitForExit($timeoutMs)
    if (-not $exited) {
        Kill-Tree $proc.Id
        $secs = [int]($timeoutMs / 1000)
        Add-Failure $failId ("超过 {0} 秒未退出（疑似死锁），已结束进程树 PID {1}" -f $secs, $proc.Id)
        return $null          # 无真实退出码可用
    }
    return $proc.ExitCode
}

# 挑一个真正空闲的端口（C 段用：让启动器**自己拉起**服务，才能读到一次性 token）
function Get-FreePort {
    $listener = New-Object System.Net.Sockets.TcpListener -ArgumentList @([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $free = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    return $free
}

# C 段会临时改写**用户真实配置**（把端口指向隔离端口）。这两个函数负责还原与清理，
# 幂等且在任何路径（正常 / 异常 / 被取消）下都要能安全调用。
function Restore-CConfig {
    if ([string]::IsNullOrEmpty($script:cCfgPath)) { return }
    if ($script:cCfgExisted) {
        Set-Content -Path $script:cCfgPath -Value $script:cCfgBackup -NoNewline -Encoding UTF8
    } elseif (Test-Path $script:cCfgPath) {
        Remove-Item $script:cCfgPath -Force -ErrorAction SilentlyContinue
    }
    $script:cCfgPath = ''
}

function Stop-CService {
    if ($script:cPort -le 0) { return }
    # ① 优先按**启动器日志里记下的 PID** 杀：探针退出时服务可能才刚 spawn、还没开始监听，
    #    只查"此刻是否在监听"会扑空，留下一个常驻 dsh（本轮实测泄漏了 4120 / 63084 两个）。
    if ($script:cSpawnedPid -gt 0 -and (Get-Process -Id $script:cSpawnedPid -ErrorAction SilentlyContinue)) {
        Kill-Tree $script:cSpawnedPid
    }
    # ② 兜底：该端口是我们刚挑出来的空闲端口 ⇒ 出现监听者必然是本段自己拉起的服务。
    #    给它最多 5 秒时间起来（spawn 到监听之间有一个窗口），起来就杀，没起来就说明本来就没起。
    for ($i = 0; $i -lt 20; $i++) {
        $conn = @(Get-NetTCPConnection -LocalPort $script:cPort -State Listen -ErrorAction SilentlyContinue)
        if ($conn.Count -gt 0) {
            foreach ($x in @($conn.OwningProcess | Select-Object -Unique)) { Kill-Tree $x }
            break
        }
        Start-Sleep -Milliseconds 250
    }
    $script:cPort = 0
    $script:cSpawnedPid = 0
}

$script:cPort = 0          # C 段的隔离端口（0 = 未使用）
$script:cSpawnedPid = 0    # C 段启动器自己拉起并写进日志的 dsh PID（用于可靠回收）
$script:cCfgPath = ''      # 被改写的配置路径（空 = 无需还原）
$script:cCfgExisted = $false
$script:cCfgBackup = $null

$script:demoTempFiles = New-Object System.Collections.Generic.List[string]
$script:demoPids = New-Object System.Collections.Generic.List[int]

try {
    # ---------------------------------------------------------------------------
    # 准备：构建验证程序
    # ---------------------------------------------------------------------------
    Write-Section "准备：构建 Job Object 验证程序"
    # 必须立刻取退出码：成功与否不能靠 Test-Path 猜——构建目录下可能留着上一次构建的
    # 陈旧二进制，那会测出一个与被测源码无关的 PASS（P1-6）。
    #
    # 产物路径**由 cargo 自己报告**（`--message-format=json` 的 `executable` 字段），不再手写
    # `target\debug\examples\...`：该路径属于构建目录的内部布局，Cargo 1.100 起的新布局
    # （`build.build-dir-new-layout`）与 `CARGO_BUILD_BUILD_DIR` 搬迁都会改变中间产物的位置。
    # 目前 example 仍会被 uplift 到 target\<profile>\examples\，但依赖这个细节没有意义 ——
    # cargo 报告的就是权威答案，且它天然消除了「陈旧二进制」这一类假 PASS。
    $demoExe = $null
    $buildJobCode = 1
    & cargo build --offline -p dsh-core --example job_object_demo --message-format=json 2>$null |
        ForEach-Object {
            try { $msg = $_ | ConvertFrom-Json } catch { return }
            if ($msg.executable) { $demoExe = $msg.executable }
        }
    $buildJobCode = $LASTEXITCODE
    if ($buildJobCode -ne 0) {
        Write-Host "FAIL: cargo build -p dsh-core --example job_object_demo 退出码 $buildJobCode" -ForegroundColor Red
        Write-Host "`n❌ 无法继续" -ForegroundColor Red
        exit 1
    }
    if (-not (Test-Path $demoExe)) {
        Write-Host "FAIL: cargo build 成功但未找到 $demoExe" -ForegroundColor Red
        Write-Host "`n❌ 无法继续" -ForegroundColor Red
        exit 1
    }
    Write-Host "  OK: $demoExe" -ForegroundColor Green

    # ---------------------------------------------------------------------------
    # 分区开关（P2-1）：先把「本次会跑哪些段」打印清楚
    # ---------------------------------------------------------------------------
    $runB = -not $SkipE2E
    $runC = -not $SkipE2E
    $runD = (-not $SkipE2E) -and (-not $SkipDistribution)
    $runE = -not $SkipE2E
    Write-Host ""
    Write-Host ("本次运行的分区：A1/A2（总是）；B={0}；C={1}；D={2}；E={3}" -f `
        $(if ($runB) { '运行' } else { '跳过' }), `
        $(if ($runC) { '运行' } else { '跳过' }), `
        $(if ($runD) { '运行' } else { '跳过' }), `
        $(if ($runE) { '运行' } else { '跳过' })) -ForegroundColor Cyan
    if ($SkipE2E) { Write-Host "  （-SkipE2E：只跑 Job Object 两项 A1/A2）" -ForegroundColor DarkGray }
    elseif ($SkipDistribution) { Write-Host "  （-SkipDistribution：D 段单文件分发校验跳过）" -ForegroundColor DarkGray }

    # ---------------------------------------------------------------------------
    # A1. 强杀回收
    # ---------------------------------------------------------------------------
    Write-Section "A1. Job Object 强杀回收（父被强杀 → 子进程应消失）"

    # 随机化的临时文件名：避免多次运行互相踩，也避免上次残留被误读为本次输出（P2-2）
    $outFile = Join-Path $env:TEMP ("dsh_jobdemo_a1_{0}.txt" -f ([guid]::NewGuid().ToString('N').Substring(0, 8)))
    $script:demoTempFiles.Add($outFile)
    Remove-Item $outFile -Force -ErrorAction SilentlyContinue

    $proc = Start-Process -FilePath $demoExe -RedirectStandardOutput $outFile -PassThru -WindowStyle Hidden
    $script:demoPids.Add($proc.Id)
    Start-Sleep -Seconds 2

    $text = if (Test-Path $outFile) { Get-Content $outFile -Raw } else { "" }
    $selfPid  = [regex]::Match($text, "SELF_PID=(\d+)").Groups[1].Value
    $childPid = [regex]::Match($text, "CHILD_PID=(\d+)").Groups[1].Value

    if (-not $selfPid -or -not $childPid) {
        Add-Failure "A1-parse" "无法解析 PID（输出: $text）"
        # 解析失败时只知道父 PID：必须杀**整棵树**，否则未解析出的子进程会存活（P2-2）
        Kill-Tree $proc.Id
    } else {
        $script:demoPids.Add([int]$childPid)
        $aliveBefore = [bool](Get-Process -Id $childPid -ErrorAction SilentlyContinue)
        Write-Host "  父 PID = $selfPid / 子 PID = $childPid（强杀前存活: $aliveBefore）"

        Kill-Quiet $selfPid
        Start-Sleep -Seconds 3

        $aliveAfter = [bool](Get-Process -Id $childPid -ErrorAction SilentlyContinue)
        Write-Host "  强杀父进程后，子进程存活: $aliveAfter"

        if ($aliveBefore -and -not $aliveAfter) {
            Write-Host "  PASS: 子进程由内核自动回收 ✓" -ForegroundColor Green
        } else {
            Add-Failure "A1-job-object" "子进程未被回收"
            if ($aliveAfter) { Kill-Quiet $childPid }
        }
    }

    # ---------------------------------------------------------------------------
    # A2. 优雅退出保留服务
    # ---------------------------------------------------------------------------
    Write-Section "A2. 优雅退出保留服务（父正常退出 → 子进程应存活）"

    $outFile2 = Join-Path $env:TEMP ("dsh_jobdemo_a2_{0}.txt" -f ([guid]::NewGuid().ToString('N').Substring(0, 8)))
    $script:demoTempFiles.Add($outFile2)
    Remove-Item $outFile2 -Force -ErrorAction SilentlyContinue

    $proc2 = Start-Process -FilePath $demoExe -ArgumentList "disarm" -RedirectStandardOutput $outFile2 -PassThru -WindowStyle Hidden
    $script:demoPids.Add($proc2.Id)
    $proc2 | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue

    $text2 = if (Test-Path $outFile2) { Get-Content $outFile2 -Raw } else { "" }
    $childPid2 = [regex]::Match($text2, "CHILD_PID=(\d+)").Groups[1].Value

    if (-not $proc2.HasExited -or -not $childPid2) {
        Add-Failure "A2-exit" "父进程未正常退出或无法解析子 PID（输出: $text2）"
        Kill-Tree $proc2.Id
        Kill-Quiet $childPid2
    } else {
        Start-Sleep -Seconds 2
        $alive2 = [bool](Get-Process -Id $childPid2 -ErrorAction SilentlyContinue)
        Write-Host "  父进程已正常退出；子 PID = $childPid2（存活: $alive2）"

        if ($alive2) {
            Write-Host "  PASS: disarm 后子进程不随父进程退出而终止 ✓" -ForegroundColor Green
        } else {
            Add-Failure "A2-disarm" "disarm 后子进程仍被回收（保留服务语义失效）"
        }
        Kill-Quiet $childPid2
    }

    # ---------------------------------------------------------------------------
    # B. 端到端自检（-SkipE2E 时跳过）
    # ---------------------------------------------------------------------------
    if ($runB) {
        Write-Section "B. 端到端自检（启动 → 就绪 → 停止）"

        # 立刻取退出码（P1-6），并按需复用构建结果给 C 段
        & cargo build --offline --release 2>&1 | Out-Null
        $buildRelCode = $LASTEXITCODE
        $appExe = Join-Path $here "target\release\dsh-app.exe"
        if ($buildRelCode -ne 0) {
            Add-Failure "B-build" "cargo build --release 退出码 $buildRelCode（不能用陈旧二进制继续测）"
        } elseif (-not (Test-Path $appExe)) {
            Add-Failure "B-build" "cargo build 成功但未找到 $appExe"
        } else {
            $log = Join-Path $env:LOCALAPPDATA "DSHLauncher\logs\launcher.log"
            $marker = if (Test-Path $log) { (Get-Content $log | Measure-Object -Line).Lines } else { 0 }

            Write-Host "  运行 $appExe --selftest（最长 180 秒）..."
            # 必须用 Start-Process 取真实退出码（GUI 子系统进程 `&` 不等）；
            # 但**不能** -Wait：没有超时会把脚本永远挂住（P1-5）。
            $proc = Start-Process -FilePath $appExe -ArgumentList '--selftest' -PassThru
            $code = Wait-ProcessWithTimeout $proc 180000 'B-timeout'

            $newLines = if (Test-Path $log) { Get-Content $log | Select-Object -Skip $marker } else { @() }
            $txt = $newLines -join "`n"

            if ($null -eq $code) {
                # 已在上面的辅助函数里记录失败；这里只补上下文
                Write-Host "  FAIL: --selftest 超时（见 B-timeout）" -ForegroundColor Red
            } elseif ($code -eq 0 -and $txt -match "自检通过") {
                Write-Host "  PASS: 端到端链路通过 ✓" -ForegroundColor Green
            } elseif ($code -eq 2) {
                Add-InstanceConflict "B-e2e" "退出码 2（日志未见「自检通过」）"
            } else {
                Add-Failure "B-e2e" "退出码 $code（日志未见「自检通过」）"
            }

            if ($newLines.Count -gt 0) {
                Write-Host "  本次日志:" -ForegroundColor DarkGray
                $newLines | Select-Object -Last 8 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
            }
        }
    }

    # ---------------------------------------------------------------------------
    # C. 设置页 IPC 往返验证（-SkipE2E 时跳过）
    # ---------------------------------------------------------------------------
    if ($runC) {
        Write-Section "C. 设置页 IPC 往返（页面按钮 → Rust 后端）"

        $appExe = Join-Path $here "target\release\dsh-app.exe"
        # 若 B 段已构建成功则这里不必重复构建；仍需确认产物存在（B 段被跳过时不构建）
        if (-not (Test-Path $appExe)) {
            & cargo build --offline --release 2>&1 | Out-Null
            $buildRelCode2 = $LASTEXITCODE
            if ($buildRelCode2 -ne 0) {
                Add-Failure "C-build" "cargo build --release 退出码 $buildRelCode2"
            }
        }
        if (-not (Test-Path $appExe)) {
            Add-Failure "C-build" "未找到 $appExe"
        } else {
            $log = Join-Path $env:LOCALAPPDATA "DSHLauncher\logs\launcher.log"

            # ------------------------------------------------------------------
            # 隔离端口（本轮修复，与 E 段同一套路）
            #
            # 旧实现直接用**用户真实配置**跑探针：若该端口上已有**外部** dsh（很常见：用户自己
            # 开着的会话），启动器读不到它的一次性 token，而本机 WebView2 里也可能没有可用 cookie
            # ⇒ 内嵌页返回 401 ⇒ 认证自愈按设计**弹窗询问**用户 ⇒ 无头探针无人应答 ⇒ 挂满 180s
            # 超时。实测：清空 WebView2 profile 后 C 段必然如此（任何"新克隆 + 本机有 dsh 在跑"
            # 的环境也一样）。改为：临时把配置指向一个**空闲端口**，让启动器自己 spawn 服务 ⇒
            # 能读到 token ⇒ 页面正常加载 ⇒ IPC 往返可判；跑完**原样还原**用户配置。
            # ------------------------------------------------------------------
            $script:cPort = Get-FreePort
            $cfg = Join-Path $env:APPDATA 'DSHLauncher\settings.toml'
            $script:cCfgPath = $cfg
            $script:cCfgExisted = Test-Path $cfg
            if ($script:cCfgExisted) { $script:cCfgBackup = Get-Content $cfg -Raw }
            $cfgDir = Split-Path -Parent $cfg
            if (-not (Test-Path $cfgDir)) { New-Item -ItemType Directory -Path $cfgDir -Force | Out-Null }
            Set-Content -Path $cfg -Value ("port = {0}`n" -f $script:cPort) -Encoding UTF8
            Write-Host ("  隔离端口: {0}（让启动器自建服务以取得 token；跑完还原用户配置）" -f $script:cPort)

            $marker = if (Test-Path $log) { (Get-Content $log | Measure-Object -Line).Lines } else { 0 }
            try {
                Write-Host "  运行 $appExe --ipc-probe（最长 180 秒）..."
                # 同样：取真实退出码 + 有界等待（GUI 子系统进程用 `&` 不等待，
                # 会在它仍持有互斥体时就往下跑；-Wait 则会永久挂住）
                $proc = Start-Process -FilePath $appExe -ArgumentList '--ipc-probe' -PassThru
                $code = Wait-ProcessWithTimeout $proc 180000 'C-timeout'
                Start-Sleep -Milliseconds 300

                $newLines = if (Test-Path $log) { Get-Content $log | Select-Object -Skip $marker } else { @() }
                $txt = $newLines -join "`n"

                # 记下启动器**自己拉起**的服务 PID（finally 里据此可靠回收，不依赖"此刻在监听"）
                $spawnMatch = [regex]::Match($txt, 'dsh web 已启动（PID (\d+)')
                if ($spawnMatch.Success) { $script:cSpawnedPid = [int]$spawnMatch.Groups[1].Value }

                # P2-3：探针自身退出码必须为 0，否则「日志里恰好有那两行」不能算通过
                if ($null -eq $code) {
                    Write-Host "  FAIL: --ipc-probe 超时（见 C-timeout）" -ForegroundColor Red
                } elseif ($code -eq 2) {
                    Add-InstanceConflict "C-exit" "探针退出码 2（互斥体被占用，IPC 结论不可信）"
                } elseif ($code -ne 0) {
                    Add-Failure "C-exit" "探针退出码 $code（应为 0，IPC 结论不可信）"
                }

                if ($null -ne $code -and $code -eq 0 -and $txt -match "设置页命令: Save") {
                    Write-Host "  PASS: 页面按钮命令已抵达 Rust 后端 ✓" -ForegroundColor Green
                } else {
                    Add-Failure "C-ipc" "未收到设置页命令（IPC 通道未打通；退出码 $code）"
                }
                # 探针按分支打开**设置窗口**（端口空闲 ⇒ 自建服务）或**内嵌界面**（接管已有服务）：
                # 两者都证明"窗口确实打开了"（曾出现过"接管分支不开窗"的缺陷，故必须断言）。
                # 旧实现只认「已打开内嵌界面」，于是在自建服务分支下**必然假失败**（本轮实测）。
                if ($null -ne $code -and $code -eq 0 -and ($txt -match "已打开内嵌界面" -or $txt -match "已打开设置窗口")) {
                    Write-Host "  PASS: 内嵌窗口已打开 ✓" -ForegroundColor Green
                } else {
                    Add-Failure "C-open" "未打开内嵌窗口（退出码 $code）"
                }
            } finally {
                # 无论成功/失败/异常：还原用户配置并回收本段拉起的服务
                Restore-CConfig
                Stop-CService
            }
        }
    }

    # ---------------------------------------------------------------------------
    # D. 无侧挂文件（单文件分发）—— 受 -SkipE2E / -SkipDistribution 控制（P2-1）
    # ---------------------------------------------------------------------------
    if ($runD) {
        Write-Section "D. 单文件分发校验"
        $side = Get-ChildItem -Path $here -Recurse -Directory -Filter "*.WebView2" -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -notmatch '\\target\\' }
        if ($side) {
            Add-Failure "D-sidecar" "exe 旁生成了 WebView2 数据目录（会破坏单文件分发）"
            $side | ForEach-Object { Write-Host "    $($_.FullName)" }
        } else {
            Write-Host "  PASS: 无 WebView2 侧挂数据目录 ✓" -ForegroundColor Green
        }
    } else {
        Write-Host ""
        Write-Host "  D. 单文件分发校验: SKIPPED" -ForegroundColor DarkGray
    }

    # ---------------------------------------------------------------------------
    # E. 服务失联检测与自动重新接管（-SkipE2E 时跳过）
    # ---------------------------------------------------------------------------
    if ($runE) {
        Write-Section "E. 服务失联检测与自动重新接管"

        $monitorScript = Join-Path $here "tools\verify-monitor.ps1"
        if (-not (Test-Path $monitorScript)) {
            Add-Failure "E-missing" "未找到 $monitorScript"
        } else {
            Write-Host "  场景：外部 dsh 被外部因素杀掉（如关闭其 CMD 窗口）后，启动器应检测到失联，"
            Write-Host "        并在该端口重新出现服务时自动重新接管（自动挑选空闲端口，不干扰正在使用的会话）"
            & pwsh -NoProfile -ExecutionPolicy Bypass -File $monitorScript
            $monCode = $LASTEXITCODE
            if ($monCode -eq 0) {
                Write-Host "  PASS: 失联检测与自动重新接管 ✓" -ForegroundColor Green
            } else {
                Add-Failure "E-monitor" "失联检测验证未通过（退出码 $monCode）"
            }
        }
    }
} finally {
    # ---------------------------------------------------------------------------
    # 清理（P2-2）：无论成功、失败还是被取消都要执行
    #   * 删除本次创建的随机临时文件；
    #   * 结束本脚本启动过的 demo 子进程（含未解析出 PID 的漏网进程）。
    # ---------------------------------------------------------------------------
    Write-Host ""
    Write-Host "清理：移除本次临时文件并回收 demo 进程" -ForegroundColor DarkGray
    foreach ($f in $script:demoTempFiles) {
        if ($f -and (Test-Path $f)) { Remove-Item $f -Force -ErrorAction SilentlyContinue }
    }
    foreach ($p in $script:demoPids) {
        if ($p -and (Get-Process -Id $p -ErrorAction SilentlyContinue)) { Kill-Tree $p }
    }
    # C 段的兜底（幂等）：即使 C 段自身抛异常或被中断，也要把用户配置还原、把自建服务回收
    Restore-CConfig
    Stop-CService
}

# ---------------------------------------------------------------------------
# 汇总
# ---------------------------------------------------------------------------
Write-Section "自检汇总"
if ($script:failed.Count -eq 0) {
    Write-Host "  ✅ 全部通过" -ForegroundColor Green
    exit 0
} else {
    Write-Host "  ❌ 失败项: $($script:failed -join ', ')" -ForegroundColor Red
    # P2-3：若**全部**失败都只是「另一个实例在运行」，用退出码 2 区分
    # （与启动器 --selftest 的退出码 2 = mutex 冲突保持一致）。混合失败仍为 1。
    if ($script:runInstanceConflicts -gt 0 -and $script:runInstanceConflicts -eq $script:failed.Count) {
        Write-Host "  ⚠ 全部失败均为「已有另一个实例在运行」→ 退出码 2" -ForegroundColor Yellow
        exit 2
    }
    exit 1
}
