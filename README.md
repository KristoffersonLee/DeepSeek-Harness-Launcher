# DeepSeek Harness Launcher

一个为 **DeepSeek Harness** 打造的 Windows 桌面启动器：双击图标即自动启动 dsh web 服务，并在内嵌的 **WebView2** 窗口中直接显示 Harness —— 无需浏览器，像原生软件一样使用。

A Windows desktop launcher for **DeepSeek Harness**: double-click to automatically start the dsh web service and view Harness directly in an embedded **WebView2** window — no browser needed, used like a native app.

**作者 / Author: [KristoffersonLee](https://github.com/)** · **v5.0.0 LTS（Rust 全量重构版 · 长期支持）**

> 📌 **v5.0.0 定稿轮包含两个 P0 修复**（界面不出现的死锁、token 导航失效导致 401）
> 与一项语义变更（**服务默认独立于启动器**）。详见 [`CHANGELOG`](docs/CHANGELOG.md) 与
> [`实施记录`](docs/IMPLEMENTATION-v5.0.0.md)。下文「v5.0.0 重构要点」保留为该版本的对照表。

> 🏷 **发布标签：5.0.0 LTS（长期支持版）**。Cargo 版本保持合法的 semver `5.0.0`；
> "LTS" 只作为**发布标签**出现在 `--version` 输出（`DSHLauncher 5.0.0 LTS`）、文档、
> 安装向导与制品名中 —— 不写进版本号（`5.0.0-LTS` 会被 semver / npm 当成**预发布**版本）。
>
> 🏷 **Release label: 5.0.0 LTS.** The Cargo version remains the legal semver `5.0.0`; "LTS" is a
> release label only (see `--version`, the docs, the installer wizard and artifact names) — it is
> deliberately kept out of the version string, where `5.0.0-LTS` would mean a *pre-release*.

> ⚠️ **重要说明 / Important Notice**
>
> 本工具基于 **DeepSeek Harness 官方预览版**构建。官方预览版仍处快速迭代，未来可能发布**破坏性更新**（端口/协议/接口、配置格式、工作目录结构等），届时本启动器可能无法兼容，**本项目也可能随之停止维护**。
>
> This launcher is built on top of the **official preview release of DeepSeek Harness**, which may introduce **breaking changes** (port/protocol/interface, config format, working-directory layout, etc.). In that case this launcher may become incompatible.

**[中文说明](#中文说明) | [English](#english)**

---

# 中文说明

> 📊 **本页所有指标数字来自 [`docs/FACTS.json`](docs/FACTS.json)**（由 `tools/gen-facts.ps1`
> 从源码与产物实测生成，不再手写）。`tools/gen-facts.ps1 -Check` 会在数字漂移时报错。

## v5.0.0 重构要点

v5 是**从零开始的 Rust 全量重写**，替代原 C# WinForms 实现：

| 维度 | v4（C#） | v5（Rust） |
|---|---|---|
| 运行时依赖 | .NET Framework 4.8 + WebView2 + 3 个旁挂 DLL | **仅 WebView2 Runtime** |
| 产物 | 199 KB exe + 902 KB 旁挂 DLL + Node 网关 | **单文件 exe 约 1 MB**（准确值见 FACTS.json） |
| 代码量 | 7,720 行（含局域网功能） | **Rust 生产代码约 11,600 行**（准确值见 FACTS.json） |
| 常驻内存 | 约 64 MB | **约 13 MB（窗口关闭态）/ 约 24 MB（窗口打开态）** |
| 强杀后残留子进程 | 可能残留 | **0**（服务独立运行，启动器被强杀不影响它，也不留残渣） |
| 端口占用者识别 | WMI + netstat（偶发需管理员） | **GetExtendedTcpTable（无需管理员）** |
| 日志 IO | 超 2 MB 后每行全量重写（O(n)） | **append-only 滚动（O(1)）** |
| 可单测逻辑 | 约 0% | **149 个单元测试**（`dsh-core` 零 GUI 依赖，数字见 [`FACTS.json`](docs/FACTS.json)） |

### ⚠️ 与 v5.0.0 初版的语义变更（重要）

v5.0.0 初版把 dsh 挂在启动器的 **Job Object** 上（`KILL_ON_JOB_CLOSE`），
导致「退出 / 崩溃 / 被安装包升级覆盖启动器」都会**切断正在进行的会话**，
只有走托盘「退出 → 否」这一条路径才能保住服务。本版修正为：

- **默认 `service_lifecycle = "independent"`**：dsh **不挂在启动器的 Job 上**，
  退出 / 崩溃 / 升级都不中断会话；下次打开启动器由 `service.json` 对账后**自动接管**。
- 需要「启动器一死就回收一切」的用户，可在设置页取消勾选「服务独立于启动器」
  （即 `tied`），恢复旧语义（内核级零残留）。
- 服务归属由 `%LOCALAPPDATA%\DSHLauncher\service.json`（PID + 端口 + **进程创建时间**）
  记录并校验，**创建时间比对**用于防止 PID 复用导致的误接管。


### 局域网共享：已彻底移除

v5 **完全移除**局域网共享功能，相关代码、依赖、配置与 UI 入口均不保留：

- 无反向代理网关（原 `lan-gateway.mjs`，1122 行已删除）
- 无 PIN / Token / 会话密钥等凭据文件
- 无防火墙规则与 UAC 提权路径
- 无二维码、PWA 注入、移动端 UI
- 无 `OLLAMA_HOST=0.0.0.0` 暴露逻辑
- 入站监听面**归零**，仅保留 `127.0.0.1`

## 功能特性

- 🖥️ **内嵌 WebView2 界面**（无需浏览器）；WebView2 不可用时自动回退 **Edge 精简窗口**，再回退默认浏览器
- 🎨 **标题栏配色跟随 Harness 主题**（异步采样页面背景色并着色 DWM 标题栏，不阻塞界面）
- ⚡ **一键启动**：双击即自动启动服务并打开界面
- 🔗 **服务独立于启动器（默认）**：退出 / 崩溃 / 升级启动器都**不会**中断正在进行的会话；下次打开自动接管。可在设置页改为「随启动器退出而停止」
- 🔄 **自动接管（含身份校验）**：识别端口上已有的 Harness 进程并接管；**若占用端口的不是 dsh，则只读不接管、绝不误杀**（采用 node.exe 镜像名 + 安装目录特征 + 父进程链三重判定）
- 🧠 **自愈能力**：启动超时保护、运行中挂起自动检测
- 🔁 **失联自愈与自动重连**：运行中持续探测，服务消失约 12 秒内即判定失联并写入日志；**自有服务**自动重启（最多 3 次，稳定运行 5 分钟后额度自动恢复），**接管的实例**则持续看守——一旦该端口重新出现服务就**自动重新接管并让界面重连**，无需手动操作
- 🔓 **孤儿锁自动恢复**：dsh 被强杀时会残留 `<file>.lock`，导致下次启动报 `atomic-write: timed out waiting for the writer lock`。启动器在启动服务前读取锁文件里的**持有者 PID**，仅当该进程确实已退出时才清理（活跃锁绝不触碰），无需人工干预
- 🔑 **token 认证适配**：自动捕获 `dsh web` 输出的一次性 token URL 并导航
- 💤 **退出保留服务**：退出时可选保留 dsh web 后台运行（解除 Job 挂载），下次启动自动接管
- 📋 **设置窗口**：端口 / 工作目录 / Node 路径 / 关闭时最小化到托盘
- 🗂️ **托盘菜单**：打开界面 / 刷新 / 浏览器打开 / 启动 / 停止 / 新手指引 / 打开日志目录 / 设置 / 关于 / 退出
- 📝 **日志文件**：`%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`（append-only 滚动，单文件 2 MB × 3 份）
- 🔒 **日志脱敏**：自动剥离 `?token=xxx`，凭据不落盘
- 📦 **零旁挂依赖**：单文件 exe，WebView2 loader 静态链接
- 🌐 **离线构建**：`cargo build --offline`，`Cargo.lock` 入库
- 🧯 **崩溃取证**：`panic = "abort"` 下通过 `SetUnhandledExceptionFilter` 在日志里留下
  `[FATAL] 未处理异常 code=0x… address=0x…` 一行（无内存分配，不会二次崩溃）
- ⌨️ **快捷键**：内嵌窗口内 `F5` / `Ctrl+R` 刷新界面，`Esc` 按「关闭到托盘」偏好隐藏窗口

## 环境要求

- Windows 10 / 11（64 位）
- Node.js（缺失时安装包可自动部署）
- dsh（`npm install -g @deepseek-ai/dsh`，缺失时安装包可自动部署）
- **WebView2 运行时**（Win10/11 通常随 Edge 自带；缺失时回退 Edge 精简窗口）

## 从源码构建

需要 **Rust 工具链**与 **Windows SDK**。工具链由仓库根的
[`rust-toolchain.toml`](rust-toolchain.toml) **钉死**：`nightly-2026-09-10`（含 `clippy` / `rustfmt`），
`rustup` 会在首次构建时自动安装它。这也是本项目采用 **Cargo build-dir Layout v2** 的前提
（该布局目前只在 nightly 提供，stable 1.98.1 = 当前最新 stable 仍是旧布局）。
回退到 stable 只需 `Remove-Item rust-toolchain.toml`，完整步骤见
[`docs/OPS-RUNBOOK.md`](docs/OPS-RUNBOOK.md) §8。

```powershell
# debug 构建
powershell -ExecutionPolicy Bypass -File build.ps1

# release 构建（体积优化：opt-level=z + lto + panic=abort + strip）
powershell -ExecutionPolicy Bypass -File build.ps1 release

# 安装包（内嵌启动器 + 图标 + 文档）
powershell -ExecutionPolicy Bypass -File build-setup.ps1
```

产物：`DSHLauncher.exe`（单文件，应用图标已内嵌）。

### 构建缓存与清理（自清洁）

`target/` 是**构建缓存、不是交付物**（`.gitignore` 已排除 `/target/`），它的体积由"两份 profile 的
全部依赖产物"构成；`cargo` 从不回收旧产物（换 feature / profile / 工具链只增不减），因此需要显式回收：

```powershell
pwsh -NoProfile -File tools\clean.ps1            # 只报告四个区域（target / 仓库根 / %TEMP% / 运行期数据）
pwsh -NoProfile -File tools\clean.ps1 -Cache     # 清 target 缓存，保留 release 交付物（推荐）
pwsh -NoProfile -File tools\clean.ps1 -Repo      # 清仓库根残留（热替换副本 / 旧产物 / 门禁日志）
pwsh -NoProfile -File tools\clean.ps1 -Temp      # 清 %TEMP% 自检残留（白名单制）
pwsh -NoProfile -File tools\clean.ps1 -All       # 全清 target\（之后必须重新构建）
pwsh -NoProfile -File tools\clean.ps1 -Cache -WhatIf   # 只打印将删除的每一项，零改动
```

四个区域与两条硬约束：

| 区域 | 开关 | 说明 |
|---|---|---|
| `target\` | `-Cache` / `-All` | `-Cache` **除 release 交付物外全部清理**（对 cargo 新增目录天然免疫，不再依赖子目录白名单） |
| 仓库根 | `-Repo` | 热替换副本 `DSHLauncher.exe.bak-*`、`*.old-*`、`*.pubtmp`、`setup*.log`、`selftest.log` |
| `%TEMP%` | `-Temp` | **白名单制**（单测临时目录 / 卸载器副本 / 自检探针）；`dsh-spill-*`、`dsh-subprocess-*` 永不触碰 |
| `%LOCALAPPDATA%\DSHLauncher` | — | **只报告、永不删除**：日志 / WebView2 profile / service.json 属卸载器职责 |

两条硬约束：①`-Cache` 前置校验 release 交付物存在（缺失即退出码 2 并给出正确顺序：先构建再清缓存）；
②清完**自校验** `docs/FACTS.json` 记录的 `artifacts.launcher_build` 字节数与实测是否一致，
并明确列出「清缓存后无需重建即可跑」的门禁（`check-consistency` / `gen-facts -Check` / `verify-version` /
模拟安装 dry-run / `--version` 核对）。被运行实例占用的热替换副本是**跳过**而非失败（Windows 语义：
已映射的镜像删不掉），退出码只在 target 清理失败时为 1。

实测（本机，260+ 依赖）：清理前 `target\` **3,550.8 MB**（`debug` 2,629 = deps 1,292 + incremental 1,030
+ build 125 + examples 87；`release` 922），`-Cache` 后 **5.8 MB**，整个仓库根目录 **3,556 MB → 11.3 MB**。
`-Cache` 会保留 `target\release\dsh-app.exe`，因为 `docs/FACTS.json` 与 `tools/verify-version.ps1`
都以它为校验输入；从清理后的树**冷构建**仍完全离线可用（实测：`cargo build --release --locked` 89.7s，
`cargo test --workspace --all-features` 67.1s，均 exit 0）。

`tools/finish-release.ps1` 会关闭增量编译（`CARGO_INCREMENTAL=0`，把一轮验证的增量产物压到 0），
并把 `target\` 体积写进收尾报告的「6b. 构建缓存体积」一行，避免体积悄悄膨胀。

## 构建目录布局兼容性（Cargo build-dir v2）

> 背景：Cargo 的 *Call for Testing: Build Dir Layout v2* 把构建目录从「按内容类型组织」改为
> 「按包名 + 构建单元哈希组织」；`deps/` 与 `.fingerprint/` 消失，中间产物落到
> `build/<包名>/<哈希>/{fingerprint,out}`。**该布局在 Cargo 1.100 起已稳定并成为默认**。
> 另外 Cargo 1.91 起（稳定版可用）可以用 `CARGO_BUILD_BUILD_DIR` / `[build] build-dir`
> 把中间产物整块搬出 `target/`，只把**最终产物**留在 `target/<profile>/`。

**本仓库的依赖面（已逐条核对）**：只依赖 CFT 明确承诺**不变**的部分 ——
`target/<profile>/dsh-app.exe` 与 `target/<profile>/dsh-uninstall.exe`（最终产物）。
没有任何脚本或构建脚本依赖 `deps/`、`.fingerprint/`、`incremental/`、中间 `examples/`，
也没有从 `OUT_DIR` 反推 target 目录（Issue #13663 那一类）；example 路径改由 cargo 报告
（`--build-plan`… 实为 `--message-format=json` 的 `executable` 字段）取得，不再猜目录。

```powershell
pwsh -NoProfile -File tools\verify-build-layout.ps1                        # 静态断言 + 打印工具链事实
pwsh -NoProfile -File tools\verify-build-layout.ps1 -Run                   # 三种配置实测
pwsh -NoProfile -File tools\verify-build-layout.ps1 -Run -IncludeNightly   # 含 nightly 新布局
```

实测（主工具链 = 钉死的 `nightly-2026-09-10`，即 **v2 默认开启**；`-Run -IncludeNightly` 全量跑过）：

| 配置 | 结果 |
|---|---|
| **默认（= v2）** | 构建通过；中间产物按包名分桶在 `target\<profile>\build\<包名>\<哈希>\`，**`deps\` 与 `.fingerprint\` 不再出现**；最终产物仍在 `target\release\`；版本链路 33/0/0 |
| `CARGO_BUILD_BUILD_DIR=<root>\build-layout-probe` | 构建通过；**1178 个中间产物整块落到该目录**、最终产物仍在 `target\release\`；`clean.ps1 -Cache` 计划包含它 |
| 回退 stable 1.98.1（旧布局） | `Remove-Item rust-toolchain.toml` 即可；仓库无任何脚本依赖内部布局，**无需改源码** |

两个配套约束（门禁会拦）：① 任何脚本硬编码构建目录内部布局即红；② `.gitignore` 必须覆盖
`/build/` 等可能被搬迁的目录，且 `clean.ps1` 必须解析生效的构建目录（否则自清洁会漏掉最大的一块缓存）。

## 自检

覆盖三项核心验证：

```powershell
powershell -ExecutionPolicy Bypass -File selftest.ps1

# 只跑 Job Object 两项（跳过端到端）
powershell -ExecutionPolicy Bypass -File selftest.ps1 -SkipE2E
```

| 项 | 验证内容 |
|---|---|
| **A1** | 强杀启动器 → dsh 子进程由内核自动回收 |
| **A2** | 优雅退出（disarm）→ 子进程继续存活（保留服务语义） |
| **B** | 端到端：启动 dsh web → 就绪探测 → 停止 |
| **C** | 设置页 IPC 往返（页面按钮 → Rust 后端）+ 内嵌窗口已打开 |
| **D** | 单文件分发（exe 旁无 WebView2 侧挂数据目录） |

也可直接运行内置自检模式（启动 → 就绪 → 停止）：

```powershell
.\DSHLauncher.exe --selftest
```

## 命令行参数

| 参数 | 说明 |
|---|---|
| `--version` / `-V` | 打印版本后退出（`DSHLauncher 5.0.0 LTS (release)`）——安装包与 CI **核对版本**的最小接口，不触碰单实例互斥体 |
| `--help` / `-h` | 打印用法与退出码后退出 |
| `--selftest` | 隐藏自检模式：启动 dsh web → 就绪探测 → 停止，然后退出（0=通过；已有实例在运行则返回 2） |
| `--settings` / `-s` | 启动时打开设置窗口 |
| `--guide` / `-g` | 启动时打开新手指引窗口 |
| `--ipc-probe` | 自检用：打开设置页后自动模拟一次按钮点击，验证 IPC 往返 |
| `--quit` | 请求已在运行的实例**优雅退出**（发布/自动化脚本应使用它替代强杀——强杀会切断正在进行的会话） |
| `--build-info` | 打印构建与修复标记后退出，并写入 `%APPDATA%\DSHLauncher\build-info.txt`（部署脚本用它判定产物真伪） |
| `--probe-identity <PID>` | 诊断用：打印对某个 PID 的完整身份判定过程（防误杀闸门的可定位性），并写 `%APPDATA%\DSHLauncher\identity-probe.txt` |

退出码契约：`0` 成功 · `1` 自检失败 · `2` 已有实例导致 `--selftest` / `--ipc-probe` 无法执行 ·
`3` `--quit` 已送达但未收到回执（实例可能仍在运行，覆盖产物前必须自行确认）。

再次双击启动器图标时，第二个实例不会重复启动服务，而是通过命名事件
（`Local\DSHLauncher_Activate_v5`）唤起已有实例的窗口。

界面始终由启动器内嵌的 WebView2 窗口承载：启动 dsh 时会显式传 `--no-open`，
**不会再额外弹出系统浏览器**。dsh web 需要一次性 token（无 token 访问返回 401），
启动器会读取 dsh 的启动输出自动取到带 token 的地址，因此托盘「打开界面」与
「在浏览器中打开」都能直接进入，无需手动登录。

退出启动器时，确认框**默认是「否」= 后台保留服务**：正在进行的会话与任务不会中断，
下次打开启动器会自动重新接管。

## 一致性校验

校验路线图中的硬约束是否被破坏（版本同源、安装/卸载互逆、禁用依赖、分层纯净、
LAN 无残留、feature 修正等，共 179 项）：

```powershell
pwsh -NoProfile -File tools\check-consistency.ps1     # 源码与配置一致性
pwsh -NoProfile -File tools\verify-version.ps1        # 版本链路（Cargo → exe 资源 → 安装包 → 卸载器）
```

`build.ps1` 会在发布前自动调用 `verify-version.ps1`，版本资源缺失或版本号漂移会直接中断构建。

## 卸载

卸载器是随安装包发布的**原生 exe**（`dsh-uninstall.exe`，v5.0.0 LTS 起取代此前的
`uninstall.cmd` + `uninstall.ps1`）。通过「设置 → 应用」卸载、或直接运行安装目录里的
`dsh-uninstall.exe`，它会清理安装文件、注册表项、桌面图标，以及
`%LOCALAPPDATA%\DSHLauncher` 下的运行日志与内嵌浏览器缓存。

**为什么改成 exe**：脚本卸载器依赖 PowerShell 与执行策略 —— 命令行 `-ExecutionPolicy Bypass`
**覆盖不了组策略**（AllSigned/Restricted），AppLocker/WDAC 也能直接封锁脚本执行，
加固过的机器上用户根本卸载不掉；原生 exe 没有这层依赖，并且路径是原生 UTF-16
（非 ASCII / 超长 / 含引号的安装目录都安全）、可带版本资源与图标、可做代码签名。

```text
dsh-uninstall.exe                # 卸载（默认**保留**用户配置）
dsh-uninstall.exe --purge        # 连用户配置一起删除（亦接受 /purge）
dsh-uninstall.exe --silent       # 只打印问题（QuietUninstallString；亦接受 /quiet）
dsh-uninstall.exe --dry-run      # 只报告影响范围，不修改任何状态（亦接受 /whatif）
dsh-uninstall.exe --clean-residue # 只清"磁盘外"残留（注册表项 + 桌面快捷方式）
dsh-uninstall.exe --help         # 用法与退出码
```

退出码：`0` 成功（含"本来就干净"）· `1` 至少一步失败 · `2` 被防护规则拒绝
（缺安装标记 / 受保护路径 / 源码树 / 残留清理准入不通过）。

**安装目录已被删除时（悬空残留）**：卸载器的安装目录来自**它自身所在的目录**，因此目录一旦
被人为删除（或被磁盘清理、安全软件隔离），三条正规入口全部失效 —— 安装目录里的
`dsh-uninstall.exe` 已随目录消失，仓库/开发目录里的副本会被源码树护栏拒绝，
`--deferred-pass` 也会因缺少安装标记被拒绝。后果是「设置 → 应用」里的卸载按钮只会报找不到
可执行文件，而注册表卸载项与桌面快捷方式成了清不掉的悬空残留。此时用残留清理模式：

```text
dsh-uninstall.exe --clean-residue --dry-run   # 先看会清什么（零改动）
dsh-uninstall.exe --clean-residue             # 清掉注册表卸载项 + 桌面快捷方式
```

它**不删除任何文件或目录**（不碰 `%APPDATA%` 用户配置，也不碰 `%LOCALAPPDATA%` 运行期数据），
并且只在"注册表里的 `InstallLocation` / `UninstallString` / `DisplayIcon` 三者互相印证、
且安装标记已不存在"时才生效（受保护路径与源码树同样拒绝）—— 安装目录仍然完整时，请照旧走
上面的正规卸载。

**默认保留用户配置**：`%APPDATA%\DSHLauncher\settings.toml`（端口 / 工作目录 / node 路径）
不会被删除，重装后设置不丢失。

**先看会删什么（推荐）**：`--dry-run` 只报告影响范围，**不会删除或修改任何东西**，
逐项列出：将删除的安装文件、注册表卸载项、桌面快捷方式、`%LOCALAPPDATA%` 运行期数据、
用户配置是保留还是清除、以及安装目录是否会被递归删除（里面有第三方文件时会保留目录）。

**防护规则**（都有单元测试）：只有安装目录存在安装标记 `.dsllauncher-install`（且内容属于
本应用）才执行删除；盘符根 / 系统目录 / 用户目录等受保护路径拒绝递归删除；
目录里若有不属于本安装包的文件，只删自己的文件并**保留目录**；只结束**从本目录启动**的
启动器实例（按映像路径比较，读不到路径就不杀）。

**平台限制（如实说明）**：Windows 不允许运行中的镜像删除自己，因此删除安装目录本身由
"把自身复制到 `%TEMP%` 后重入"的副本完成；那份副本由**系统在下次重启时**清理
（实测：对自身取 `DELETE` 权限会被拒绝，这是平台限制而非遗漏）。每次卸载还会顺手
清掉 `%TEMP%` 里历史遗留的副本。

## 目录结构

```
Cargo.toml                              workspace 根配置（依赖与 release profile）
Cargo.lock                              锁定版本，离线可复现构建
crates/
  dsh-core/                             零 GUI 依赖的纯逻辑层（可 cargo test 覆盖）
    src/config.rs                        强类型配置（TOML + 旧 ini 迁移 + 校验 + schema 版本）
    src/process.rs                       子进程生命周期（independent / tied）+ 进程树终止
    src/service_record.rs                服务簿记 service.json + 启动对账（防 PID 复用）
crates/dsh-uninstall/                原生卸载器（取代脚本卸载器：护栏 + dry-run + 两阶段自删除）
crates/dsh-buildinfo/                构建期共享库：版本资源 / rc.exe 定位（启动器与卸载器共用一份）
    src/port.rs                          GetExtendedTcpTable 端口探测（无需管理员）
    src/probe.rs                         就绪探测 + 代际号防陈旧结果
    src/log.rs                           append-only 滚动日志 + token 脱敏
    src/maintenance.rs                   归档会话清理（reparse point 拒绝、JSON 护栏等）
    src/dsh.rs                           node/bin.js 解析、版本查询、升级、健康检查
    src/single_instance.rs               单实例互斥体 + 唤起事件 + 退出请求事件
    examples/job_object_demo.rs          Job Object 双语义验证程序
    examples/token_capture_probe.rs      token 捕获链路的运行期验证（含竞态复现）
    examples/active_session_probe.rs     活跃会话探测的运行期验证（交叉验证，防静默失效）
  dsh-ui/                               窗口 / 托盘 / 菜单 / 主题
    src/harness.rs                       内嵌窗口 + 异步主题采样 + 隐藏/显示
    src/settings.rs                      设置页（IPC + 双层转义）
    src/theme.rs                         DWM 着色 + 系统深浅色探测
    src/guide.rs                         新手指引窗口（渲染 ui/guide.html）
  dsh-app/                              唯一入口：组装 core + ui，运行 tao 事件循环
    src/service.rs                       服务生命周期编排（启动/接管/停止/失联自愈）
    src/crash.rs                         崩溃取证（SetUnhandledExceptionFilter，零分配）
ui/settings.html                        设置页（静态 HTML，内嵌进 exe）
ui/guide.html                           新手指引页（内嵌进 exe，唯一文案来源）
build.ps1 / build-setup.ps1             构建脚本（build.ps1 支持热替换发布 + 清理历史产物）
selftest.ps1                            自检脚本（A1 / A2 / B / C / D / E）
tools/verify-deployed.ps1               校验根产物修复标记 + 运行实例确有窗口且不 hung
tools/check-consistency.ps1             一致性校验（179 项，含行为性硬约束）
tools/verify-version.ps1                版本链路一致性校验（Cargo → exe 资源 → 安装包 → 卸载器）
tools/verify-service-lifecycle.ps1      服务独立性验证（含 -Force 判决性实验）
tools/verify-token-navigation.ps1       token 导航验证（静态 + HTTP 事实 + 日志断言）
tools/verify-install-layout-dryrun.ps1  模拟安装目录的卸载 dry-run（准入 + 载荷清单 + 零改动）
tools/verify-build-layout.ps1          构建目录布局兼容性验证（Cargo build-dir v2 / CFT；-Run 三配置实测）
tools/gen-facts.ps1                     生成 docs/FACTS.json（文档数字的唯一来源）
docs/TECHNICAL-ROADMAP.md               重构技术路线（§15 为 v5.0.0 定稿轮现状说明）
docs/AUDIT-REPORT-v5.0.0.md             深度审查报告（问题清单与分维度分析）
docs/IMPLEMENTATION-v5.0.0.md           v5.0.0 定稿轮处置记录与验证证据
docs/RELEASE-VERIFICATION-v5.0.0-LTS.md 5.0.0 LTS 发布工程轮验证记录（命令与结果）
docs/OPS-RUNBOOK.md                     发布/维护操作手册（含真实事故复盘）
docs/MAINTENANCE.zh.md / .en.md         dsh 升级与维护手册
```

## 文档地图

15 份文档按**用途**分三类 —— 需要什么就读对应那一份，不必通读：

| 文档 | 用途 | 什么时候读 |
|---|---|---|
| [`README.md`](README.md)（本文） | 用户入口：安装 / 使用 / 构建 / 卸载 / 自检 | 第一次接触 |
| [`docs/RELEASE_NOTES_v5.0.0.md`](docs/RELEASE_NOTES_v5.0.0.md) | 本版**用户可见**变更（中英） | 想知道"这版改了什么" |
| [`docs/MAINTENANCE.zh.md`](docs/MAINTENANCE.zh.md) · [`en`](docs/MAINTENANCE.en.md) | dsh 升级与维护手册（含故障排查） | dsh 出问题 / 要升级 dsh |
| [`docs/OPS-RUNBOOK.md`](docs/OPS-RUNBOOK.md) | 发布与运维手册 + **事故记录**（含恢复步骤与预防规则） | 发版 / 排查环境问题 |
| [`docs/TECHNICAL-ROADMAP.md`](docs/TECHNICAL-ROADMAP.md) | 设计与决策档案（§0–§12）+ 实施记录（§13–§15） | 想理解"为什么这样设计" |
| [`docs/CHANGELOG.md`](docs/CHANGELOG.md) | 版本历史（v1–v5，中英） | 对比版本差异 |

**证据档案**（按轮次留痕，**不必通读**，排查具体问题时按需查）：

| 文档 | 内容 |
|---|---|
| [`docs/AUDIT-REPORT-v5.0.0.md`](docs/AUDIT-REPORT-v5.0.0.md) | v5.0.0 定稿前的审计问题总表 |
| [`docs/IMPLEMENTATION-v5.0.0.md`](docs/IMPLEMENTATION-v5.0.0.md) · [`-prev-round`](docs/IMPLEMENTATION-v5.0.0-prev-round.md) · [`-lts-final`](docs/IMPLEMENTATION-v5.0.0-lts-final.md) | 各轮的逐项处置记录（问题 → 方案 → 结果） |
| [`docs/RELEASE-VERIFICATION-v5.0.0-LTS.md`](docs/RELEASE-VERIFICATION-v5.0.0-LTS.md) | 五轮发布验证证据（§1–§11，含"验证中发现的缺陷"与"失效结论更正"） |
| [`docs/CFT-FEEDBACK.md`](docs/CFT-FEEDBACK.md) | 待提交给上游 Cargo 的反馈草稿 |

**生成物 / 工具数据（勿手改）**：

| 文件 | 说明 |
|---|---|
| [`docs/FACTS.json`](docs/FACTS.json) | 文档数字的**唯一来源**，由 `tools/gen-facts.ps1` 实测生成；`-Check` 拦截漂移 |
| [`docs/FINISH-REPORT.md`](docs/FINISH-REPORT.md) | 每次 `tools/finish-release.ps1` 生成 |

> **数字口径**：README / CHANGELOG / 路线图 / 发布说明里的单元测试数、一致性项数、产物体积
> **一律以 `FACTS.json` 为准** —— 改完代码跑 `pwsh tools\gen-facts.ps1` 同步，`-Check` 会告诉你哪里漂了。

## 技术栈

- **GUI**：`wry`（WebView2 COM 直连）+ `tao`（窗口与事件循环）
- **托盘 / 菜单**：`tray-icon` + `muda`（原生菜单）
- **Window API**：`windows` crate（Job Object / IpHelper / DWM / MessageBox / Debug）
  - Job Object 仅在 `service_lifecycle = "tied"` 时用于托管 dsh；默认 `independent`
    用 `CREATE_BREAKAWAY_FROM_JOB` 让 dsh 独立于启动器（见 [`实施记录`](docs/IMPLEMENTATION-v5.0.0.md) §2.1）
- **并发**：`std::thread` + `mpsc`（不引入异步运行时）
- **配置**：`serde` + `toml`
- 刻意**不引入**：`tauri`、`tokio`、`regex`、`chrono`、`semver`、`tracing-*`（依赖精简与离线可构建）

## 许可证

[MIT License](LICENSE) © 2026 KristoffersonLee

---

# English

## v5.0.0 Rewrite Highlights

v5 is a **from-scratch Rust rewrite** replacing the original C# WinForms implementation.

> 📊 Every metric on this page comes from [`docs/FACTS.json`](docs/FACTS.json), generated
> from the sources and artifacts by `tools/gen-facts.ps1`. `-Check` fails on drift.

| Aspect | v4 (C#) | v5 (Rust) |
|---|---|---|
| Runtime deps | .NET Framework 4.8 + WebView2 + 3 side-by-side DLLs | **WebView2 Runtime only** |
| Artifacts | 199 KB exe + 902 KB DLLs + Node gateway | **single-file exe ~1 MB** (see FACTS.json) |
| Code size | 7,720 lines (incl. LAN) | **~8,800 lines of production Rust** |
| Resident memory | ~64 MB | **~13 MB (window closed) / ~24 MB (window open)** |
| Orphans after force-kill | possible | **0** (the service runs independently of the launcher) |
| Port→PID lookup | WMI + netstat (occasionally needs admin) | **GetExtendedTcpTable (no admin)** |
| Log IO | full rewrite per line past 2 MB (O(n)) | **append-only rolling (O(1))** |
| Unit-testable logic | ~0% | **149 unit tests** (`dsh-core`, zero GUI deps; see [`FACTS.json`](docs/FACTS.json)) |

### ⚠️ Semantic change vs. the initial v5.0.0 (important)

The initial v5.0.0 attached dsh to the launcher's **Job Object** (`KILL_ON_JOB_CLOSE`),
so quitting / crashing / being replaced by an installer **cut the running session** —
only the tray "Exit → No" path preserved the service. This release changes that:

- **Default `service_lifecycle = "independent"`**: dsh is **not** in the launcher's job
  object, so quitting / crashing / upgrading never interrupts a session; the next launch
  re-adopts it after reconciling `service.json`.
- Users who want "kill everything when the launcher dies" can uncheck
  *service is independent of the launcher* in Settings (that restores `tied`, the
  kernel-level zero-residue guarantee).
- Ownership is tracked in `%LOCALAPPDATA%\DSHLauncher\service.json` (PID + port +
  **process creation time**); the creation-time comparison prevents a recycled PID from
  being mistaken for our service.


### LAN sharing: fully removed

v5 **completely removes** LAN sharing — no code, dependencies, configuration, or UI entry points remain:

- No reverse-proxy gateway (the 1,122-line `lan-gateway.mjs` is deleted)
- No PIN / token / session-secret credential files
- No firewall rules and no UAC elevation path
- No QR code, PWA injection, or mobile UI
- No `OLLAMA_HOST=0.0.0.0` exposure
- Inbound attack surface is **zero** — only `127.0.0.1`

## Features

- 🖥️ **Embedded WebView2 UI** (no browser); falls back to a lightweight **Edge window**, then the default browser
- 🎨 **Title bar follows the Harness theme** (async sampling of the page background → DWM attributes; never blocks the UI)
- ⚡ **One-click start**: double-click to start the service and open the UI
- 🔗 **Service independent of the launcher (default)**: quitting / crashing / upgrading the launcher never interrupts a running session; the next launch re-adopts it. Switchable in Settings to "stop with the launcher"
- 🔄 **Auto-adopt with identity check**: takes over an existing Harness process on the port — but if the port is held by a **non-dsh** program it only reads and never adopts or kills it (node.exe image name + install-path markers + parent-process chain)
- 🧠 **Self-healing**: startup timeout guard and hang detection
- 🔁 **Loss detection & auto-reconnect**: the port is probed continuously; a vanished service is declared lost within ~12 s and logged. An **own** service is auto-restarted (up to 3 times, quota refilled after 5 minutes of healthy uptime); an **adopted** instance is kept under watch, so as soon as that port serves again the launcher **re-adopts it and reconnects the UI** — no manual step needed
- 🔓 **Orphan lock recovery**: a force-killed dsh leaves `<file>.lock` behind, making the next start fail with `atomic-write: timed out waiting for the writer lock`. Before starting the service the launcher reads the **owner PID** from the lock file and removes it only when that process is really gone (a live lock is never touched) — no manual cleanup needed
- 🔑 **Token-auth adaptation**: captures the one-time token URL printed by `dsh web`, and **only navigates once that token is in hand** — no more 401 "authentication required" page
- 💤 **Keep service on exit**: the default; the service is not attached to the launcher's job at all
- 📋 **Settings window**: port / working directory / Node path / minimize-to-tray / service independence
- 🗂️ **Tray menu**: open / refresh / open in browser / start / stop / guide / log folder / settings / about / quit
- 📝 **Log file**: `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` (append-only rolling, 2 MB × 3 files)
- 🔒 **Log redaction**: strips `?token=xxx`; credentials never hit disk
- 📦 **No side-by-side deps**: single-file exe, statically linked WebView2 loader
- 🌐 **Offline build**: `cargo build --offline` with a committed `Cargo.lock`
- 🧯 **Crash forensics**: under `panic = "abort"` a `SetUnhandledExceptionFilter` writes
  `[FATAL] unhandled exception code=0x… address=0x…` into the log (no allocation, cannot re-crash)
- ⌨️ **Shortcuts**: `F5` / `Ctrl+R` reload the embedded UI, `Esc` hides it (per the tray preference)

## Prerequisites

- Windows 10 / 11 (64-bit)
- Node.js (the installer can deploy it automatically)
- dsh (`npm install -g @deepseek-ai/dsh`, the installer can deploy it automatically)
- **WebView2 Runtime** (usually ships with Edge; if missing the app falls back to a lightweight Edge window)

## Building from Source

Requires a **Rust toolchain** (MSVC) and the **Windows SDK**:

```powershell
powershell -ExecutionPolicy Bypass -File build.ps1           # debug
powershell -ExecutionPolicy Bypass -File build.ps1 release   # release
powershell -ExecutionPolicy Bypass -File build-setup.ps1     # installer
```

Output: `DSHLauncher.exe` (single file, app icon embedded).

### Build cache & cleanup (self-cleaning)

`target/` is a **build cache, not a deliverable** (already excluded by `.gitignore`), sized by "every
dependency artifact of both profiles". Cargo never reclaims old artifacts (changing features, profiles
or toolchains only adds), so reclaiming must be explicit:

```powershell
pwsh -NoProfile -File tools\clean.ps1            # report only — deletes nothing
pwsh -NoProfile -File tools\clean.ps1 -Cache     # drop caches: debug profile + dependency caches (keeps release artifacts)
pwsh -NoProfile -File tools\clean.ps1 -All       # wipe target\ entirely (a rebuild is then required)
```

Measured on this machine (260+ dependencies): `target\` was **3,550.8 MB** (`debug` 2,629 = deps 1,292 +
incremental 1,030 + build 125 + examples 87; `release` 922) and is **5.8 MB** after `-Cache` — the whole
repo root went from **3,556 MB to 11.3 MB**. `-Cache` keeps `target\release\dsh-app.exe` because
`docs/FACTS.json` and `tools/verify-version.ps1` validate against it; a cold build from the cleaned tree
is still fully offline (measured: `cargo build --release --locked` 89.7 s, `cargo test --workspace
--all-features` 67.1 s, both exit 0).

`tools/finish-release.ps1` turns incremental compilation off (`CARGO_INCREMENTAL=0`, cutting a full
verification round's incremental output to zero) and records the `target\` size under
"6b. build-cache size" in its report so growth is always visible.

## Self-test

```powershell
powershell -ExecutionPolicy Bypass -File selftest.ps1
```

| Item | What it verifies |
|---|---|
| **A1** | Force-kill the launcher → the dsh child is reaped by the kernel (or survives, under `independent`) |
| **A2** | Graceful exit → the child keeps running (keep-service semantics) |
| **B** | End-to-end: start dsh web → readiness probe → stop |
| **C** | Settings-page IPC round-trip + embedded window opened |
| **D** | Single-file distribution (no WebView2 sidecar folder next to the exe) |
| **E** | Loss detection and automatic re-adoption |

Targeted verifiers for the two behavioural fixes in this release:

```powershell
# Service independence: static checks (+ a destructive experiment with -Force)
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1 -Force

# Token navigation: static checks + HTTP facts (401 without token) + log assertions
pwsh -NoProfile -File tools\verify-token-navigation.ps1
```

## Command line

| Flag | Description |
|---|---|
| `--version` / `-V` | Print the version and exit (`DSHLauncher 5.0.0 LTS (release)`) — the minimal interface for installers and CI to verify the build; it never touches the single-instance mutex |
| `--help` / `-h` | Print usage and the exit-code contract, then exit |
| `--selftest` | Hidden self-test: start dsh web → readiness probe → stop, then exit (0 = pass; 2 = another instance is running) |
| `--settings` / `-s` | Open the settings window at startup |
| `--guide` / `-g` | Open the getting-started guide at startup |
| `--ipc-probe` | Self-test helper: open settings and simulate one "save" click to verify the IPC round-trip |
| `--quit` | Ask the already-running instance to **exit gracefully** (use this from scripts instead of force-killing, which would cut an in-flight session); exit code 3 = request delivered but no acknowledgement |
| `--build-info` | Print the build/fix markers and exit, also writing `%APPDATA%\DSHLauncher\build-info.txt` |
| `--probe-identity <PID>` | Diagnostics: print every step of the dsh identity decision for a PID, also writing `%APPDATA%\DSHLauncher\identity-probe.txt` |

Exit codes: `0` success · `1` self-test failed · `2` an existing instance blocked `--selftest`/`--ipc-probe` · `3` `--quit` got no acknowledgement (the instance may still be running).

## Consistency check

```powershell
pwsh -NoProfile -File tools\check-consistency.ps1      # 179 source/config invariants
pwsh -NoProfile -File tools\verify-version.ps1         # version chain: Cargo → exe resource → installer → uninstaller
pwsh -NoProfile -File tools\gen-facts.ps1 -Check       # docs' numbers match measured reality
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1
pwsh -NoProfile -File tools\verify-token-navigation.ps1
```

`build.ps1` invokes `verify-version.ps1` before publishing, so a missing version resource or a
version drift aborts the build. `tools/gen-facts.ps1` is the single source of the numbers quoted
in the docs (see `docs/FACTS.json`); `-Check` fails when code and docs drift apart.

## Uninstall

The uninstaller is a **native exe shipped with the installer** (`dsh-uninstall.exe`; since
v5.0.0 LTS it replaces the former `uninstall.cmd` + `uninstall.ps1`). Uninstall through
Settings → Apps, or run `dsh-uninstall.exe` from the install directory: it removes the installed
files, the registry entry, the desktop shortcut and the logs / embedded-browser caches under
`%LOCALAPPDATA%\DSHLauncher`.

**Why an exe**: the script uninstaller depended on PowerShell and execution policy — a command-line
`-ExecutionPolicy Bypass` **cannot override Group Policy** (AllSigned/Restricted) and AppLocker/WDAC
can block script execution outright, so on a hardened machine the user simply cannot uninstall.
A native exe drops that dependency, handles UTF-16 paths natively (non-ASCII, very long or quoted
install directories), and can carry a version resource, an icon and a code signature.

```text
dsh-uninstall.exe                # uninstall (keeps your config by default)
dsh-uninstall.exe --purge        # also remove the user config (also accepts /purge)
dsh-uninstall.exe --silent       # print problems only (QuietUninstallString; also /quiet)
dsh-uninstall.exe --dry-run      # report the full scope, change nothing (also /whatif)
dsh-uninstall.exe --clean-residue # clean "off-disk" leftovers only (registry key + shortcut)
dsh-uninstall.exe --help         # usage and exit codes
```

Exit codes: `0` success (including "already clean") · `1` at least one step failed · `2` refused by a
guard (missing install marker / protected path / source tree / residue-cleanup admission).

**When the install directory is already gone (orphaned leftovers)**: the uninstaller derives the
install directory from **its own location**, so if that directory was deleted by hand (or by a disk
cleaner / AV quarantine) all three normal entry points are blocked — the `dsh-uninstall.exe` inside
it vanished with the directory, a copy in a source tree is refused by the source-tree guard, and
`--deferred-pass` is refused for a missing install marker. The result: the Uninstall button under
Settings → Apps only reports a missing executable, and the registry entry plus the desktop shortcut
become leftovers that nothing can remove. Use the residue mode:

```text
dsh-uninstall.exe --clean-residue --dry-run   # see what would be cleaned (changes nothing)
dsh-uninstall.exe --clean-residue             # remove the registry entry + desktop shortcut
```

It **deletes no files and no directories** (it never touches `%APPDATA%` user config or
`%LOCALAPPDATA%` runtime data), and it only runs when the registry's `InstallLocation` /
`UninstallString` / `DisplayIcon` agree with each other while the install marker is gone (protected
paths and source trees are refused as well). When the install directory is still intact, use the
normal uninstall above.

**Your configuration is preserved by default**: `%APPDATA%\DSHLauncher\settings.toml`
(port / working directory / node path) survives a reinstall.

**See what it would remove first (recommended)**: `--dry-run` reports the full scope — installed
files, registry entry, desktop shortcuts, `%LOCALAPPDATA%` runtime data, whether your config is kept
or purged, and whether the install directory itself would be deleted recursively (kept when it holds
third-party files) — and **changes nothing**.

**Guards** (all unit-tested): deletion requires the `.dsllauncher-install` marker (whose content must
belong to this app); protected roots (volume roots, system/user directories) are never deleted
recursively; a directory holding files that are not ours is kept with only our files removed; and only
launcher instances **started from this directory** are stopped (compared by image path — if the path
cannot be read, nothing is killed).

**Platform limitation (stated plainly)**: Windows forbids a running image from deleting itself, so the
removal of the install directory itself is performed by a copy of the uninstaller re-entered from
`%TEMP%`; that copy is removed by the OS **at the next reboot** (measured: requesting `DELETE` access
to one's own image is denied — a platform limitation, not an oversight). Every uninstall also sweeps
away stale copies left in `%TEMP%` by earlier runs.

## Documentation map

Fifteen documents, grouped by purpose — read only the one you need:

| Document | Purpose | When |
|---|---|---|
| [`README.md`](README.md) (this file) | User entry: install / use / build / uninstall / self-test | First contact |
| [`docs/RELEASE_NOTES_v5.0.0.md`](docs/RELEASE_NOTES_v5.0.0.md) | **User-visible** changes in this release (zh + en) | "What changed?" |
| [`docs/MAINTENANCE.en.md`](docs/MAINTENANCE.en.md) · [`zh`](docs/MAINTENANCE.zh.md) | dsh upgrade & maintenance guide (incl. troubleshooting) | dsh breaks / upgrading dsh |
| [`docs/OPS-RUNBOOK.md`](docs/OPS-RUNBOOK.md) | Release & ops runbook + **incident records** (recovery steps, prevention rules) | Publishing / environment issues |
| [`docs/TECHNICAL-ROADMAP.md`](docs/TECHNICAL-ROADMAP.md) | Design & decision archive (§0–§12) + implementation records (§13–§15) | "Why is it built this way?" |
| [`docs/CHANGELOG.md`](docs/CHANGELOG.md) | Version history (v1–v5, zh + en) | Comparing versions |

**Evidence archive** (append-only per round — no need to read in full; look things up as needed):
[`AUDIT-REPORT-v5.0.0.md`](docs/AUDIT-REPORT-v5.0.0.md) (audit findings),
[`IMPLEMENTATION-v5.0.0.md`](docs/IMPLEMENTATION-v5.0.0.md) + [`-prev-round`](docs/IMPLEMENTATION-v5.0.0-prev-round.md)
+ [`-lts-final`](docs/IMPLEMENTATION-v5.0.0-lts-final.md) (per-round disposition logs),
[`RELEASE-VERIFICATION-v5.0.0-LTS.md`](docs/RELEASE-VERIFICATION-v5.0.0-LTS.md) (five rounds of release
evidence, including defects found while verifying and corrected conclusions),
[`CFT-FEEDBACK.md`](docs/CFT-FEEDBACK.md) (draft feedback for upstream Cargo).

**Generated / tool data — do not edit by hand**: [`FACTS.json`](docs/FACTS.json) is the **single source
of truth** for the numbers quoted in the docs (produced by `tools/gen-facts.ps1`; `-Check` fails on drift);
[`FINISH-REPORT.md`](docs/FINISH-REPORT.md) is written by `tools/finish-release.ps1` on every run.

## Tech Stack

- **GUI**: `wry` (WebView2 COM) + `tao` (windows & event loop)
- **Tray / menu**: `tray-icon` + `muda`
- **Windows APIs**: `windows` crate (Job Object / IpHelper / DWM / MessageBox)
- **Concurrency**: `std::thread` + `mpsc` (no async runtime)
- **Config**: `serde` + `toml`
- Deliberately **not** used: `tauri`, `tokio`, `regex`, `chrono`, `semver`, `tracing-*`

## License

[MIT License](LICENSE) © 2026 KristoffersonLee
