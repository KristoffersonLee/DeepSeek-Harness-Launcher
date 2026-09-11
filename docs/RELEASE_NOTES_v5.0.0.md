# DeepSeek Harness Launcher v5.0.0 LTS

> 📌 **[中文](#中文更新说明)** · **[English](#english-release-notes)** · **[重构概要](#重构概要--rewrite-summary)** · **[实测指标](#实测指标--measured-metrics)** · **[兼容性](#兼容性--compatibility)** · **[资源](#资源--assets)**
>
> 发布日期 / Release Date: 2026-09-10
>
> 🏷 **发布标签 / Release label: 5.0.0 LTS（长期支持 / Long-Term Support）** ——
> Cargo 版本保持合法 semver `5.0.0`，"LTS" 只作发布标签（`--version` 输出、文档、安装向导、制品名）。
> The Cargo version remains the legal semver `5.0.0`; "LTS" is a release label only.
>
> ⚠️ **这是一次从零开始的全量重写**（C# WinForms → Rust），架构、依赖与构建链全部更换。

---

<a name="中文更新说明"></a>
## 中文更新说明

### 🎯 重构概要

v5 用 **Rust 从零重写**整个启动器，替换原 C# WinForms 实现。技术路线见
[`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md)。

| 维度 | v4（C#） | v5（Rust） |
|---|---|---|
| 运行时依赖 | .NET Framework 4.8 + WebView2 + 3 个旁挂 DLL | **仅 WebView2 Runtime** |
| 产物 | exe 199 KB + 旁挂 DLL 902 KB | **单文件 exe 1,055.5 KB（1,080,832 B）**（含图标与版本资源；准确值见 FACTS.json） |
| 常驻内存（工作集） | 65,520 K（约 64 MB） | **23.4 MB**（专用内存 3.7 MB；启动到界面可响应 **45 ms**） |
| 代码量 | 7,720 行（含局域网） | **Rust 生产 12,137 行 + 构建脚本 80 + 示例 431 + HTML 217**（准确值见 FACTS.json） |
| 依赖包 | — | 266（`Cargo.lock` 锁定，可离线构建） |
| 强杀后残留子进程 | 可能残留 | **0**（默认服务独立于启动器，强杀启动器不影响它；`tied` 模式由 Job Object 内核级保证） |
| 端口占用者识别 | WMI + netstat（偶发需管理员） | **GetExtendedTcpTable**（无需管理员） |
| 日志 IO | 超 2 MB 后逐行全量重写（O(n)） | **append-only 滚动（O(1)）** |
| 可单测逻辑 | 约 0% | **149 个单元测试**（`dsh-core` 97 · `dsh-ui` 13 · `dsh-app` 12 · `dsh-uninstall` 21 · `dsh-buildinfo` 6；`dsh-core` 零 GUI 依赖） |

### 🏗 架构

```
crates/
  dsh-core/   纯逻辑，零 GUI 依赖 —— 可被 cargo test 完整覆盖
              config / process / port / probe / log / maintenance / dsh / icon / single_instance
  dsh-ui/     窗口 / 托盘 / 菜单 / 主题 / 设置页 IPC
  dsh-app/    唯一入口：组装 core + ui，运行 tao 事件循环
ui/           设置页与新手指引（静态 HTML，内嵌进 exe）
```

技术选型：`wry`（WebView2 COM 直连）+ `tao`（窗口与事件循环）+ `tray-icon` / `muda`（原生托盘菜单）+ `windows`（Job Object / IpHelper / DWM / MessageBox）+ `std::thread` + `mpsc`（不引入异步运行时）。

**刻意不引入**：`tauri`、`tokio`、`regex`、`chrono`、`semver`、`tracing-*`——以保持依赖精简与离线可构建（由 `tools/check-consistency.ps1` 强制校验）。

### 🗑 破坏性变更：局域网共享彻底移除

v5 **完全移除**局域网共享，相关代码、依赖、配置与 UI 入口均不保留：

| 删除项 | 规模 |
|---|---|
| `lan-gateway.mjs`（Node 反向代理） | 1,098 行 |
| `LanAccess.cs`（LAN 探测 / PIN / 防火墙） | 680 行 |
| `DSHLauncher.cs`（旧 C# 启动器，含 LAN 逻辑） | 4,275 行 |
| `whale-256.png`（LAN 二维码 / PWA 图标） | 6.3 KB |
| `lib/` 三个旁挂 DLL | 896 KB |
| **合计** | **约 6,400 行 + 902 KB** |

同时移除：PIN / Token / 会话密钥等凭据文件、防火墙规则与 UAC 提权路径、Ollama 暴露逻辑、二维码与移动端 UI。

**入站监听面归零**——仅保留 `127.0.0.1`。

> 若你曾使用 v4 的局域网共享，升级后 `%APPDATA%\DSHLauncher` 下可能残留 `lan-*.txt` / `lan-gateway.mjs` / `firewall-add.ps1`，这些文件 v5 永不读取，可安全删除。

### 🆕 新功能

- **失联自愈与自动重连**：运行中持续探测（每 1.5 秒），服务消失约 12 秒内判定失联并写日志；
  **自有服务**自动重启（最多 3 次）；**接管的外部实例**持续看守——该端口一旦重新出现服务就
  **自动重新接管并让界面重连**，无需任何手动操作
- **孤儿锁自动恢复**：见下方「修复」
- **设置页 IPC**：设置窗口的按钮真正生效（保存设置 / 启动 / 停止 / 打开日志目录 / 清理归档会话），
  支持回填当前配置与状态回显
- **命令行参数**：`--selftest`（自检）、`--settings` / `-s`（启动时打开设置）、`--guide` / `-g`（新手指引）、
  `--ipc-probe`（IPC 往返自检）
- **指引与关于对话框**：原生 MessageBox，含端口 / 工作目录 / 托盘 / 退出语义说明
- **新手指引窗口**：托盘「新手指引」与 `--guide` 打开独立窗口，直接渲染内嵌的
  `ui/guide.html`（支持滚动与命令复制）；WebView2 不可用时回退 MessageBox 纯文本，
  纯文本同样从该 HTML 提取——指引文案**只有一处来源**
- **双击图标唤起已有窗口**：第二个实例不再静默退出，而是通过命名事件
  （`Local\DSHLauncher_Activate_v5`）通知首实例把内嵌窗口显示到前台（与 v4 `ShowWindow` 语义一致）
- **卸载残留清理（`--clean-residue`）**：安装目录已被删除（手工删除 / 磁盘清理 / 安全软件隔离）时，
  「设置 → 应用」里的卸载按钮只会报找不到可执行文件，而注册表卸载项与桌面快捷方式成了
  **清不掉的悬空残留**（正规入口都依赖安装目录本身）。该模式只清这两处**磁盘外**残留，
  **不删除任何文件或目录**（不碰用户配置与运行期数据）；准入要求注册表
  `InstallLocation` / `UninstallString` / `DisplayIcon` 三者互相印证，且安装标记已不存在。
  支持 `--dry-run` 先看后删

### 🛡 发布前审计轮（v5.0.0 定稿前整改）

本版在对外发布前又做了一轮全量审计，以下问题**均已确认并修复**：

| 问题 | 影响 | 修复 |
|---|---|---|
| exe **完全没有版本资源** | 资源管理器属性页全空、安装包 `DisplayIcon` 无版本信息 | `build.rs` 生成 `VS_VERSION_INFO`，版本号取自 `CARGO_PKG_VERSION`（单一来源） |
| rc.exe 查找会选到 **arm64** 架构 | 在 x64 宿主机上以 `os error 216` 失败，或静默跳过资源嵌入 | 按宿主架构筛选 SDK 目录；失败时**让构建失败**而非打 warning |
| 安装包版本硬编码 **4.2.4** | 与 `Cargo.toml` 的 5.0.0 冲突，注册表 `DisplayVersion` 显示旧版本 | 安装包 `AppVersion` 改为 5.0.0，`tools/verify-version.ps1` 强制校验同源 |
| 卸载**无条件删除** `%APPDATA%\DSHLauncher` | 用户端口/工作目录/node 路径配置被静默删除 | 默认保留配置，仅清理日志与浏览器缓存；`--purge` 才删配置 |
| 「清理归档会话」**没有先停服务** | 与界面文案承诺不符，服务占用目录时删除失败 | 清理前先停服务；清理移出 UI 线程（原实现最多阻塞主线程数秒） |
| 升级路径把 registry 返回值拼进 `cmd.exe /c` | 命令注入面（`1.0.0 & calc` 之类） | 直接调用 `npm.cmd` 并以独立参数传参 + semver 白名单校验 |
| 设置页状态回写**直接拼 JS 源码** | 路径含引号/`</script>` 时可注入脚本 | 双层转义（JSON → JS 字符串字面量），尖括号转 `\u003c` |
| 托盘事件**每帧只处理一个** | 连续点击菜单需多帧才全部生效 | 循环排空事件队列 |
| 就绪探测的总耗时**超出预算** | 300ms 预算实测跑成 803ms（单次探测自带 800ms 超时） | 每次探测的 connect 超时按剩余预算截断（新增回归测试） |
| `dsh-app/src/maintenance.rs` 无人引用 | 死代码：声明了升级/健康检查能力但界面无入口 | 删除整个模块；`dsh-core` 侧能力保留并新增注入测试 |
| `ui/guide.html` 无人引用 | 死资源：内嵌进 exe 却从不显示 | 接入新手指引窗口（见「新功能」） |

### 🔧 修复与优化

- **启动器启动即卡死（严重）**：`ServiceHandle` 在持有内部锁的状态下再次加锁记录日志，
  `std::sync::Mutex` 不可重入 → **自死锁**，程序停在「托盘图标已创建」后不再前进。
  改为免加锁的 `emit_*_with(&inner, ..)` 辅助函数（顺带消除了日志重复行与 `[INFO] [INFO]` 前缀）
- **接管已有服务时不打开界面（严重）**：`start()` 的「端口已有服务」分支只设置状态、
  未发出 `Ready` 事件，导致 `open_harness()` 从不被调用——**双击启动器后一直卡住、界面永不出现**。
  现三条启动分支都会发 `Ready`，并记录被接管进程 PID 供托盘「停止服务」使用
- **WebView2 在 exe 旁生成数据目录（严重）**：未显式指定用户数据目录时，WebView2 会在
  `<exe>.WebView2\` 建 profile——**破坏单文件分发，且装到 `Program Files` 等只读目录会直接初始化失败**。
  现显式指向 `%LOCALAPPDATA%\DSHLauncher\webview2-profile`，并全进程共享一份环境（同时降低内存）
- **设置窗口按钮全是死的（严重）**：设置页此前没有 IPC，点击只改文字、不执行任何动作，
  永远停在「正在启动…」。已实现完整的 `window.ipc.postMessage` → wry handler → mpsc → 主循环链路
- **双击出现终端窗口**：Rust 默认链接为 CONSOLE 子系统，双击启动器会额外弹出控制台窗口。
  release 构建现声明 `windows_subsystem = "windows"`（debug 保留控制台便于开发）
- **窗口标题栏不显示 logo**：未显式设置窗口图标时 Windows 用窗口类默认图标。
  现解析内嵌 `app.ico` 并设置 32×32 窗口图标（托盘图标同源）
- **dsh 孤儿锁导致启动失败**：dsh 用 `wx` 排他创建的 `<file>.lock` 串行化写入，进程被强杀时
  `finally { rm }` 不执行 → 之后启动报
  `atomic-write: timed out waiting for the writer lock`。
  启动器现在启动服务前读取锁文件中的**持有者 PID**，**仅当该进程确实已退出**才清理
  （活跃锁绝不触碰；内容不可解析时要求文件静置 5 秒以避开"创建后尚未写入"的竞态）
- **配置迁移不落盘**：`settings.ini → settings.toml` 迁移只在内存生效，每次启动重复读取 ini。
  现迁移成功即持久化
- **配置损坏静默降级**：v4 解析失败静默用默认值，用户不知设置未生效。现强类型校验 +
  明确弹窗提示（含文件路径与重置指引）
- **单实例测试依赖外部状态**：测试使用固定互斥体名，只要有启动器在运行测试必失败。
  改为 `acquire_named()` 使用唯一名，测试变为确定性
- **测试假通过（严重）**：exe 改为 GUI 子系统后，PowerShell 的 `&` **不再等待** GUI 进程，
  `$LASTEXITCODE` 会残留上一条命令的值 → 自检 B 段假通过、C 段又因与 B 争互斥体而失败。
  自检脚本改用 `Start-Process -Wait -PassThru` 取真实退出码；且 `--selftest` / `--ipc-probe`
  在互斥体冲突时**返回退出码 2**（此前返回 0，CI 会误判通过）
- **精简**：移除重复的 `single_instance` 实现（`dsh-app` 与 `dsh-core` 各一份），统一到 `dsh-core`

### 📋 一致性修正

- **版本号统一 v5.0.0**：`Cargo.toml` workspace 版本为唯一来源，`main.rs` 用
  `env!("CARGO_PKG_VERSION")` 编译期注入，不再硬编码
- **构建链更换**：`build.ps1` 由 `csc.exe` 改为 `cargo build --offline`；
  安装包 `build-setup.ps1` 不再内嵌 WebView2 DLL（v5 由 `windows-link` 静态链接）
- **新增校验工具**：`tools/check-consistency.ps1` 重写为 v5 版（53 项，校验版本同源、安装卸载互逆、禁用依赖、
  分层纯净、LAN 无残留、feature 修正、单文件分发等硬约束）

### 🧭 收尾整改（已并入 v5.0.0）

v5.0.0 对外发布前又完成了一轮审计处置（曾以内部迭代标识记录、**从未单独发布**），
其内容**已全部并入本版**，不再另立版本号：

- **服务与启动器解耦（行为语义变更，重要）**：默认 `service_lifecycle = "independent"`，
  dsh **不再挂在启动器的 Job Object 上** —— 退出 / 崩溃 / 被安装包升级覆盖 / 注销重启
  都**不会**中断正在进行的会话；归属改由 `%LOCALAPPDATA%\DSHLauncher\service.json`
  （PID + 端口 + **进程创建时间**）簿记并在启动时对账。需要「内核级零残留」的旧语义，
  可在设置页取消勾选「服务独立于启动器」（`tied`）。
- **修复「启动器只剩托盘、界面永不出现」的持锁 spawn 死锁（P0）**。
- **修复 token 捕获竞态（P0）**：实测「就绪」比「token 到达」早约 914 ms，
  旧实现此刻已用无 token 地址导航 → 界面是 **HTTP 401** 认证失败页。
- **防误杀**：非 dsh 程序占用端口时**只读不接管、不终止**；`stderr` 排空；
  `tray_on_close` 接线；`F5`/`Ctrl+R`/`Esc`；主题跟随（异步）；崩溃取证；
  `--quit` / `--build-info` / `--probe-identity`；`build.ps1` 支持热替换发布。

详见 [`CHANGELOG.md`](CHANGELOG.md) 的 v5.0.0 段落与
[`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md)（含逐项处置与运行期验证证据）。

### 🏷 发布工程轮：LTS 标签与 `--version` 入口（已并入 v5.0.0 LTS）

- **版本展示统一为 `5.0.0 LTS`**：Cargo 保持合法 semver `5.0.0`；`LTS` 是发布标签，
  出现在 `--version`、README、CHANGELOG、发布说明、安装向导标题与收尾报告标题中。
  刻意**不**写进版本号——`5.0.0-LTS` 在 semver 里表示"5.0.0 之前的预发布版本"。
- **新增命令行入口**：`--version` / `-V`、`--help` / `-h`（均在单实例检查**之前**处理，
  不抢互斥体、不开窗）。此前 `DSHLauncher.exe --version` 会被当成普通启动，
  于是"装完核对版本"这一步无法自动化。
- **一致性门禁扩到 147 项**（新增 29 条发布工程硬约束 + 离线版 `cargo machete` 等价检查）。
- **运行期加固**：启动服务移出 UI 线程、端口探测集中限流（此前每帧一次 300 ms 阻塞 connect）、
  外部命令探测限时、进程存活判定改用 `WaitForSingleObject`、进程树终止改为迭代 +
  去重、TCP 监听表读写对齐安全、`~` 定位失败不再退化成相对路径。逐条见
  [`CHANGELOG.md`](CHANGELOG.md) 的「发布工程轮」小节。

<a name="english-release-notes"></a>
## English Release Notes

### 🎯 Rewrite Summary

v5 is a **from-scratch Rust rewrite** of the whole launcher, replacing the C# WinForms
implementation. See [`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md).

| Aspect | v4 (C#) | v5 (Rust) |
|---|---|---|
| Runtime deps | .NET Framework 4.8 + WebView2 + 3 side-by-side DLLs | **WebView2 Runtime only** |
| Artifacts | 199 KB exe + 902 KB DLLs | **single-file exe, 1,055.5 KB (1,080,832 B)** (icon + version resource; see FACTS.json) |
| Working set | 65,520 K (~64 MB) | **23.4 MB** (private 3.7 MB; **45 ms** to a responsive UI) |
| Code size | 7,720 lines (incl. LAN) | **11,648 Rust production lines** + 80 build script + 431 example + 217 HTML (see FACTS.json) |
| Dependencies | — | 266, pinned by `Cargo.lock`, offline-buildable |
| Orphans after force-kill | possible | **0** (the service is independent of the launcher by default, so force-killing the launcher does not affect it; under `tied` the kernel-level Job Object guarantees it) |
| Port→PID lookup | WMI + netstat (occasionally needs admin) | **GetExtendedTcpTable** (no admin) |
| Log IO | full rewrite per line past 2 MB (O(n)) | **append-only rolling (O(1))** |
| Unit-testable logic | ~0% | **149 unit tests** (`dsh-core` 97 · `dsh-ui` 13 · `dsh-app` 12 · `dsh-uninstall` 21 · `dsh-buildinfo` 6; `dsh-core` has zero GUI deps) |

### 🗑 Breaking change: LAN sharing fully removed

v5 **completely removes** LAN sharing — ~6,400 lines and 902 KB deleted, along with PIN/token
credentials, firewall rules, the UAC elevation path, the Ollama exposure and the mobile UI.
**Inbound attack surface is zero** — only `127.0.0.1`.

> Upgrading from v4 may leave `lan-*.txt` / `lan-gateway.mjs` / `firewall-add.ps1` under
> `%APPDATA%\DSHLauncher`. v5 never reads them; they can be deleted safely.

### 🆕 New features

- **Loss detection & auto-reconnect**: the port is probed every 1.5 s; a vanished service is
  declared lost within ~12 s and logged. An **own** service is auto-restarted (up to 3 times);
  an **adopted** instance is kept under watch, so as soon as that port serves again the launcher
  **re-adopts it and reconnects the UI** — no manual step
- **Orphan lock recovery** (see below)
- **Settings-page IPC**: the settings buttons now actually work (save / start / stop / open log
  folder / clean archived sessions), with current values prefilled and status echoed back
- **CLI flags**: `--selftest`, `--settings` / `-s`, `--guide` / `-g`, `--ipc-probe`
- **Residue cleanup (`--clean-residue`)**: when the install directory was deleted by hand (or by a disk
  cleaner / AV quarantine), the Uninstall button under Settings → Apps only reports a missing
  executable, and the registry entry plus the desktop shortcut become leftovers nothing can remove.
  This mode cleans exactly those two **off-disk** leftovers and **deletes no files or directories**
  (user config and runtime data untouched); admission requires the registry's `InstallLocation` /
  `UninstallString` / `DisplayIcon` to agree with each other while the install marker is gone.
  Supports `--dry-run`

### 🔧 Fixes & improvements

- **Launcher hung on startup (critical)**: `ServiceHandle` re-locked a non-reentrant
  `std::sync::Mutex` while logging → **self-deadlock**. Replaced with lock-free `emit_*_with(&inner, ..)`
- **No window when adopting an existing service (critical)**: the adopt branch set state but never
  emitted `Ready`, so `open_harness()` was never called — **double-clicking appeared to hang forever**
- **WebView2 created a data folder next to the exe (critical)**: broke single-file distribution and
  **failed outright in read-only install locations**. Now pinned to
  `%LOCALAPPDATA%\DSHLauncher\webview2-profile`, shared process-wide
- **Settings buttons were dead (critical)**: no IPC existed. Full
  `window.ipc.postMessage` → wry handler → mpsc → event-loop path implemented
- **Stray console window on double-click**: Rust defaults to the CONSOLE subsystem; release builds
  now declare `windows_subsystem = "windows"`
- **Title-bar logo missing**: window icon is now parsed from the embedded `app.ico` and applied
- **dsh orphan lock**: dsh serialises writes through an `wx`-created `<file>.lock`; a force-killed
  process leaves it behind and the next start fails with
  `atomic-write: timed out waiting for the writer lock`. The launcher now reads the **owner PID**
  from the lock and removes it **only when that process is really gone**
- **Config migration was memory-only**; now persisted
- **Test false-positives (critical)**: after switching to the GUI subsystem, PowerShell's `&` no
  longer waits for the process, so `$LASTEXITCODE` was stale → false pass. Self-test now uses
  `Start-Process -Wait -PassThru`, and `--selftest` / `--ipc-probe` **exit 2** on mutex conflict
  (previously 0, a CI false pass)

### 🧭 Finalisation changes (folded into v5.0.0)

A further audit pass was completed before v5.0.0 shipped (recorded under an internal iteration label
that was **never released on its own**); **everything from it is part of this release**, with no
separate version number:

- **Service decoupled from the launcher (behavioural change, important)**: with the default
  `service_lifecycle = "independent"`, dsh is **no longer attached to the launcher's Job Object** —
  quitting / crashing / being replaced by an installer / signing out **never** interrupts a running
  session. Ownership moves to `%LOCALAPPDATA%\DSHLauncher\service.json` (PID + port +
  **process creation time**), reconciled at startup. Uncheck *service is independent of the launcher*
  in Settings (`tied`) for the old kernel-level zero-residue semantics.
- **Fixed the lock-held-spawn deadlock that left a tray-only process with no window (P0)**.
- **Fixed the token-capture race (P0)**: readiness was measured ~914 ms ahead of the token, and the
  old code navigated to the token-less URL at that moment — the embedded window showed **HTTP 401**.
- **No collateral kills**: when a non-dsh program owns the port the launcher only reads it and
  **never adopts or kills** it; `stderr` drained; `tray_on_close` wired; `F5`/`Ctrl+R`/`Esc`;
  theme following (async); crash forensics; `--quit` / `--build-info` / `--probe-identity`;
  hot-swap publishing in `build.ps1`.

See the v5.0.0 entry in [`CHANGELOG.md`](CHANGELOG.md) and
[`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md) for the item-by-item disposition and the
runtime verification evidence.

### 🏷 Release-engineering round: the LTS label and the `--version` entry point (folded into v5.0.0 LTS)

- **The displayed version is now `5.0.0 LTS`**: Cargo keeps the legal semver `5.0.0`; `LTS` is a
  release label shown by `--version`, in the README / CHANGELOG / release notes, in the installer
  wizard header and in the release-report title. It is deliberately **not** part of the version
  string: `5.0.0-LTS` would mean "a pre-release of 5.0.0" in semver terms.
- **New command-line entry points**: `--version` / `-V` and `--help` / `-h`, both handled **before**
  the single-instance check (no mutex, no window). Previously `DSHLauncher.exe --version` was treated
  as an ordinary launch, so "verify the version after installing" could not be automated.
- **Consistency gate grown to 147 checks** (29 new release-engineering invariants plus an offline
  `cargo machete` equivalent).
- **Runtime hardening**: service start moved off the UI thread, central throttled port probing
  (previously a blocking 300 ms connect on every frame), bounded external-command probes, liveness via
  `WaitForSingleObject`, iterative de-duplicated process-tree termination, alignment-safe TCP listener
  table access, and no more relative-path fallback when `~` cannot be resolved. Item by item in the
  "Release-engineering round" section of [`CHANGELOG.md`](CHANGELOG.md).

<a name="重构概要--rewrite-summary"></a>
## 重构实施记录 / Implementation Record

本节记录计划外的偏离与实现期发现的缺陷，作为工程档案。

| 项 | 说明 |
|---|---|
| 计划外新增代码 | 失联监测（约 150 行）、ICO 解析器（约 140 行 + 测试）、原生对话框、Edge 回退、设置页 IPC |
| 计划外新增测试 | 失联监测验证 `tools/verify-monitor.ps1`、图标验证 `tools/verify-icon.ps1` |
| 路线图未覆盖的设计冲突 | Job Object 的 `KILL_ON_JOB_CLOSE` 会让 v2.0「退出保留服务」失效 → 新增 `disarm_kill_on_close()`，优雅退出时按用户选择解除挂载（**强杀路径不受影响**） |
| 路线图修正 | `wry` 使用 `WebContext::new(Some(dir))` 而非 `with_user_data_folder`；`evaluate_script` 而非 `eval`；`JOB_OBJECT_LIMIT` 而非 `JOB_OBJECT_LIMIT_FLAGS` |
| 上游协议考证 | `dsh-atomic-write` 的锁文件内容即持有者 PID（`writeFile(lockPath, \`${process.pid}\n\`, {flag:'wx'})`），据此实现精确孤儿判定 |

<a name="实测指标--measured-metrics"></a>
## 实测指标（重构初版快照）/ Measured Metrics (initial-rewrite snapshot)

| 指标 | v4（基线） | v5（实测） | 变化 |
|---|---|---|---|
| 启动器工作集 | 65,520 K | **23.4 MB** | -64% |
| 启动器专用内存 | — | **3.7 MB** | — |
| 线程数 | — | 12 | — |
| 产物 | exe 199 KB + DLL 902 KB | **单文件 934.5 KB** | 更小且无旁挂 |
| 代码量 | 7,720 行 | **4,089 行** | -47% |
| 单元测试 | 0 | **48** | — |
| 一致性校验项 | — | **31** | — |
| 冷启动到界面 | — | 约 7–9 秒（含 dsh 就绪） | — |

> 说明：启动器工作集 **23.4 MB** 略高于路线图目标 `≤ 20 MB`；其专用内存仅 3.7 MB，
> 工作集包含 WebView2 loader 等共享页。WebView2 渲染进程为**独立进程**，不计入启动器。

<a name="兼容性--compatibility"></a>
## 兼容性 / Compatibility

- dsh ≥ 0.1.5（任意 dist-tag 通道）；Windows 10/11 64 位；WebView2 Runtime
  （缺失时自动回退 **Edge 精简窗口**，再回退默认浏览器）
- 构建需要：**由 [`rust-toolchain.toml`](https://github.com/KristoffersonLee/DeepSeek-Harness-Launcher/blob/main/rust-toolchain.toml) 钉死的 `nightly-2026-09-10`**
  （含 `clippy` / `rustfmt`；`rustup` 会在首次构建时自动安装）+ Windows SDK（`rc.exe` 用于内嵌图标）。
  构建缓存采用 Cargo **build-dir Layout v2**；回退到 stable 只需删除该文件 —— 见
  [`OPS-RUNBOOK.md`](OPS-RUNBOOK.md) §8
- 配置：v4 的 `%APPDATA%\DSHLauncher\settings.ini` 会在首次启动时**自动迁移**为
  `settings.toml`（仅迁移端口 / 工作目录 / Node 路径 / 托盘选项）
- **不再需要**：.NET Framework、旁挂 WebView2 DLL、Node 局域网网关

<a name="资源--assets"></a>
## 资源 / Assets

- 启动器 / Launcher：`DSHLauncher.exe`（**单文件**，图标已内嵌，无旁挂依赖）
- 安装包 / Installer：`DSHLauncherSetup.exe`（内嵌启动器 + README + 中英双语维护手册）
- 文档 / Docs：`README.md` · `docs/TECHNICAL-ROADMAP.md` · `docs/CHANGELOG.md` ·
  `docs/MAINTENANCE.zh.md` / `MAINTENANCE.en.md` · 本文件
- 校验工具 / Verification：`selftest.ps1`（A1/A2/B/C/D/E）·
  `tools/check-consistency.ps1`（**179 项**）· `tools/verify-version.ps1` · `tools/verify-embedded.ps1`
  （安装包内嵌资源与产物逐字节比对）· `tools/verify-monitor.ps1` · `tools/verify-icon.ps1` ·
  `tools/finish-release.ps1`（一键发布：15 步全通过）

### 🔒 依赖安全审计

发布前对 `Cargo.lock` 做了全量漏洞扫描（`cargo audit`，advisory-db 1243 条；离线环境可用等价的
OSV 通道 `tools/osv-audit.ps1`，数据同为 RustSec 镜像）：

| 范围 | 包数 | 漏洞记录 | 结论 |
|---|---|---|---|
| **Windows 构建图（真正进 exe）** | **138** | **0** | ✅ 无 |
| 全平台 lockfile | 266 | 3（`glib 0.18.5` ×2、`proc-macro-error 1.0.4`） | ⚪ 均属 Linux 专用 GTK 栈 |

这 3 条记录来自 `tao` 在 **Linux** 上默认启用的 GTK 栈
（`gtk → glib → glib-macros → proc-macro-error`），本项目已声明
`tao = { default-features = false }`，**Windows 构建不编译这些包**（已用
`cargo tree -i <crate> --target x86_64-pc-windows-msvc` 逐一验证为不存在）。
`tools/check-consistency.ps1` 已加防回归检查：GUI crate 若去掉 `default-features = false` 会直接失败。

### 🖥 会话体验修复（用户实测反馈）

| 问题 | 根因 | 修复 |
|---|---|---|
| 启动器会**额外弹出浏览器网页版** | `dsh web` 的默认行为就是自己打开系统默认浏览器（`--no-open` 才能关闭）；启动器拉起它时没有传该参数 | 子进程命令行追加 `--no-open`，界面只由启动器内嵌 WebView2 承载 |
| 托盘「在浏览器中打开」会**打开却报 401** | dsh web 要求一次性 token，普通地址返回 **HTTP 401**；token 只在 dsh 的 stdout 启动行里 | 持续读取子进程 stdout，解析 `dsh web: http://…?token=…` 并在内存中复用它（日志脱敏）；开窗/重连一律优先用带 token 地址 |
| **退出启动器容易误停服务、会话被强行断开** | 退出确认框 `MB_YESNOCANCEL` 默认焦点在「是 = 停止服务」，文案也没写清后果 | 默认焦点改为「否 = 后台保留服务」，文案明确列出三种选择的后果 |

> 保留服务的能力一直存在（Job Object `disarm_kill_on_close`，`selftest.ps1` A2 覆盖）；
> 本次修的是**默认值误导**。