# verify-version.ps1 — 全项目版本/产品元数据一致性硬校验
#
# 为什么需要它：v5.0.0 之前版本号写在两处（Cargo.toml 的 5.0.0 与安装包的 4.2.4），
# 且 Rust 启动器的 exe **完全没有版本资源**（FileVersion/ProductVersion 为空），
# 于是「安装包显示 4.2.4、启动器属性页空白、注册表 DisplayVersion=4.2.4」三者互相矛盾。
# 本脚本把这条链路钉死：Cargo.toml → 编译出的 exe 版本资源 → 安装包常量 → 卸载器。
#
# 用法: pwsh -NoProfile -File tools\verify-version.ps1 [-Exe <路径>] [-SkipBinary] [-RequireInstaller]
#       退出码 0 = 一致；1 = 存在不一致
#
# 汇总口径：`N passed / M skipped / K failed`。
#   被跳过的检查**绝不**计入 passed —— 否则「安装包产物不存在」这种什么都没看
#   的情况会显示成绿色通过（P2-7）。默认仍允许跳过（安装包可能尚未构建），
#   但需要硬门禁时加 `-RequireInstaller` 把「安装包产物缺失」变成失败。
#
# 默认校验 target\release\dsh-app.exe（cargo 产物）。
# build.ps1 在**拷贝到仓库根之前**调用它：这样即使根目录的 DSHLauncher.exe 正被
# 运行中的旧实例占用（Windows 会拒绝覆盖），也不会出现“校验的是上一版产物”的假通过。

param(
    [string]$Exe,
    [switch]$SkipBinary,
    [switch]$RequireInstaller
)

# Windows PowerShell 5.1 的 Get-Content 默认按 **ANSI 代码页**（本机 gb2312）解码，
# 会把本仓库的 UTF-8 源码/文档读成乱码 -> 全部中文断言假失败。这条全局默认参数在 PS 5.1/7 都有效。
$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
Set-Location $root

$pass = New-Object System.Collections.Generic.List[string]
$fail = New-Object System.Collections.Generic.List[string]
$skipped = New-Object System.Collections.Generic.List[string]

function Add-Pass($m) { $script:pass.Add($m) }
function Add-Fail($m) { $script:fail.Add($m) }
function Add-Skip($m) { $script:skipped.Add($m) }

# ---------------------------------------------------------------------------
# 1. 版本唯一来源：Cargo.toml 的 [workspace.package] version
# ---------------------------------------------------------------------------
$cargoToml = Get-Content (Join-Path $root 'Cargo.toml') -Raw
$m = [regex]::Match($cargoToml, '(?ms)^\[workspace\.package\].*?^\s*version\s*=\s*"([^"]+)"')
if (-not $m.Success) {
    Add-Fail 'Cargo.toml 的 [workspace.package] 未声明 version'
    $version = $null
} else {
    $version = $m.Groups[1].Value
    Add-Pass "Cargo.toml 版本唯一来源 = $version"
}

if ($null -eq $version) {
    Write-Host '无法确定版本号，终止校验' -ForegroundColor Red
    exit 1
}

# 三个 crate 必须 version.workspace = true（不得各自写死版本）
foreach ($c in @('crates/dsh-core/Cargo.toml', 'crates/dsh-ui/Cargo.toml', 'crates/dsh-app/Cargo.toml')) {
    $t = Get-Content (Join-Path $root $c) -Raw
    if ($t -match '(?m)^\s*version\.workspace\s*=\s*true') {
        Add-Pass "$c 继承 workspace 版本"
    } else {
        Add-Fail "$c 未使用 version.workspace = true（会出现版本漂移）"
    }
}

# Cargo.lock 中本项目的三个包版本必须与 Cargo.toml 一致
$lock = Join-Path $root 'Cargo.lock'
if (Test-Path $lock) {
    $lockText = Get-Content $lock -Raw
    foreach ($pkg in @('dsh-core', 'dsh-ui', 'dsh-app')) {
        $pm = [regex]::Match($lockText, "(?ms)^name\s*=\s*`"$([regex]::Escape($pkg))`"\r?\nversion\s*=\s*`"([^`"]+)`"")
        if (-not $pm.Success) {
            Add-Fail "Cargo.lock 未包含 $pkg"
        } elseif ($pm.Groups[1].Value -ne $version) {
            Add-Fail "Cargo.lock 中 $pkg 版本为 $($pm.Groups[1].Value)，应为 $version"
        } else {
            Add-Pass "Cargo.lock 中 $pkg = $version"
        }
    }
} else {
    Add-Fail 'Cargo.lock 缺失（无法离线复现构建）'
}

# ---------------------------------------------------------------------------
# 2. 安装包（C#）：版本必须**同源**（v5.0.0 审计后改为从 AssemblyFileVersion 反射读取）
# ---------------------------------------------------------------------------
$setupPath = Join-Path $root 'DSHLauncherSetup.cs'
if (-not (Test-Path $setupPath)) {
    Add-Fail 'DSHLauncherSetup.cs 缺失'
} else {
    $setup = Get-Content $setupPath -Raw
    # 审计前这里写死 `AppVersion = "5.0.0"`，与 Cargo.toml 构成**两处手写版本**，
    # 漏改一处就会出现「exe 是 5.0.0、注册表 DisplayVersion 是旧版」的漂移。
    # 现在只校验实现方式（反射读取）+ 产物实际版本资源，不再要求第二个常量。
    if ($setup -match 'ResolveAppVersion') {
        Add-Pass '安装包版本由 AssemblyFileVersion 反射读取（不再有第二处手写版本）'
    } else {
        Add-Fail 'DSHLauncherSetup.cs 未通过 ResolveAppVersion 反射读取版本'
    }
    if ([regex]::IsMatch($setup, 'AppVersion\s*=\s*"[0-9]')) {
        Add-Fail '安装包中仍有手写的 AppVersion 字面量（会与 Cargo.toml 漂移）'
    } else {
        Add-Pass '安装包中无手写版本字面量'
    }

    # 安装包**产物**的版本资源必须与 Cargo.toml 一致（gen-setup-version.ps1 是否生效）
    $setupExe = Join-Path $root 'DSHLauncherSetup.exe'
    if (Test-Path $setupExe) {
        $svi = (Get-Item $setupExe).VersionInfo
        if ([string]::IsNullOrWhiteSpace($svi.FileVersion) -or $svi.FileVersion -eq '0.0.0.0') {
            Add-Fail "安装包自身缺少版本资源（FileVersion=$($svi.FileVersion)）—— tools/gen-setup-version.ps1 未生效"
        } elseif ($svi.FileVersion -notlike "$version*") {
            Add-Fail "安装包 FileVersion = $($svi.FileVersion)，应为 $version.*"
        } else {
            Add-Pass "安装包自身 FileVersion = $($svi.FileVersion)，ProductName = $($svi.ProductName)"
        }
    } elseif ($RequireInstaller) {
        # -RequireInstaller：本项是硬门禁，缺失即失败（不再静默记为通过）
        Add-Fail '未找到 DSHLauncherSetup.exe（-RequireInstaller 要求必须校验安装包产物）'
    } else {
        Add-Skip '（未构建 DSHLauncherSetup.exe，跳过安装包自身版本资源校验）'
    }

    # 注册表项必须写 DisplayVersion / QuietUninstallString
    if ($setup -match 'SetValue\("DisplayVersion",\s*Program\.AppVersion\)') {
        Add-Pass '注册表 DisplayVersion 取自 AppVersion（单一来源）'
    } else {
        Add-Fail '注册表 DisplayVersion 未绑定 AppVersion'
    }
    if ($setup -match 'QuietUninstallString') {
        Add-Pass '注册表写入 QuietUninstallString（支持静默卸载）'
    } else {
        Add-Fail '注册表缺少 QuietUninstallString'
    }
    # 卸载默认必须保留用户配置
    if ($setup -match '\$env:APPDATA\\DSHLauncher') {
        Add-Fail '卸载脚本无条件删除 %APPDATA%\DSHLauncher（会丢失用户配置）'
    } elseif ($setup -match 'PURGE') {
        Add-Pass '卸载脚本区分默认保留 / --purge 清理用户配置'
    } else {
        Add-Fail '卸载脚本未实现 PURGE 分支（用户配置处理策略不明确）'
    }
    # 不得残留旧 C# 启动器版本号
    if ($setup -match '"4\.\d+\.\d+"') {
        Add-Fail '安装包中仍残留 4.x 版本号字面量'
    } else {
        Add-Pass '安装包中无 4.x 版本号残留'
    }
}

# ---------------------------------------------------------------------------
# 2b. 卸载器：**原生 exe 单一实现**（v5.0.0 LTS 起取代 uninstall.cmd + uninstall.ps1）
#
# 为什么不再是脚本：命令行 `-ExecutionPolicy Bypass` 覆盖不了组策略
# （AllSigned/Restricted），AppLocker/WDAC 也能封锁脚本执行 ⇒ 加固环境下卸载不掉；
# 且脚本版延迟删目录还要再依赖一次 PowerShell。原生 exe 没有这层依赖。
# ---------------------------------------------------------------------------
foreach ($rel in @('uninstall.cmd', 'uninstall.ps1')) {
    if (Test-Path (Join-Path $root $rel)) {
        Add-Fail "$rel 仍然存在：卸载器必须只有一份实现（原生 dsh-uninstall.exe）"
    } else {
        Add-Pass "脚本卸载器已删除：$rel"
    }
}
$unExe = Join-Path $root 'target\release\dsh-uninstall.exe'
if (Test-Path $unExe) {
    $uvi = (Get-Item $unExe).VersionInfo
    if (@($version, "$version.0") -notcontains $uvi.FileVersion) {
        Add-Fail "卸载器 FileVersion = $($uvi.FileVersion)，应为 $version（与 Cargo.toml 同源）"
    } else {
        Add-Pass "卸载器 FileVersion = $($uvi.FileVersion)（对应 $version）"
    }
    if ($uvi.ProductVersion -ne $version) {
        Add-Fail "卸载器 ProductVersion = $($uvi.ProductVersion)，应为 $version"
    } else {
        Add-Pass "卸载器 ProductVersion = $version"
    }
    if ($uvi.OriginalFilename -ne 'dsh-uninstall.exe') {
        Add-Fail "卸载器 OriginalFilename = $($uvi.OriginalFilename)，应为 dsh-uninstall.exe"
    } else {
        Add-Pass '卸载器 OriginalFilename = dsh-uninstall.exe'
    }
    if ([string]::IsNullOrWhiteSpace($uvi.ProductName)) {
        Add-Fail '卸载器缺少 ProductName（属性页会空白）'
    } else {
        Add-Pass "卸载器 ProductName = $($uvi.ProductName)"
    }
    # 图标资源必须真的在 exe 里（与启动器同样的实测口径，不用假断言）
    try {
        Add-Type -AssemblyName System.Drawing -ErrorAction Stop
        $ico = [System.Drawing.Icon]::ExtractAssociatedIcon($unExe)
        if ($ico -and $ico.Width -gt 1 -and $ico.Height -gt 1) {
            Add-Pass "卸载器含图标资源（$($ico.Width)x$($ico.Height)）"
        } else {
            Add-Fail '卸载器缺少图标资源'
        }
    } catch {
        Add-Fail "无法校验卸载器图标资源：$($_.Exception.Message)"
    }
    # 安装包必须内嵌它（否则"应用和功能"里的卸载入口指向一个不存在的文件）
    if ($setup -and $setup -notmatch 'dsh-uninstall\.exe') {
        Add-Fail '安装包（DSHLauncherSetup.cs）未内嵌 dsh-uninstall.exe'
    } elseif ($setup) {
        Add-Pass '安装包内嵌 dsh-uninstall.exe'
    }
    if ($setup -and $setup -notmatch '"UninstallString"\s*,\s*"\\""\s*\+\s*uninstallExe') {
        Add-Fail '注册表 UninstallString 未指向 dsh-uninstall.exe'
    } elseif ($setup) {
        Add-Pass '注册表 UninstallString 指向 dsh-uninstall.exe'
    }
} elseif ($RequireInstaller) {
    Add-Fail '未找到 target\release\dsh-uninstall.exe（-RequireInstaller 要求校验卸载器产物）'
} else {
    Add-Skip '（未构建 dsh-uninstall.exe，跳过卸载器产物校验）'
}

# ---------------------------------------------------------------------------
# 3. 编译产物：exe 的版本资源必须与 Cargo.toml 一致
# ---------------------------------------------------------------------------
if ($SkipBinary) {
    # 跳过 ≠ 通过：绝不能进 $pass，否则汇总会绿（P2-7）
    Add-Skip '（-SkipBinary）跳过 exe 版本资源校验'
} else {
    if ([string]::IsNullOrWhiteSpace($Exe)) {
        $exe = Join-Path $root 'target\release\dsh-app.exe'
    } elseif ([System.IO.Path]::IsPathRooted($Exe)) {
        $exe = $Exe
    } else {
        $exe = Join-Path $root $Exe
    }
    # 允许把本脚本当作「构建产物校验入口」直接调用：产物不存在时先构建。
    # 注意必须构建 **release**——Windows 资源通过 `cargo:rustc-link-arg` 注入，
    # 该指令只对匹配的 profile 生效，debug profile 的产物没有版本资源。
    # $buildCode 预置为 0：只有真正跑了构建才会被改写（避免未构建时误判为失败）。
    $buildCode = 0
    if (-not (Test-Path $exe)) {
        Write-Host "未找到 $exe，先执行 cargo build --release …" -ForegroundColor DarkGray
        & cargo build --release --offline 2>&1 | Select-Object -Last 3 | ForEach-Object { Write-Host $_ }
        # 立刻取退出码（P1-6）：不能靠 Test-Path 猜——编译失败时 target\release
        # 下可能留着上一次的陈旧产物，于是校验的是旧二进制并报告通过。
        $buildCode = $LASTEXITCODE
        if ($buildCode -ne 0) {
            Add-Fail "cargo build --release --offline 退出码 $buildCode（无法产出可信二进制）"
        }
    }
    if (-not (Test-Path $exe)) {
        Add-Fail "未找到产物 $exe（请先 cargo build --release）"
    } elseif ($buildCode -ne 0) {
        # 构建失败：即使磁盘上有陈旧产物也不再据此判定通过
        Add-Fail "因 cargo build 失败，跳过对陈旧产物 $exe 的版本资源判定"
    } else {
        $vi = (Get-Item $exe).VersionInfo
        if ([string]::IsNullOrWhiteSpace($vi.FileVersion)) {
            Add-Fail 'exe 缺少版本资源（FileVersion 为空）—— rc.exe 未参与链接'
        } else {
            $parts = $vi.FileVersion.Split('.')
            $expectNumeric = "$version.0"   # 5.0.0 → 5.0.0.0
            if ([string]::IsNullOrWhiteSpace($parts)) { $gotNumeric = '' }
            elseif ($parts.Length -eq 4) { $gotNumeric = ($parts -join '.') }
            elseif ($parts.Length -eq 3) { $gotNumeric = "$($vi.FileVersion).0" }
            else { $gotNumeric = $vi.FileVersion }
            if ($gotNumeric -ne $expectNumeric) {
                Add-Fail "exe FileVersion = $($vi.FileVersion)，应为 $expectNumeric"
            } else {
                Add-Pass "exe FileVersion = $($vi.FileVersion)（对应 $version）"
            }
            if ($vi.ProductVersion -ne $version) {
                Add-Fail "exe ProductVersion = $($vi.ProductVersion)，应为 $version"
            } else {
                Add-Pass "exe ProductVersion = $version"
            }
            foreach ($field in @('CompanyName', 'FileDescription', 'ProductName', 'LegalCopyright', 'OriginalFilename')) {
                if ([string]::IsNullOrWhiteSpace($vi.$field)) {
                    Add-Fail "exe 版本资源缺少 $field"
                } else {
                    Add-Pass "exe $field = $($vi.$field)"
                }
            }
            if ($vi.OriginalFilename -ne 'DSHLauncher.exe') {
                Add-Fail "exe OriginalFilename = $($vi.OriginalFilename)，安装包释放的是 DSHLauncher.exe"
            }
            # 版本资源必须与安装包声明的产品名一致
            if ($setup) {
                $pn = [regex]::Match($setup, 'AppName\s*=\s*"([^"]+)"')
                if ($pn.Success -and $vi.ProductName -ne $pn.Groups[1].Value) {
                    Add-Fail "exe ProductName（$($vi.ProductName)）≠ 安装包 AppName（$($pn.Groups[1].Value)）"
                } elseif ($pn.Success) {
                    Add-Pass "exe ProductName 与安装包 AppName 一致：$($pn.Groups[1].Value)"
                }
            }
            # 图标资源必须**真的**存在于 exe 里：Explorer / 任务栏 / 托盘都用它。
            #
            # 旧断言只检查 `FileDescription` 非空，却把结论写成"含图标资源"——
            # 这是一个**假断言**：v5.0.0 定稿轮实测踩到，当时 exe 的 .rc 里没有 ICON 行
            # （build.rs 只在图标存在时才声明 rerun-if-changed，导致图标补回来后不再重跑），
            # 产出的 exe 体积少了约 21 KB、图标资源为空，而这条断言照样 PASS。
            # 现在用 ExtractAssociatedIcon 真正取一次图标。
            try {
                Add-Type -AssemblyName System.Drawing -ErrorAction Stop
                $ico = [System.Drawing.Icon]::ExtractAssociatedIcon($exe)
                if ($ico -and $ico.Width -gt 1 -and $ico.Height -gt 1) {
                    Add-Pass "exe 含图标资源（$($ico.Width)x$($ico.Height)）"
                } else {
                    Add-Fail 'exe 缺少图标资源（ExtractAssociatedIcon 返回空/占位图）'
                }
            } catch {
                Add-Fail "无法校验 exe 图标资源：$($_.Exception.Message)"
            }
            if ($vi.FileDescription) { Add-Pass 'exe 版本资源块存在' }
            else { Add-Fail 'exe 版本资源块缺少 FileDescription' }
        }
    }
}

# ---------------------------------------------------------------------------
# 汇总
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host "==== 版本一致性校验（期望 $version）====" -ForegroundColor Cyan
foreach ($p in $pass) { Write-Host "  [PASS] $p" -ForegroundColor Green }
foreach ($s in $skipped) { Write-Host "  [SKIP] $s" -ForegroundColor Yellow }
if ($fail.Count -gt 0) {
    foreach ($f in $fail) { Write-Host "  [FAIL] $f" -ForegroundColor Red }
    # 明确的 通过/跳过/失败 三段口径：跳过项绝不计入通过
    Write-Host "结果: $($pass.Count) passed / $($skipped.Count) skipped / $($fail.Count) failed" -ForegroundColor Red
    exit 1
}
Write-Host "结果: $($pass.Count) passed / $($skipped.Count) skipped / 0 failed —— 无冲突 OK" -ForegroundColor Green
exit 0
