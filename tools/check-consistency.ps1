# check-consistency.ps1 — DSHLauncher v5 (Rust) 一致性校验
#
# 校验路线图（docs/TECHNICAL-ROADMAP.md）中声明的硬约束是否被破坏：
#   1) 版本号统一：workspace 版本 = 5.0.0，且源码不硬编码版本字符串
#   2) 依赖精简：禁用 crate 不得出现（tauri / tokio / regex / chrono / semver / tracing-*）
#   3) 分层纯净：dsh-core 不得依赖任何 GUI crate（最高价值约束）
#   4) LAN 彻底移除：源码 / UI / 脚本中不得残留局域网关联标识
#   5) 关键文件齐全：Cargo.lock（离线可复现）、ui/*.html、构建与自检脚本
#   6) feature 修正：windows 必须用 Win32_Graphics_Dwm；tray-icon/muda 必须关默认 feature
#
# 用法: pwsh -NoProfile -File tools\check-consistency.ps1   （退出码 0=通过 1=不通过）
# 兼容: PowerShell 5.1+ / 7+（仅使用内置 cmdlet）
#
# 汇总输出：除中文汇总行外，还输出机器可读的一行
#   CHECK_SUMMARY passed=<n> failed=<m> total=<n+m>
# 该行**无论成败都会输出**，供 tools/gen-facts.ps1 稳定解析检查项总数
# （否则某次失败会让记录到的总数变成 0/缺失）。

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here

# 版本号的**唯一来源**是 Cargo.toml 的 [workspace.package] version（与
# tools/verify-version.ps1 用同一个锚定模式）。这里不再写死字面量：旧实现把
# '5.0.0' 硬编码成第二处版本号，并用**未锚定**的正则在整个 Cargo.toml 上匹配 ——
# 既会与单一来源漂移，也可能被注释/别处的 5.0.0 满足（P2-9）。
$issues = New-Object System.Collections.Generic.List[string]
$passed = New-Object System.Collections.Generic.List[string]

function Add-Pass($m) { $script:passed.Add($m) }
function Add-Issue($m) { $script:issues.Add($m) }

function Read-Text($rel) {
    $p = Join-Path $root $rel
    if (-not (Test-Path $p)) { return $null }
    return (Get-Content $p -Raw)
}

function Test-FileExists($rel, $what) {
    $p = Join-Path $root $rel
    if (Test-Path $p) { Add-Pass "$what 存在（$rel）" }
    else { Add-Issue "$what 缺失：$rel" }
}

# 去掉注释（`//` 行注释 + `/* ... */` 块注释）后再做判定。
# 为什么需要：本仓库的注释里大量出现 API 名/标识符（例如解释"为什么不用
# eprintln!"、`[`spawn_stderr_drain`]` 这类文档链接），直接匹配源码会把**说明文字**
# 当成"接线存在"的证据 —— 一个纯粹的注释/文档提及就能让 55 条接线类断言通过（P1-11）。
#
# 实现要点：
#   * 块注释必须支持**跨行**（旧实现只逐行去掉 `//`，`/* ... */` 完全没处理）；
#   * 为了尽量保持行号与换行结构，只把被注释掉的**代码字符**丢弃，换行原样保留；
#   * 状态机需要正确处理字符字面量与字符串里的 `//`，否则会把正常代码切掉
#     （例如 `let sep = "//";` / Windows 路径）。
function Strip-Comments([string]$text) {
    if ($null -eq $text) { return '' }
    $out = New-Object System.Text.StringBuilder
    # state: 0=code 1=line-comment 2=block-comment 3=string 4=char
    # 注意：这里**不能**用 PowerShell 的 switch + continue —— 在 switch 内的 `continue`
    # 只作用于 switch 本身，不会继续外层 while，于是状态会"改了却仍执行原分支"，
    # 注释根本剥不掉（本函数的第一版就踩了这个坑）。改用 if/elseif。
    $state = 0
    $strDelim = [char]0
    $strRaw = $false
    $rawHashes = 0
    $i = 0
    $n = $text.Length
    while ($i -lt $n) {
        $c = $text[$i]
        $d = if ($i + 1 -lt $n) { $text[$i + 1] } else { [char]0 }
        if ($state -eq 0) {
            if ($c -eq '/' -and $d -eq '/') { $state = 1; $i += 2 }
            elseif ($c -eq '/' -and $d -eq '*') { $state = 2; $i += 2 }
            elseif ($c -eq 'r' -and ($d -eq '"' -or $d -eq '#')) {
                # Rust 原始字符串：r"..." / r#"..."#
                $hashes = 0
                $j = $i + 1
                while ($j -lt $n -and $text[$j] -eq '#') { $hashes++; $j++ }
                if ($hashes -gt 0 -and ($j -ge $n -or $text[$j] -ne '"')) {
                    # 不是原始字符串（例如标识符 r#foo）：按普通代码处理
                    [void]$out.Append($c); $i++
                } else {
                    [void]$out.Append($c)
                    $state = 3; $strDelim = '"'; $strRaw = $true; $rawHashes = $hashes
                    $i = $j + 1
                }
            }
            elseif ($c -eq '"') { $state = 3; $strDelim = '"'; $strRaw = $false; [void]$out.Append($c); $i++ }
            elseif ($c -eq "'") {
                # 区分字符字面量与生命周期（'a、'static、'static>）：
                # 生命周期**不是**字符字面量。误判的代价极大 —— 一旦把 `'static>`
                # 当成字面量开始，扫描器会一直"在字面量里"，直到下一个 `'` 才复位，
                # 于是整段代码的注释都剥不掉（本函数第一版就这样漏了 200 多行）。
                # 判定：`'` + 标识符起始字符 + 后续标识符字符，且其后不是 `'`。
                $isLifetime = $false
                if ($d -eq '_' -or [char]::IsLetter($d)) {
                    $j = $i + 2
                    while ($j -lt $n -and ([char]::IsLetterOrDigit($text[$j]) -or $text[$j] -eq '_')) { $j++ }
                    $isLifetime = ($j -ge $n) -or ($text[$j] -ne "'")
                }
                if ($isLifetime) { [void]$out.Append($c); $i++ }
                else { $state = 4; $strDelim = "'"; [void]$out.Append($c); $i++ }
            } else { [void]$out.Append($c); $i++ }
        } elseif ($state -eq 1) {
            if ($c -eq "`n") { [void]$out.Append($c); $state = 0 }
            $i++
        } elseif ($state -eq 2) {
            if ($c -eq '*' -and $d -eq '/') { $state = 0; $i += 2 }
            else {
                if ($c -eq "`n") { [void]$out.Append($c) }
                $i++
            }
        } elseif ($state -eq 3) {
            if ($c -eq '\' -and -not $strRaw) {
                [void]$out.Append($c)
                if ($d -ne [char]0) { [void]$out.Append($d) }
                $i += 2
            } elseif ($strRaw -and $c -eq '"' -and $rawHashes -gt 0) {
                $ok = $true
                for ($k = 1; $k -le $rawHashes; $k++) {
                    if ($i + $k -ge $n -or $text[$i + $k] -ne '#') { $ok = $false; break }
                }
                if ($ok) {
                    [void]$out.Append($c)
                    for ($k = 1; $k -le $rawHashes; $k++) { [void]$out.Append('#') }
                    $i += $rawHashes + 1
                    $state = 0
                } else { [void]$out.Append($c); $i++ }
            } else {
                if ($c -eq '"') { $state = 0 }
                [void]$out.Append($c); $i++
            }
        } else {
            # state 4：字符字面量
            if ($c -eq '\') {
                [void]$out.Append($c)
                if ($d -ne [char]0) { [void]$out.Append($d) }
                $i += 2
            } else {
                if ($c -eq $strDelim) { $state = 0 }
                [void]$out.Append($c); $i++
            }
        }
    }
    return $out.ToString()
}

# 该文件**非注释源码**里是否出现标识符。用于把"接线类"断言从
# "名字出现在任何地方（含注释/文档）"收紧到"真的在代码里出现"。
function Test-CodeMatch([string]$text, [string]$pattern) {
    if ([string]::IsNullOrEmpty($text)) { return $false }
    return [regex]::IsMatch((Strip-Comments $text), $pattern)
}
# 带缓存的版本：同一文件会做几十条断言，不必重复解析注释
$script:__strippedCache = @{}
function Get-CodeText([string]$path, [string]$raw) {
    if ([string]::IsNullOrEmpty($raw)) { return '' }
    if (-not $script:__strippedCache.ContainsKey($path)) {
        $script:__strippedCache[$path] = Strip-Comments $raw
    }
    return $script:__strippedCache[$path]
}

# ---------------------------------------------------------------------------
# 1. 版本号统一
#
# 期望版本从 Cargo.toml 的 [workspace.package] 解析（同一处唯一来源），
# 再用**锚定**模式判定；解析不到就直接失败（不能拿一个猜的值继续往下比）。
# ---------------------------------------------------------------------------
$cargo = Read-Text 'Cargo.toml'
$EXPECT_VERSION = $null
if ($null -eq $cargo) {
    Add-Issue 'Cargo.toml 缺失'
} else {
    $vm = [regex]::Match($cargo, '(?ms)^\[workspace\.package\].*?^\s*version\s*=\s*"([^"]+)"')
    if (-not $vm.Success) {
        Add-Issue 'Cargo.toml 的 [workspace.package] 未声明 version（无法确定期望版本）'
    } else {
        $EXPECT_VERSION = $vm.Groups[1].Value
        Add-Pass "workspace 版本（单一来源）= $EXPECT_VERSION"
    }
}
if ($null -eq $EXPECT_VERSION) {
    # 后续所有 "$EXPECT_VERSION" 断言都必须有值可比，否则会静默"通过"
    Add-Issue '无法确定期望版本，后续版本类断言不可信（已中止）'
    Write-Host ''
    Write-Host '==== DSHLauncher v5 (Rust) 一致性校验 ====' -ForegroundColor Cyan
    foreach ($p in $passed) { Write-Host "  [PASS] $p" -ForegroundColor Green }
    foreach ($i in $issues) { Write-Host "  [FAIL] $i" -ForegroundColor Red }
    Write-Host "CHECK_SUMMARY passed=$($passed.Count) failed=$($issues.Count) total=$($passed.Count + $issues.Count)"
    Write-Host "结果: $($issues.Count) 项不通过 / $($passed.Count) 项通过" -ForegroundColor Red
    exit 1
}

# 源码不得硬编码版本号（应使用 env!("CARGO_PKG_VERSION")）
$mainRs = Read-Text 'crates/dsh-app/src/main.rs'
if ($null -ne $mainRs) {
    if ((Get-CodeText 'crates/dsh-app/src/main.rs' $mainRs) -match 'v5\.\d+\.\d+') {
        Add-Issue 'main.rs 硬编码版本号字符串，应改用 env!("CARGO_PKG_VERSION")'
    } else {
        Add-Pass 'main.rs 未硬编码版本号'
    }
    if ((Get-CodeText 'crates/dsh-app/src/main.rs' $mainRs) -match 'CARGO_PKG_VERSION') { Add-Pass 'main.rs 使用编译期版本号' }
    else { Add-Issue 'main.rs 未使用 env!("CARGO_PKG_VERSION")' }
}

# ---------------------------------------------------------------------------
# 1b. 文档版本一致性（防止文档与代码版本漂移）
# ---------------------------------------------------------------------------
$readme = Read-Text 'README.md'
if ($null -ne $readme) {
    if ($readme -match [regex]::Escape($EXPECT_VERSION)) { Add-Pass "README 声明 v$EXPECT_VERSION" }
    else { Add-Issue "README 未声明 v$EXPECT_VERSION" }
}

$changelog = Read-Text 'docs/CHANGELOG.md'
if ($null -eq $changelog) {
    Add-Issue 'docs/CHANGELOG.md 缺失'
} else {
    # 版本导航需含新版本锚点，且需有对应段落标题
    $anchor = 'v' + ($EXPECT_VERSION -replace '\.', '')
    if ($changelog -match [regex]::Escape("#$anchor-")) { Add-Pass "CHANGELOG 版本导航含 $anchor" }
    else { Add-Issue "CHANGELOG 版本导航缺少 $anchor 锚点" }
    if ($changelog -match [regex]::Escape("## v$EXPECT_VERSION")) { Add-Pass "CHANGELOG 含 v$EXPECT_VERSION 段落" }
    else { Add-Issue "CHANGELOG 缺少 v$EXPECT_VERSION 段落" }
}

# 发布说明文件需与版本号同名且标题一致
$notesRel = "docs/RELEASE_NOTES_v$EXPECT_VERSION.md"
$notes = Read-Text $notesRel
if ($null -eq $notes) {
    Add-Issue "缺少发布说明 $notesRel"
} elseif ($notes -match [regex]::Escape("# DeepSeek Harness Launcher v$EXPECT_VERSION")) {
    Add-Pass "发布说明标题为 v$EXPECT_VERSION"
} else {
    Add-Issue "发布说明 $notesRel 标题未使用 v$EXPECT_VERSION"
}

# 维护手册需反映新版本
foreach ($m in @('docs/MAINTENANCE.zh.md', 'docs/MAINTENANCE.en.md')) {
    $t = Read-Text $m
    if ($null -eq $t) { Add-Issue "缺少 $m"; continue }
    if ($t -match [regex]::Escape("v$EXPECT_VERSION")) { Add-Pass "$m 已同步 v$EXPECT_VERSION" }
    else { Add-Issue "$m 未提及 v$EXPECT_VERSION" }
}

# ---------------------------------------------------------------------------
# 1c. 安装包 / 卸载器 / 启动器版本与产品元数据必须同源
#     （v5.0.0 之前的真实缺陷：Cargo.toml=5.0.0、安装包 AppVersion=4.2.4、
#       exe 无版本资源 → 三处互相矛盾）
# ---------------------------------------------------------------------------
$setupText = Read-Text 'DSHLauncherSetup.cs'
if ($null -eq $setupText) {
    Add-Issue 'DSHLauncherSetup.cs 缺失（无法安装/卸载）'
} else {
    # v5.0.0 审计后：安装包不再手写版本常量，改为从自身 AssemblyFileVersion 反射读取
    # （唯一来源仍是 Cargo.toml → tools/gen-setup-version.ps1）。
    if ($setupText -match 'ResolveAppVersion') {
        Add-Pass '安装包版本由 AssemblyFileVersion 反射读取（无第二处手写版本）'
    } else {
        Add-Issue 'DSHLauncherSetup.cs 未通过 ResolveAppVersion 反射读取版本'
    }
    if ([regex]::IsMatch($setupText, 'AppVersion\s*=\s*"[0-9]')) {
        Add-Issue '安装包中仍有手写版本字面量（会与 Cargo.toml 漂移）'
    } else {
        Add-Pass '安装包中无手写版本字面量'
    }

    if ($setupText -match '"4\.\d+\.\d+"') {
        Add-Issue '安装包中残留 4.x 版本号字面量'
    } else {
        Add-Pass '安装包中无旧版本号残留'
    }

    if ($setupText -match 'SetValue\("DisplayVersion",\s*Program\.AppVersion\)') {
        Add-Pass '注册表 DisplayVersion 绑定 AppVersion（单一来源）'
    } else {
        Add-Issue '注册表 DisplayVersion 未绑定 AppVersion'
    }

    if ($setupText -match 'QuietUninstallString') { Add-Pass '注册表含 QuietUninstallString' }
    else { Add-Issue '注册表缺少 QuietUninstallString（无法静默卸载）' }

    # 卸载必须默认保留用户配置（settings.toml 是用户显式设置）
    if ($setupText -match '\$env:APPDATA\\DSHLauncher') {
        Add-Issue '卸载脚本无条件删除 %APPDATA%\DSHLauncher（会丢失用户配置）'
    } elseif ($setupText -match 'PURGE|Purge') {
        Add-Pass '卸载脚本默认保留用户配置，--purge 才清理'
    } else {
        Add-Issue '卸载脚本缺少 PURGE 分支'
    }

    # 卸载器：v5.0.0 LTS 起为**原生 exe**（`dsh-uninstall.exe`），脚本卸载器已删除。
    # 为什么不再用脚本：`-ExecutionPolicy Bypass` 覆盖不了组策略（AllSigned/Restricted），
    # AppLocker/WDAC 也能封锁脚本执行 ⇒ 加固环境下卸载不掉；且延迟删除目录还要再依赖一次
    # PowerShell（模板里还要往 %TEMP% 写临时脚本）。原生 exe 没有这层依赖。
    if ($setupText -match 'dsh-uninstall\.exe') { Add-Pass '安装包内嵌原生卸载器 dsh-uninstall.exe' }
    else { Add-Issue '安装包未内嵌原生卸载器（卸载器列表里找不到 dsh-uninstall.exe）' }
    if ($setupText -match '"UninstallString"\s*,\s*"\\""\s*\+\s*uninstallExe') { Add-Pass '注册表 UninstallString 指向原生卸载器' }
    else { Add-Issue '注册表 UninstallString 未指向 dsh-uninstall.exe' }
    if ($setupText -match 'HKCU\\\\|UninstallKey') { Add-Pass '注册表删除带 hive 前缀' }
    else { Add-Issue '注册表删除可能缺 hive 前缀（会静默失败留下卸载项）' }
}

# 卸载器必须是**单一实现**：脚本卸载器（uninstall.cmd / uninstall.ps1）不得复活，
# 否则就是两套逻辑、两处漂移面（本轮正是把它们删掉换成原生 exe）。
foreach ($rel in @('uninstall.cmd', 'uninstall.ps1')) {
    if (Test-Path (Join-Path $root $rel)) {
        Add-Issue "$rel 仍然存在：卸载器必须只有一份实现（原生 dsh-uninstall.exe）"
    } else {
        Add-Pass "脚本卸载器已删除：$rel"
    }
}

# 卸载器 crate 的存在性、护栏、开关与载荷清单（与安装器逐项对齐）
$unRs = Read-Text 'crates/dsh-uninstall/src/main.rs'
if ($null -eq $unRs) {
    Add-Issue '缺少 crates/dsh-uninstall（原生卸载器）'
} else {
    Add-Pass '原生卸载器 crate 存在（crates/dsh-uninstall）'
}
$planRs = Read-Text 'crates/dsh-uninstall/src/plan.rs'
$guardsRs = Read-Text 'crates/dsh-uninstall/src/guards.rs'
$stepsRs = Read-Text 'crates/dsh-uninstall/src/steps.rs'
$platformRs = Read-Text 'crates/dsh-uninstall/src/platform.rs'
if ($null -ne $planRs -and $null -ne $guardsRs -and $null -ne $stepsRs -and $null -ne $platformRs) {
    $planCode = Get-CodeText 'crates/dsh-uninstall/src/plan.rs' $planRs
    $guardsCode = Get-CodeText 'crates/dsh-uninstall/src/guards.rs' $guardsRs
    $stepsCode = Get-CodeText 'crates/dsh-uninstall/src/steps.rs' $stepsRs
    $platformCode = Get-CodeText 'crates/dsh-uninstall/src/platform.rs' $platformRs

    # 护栏：标记必需、受保护路径、外来文件保护、源码树识别
    if ($planCode -match 'MARKER_NAME' -and $planCode -match 'check_admission' -and $planCode -match 'is_file\(\)') {
        Add-Pass '卸载器要求安装标记 .dsllauncher-install 才执行删除'
    } else {
        Add-Issue '卸载器缺少"安装标记必需"护栏（会误删源码树/便携目录）'
    }
    if ($guardsCode -match 'fn\s+is_protected_root' -and $guardsCode -match 'fn\s+is_source_tree' -and
        $guardsCode -match 'fn\s+foreign_entries') {
        Add-Pass '卸载器含三道护栏（受保护路径 / 源码树识别 / 外来文件保护）'
    } else {
        Add-Issue '卸载器护栏不完整（受保护路径 / 源码树 / 外来文件）'
    }
    # 只结束本目录的启动器（按映像路径），绝不误杀别处安装
    if ($stepsCode -match 'stop_launcher_in' -and $platformCode -match 'same_path' -and
        $platformCode -match 'process_image_path') {
        Add-Pass '卸载器只结束安装目录内的启动器实例（按映像路径比较，读不到路径则不杀）'
    } else {
        Add-Issue '卸载器结束进程时可能误杀别处安装的启动器'
    }
    # 默认保留用户配置；只有 --purge 才删
    if ($stepsCode -match 'ctx\.purge' -and $stepsCode -match 'roaming_dir' -and $stepsCode -match '已保留用户配置') {
        Add-Pass '卸载器默认保留 %APPDATA% 用户配置（仅 --purge 删除）'
    } else {
        Add-Issue '卸载器未区分"默认保留 / --purge 删除"用户配置'
    }
    # P0 交叉校验：安装标记的**内容判定**必须与安装器实际写入的文本一致。
    #
    # 实测缺陷：安装器写产品名（`DeepSeek Harness Launcher`），卸载器却要求内容含内部标识
    # （`DSHLauncher`）—— 两者互不包含，于是**每一台真实安装都被自己的卸载器拒绝**
    # （退出码 2，什么都不删）。这里把三方钉在一起：Rust 常量、卸载器判定函数、C# 安装器常量。
    $planRs2 = Read-Text 'crates/dsh-uninstall/src/plan.rs'
    $libRs2 = Read-Text 'crates/dsh-core/src/lib.rs'
    $setupText2 = Read-Text 'DSHLauncherSetup.cs'
    $setupAppName = if ($setupText2 -match 'AppName\s*=\s*"([^"]+)"') { $Matches[1] } else { '' }
    $productName = if ($libRs2 -match 'PRODUCT_NAME:\s*&str\s*=\s*"([^"]+)"') { $Matches[1] } else { '' }
    if ($null -ne $planRs2 -and $planRs2 -match 'fn marker_is_ours' -and
        $planRs2 -match 'marker_is_ours\(' -and $productName -and $setupAppName -eq $productName -and
        $null -ne $setupText2 -and $setupText2 -match 'product=' ) {
        Add-Pass '安装标记判定与安装器写入内容一致（产品名 $productName，产品标识同样写入标记）'
    } else {
        Add-Issue '安装标记判定可能与安装器写入内容不一致（真实安装会被拒绝卸载；或标记缺少产品标识）'
    }
    # 该缺陷只能用「模拟安装目录 + dry-run」发现：仓库根是源码树，永远走不到准入通过的分支
    if (Test-Path (Join-Path $root 'tools/verify-install-layout-dryrun.ps1')) {
        Add-Pass '提供模拟安装目录的 dry-run 校验（tools/verify-install-layout-dryrun.ps1）'
    } else {
        Add-Issue '缺少模拟安装目录的 dry-run 校验：卸载器准入路径在源码树上永远测不到'
    }

    # dry-run 必须零改动：其中不得出现删除类调用
    $dryBlock = [regex]::Match($stepsCode, '(?s)fn\s+print_plan.*?\n\}').Value
    if ($dryBlock -and -not (Test-CodeMatch $dryBlock 'remove_file|remove_dir_all|kill_process_tree|remove_uninstall_key|remove_desktop_shortcuts')) {
        Add-Pass '卸载器 dry-run 分支不含任何状态变更调用'
    } else {
        Add-Issue '卸载器 dry-run 分支含状态变更调用（会真的改动系统）'
    }
    # 两阶段（运行中的镜像无法自删）
    if ($stepsCode -match 'deferred_pass' -and $stepsCode -match 'schedule_delete_on_reboot') {
        Add-Pass '卸载器分两阶段（复制自身到 %TEMP% 重入 + 重启时清理副本）'
    } else {
        Add-Issue '卸载器缺少两阶段自删除（运行中的镜像删不掉自己 → 目录会残留）'
    }

    # 残留清理模式（--clean-residue）：安装目录被删除后，注册表项与快捷方式必须仍有出路。
    #
    # 实测缺口：卸载器从**自身所在目录**推导安装目录 ⇒ 目录被删掉后没有任何正规入口
    # （安装目录里的卸载器随目录消失、源码树副本被源码树护栏拒绝、--deferred-pass 被
    # "缺少安装标记"拒绝），于是 HKCU 卸载项与桌面快捷方式成为**永远清不掉的悬空残留**，
    # 设置 → 应用里的卸载按钮只会报找不到可执行文件。
    $cliRs = Read-Text 'crates/dsh-uninstall/src/cli.rs'
    if ($null -ne $cliRs) {
        $cliCode = Get-CodeText 'crates/dsh-uninstall/src/cli.rs' $cliRs
        if ($cliCode -match 'clean_residue' -and $cliCode -match 'cleanresidue') {
            Add-Pass '卸载器提供残留清理模式 --clean-residue（安装目录被删后仍能清掉注册表项/快捷方式）'
        } else {
            Add-Issue '卸载器缺少残留清理模式：安装目录被删除后注册表项与快捷方式永远清不掉'
        }
        if ($cliCode -match 'clean-residue') {
            Add-Pass '帮助文本（--help）说明 --clean-residue，用户可发现该模式'
        } else {
            Add-Issue '帮助文本未说明 --clean-residue（模式存在但用户发现不了）'
        }
    }
    $residueBlock = [regex]::Match($stepsCode, '(?s)pub fn residue_pass.*?\n\}').Value
    if ($residueBlock) {
        # 残留清理**绝不能**触碰文件系统：那会让"没有目录归属证据"的模式获得删除权
        if (Test-CodeMatch $residueBlock 'remove_file|remove_dir|remove_dir_all|kill_process_tree|copy_file') {
            Add-Issue '残留清理模式含文件系统变更调用（超出"只清磁盘外残留"的承诺）'
        } else {
            Add-Pass '残留清理模式不做任何文件系统变更（只清注册表项与桌面快捷方式）'
        }
        # 准入：注册表三个值必须互相印证，否则会去清理同名/伪造的键
        if ($guardsCode -match 'fn\s+residue_target_of' -and $guardsCode -match 'install_location' -and
            $guardsCode -match 'is_our_payload_name' -and $platformCode -match 'read_uninstall_value') {
            Add-Pass '残留清理用注册表三值（InstallLocation/UninstallString/DisplayIcon）互相印证，避免清理同名键'
        } else {
            Add-Issue '残留清理未校验注册表值与 InstallLocation 的一致性（可能清理别的程序的键）'
        }
        # 准入：受保护路径 / 源码树 / 安装标记仍在 ⇒ 拒绝（目录完整就该走正规卸载）
        if ($residueBlock -match 'residue_dir_allowed' -and $guardsCode -match 'fn\s+residue_dir_allowed' -and
            $guardsCode -match 'MARKER_NAME' -and $guardsCode -match 'is_source_tree') {
            Add-Pass '残留清理复用了标记必需 / 受保护路径 / 源码树三道护栏（目录完整时拒绝执行）'
        } else {
            Add-Issue '残留清理缺少"安装标记仍在则拒绝"的准入（可能绕过正规卸载护栏）'
        }
        # 只报告能力：残留清理也必须能 dry-run
        if ($residueBlock -match 'dry_run' -and $residueBlock -match 'DRY-RUN') {
            Add-Pass '残留清理支持 --dry-run（先看后删）'
        } else {
            Add-Issue '残留清理不支持 --dry-run（用户无法先确认影响范围）'
        }
    } else {
        Add-Issue '未找到步骤 residue_pass（残留清理模式未接线）'
    }
    # 快捷方式的"匹配规则"必须只有一份实现：dry-run 报告与真实删除若各写一份，
    # 会出现"dry-run 说有 2 个、真删只删了 1 个"的静默漂移。
    if ($platformCode -match 'fn\s+desktop_shortcuts' -and
        $stepsCode -match 'platform::desktop_shortcuts\(\)' -and
        -not (Test-CodeMatch ([regex]::Match($stepsCode, '(?s)fn\s+print_plan.*?\n\}').Value) 'starts_with\("dsh harness"\)')) {
        Add-Pass '快捷方式匹配规则单一实现（列表与删除共用，dry-run 不再各写一份）'
    } else {
        Add-Issue '快捷方式匹配逻辑出现第二份实现（dry-run 报告可能与真实删除不一致）'
    }
    # 载荷清单必须与安装器 Payloads 完全一致（"安装器写什么，卸载器就清什么"）
    $payloadMatch = [regex]::Match($planCode, '(?s)const\s+PAYLOADS\s*:.*?\[(.*?)\];')
    $unPayloads = @()
    if ($payloadMatch.Success) {
        foreach ($m in [regex]::Matches($payloadMatch.Groups[1].Value, '"([^"]+)"')) { $unPayloads += $m.Groups[1].Value }
    }
    $setupPayloadMatch = [regex]::Match($setupText, '(?s)Payloads\s*=\s*new string\[\]\s*\{(.*?)\};')
    $setupPayloads = @()
    if ($setupPayloadMatch.Success) {
        foreach ($m in [regex]::Matches($setupPayloadMatch.Groups[1].Value, '"([^"]+)"')) { $setupPayloads += $m.Groups[1].Value }
    }
    if ($unPayloads.Count -gt 0 -and $setupPayloads.Count -gt 0) {
        $onlyUn = $unPayloads | Where-Object { $setupPayloads -notcontains $_ }
        $onlySetup = $setupPayloads | Where-Object { $unPayloads -notcontains $_ }
        if ($onlyUn.Count -eq 0 -and $onlySetup.Count -eq 0) {
            Add-Pass "卸载器载荷清单与安装器 Payloads 完全一致（$($unPayloads.Count) 项）"
        } else {
            Add-Issue ("载荷清单漂移：仅卸载器有 [$($onlyUn -join ', ')]，仅安装器有 [$($onlySetup -join ', ')]（会导致残留或误删）")
        }
    } else {
        Add-Issue '无法解析载荷清单（安装器 Payloads 或卸载器 PAYLOADS），无法校验漂移'
    }

    # 遗留载荷清单（上一版的脚本卸载器）也必须两侧一致：
    # 漏一处就会"升级后两套卸载器并存"，或卸载时把旧脚本误判成第三方文件而保留目录。
    $legacyMatch = [regex]::Match($planCode, '(?s)const\s+LEGACY_PAYLOADS\s*:.*?\[(.*?)\];')
    $unLegacy = @()
    if ($legacyMatch.Success) {
        foreach ($m in [regex]::Matches($legacyMatch.Groups[1].Value, '"([^"]+)"')) { $unLegacy += $m.Groups[1].Value }
    }
    $obsoleteMatch = [regex]::Match($setupText, '(?s)ObsoletePayloads\s*=\s*new string\[\]\s*\{(.*?)\};')
    $setupObsolete = @()
    if ($obsoleteMatch.Success) {
        foreach ($m in [regex]::Matches($obsoleteMatch.Groups[1].Value, '"([^"]+)"')) { $setupObsolete += $m.Groups[1].Value }
    }
    if ($unLegacy.Count -gt 0 -and $setupObsolete.Count -gt 0) {
        $onlyUn2 = $unLegacy | Where-Object { $setupObsolete -notcontains $_ }
        $onlySetup2 = $setupObsolete | Where-Object { $unLegacy -notcontains $_ }
        if ($onlyUn2.Count -eq 0 -and $onlySetup2.Count -eq 0) {
            Add-Pass "遗留载荷清单两侧一致（$($unLegacy.Count) 项：升级时安装器清除、卸载时卸载器清除）"
        } else {
            Add-Issue ("遗留载荷清单漂移：仅卸载器有 [$($onlyUn2 -join ', ')]，仅安装器有 [$($onlySetup2 -join ', ')]")
        }
    } else {
        Add-Issue '无法解析遗留载荷清单（安装器 ObsoletePayloads 或卸载器 LEGACY_PAYLOADS）'
    }
}

# ---------------------------------------------------------------------------
# 1d. 依赖安全审计入口 + GUI crate 默认 feature（防 advisory 回归）
# ---------------------------------------------------------------------------
Test-FileExists 'tools/osv-audit.ps1' '依赖安全审计脚本（OSV 通道）'
Test-FileExists 'docs/OPS-RUNBOOK.md' '发布/维护操作手册'

# 关默认 feature 不只是"离线可构建"：tao 的默认 feature 在 Linux 上会拉入 GTK 栈
# （gtk → glib 0.18.5 → glib-macros → proc-macro-error 1.0.4），而 glib 与
# proc-macro-error 各带一条 RustSec advisory（RUSTSEC-2024-0429 / RUSTSEC-2024-0370）。
# 一旦有人去掉该声明，Linux 构建就会真的引入这两个有漏洞的包。
$cargoNoComment = if ($null -ne $cargo) {
    (($cargo -split "`n") | ForEach-Object { $_ -replace '#.*$', '' }) -join "`n"
} else { '' }
foreach ($crate in @('tao', 'wry', 'tray-icon', 'muda')) {
    $line = ($cargoNoComment -split "`n" | Where-Object { $_ -match "^\s*$([regex]::Escape($crate))\s*=" })
    if ($line -and ($line -match 'default-features\s*=\s*false')) {
        Add-Pass "$crate 已关闭默认 feature（避免 Linux 拉入带 advisory 的 GTK 栈）"
    } elseif ($line) {
        Add-Issue "$crate 未关闭默认 feature：Linux 路径会拉入 gtk/glib（含 RUSTSEC-2024-0429）"
    } else {
        # P2-8：把 Cargo.toml 里这一行**删掉**时，`if`/`elseif` 双双落空 —— 既不 Pass
        # 也不 Issue，检查静默消失。必须显式断言"该 GUI crate 仍然存在"。
        Add-Issue "Cargo.toml 中找不到 GUI crate $crate 的依赖行（被删除？该检查将失去对象）"
    }
}


# 版本一致性校验脚本必须存在（全链路校验入口）
Test-FileExists 'tools/verify-version.ps1' '版本一致性校验脚本'

# 构建脚本必须在拷贝产物后校验版本资源（防止 rc.exe 静默失败）
$buildText = Read-Text 'build.ps1'
if ($null -ne $buildText) {
    if ($buildText -match 'verify-version\.ps1') { Add-Pass 'build.ps1 在打包后校验版本资源' }
    else { Add-Issue 'build.ps1 未调用 tools/verify-version.ps1（版本资源可能静默缺失）' }
}

# 内嵌资源不得成为死文件：settings.html / guide.html 必须被源码 include
$uiRefs = ''
foreach ($rel in @('crates/dsh-ui/src/settings.rs', 'crates/dsh-ui/src/guide.rs')) {
    $t = Read-Text $rel
    if ($null -ne $t) { $uiRefs += "`n$t" }
}
foreach ($html in @('ui/settings.html', 'ui/guide.html')) {
    if ($uiRefs -match [regex]::Escape($html)) { Add-Pass "$html 已被 Rust 源码内嵌引用" }
    else { Add-Issue "$html 没有任何源码引用（死资源：界面里根本看不到）" }
}

# 发布说明需记录实现期缺陷与验证方式（防止"只写功能不写坑"）
if ($null -ne $notes) {
    foreach ($k in @('失联', '孤儿锁', 'Job Object')) {
        if ($notes -match [regex]::Escape($k)) { Add-Pass "发布说明已记录：$k" }
        else { Add-Issue "发布说明缺少关键条目：$k" }
    }
}

# 路线图需含实施记录章节
$roadmap = Read-Text 'docs/TECHNICAL-ROADMAP.md'
if ($null -ne $roadmap) {
    if ($roadmap -match '## 13\. 实施记录') { Add-Pass '路线图含「13. 实施记录」章节' }
    else { Add-Issue '路线图缺少「13. 实施记录」章节' }
    if ($roadmap -match '回归清单逐项验收') { Add-Pass '路线图含回归清单验收结果' }
    else { Add-Issue '路线图缺少回归清单验收结果' }
}

# ---------------------------------------------------------------------------
# 2. 依赖精简：禁用 crate
# ---------------------------------------------------------------------------
$manifestText = ''
foreach ($m in @('Cargo.toml', 'crates/dsh-core/Cargo.toml', 'crates/dsh-ui/Cargo.toml', 'crates/dsh-app/Cargo.toml')) {
    $t = Read-Text $m
    if ($null -ne $t) { $manifestText += "`n# --- $m ---`n" + $t }
}

# 注意：注释里提到这些名字是允许的（说明"不使用"），因此按【依赖行】判定：
# 形如 `name = ...` 或 `name.workspace = true` / `name = { ... }`
$banned = @('tauri', 'tokio', 'regex', 'chrono', 'semver', 'tracing-subscriber', 'tracing-appender')
foreach ($b in $banned) {
    $pattern = "(?m)^\s*$([regex]::Escape($b))\s*(=|\.[a-z-]+\s*=)"
    if ($manifestText -match $pattern) {
        Add-Issue "依赖清单出现被禁用的 crate：$b"
    } else {
        Add-Pass "未引入禁用 crate：$b"
    }
}

# ---------------------------------------------------------------------------
# 3. 分层纯净：dsh-core 不得依赖 GUI
# ---------------------------------------------------------------------------
$coreToml = Read-Text 'crates/dsh-core/Cargo.toml'
if ($null -eq $coreToml) {
    Add-Issue 'crates/dsh-core/Cargo.toml 缺失'
} else {
    $guiCrates = @('wry', 'tao', 'tray-icon', 'muda')
    $dirty = @()
    foreach ($g in $guiCrates) {
        if ($coreToml -match "(?m)^\s*$([regex]::Escape($g))\s*(=|\.[a-z-]+\s*=)") { $dirty += $g }
    }
    if ($dirty.Count -gt 0) {
        Add-Issue ("dsh-core 依赖了 GUI crate（破坏可测试性）：" + ($dirty -join ', '))
    } else {
        Add-Pass 'dsh-core 无 GUI 依赖（可被 cargo test 覆盖）'
    }
}

# ---------------------------------------------------------------------------
# 4. LAN 彻底移除（源码 / UI / 脚本）
# ---------------------------------------------------------------------------
$scanDirs = @('crates', 'ui')
$lanMarkers = @(
    'lan-gateway', 'DSH_LAN_PIN', 'DSH_LAN_HOST', 'DSH_LAN_PORT', 'DSH_LAN_TOKEN',
    'OLLAMA_HOST', 'OLLAMA_ORIGINS', 'lan-pin', 'lan-token', 'lan-secret',
    'advfirewall', 'TryAddRule', 'DetectLanIp', 'IsGatewayRunning'
)
$lanHits = New-Object System.Collections.Generic.List[string]
foreach ($d in $scanDirs) {
    $dirPath = Join-Path $root $d
    if (-not (Test-Path $dirPath)) { continue }
    $files = Get-ChildItem -Path $dirPath -Recurse -File -Include *.rs, *.html, *.js, *.css -ErrorAction SilentlyContinue
    foreach ($f in $files) {
        foreach ($marker in $lanMarkers) {
            $hit = Select-String -Path $f.FullName -Pattern ([regex]::Escape($marker)) -SimpleMatch -ErrorAction SilentlyContinue
            if ($hit) {
                $rel = $f.FullName.Replace("$root\", '')
                $lanHits.Add("$rel : $marker")
            }
        }
    }
}
if ($lanHits.Count -gt 0) {
    Add-Issue ('局域网关联标识残留：' + (($lanHits | Select-Object -Unique) -join '; '))
} else {
    Add-Pass '源码与 UI 无局域网关联标识残留'
}

# LAN 资源文件必须不存在
foreach ($f in @('lan-gateway.mjs', 'whale-256.png')) {
    if (Test-Path (Join-Path $root $f)) { Add-Issue "LAN 资源未删除：$f" }
    else { Add-Pass "LAN 资源已删除：$f" }
}

# ---------------------------------------------------------------------------
# 4b. 单文件分发：不得保留旁挂 DLL 与旧 C# 启动器源码
# ---------------------------------------------------------------------------
$libDir = Join-Path $root 'lib'
if (Test-Path $libDir) {
    $dlls = Get-ChildItem -Path $libDir -Recurse -Filter *.dll -ErrorAction SilentlyContinue
    if ($dlls) {
        Add-Issue ("仍存在旁挂 DLL（v5 由 windows-link 静态链接，应删除）：" + (($dlls | ForEach-Object { $_.Name }) -join ', '))
    } else {
        Add-Pass '无旁挂 DLL 残留（lib/ 为空目录）'
    }
} else {
    Add-Pass '无旁挂 DLL 目录（lib/ 已删除）'
}

foreach ($f in @('DSHLauncher.cs', 'LanAccess.cs')) {
    if (Test-Path (Join-Path $root $f)) { Add-Issue "旧 C# 启动器源码未删除（已被 Rust 取代）：$f" }
    else { Add-Pass "旧 C# 启动器源码已删除：$f" }
}

# build-setup.ps1 不得再内嵌 WebView2 DLL
$setupPs1 = Read-Text 'build-setup.ps1'
if ($null -ne $setupPs1) {
    if ($setupPs1 -match 'WebView2Loader\.dll|Microsoft\.Web\.WebView2') {
        Add-Issue 'build-setup.ps1 仍内嵌 WebView2 DLL（v5 无需旁挂）'
    } else {
        Add-Pass 'build-setup.ps1 不再内嵌 WebView2 DLL'
    }
}

# ---------------------------------------------------------------------------
# 5. 关键文件齐全
# ---------------------------------------------------------------------------
Test-FileExists 'Cargo.lock'                  '依赖锁文件（离线可复现）'
Test-FileExists 'build.ps1'                   '构建脚本'
Test-FileExists 'selftest.ps1'                '自检脚本'
Test-FileExists 'ui/settings.html'            '设置页'
Test-FileExists 'ui/guide.html'               '新手指引页'
Test-FileExists 'crates/dsh-app/build.rs'     '图标嵌入构建脚本'
Test-FileExists 'crates/dsh-core/examples/job_object_demo.rs' 'Job Object 验证程序'

# ---------------------------------------------------------------------------
# 5b. 自检脚本的判据必须与产品**实际**行为对齐
#
# 背景（实测，同一类问题已踩两次）：
#   * 脚本用**日志措辞**判定成败，产品改了措辞 ⇒ 断言永远失败（假负例）。
#     `tools/verify-token-navigation.ps1` 的同类误报已修，`tools/verify-monitor.ps1` 漏了：
#     它 grep 的「已接管端口」在产品里**已不存在**（现在打印「直接接管」），
#     于是自检 E 段无论环境如何都判失败；
#   * 脚本只检查"端口在听"就断言"外部 dsh 就绪"：外部 dsh 因端口被占而绑不上时，
#     后续「杀外部 dsh → 端口仍被占用 → 检测不到失联 → 未重新接管」全是**误导性** FAIL；
#   * 默认端口写死 3099，而 3099 可能**正是用户当前会话的端口**，脚本还会去杀该端口的所有者。
# ---------------------------------------------------------------------------
$monitorRaw = Read-Text 'tools/verify-monitor.ps1'
if ($null -eq $monitorRaw) {
    Add-Issue '缺少 tools/verify-monitor.ps1（自检 E 段的失联检测验证）'
} else {
    $monitorCode = Get-CodeText 'tools/verify-monitor.ps1' $monitorRaw
    # 判据必须与产品真实措辞对齐。
    #
    # 注意：`Get-CodeText` 的注释剥离是为 **Rust** 写的（只认 `//` 与 `/* */`），
    # **不认 PowerShell 的 `#`** —— 所以不能靠"剥注释后不含旧措辞"来判定
    # （本断言第一版正是如此，结果被自己注释里提到的旧措辞判成失败）。
    # 这里直接锚定**判定条件本身**，并与产品源码交叉印证。
    $serviceRsForMonitor = Read-Text 'crates/dsh-app/src/service.rs'
    $adoptRx = 'if\s*\(\s*\$t2\s+-match\s+''直接接管'''
    if ($null -ne $serviceRsForMonitor -and $serviceRsForMonitor -match '直接接管' -and
        $monitorRaw -match $adoptRx) {
        Add-Pass '失联检测验证的接管判据与产品实际措辞一致（产品打印「直接接管」，脚本按同一措辞判定）'
    } else {
        Add-Issue '失联检测验证的接管判据与产品措辞不一致（措辞漂移会让该断言永远失败）'
    }
    # 前置校验：端口上监听的必须**正是**本次启动的进程
    if ($monitorCode -match 'function\s+Assert-Owner' -and
        ([regex]::Matches($monitorCode, 'Assert-Owner\s+\$Port')).Count -ge 2) {
        Add-Pass '失联检测验证校验端口监听者确实是本次启动的进程（杜绝误导性 FAIL）'
    } else {
        Add-Issue '失联检测验证未校验端口所有权：外部 dsh 绑不上端口时会给出误导性失败'
    }
    # 绝不结束不是自己启动的进程；默认端口自动挑选而非写死
    if ($monitorCode -match 'Get-FreePort' -and $monitorCode -match '\$cleanup -notcontains' -and
        -not ($monitorCode -match 'TASKKILL\s+/F\s+/PID\s+\$left\.OwningProcess')) {
        Add-Pass '失联检测验证只结束自己启动的进程，且默认使用自动挑选的空闲端口'
    } else {
        Add-Issue '失联检测验证可能结束不属于自己的进程（会打断用户正在使用的会话）'
    }
}

# ---------------------------------------------------------------------------
# 6. feature 修正（照抄旧文档会编译失败的两处）
# ---------------------------------------------------------------------------
# 剥离 TOML 行内注释后再判定：注释中说明"不使用某 feature"不应算违规
$cargoStripped = if ($null -ne $cargo) {
    (($cargo -split "`n") | ForEach-Object { $_ -replace '#.*$', '' }) -join "`n"
} else { '' }

if ($null -ne $cargo) {
    if ($cargoStripped -match 'Win32_Graphics_Dwm') { Add-Pass 'windows 使用正确的 Win32_Graphics_Dwm' }
    else { Add-Issue 'windows 缺少 Win32_Graphics_Dwm feature' }

    if ($cargoStripped -match 'Win32_UI_Dwm') { Add-Issue 'windows 使用了不存在的 Win32_UI_Dwm（应为 Win32_Graphics_Dwm）' }
    else { Add-Pass 'windows 未使用不存在的 Win32_UI_Dwm' }

    $missingNoDefault = @()
    $absentCrates = @()
    foreach ($c in @('tray-icon', 'muda', 'wry', 'tao')) {
        $line = ($cargoStripped -split "`n" | Where-Object { $_ -match "^\s*$([regex]::Escape($c))\s*=" })
        if ($line -and ($line -notmatch 'default-features\s*=\s*false')) { $missingNoDefault += $c }
        elseif (-not $line) { $absentCrates += $c }
    }
    if ($missingNoDefault.Count -gt 0) {
        Add-Issue ("以下 crate 未关闭默认 feature（会拉入 Linux 依赖导致离线解析失败）：" + ($missingNoDefault -join ', '))
    } elseif ($absentCrates.Count -gt 0) {
        # P2-8：依赖行被整行删除时不能静默通过 —— 显式断言该 crate 仍然存在。
        Add-Issue ("Cargo.toml 中找不到以下 GUI crate 的依赖行（被删除？检查失去对象）：" + ($absentCrates -join ', '))
    } else {
        Add-Pass 'GUI crate 均已 default-features = false'
    }

    if ($cargoStripped -match 'Win32_Security') { Add-Pass 'windows 含 Win32_Security（SECURITY_ATTRIBUTES）' }
    else { Add-Issue 'windows 缺少 Win32_Security feature' }

    if ($cargoStripped -match 'Win32_Networking_WinSock') { Add-Pass 'windows 含 Win32_Networking_WinSock（AF_INET）' }
    else { Add-Issue 'windows 缺少 Win32_Networking_WinSock feature' }

    if ($cargoStripped -match 'opt-level\s*=\s*"z"' -and $cargoStripped -match 'lto\s*=\s*true' -and $cargoStripped -match 'panic\s*=\s*"abort"' -and $cargoStripped -match 'strip\s*=\s*true') {
        Add-Pass 'release profile 已启用体积优化（opt-level=z / lto / panic=abort / strip）'
    } else {
        Add-Issue 'release profile 体积优化不完整'
    }
}

# ---------------------------------------------------------------------------
# 7. v5.0.0 审计后的行为性硬约束（每一项都对应一个已修复的真实缺陷）
#
# 为什么进一致性校验而不是只写单测：这些都是「接线类」缺陷——代码单元都正确，
# 但没人调用 / 默认值反了 / 某条路径漏了通知。单测覆盖不到，只有源码级检查能防回归。
#
# 注意：本节的断言一律基于 Get-CodeText（已剥注释），并且尽量要求**调用/定义形式**
# （`foo(` / `fn foo`），而不是裸标识符 —— 否则一句注释或文档链接就能让它通过（P1-11）。
# ---------------------------------------------------------------------------
$procRs = Read-Text 'crates/dsh-core/src/process.rs'
if ($null -ne $procRs) {
    # 只在**非注释源码**里判定：注释/文档里提到某个 API 名，过去会让这些
    # "接线类"断言假通过（P1-11）。
    $procCode = Get-CodeText 'crates/dsh-core/src/process.rs' $procRs
    # 缺陷 A1：dsh 绑死在启动器的 Job 上，退出/崩溃/升级即切断会话
    # 必须是真实的常量定义 / 使用（`const NAME: u32` 或作为参数出现），
    # 光是注释里写一句 CREATE_BREAKAWAY_FROM_JOB 不算。
    if ($procCode -match '(?m)^\s*const\s+CREATE_BREAKAWAY_FROM_JOB\s*:' -or
        $procCode -match '\|\s*CREATE_BREAKAWAY_FROM_JOB\b') {
        Add-Pass '子进程支持脱离父 Job（CREATE_BREAKAWAY_FROM_JOB 已定义并用于 creation_flags）'
    } else {
        Add-Issue '子进程无法脱离父 Job：启动器退出/崩溃会切断正在进行的 dsh 会话'
    }
    if ($procCode -match '\bChildLifecycle::Independent\b' -and $procCode -match '\bChildLifecycle::Tied\b') {
        Add-Pass '保留 independent / tied 两种服务生命周期语义'
    } else {
        Add-Issue '缺少服务生命周期语义之一（应为 independent 默认 + tied 可选）'
    }
    # 缺陷 B2：stderr 管道未排空 → 子进程写满缓冲区后假死
    # 要求**调用或定义形式**（`spawn_stderr_drain(` / `fn spawn_stderr_drain`），
    # 而不是随手写个名字（文档链接 `[`spawn_stderr_drain`]` 就能满足旧断言）。
    if ($procCode -match '(?:\bfn\s+spawn_stderr_drain\b|\bspawn_stderr_drain\s*\()') {
        Add-Pass 'stderr 已被持续排空（防管道写满导致子进程假死）'
    } else {
        Add-Issue 'stderr 接了管道但未排空：dsh 写满缓冲区会永久阻塞'
    }
    # 缺陷：eprintln! 在 GUI 子系统下写失败会 panic（panic=abort ⇒ 整进程崩溃）
    if ($procCode -match 'eprintln!') {
        Add-Issue 'process.rs 仍使用 eprintln!（GUI 子系统下 stderr 无效时会 panic）'
    } else {
        Add-Pass 'process.rs 不使用 eprintln!（改为忽略错误的 stderr 写入）'
    }
}

$cfgRs = Read-Text 'crates/dsh-core/src/config.rs'
if ($null -ne $cfgRs) {
    $cfgCode = Get-CodeText 'crates/dsh-core/src/config.rs' $cfgRs
    # 缺陷 A1 的默认值：必须默认「服务独立于启动器」
    if ($cfgCode -match '(?s)enum ServiceLifecycle\b.*?#\[default\]\s*\r?\n\s*Independent') {
        Add-Pass 'service_lifecycle 默认 independent（退出启动器不切断会话）'
    } else {
        Add-Issue 'service_lifecycle 默认值不是 independent'
    }
    # schema 版本必须有**真实定义**（`const CURRENT_SCHEMA_VERSION: u32 = ...`），
    # 而不是只被文档/注释提及。
    if ($cfgCode -match '(?m)^\s*(?:pub\s+)?const\s+CURRENT_SCHEMA_VERSION\s*:') {
        Add-Pass '配置含 schema 版本定义（便于将来迁移）'
    } else {
        Add-Issue '配置缺少 schema 版本字段定义'
    }
}

$svcRs = Read-Text 'crates/dsh-app/src/service.rs'
if ($null -ne $svcRs) {
    $svcCode = Get-CodeText 'crates/dsh-app/src/service.rs' $svcRs
    # 缺陷 A2：Ready 事件必须能表达「就绪但暂无 token」，否则会用 401 地址导航
    # 注意：判定要**与格式无关**——cargo fmt 会把结构体变体折成一行，写死换行的正则会失效
    if ($svcCode -match 'Ready\s*\{[^}]*url\s*:\s*Option<String>') {
        Add-Pass 'Ready 事件的 url 为 Option（可表达「就绪但暂无 token」）'
    } else {
        Add-Issue 'Ready 事件未使用 Option：无 token 时会被导航到 HTTP 401 页面'
    }
    # 日志留痕必须是**真实字符串实参**（`logger.xxx("已捕获 dsh 就绪地址...")`），
    # 注释里写一句同样的话不算"留痕"。
    if ($svcCode -match '"\s*已捕获 dsh 就绪地址') {
        Add-Pass '存在 token 捕获回路的日志留痕'
    } else {
        Add-Issue '缺少 token 捕获回路的留痕（无法证明链路生效）'
    }
    # 缺陷 B1：必须做进程身份校验，否则会误杀无关程序
    # 要求"调用形式"，防止只留下文档提及。
    # 真正的身份判定函数定义在 dsh-core::process，本处断言的是**接管/停止路径确实调用它**。
    if ($svcCode -match 'is_dsh_harness_process\s*\(') {
        Add-Pass '接管/停止前做 dsh 进程身份校验（防误杀无关程序）'
    } else {
        Add-Issue '未做进程身份校验：端口被非 dsh 程序占用时可能被误杀'
    }
    # 独立生命周期的归属保障：service.json 簿记 + 启动对账
    if ($svcCode -match '\bServiceRecord\b' -and $svcCode -match 'reconcile\s*\(') {
        Add-Pass '使用 service.json 簿记 + 启动对账（独立生命周期的归属保障）'
    } else {
        Add-Issue '缺少 service.json 簿记 / 启动对账'
    }
}

$appRs = Read-Text 'crates/dsh-app/src/app.rs'
if ($null -ne $appRs) {
    $appCode = Get-CodeText 'crates/dsh-app/src/app.rs' $appRs
    # 缺陷 A2 的关键：只有带 token 的地址才允许写入 pending_url
    if ($appCode -match '(?s)if contains_token\(&u\)\s*\{\s*self\.pending_url') {
        Add-Pass '只有带 token 的地址才写入 pending_url'
    } else {
        Add-Issue 'pending_url 可能被无 token 地址覆盖（等待逻辑会退化为不可达）'
    }
    # 缺陷 B3：tray_on_close 必须有真实消费方
    if ($appCode -match '(?s)fn request_close_window.*?config\.tray_on_close') {
        Add-Pass 'tray_on_close 已被消费（关闭窗口 = 隐藏到托盘）'
    } else {
        Add-Issue 'tray_on_close 无消费方：界面承诺「关闭最小化到托盘」但实现是销毁窗口'
    }
    # P0：托盘「打开界面」/ 双击图标的唤起必须能**恢复被隐藏的窗口**。
    # 旧实现的 `pump_pending_open` 在 `harness.is_some()` 时直接 return，
    # 于是 `open_harness` 里「显示已存在窗口」的分支**永远不可达**：
    # 窗口一旦关闭到托盘（`tray_on_close = true`，默认行为）就再也回不来。
    $pumpBody = [regex]::Match($appCode, '(?s)fn pump_pending_open.*?\n    \}').Value
    if ($pumpBody -match 'harness\.is_some') {
        Add-Issue 'pump_pending_open 在窗口已存在时短路返回：关闭到托盘的窗口无法被重新显示'
    } elseif ($pumpBody -notmatch 'open_harness') {
        Add-Issue 'pump_pending_open 未驱动 open_harness：待打开的界面永远不会出现'
    } elseif ($appCode -notmatch '(?s)if self\.harness\.is_some\(\)\s*\{\s*let url = self\.auth_url_settled\(\)') {
        Add-Issue 'open_harness 缺少「窗口已存在 ⇒ 显示」分支：隐藏到托盘的窗口无法恢复'
    } else {
        Add-Pass '隐藏到托盘的窗口可被恢复（open_harness 显示已存在窗口，pump 不短路）'
    }
    # 缺陷 E1：主题采样必须被接线（要求调用形式）
    if ($appCode -match 'begin_theme_sample\s*\(') {
        Add-Pass '主题采样已接线（异步回调，不阻塞事件循环）'
    } else {
        Add-Issue '主题采样未接线：标题栏不会跟随页面主题'
    }
    if ($appCode -match 'eprintln!') {
        Add-Issue 'app.rs 仍使用 eprintln!（GUI 子系统下会 panic）'
    } else {
        Add-Pass 'app.rs 不使用 eprintln!'
    }
    # F5/Ctrl+R/Esc 在 ui/guide.html 里承诺过，必须有实现
    if ($appCode -match 'WindowEvent::KeyboardInput') {
        Add-Pass '已处理键盘事件（F5 / Ctrl+R / Esc 不是空承诺）'
    } else {
        Add-Issue '未处理键盘事件：新手指引承诺的 F5/Ctrl+R/Esc 全部无效'
    }

    # 缺陷：「清理归档会话」会停服务却不告知、也不把服务拉回来
    #（实测：用户以为是启动器自己断了他的会话，且清理后界面打不开、只能手动启动服务）
    # 要求调用形式：只在注释里写 confirm_dangerous 不算"有二次确认"。
    if ($appCode -match 'confirm_dangerous\s*\(') {
        Add-Pass '「清理归档会话」有二次确认（明示"停服务 / 会话中断 / 不可恢复"）'
    } else {
        Add-Issue '「清理归档会话」无二次确认：会静默停掉正在服务的 dsh'
    }
    if ($appCode -match '(?:fn\s+restore_service_after_cleanup\b|restore_service_after_cleanup\s*\()') {
        Add-Pass '归档清理结束后按需自动重启服务（不会停在"服务已停"状态）'
    } else {
        Add-Issue '归档清理后不恢复服务：用户会停在"服务已停、界面打不开"的状态'
    }
    # 缺陷：清理会打断正在运行的会话，却没有任何预警（要求调用形式：探测必须被真的调用）
    if ($appCode -match 'probe_active_sessions\s*\(') {
        Add-Pass '清理前探测活跃会话（避免打断正在对话/跑任务）'
    } else {
        Add-Issue '清理前不探测活跃会话：会静默打断正在进行的对话或任务'
    }
    # 修复：该探测（外部命令，实测约 1.3 秒）必须在 **worker 线程**执行。
    # 旧实现直接跑在 UI 线程上 ⇒「点清理按钮后界面卡住一秒多」，
    # 与 `ui-thread-never-blocks` 的修复目标冲突。
    if ($appCode -match 'dsh-cleanup-check' -and $appCode -match 'CleanupPrecheck') {
        Add-Pass '归档清理前的活跃会话探测在 worker 线程执行（UI 线程不阻塞）'
    } else {
        Add-Issue '归档清理前的活跃会话探测可能在 UI 线程执行（界面会卡住一秒以上）'
    }

    # 退出策略：independent 不弹窗（服务无碍），tied 每次询问（退出会停服务）
    # 缺陷：勾了「服务独立于启动器」却每次退出都弹窗 —— 选项形同虚设，用户会以为没生效
    if ($appCode -match '(?s)fn begin_exit.*?if !tied\s*\{') {
        Add-Pass 'independent 模式下退出不弹窗（服务独立则无需每次确认）'
    } else {
        Add-Issue 'independent 模式下退出仍会弹窗：该设置形同虚设'
    }
    # 注意：真实调用带模块前缀（crate::dialog::confirm_service_stop(...)），
    # 因此用词边界匹配调用形式，而不是要求它紧跟在 fn 后面。
    if ($appCode -match '\bconfirm_service_stop\(own, adopted, true\)') {
        Add-Pass 'tied 模式下退出仍会询问（退出会停服务，必须确认）'
    } else {
        Add-Issue 'tied 模式下退出不询问：会静默停掉正在服务的 dsh'
    }

    # 缺陷：只是取消勾选「服务独立于启动器」，却重启服务把正在进行的会话打断
    #（实测：服务进入 Error → 退出时连"要不要停服务"都没有可询问的对象）
    # only_lifecycle 是本地变量，要求它以 `= ` 赋值的形式真实存在。
    if ($appCode -match '\bonly_lifecycle\b\s*=') {
        Add-Pass '生命周期变更不重启服务（取消勾选不会打断会话）'
    } else {
        Add-Issue '生命周期变更会重启服务：仅切换设置就会打断正在进行的会话'
    }
}

# 活跃会话探测必须存在且信号完整（进程信号 + 文件信号）
$maintRs2 = Read-Text 'crates/dsh-core/src/maintenance.rs'
if ($null -ne $maintRs2) {
    $maintCode2 = Get-CodeText 'crates/dsh-core/src/maintenance.rs' $maintRs2
    # "会话运行器进程"这一硬信号 = 真的写了一个按命令行里的 dsh-subprocess-local
    # 统计 runner 进程数的函数，并且函数体里确实带着这个匹配串。
    # 注意：该标记在源码里被**嵌在 powershell 脚本的字符串字面量内**（`... -like
    # '*dsh-subprocess-local*'`），所以不能要求出现带引号的独立字面量 `"dsh-..."`；
    # 但"函数名 + 标记串同时在非注释代码里"足以排除"只在注释/说明里提一句"的假通过。
    $hasRunnerScan = $maintCode2 -match 'fn\s+count_session_runner_processes\b'
    $hasRunnerMarker = $maintCode2 -match 'dsh-subprocess-local'
    if ($hasRunnerScan -and $hasRunnerMarker) {
        Add-Pass '活跃会话探测存在且含"会话运行器进程"这一硬信号'
    } else {
        Add-Issue '活跃会话探测缺少"会话运行器进程"信号，仅靠文件时间会误判'
    }
    if ($maintCode2 -match '(?m)^\s*(?:pub\s+)?const\s+ACTIVE_SESSION_WINDOW\s*:') {
        Add-Pass '活跃会话探测含"文件近期写入"软信号与窗口常量'
    } else {
        Add-Issue '活跃会话探测缺少文件近期写入信号'
    }
}

# 生命周期切换不得杀掉在跑的子进程（Job 归属是创建时决定的，只能对以后的新进程生效）
$svcRs2 = Read-Text 'crates/dsh-app/src/service.rs'
if ($null -ne $svcRs2) {
    $svcCode2 = Get-CodeText 'crates/dsh-app/src/service.rs' $svcRs2
    # ---------------------------------------------------------------------
    # 断言：生命周期切换**只记录、不杀子进程**。
    #
    # 为什么不能直接写死旧正则 `lifecycle\(\) != lifecycle.*?set_lifecycle`：
    # rustfmt 之后真实代码是 `p.lifecycle()`（`!=` 左侧带 `p.`），旧正则在当前
    # 仓库里**根本匹配不上**，于是这条断言一直是 FAIL（而非"检查通过"）。
    #
    # 也不能写成"全文出现 set_lifecycle 且全文没有 stop"：那既依赖书写顺序、
    # 又会把文件里任何一处无关的 stop 卷进来，退化成"全文禁止 stop"。
    #
    # 这里采用最直接、可逐条验证的判定：
    #   1) 必须在**非注释源码**里找到生命周期决策的两个端点（p.lifecycle() 比较、
    #      set_lifecycle(lifecycle) 调用），否则视为接线缺失 → FAIL；
    #   2) 从比较点起、到 set_lifecycle 之后 500 字符的有界区间内，不得出现
    #      `<x>.stop(` —— 这就是"先停服务再改设置"的真实缺陷形态。
    #      注意模式必须是 `[.]stop\s*\(` 而不是 `process[.]stop\(\)`：真实调用形如
    #      `inner.with_process(|p| p.stop())`（接收者是 `p`），写成 literally
    #      `process.stop()` 永远匹配不上 —— 那样断言会变成恒真，等于没检查。
    # 这条断言仍然**可失败**：把 `.stop(` 插到比较点与 set_lifecycle 之间即会 FAIL
    # （已用副本注入实验验证）。
    # ---------------------------------------------------------------------
    $lcCmp = [regex]::Match($svcCode2, 'p\.lifecycle\(\)')
    $lcSet = [regex]::Match($svcCode2, 'set_lifecycle\(lifecycle\)')
    $lcOk = $false
    if ($lcCmp.Success -and $lcSet.Success -and $lcSet.Index -gt $lcCmp.Index) {
        $gap = ($lcSet.Index - $lcCmp.Index) + 500
        if ($gap -le 2500) {
            $between = $svcCode2.Substring($lcCmp.Index, $gap)
            $lcOk = ($between -notmatch '[.]stop\s*\(')
        } else {
            # 两个端点相距过远：说明决策已被挪走/结构被改坏，不能默认通过
            $lcOk = $false
        }
    }
    if ($lcOk) {
        Add-Pass '生命周期切换只记录、不杀子进程（切换设置不会打断会话）'
    } else {
        Add-Issue '生命周期切换会杀掉在跑的子进程：仅切换设置就会打断正在进行的会话'
    }
    # 身份判定必须区分「明确不是 dsh」与「读不到信息」，否则会谎报"被非 dsh 程序占用"
    # 三态判定函数定义在 dsh-core::process，本处断言的是**调用形式**（名字出现在注释里不算）。
    if ($svcCode2 -match 'classify_dsh_identity\s*\(') {
        Add-Pass '端口占用者身份判定用三态（区分"不是 dsh"与"读不到"）'
    } else {
        Add-Issue '身份判定未区分三态：读不到进程信息时会谎报"被非 dsh 程序占用"'
    }
    # 可诊断入口（--probe-identity）：真正的定义在 cli.rs / main.rs，
    # 旧断言在 service.rs 上匹配裸名 'probe_identity'，既指错了文件、也能被注释满足。
    $cliRs = Read-Text 'crates/dsh-app/src/cli.rs'
    $mainCliRs = Read-Text 'crates/dsh-app/src/main.rs'
    $cliCode = Get-CodeText 'crates/dsh-app/src/cli.rs' $cliRs
    $mainCliCode = Get-CodeText 'crates/dsh-app/src/main.rs' $mainCliRs
    if ($cliCode -match '"--probe-identity"' -and $mainCliCode -match 'args\.probe_identity') {
        Add-Pass '身份判定提供可诊断入口（--probe-identity 已解析并在 main 中消费）'
    } else {
        Add-Issue '身份判定缺少可诊断入口（--probe-identity 未接线）'
    }
}

# 发布脚本必须支持热替换（否则每次发布都要先退出启动器 → 中断会话）
$buildPs1 = Read-Text 'build.ps1'
if ($null -ne $buildPs1) {
    if ($buildPs1 -match 'Rename-Item' -and $buildPs1 -match 'Copy-Item') {
        Add-Pass 'build.ps1 支持热替换（改名让位后放入，无需退出启动器）'
    } else {
        Add-Issue 'build.ps1 只做直接覆盖：启动器运行时发布必失败，用户只能先退出（会中断会话）'
    }
    if ($buildPs1 -match 'pubtmp') {
        Add-Pass 'build.ps1 会清理历史产物（*.bak-* / *.old-* / *.pubtmp）'
    } else {
        Add-Issue 'build.ps1 不清理历史产物，根目录会堆积多个历史 exe'
    }
}

# 崩溃取证：panic=abort 下必须能留下 FATAL 记录
$crashRs = Read-Text 'crates/dsh-app/src/crash.rs'
if ($null -eq $crashRs) {
    Add-Issue '缺少 crash.rs：panic=abort 下崩溃时日志里没有任何记录'
} else {
    $crashCode = Get-CodeText 'crates/dsh-app/src/crash.rs' $crashRs
    # 必须是**真实的注册调用**：`SetUnhandledExceptionFilter(Some(unhandled_filter))`。
    # 光有 `use ...SetUnhandledExceptionFilter` 导入（甚至只是注释里提一句名字）
    # 都不能证明过滤器真的被注册 —— 导入而不调用，崩溃时依然没有任何记录。
    if ($crashCode -match 'SetUnhandledExceptionFilter\s*\(') {
        Add-Pass '已注册未处理异常过滤器（崩溃取证）'
    } else {
        Add-Issue 'crash.rs 未注册 SetUnhandledExceptionFilter'
    }
    if ($crashCode -match 'format!') {
        Add-Issue 'crash.rs 的异常处理器使用了 format!（崩溃上下文不可分配内存）'
    } else {
        Add-Pass '崩溃处理器无内存分配（只用栈缓冲 + WriteFile）'
    }
}

# 维护路径：锁收集不得跟随 reparse point；workspace.json 改写必须有护栏
$maintRs = Read-Text 'crates/dsh-core/src/maintenance.rs'
if ($null -ne $maintRs) {
    $maintCode = Get-CodeText 'crates/dsh-core/src/maintenance.rs' $maintRs
    if ($maintCode -match '(?s)fn collect_lock_files.*?is_reparse_point\s*\(') {
        Add-Pass '锁文件收集拒绝跟随 reparse point（防越界删除）'
    } else {
        Add-Issue '锁文件收集可能跟随 junction/符号链接，越界删除外部文件'
    }
    if ($maintCode -match '\bunique_key_index\b') {
        Add-Pass 'workspace.json 改写前检查 key 唯一性（防区间错位写坏文件）'
    } else {
        Add-Issue 'workspace.json 改写缺少 key 唯一性护栏'
    }
}

# 日志：归档文件数必须可被校验（旧实现会多出第 4 个文件）
$logRs = Read-Text 'crates/dsh-core/src/log.rs'
if ($null -ne $logRs) {
    $logCode = Get-CodeText 'crates/dsh-core/src/log.rs' $logRs
    # 要求定义/调用形式（`fn retained_file_count(`），文档链接 `[`retained_file_count`]` 不算。
    if ($logCode -match '\bfn\s+retained_file_count\s*\(' -or $logCode -match '\bretained_file_count\s*\(') {
        Add-Pass '日志提供 retained_file_count()（滚动文件数可被校验）'
    } else {
        Add-Issue '日志未暴露保留文件数，无法校验「2 MB × 3 份」是否属实'
    }
}

# 验证脚本必须存在（把「已修复」变成「可复验」）
Test-FileExists 'tools/verify-deployed.ps1' '落地校验脚本（产物标记 + 窗口）'
Test-FileExists 'tools/verify-service-lifecycle.ps1' '服务生命周期验证脚本'
Test-FileExists 'tools/verify-token-navigation.ps1'  'token 导航验证脚本'
# 运行期探针：用与生产完全相同的代码路径验证「stdout token 捕获是否真的会触发」。
# 这是最容易静默失效的一环（管道接线/前缀/时机任一变化都会让它失效，而日志里
# 只表现为「少了一行」），因此必须有可执行的运行期证据，而不是只靠源码扫描。
Test-FileExists 'crates/dsh-core/examples/token_capture_probe.rs' 'token 捕获运行期探针'
$procExample = Read-Text 'crates/dsh-core/examples/token_capture_probe.rs'
if ($null -ne $procExample) {
    $procExCode = Get-CodeText 'crates/dsh-core/examples/token_capture_probe.rs' $procExample
    # 必须是真实调用/类型引用，而不是注释里写"走生产路径"。
    if ($procExCode -match 'start_dsh\s*\(' -and $procExCode -match '\bReadyHook\b' -and $procExCode -match '\bReadyProbe\b') {
        Add-Pass 'token 捕获探针走生产路径（start_dsh + ReadyHook + ReadyProbe）'
    } else {
        Add-Issue 'token 捕获探针未走生产路径，验证结论不可信'
    }
    if ($procExCode -match 'is_port_listening\(port\)' -and $procExCode -match 'pm\.stop\(\)') {
        Add-Pass 'token 捕获探针自带残留自检（停止后端口必须释放）'
    } else {
        Add-Issue 'token 捕获探针未自检残留'
    }
}

# 必须有脚本可用的**优雅**退出入口（否则发布脚本只能强杀 → 切断会话）
$cliRs = Read-Text 'crates/dsh-app/src/cli.rs'
$mainRsQuit = Read-Text 'crates/dsh-app/src/main.rs'
if ($null -ne $cliRs -and $null -ne $mainRsQuit) {
    # 都要求非注释源码里的真实定义/调用形式
    $cliQuitCode = Get-CodeText 'crates/dsh-app/src/cli.rs' $cliRs
    $mainQuitCode = Get-CodeText 'crates/dsh-app/src/main.rs' $mainRsQuit
    if ($cliQuitCode -match '"--quit"' -and $mainQuitCode -match 'request_existing_instance_quit\s*\(') {
        Add-Pass '提供 --quit 优雅退出入口（脚本/CI 可编排，无需强杀）'
    } else {
        Add-Issue '缺少 --quit 优雅退出入口：发布脚本只能强杀启动器（会切断 dsh 会话）'
    }
}

# 文档数字单一来源：FACTS.json 必须存在且提供校验入口
Test-FileExists 'docs/FACTS.json' '文档数字单一来源（FACTS.json）'
Test-FileExists 'tools/gen-facts.ps1' '事实文件生成/校验脚本'

# ---------------------------------------------------------------------------
# 7c. 发布工程与运行时加固的硬约束（发布工程轮新增）
#
# 每一条都对应一个"会在发布/运行期真实咬人"的缺陷，且都能用**非注释源码**判定：
#   * 没有 --version/--help ⇒ 安装后无法自动化核对版本（--version 会被当成普通启动，
#     抢互斥体、弹窗、甚至唤起已有实例）；
#   * LTS 若写进 Cargo 版本 ⇒ 变成 semver 预发布版本（5.0.0-LTS < 5.0.0），
#     会改变依赖解析与 npm dist-tag 语义；
#   * UI 线程里的阻塞探测 ⇒ 每帧一次 300 ms connect（服务未起来时界面几乎全卡）；
#   * 无超时的外部命令 ⇒ PowerShell/WMI 一卡，清理流程与界面状态永久挂住；
#   * 用 STILL_ACTIVE 哨兵判存活 ⇒ `exit(259)` 的进程被永久判为"活着"；
#   * 递归杀进程树 ⇒ 深树/环状父子关系吃爆线程栈（dsh-stop 线程只有 256 KB）。
# ---------------------------------------------------------------------------
$cliPubRs = Read-Text 'crates/dsh-app/src/cli.rs'
$mainPubRs = Read-Text 'crates/dsh-app/src/main.rs'
if ($null -ne $cliPubRs -and $null -ne $mainPubRs) {
    $cliPubCode = Get-CodeText 'crates/dsh-app/src/cli.rs' $cliPubRs
    $mainPubCode = Get-CodeText 'crates/dsh-app/src/main.rs' $mainPubRs
    if ($cliPubCode -match '"--version"\s*\|\s*"-V"' -and $cliPubCode -match '"--help"\s*\|\s*"-h"' -and
        $mainPubCode -match 'args\.version' -and $mainPubCode -match 'args\.help') {
        Add-Pass '提供 --version / --help（安装包与 CI 可自动核对版本）'
    } else {
        Add-Issue '缺少 --version / --help：安装后无法自动核对版本（会被当成普通启动）'
    }
    # 发布通道标签必须与版本号分离
    if ($cliPubCode -match 'RELEASE_CHANNEL' -and $cliPubCode -match 'version_text\s*\(') {
        if ($EXPECT_VERSION -match '[A-Za-z]') {
            Add-Issue "Cargo 版本 $EXPECT_VERSION 含字母：LTS 属发布标签，写进版本会变成预发布语义"
        } else {
            Add-Pass "发布通道标签（LTS）与 semver 版本（$EXPECT_VERSION）分离"
        }
    } else {
        Add-Issue '缺少发布通道标签（RELEASE_CHANNEL），版本输出无法体现 LTS'
    }
    # --version / --help 的输出**必须真的可达**：GUI 子系统下 `AttachConsole` 会把标准句柄
    # 指向控制台，从而丢掉「重定向到文件/管道」的 stdout —— 实测 `--version > file` 得到 0 字节，
    # 也就是说安装包与 CI 的版本核对入口完全静默。要求先判定已有 stdout 句柄再决定是否借用控制台。
    if ($mainPubCode -match 'fn has_stdout_handle' -and
        $mainPubCode -match '(?s)fn emit_console_text.*?has_stdout_handle()') {
        Add-Pass '--version/--help 输出可达（已重定向时不借用控制台，避免 stdout 被丢弃）'
    } else {
        Add-Issue '--version/--help 可能静默：stdout 被重定向时 AttachConsole 会丢弃输出'
    }
}

# 启动服务不得在 UI 线程执行（对账 / 端口探测 / 锁清理 / spawn 全是阻塞调用）
$appRsPub = Read-Text 'crates/dsh-app/src/app.rs'
if ($null -ne $appRsPub) {
    $appPubCode = Get-CodeText 'crates/dsh-app/src/app.rs' $appRsPub
    if ($appPubCode -match '\bfn\s+spawn_start\s*\(' -and $appPubCode -match 'self\.spawn_start\(\)') {
        Add-Pass '启动服务走 worker 线程（spawn_start），UI 线程不执行阻塞启动'
    } else {
        Add-Issue '启动服务仍在 UI 线程执行（对账/探测/锁清理会冻住界面）'
    }
    # 端口探测必须集中限流：`is_port_listening` 只允许出现在 port_reachable 里
    $probeHits = ([regex]::Matches($appPubCode, 'is_port_listening\s*\(')).Count
    if ($appPubCode -match '\bfn\s+port_reachable\s*\(' -and $appPubCode -match 'READY_PROBE_INTERVAL' -and $probeHits -eq 1) {
        Add-Pass '端口可达性探测集中限流（READY_PROBE_INTERVAL，单点调用）'
    } else {
        Add-Issue "端口探测未集中限流（is_port_listening 出现 $probeHits 次，应为 1 次且位于 port_reachable）"
    }
}

# 外部命令必须有超时（无超时的 output() 会把调用方永久挂住）
$maintRsPub = Read-Text 'crates/dsh-core/src/maintenance.rs'
if ($null -ne $maintRsPub) {
    $maintPubCode = Get-CodeText 'crates/dsh-core/src/maintenance.rs' $maintRsPub
    # 只看**生产代码**（`#[cfg(test)]` 之前）：测试里的 `mklink` 等一次性命令
    # 不需要超时，把它们算进来会让断言变成噪声。
    $maintProdPart = ($maintRsPub -split '#\[cfg\(test\)\]')[0]
    if ($maintPubCode -match '\bfn\s+run_capture_within\s*\(' -and $maintPubCode -match 'PROBE_TIMEOUT' -and
        -not (Test-CodeMatch $maintProdPart '\.output\(\)')) {
        Add-Pass '外部命令探测限时（run_capture_within + 超时即杀进程树）'
    } else {
        Add-Issue '外部命令探测未限时（Command::output() 无超时，WMI/PowerShell 卡住会挂住调用方）'
    }
}

# 进程存活判定与进程树终止的加固
$procRsPub = Read-Text 'crates/dsh-core/src/process.rs'
if ($null -ne $procRsPub) {
    $procPubCode = Get-CodeText 'crates/dsh-core/src/process.rs' $procRsPub
    if ($procPubCode -match '(?s)fn\s+is_process_alive[^}]*WaitForSingleObject' -and
        $procPubCode -match 'WAIT_TIMEOUT') {
        Add-Pass '进程存活判定基于 WaitForSingleObject（不受退出码 259 哨兵歧义影响）'
    } else {
        Add-Issue '进程存活判定仍依赖退出码哨兵（exit(259) 的进程会被永久判为存活）'
    }
    if ($procPubCode -match 'MAX_TREE_NODES' -and -not (Test-CodeMatch $procRsPub 'kill_process_tree_into')) {
        Add-Pass '进程树终止为迭代 + 去重（不递归，深树/环状父子不会栈溢出）'
    } else {
        Add-Issue '进程树终止仍为递归实现（深树或环状父子关系有栈溢出风险）'
    }
    # 性能：进程枚举必须**一次成表**后复用，而不是每个 PID/每一级各建一份全表快照。
    # 旧实现：身份判定最多 8 次全表枚举；卸载器对每个 PID 各枚举一次（300 进程 ⇒ 300 次）。
    $platformRs = Read-Text 'crates/dsh-uninstall/src/platform.rs'
    if ($procPubCode -match 'pub struct ProcessTable' -and
        $procPubCode -match 'fn classify_dsh_identity_in' -and
        -not ($procPubCode -match '(?m)^fn child_pids_of\(') -and
        $null -ne $platformRs -and $platformRs -match 'ProcessTable::capture') {
        Add-Pass '进程枚举一次成表后复用（身份判定 / 进程树 / 卸载器共享同一份快照）'
    } else {
        Add-Issue '进程枚举仍在重复建快照：身份判定与卸载器会做 O(进程数) 次全表枚举'
    }
    # 日志：常开句柄 + 滚动在超限之前发生（每行 open/close 是纯浪费的系统调用）
    $logCode = Get-CodeText 'crates/dsh-core/src/log.rs' (Read-Text 'crates/dsh-core/src/log.rs')
    if ($logCode -match 'struct Sink' -and $logCode -match 'fn rotate\(' -and
        -not ($logCode -match 'fn maybe_rotate\(|self\.maybe_rotate')) {
        Add-Pass '日志写入复用常开句柄，滚动判定改用句柄 fstat（不再每行 open/close）'
    } else {
        Add-Issue '日志仍每行 open/write/close：UI 线程与 stderr 排空线程上的重复打开开销'
    }
}

# 卸载器的 dry-run 断言（"卸载会删什么"必须能在不删任何东西的前提下验证）已随实现迁移：
# 旧脚本时代的断言盯的是 uninstall.ps1 / DSHLauncherSetup.cs；现在是原生 exe，
# 对应断言集中在 §1c 之后（卸载器 crate 的 print_plan 不得含状态变更调用）。


# 服务簿记必须落在 %LOCALAPPDATA%（可被卸载器清理），不得落在 %APPDATA%（卸载保留）
$recRs = Read-Text 'crates/dsh-core/src/service_record.rs'
if ($null -ne $recRs) {
    $recCode = Get-CodeText 'crates/dsh-core/src/service_record.rs' $recRs
    if ($recCode -match 'local_data_dir\(\)\s*\.\s*join\(\s*"service\.json"\s*\)' -and
        $recCode -match 'clear_legacy_record' -and
        $recCode -match 'app_data_dir\(\)\s*\.\s*join\(\s*"service\.json"\s*\)') {
        Add-Pass 'service.json 位于 %LOCALAPPDATA%（卸载可清理）+ 旧位置一次性清理'
    } else {
        Add-Issue 'service.json 路径不符合文档/卸载器的约定（应位于 %LOCALAPPDATA%，并清理 %APPDATA% 下的旧文件）'
    }
}

# 内嵌界面的「需要认证」自愈（用户实测故障：接管外部 dsh 时界面显示 401 提示页）
$harnessRs = Read-Text 'crates/dsh-ui/src/harness.rs'
if ($null -ne $harnessRs -and $null -ne $appRsPub) {
    $harnessCode = Get-CodeText 'crates/dsh-ui/src/harness.rs' $harnessRs
    if ($harnessCode -match 'AUTH_CHECK_JS' -and $harnessCode -match '\bfn\s+begin_auth_check' -and
        $harnessCode -match 'authentication required') {
        Add-Pass '内嵌页面提供认证自检（AUTH_CHECK_JS + begin_auth_check，判据与 dsh 401 文案一致）'
    } else {
        Add-Issue '缺少内嵌页面认证自检：接管外部 dsh 且无 cookie 时界面会停在 401 提示页而不自知'
    }
    if ($appPubCode -match '\bfn\s+maybe_check_auth' -and $appPubCode -match 'self\.maybe_check_auth\(\)' -and
        $appPubCode -match '\bfn\s+recover_from_auth_required' -and $appPubCode -match '\bfn\s+apply_auth_recovery') {
        Add-Pass '认证失败会自愈（检测 → 探测活跃会话 → 重启服务取得带 token 地址）'
    } else {
        Add-Issue '认证失败没有自愈路径（用户只能看到 401 提示页）'
    }
    # 保守性：只有"明确没有活跃会话"才允许不询问就重启
    if ($appPubCode -match '\bfn\s+auth_recovery_action' -and $appPubCode -match 'any_activity\(\)') {
        Add-Pass '认证自愈的自动重启仅在"确无活跃会话"时触发（否则询问用户）'
    } else {
        Add-Issue '认证自愈可能在不询问的情况下重启服务（有中断用户任务的风险）'
    }
    # 活跃会话探测依赖外部命令，必须在 worker 线程（UI 线程不得阻塞）
    if ($appPubCode -match '"dsh-auth-probe"') {
        Add-Pass '认证自愈前的活跃会话探测在 worker 线程执行（UI 线程不阻塞）'
    } else {
        Add-Issue '认证自愈前的活跃会话探测可能阻塞 UI 线程（Get-CimInstance 可达数秒）'
    }
    $dialogRsPub = Read-Text 'crates/dsh-app/src/dialog.rs'
    if ($null -ne $dialogRsPub -and (Test-CodeMatch $dialogRsPub 'confirm_restart_for_auth')) {
        Add-Pass '提供「是否重启服务以恢复界面认证」的确认对话框'
    } else {
        Add-Issue '缺少认证恢复确认对话框（有活跃会话时无法让用户决定）'
    }
}

# 服务启动路径不得自死锁：`std::sync::Mutex` 不可重入，同一线程重复 lock() 即永久阻塞
$svcRs = Read-Text 'crates/dsh-app/src/service.rs'
if ($null -ne $svcRs) {
    $svcCode = Get-CodeText 'crates/dsh-app/src/service.rs' $svcRs
    $startBody = [regex]::Match($svcCode, '(?s)fn\s+start\(&self\).*?(?=fn\s+stop)').Value
    $locks = ([regex]::Matches($startBody, 'self\.inner\.lock\(\)')).Count
    # 恰好 2 次：函数入口取守卫 + spawn 之后重新取锁回写状态。
    # 第 3 次几乎一定是"在持有守卫时又 lock 一次"的自死锁（实测踩过：启动服务在
    # 「清理遗留锁」之后永久阻塞，且后续所有服务操作一起卡死）。
    if ($locks -eq 2) {
        Add-Pass 'start() 的 ServiceInner 加锁次数正确（入口 + spawn 后回写共 2 次，无重入自死锁）'
    } else {
        Add-Issue "start() 内 self.inner.lock() 出现 $locks 次（应为 2）—— 疑似同一线程重复加锁的自死锁"
    }
    if ($svcCode -match 'Arc::clone\(&inner\.process\)') {
        Add-Pass 'spawn 之前从句柄取出 ProcessManager（不重复加锁）'
    } else {
        Add-Issue 'spawn 之前未从句柄取出 ProcessManager（可能重复加锁）'
    }
}

# `Dsh::version/dist_tags/upgrade/check_health` 已作为死代码彻底删除（v5.0.0 LTS 定稿轮）。
#
# 它们从 v5 重写起就没有任何调用方（`dsh-app` 侧的包装层早已删除），却带着一整条 npm 调用链
# （定位 npm.cmd / 拼 registry 参数 / 超时 + 进程树回收 / JSON 解析）持续漂移。删除后：
#   * 用户能力没有丢 —— 升级与修复原生模块的手动步骤在 `docs/MAINTENANCE.zh/en.md` §1.3–1.5；
#   * 原本由 `is_safe_version` 白名单承担的安全属性改由**结构**保证（见下面的断言）。
$dshRs = Read-Text 'crates/dsh-core/src/dsh.rs'
if ($null -ne $dshRs) {
    $dshCode = Get-CodeText 'crates/dsh-core/src/dsh.rs' $dshRs
    $revived = @()
    foreach ($dead in @('fn version', 'fn dist_tags', 'fn upgrade', 'fn check_health', 'fn is_safe_version', 'fn run_capture', 'fn resolve_npm')) {
        if ($dshCode -match [regex]::Escape($dead) + '\s*\(') { $revived += $dead }
    }
    if ($revived.Count -eq 0) {
        Add-Pass 'dsh.rs 已无死代码 API（version/dist_tags/upgrade/check_health/is_safe_version 均已删除）'
    } else {
        Add-Issue "dsh.rs 重新出现无调用方的死代码 API：$($revived -join ', ')"
    }
    # 安全属性（命令注入面）必须由结构保证：**生产代码**里不含任何 shell 拼串调用。
    # （`#[cfg(test)]` 之前的部分：测试里出现 `...\cmd.exe` 是"必须被拒绝的 node_path"
    #   夹具，属于正当用法，不能算违规。）
    $dshProdPart = ($dshRs -split '#\[cfg\(test\)\]')[0]
    if (-not (Test-CodeMatch $dshProdPart 'cmd\.exe|cmd\s*/c|Command::new\("cmd')) {
        Add-Pass 'dsh.rs 生产代码不含 shell/cmd.exe 拼串调用（进程调用一律参数数组，无命令注入面）'
    } else {
        Add-Issue 'dsh.rs 生产代码出现 cmd.exe / shell 拼串调用（命令注入面）'
    }
}

# 构建缓存自清洁：`target\` 是缓存不是交付物，必须有可重复执行的回收工具与忽略规则
# 构建缓存与工作区自清洁：`target\` 是缓存不是交付物，且仓库根/%TEMP% 也会积累残留，
# 因此必须有一个可重复执行、**可自校验**的回收工具（清完要能证明没破坏 FACTS / 交付物）。
Test-FileExists 'tools/clean.ps1' '工作区自清洁工具（tools/clean.ps1）'
$cleanRs = Read-Text 'tools/clean.ps1'
if ($null -ne $cleanRs) {
    $cleanCode = Get-CodeText 'tools/clean.ps1' $cleanRs
    # 默认必须「只报告、不删」：任何清理动作（-Cache / -All / -Repo / -Temp）都必须显式给出
    if ($cleanCode -match 'if\s*\(\s*-not\s*\(\s*\$Cache\s*-or\s*\$All\s*-or\s*\$Repo\s*-or\s*\$Temp\s*\)\s*\)') {
        Add-Pass 'clean.ps1 默认为只读报告（清理必须显式给出 -Cache / -All / -Repo / -Temp）'
    } else {
        Add-Issue 'clean.ps1 缺少「默认只报告」保护（可能在未显式要求时删除缓存）'
    }
    # 必须保留 release 交付物：FACTS.artifacts.launcher_build 与 verify-version.ps1 依赖它们
    if ($cleanCode -match 'dsh-app\.exe' -and $cleanCode -match 'dsh-uninstall\.exe' -and $cleanCode -match 'FACTS') {
        Add-Pass 'clean.ps1 明确保留 release 产物（启动器 + 卸载器，FACTS / verify-version 依赖）'
    } else {
        Add-Issue 'clean.ps1 未保护全部 release 产物：-Cache 后 gen-facts -Check / verify-version 会失败'
    }
    # -WhatIf 必须零改动：删除动作只能出现在 WhatIf 分支**之后**
    $whatIfIdx = $cleanCode.IndexOf('if ($WhatIf)')
    $removeIdx = $cleanCode.IndexOf('Remove-Item -LiteralPath $item.Path')
    if ($whatIfIdx -ge 0 -and $removeIdx -gt $whatIfIdx) {
        Add-Pass 'clean.ps1 的 -WhatIf 在执行删除之前退出（先看后删，可 dry-run）'
    } else {
        Add-Issue 'clean.ps1 的 -WhatIf 未挡住删除：可能在只读模式下改动磁盘'
    }
    # %TEMP% 清理必须白名单制，且**显式排除** DSH 运行时正在使用的目录
    if ($cleanCode -match 'dsh-spill' -and $cleanCode -match 'dsh-subprocess' -and $cleanCode -match '\$tempWhitelist') {
        Add-Pass 'clean.ps1 的 %TEMP% 清理为白名单制，并排除 DSH 运行时目录（dsh-spill / dsh-subprocess）'
    } else {
        Add-Issue 'clean.ps1 可能误删 DSH 运行时目录（%TEMP% 须白名单制并排除 dsh-spill / dsh-subprocess）'
    }
    # 运行期数据（%LOCALAPPDATA%）只报不删：那是卸载器的职责，删掉会清掉用户登录态与服务簿记
    if ($cleanCode -match '\$runtime' -and $cleanCode -notmatch 'Remove-Item[^\r\n]*\$runtime') {
        Add-Pass 'clean.ps1 对 %LOCALAPPDATA% 运行期数据只报告不删除（属卸载器职责）'
    } else {
        Add-Issue 'clean.ps1 可能删除 %LOCALAPPDATA% 运行期数据（用户登录态 / 服务簿记会丢）'
    }
    # 被占用的副本必须降级为「跳过」而不是失败：热替换副本是运行中实例的映像，Windows 删不掉
    if ($cleanCode -match '\$skipped' -and $cleanCode -match "Get-Process -Name 'DSHLauncher'") {
        Add-Pass 'clean.ps1 把「被运行实例占用的副本」降级为跳过（不污染退出码）'
    } else {
        Add-Issue 'clean.ps1 未区分「删不掉」与「被占用」：热替换副本会让清理误报失败'
    }
}
# 构建目录布局（Cargo build-dir v2 / CARGO_BUILD_BUILD_DIR）：工具链不得依赖**未承诺**的内部布局。
#
# 背景：Cargo 1.100 起，新布局把中间产物按「包名 + 构建单元哈希」分桶（deps/ 与 .fingerprint/ 消失）；
# Cargo 1.91 起（稳定版可用）还能用 CARGO_BUILD_BUILD_DIR / [build] build-dir 把中间产物整块搬出 target。
# CFT 明确承诺**不变**的只有「最终产物在 target/<profile>/ 内的布局」，因此脚本只允许依赖最终产物路径。
Test-FileExists 'tools/verify-build-layout.ps1' '构建目录布局兼容性验证（tools/verify-build-layout.ps1）'
$layoutScripts = @(Get-ChildItem -Path $root -Recurse -File -Filter '*.ps1' -ErrorAction SilentlyContinue |
    Where-Object {
        # 路径分量判定（不用正则，避免转义噪音）：跳过 target/ 构建目录/.git 下的脚本
        $parts = $_.FullName.Split([char]92)
        -not ($parts -contains 'target' -or $parts -contains 'build' -or $parts -contains '.git') -and
        $_.Name -ne 'verify-build-layout.ps1' -and $_.Name -ne 'check-consistency.ps1'
    })
$layoutHits = @()
$layoutMarkers = @('deps' + [string][char]92, '.fingerprint', ([string][char]92 + 'debug' + [string][char]92 + 'examples'))
foreach ($s in $layoutScripts) {
    $raw = Get-Content -LiteralPath $s.FullName -Raw
    $noBlock = [regex]::Replace($raw, '(?s)<#.*?#>', '')
    $code = ($noBlock -split [string][char]10 | Where-Object { $_.TrimStart() -notmatch '^#' }) -join [string][char]10
    foreach ($mk in $layoutMarkers) {
        if ($code -like ('*' + $mk + '*')) { $layoutHits += ($s.Name + ' -> ' + $mk) }
    }
}
if ($layoutHits.Count -eq 0) {
    Add-Pass '脚本未硬编码构建目录的内部布局（deps / .fingerprint / 中间 examples）'
} else {
    Add-Issue ('脚本依赖构建目录内部布局（新布局 / 搬迁后会失效）：' + ($layoutHits -join '; '))
}
$giLayout = Read-Text '.gitignore'
if ($null -ne $giLayout -and $giLayout -match '(?m)^/build/') {
    Add-Pass '.gitignore 覆盖可能被搬迁的构建目录（/build/）'
} else {
    Add-Issue '.gitignore 未覆盖被搬迁的构建目录：CARGO_BUILD_BUILD_DIR 之后会被当成未跟踪文件'
}

# 布局探针**不得污染默认 target**。
#
# `cargo +nightly` 指向**滚动** nightly，而本仓库主工具链是**钉死的日期版** nightly
# （rustc 提交号不同 ⇒ 代码生成不同）。探针一旦写默认 `target\`，就会把交付物换成另一个
# 编译器产出的二进制 —— 而 `FACTS.json` 与 `verify-version.ps1` 依赖"根产物与
# `target\release\` 同源"。实测踩到：卸载器 316,928 B → 317,440 B，
# 且 `gen-facts -Check` **看不见**这种漂移（它只比尺寸/版本，不比字节）。
$layoutProbeText = Read-Text 'tools/verify-build-layout.ps1'
if ($null -ne $layoutProbeText) {
    if ($layoutProbeText -match 'CARGO_TARGET_DIR' -and $layoutProbeText -match 'build-layout-probe-nightly') {
        Add-Pass '布局探针（nightly）使用独立 CARGO_TARGET_DIR，不会覆写默认 target 的交付物'
    } else {
        Add-Issue '布局探针直接写默认 target\：会把交付物换成另一个工具链的产物（且 FACTS 看不见该漂移）'
    }
}
$cleanLayout = Read-Text 'tools/clean.ps1'
if ($null -ne $cleanLayout -and $cleanLayout -match 'CARGO_BUILD_BUILD_DIR' -and $cleanLayout -match 'Resolve-BuildDir') {
    Add-Pass 'clean.ps1 会解析生效的构建目录（搬迁后不会漏掉最大的一块缓存）'
} else {
    Add-Issue 'clean.ps1 未识别被搬迁的构建目录：自清洁会漏掉最大的缓存'
}
if ($null -ne $cleanLayout -and $cleanLayout -match 'CACHEDIR.TAG' -and $cleanLayout -match 'legacyBuildDirs') {
    # 还要**保留**这个标记。它不在保留清单里时 `-Cache` 会删掉它，而 cargo 只在**创建**构建目录时
    # 写它 —— 实测（stable / v2 / CARGO_BUILD_BUILD_DIR 三种配置 cargo 都会写）：被删掉之后
    # 后续多次 `cargo build` 都不补写，于是上千 MB 缓存会被备份软件照单全收。
    $keepBlock = [regex]::Match($cleanLayout, '(?s)\$keepRel\s*=\s*@\((.*?)\)').Groups[1].Value
    if ($keepBlock -match 'CACHEDIR\.TAG') {
        Add-Pass 'clean.ps1 既用 CACHEDIR.TAG 识别遗留构建目录，也在 -Cache 时保留该标记（cargo 不会补写）'
    } else {
        Add-Issue 'clean.ps1 未保留 CACHEDIR.TAG：-Cache 会删掉缓存标记，而 cargo 不会补写（备份软件不再跳过该目录）'
    }
} else {
    Add-Issue 'clean.ps1 只认当前生效的构建目录：历史搬迁留下的目录会成为清理死角'
}
$gitignore = Read-Text '.gitignore'
if ($null -ne $gitignore) {
    if ($gitignore -match '(?m)^/target/\s*$') {
        Add-Pass '.gitignore 排除 /target/（构建缓存不入库）'
    } else {
        Add-Issue '.gitignore 未排除 /target/：构建缓存可能被提交'
    }
    if ($gitignore -match '(?m)^\*\.pdb\s*$') {
        Add-Pass '.gitignore 排除 *.pdb'
    } else {
        Add-Issue '.gitignore 未排除 *.pdb（构建中间物可能被提交）'
    }
}

# 直接依赖必须被本 crate 源码真实引用（离线版 cargo-machete 等价检查）
# 说明：网络不可用时 `cargo machete` / `cargo udeps` 无法安装与运行，这里用
# "清单声明的每个直接依赖，其 crate 名（`-` → `_`）必须出现在该 crate 的源码中"
# 作为等价判据。它覆盖了「声明了却完全没用」这一类未使用依赖。
# crate 列表从 workspace members 解析 —— 写死会让新增 crate 静默逃过检查。
$wsCrates = @()
$wsMembers = [regex]::Match($cargo, '(?ms)^\[workspace\](.*?)(^\[|\z)').Groups[1].Value
foreach ($m in [regex]::Matches($wsMembers, '"crates/([^"]+)"')) { $wsCrates += $m.Groups[1].Value }
if ($wsCrates.Count -eq 0) { Add-Issue '无法从 Cargo.toml 解析 workspace members（未使用依赖检查失去对象）' }
foreach ($crate in $wsCrates) {
    $manifestPath = "crates/$crate/Cargo.toml"
    $manifest = Read-Text $manifestPath
    if ($null -eq $manifest) { continue }
    # 只取 [dependencies] 段（不含 build-dependencies / dev-dependencies）
    $depBlock = [regex]::Match($manifest, '(?ms)^\[dependencies\](.*?)(^\[|\z)').Groups[1].Value
    $depNames = @()
    foreach ($line in ($depBlock -split "`n")) {
        $m = [regex]::Match($line, '^\s*([A-Za-z0-9_\-]+)\s*=')
        if ($m.Success) { $depNames += $m.Groups[1].Value }
    }
    $srcFiles = Get-ChildItem -Path (Join-Path $root "crates/$crate/src") -Recurse -File -Filter *.rs -ErrorAction SilentlyContinue
    $srcText = ''
    foreach ($f in $srcFiles) { $srcText += (Get-Content $f.FullName -Raw) + "`n" }
    $codeText = Strip-Comments $srcText
    $unused = @()
    foreach ($d in $depNames) {
        $ident = $d -replace '-', '_'
        if ($codeText -notmatch "\b$([regex]::Escape($ident))\b") { $unused += $d }
    }
    if ($unused.Count -eq 0) {
        Add-Pass "$crate 无未使用的直接依赖（离线 machete 等价检查）"
    } else {
        Add-Issue "$crate 声明了但源码未引用的依赖：$($unused -join ', ')"
    }
}

# [workspace.dependencies] 的每一项都必须真的被某个成员引用。
#
# 背景（工具链升级实测）：Cargo 1.100 起会对"在 `[workspace.dependencies]` 里声明、
# 但没有任何成员引用"的条目发出 `unused_workspace_dependencies` 警告 —— 把主工具链切到
# nightly（build-dir Layout v2）后立刻报出 `dsh-core` / `dsh-ui` 两项：三个成员当时用的是
# 裸 `{ path = "../..." }`，于是 workspace 里那两条声明成了死条目。
# 该警告既污染构建输出，也是"依赖清理想删而没删干净"的信号，因此在这里钉死。
$wsDepBlock = [regex]::Match($cargo, '(?ms)^\[workspace\.dependencies\](.*?)(^\[|\z)').Groups[1].Value
$wsDepNames = @()
foreach ($line in ($wsDepBlock -split "`n")) {
    $m = [regex]::Match(($line -replace '#.*$', ''), '^\s*([A-Za-z0-9_\-]+)\s*=')
    if ($m.Success) { $wsDepNames += $m.Groups[1].Value }
}
if ($wsDepNames.Count -eq 0) {
    Add-Issue '无法解析 [workspace.dependencies]（workspace 依赖单一来源检查失效）'
} else {
    $memberText = ''
    foreach ($mf in (Get-ChildItem -Path (Join-Path $root 'crates') -Recurse -File -Filter 'Cargo.toml' -ErrorAction SilentlyContinue)) {
        $memberText += (Get-Content -LiteralPath $mf.FullName -Raw) + "`n"
    }
    $unusedWs = @()
    foreach ($d in $wsDepNames) {
        $esc = [regex]::Escape($d)
        # 形态一：`serde = { workspace = true }`；形态二：`serde.workspace = true`
        $inline = '(?m)^\s*' + $esc + '\s*=\s*\{[^}]*workspace\s*=\s*true'
        $dotted = '(?m)^\s*' + $esc + '\s*\.\s*workspace\s*=\s*true'
        if ($memberText -notmatch $inline -and $memberText -notmatch $dotted) { $unusedWs += $d }
    }
    if ($unusedWs.Count -eq 0) {
        Add-Pass "[workspace.dependencies] 每项都被成员引用（$($wsDepNames.Count) 项，无 unused_workspace_dependencies 警告）"
    } else {
        Add-Issue ('[workspace.dependencies] 中未被任何成员引用（Cargo 1.100+ 会警告）：' + ($unusedWs -join '、'))
    }
}

# ---------------------------------------------------------------------------
# 7b. 文档里引用的关键数字必须与 FACTS.json 一致
#
# 背景：v5.0.0 审计发现**同一指标在 5 份文档里有 3–4 个互相矛盾的值**
# （单元测试 28/48/58、exe 体积 490/934.5/952.5/973,312）。
# `gen-facts.ps1 -Check` 只能发现"源码变了"，发现不了"文档没跟着改"——因为文档
# 不在它的扫描范围里。这里补上这一环：把文档中**当前版本**引用的数字与 FACTS 对齐。
#
# 只检查"当前状态"类断言（README 的能力表、CHANGELOG 的 v5.0.0 段、路线图 §15）。
# 历史段落（v5.0.0 及更早、审计报告快照）里的旧值**故意保留**，不做断言。
#
# ## 为什么**不**断言"一致性项数"（重要，踩过两次）
#
# 该数字是**本脚本自身的产出**：往本文件里加一条断言，这个数就变，于是文档里的数
# 立刻过期 —— 形成「加断言 → 文档过期 → gen-facts 解析失败（拿不到数字而写 0）
# → 断言更加失败」的自指循环，且**没有任何增益**（断言条数多一条少一条并不代表缺陷）。
# 因此：单测数量这类**外部可测**的量做断言，自指的量不做 —— 文档里若要引用它，
# 只需注明"以 FACTS.json 为准"，不参与断言。
# ---------------------------------------------------------------------------
if (Test-Path (Join-Path $root 'docs/FACTS.json')) {
    try {
        $facts = Get-Content (Join-Path $root 'docs/FACTS.json') -Raw | ConvertFrom-Json
        $fTests = [int]$facts.unit_tests.total

        # --- README：能力表里的单元测试数 ---
        if ($null -ne $readme) {
            if ($readme -match "\*\*(\d+) 个单元测试\*\*") {
                $n = [int]$Matches[1]
                if ($n -eq $fTests) { Add-Pass "README 单元测试数 = FACTS（$fTests）" }
                else { Add-Issue "README 单元测试数 = $n，FACTS = $fTests（文档未同步）" }
            }
            if ($readme -match '\*\*(\d+) unit tests\*\*') {
                $n = [int]$Matches[1]
                if ($n -eq $fTests) { Add-Pass "README(EN) 单元测试数 = FACTS（$fTests）" }
                else { Add-Issue "README(EN) 单元测试数 = $n，FACTS = $fTests（文档未同步）" }
            }
            # exe 体积只能给量级断言（会随构建变化）：必须与 FACTS 的 KiB 取整一致
            $art = $facts.artifacts.launcher_root
            if ($null -ne $art -and $art.kib) {
                $mb = [math]::Round([double]$art.kib / 1024, 1)
                $pattern = [regex]::Escape("exe 约 $mb MB")
                if ($readme -match $pattern) { Add-Pass "README exe 体积量级 = FACTS（约 $mb MB）" }
                else { Add-Issue "README 未按 FACTS 声明 exe 体积量级（应为「约 $mb MB」）" }
            }
        }

        # --- CHANGELOG：v5.0.0 段的单元测试数（中英必须同源且与 FACTS 一致）---
        if ($null -ne $changelog) {
            foreach ($p in @(
                    @{ rx = '\| 单元测试 \| 65 \| \*\*(\d+)\*\* \|'; who = 'CHANGELOG' },
                    @{ rx = '\| Unit tests \| 65 \| \*\*(\d+)\*\* \|'; who = 'CHANGELOG(EN)' }
                )) {
                if ($changelog -match $p.rx) {
                    $n = [int]$Matches[1]
                    if ($n -eq $fTests) { Add-Pass "$($p.who) 单元测试数 = FACTS（$fTests）" }
                    else { Add-Issue "$($p.who) 单元测试数 = $n，FACTS = $fTests" }
                } else {
                    Add-Issue "$($p.who) 缺少 v5.0.0 单元测试实测行"
                }
            }
        }

        # --- 路线图 §15.5 实测表 ---
        if ($null -ne $roadmap) {
            # 该表在路线图里是**无竖线**的三列表（`单元测试   65   **97**（…）`），
            # 不是 Markdown 管道表，所以两种写法都要认：写死 `\| 单元测试 \| 65 \|`
            # 会因为格式差异而"匹配不到行"，把「文档没写」和「格式不同」混为一谈。
            $rm = [regex]::Match($roadmap, '单元测试\s+\|\s*65\s+\|\s+\*\*(\d+)\*\*|单元测试\s+65\s+\*\*(\d+)\*\*')
            if ($rm.Success) {
                $n = [int]$(if ($rm.Groups[1].Success) { $rm.Groups[1].Value } else { $rm.Groups[2].Value })
                if ($n -eq $fTests) { Add-Pass "路线图 §15.5 单元测试数 = FACTS（$fTests）" }
                else { Add-Issue "路线图 §15.5 单元测试数 = $n，FACTS = $fTests" }
            } else {
                # P2-8：缺 `else` 时，把这一行删掉就既不 Pass 也不 Issue —— 检查静默消失。
                Add-Issue '路线图 §15.5 缺少「单元测试 65 **N**」实测行（无法校验该数字）'
            }
        }
    } catch {
        Add-Issue "解析 FACTS.json 失败：$($_.Exception.Message)"
    }
}

# ---------------------------------------------------------------------------
# 7c. PowerShell 脚本编码：含非 ASCII 的 .ps1 必须带 UTF-8 BOM
#
# 实测（zh-CN / ANSI 代码页 gb2312）：没有 BOM 时，Windows PowerShell 5.1 会把 UTF-8 的
# 中文注释按 GB2312 解码，进而吞掉引号/反斜杠，报出一堆 "Missing closing ')'" /
# "Unexpected token" —— 也就是说 README 里承诺的构建入口（`powershell -File build.ps1`）
# 在中/日/韩 Windows 上根本跑不起来。BOM 让 5.1 也按 UTF-8 解码，PS 7 不受影响。
# ---------------------------------------------------------------------------
$ps1MissingBom = @()
Get-ChildItem -Path $root -Recurse -File -Filter *.ps1 -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch '\\(target|\.git)(\\|$)' } |
    ForEach-Object {
        $bytes = [System.IO.File]::ReadAllBytes($_.FullName)
        $hasBom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
        $nonAscii = $false
        foreach ($b in $bytes) { if ($b -gt 0x7F) { $nonAscii = $true; break } }
        if ($nonAscii -and -not $hasBom) { $ps1MissingBom += $_.FullName.Replace("$root\", '') }
    }
if ($ps1MissingBom.Count -eq 0) {
    Add-Pass '含非 ASCII 的 .ps1 均带 UTF-8 BOM（Windows PowerShell 5.1 也能正确解码）'
} else {
    Add-Issue ('以下 .ps1 含非 ASCII 但缺少 UTF-8 BOM（PS 5.1 下会解析失败）：' + ($ps1MissingBom -join ', '))
}

# 同上：即使脚本自身有 BOM，PS 5.1 的 `Get-Content` 仍按**代码页**解码文件内容，
# 于是读 UTF-8 源码/文档得到乱码、中文断言全部假失败（实测 PS 5.1 下 35 项假失败）。
# 修法是每个脚本固定一次全局默认参数；这里断言它没有被误删。
$ps1NoEncodingDefault = @()
Get-ChildItem -Path $root -Recurse -File -Filter *.ps1 -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch '\\(target|\.git)(\\|$)' } |
    ForEach-Object {
        $bytes = [System.IO.File]::ReadAllBytes($_.FullName)
        $nonAscii = $false
        foreach ($b in $bytes) { if ($b -gt 0x7F) { $nonAscii = $true; break } }
        if (-not $nonAscii) { return }
        $txt = [System.IO.File]::ReadAllText($_.FullName, [System.Text.UTF8Encoding]::new($false))
        if ($txt -notmatch 'Get-Content:Encoding') { $ps1NoEncodingDefault += $_.FullName.Replace("$root\", '') }
    }
if ($ps1NoEncodingDefault.Count -eq 0) {
    Add-Pass '所有含非 ASCII 的 .ps1 均把 Get-Content 默认编码固定为 UTF-8（PS 5.1/7 行为一致）'
} else {
    Add-Issue ('以下 .ps1 未固定 Get-Content 编码（PS 5.1 下会把 UTF-8 读成乱码）：' + ($ps1NoEncodingDefault -join ', '))
}

# ---------------------------------------------------------------------------
# 汇总
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '==== DSHLauncher v5 (Rust) 一致性校验 ====' -ForegroundColor Cyan
foreach ($p in $passed) { Write-Host "  [PASS] $p" -ForegroundColor Green }
if ($issues.Count -gt 0) {
    foreach ($i in $issues) { Write-Host "  [FAIL] $i" -ForegroundColor Red }
    # 机器可读的稳定计数（P2-10）：无论成败都输出，供 gen-facts.ps1 解析，
    # 这样"检查项总数"不会因为某次失败而变成 0/缺失。
    Write-Host ("CHECK_SUMMARY passed={0} failed={1} total={2}" -f $passed.Count, $issues.Count, ($passed.Count + $issues.Count))
    Write-Host "结果: $($issues.Count) 项不通过 / $($passed.Count) 项通过" -ForegroundColor Red
    exit 1
} else {
    Write-Host ("CHECK_SUMMARY passed={0} failed=0 total={1}" -f $passed.Count, $passed.Count)
    Write-Host "结果: 全部 $($passed.Count) 项通过 OK" -ForegroundColor Green
    exit 0
}
