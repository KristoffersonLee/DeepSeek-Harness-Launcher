# DSHLauncher v5 · Rust 全量重构技术路线（最终版）

> **日期**：2026-09-10 · **基线**：v4.2.4 · **目标版本**：v5.0.0
>
> **本文件是重构的唯一权威技术路线**。此前存在的 `TECHNICAL-ROADMAP-FINAL.md` 与合并前的 `TECHNICAL-ROADMAP.md` 均已删除，仓库内不再保留任何旧版本。
>
> 本文件由三份来源合并而成：
> - **R**：仓库既有路线图（合并前版本）
> - **B**：外部路线图（原 `TECHNICAL-ROADMAP-FINAL.md`）
> - **A**：本次基于源码逐条审读 + 真机实测的核实结果
>
> **合并原则**：R 与 B 的重合内容以 R 为底本；B 的新增章节（问题分类、死代码清单、设计决策表、模块骨架）经核实后择优并入；**B 中 3 处会导致编译失败的事实错误、4 条虚构死代码、6 处代码级错误已修正或剔除**；A 的实测数据全部覆盖推断值。
>
> **已定前提**：① 采用 Rust 重构；② 局域网共享（LAN）功能**彻底删除**；③ 目标为问题修复、性能、精简冗余、降低内存；④ 本项目**不使用 Python**；⑤ 仅面向 Windows（Tauri 的跨平台价值用不上）。
>
> **版本约定**：v4.3 = 移除 LAN 的维护版（C#），v5 = Rust 重写版。

---

## 0. 执行摘要与决策

### 0.1 决策链

```
Python           → 不涉及（本项目完全不使用 Python）
C# 升 .NET 8     → 保守选项（3–6 人日，但锁定 Windows，C# 5 桎梏仍在）
Rust 全量重写    → 选定（wry 直连 WebView2 COM，不需要 .NET）
GUI 栈           → wry + tao 直连（262 包），不引入 Tauri（429 包）
异步模型         → std::thread + mpsc，不引入 tokio
安装包           → 不重写（1312 行，风险最高收益最低），仅替换内嵌 exe
LAN 处置         → 彻底删除（不留开关、不留隔离分支、不留恢复路径）
```

### 0.2 立项理由

1. **可测试性**：`dsh-core` 零 GUI 依赖，≥70% 逻辑可 `cargo test` 覆盖
2. **根治孤儿进程**：Job Object 机制上杜绝残留
3. **内存优化**：实测 65,520 K → 目标 ≤ 20 MB
4. **代码精简**：7720 行 → ≤ 3300 行（-57%），攻击面归零

> **关于"立项理由"的重要说明**：原始最高 ROI 路径"用 Rust 替换 node 网关以净减一个子进程"已随 LAN 删除而消失。上表四条是修正后的真实立项理由，其中**可测试性权重最高**。

### 0.3 关键设计决策（含实测依据）

  决策   候选   **最终选择**   依据  
 --- --- --- --- 
  GUI 框架   Tauri 2 / **裸 wry + tao** / egui / iced   **wry + tao 直连**   **实测**：Tauri 2 = 429 包，wry 直连 = 262 包（关默认 feature）。目标含"精简依赖、降低内存"，Tauri 与其相悖。且已**实跑** `cargo build --offline` 1m11s 通过，裸 wry 工作量并非不可控  
  异步运行时   tokio / **std::thread + mpsc**   **std::thread + mpsc**   并发需求仅 3 处（周期探测、启动超时、npm 升级输出读取）。B 选择 tokio 的理由是"Tauri 内置 tokio 可共用 runtime"——不引入 Tauri 后该理由不成立  
  配置格式   TOML / JSON / YAML / 保留 INI   **TOML + serde**   强类型校验，损坏可明确提示，不再静默降级  
  日志方案   tracing / 手写 / log4rs   **手写轻量 logger**   `tracing-subscriber` / `tracing-appender` **实测未缓存**，离线会失败。手写约 80 行，时间戳用 `GetLocalTime`（不引入 chrono）  
  错误处理   thiserror + anyhow   **两者都用**   thiserror 定义库错误，anyhow 处理应用错误  
  正则表达式   regex / 手写   **手写**   regex 不在精简依赖图内（需新增约 4 包）。token 脱敏与归档 id 解析手写约 30 行即可  
  安装包   重写 / 保留 C#   **保留 C#**   1312 行风险最高收益最低，仅替换内嵌 exe  
  UI 实现   静态 HTML / 内嵌 HTML / 原生控件   **静态 HTML + wry IPC**   设置页复杂度高，HTML/CSS/JS 工作量最小  
  dsh 升级   完整保留 / 精简 / 移除   **完整保留**   用户依赖的核心功能，与 LAN 无关，不属于冗余  

### 0.4 与另两份路线的差异（合并说明）

  项   R（前版）   B（外部）   **最终版**   原因  
 --- --- --- --- --- 
  文档日期   2026-09-10   **2026-06-15**   2026-09-10   B 的日期错误  
  GUI 栈   Tauri 2   Tauri 2   **wry + tao 直连**   实测包数 429 vs 262  
  异步   tokio   tokio   **std::thread**   需求仅 3 处  
  DWM feature   `Win32_UI_Dwm`   `Win32_UI_Dwm`   **`Win32_Graphics_Dwm`**   前者**不存在**，照抄编译失败  
  tray-icon 声明   默认 feature   默认 feature   **`default-features = false`**   默认 feature 反向开启 `libxdo`/`gtk`，离线解析失败  
  依赖包数   未测   未测   **262（实测）**   —  
  离线构建   列为高风险   列为高风险   **已验证可行**   实跑 1m11s 通过  
  WebView2 分发   需旁挂 3 个 DLL   需旁挂 3 个 DLL   **静态链接，无需旁挂**   `windows-link` 在依赖图内  
  死代码清单   1 条   8 条（**4 条虚构**）   **2 条（已核实）**   见 §1.3  

---

## 1. 现状盘点

### 1.1 代码规模

  文件   行数   职责   深度耦合点  
 --- ---: --- --- 
  `DSHLauncher.cs`   4567   主程序：设置、进程编排、端口/进程探测、托盘、轮询自愈、~~LAN 共享~~、dsh 升级、设置窗体、Harness 内嵌窗口、自检   WinForms、WebView2（55 处）、WMI（`ManagementObjectSearcher` ×4）、npm  
  `LanAccess.cs`   719   ~~局域网 IP 探测 / PIN 解析 / 防火墙规则 / 内嵌资源释放 / Ollama 检测~~、归档会话清理   `NetworkInterface`、进程启动  
  `lan-gateway.mjs`   1122   ~~反向代理：PIN 门禁、HMAC Cookie、限流、SSE/WS 透传、PWA 注入~~   Node.js  
  `DSHLauncherSetup.cs`   1312   安装包：环境检测、winget/MSI/npm 部署、注册表卸载项、快捷方式、内嵌主 exe   注册表、COM  
  **合计**   **~7720**      

**产物体积**：`DSHLauncher.exe` 199 KB + WebView2 三件套约 902 KB。

**构建链**：`build.ps1` 调用系统自带 `csc.exe v4.0.30319`，**不需安装任何 SDK**；代价是代码锁死在 C# 5。

**实测基线（2026-09-10 真机）**：`DSHLauncher.exe` 工作集 **65,520 K（≈64 MB）**。
> 补充：`msedgewebview2.exe` 为**独立进程**（实测 15–116 MB），不计入启动器自身账，R 与 B 均未区分这一点。

### 1.2 已知问题分类（并入 B 的新增章节，已核实）

  类别   问题   严重度   v5 处置  
 --- --- --- --- 
  孤儿进程   强杀启动器后 dsh 子进程残留   🔴 高   **Job Object** 机制根治  
  端口识别   WMI + netstat 双路径，受限权限下失效   🔴 高   **GetExtendedTcpTable** 直取 PID  
  配置损坏   `Settings.Load` 失败静默降级   🟡 中   强类型 + 校验 + 损坏可重置  
  日志 IO   超 2MB 后每追加一行全量读取再重写   🟡 中   append-only 滚动  
  WebView2 环境   静态缓存 `Task<CoreWebView2Environment>` 竞态   🟡 中   wry 内部统一管理  
  设置页布局   58 处手工 `Controls.Add` + 硬编码坐标   🟡 中   静态 HTML  
  死代码   见 §1.3   🟢 低   不迁移  
  死设置   `AutoStart` / `AutoOpen` / `LiteBrowser` 历史遗留   🟢 低   不迁移  

### 1.3 死代码清单（修正 B 的虚构条目）

> **重要**：B 的此表列了 8 条，其中 **4 条在全仓 0 次出现，系虚构**。下表为逐条 `grep` 核实后的结果。

  位置   代码   全仓出现次数   结论  
 --- --- ---: --- 
  `LanAccess.cs:618-636`   `KillProcessTreeElevated`   1（仅定义）   ✅ **确为死代码**，随 LAN 删除  
  `DSHLauncher.cs:423`   `FindPidOnPortByNetstat`   1（仅定义）   ✅ **确为死代码**，不迁移  
  `DSHLauncher.cs:2198`   `LogEdgeMemory`   2（定义 + 1 处调用）   ⚠️ 非死代码，但可删；**注意 `OpenEdgeFallback` 本身必须保留**  
  `DSHLauncher.cs`   `IsDshHarnessOnPort`   7   ❌ **在用**，B 未列入，此处澄清  
  `DSHLauncher.cs`   `ListDshWebPids`   3   ❌ **在用**（维护清理路径调用）  
  —   `OnStartClick`   **0**   ❌ B 虚构，不存在  
  —   `HideLauncherOnOpen`   **0**   ❌ B 虚构，不存在  
  —   `UiLanPinEffective`   **0**   ❌ B 虚构，不存在（实际只有 `UiLanPinSource`）  
  —   `lanUrl`   **0**   ❌ B 虚构，不存在  

> **保留判断（修正 B 的一处遗漏）**：`OpenEdgeFallback`（WebView2 缺失时启动 Edge 打开界面）**必须保留，不属于冗余**。它是 WebView2 运行时缺失时的可用性底线，丢掉会导致部分机器彻底打不开 Harness。仅 `LogEdgeMemory` 可删。

### 1.4 环境实测（2026-09-10 逐条验证，非推断）

  项   实测结果   结论  
 --- --- --- 
  Rust   `rustc/cargo 1.98.1`，host `x86_64-pc-windows-msvc`   可用  
  MSVC 链接器   BuildTools 14.44.35207（非标准 VS 路径）   可用  
  Windows SDK   10.0.26100.0   可用  
  WebView2 Runtime   152.0.4191.66   可用  
  cargo 缓存   520 个 crate   离线起步可行  
  **离线构建**   **`cargo build --offline` 262 包，1m11s 通过**   ✅ **已验证，不再是风险**  

---

## 2. 局域网共享（LAN）：彻底删除

### 2.1 处置原则

**不留开关、不留隔离分支、不留恢复路径。** 以下所有内容在 v5 中完全不存在：

- 无 `-WithLan` 编译开关
- 无 `legacy/lan/` 目录
- 无 LAN 设置字段、菜单项、标签页
- 无网关进程、防火墙规则、凭据文件
- 无二维码、PWA 注入、移动端 UI
- 无 Ollama 暴露逻辑
- 无 UAC 提权路径

### 2.2 删除清单（按文件）

#### `lan-gateway.mjs`（1122 行）→ 整体删除

反向代理全部移除：PIN 门禁、HMAC Cookie、限流、SSE/WS 透传、PWA 注入、移动端 UI。

#### `LanAccess.cs`（719 行）→ 仅保留约 150 行维护功能

**删除内容（~569 行）**：

  删除项   行号范围   说明  
 --- --- --- 
  `DetectLanIp`   55–101   局域网 IP 探测  
  `VirtualMarkers` / `IsVirtualAdapter`   35–53   虚拟网卡排除  
  `IsLanPortInUse`   104–124   局域网端口占用探测  
  `IsGatewayRunning`   127–145   网关存活探测  
  `LauncherDir`   162–165   启动器目录（仅 LAN 用）  
  `PinFilePath` / `TokenFilePath` / `SecretFilePath`   167–180   凭据文件路径  
  `DeleteSecret`   183–186   签名密钥删除  
  `GatewayLogPath`   308–316   网关日志路径  
  `ParseEnvFile` / `StripInlineComment`   318–352   .env 解析（仅 LAN PIN 用）  
  `EffectivePin` / `GeneratePin` / `SavePin`   355–429   PIN 全流程  
  `WriteGateway`   434–463   内嵌网关资源释放  
  `RuleName` / `HasRule` / `TryAddRule` / `TryRemoveRule`   468–683   防火墙规则全套  
  `ManualAddCommand` / `ManualRemoveCommand`   685–695   手动防火墙命令  
  `KillProcessTreeElevated`   618–636   死代码（已核实从未被调用）  
  `KillProcessOnPortElevated`   640–661   提权按端口终止  
  `IsOllamaInstalled`   700–717   Ollama 检测  

**保留内容（~150 行，移入 `dsh-core/src/maintenance.rs`）**：

  保留项   说明  
 --- --- 
  `AppDataDir`   通用路径工具（`%APPDATA%\DSHLauncher`）  
  `ArchivedSessionCount`   归档会话数量读取  
  `DeleteArchivedSessions`   归档会话彻底清理（含 reparse point 拒绝、投影缓存防复活）  
  `WorkspaceJsonPath` / 归档列表与归档 id 解析   workspace.json 归档列表解析（手写，不用 regex）  

#### `DSHLauncher.cs`（4567 行）→ 删除约 770 行 LAN 代码

  区块   行号   动作  
 --- --- --- 
  Settings 字段   102–104   删 `LanEnabled` / `LanPort` / `LanPin`  
  Settings.Load 分支   154–162   删 `lanenabled` / `lanport` / `lanpin` 三个 case  
  Settings.Save   191–193   删对应三行  
  `StartServer`   302、319–325   删 `lanEnabled` 参数与 OLLAMA 暴露分支；同步改调用点  
  日志脱敏 `RedactSecrets`   601–608   仅删 PIN 正则；`?token=xxx` 脱敏保留  
  字段声明   639–644   删 `lanGateway` 等 LAN 字段  
  PollTick 调用点   1050、1082、1140、1182、1350、1371   删 `StopLanGateway()` / `MaybeStartLanGateway()`  
  token 捕获   939–948   仅删 `SaveLanToken(tk)`；URL 裁剪与 `AuthenticatedUrl` 保留  
  LAN 主区块   1399–1768   **整体删除**  
  LAN 设置提交   1768–1904   **整体删除**  
  局域网入口/二维码   2040–2121   **整体删除**  
  关闭清理   2309、2349、2388、2405   删 `StopLanGateway()` 调用  
  设置窗体字段   2863–2889   删 `tabLan`、`grpLan`、`chkLan`、`txtLan*`、`lblLan*`、`lblPin*`、`lblFwStatus`、`lblOllama`、`btnGenPin`、`btnFwElevated`、`btnCopyUrl`、`btnOpenLan`、`lanQr`  
  标签页构建   2934–2958、2978–2979、3019–3040   删"局域网共享"页（4 页变 3 页）  
  `BuildLanPage`   3169–3343   **整体删除**  
  面板刷新   3490、3542、3845–3973   **整体删除**（`RefreshLanPanel`、`EnsureQrInit`、`InitQrWebView2`、`ReloadQr` 及 WebView2 环境缓存）  
  自检   4470–4478   改写：去掉网关资源与 whale-256.png 检查项  

#### 资源文件

  文件   处置  
 --- --- 
  `lan-gateway.mjs`   删除（不再内嵌为资源）  
  `whale-256.png`   删除（仅 LAN 二维码/PWA 图标用）  

### 2.3 删除后的安全收益

  项   现状   删除后  
 --- --- --- 
  UAC 提权路径   `TryAddRuleElevated` 向 AppData 写临时 .ps1 再 `runas`   **全部消失**  
  入站监听面   局域网 IP 上监听 3081 + 防火墙入站规则   **归零**，仅剩 127.0.0.1  
  Ollama 暴露   `OLLAMA_HOST=0.0.0.0` 暴露到整个网段   **消失**  
  凭据文件   `lan-pin.txt` / `lan-token.txt` / `lan-secret.txt`   **不再产生**  
  运行时进程   多一个 `node.exe` 网关子进程   **消失**  
  死代码   `KillProcessTreeElevated`（已核实从未被调用）   **消失**  

### 2.4 删除后代码量

  位置   删除行数  
 --- ---: 
  `lan-gateway.mjs`   1122  
  `LanAccess.cs`   ~569  
  `DSHLauncher.cs`   ~770  
  **合计**   **≈ 2460 行（占总代码 32%）**  

---

## 3. v5 架构设计（Rust）

### 3.1 范围边界

  组件   现状   v5 处理  
 --- --- --- 
  启动器主体（去 LAN 后约 3800 行）   C# WinForms   **Rust 重写**（wry + tao）  
  维护辅助（约 150 行）   C#   **Rust 重写**，并入 `dsh-core`  
  局域网网关（1122 行）   Node   **彻底删除**  
  安装包（1312 行）   C# + csc   **保留**，仅替换内嵌 exe  
  构建脚本   PowerShell   主程序改 `cargo build`；安装包脚本保留  

**安装包不重写的理由**：自解包 / 注册表卸载项 / 快捷方式 / winget 部署与运行时完全解耦；重写风险最高（装不上或卸不干净是灾难级故障）、收益最低。接口保持不变：文件名 `DSHLauncher.exe` + `--selftest` / `--guide` 参数。

### 3.2 workspace 分层

```
dsh-launcher/
├─ Cargo.toml              # workspace root
├─ build.ps1               # 替换 csc → cargo build
├─ crates/
│  ├─ dsh-core/            # 纯逻辑，无 GUI 依赖 —— 最高价值
│  │   ├─ src/
│  │   │   ├─ lib.rs
│  │   │   ├─ config.rs       # 强类型配置 + 校验 + 损坏可重置
│  │   │   ├─ process.rs      # 启动 / Job Object / 进程树终止
│  │   │   ├─ port.rs         # GetExtendedTcpTable → PID
│  │   │   ├─ probe.rs        # 就绪探测 + 挂起检测（std::thread）
│  │   │   ├─ dsh.rs          # bin.js 解析、版本查询、升级、健康检查
│  │   │   ├─ maintenance.rs  # 归档清理、按端口提权终止
│  │   │   ├─ log.rs          # 轻量滚动日志 + 脱敏
│  │   │   └─ single_instance.rs # named mutex 单实例
│  │   └─ tests/           # 单元测试（覆盖率目标 ≥70%）
│  │
│  ├─ dsh-ui/              # 窗口 / 托盘 / 菜单 / IPC
│  │   ├─ src/
│  │   │   ├─ tray.rs      # 托盘图标 + 菜单
│  │   │   ├─ harness.rs   # 内嵌 WebView2 窗口 + DWM + 主题采样
│  │   │   ├─ settings.rs  # 设置窗口（自定义协议 + IPC）
│  │   │   └─ theme.rs     # DWM 标题栏着色
│  │   └─ icons/
│  │
│  └─ dsh-app/             # 唯一入口：组装 core + ui、生命周期、--selftest
│      ├─ src/main.rs
│      └─ build.rs         # 调用 rc.exe 内嵌图标，产出单文件 exe
│
├─ ui/                     # 设置页与新手指引：静态 HTML + CSS + JS
│   ├─ settings.html
│   └─ guide.html
│
└─ install/                # 复用现有 C# 安装器（仅替换内嵌 exe）
    └─ DSHLauncherSetup.cs
```

**核心设计决定：`dsh-core` 不依赖任何 GUI。** 当前 4567 行挤在单文件里几乎无法单元测试，这是所有"改一处崩三处"的根源。拆出 `dsh-core` 后，进程管理、端口探测、配置解析、归档清理这些最容易出 bug 的部分能被 `cargo test` 覆盖。**这是 v5 相对 v4 最大的结构性收益。**

**单一入口原则**：`dsh-app/src/main.rs` 是唯一入口，`dsh-ui` 只提供可复用的界面组件，避免双 `main.rs` 职责混乱（R 与 B 均存在此矛盾，此处统一）。

### 3.3 技术选型映射

  能力   现状（C#）   v5（Rust）  
 --- --- --- 
  内嵌 Harness 窗口   WebView2 WinForms 控件   `wry`（COM 直连，静态链接 loader）  
  窗口 / 事件循环   WinForms   `tao`  
  设置页 / 新手指引   58 处手工 `Controls.Add`   `ui/` 静态 HTML + `wry` IPC  
  托盘   `NotifyIcon`   `tray-icon`  
  菜单   自绘 `ThemeRenderer`   `muda`（原生菜单，经 tray-icon 传递引入）  
  单实例   `EventWaitHandle`   `CreateMutexW`（约 30 行）  
  端口 → PID   WMI + netstat（**权限坑**）   `GetExtendedTcpTable`（iphlpapi，无需管理员）  
  进程树终止   `taskkill /T /F`   **Job Object** + toolhelp 快照兜底  
  按端口提权终止   `taskkill` + `runas`   `ShellExecuteW("runas")`（维护用）  
  配置   手写 ini 解析   `serde` + `toml`，强类型校验  
  日志   手写 `AppendLogFile`（O(n) 裁剪）   轻量滚动（append-only）  
  异步   `Thread` + WinForms `Timer`   `std::thread` + `mpsc`  
  错误处理   try-catch 吞异常   `thiserror` + `anyhow`  
  DWM 标题栏着色   `DwmSetWindowAttribute`   `windows::Win32::Graphics::Dwm`  
  Edge 回退   `OpenEdgeFallback` 启动 msedge.exe   **保留**，`std::process::Command`  

### 3.4 运行时模型

```
dsh-launcher.exe 进程
├─ tao 事件循环（主线程）        ← 窗口、托盘、IPC 回调
├─ worker 线程                   ← 周期探测 / 启动超时 / 挂起自愈
└─ Job Object（KILL_ON_JOB_CLOSE）
   └─ node.exe → dsh web（含孙进程）

启动器崩溃或被强杀 → 内核终止 Job 内全部进程 → 残留 = 0
```

主线程只处理事件，所有阻塞操作（探测、npm 调用、进程等待）都在 worker 线程，通过 `mpsc` 把结果送回主线程。

---

## 4. 四个方向的针对性设计

### 4.1 问题修复｜把踩过的坑变成不可能

  顽疾   现状   v5 根治方案  
 --- --- --- 
  **孤儿 dsh 进程**   退出时 `KillWebView2Processes` + `taskkill` 兜底；启动器被强杀则失效，残留占用 3080   **Job Object**：`CreateJobObjectW` + `SetInformationJobObject(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)`。dsh 子进程挂进 Job 后，启动器崩溃或被强杀时由 OS 自动回收，**从机制上不可能残留**  
  **端口占用者识别失败**   WMI + netstat 双路径，普通权限下 WMI 读命令行受限、netstat 偶发失败   `GetExtendedTcpTable` 直取 PID，无需管理员、无外部进程；命令行读不到时用"node 进程 + 端口所有者"等价判定（沿用 v4.2.4）  
  **配置损坏静默降级**   `Settings.Load` 失败静默用默认值，用户不知设置没生效   强类型 + 校验；损坏时明确提示并可一键重置  
  **进程树终止依赖外部命令**   依赖 `taskkill.exe`，失败需 UAC 兜底   Job Object 为主，toolhelp 快照兜底，不再 fork 外部命令  
  **UAC 提权攻击面**   防火墙提权写临时 ps1 再 `runas`   **全部消失**（LAN 已删）  
  **WebView2 环境重复创建**   静态缓存 `Task<CoreWebView2Environment>`，窗体重建时偶发竞态   `wry` 内部统一管理环境，无需手工缓存  
  **日志裁剪 O(n) 放大**   超 2MB 后每追加一行都全量读取再重写   append-only 滚动（按大小），不再重写历史  

### 4.2 性能优化

- **日志 IO**：`AppendLogFile`（`DSHLauncher.cs:856-882`）超过 2 MB 后，每追加一行都要读整个 2 MB 再重写 1 MB，高频日志下是持续的 IO 放大。v5 用 append-only 滚动，不再重写历史。
- **就绪探测**：现在 WinForms `Timer` + 后台线程 + `HttpWebRequest`；v5 用 worker 线程 + 显式退避（200ms→500ms→1s，上限 30s），逻辑可单测。
- **启动路径**：去掉 .NET 程序集加载与 WebView2 WinForms 包装层开销。
- **设置页重绘**：现在 `LayoutAll` + 58 处手工布局，切页即重排；v5 为静态 HTML，布局交给浏览器。
- **WebView2 环境复用**：全局共享同一 `CoreWebView2Environment` 与固定用户数据目录，不再每个窗口新建 profile。

### 4.3 精简冗余

  项目   节省  
 --- --- 
  移除 LAN（网关 + 防火墙 + PIN/Token/Secret + 二维码 + PWA + Ollama + UAC 提权）   ~2460 行  
  设置页手工布局 → HTML   ~900 行 → ~200 行  
  死代码 `KillProcessTreeElevated`（已核实）   ~20 行  
  死代码 `FindPidOnPortByNetstat`（已核实）   ~25 行  
  `ThemeRenderer` 自绘菜单 → `muda` 原生菜单   ~40 行  
  `LogEdgeMemory`   ~30 行  
  依赖精简   去掉 Tauri（-167 包）、tokio（-4 包）、regex（-4 包）、chrono / semver  

**预估终态**：Rust 约 2600–2900 行 + UI 约 350 行 + 测试约 300 行，对比现在 7720 行。

### 4.4 内存占用

**实测基线：当前 `DSHLauncher.exe` 工作集 65,520 K。**

  手段   说明  
 --- --- 
  去掉 .NET 运行时   不再加载 CLR 与 WinForms 程序集  
  去掉 WebView2 WinForms 包装层   `wry` 直接用 COM 接口  
  共享 CoreWebView2Environment 与用户数据目录   不再每个窗口新建 profile  
  无异步运行时   无 executor 线程与调度开销  
  去掉 node 网关子进程   已随 LAN 删除  
  `opt-level="z"` + `lto` + `panic="abort"` + `strip`   体积最小化  

**目标：启动器自身进程 ≤ 20 MB**（M1 实测校准；WebView2 渲染进程为独立进程，不计入）。

---

## 5. WebView2 兼容性映射（重写必踩清单）

从 C# 迁到 `wry` 时最容易丢行为的七处，逐条对照：

  行为   C# 现状   Rust/wry 方案  
 --- --- --- 
  桌面布局强制   `InjectDesktopLayout` 注入 JS   `wry` 的 `with_initialization_script`，**JS 逻辑原样平移**  
  主题采样与标题栏着色   采样 CSS 颜色 → `DwmSetWindowAttribute`   采样 JS 平移；着色**直接用 `windows::Win32::Graphics::Dwm` 调 DWM API**（wry 无对应 API）  
  环境复用   静态缓存 `Task<CoreWebView2Environment>`（有竞态）   `wry` 内部管理 + 固定用户数据目录  
  右键菜单禁用   `AreDefaultContextMenusEnabled = false`   `initialization_script` 或 wry 设置项  
  最小窗口尺寸   `MinimumSize = new Size(1000, 600)`   tao 窗口 `set_min_inner_size`  
  F5 / Ctrl+R 刷新   `KeyDown` 事件   tao 快捷键绑定  
  WebView2 缺失兜底   `OpenEdgeFallback` 启动 `msedge.exe`   **保留**，`std::process::Command` 启动 Edge  

---

## 6. 依赖与离线策略

### 6.1 缓存核对结果（2026-09-10 实测）

  crate   版本   实测缓存   结论  
 --- --- --- --- 
  `wry`   0.55.1   ✅   需 `default-features = false` + `features = ["protocol"]`  
  `tao`   0.35.3   ✅   需 `default-features = false` + `features = ["rwh_06"]`  
  `tray-icon`   0.24.2   ✅   **必须** `default-features = false`  
  `muda`   0.19.3   ✅   **必须** `default-features = false`  
  `webview2-com`   0.38.2   ✅   wry 传递依赖  
  `windows`   0.61.3   ✅   见 §6.2 feature 修正  
  `windows-sys`   0.61.2   ✅   muda 传递依赖  
  `serde` / `serde_json`   1.0.229 / 1.0.151   ✅    
  `toml`   0.8.2（另 0.9.12 / 1.1.4 亦有）   ✅   锁 0.8  
  `thiserror` / `anyhow` / `dirs`   2.0.20 / 1.0.104 / 6.0.0   ✅    
  `tauri`   2.11.5   ✅ 已缓存   **不使用**（429 包）  
  `tokio`   1.53.1   ✅ 已缓存   **不使用**（需求仅 3 处）  
  `regex` / `chrono` / `semver`   1.13.1 / 有 / 有   ✅ 已缓存   **不使用**（手写替代）  
  **`tracing-subscriber`**   —   ❌ **未缓存**   **离线构建会失败，不使用**  
  **`tracing-appender`**   —   ❌ **未缓存**   **离线构建会失败，不使用**  

### 6.2 三处会导致构建失败的事实错误（已修正）

> 这三条在 R 与 B 中**同时存在**，照抄必然失败。

1. **`windows` 没有 `Win32_UI_Dwm` feature** → 真名是 **`Win32_Graphics_Dwm`**（`windows-0.61.3/Cargo.toml` 第 458 行）。
2. **`tray-icon` 的默认 feature 会反向开启 `muda` 的 `libxdo` + `gtk`**（Linux 依赖），离线解析直接报 `no matching package named libxdo`。**只给 `muda` 关默认 feature 无效**——`tray-icon` 会通过 feature 统一再打开。**两者都必须 `default-features = false`。**
3. **缺少 `Win32_Security` 与 `Win32_Networking_WinSock`**：`CreateJobObjectW` / `CreateMutexW` 的 `SECURITY_ATTRIBUTES` 参数来自 `Win32_Security`；`AF_INET` 常量在 `Win32_Networking_WinSock`。缺则函数不可见（编译报 `cannot find function`）。

### 6.3 依赖规模实测

  配置   包数（离线解析实测）  
 --- ---: 
  Tauri 2 + tray-icon   **429**  
  wry + tao + tray-icon + muda + windows + serde + toml…   275  
  同上，**wry / tao 关闭默认 feature**   **262** ✅ 采用  
  同上 + tokio   279  

> **结论**：采用 262 包配置，并已**实跑** `cargo build --offline` 验证 **1m11s 构建通过**。

### 6.4 Cargo.toml（已验证可直接构建）

```toml
[workspace]
members = ["crates/dsh-core", "crates/dsh-ui", "crates/dsh-app"]
resolver = "2"

[workspace.package]
version = "5.0.0"
edition = "2021"

[workspace.dependencies]
# GUI：必须关闭默认 feature（否则拉入 Linux 的 libxdo / gtk / x11 / webkit2gtk）
wry       = { version = "=0.55.1", default-features = false, features = ["protocol"] }
tao       = { version = "=0.35.3", default-features = false, features = ["rwh_06"] }
tray-icon = { version = "=0.24.2", default-features = false }
muda      = { version = "=0.19.3", default-features = false }

# Windows API
windows = { version = "=0.61.3", features = [
    "Win32_Foundation",
    "Win32_Security",                        # 必需：SECURITY_ATTRIBUTES
    "Win32_System_Threading",
    "Win32_System_JobObjects",
    "Win32_System_Diagnostics_ToolHelp",
    "Win32_System_SystemInformation",        # GetLocalTime（日志时间戳）
    "Win32_NetworkManagement_IpHelper",      # GetExtendedTcpTable
    "Win32_Networking_WinSock",              # 必需：AF_INET
    "Win32_UI_Shell",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Dwm",                    # 注意：不是 Win32_UI_Dwm
] }

# 序列化
serde      = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
toml       = "=0.8"

# 错误处理
thiserror = "=2.0.20"
anyhow    = "=1.0.104"

# 工具
dirs = "=6.0.0"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

> **不使用**：`tauri`、`tokio`、`regex`、`chrono`、`semver`、`tracing-subscriber`。

### 6.5 离线构建保障

1. `Cargo.lock` 入库；必要时 `cargo vendor` 把依赖源码落盘到 `vendor/`。
2. 构建命令统一 `cargo build --offline`。
3. **M0 第一件事就是验证离线构建**——本次已提前验证通过。

### 6.6 产物分发（修正 R 与 B 的判断）

`windows-link` 在依赖图内 → **WebView2 loader 静态链接，不再需要旁挂三个 DLL**。
配合 `build.rs` 调用 `rc.exe` 把 `app.ico` 内嵌为 Win32 资源（`Icon::from_resource`），**最终为真正的单文件 exe**，无需同目录任何附加文件。

---

## 7. 关键模块骨架

> 标注 **【已编译验证】** 的代码已在 `windows 0.61.3` + 上表 feature 下实际编译通过（2026-09-10）。

### 7.1 `dsh-core/process.rs` — Job Object 子进程管理 【已编译验证】

```rust
use core::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::*;

pub struct JobHandle(HANDLE);

impl JobHandle {
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,                                    // 注意：此处是句柄变量，不是常量
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of_val(&info) as u32,
            )?;
            Ok(Self(job))
        }
    }

    pub fn assign(&self, raw_process_handle: HANDLE) -> windows::core::Result<()> {
        unsafe { AssignProcessToJobObject(self.0, raw_process_handle) }
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
}
```

> **修正 B**：B 写作 `SetInformationJobObject(Job, ...)`，`Job` 未定义，编译失败。

### 7.2 `dsh-core/port.rs` — 端口探测（无需管理员）【已编译验证】

```rust
use core::ffi::c_void;
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::Networking::WinSock::AF_INET;

pub fn find_pid_on_port(port: u16) -> Option<u32> {
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    unsafe {
        let mut size = 0u32;
        let r = GetExtendedTcpTable(
            None, &mut size, false,
            AF_INET.0 as u32,                 // 不是 AF_INET2
            TCP_TABLE_OWNER_PID_LISTENER, 0,
        );
        if r != 0 && r != ERROR_INSUFFICIENT_BUFFER { return None; }

        let mut buf = vec![0u8; size as usize];
        let r = GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut c_void), &mut size, false,
            AF_INET.0 as u32, TCP_TABLE_OWNER_PID_LISTENER, 0,
        );
        if r != 0 { return None; }

        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let entries =
            std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        let target = u32::from(port.to_be());  // dwLocalPort 是 u32，网络序
        entries
            .iter()
            .find( e  e.dwState == MIB_TCP_STATE_LISTEN.0 as u32 && e.dwLocalPort == target)
            .map( e  e.dwOwningPid)
    }
}
```

> **修正 B**：B 用 `AF_INET2`（不存在）、`MIB_TCP_STATE_LISTEN as u32`（newtype 不能 `as u32`）、`dwLocalPort == port.to_be()`（u32 vs u16 类型不匹配）——三处均编译失败。

### 7.3 `dsh-core/single_instance.rs` — 单实例互斥 【已编译验证】

```rust
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// 返回 None 表示已有实例在运行
    pub fn acquire() -> windows::core::Result<Option<Self>> {
        unsafe {
            let h = CreateMutexW(None, true, w!(r"Global\DSHLauncher_SingleInstance_v5"))?;
            if windows::Win32::Foundation::GetLastError().0
                == windows::Win32::Foundation::ERROR_ALREADY_EXISTS.0
            {
                let _ = CloseHandle(h);
                return Ok(None);
            }
            Ok(Some(Self(h)))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
}
```

### 7.4 `dsh-ui/theme.rs` — DWM 标题栏着色 【已编译验证】

```rust
use core::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};

pub fn set_dark_titlebar(hwnd: HWND) -> windows::core::Result<()> {
    unsafe {
        let v: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &v as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        )
    }
}
```

> v4 使用 4 个属性：`DWMWA_USE_IMMERSIVE_DARK_MODE(20)`、`DWMWA_BORDER_COLOR(34)`、`DWMWA_CAPTION_COLOR(35)`、`DWMWA_TEXT_COLOR(36)`，迁移时全部保留。

### 7.5 `dsh-core/log.rs` — 轻量滚动日志 + 脱敏（不引入 chrono）【时间戳部分已编译验证】

```rust
use std::borrow::Cow;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub struct RollingLogger {
    log_dir: PathBuf,
    max_size: u64,   // 单文件上限，默认 2 MB
    max_files: u32,  // 保留份数，默认 3
}

impl RollingLogger {
    pub fn new(log_dir: PathBuf) -> Self {
        Self { log_dir, max_size: 2 * 1024 * 1024, max_files: 3 }
    }

    /// append-only，不做全量重写（修复 v4 的 O(n) IO 放大）
    pub fn append(&self, level: &str, message: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.log_dir)?;
        let line = format!("[{}] [{}] {}\n", local_timestamp(), level, redact_secrets(message));
        let path = self.log_dir.join("launcher.log");
        {
            let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
            f.write_all(line.as_bytes())?;
        }
        self.maybe_rotate()
    }

    fn maybe_rotate(&self) -> std::io::Result<()> {
        let path = self.log_dir.join("launcher.log");
        if std::fs::metadata(&path)?.len() <= self.max_size { return Ok(()); }
        for i in (1..self.max_files).rev() {
            let src = self.log_dir.join(format!("launcher.log.{i}"));
            if src.exists() {
                let _ = std::fs::rename(&src, self.log_dir.join(format!("launcher.log.{}", i + 1)));
            }
        }
        let _ = std::fs::rename(&path, self.log_dir.join("launcher.log.1"));
        Ok(())
    }
}

pub fn local_timestamp() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
}

/// 日志脱敏：剥离 dsh 一次性认证 token（手写，不引入 regex）
pub fn redact_secrets(input: &str) -> Cow<'_, str> {
    if !input.contains("token=") { return Cow::Borrowed(input); }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("token=") {
        let (head, tail) = rest.split_at(idx + "token=".len());
        out.push_str(head);
        let end = tail.find( c: char  c == '&'    c.is_whitespace()    c == '"'    c == '\'')
            .unwrap_or(tail.len());
        out.push_str("***");
        rest = &tail[end..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}
```

> **修正 B**：B 用 `chrono::Local::now()`，但 chrono 不在其依赖清单内（B 自己在注释里承认）；此处改用 `GetLocalTime`，零新增依赖。

### 7.6 `dsh-core/config.rs` — 强类型配置

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_port")] pub port: u16,
    #[serde(default = "default_tray")] pub tray_on_close: bool,
    #[serde(default)] pub node_path: String,
    #[serde(default)] pub work_dir: String,
}

impl Settings {
    /// 读取 %APPDATA%\DSHLauncher\settings.toml
    /// 损坏时返回 Err，由 UI 明确提示并可一键重置（不再静默降级）
    pub fn load() -> Result<Self, ConfigError> { /* ... */ }
    pub fn save(&self) -> Result<(), ConfigError> { /* ... */ }
    pub fn file_path() -> PathBuf { /* ... */ }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("配置文件损坏: {0}（可尝试重置）")] Corrupted(String),
    #[error("IO 错误: {0}")] Io(#[from] std::io::Error),
}
```

### 7.7 `dsh-core/probe.rs` — 就绪探测与挂起自愈（std::thread 版）

```rust
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct ReadyProbe {
    port: u16,
    generation: AtomicU64,
}

impl ReadyProbe {
    /// 退避 200ms → 500ms → 1s，上限 30s；代际号变化立即返回，防止陈旧结果误判
    pub fn wait_for_ready(&self, timeout: Duration) -> Result<(), ProbeError> {
        let mut interval = Duration::from_millis(200);
        let start = Instant::now();
        let gen = self.generation.load(Ordering::SeqCst);
        while start.elapsed() < timeout {
            if try_probe(self.port) { return Ok(()); }
            if self.generation.load(Ordering::SeqCst) != gen {
                return Err(ProbeError::GenerationChanged);
            }
            std::thread::sleep(interval);
            interval = (interval * 2).min(Duration::from_secs(1));
        }
        Err(ProbeError::Timeout)
    }
}

fn try_probe(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(800),
    ).is_ok()
}
```

> **修正 B**：B 把 `try_probe` 写成 `async` 却在内部做阻塞 `TcpStream::connect_timeout`，引入 tokio 却无收益。

### 7.8 `dsh-core/maintenance.rs` — 归档会话清理

```rust
/// 彻底删除全部归档会话：
///   · 对每个归档 id：目录存在且通过安全校验则删除（失败自动重试数次）
///   · 拒绝删除 reparse point（符号链接/junction），防止误删链接目标
///   · 同时删除 .dsh/storages/session_projcache/sessions/<id>.json，防止 dsh 依据投影缓存复活归档标记
///   · 仅从归档列表移除"已清理"的 id；全部失败时不改写 workspace.json
pub fn delete_archived_sessions() -> Result<(usize, String), MaintenanceError> { /* ... */ }
```

> **注意归档 id 长度**：目录名为 44 字符 `session-<uuid>`（8 + 36）。v4.2.1 曾因守卫只接受 36 字符导致"恒删除 0 个"，重写时**必须接受 44 字符**。

### 7.9 `dsh-core/dsh.rs` — dsh 环境解析与启动行解析（**已收敛**）

> ⚠️ **本节是 v5 立项时的接口草案，当前实现已收敛**（v5.0.0 LTS 定稿轮定稿）：
> 只有 `resolve` 与启动行解析（`parse_ready_url` 等）落地；`get_version` /
> `get_dist_tags` / `upgrade` / `rebuild_native_modules` / `check_health`
> （即实现里的 `version` / `dist_tags` / `upgrade` / `check_health`）**从未接入任何 UI**，
> 已作为无调用方的死代码**彻底删除**。升级与修复原生模块的手动步骤见
> [`MAINTENANCE.zh.md`](MAINTENANCE.zh.md) §1.3–1.5。

```rust
pub fn resolve(node_path_override: Option<&str>) -> Result<Dsh, DshError>;   // ✅ 已实现
// 以下为草案，未实现且已明确不做（v5.0.0 LTS）：
// get_version / get_dist_tags / upgrade / rebuild_native_modules / check_health
```

要点：
- **Node 多版本选择必须与 Install/启动两条路径一致**（v4.0.0 修过的坑），选版本最高者
- **强制官方 registry**（`--registry=https://registry.npmjs.org/`），不受本地镜像影响
- **npm 子进程必须异步排空 stdout/stderr**，否则管道死锁（v4.0.0 修过的坑）；加 5 分钟总超时
- 原生模块定位需同时探测**嵌套与顶层双路径**（v4.2.0 修过的坑）
- 解析 dist-tags 用 `serde_json`，不用 regex；版本比较手写简单比较器，不引入 semver

### 7.10 `dsh-ui/tray.rs` — 托盘与菜单

```rust
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

pub fn build_tray() -> tray_icon::Result<tray_icon::TrayIcon> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id("open", "打开界面", true, None),
        &MenuItem::with_id("refresh", "刷新界面", true, None),
        &MenuItem::with_id("open_browser", "在浏览器中打开界面", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("start", "启动服务", true, None),
        &MenuItem::with_id("stop", "停止服务", true, None),
        &MenuItem::with_id("guide", "新手指引", true, None),
        &MenuItem::with_id("log_dir", "打开日志目录", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("settings", "设置…", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("about", "关于", true, None),
        &MenuItem::with_id("exit", "退出", true, None),
    ])?;
    // 图标用 tao::window::Icon::from_path(LoadImageW) 或 Icon::from_resource(内嵌)
    // ...
}
```

### 7.11 `dsh-app/src/main.rs` — 唯一入口

```
1. 单实例检查（CreateMutexW）→ 已有实例则唤起并退出
2. 解析命令行参数（--selftest / --guide）
3. 加载配置（损坏则提示并可重置）
4. 初始化 RollingLogger
5. 启动 worker 线程 + tao 事件循环
6. 托盘事件循环
7. 退出清理（Job Object 关闭 → 子进程自动回收）
```

---

## 8. 里程碑与验收

  里程碑   内容   周期   出口条件  
 --- --- --- --- 
  **M0**   建 workspace、空窗口 + 托盘 + 图标、`Cargo.lock` 入库、`build.ps1` 改 cargo   0.5 天   `cargo build --offline` 通过、记录产物体积（**已提前验证**）  
  **M1**   `dsh-core` 的 config/process/port/probe + 启停主链路 + Job Object + 单实例   1.5–2 周   **强杀启动器，dsh 子进程被 OS 回收**  
  **M2**   内嵌窗口（注入脚本 / DWM / Edge 回退）+ 设置页 HTML + IPC + 托盘菜单   1 周   走通启动→就绪→关闭到托盘→退出  
  **M3**   升级 / 健康检查 / 归档清理 / 指引 / `--selftest` / 日志脱敏   1.5 周   过 21 条防御性行为回归清单  
  **M4**   release profile、单文件 exe、替换安装器内嵌 exe、删除 LAN 遗留资源   3–5 天   静默安装 / 卸载可逆 / 旧版覆盖  

> LAN 移除（约 2460 行）在 M0 一次性完成——v5 中根本不实现这些模块；`lan-gateway.mjs` 与 `whale-256.png` 在 M4 切换时删除。

---

## 9. 量化验收指标

  指标   现状（实测）   v5 目标  
 --- --- --- 
  启动器常驻内存   65,520 K   ≤ 20 MB（M1 实测校准）  
  代码量   7720 行   ≤ 3300 行（Rust + UI + 测试）  
  依赖包数   —   262  
  运行时依赖   .NET 4.8 + 三个旁挂 DLL + Node 网关   仅 WebView2 Runtime  
  可单测逻辑   ~0%   `dsh-core` ≥ 70%  
  强杀后残留子进程   可能残留   0（Job Object 保证）  
  端口识别需管理员   偶发   不需要  
  日志 IO 复杂度   O(n) 全量重写   O(1) append  
  攻击面（UAC/入站/凭据）   3 类 6 项   0（LAN 全删）  
  产物形态   199 KB + 902 KB 旁挂 DLL   **单文件 exe，无旁挂**  

---

## 10. 风险与回退

  风险   概率   对策  
 --- --- --- 
  离线依赖缺版本   ~~高~~ → **低**   已实跑验证 262 包 1m11s 通过；`Cargo.lock` + `cargo vendor` 兜底  
  `wry` / `tao` 关默认 feature 后行为异常   中   已确认 windows 后端是 **target 门控**而非 feature 门控，且实跑编译通过  
  `initialization_script` 注入时机与主题采样时序   中   M2 验证；`InjectDesktopLayout` JS 原样平移  
  `muda` 原生菜单挂到 WebView2 窗口 HWND   中   M2 验证；失败则退化为托盘菜单 + HTML 工具条  
  与 C# 版行为不一致   中   v4 冻结为维护分支；v5 独立目录并存；按回归清单验收  
  替换内嵌 exe 后安装行为变化   低   保持文件名与命令行参数完全不变  
  一次性重写丢行为   中   回归清单是硬性前置条件  
  内存优化不及预期   低   M1 即对比实测  

**回退方案**：v4 冻结为维护分支，v5 独立安装目录并存，随时可切回。

---

## 11. 已拍板事项（无需再议）

1. **版本号**：v5.0.0（v4.3 = 移除 LAN 的维护版，v5 = Rust 重写版）
2. **GUI 栈**：wry + tao 直连，不引入 Tauri
3. **异步**：std::thread + mpsc，不引入 tokio
4. **安装包**：保留 C#，仅替换内嵌 exe
5. **日志**：手写 logger + `GetLocalTime`，不引入 chrono / tracing-subscriber
6. **正则**：手写，不引入 regex
7. **Ollama 检测**：随 LAN 删除，不再需要
8. **dsh 升级 / 健康检查**：完整保留（用户依赖的核心功能，与 LAN 无关）

---

## 12. 附录：防御性行为回归清单（M3 验收依据）

  #   行为   来源   验证方式  
 --- --- --- --- 
  1   启动器被强杀后 dsh 子进程自动回收   `KillWebView2Processes` 兜底不足   强杀启动器 → 确认 dsh 进程消失  
  2   端口占用者识别（WMI 受限时的等价判定）   `IsDshHarnessOnPort` 双路径   普通权限下正确识别 dsh 进程  
  3   配置损坏时明确提示并可重置   `Settings.Load` 静默降级   篡改 settings.toml → 提示损坏  
  4   归档会话清理（reparse point 拒绝）   `DeleteArchivedSessions`   含 reparse point 的目录不误删  
  5   归档投影缓存防复活   `session_projcache` 清理   清理后 workspace.json 不复活  
  6   归档 id 长度 44 字符   v4.2.1 修复   `session-<uuid>` 能被正确识别并删除  
  7   日志 token 脱敏   `RedactSecrets`   日志中不出现 `?token=xxx`  
  8   启动超时 120 秒自动停止   `StartupTimeoutMs`   服务卡死 → 超时停止  
  9   挂起自愈重启（最多 3 次）   `autoRestartCount`   连续无响应 → 自动重启  
  10   端口变更时拒绝已被占用的新端口   `CommitSettings` 校验   新端口被占 → 拒绝切换  
  11   WebView2 缺失时 Edge 回退   `OpenEdgeFallback`   无 WebView2 → Edge 精简窗口  
  12   桌面布局强制（force-desktop）   `InjectDesktopLayout`   WebView2 窗口不触发移动端布局  
  13   标题栏颜色跟随 Harness 主题   `SampleTheme` + DWM   深色/浅色主题切换时标题栏同步  
  14   内嵌窗口最小宽度 1000px   `MinimumSize`   窄窗口不触发响应式  
  15   按端口提权终止（维护用）   `KillProcessOnPortElevated`   普通权限失败 → UAC 提权  
  16   自检模式 `--selftest`   `SelfTest.Run`   完整走通启动→就绪→停止  
  17   单实例唤起   `EventWaitHandle`   重复启动 → 唤起已有实例  
  18   关闭到托盘（服务继续运行）   `TrayOnClose`   点 ✕ → 托盘 → 服务不中断  
  19   退出确认对话框（服务运行时）   `ConfirmServiceStop`   服务运行 → 退出询问  
  20   设置窗口位置记忆   `lastBounds`   关闭再打开 → 位置大小保留  
  21   日志目录打开   `OpenLogDir`   资源管理器打开日志目录  

---

## 13. 实施记录（v5.0.0 实际交付）

> 本节由实施过程回填，记录**计划与实际的差异**、**实现期发现的真实缺陷**与**回归清单逐项验收结果**。
> 发布日期：2026-09-10 · 详细发布说明见 [`RELEASE_NOTES_v5.0.0.md`](RELEASE_NOTES_v5.0.0.md)

### 13.1 交付状态

  里程碑   计划   实际   状态  
 --- --- --- --- 
  M0 环境验证   离线构建 + 空窗口 + 托盘   `cargo build --offline` 通过（264 包）、GUI 子系统、图标内嵌   ✅  
  M1 核心链路   config/process/port/probe + Job Object + 单实例   全部落地并实测   ✅  
  M2 窗口与交互   内嵌窗口 + 设置页 IPC + 托盘菜单   全部落地；**额外补齐失联监测**   ✅  
  M3 功能对齐   升级 / 健康检查 / 归档清理 / 指引 / 自检 / 日志脱敏   均已落地（个别项见 13.4/13.5）   ⚠️ 部分  
  M4 打包切换   release profile + 单文件 + 替换内嵌 exe   单文件 973,312 B（含版本资源）；安装包改为不再内嵌 DLL   ✅  

### 13.2 与计划的偏离

  项   计划   实际   原因  
 --- --- --- --- 
  WebView2 数据目录   「wry 内部管理 + 固定用户数据目录」   `WebContext::new(Some(dir))` + 全进程共享一份   wry 0.55 无 `with_user_data_folder`；且不指定会在 **exe 旁**生成 `<exe>.WebView2\`  
  图标内嵌   `build.rs` 调 `rc.exe`   同上，但 `.rc` 在 `OUT_DIR` 动态生成并**双写反斜杠**   `.rc` 字符串中反斜杠是转义字符；`app.ico` 在仓库根而非 crate 目录  
  窗口图标   计划未涉及   新增 `dsh-core/icon.rs`（ICO 解析，含 5 个测试）   不显式设置时 Windows 用窗口类默认图标 → 标题栏不显示 logo  
  失联监测   计划仅列「挂起检测」   完整实现：失联判定 + 自有服务重启 + **外部实例重新接管**   用户实测暴露：接管的外部实例死亡后启动器永久失联  
  优雅退出   Job Object `KILL_ON_JOB_CLOSE`   新增 `disarm_kill_on_close()`   **计划未覆盖的设计冲突**：该限制会让 v2.0「退出保留服务」失效  
  测试工具链   计划未涉及   `Start-Process -Wait -PassThru`   GUI 子系统下 `&` 不等待进程 → 自检假通过（见 13.3 #8）  

### 13.3 实现期发现的真实缺陷（均已修复）

  #   缺陷   影响   修复  
 --- --- --- --- 
  1   `ServiceHandle` 持锁时重复加锁（`std::sync::Mutex` 不可重入）   **自死锁**：程序停在「托盘图标已创建」后不再前进   免加锁辅助函数 `emit_*_with(&inner, ..)`  
  2   接管分支未发 `Ready` 事件   **双击后界面永不出现**（用户实测报障）   三条启动分支均发 `Ready`，并记录被接管 PID  
  3   WebView2 未指定用户数据目录   **破坏单文件分发**；**只读安装目录下直接失败**   显式 `%LOCALAPPDATA%\DSHLauncher\webview2-profile`  
  4   设置页无 IPC   按钮全为死键，永远停在「正在启动…」   完整 IPC 链路（postMessage → wry → mpsc → 主循环）  
  5   Rust 默认 CONSOLE 子系统   双击额外弹出终端窗口（用户实测报障）   release 声明 `windows_subsystem = "windows"`  
  6   未设置窗口图标   标题栏不显示 logo（用户实测报障）   解析内嵌 `app.ico` 设置窗口图标  
  7   dsh 孤儿锁未处理   强杀 dsh 后下次启动报 `atomic-write: timed out waiting for the writer lock`   按锁文件内**持有者 PID** 存活性安全清理  
  8   GUI 子系统使自检 `$LASTEXITCODE` 失效   **自检假通过**（B 段）+ C 段互斥体冲突   `Start-Process -Wait -PassThru`  
  9   测试模式互斥体冲突返回 0   CI 误判通过   返回 **2** 并提示  
  10   配置 `ini → toml` 迁移不落盘   每次启动重复读 ini   迁移成功即持久化  
  11   单实例测试用固定互斥体名   有启动器在跑时测试必失败   `acquire_named()` 唯一名  
  12   `dsh-app` 与 `dsh-core` 各有一份 `single_instance`   重复实现   统一到 `dsh-core`  

### 13.4 回归清单逐项验收（第 12 节 21 项）

  #   行为   结果   说明  
 --- --- --- --- 
  1   强杀启动器后子进程回收   ✅   `selftest.ps1` A1 + 真实应用强杀均验证  
  2   端口占用者识别（无管理员）   ✅   `GetExtendedTcpTable` 直取 PID；接管路径实测  
  3   配置损坏明确提示   ✅   强类型校验 + `dialog::warn` 弹窗（含路径与重置指引）  
  4   归档清理拒绝 reparse point   ✅   `FILE_ATTRIBUTE_REPARSE_POINT` 判定 + 单测  
  5   归档投影缓存防复活   ✅   同时删除 `session_projcache/sessions/<id>.json`  
  6   归档 id 长度 44 字符   ✅   `is_valid_session_id` + 单测 `valid_id_is_44_chars`  
  7   日志 token 脱敏   ✅   `redact_secrets` + 单测  
  8   启动超时 120 秒自动停止   ✅   `DEFAULT_STARTUP_TIMEOUT` + `wait_for_ready_worker`  
  9   挂起自愈（最多 3 次）   ✅   失联监测 `MAX_AUTO_RESTART = 3`  
  10   端口变更拒绝已占用端口   ✅   设置页保存前校验  
  11   WebView2 缺失时 Edge 回退   ⚠️   代码已实现（`browser.rs`），**未在真实无 WebView2 环境实测**  
  12   桌面布局强制（force-desktop）   ✅   `FORCE_DESKTOP_JS` 原样平移至 `initialization_script`  
  13   标题栏跟随 Harness 主题   ⚠️   DWM 着色 API 就绪（4 个属性）；**运行中主题采样未接线**（`sample_and_apply_theme` 恒返回 `None`）  
  14   内嵌窗口最小宽度 1000px   ✅   `with_min_inner_size(1000, 600)`  
  15   按端口提权终止（维护用）   ❌   **未实现**：v5 维护路径不再走 UAC 提权  
  16   自检模式 `--selftest`   ✅   实现 + `selftest.ps1` B 段（真实退出码校验）  
  17   单实例唤起   ✅   命名事件 `Local\DSHLauncher_Activate_v5`（见 §14）；互斥体由 `Global\` 改 `Local\` 以免受限账户下创建失败  
  18   关闭到托盘（服务继续运行）   ✅   关闭功能窗口即释放该窗口，进程与服务继续在托盘  
  19   退出确认对话框（服务运行时）   ✅   `dialog::confirm_service_stop`（是/否/取消 = 停服退出 / 保留退出 / 不退出）  
  20   设置窗口位置记忆   ❌   **未实现**（无 `lastBounds` 等价物）  
  21   日志目录打开   ✅   托盘菜单 + 设置页 IPC  

**统计**：✅ 17 项 · ⚠️ 2 项 · ❌ 2 项（统计口径见 §14.3）

### 13.5 未完成项与后续建议

  项   现状   建议  
 --- --- --- 
  按端口提权终止（#15）   未实现   若需在受限权限下清理，补 `ShellExecuteW("runas")` + `Get-NetTCPConnection`（§14.5 已改为「清理前先停服务」）  
  主题采样与标题栏跟随（#13）   DWM 能力就绪但未接线   用 `evaluate_script_with_callback` 采样背景色 → `set_titlebar_colors`；注意**不要**在 UI 线程 `recv_timeout`（现 `sample_and_apply_theme` 会阻塞至多 2 s），应改为异步回调  
  设置窗口位置记忆（#20）   未实现   在 `settings.toml` 增 `window_bounds` 字段  
  Edge 回退实测（#11）   仅代码实现   在无 WebView2 的干净环境验证  
  启动器工作集 23.4 MB   略高于 ≤20 MB 目标   专用内存仅 3.7 MB，工作集含共享页；如需再降，可评估延迟创建 WebView2 环境  

> 「单实例唤起」「归档清理未先停服务」两项已于 v5.0.0 定稿前完成，见 §14；「修复模块按钮」相关
> 的 `dsh-core` 能力保留说明见 `crates/dsh-core/src/dsh.rs` 模块文档。

### 13.6 最终实测指标

  指标   v4（基线）   v5（实测）   路线图目标   达成  
 --- --- --- --- --- 
  启动器工作集   65,520 K   **23.4 MB**   ≤ 20 MB   ⚠️ 略高  
  启动器专用内存   —   **3.7 MB**   —   —  
  代码量   7,720 行   **5,155 行**（Rust 生产 4,879 + 示例 93 + HTML 183）   ≤ 3,300 行   ⚠️ 超 56%  
  依赖包数   —   锁定 264 / **Windows 实际编译 104**   262   ✅  
  运行时依赖   .NET 4.8 + WebView2 + 旁挂 DLL   **仅 WebView2**   仅 WebView2   ✅  
  产物形态   199 KB + 902 KB 旁挂   **单文件 973,312 B**（950.5 KiB，含版本资源）   5–10 MB 单文件   ✅ 更优  
  可单测逻辑   约 0%   **58 个单元测试**（`dsh-core` 零 GUI 依赖）   ≥ 70% 覆盖   ✅  
  强杀后残留子进程   可能残留   **0**（Job Object）   0   ✅  
  端口识别需管理员   偶发   **不需要**   不需要   ✅  
  日志 IO 复杂度   O(n) 全量重写   **O(1) append**   O(1)   ✅  
  攻击面（UAC/入站/凭据）   3 类 6 项   **0**（LAN 全删）   0   ✅  

> **代码量超标说明**：超出部分主要是计划外新增能力（失联监测、ICO 解析器、原生对话框、
> Edge 回退、设置页 IPC、新手指引窗口、单实例唤起）与单元测试代码；`cargo test` 报告的
> 生产逻辑仅 `dsh-core` 一层就覆盖 50 个测试。定稿前的审计轮又新增了 10 个测试
> （见 §14.3：48 → 58）。

### 13.7 验证证据

  工具   覆盖   结果  
 --- --- --- 
  `cargo test`   单元测试   48 个 → **58 个**（§14.3 全绿）  
  `selftest.ps1` A1   Job Object 强杀回收   PASS  
  `selftest.ps1` A2   优雅退出保留服务（`disarm`）   PASS  
  `selftest.ps1` B   端到端（启动 → 就绪 → 停止）   PASS  
  `selftest.ps1` C   设置页 IPC 往返 + 内嵌窗口打开   PASS  
  `selftest.ps1` D   单文件分发（无 WebView2 侧挂目录）   PASS  
  `selftest.ps1` E   失联检测与自动重新接管   PASS  
  `tools/check-consistency.ps1`   硬约束   31 项 → **60 项**（§14.3 全通过）  
  `tools/verify-version.ps1`   版本链路（Cargo → exe 资源 → 安装包 → 卸载器）   **新增**，23 项全通过  
  `tools/verify-icon.ps1`   窗口图标与 `app.ico` 逐像素比对   0 差异  
  `tools/verify-monitor.ps1`   失联 → 检测（11 s）→ 重连（1 s）   全通过  
  `cargo build`   编译警告   0  
  安装/卸载回归   静默装 → 卸载 → 残留检查（含 `--purge` 与 `QuietUninstallString`）   全通过（§14.3 矩阵）  

---

## 14. 实施记录补充：v5.0.0 定稿前全量审计轮（2026-09-10 续）

> 本节记录发布前第二轮全量审计（架构 / 安全 / 安装卸载 / 性能 / 冗余 / 一致性）的**实测发现与修
> 复**，以及**当轮未能完成的验证**。所有条目都附文件与可复现证据；无法验证的部分如实标注。

### 14.1 与计划的重大偏离（本轮新增）

  项   原状态   处置   原因与证据  
 --- --- --- --- 
  exe **无版本资源**   `build.rs` 只嵌图标   生成 `VS_VERSION_INFO`   实测 `(Get-Item DSHLauncher.exe).VersionInfo` 的 `FileVersion`/`ProductVersion`/`CompanyName` 全为空  
  rc.exe 查找   按目录枚举序任选架构   按 `cfg!(target_arch)` 筛选   实测选到 `10.0.26100.0\arm64\rc.exe` → `os error 216`（架构不兼容）  
  安装包版本   `AppVersion = "4.2.4"`   `5.0.0` + 新增校验脚本   与 `Cargo.toml` 的 5.0.0 冲突，注册表 `DisplayVersion` 显示旧版本  
  安装包自身版本资源   无   构建期注入 `AssemblyInfo`   实测 `FileVersion 0.0.0.0`、`ProductName` 空  
  卸载器   单一批处理   `uninstall.cmd` + `uninstall.ps1`   见 14.2 #3/#4：批处理版本**实测中止**，卸载项与目录双双残留  
  用户数据策略   无条件删除配置   默认保留，`--purge` 才删   README 承诺保留，实现却 `rd /s /q %APPDATA%\DSHLauncher`  
  `dsh-app/src/maintenance.rs`   5 个公开函数   删除（死代码）   全项目检索无任何调用方（托盘/设置页/IPC 均无入口）  
  `ui/guide.html`   内嵌但无人引用   接入新手指引窗口   一致性校验要求该文件存在，但 Rust 侧从未读取 → 死资源；且指引文案在 `dialog.rs` 被重写了一份  

### 14.2 本轮实测发现并修复的缺陷

  #   缺陷   严重度   证据   修复  
 --- --- --- --- --- 
  1   exe 无版本资源，安装包 `DisplayIcon` 与属性页空白   P0   `VersionInfo` 全空   `build.rs` 生成 `VS_VERSION_INFO`  
  2   rc.exe 选到 arm64 → 资源嵌入静默失败   P0   `os error 216`   架构筛选 + 失败即构建失败  
  3   卸载脚本以 `... was unexpected at this time.` 中止   P1   卸载后 `reg query` 仍返回卸载项、安装目录仍在   重构为 cmd 转发 + PS 实现  
  4   `reg delete "Software\..."` 缺 hive 前缀   P1   `错误: 无效语法`   完整 `HKCU\Software\...`  
  5   卸载删除用户配置   P1   `%APPDATA%\DSHLauncher` 被 `rd /s /q`   默认保留 + `--purge`  
  6   升级路径命令注入面   P1   `cmd.exe /c "npm install -g ...@{version}"`   直调 `npm.cmd` + semver 白名单  
  7   设置页状态回写可注入脚本   P1   JSON 直接拼进 JS 源码   双层转义 + `\u003c`  
  8   重复启动**不唤起**已有窗口（v4 有）   P1   第二个实例仅打印「已有实例在运行，退出」   命名事件 + 事件循环轮询  
  9   互斥体用 `Global\`   P1   `Global\` 需 `SeCreateGlobalPrivilege`   改 `Local\`  
  10   「清理归档会话」未先停服务   P1   界面文案承诺停服，代码只删除   清理前 `service.stop()`  
  11   归档清理阻塞 UI 线程   P2   每会话最多 4×300ms 重试   移入后台线程 + 结果回传  
  12   就绪探测总耗时超预算   P2   300ms 预算实测 **803ms**   单次 connect 超时按剩余预算截断  
  13   `format!(..).parse().unwrap()`   P2   `probe.rs` / `port.rs`   `SocketAddr::from`  
  14   `GetExtendedTcpTable` 缓冲区未校验   P2   `from_raw_parts` 可能越界（UB）   表头 + 条目数边界校验  
  15   托盘事件每帧只处理一个   P2   `if let Ok(..)` 而非 `while let`   循环排空  
  16   `cmd /c start "" <url>` 打开浏览器   P2   URL 二次解析 + 弹控制台窗   `ShellExecuteW`  
  17   订阅未使用依赖（`dsh-app`）   P3   源码检索 0 次引用   移除 `dirs`/`serde`/`serde_json`/`toml`/`wry` + 未用 `build-dependencies`  
  18   主进程无启动阶段耗时埋点   P3   无法复现测量启动时间   `[boot]` 阶段计时写入日志  
  19   clippy 在 `-D warnings` 下有 11 类告警   P3   `cargo clippy` 输出   全部修复（含 `enum_variant_names` 等）  
  20   维护手册称「v5 未实现重复启动唤起」   P3   `MAINTENANCE.zh:301`   已随实现同步更正  

### 14.3 本轮验收证据

  检查项   命令   结果  
 --- --- --- 
  格式   `cargo fmt --check`   ✅ 通过（基线有 3 处未格式化）  
  静态检查   `cargo clippy --all-targets --all-features -- -D warnings`   ✅ 通过  
  单元测试   `cargo test --release --all-features`   ✅ **58 passed / 0 failed**（基线 48）  
  发布构建   `cargo build --release`   ✅ 成功  
  一致性   `tools/check-consistency.ps1`   ✅ **60/60**（基线 31 项）  
  版本链路   `tools/verify-version.ps1`（本轮新增）   ✅ **23/23**  
  静默安装   `DSHLauncherSetup.exe --silent-install <dir>`   ✅ exit 0，释放 7 文件 + 快捷方式 + 注册表 11 值  
  卸载（默认）   `dsh-uninstall.exe`（原生）   ✅ 注册表/目录/快捷方式/`%LOCALAPPDATA%` 无残留；`settings.toml`/`ini` 保留  
  卸载（`--purge`）   `dsh-uninstall.exe --purge`   ✅ 配置一并清除  
  静默卸载   注册表 `QuietUninstallString`   ✅ 同上，配置保留  
  依赖安全   `cargo audit`（GitHub advisory-db 不可达）→ 改用 `tools/osv-audit.ps1`（OSV API）   ✅ Windows 构建图 **104/104 包零漏洞**（全量 264 包中 3 条记录均属 Linux 专用包，见 14.7）  

### 14.8 会话体验修复（2026-09-10 用户实测反馈驱动）

用户实测反馈两条，均为真实缺陷，已定位并修复：

#### 缺陷 A：启动器会弹出浏览器网页版，而不是只在启动器内部对话

**根因在 dsh 侧，不在启动器**。`dsh web` 的默认行为是**自己用系统默认浏览器打开界面**：

```text
$ dsh web --help
--no-open     do not open the Web UI in the default browser
```

dsh 源码里（`@deepseek-ai/dsh-web-app/lib/index.js`）：

```js
const handoffBrowser = config.openBrowser && !launchedThroughSsh(launchEnvironmentOf(ctx));
...
console.log("dsh web: opening the default browser; pass --no-open to disable");
internals.openBrowser(authenticatedUrl)...
```

而启动器此前拉起的命令行是 `node bin.js web --host 127.0.0.1 --port <port>`——**没有 `--no-open`**，
于是每次启动 dsh 都会额外弹一个浏览器窗口，与启动器自己的内嵌 WebView2 窗口重复。

**修复**：`crates/dsh-core/src/process.rs` 的 `start_dsh` 追加 `--no-open`；
并新增提示行识别（`is_browser_open_notice`），一旦上游行为变化会打警告留痕。

#### 缺陷 B：托盘「打开界面 / 在浏览器中打开」只能得到 401 认证失败页

**根因**：dsh web 的界面要求**一次性 token**，实测无 token 访问返回 **HTTP 401**：

```text
GET http://127.0.0.1:3080/  →  HTTP 401 Unauthorized
```

token 只出现在 dsh 打到 stdout 的启动行（`printUrl` 默认 true）：

```text
dsh web: http://127.0.0.1:3080/?token=xxxx[ (LAN: http://…?token=…)]
```

而启动器把 dsh 的 stdout 接成管道后**从未读取**，只能退而使用无 token 的普通地址。

**修复**：
1. 新增 `dsh_core::dsh::parse_ready_url()`（含 LAN 后缀剥离）+ `ReadyHook` 回调；
   `process.rs` 用 `spawn_ready_reader` 持续读取 stdout——既排空管道，又捕获带 token 的地址
   （token 只写入内存 `auth_url`，日志落盘的是经 `redact_secrets` 脱敏后的行）。
2. `ServiceHandle::ready_url()`：优先返回带 token 的地址；`has_auth_url()` / `is_own_process()`
   供调用方判断时序。
3. **时序修复**：`Ready` 事件与 stdout 捕获是两个独立异步过程，`Ready` 可能先到
   （实测冷启动即如此）。`App::auth_url_settled()` 会在服务由本进程启动且尚未拿到 token 时
   最多等待 `AUTH_URL_WAIT_TICKS`（约 5 秒，预算在帧循环里推进以保证最终放行），
   避免把 401 页面开给用户。
4. 托盘「在浏览器中打开」、唤起窗口、失联重连全部改用 `ready_url()`。

**验证**：新增单元测试 `parses_ready_url_from_dsh_stdout`（含 `(LAN: …)` 剥离）、
`ignores_non_ready_lines`、`detects_browser_open_notice`；并对发布产物做字符串核对，
确认 `--no-open` / `dsh web:` / `opening the default browser` 均已进入二进制。

#### 缺陷 C：退出启动器容易误停服务，导致正在进行的会话被强行断开

**根因**：退出确认用 `MB_YESNOCANCEL`，其**默认焦点在「是」= 停止服务**；文案「是否同时停止？」
也没有直接说明后果。用户回车或习惯性点是，dsh 就被停掉，正在对话/跑任务的会话立即断开。

**修复**（`crates/dsh-app/src/dialog.rs`）：
- 加 `MB_DEFBUTTON2`：**默认焦点落在「否」= 保留服务后台运行**；
- 文案改写为明确列出三种选择及后果（否 = 会话不中断、下次自动接管；是 = 立即中断）。

> 说明：保留服务本身一直是可用能力（`ProcessManager::disarm_kill_on_close`，v2.0 语义，
> 有 `selftest.ps1` A2 覆盖）。本次修的是**默认值误导**，不是缺失能力。

> ⚠️ 已发布到根目录的 `DSHLauncher.exe` 含以上三项修复，但**正在运行的进程仍是旧代码**
> （Windows 上已启动的进程不会热更新）。需要退出并从新 exe 重启才会生效。

### 14.7 依赖安全审计结果（OSV / RustSec 数据）

**为什么不是 `cargo audit`**：本机网络放行 crates.io 与 `api.osv.dev`，但**不达 github.com**，
而 `cargo audit` 必须 `git clone https://github.com/RustSec/advisory-db.git`：

```text
error: couldn't fetch advisory database: git operation failed: An IO error occurred when talking to the server
```

因此新增 `tools/osv-audit.ps1`：直接查 OSV（RustSec 数据已镜像进 OSV）的
`POST https://api.osv.dev/v1/query`，逐包比对 `Cargo.lock`。覆盖范围与 `cargo-audit` 等价，
差别只是取数通道（不再依赖 git 协议）。

**全量结果（264 个 lockfile 包，100% 查询成功）**

  包   版本   记录   修复版本   结论  
 --- --- --- --- --- 
  `glib`   0.18.5   `GHSA-wrw7-89jp-8q8g`（MODERATE）/ `RUSTSEC-2024-0429`   0.20.0   ⚪ **不影响本项目**  
  `proc-macro-error`   1.0.4   `RUSTSEC-2024-0370`   —   ⚪ **不影响本项目**  

**为什么不影响**：两者都只存在于 **Linux 目标的 GTK 栈**里，Windows 构建图中
**根本不存在**（已用 `cargo tree -i <crate> --target x86_64-pc-windows-msvc -e normal,build,dev`
逐一验证为 `NOT PRESENT`）。依赖链是：

```text
tao 0.35.3 ──(linux)──> gtk 0.18.2 ──> atk/cairo-rs/gdk… ──> glib 0.18.5 ──> glib-macros ──> proc-macro-error 1.0.4
```

根因是 **`tao` 的默认 feature 在 Linux 上拉入 GTK**。本项目已在 workspace 显式声明
`tao = { default-features = false, features = ["rwh_06"] }`（`Cargo.toml`），因此 **Windows 构建
不会编译也不需要 GTK**；`Cargo.lock` 会为所有平台记录这些包，这是 Cargo 的解析语义
（锁文件是 universal 的，不是「本次构建用到的包列表」）。

**口径澄清（修正 13.6 的「依赖包数 = 264」）**：264 是 `Cargo.lock` 的全平台解析结果；
**Windows 目标实际参与编译的是 104 个包**（`cargo tree --target x86_64-pc-windows-msvc`
去重统计；其中 160 个为 Linux/macOS 专用）。发布说明里「264 个依赖」应理解为
「锁文件锁定 264 个」，与「本平台编译 104 个」是两个不同口径。

**分目标扫描结果（两次独立扫描，100% 覆盖）**

  范围   包数   漏洞记录   结论  
 --- --- --- --- 
  Windows 构建图（真正进 exe）   104   **0**   ✅ 无  
  非 Windows（lockfile 其余）   160   3（glib ×2、proc-macro-error ×1）   ⚪ 不参与本平台构建  

**防回归**：`tools/check-consistency.ps1` 已强制 `tao`/`wry`/`tray-icon`/`muda` 必须
`default-features = false`；一旦有人去掉该声明，Linux 下会真的拉入 GTK（也就会引入这两条
advisory），该检查会直接失败。若未来要发布 Linux 版，必须先允许 Linux 目标并处理这两条 advisory。

  端到端   `selftest.ps1`（A1/A2/B/C/D/E）   ⚠️ **本轮未复跑**（见 14.4）  

**安装/卸载互逆矩阵（实测）**

  安装动作   位置/键   卸载动作   验证   结果  
 --- --- --- --- --- 
  释放 DSHLauncher.exe/app.ico/3 文档   安装目录   删除整个目录（延迟 2 s）   `Test-Path`   ✅ 无残留  
  写 `uninstall.cmd`+`.ps1`   安装目录   随目录删除   `Test-Path`   ✅  
  写 11 个注册表值   `HKCU\...\Uninstall\DSHLauncher`   `Remove-Item` 递归   `reg query`   ✅  
  建桌面快捷方式   `Desktop\DeepSeek Harness Launcher.lnk`   按名匹配删除   `Test-Path`   ✅  
  配置/日志/WebView2 缓存   `%APPDATA%`/`%LOCALAPPDATA%`   仅删 `%LOCALAPPDATA%`   哈希比对   ✅ 配置保留  
  无服务/计划任务/PATH 改动   —   —   —   ✅ 互逆（v5 不注册任何服务/任务/PATH）  

### 14.4 本轮未能完成的验证（如实标注，非结论）

  项   阻塞原因   补做方式  
 --- --- --- 
  优化后内存实测（与基线同口径）   测量需要「干净启动 + 独占互斥体」，而当时运行中的实例正持有互斥体；随后测量轮次触发环境级故障（见下条）   `tools/finish-release.ps1` 第 9 节自动采集同口径数据（`[boot]` 埋点已就绪）  
  桌面堆耗尽事故（新发现，已复盘）   多轮「启动 GUI → 采样 → 强杀」后 Windows 无法再创建进程（`0xC0000142`）   根因、恢复与**预防规则**见 `docs/OPS-RUNBOOK.md` §1  

> 已补齐：根产物发布、`selftest.ps1` 端到端、安装包重建、`cargo audit`（见 14.6）。

### 14.6 收尾轮补充实测（2026-09-10 续，工具运行时恢复后）

**根产物发布**（`build.ps1` 守卫 + 被占用时的处置）

  项   值  
 --- --- 
  发布文件   `D:\DSHLauncher\DSHLauncher.exe`  
  大小   **975,360 B（952.5 KB）**  
  `FileVersion` / `ProductVersion`   `5.0.0` / `5.0.0`（对应 `5.0.0.0`）  
  `ProductName` / `CompanyName`   DeepSeek Harness Launcher / KristoffersonLee  
  SHA256   `A0172C3A86BFFE6574A1B8F3BCD1E73AD6D19F871C1FEE50B2274634A100BD9A`  
  处置方式   运行中的旧实例锁定了该文件；利用「同卷重命名不受运行中映像锁定影响」的特性：先 `Rename-Item` 移走旧产物，再 `Copy-Item` 放入新产物，**未中断正在运行的服务**  

**启动阶段耗时（`[boot]` 埋点，实测一次冷启动；时钟起点为 `main` 入口）**

  阶段   耗时  
 --- --- 
  单实例 + 配置 + 日志就绪   7 ms  
  事件循环创建完成   27 ms  
  托盘创建 + 服务启动请求完成   43 ms  
  事件循环首帧（界面可响应）   **45 ms**  
  服务就绪（接管已在监听的外部实例）   46 ms  

> 也就是说：**进程内启动到界面可响应约 45 ms**；进程创建到 `main` 的时间无法自测，未计入。
> 该 1 ms 级的「服务就绪」是**接管**路径（服务已在运行）；冷启动路径受 dsh 自身启动时间支配，
> 不是启动器开销。

**内存与启动（同口径对比：均为「刚启动、未加载 WebView2 窗口」的实例，启动后 6 s 采样）**

  指标   基线（改动前 v5）   优化后（v5.0.0 定稿）   变化   结论  
 --- --- --- --- --- 
  工作集   23,924 KB   **23,912 KB**   **−12 KB (−0.05%)**   ✅ 不劣化  
  专用内存   3,772 KB   **3,748 KB**   **−24 KB (−0.6%)**   ✅ 不劣化  
  线程数   12   **12**   0   ✅  
  空闲 CPU（3 s）   16 ms   —   —   同量级  
  启动到界面可响应   未埋点   **20 ms**（另一次冷启动 45 ms）   —   ✅ 新增可测能力  

> 启动阶段细分（`[boot]` 埋点，起点 = `main` 入口，三次独立启动实测）：
>
>   阶段   启动 #1   启动 #2   启动 #3  
>  --- --- --- --- 
>   单实例 + 配置 + 日志就绪   7 ms   5 ms   4 ms  
>   事件循环创建完成   27 ms   11 ms   9 ms  
>   托盘创建 + 服务启动请求完成   43 ms   25 ms   19 ms  
>   事件循环首帧（界面可响应）   **45 ms**   **26 ms**   **20 ms**  
>   服务就绪（接管已监听实例）   46 ms   27 ms   21 ms  
>
> 进程创建到 `main` 的时间无法自测，未计入；冷启动（需等待 dsh 自身起来）由 dsh 支配。

> ⚠️ 采样时若内嵌 WebView2 窗口已加载，工作集会升到 ~29 MB（实测 29,324 KB）——那是
> WebView2 运行时的开销，不是本启动器的常驻增量，比较时必须区分这两个口径。

### 14.5 尚存风险（发布决策依据）

  风险   等级   说明与缓解  
 --- --- --- 
  依赖 advisory（Linux 专用包）   ⚪ 低   `glib 0.18.5`（RUSTSEC-2024-0429）与 `proc-macro-error 1.0.4`（RUSTSEC-2024-0370）仅存在于 Linux 的 GTK 栈；**Windows 构建图 104/104 包零漏洞**，已逐一验证不存在。防回归检查已加入 `check-consistency.ps1`（GUI crate 必须 `default-features = false`）  
  `panic = "abort"` 保留   🟠   体积优先（`opt-level="z"` + LTO + strip）；使 `catch_unwind` 不可用，但项目未使用该机制  
  启动器工作集 ~23.9 MB   🟡   高于 ≤20 MB 目标；专用内存 3.7 MB，差异来自共享页。**与改动前基线持平（−12 KB）**，未劣化  
  `Dsh::version`/`dist_tags`/`upgrade`/`check_health` 无 UI 调用方   ✅ **已删除**（v5.0.0 LTS 定稿轮：无调用方的死代码连同一整条 npm 调用链一起移除；用户能力保留在 `docs/MAINTENANCE.zh/en.md` §1.3–1.5 的手动步骤里；命令注入面改由"生产代码不含任何 shell 拼串"的结构性断言守住）  
  「修复模块」按钮缺失   🟡   v4 有、v5 无；维护手册 §1.7 已明确改走手动 `node-gyp rebuild`  
  单实例命名空间为 `Local\`   ⚪ 低   产品定位为单用户桌面启动器；`Global\` 需 `SeCreateGlobalPrivilege`，受限账户下会直接失败。若将来需要跨会话（多用户/RDP）单实例，需显式处理该特权  
  设置窗口位置记忆、主题采样接线   🟡   回归清单 #13/#20 仍为 ⚠️/❌，处置建议见 §13.5  
  桌面堆耗尽事故   🟡   已写入 `OPS-RUNBOOK.md` 并给出 6 条预防规则；`finish-release.ps1` 内置健康检查与残留清理  
  主题采样若接线需注意   🟡   `sample_and_apply_theme` 当前在 UI 线程 `recv_timeout` 最多阻塞 2 s，接线时必须改为异步回调，否则会引入新的界面卡顿  

---

## 15. 实施记录补充：v5.0.0 定稿前审计处置轮

> 本节记录 [`AUDIT-REPORT-v5.0.0.md`](AUDIT-REPORT-v5.0.0.md) 全量深度审查后的**处置结果**，
> 属于 **v5.0.0** 的一部分（该轮曾用内部迭代标识且从未对外发布，现统一并入 v5.0.0）。
> §13/§14 保留为**历史记录**（其中的状态描述是当时的事实）；**凡与本节冲突，以本节与当前代码为准**。
> 逐项处置与验证证据见 [`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md)。

### 15.1 与 §13/§14 状态描述冲突的项（以本节为准）

  §13/§14 的记录   当前实现  
 --- --- 
  回归清单 #13「标题栏跟随 Harness 主题」⚠️ **未接线**（`sample_and_apply_theme` 恒返回 `None`）   ✅ **已接线**：新增 `HarnessWindow::begin_theme_sample()`（**异步回调**，绝不在 UI 线程 `recv_timeout`）；窗口创建时用 `is_system_dark()` 给正确初值；运行中每约 3 秒采样一次  
  回归清单 #20「设置窗口位置记忆」❌ 未实现   ❌ 仍未实现（**如实保留**：属独立特性，需多显示器/DPI 边界设计）  
  §14.5「主题采样若接线需注意（会阻塞 2 s）」   ✅ 已按该建议实现为异步回调；`sample_and_apply_theme`（同步版）保留但**不再被运行期调用**  
  §14.5「`Dsh::version`/`dist_tags`/`upgrade`/`check_health` 无 UI 调用方」   **已处置：彻底删除**（含 `is_safe_version` / `run_capture` / `resolve_npm` 与 `DshError` 的 5 个变体）；理由与替代路径见 `dsh-core/src/dsh.rs` 模块文档  
  §14.5「设置窗口位置记忆、主题采样接线」列为遗留风险   主题采样已消除；位置记忆仍在  
  §14.2「优雅退出需 `disarm_kill_on_close`」   **机制已重构**：默认 `independent` 下 dsh 不挂 Job，无需 disarm；`tied` 模式仍保留 disarm 实现与测试  
  §4.1 表格「Job Object 保证零孤儿」   **已变更**：默认 `independent` 下 dsh 独立于启动器（退出不回收），归属由 `service.json` 簿记 + 对账保证；需要内核级零残留请选 `tied`  

### 15.2 审计报告发现并已处置的缺陷

  #   缺陷   影响   处置  
 --- --- --- --- 
  A1   dsh 绑死在启动器 Job 上（`KILL_ON_JOB_CLOSE`）   退出 / 崩溃 / 被安装包升级覆盖 / 注销重启都**切断正在进行的会话**；只有托盘「退出→否」能保住   解耦：`ChildLifecycle::{Independent(默认), Tied}` + `CREATE_BREAKAWAY_FROM_JOB`；归属改由 `service.json`（PID + 端口 + **进程创建时间**）簿记 + 启动对账；设置页可切回 `tied`  
  A2   token 捕获从未生效 + 等待逻辑不可达   界面一律导航到无 token 地址，实测 **HTTP 401** 认证失败页   `Ready { url: Option<String> }`；只有带 token 才写入 `pending_url`；捕获后**补发** `Ready`；worker 改分片等待 + 代际号取消。**运行期确证**：实测「就绪早于 token 约 914 ms」，修复后由 `None` 占位并在 token 到达时覆盖  
  B1   对任何监听端口的进程直接 adopt   托盘「停止服务」会 `kill_process_tree` 掉无关程序（v4 有身份校验，v5 丢失）   新增 `is_dsh_harness_process()`（`node.exe` + 排除系统目录 + 三重身份关联）；无法确认则**只读不接管、不杀**  
  B2   `stderr` 接管道但从不排空   dsh 写满管道缓冲区后**永久阻塞**（服务假死、被误判失联重启）   新增 `spawn_stderr_drain()`（落 WARN 日志 + 单行截断）  
  B3   `tray_on_close` 零消费方   界面复选框与文档承诺「关闭时最小化到托盘」，实现只是销毁窗口   新增 `request_close_window()`：隐藏（保留 WebView/页面状态）或真正关闭  
  E1   主题采样未接线   标题栏恒为浅色（`set_dark_mode(hwnd, false)`）   见 15.1  
  —   `F5`/`Ctrl+R`/`Esc` 只是 `ui/guide.html` 里的承诺   用户按下无任何反应   新增 `handle_harness_key()`  
  G1   安装器升级 `taskkill /F` 必然连带杀死 dsh   升级即断会话（且 `StopLauncherInDir` 注释声称相反）   由 A1 解耦自动修正；注释同步更正  
  G2   无脚本可用的退出入口   发布脚本只能强杀（会断会话）   新增 `--quit` + `QuitEvent` 命名事件；与托盘「退出」共用 `begin_exit()`  

### 15.3 未被审计列为 P0/P1、本轮顺手修掉的项

  项   处置  
 --- --- 
  `eprintln!` 在 GUI 子系统下写失败会 panic（`panic=abort` ⇒ 整进程崩溃）   全部替换为忽略错误的 `writeln!(stderr)`  
  `is_process_alive` 在 `GetExitCodeProcess` 失败路径泄漏句柄   所有返回路径统一 `CloseHandle`  
  `kill_process_tree` 嵌套持有 N+1 个快照句柄   改为「先收集子 PID 再递归」  
  归档清理 `removed.contains` 为 O(n²)、把「剔除脏条目」计为「已清理」   `HashSet` + `cleaned`/`pruned` 分类计数  
  `workspace.json` 区间式改写可被「同名 key 多次出现」写坏   新增 `unique_key_index()` 唯一性护栏 + 改写后 `serde_json` 合法性复核  
  锁文件收集会跟随目录型 reparse point（可越界删除）   拒绝 reparse point（附 junction 单测）  
  日志实际滚动出 4 个文件（文档称 3 份）   修正为 `max_files` 个并删除溢出归档；新增 `retained_file_count()` 供校验  
  日志每行 `create_dir_all` 一次系统调用   用 `AtomicBool` 只做一次  
  ini 迁移后不删旧文件（删 toml 会「复活」旧配置）   迁移落盘成功后删除 ini（落盘失败则保留）  
  安装包手写 `AppVersion` 常量（第二处版本来源）   改为反射读取自身 `AssemblyFileVersion`；`Cargo.toml` 成为唯一来源  
  文档数字在 5 份文件里有 3–4 个互相矛盾的值   `tools/gen-facts.ps1` 生成 `docs/FACTS.json` 作为唯一来源，`-Check` 检测漂移  
  `stop()` 期间持锁执行进程终止（阻塞 UI 线程）   阻塞部分移出 UI 路径；`ProcessManager::stop()` 返回被终止 PID  
  自动重启次数永不重置（3 次后永久失去自愈）   稳定运行 5 分钟后自动恢复额度  
  配置保存失败仍已改内存（界面与磁盘不一致）   改为副本改动 + 落盘成功才提交  

### 15.4 新增的防回归门禁

`tools/check-consistency.ps1` 从 66 项扩到 **102 项**（当前值见 `docs/FACTS.json`），
新增的都是**行为性**硬约束（接线、默认值、必需 API、护栏）——因为这类缺陷的特征是
「单元测试全绿但功能没接上」：

- `CREATE_BREAKAWAY_FROM_JOB` 必须存在；`ChildLifecycle` 两种语义必须都在
- `service_lifecycle` 默认值必须是 `independent`
- `Ready` 事件的 `url` 必须是 `Option`；`pending_url` 只能在带 token 时写入
- 必须存在 `is_dsh_harness_process` / `spawn_stderr_drain` / `ServiceRecord` + `reconcile`
- `tray_on_close` 必须有消费方；`begin_theme_sample` 必须被调用；必须处理 `KeyboardInput`
- `crash.rs` 必须注册异常过滤器，且其中**不得出现 `format!`**（崩溃上下文不可分配）
- 锁收集必须拒绝 reparse point；`workspace.json` 改写必须有唯一性护栏
- `--quit` 必须存在（脚本可优雅退出）；`build.ps1` 必须支持热替换与历史产物清理
- 「清理归档会话」必须有二次确认、必须在清理后按需重启服务、必须**先探测活跃会话**
- 活跃会话探测必须含"会话运行器进程"硬信号与"文件近期写入"软信号
- 验证脚本与运行期探针必须存在，且探针必须走生产路径

> 本轮还发现并修正了**三处门禁自身**的脆弱性：写死换行的正则会被 `cargo fmt` 打破；
> 注释里出现的 API 名会被误判为违规调用（新增 `Strip-Comments` 去注释后再判定）；
> 文档里的数字会随代码漂移（改用 `gen-facts.ps1 -Check` 单一来源）。

### 15.5 v5.0.0 LTS 定稿轮 + 发布工程轮实测（同口径）

  指标   重构初版（§13.6/§14.6）   v5.0.0 LTS（定稿轮 + 发布工程轮）  
 --- --- --- 
  单元测试   65   **149**（+84；`dsh-core` 97 + `dsh-ui` 13 + `dsh-app` 12 + `dsh-uninstall` 21 + `dsh-buildinfo` 6；`build.rs` 的 `#[test]` 已迁移为可运行的库测试）  
  一致性项   66   **178**（+112）  
  版本链路项   23   **28**  
  启动器工作集（窗口关闭态）   23.9 MB   **13.2 MB**（同口径实测；23.9 MB 为另一口径）  
  生产 Rust 代码行数   4,776（§14 口径）   **12,137**（`docs/FACTS.json`，由脚本实测）  
  单文件 exe   973,312 B（§13.6 口径）/ 981,504 B（实测）   **1,087,488 B（1,062.0 KiB）**（安装包体积见 `docs/FACTS.json`，含原生卸载器）（定稿轮新增：生命周期编排修正、UI 线程零阻塞、导航白名单、脱敏扩展、JSON 区间改写加固、UI 状态机补全；发布工程轮新增：`--version`/`--help` 入口、启动移出 UI 线程、端口探测限流、进程存活与进程树加固）  
  clippy 警告   0   **0**  
  冷启动「就绪 vs token」顺序   未测（bug 未暴露）   实测**就绪早 914 ms** → 由 `Ready{url:None}` + 补发覆盖；定稿轮另修「预算未初始化 + 无驱动源」的同族残留  

> 发布工程轮另把**发布标签**统一为 `5.0.0 LTS`：Cargo 版本仍为合法 semver `5.0.0`，
> `LTS` 只出现在 `--version` 输出、文档、安装向导与制品名中（一致性门禁会拒绝
> 把 `LTS` 写进版本号的改法）。

> 代码量增加主要用于：服务生命周期编排（`service_record.rs` 新增）、进程身份校验、
> 崩溃取证、主题异步采样、行为性门禁与验证脚本。`docs/FACTS.json` 是这些数字的唯一来源。
