# DSHLauncher 5.0.0 LTS — 发布工程轮验证记录

> 本文档记录 **5.0.0 LTS 发布工程轮**在本机（Windows，开发机）实际执行的验证命令、结果与
> 未能执行项的原因。它与 [`AUDIT-REPORT-v5.0.0.md`](AUDIT-REPORT-v5.0.0.md)、
> [`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md) 同属工程档案：
> 前者是"定稿轮"的问题清单与处置，本文是"发布工程轮"的**发布验证证据**。
>
> 权威数字（单元测试数、代码行数、产物体积、一致性项数）来自
> [`FACTS.json`](FACTS.json)，由 `tools/gen-facts.ps1` 实测生成。

## 1. 环境

| 项 | 值 |
|---|---|
| 平台 | Windows（本机开发环境） |
| Rust | `cargo 1.98.1 (797e8a9bc 2026-08-05)` / `rustc 1.98.1`，host = `x86_64-pc-windows-msvc` |
| 已安装 target | 仅 `x86_64-pc-windows-msvc` |
| 网络 | **离线**（`cargo audit` 无法拉取 advisory-db；`cargo install` 不可用） |
| 已安装 cargo 工具 | `cargo-clippy` / `cargo-fmt` / `cargo-audit`（DB 不可达） |
| 未安装且离线无法安装 | `cargo-deny`、`cargo-machete`、`cargo-udeps` |
| 版本（唯一来源） | workspace `Cargo.toml` → `5.0.0`；发布标签 `LTS`（`crates/dsh-app/src/cli.rs::RELEASE_CHANNEL`） |

## 2. 静态与单元测试

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | **通过**（exit 0，无 diff） |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | **通过**（0 警告） |
| `cargo test --workspace --all-features --offline` | **通过**：`dsh-app` 6 + `dsh-core` 92 + `dsh-ui` 11 = **109 passed / 0 failed** |
| `cargo build --release --locked --offline` | **通过**（exit 0） |
| `cargo check --workspace --all-targets --offline` | **通过** |

## 3. 版本链路与一致性门禁

| 命令 | 结果 |
|---|---|
| `pwsh -File tools/verify-version.ps1 -Exe target\release\dsh-app.exe` | **28 passed / 0 skipped / 0 failed** |
| `pwsh -File tools/verify-version.ps1 -Exe .\DSHLauncher.exe -RequireInstaller` | **通过**（含安装包产物与图标资源断言） |
| `pwsh -File tools/check-consistency.ps1` | `CHECK_SUMMARY passed=129 failed=0 total=129` · **全部 129 项通过** |
| `pwsh -File tools/gen-facts.ps1 -Check` | **FACTS.json 与实测一致 OK（已比对 29 项事实）** |

本轮新增的一致性硬约束（8 → 11 条，总项数 115 → 129）：

1. 提供 `--version` / `--help`（安装包与 CI 可自动核对版本）；
2. 发布通道标签（LTS）与 semver 版本分离（拒绝把 `LTS` 写进 Cargo 版本）；
3. 启动服务走 worker 线程（`spawn_start`），UI 线程不执行阻塞启动；
4. 端口可达性探测集中限流（`READY_PROBE_INTERVAL`，`is_port_listening` 单点调用）；
5. 外部命令探测限时（`run_capture_within` + 超时即杀进程树，生产代码不含无超时 `output()`）；
6. 进程存活判定基于 `WaitForSingleObject`（不受退出码 259 哨兵歧义影响）；
7. 进程树终止为迭代 + 去重（不递归）；
8. `service.json` 位于 `%LOCALAPPDATA%`（卸载可清理）+ 旧位置一次性清理；
9. 卸载模板支持 `-DryRun` / `--dry-run`（只报告影响范围）；
10. dry-run 分支不含任何状态变更调用（`Remove-Item` / `taskkill` / `Set-ItemProperty` / `New-Item`）；
11. 生成的卸载器同样支持 `--dry-run`；另加**离线版 `cargo machete` 等价检查**（三个 crate 的直接依赖都必须被本 crate 源码引用）。

## 4. 命令行与启动冒烟（真实产物）

| 命令 | 结果 |
|---|---|
| `DSHLauncher.exe --version` | `DSHLauncher 5.0.0 LTS (release)`，exit 0 |
| `DSHLauncher.exe -V` | 同上（短选项） |
| `DSHLauncher.exe --help` | 打印用法 + 退出码契约，exit 0 |
| `DSHLauncher.exe --build-info` | `version=5.0.0` + **16** 个修复标记；同时写 `%APPDATA%\DSHLauncher\build-info.txt` |
| `DSHLauncher.exe --probe-identity 15472` | `verdict=IsDsh`（依据：映像路径 == 已解析的 dsh node.exe） |
| `DSHLauncher.exe --probe-identity 4` | `verdict=NotDsh`（镜像名 `System`），exit 0 |
| `DSHLauncher.exe --selftest` | **exit 0**；日志：`测试端口: 17497` → `PASS: 服务已就绪（端口 17497）` → `==== 自检通过 ====`；结束后端口无残留、进程已回收 |
| `DSHLauncher.exe --ipc-probe` | **exit 0**；日志出现 `设置页命令: Save` → `设置已保存（端口 3080）` → `[探针] 结束` |
| 启动（GUI，接管既有服务） | `[boot] 事件循环首帧（界面可响应）: 16ms`、`[boot] 服务就绪: 469ms`；日志：`端口 3080 上已有 dsh 服务在运行（PID 15472），直接接管。` |
| `DSHLauncher.exe --quit` | `outcome=Acknowledged`，**exit 0**；启动器退出后既有 dsh 会话（PID 15472）**仍然存活**（`independent` 语义） |

> 冷启动路径上，服务启动请求现在是**提交给后台线程**（`[boot] 服务启动请求已提交（后台线程）: 16ms`），
> 因此"界面可响应"不再被对账 / 端口探测 / 锁清理 / spawn 拖延。

## 5. 安装包 / 卸载包一致性（真实安装 + dry-run + 真实卸载）

安装用 `DSHLauncherSetup.exe --silent-install`（默认目录 `%LOCALAPPDATA%\Programs\DSHLauncher`）。

### 5.1 安装后逐项核对（exit code 0）

| 检查项 | 结果 |
|---|---|
| 落盘文件 | `DSHLauncher.exe` / `app.ico` / `README.md` / `MAINTENANCE.zh.md` / `MAINTENANCE.en.md` / `uninstall.cmd` / `uninstall.ps1` + 标记文件 `.dsllauncher-install`（含 `version=5.0.0.0 LTS`） |
| 注册表卸载项 | `HKCU\...\Uninstall\DSHLauncher`：`DisplayName=DeepSeek Harness Launcher`、`DisplayVersion=5.0.0.0`、`Publisher=KristoffersonLee`、`InstallLocation` 正确、`UninstallString` / `QuietUninstallString`（后者带 `--silent`）、`DisplayIcon` 指向 exe |
| 桌面快捷方式 | `DeepSeek Harness Launcher.lnk` 已创建 |
| 安装副本版本 | `FileVersion=5.0.0`、`ProductVersion=5.0.0`、`ProductName=DeepSeek Harness Launcher` |
| 安装副本 `--version` | `DSHLauncher 5.0.0 LTS (release)` |
| `uninstall.cmd` | 纯 ASCII（`nonAscii=0`）、全 CRLF（`CRLF=6 bareLF=0`） |

### 5.2 卸载 dry-run（原生卸载器 `dsh-uninstall.exe --dry-run`）

`dsh-uninstall.exe --dry-run`（亦接受 `/whatif`）在**不修改任何状态**的前提下逐项报告影响范围：

```text
DRY-RUN：卸载影响范围（不会修改任何内容）
  安装目录            : C:\Users\...\AppData\Local\Programs\DSHLauncher
  安装标记            : ...\.dsllauncher-install（已具备）
  将删除的安装文件    : 14 个（其中存在 5 个）
      （正在运行的 dsh-uninstall.exe 由后台副本在第二阶段删除）
  注册表卸载项        : HKCU\...\Uninstall\DSHLauncher → 存在，将被删除
  桌面快捷方式        : 1 个将被删除
  运行期数据          : C:\Users\...\AppData\Local\DSHLauncher → 存在，将被删除（日志 / WebView2 profile / service.json）
  用户配置            : C:\Users\...\AppData\Roaming\DSHLauncher → **保留**（默认）
  安装目录本身        : 递归删除（由后台副本执行，约 1 秒后）
DRY-RUN：以上为完整影响范围；未改动任何内容，也未校验卸载后置条件。
```

执行前后核对：安装目录 7 项、注册表项、桌面快捷方式**全部原样存在**（零改动）。
源码树护栏同样实测：在仓库根运行 `dsh-uninstall.exe` 会被拒绝并给出退出码 **2**。

### 5.3 真实卸载（`dsh-uninstall.exe --silent`，exit 0）与零残留核对

| 检查项 | 期望 | 实测 |
|---|---|---|
| 安装目录 | 删除 | **已删除**（由 `%TEMP%` 副本完成，约 1 秒） |
| 注册表卸载项 | 删除 | **已删除** |
| 桌面快捷方式 | 删除 | **已删除** |
| `%LOCALAPPDATA%\DSHLauncher`（日志 / WebView2 profile / service.json） | 删除 | **已删除** |
| `%APPDATA%\DSHLauncher\settings.toml`（用户配置） | **保留** | **保留** |
| `%TEMP%` 中的卸载器副本 | 由系统在下次重启时清理 | 1 份（约 300 KB）；本次卸载顺手清掉了历史遗留副本 |

> Windows 不允许运行中的镜像删除自己（实测对自身取 `DELETE` 权限被拒），因此删除安装目录由
> "复制自身到 `%TEMP%` 后重入"的副本执行；该副本只能由系统在**下次重启时**清理。
> 这是平台限制而非遗漏，已写入 README 与 `--help`。

### 5.4 安装 → 卸载 → 再安装（含升级路径）

卸载验证完成后**重新安装**并再次核对：文件、注册表项、快捷方式、`--version` 全部正确。
另外验证了**升级路径**：第二次安装时安装器按 `ObsoletePayloads` 清除了上一版遗留的
`uninstall.cmd` / `uninstall.ps1`
（实测升级前目录里同时存在两套卸载器，升级后只剩原生 `dsh-uninstall.exe`）。

> ⚠️ **后续状态更新**：本节记录的是**当时**的状态。此后安装目录
> `%LOCALAPPDATA%\Programs\DSHLauncher` 被删除，而 `HKCU\...\Uninstall\DSHLauncher`
> 与桌面快捷方式仍然存在，且都指向不存在的文件 —— 即"**悬空残留**"。因此"当前机器处于
> 已安装 5.0.0 LTS 状态"这句结论**不再成立**；该状态正是 `--clean-residue` 的适用场景，
> 已在第四轮实测清理（见 §10）。

### 5.5 本轮验证抓出的两个真实缺陷（只有真机跑才会暴露）

| # | 现象 | 根因 | 修复 |
|---|---|---|---|
| 1 | dry-run 报告"注册表卸载项 → **不存在**"，而键其实就在那里 | 用 `RegGetValueW(HKCU, subkey, 值名=null, …)` 去查**键的默认值**；我们的键**没有默认值** ⇒ 查不到 ⇒ 误判为"键不存在"（`remove_uninstall_key` 的后置校验也会跟着误报） | 改用 `RegOpenKeyExW` 查**键**本身 |
| 2 | 真实卸载后**安装目录仍然存在**（其余都干净） | 阶段二把"参数给的目标目录"与"本程序所在目录"做相等校验 —— 但阶段二的副本住在 `%TEMP%`，两者**永远不等** ⇒ 直接拒绝执行；真正该拦的是"目标 = 本程序所在目录"（那等于让副本递归删掉 `%TEMP%` 自己所在的目录） | 阶段二以参数为目标目录；护栏改为"目标 ≠ 自身所在目录"，并加"标记内容必须属于本应用" |

> 这两条都是**静态审查与单元测试都发现不了**的：前者是 Win32 语义细节，后者是跨阶段的路径语义。
> 它们同时也是"卸载器不该只靠一次 dry-run 就宣称可用"的证据 —— 本轮因此把
> 安装 → dry-run → 真实卸载 → 零残留 → 重装 → 升级清理 全部跑了一遍。

### 5.6 卸载器版本资源与注册表链路（`verify-version.ps1`，33 项全通过）

`dsh-uninstall.exe` 与启动器同样带完整版本资源，并由 `tools/verify-version.ps1` 校验：
`FileVersion = 5.0.0`、`ProductVersion = 5.0.0`、`OriginalFilename = dsh-uninstall.exe`、
`ProductName = DeepSeek Harness Launcher`、含图标资源；同时校验安装包**内嵌了它**、
注册表 `UninstallString` **指向它**，以及仓库里的脚本卸载器**已被删除**（单一实现）。
## 6. 用户实测反馈轮（第二轮）：两个真实故障与一处死代码

本节记录用户在**已安装 5.0.0 LTS 副本**上实测反馈的两条故障，以及随之清掉的一处死代码。
两条故障都已修复并用**真实产物**端到端验证。

### 6.1 故障一：服务启动路径自死锁（P0，本轮定位并修复）

**现象**：在**端口上没有服务**的情况下启动/重启服务（全新环境、手动停服后重启、换端口），
服务永远不会起来；界面（托盘/窗口）却完全正常。

**定位过程（可复现）**：把配置端口临时改成 `17499`（该端口无服务，走 spawn 分支）后启动，
日志停在：

```text
[INFO] [start] 服务对账: 9ms
[INFO] [start] 端口 17499 探测（is_port_listening=false）: 299ms
[INFO] [start] 解析 dsh 路径（node=C:\Program Files\nodejs\node.exe）: 0ms
[INFO] [start] 清理遗留锁（命中 0 个）: 8ms
（此后 5 分钟无任何日志；进程存活、UI 线程仍在消息循环）
```

**根因**：`ServiceHandle::start()` 在 spawn 之前写了
`let pm = { let g = self.inner.lock()…; Arc::clone(&g.process) };`，而函数开头取得的
`inner` 守卫**此刻仍然存活** —— `std::sync::Mutex` 不可重入，同一线程第二次 `lock()`
**永久阻塞**。更严重的是死锁发生在**持有 `ServiceInner` 锁**的状态下，因此后续任何服务操作
（`state()` / `set_config()` / `stop()`）都会一起卡死。

**为什么长期没暴露**：正常使用中端口上总有上一次启动器留下的 dsh，`start()` 走"接管"分支
就提前 return 了；只有"端口上没有服务"才会走到该分支，而那条路径没有测试覆盖。

**修复**：改为从句柄直接取 `ProcessManager`（`Arc::clone(&inner.process)`，不再二次加锁），
并新增**启动阶段计时埋点**（`[start] 服务对账 / 端口探测 / 解析 dsh 路径 / 清理遗留锁 / spawn 子进程`）
——正是这组埋点把故障定位到"清理遗留锁之后无日志"。同时加了两道防线：
`tools/check-consistency.ps1` 断言 `start()` 内 `self.inner.lock()` **恰好 2 次**；
以及可选的真实 spawn 回归测试 `service::tests::start_without_existing_service_returns_within_budget`
（`DSH_TEST_SPAWN=1` 开启，默认跳过）。

**修复后实测（同一隔离端口 17499，真实产物）**：

```text
[INFO] [start] 服务对账: 0ms
[INFO] [start] 端口 17499 探测（is_port_listening=false）: 317ms
[INFO] [start] 解析 dsh 路径（node=C:\Program Files\nodejs\node.exe）: 0ms
[INFO] [start] 清理遗留锁（命中 0 个）: 7ms
[INFO] [start] spawn 子进程: 12ms
[INFO] dsh web 已启动（PID 17104，生命周期 independent（服务独立于启动器））
[INFO] 服务就绪 ✓（地址 token 已脱敏：http://127.0.0.1:17499/）      ← 7483ms
[INFO] 已捕获 dsh 就绪地址（token 已脱敏）：dsh web: http://127.0.0.1:17499/?token=***
[INFO] 已打开内嵌界面：http://127.0.0.1:17499/?token=***
[INFO] 内嵌界面认证正常（带 token 地址导航，或本机已有有效会话 cookie）。   ← 认证自检通过
[INFO] 标题栏已跟随页面主题（rgb 21,21,23）
```

端口 17499 已监听、界面已用带 token 地址打开并通过认证自检；测试结束后已优雅退出并
停掉该测试实例、恢复用户配置（端口 3080，该服务未受任何影响）。

### 6.2 故障二：内嵌界面显示 `dsh web authentication required`

**现象**：打开启动器，内嵌窗口显示
`dsh web authentication required; reopen the URL printed by dsh web.`，而不是 Harness 界面。

**根因（读上游源码确认，`@deepseek-ai/dsh-client-connection` 的 `BrowserAuth`）**：
`dsh web` 的浏览器认证只有两条路：
1. 启动时打印的**一次性 launch token**（`GET /?token=…` → 303 并种下会话 cookie）——
   它是 `processLaunchToken` 的 `WeakMap`，**只存在于 dsh 进程内存**，接管"别处启动"的服务
   时启动器**读不到**；
2. **已种下的签名 cookie**（`dsh-auth-<sha256(authority)>`，默认 30 天）。

而旧实现在"接管外部实例"时会把**不含 token 的普通地址**当作正常界面打开 ——
只要 WebView2 profile 里没有可用 cookie，用户看到的就是 401 提示页，且启动器**毫无觉察**。

**触发条件**：本次会话前恰好清理过 `%LOCALAPPDATA%\DSHLauncher`（卸载验证会按设计删除
WebView2 profile），cookie 随之丢失；同时端口 3080 上的 dsh 是本启动器之外启动的。

**修复（自愈）**：
- 窗口打开后、以及**每次导航后**，在**页面上下文**里自检是否为 401 提示页
  （`AUTH_CHECK_JS` → `AuthState::{Ok, Required, Unknown}`；裸 socket 探测区分不了
  "WebView2 已带 cookie"的情况，因此必须在页面里判断）；
- 命中 `Required` 时：在 worker 线程探测活跃会话，然后
  **无活跃会话 → 自动重启服务**；**有活跃会话 → 明确询问**（默认「是」）。
  重启由启动器自己拉起 dsh ⇒ 能读到带 token 的地址 ⇒ 打开界面并种下 cookie（此后即使再次
  接管外部实例也能正常打开）。

**验证**：见 §6.1 的最后四行日志 —— 带 token 地址导航后自检输出
`内嵌界面认证正常`。认证自检的解析逻辑有单测覆盖（`dsh-ui`），自愈决策有单测覆盖
（`dsh-app`：**只有"确无活跃会话"才允许不询问就重启**），接线由一致性门禁强制。

### 6.3 死代码清除：`Dsh::{version, dist_tags, upgrade, check_health}`

**问题**：这四个公开方法自 v5 重写起**没有任何调用方**，一直以"公开库 API，不进调用图所以
不增加体积"为由保留。它们实际带着一整条 npm 调用链（`is_safe_version`、`run_capture` 的
超时 + 进程树回收、`resolve_npm`、JSON 解析）与 5 个专用错误变体。

**处置**：**彻底删除**（含上述辅助函数与错误变体）。用户能力不丢——升级与修复原生模块的
手动步骤完整保留在 [`MAINTENANCE.zh.md`](MAINTENANCE.zh.md) §1.3–1.5；
`is_safe_version` 原本承担的安全属性改由**结构**保证，并由门禁强制：
`dsh-core` 生产代码**不含任何 `cmd.exe` / shell 拼串调用**，所有进程调用一律参数数组。
门禁同时新增"死代码 API 不得复活"断言。

## 7. 5.0.0 LTS 一致性核对表

| 位置 | 期望 | 实测 |
|---|---|---|
| `Cargo.toml` `[workspace.package] version` | `5.0.0` | ✅ `5.0.0` |
| 三个成员的 `version.workspace` | `true` | ✅ |
| `Cargo.lock`（三个包） | `5.0.0` | ✅ |
| `DSHLauncher.exe` FileVersion / ProductVersion | `5.0.0` / `5.0.0` | ✅ |
| `DSHLauncherSetup.exe` FileVersion | `5.0.0.0` | ✅ |
| 注册表 `DisplayVersion` | `5.0.0.0`（单一来源 `AppVersion`） | ✅ |
| 安装标记 `.dsllauncher-install` | `version=5.0.0.0 LTS` | ✅ |
| `--version` 输出 | `DSHLauncher 5.0.0 LTS (release)` | ✅ |
| `--help` 输出 | 含版本 + 发布标签 + 退出码契约 | ✅ |
| 「关于」对话框 | `DeepSeek Harness Launcher v5.0.0 LTS（LTS 长期支持版）` | ✅（代码路径，见 `dialog.rs`） |
| 安装向导标题 | `… 一键安装包 v5.0.0 LTS` | ✅（`DSHLauncherSetup.cs`） |
| README（中/英） | 声明 `5.0.0` 且带 LTS 发布标签 | ✅ |
| CHANGELOG（中/英） | `## v5.0.0` 段落 + LTS 发布标签行 | ✅ |
| `docs/RELEASE_NOTES_v5.0.0.md` | 标题 `… v5.0.0 LTS` + 发布标签说明 | ✅ |
| `docs/MAINTENANCE.zh/en.md` | 提及 `v5.0.0`（LTS）与 `--version` 核对方式 | ✅ |
| `docs/TECHNICAL-ROADMAP.md` §15.5 | 当前指标与 `FACTS.json` 一致 | ✅ |
| `tools/check-consistency.ps1` | 版本取自 `Cargo.toml` 单一来源，无第二次硬编码 | ✅ |
| `tools/finish-release.ps1` | 报告标题/版本标签**从 Cargo.toml 与 cli.rs 解析**，不再手写 | ✅ |
| Cargo 版本合法性 | 不含 `LTS`（避免 `5.0.0-LTS` 被当作预发布语义） | ✅ |

> 制品名策略：仓库内固定名（`DSHLauncher.exe` / `DSHLauncherSetup.exe`）保持不变，
> 由 `--version`、`--build-info`、安装向导标题、`setup.log` 与收尾报告标题承载 `LTS` 标签；
> 这样既不破坏既有的安装/卸载/热替换路径约定，也不会把标签混进 semver。

---

## 9. 最终轮（第三轮深度审查）验证记录

本轮（2026-09-11）在 5.0.0 LTS 发布后做了一次无禁区深度审查，详细处置见
[`IMPLEMENTATION-v5.0.0-lts-final.md`](IMPLEMENTATION-v5.0.0-lts-final.md)。以下是**本轮**的验证证据。

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | **通过**（exit 0） |
| `cargo clippy --all-targets --all-features -- -D warnings` | **通过**（0 警告） |
| `cargo test --all-features` | **145 passed / 0 failed**：`dsh-core` 97 · `dsh-ui` 13 · `dsh-app` 12 · `dsh-uninstall` 17 · `dsh-buildinfo` 6 |
| `cargo build --release --locked --offline` | **通过** |
| `pwsh -File tools/check-consistency.ps1` | `CHECK_SUMMARY passed=165 failed=0 total=165` |
| `powershell -File tools/check-consistency.ps1`（**PS 5.1**） | `passed=165 failed=0 total=165`（与 PS 7 同结果；修复前为 35 项假失败） |
| `pwsh -File tools/gen-facts.ps1 -Check` | **FACTS.json 与实测一致 OK（已比对 31 项事实）** |
| `pwsh -File tools/verify-version.ps1 -Exe .\DSHLauncher.exe -RequireInstaller` | **33 passed / 0 skipped / 0 failed** |
| `pwsh -File tools/osv-audit.ps1`（`cargo audit` 的离线替代通道） | 266 个包全查；3 条 advisory 全部 **NOT-IN-GRAPH**（仅存在于非 Windows 依赖图）⇒ Windows 构建图内 0 漏洞 |
| `powershell -File build.ps1 release`（README 承诺的入口） | **成功**（修复 BOM 前该命令直接解析失败） |
| `powershell -File build-setup.ps1` | **成功**：安装包重建并内嵌新版启动器/卸载器 |
| `DSHLauncher.exe --version` | `DSHLauncher 5.0.0 LTS (release)` |
| `DSHLauncher.exe --help` | 正常（含版本、发布标签与退出码契约） |
| `DSHLauncher.exe --build-info` | `version=5.0.0` + 全部修复标记 |
| `dsh-uninstall.exe --dry-run` | 完整影响范围报告，**零改动** |
| `pwsh -File tools/verify-install-layout-dryrun.ps1`（**模拟安装目录**，本轮新增） | **PASS**：准入通过，6 项载荷 + `.tmp` 暂存残留 + 上一版遗留脚本全部被列出，目录内容零改动（修复前该场景为 exit 2「拒绝执行任何删除」——见最终轮实施记录 §2.1 的 P0） |
| `pwsh -File tools/clean.ps1`（只报告） / `-WhatIf` | 四区域报告可读；`-WhatIf` 组合（`-Cache -Repo -Temp`）实测**零改动**（逐项复核文件仍在） |
| `pwsh -File tools/clean.ps1 -Repo -Temp` | 清掉 6 项历史残留（含早期轮次的 300 KB 探针与 `setup.log`），回收 0.3 MB |
| `pwsh -File tools/clean.ps1 -Cache` | 回收 3.1 MB，保留 release 交付物 4.5 MB；**清完 `FACTS` 自校验通过**，且 `check-consistency`（160/160）/ `gen-facts -Check` / `verify-version`（33/0/0）/ 模拟安装 dry-run / `--version` 全部仍绿 |
| `dsh-uninstall.exe --help` | 用法与退出码契约正常 |


### 9.2 构建目录布局（Cargo build-dir v2 / CFT）验证

`tools/verify-build-layout.ps1 -Run -IncludeNightly` 的实测输出（本机，三次 release 全量构建）：

| 配置 | 证据 | 结果 |
|---|---|---|
| 默认布局 | `cargo build --release --locked` 通过；`target\release\dsh-app.exe` / `dsh-uninstall.exe` 存在；版本链路 **33/0/0** | ✅ |
| `CARGO_BUILD_BUILD_DIR=<root>\build-layout-probe` | 构建通过；**1086 个中间产物**落到该目录；最终产物仍在 `target\release\`；`clean.ps1 -Cache` 计划包含该目录 | ✅ |
| nightly `cargo 1.100.0-nightly (2026-09-04)`（新布局默认开启） | 构建通过；`target\release` 顶层出现 `build`（按包名分桶）+ 保留旧布局目录；最终产物路径不变 | ✅ |

静态断言（门禁同款）：脚本未硬编码 `deps`/`.fingerprint`/中间 `examples` ✅；`selftest.ps1` 的 example
路径来自 cargo 报告 ✅；`.gitignore` 覆盖被搬迁的构建目录 ✅；`clean.ps1` 解析生效构建目录 ✅。

> 结论：本仓库只依赖 CFT 承诺不变的「最终产物在 `target/<profile>/` 内的布局」，
> 三种配置下最终产物路径与版本链路完全一致；工具链不依赖未承诺的内部布局。

### 9.1 未执行项（与原因）

| 校验 | 原因 |
|---|---|
| `cargo audit` | 需 `git clone https://github.com/RustSec/advisory-db`，本环境无法访问 GitHub；改用同源数据的 OSV 通道 |
| `cargo deny` / `cargo machete` / `cargo udeps` | 工具未安装且环境离线；`check-consistency.ps1` 内置等价的离线依赖使用检查（5 个 crate 全绿） |
| `cargo check --target <其它平台>` | 项目仅支持 Windows，本机也只装了 `x86_64-pc-windows-msvc` |
| `--selftest` 端到端 | 本机已有启动器实例持有单实例互斥体；按设计此时 `--selftest` 返回 **2**（避免把「未执行验证」误判为通过） |
| 真实破坏性卸载 | 会删除用户配置与运行期数据；本轮以 `--dry-run` + 静态护栏断言 + 载荷清单比对替代 |

> 说明：仓库根的 `DSHLauncher.exe` 通过「改名热替换」更新（Windows 不允许覆盖正在运行的镜像）。
> 因此**当前正在运行的实例仍是旧映像**，新版在下次启动启动器时生效 —— 这是既有设计，
> 不是本轮缺陷（`docs/OPS-RUNBOOK.md` §1c 有完整说明）。

---

## 10. 第四轮（卸载残留清理 + 结论复核）验证记录

本轮起因是一次**结论复核**：核对上一轮的交付状态时，发现其中三处"状态声明"已经过期
（运行中的实例、`target\` 体积、根产物与 `target\release\` 的对应关系），并暴露出一个
**真实缺陷**（悬空残留无法清理）。复核同时确认：门禁与单测结论仍然成立
（当时实测 165/165、145 单测、clippy 0 警告、fmt 干净）。

### 10.1 本轮修掉的缺陷：安装目录被删除后残留无法清理（P1）

**状态**：`%LOCALAPPDATA%\Programs\DSHLauncher` 已不存在，但
`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher` 与桌面
`DeepSeek Harness Launcher.lnk` 仍然存在，且都指向不存在的文件。

**为什么清不掉**（三条正规入口**实测**全部被拒）：

| 入口 | 实测结果 |
|---|---|
| 安装目录里的 `dsh-uninstall.exe` | 随安装目录一起消失，根本不存在（「设置 → 应用」的卸载按钮因此只会报找不到可执行文件） |
| 仓库/开发目录里的副本：`.\dsh-uninstall.exe --dry-run` | exit **2**：源码树护栏（"这是源码树/开发目录…已拒绝执行"） |
| `.\dsh-uninstall.exe --deferred-pass <安装目录> --dry-run` | exit **2**：缺少安装标记 `.dsllauncher-install` |
| 卸载器 CLI 是否接受"指定目录" | **不接受** —— 安装目录只能从**自身所在目录**推导（`crates/dsh-uninstall/src/main.rs`） |

**修复**：新增残留清理模式 `--clean-residue`（涉及 `cli.rs` / `guards.rs` / `steps.rs` / `platform.rs`）：

* 只清"磁盘外"残留：HKCU 卸载项 + 桌面快捷方式，**不做任何文件系统变更**（门禁断言强制）；
* 准入比正规卸载更严：`InstallLocation` / `UninstallString` / `DisplayIcon` **三值互相印证**
  （可执行文件必须位于 `InstallLocation` 下、且文件名属于本安装包载荷），受保护路径与源码树拒绝；
* 安装标记**仍然存在**时明确拒绝 —— 安装目录完整就该走正规卸载，残留清理不能成为绕过护栏的捷径；
* 支持 `--dry-run`；重复执行为 no-op（报告"没有注册表残留"）。

顺带收敛了一处重复实现：`platform::desktop_shortcuts()` 现在是快捷方式匹配规则的**唯一实现**
（此前 `print_plan` 里另有一份逐字重复的匹配逻辑，属"dry-run 报告与真实删除可能不一致"的漂移风险）。

### 10.2 实测（真实悬空状态，不是模拟）

```text
> dsh-uninstall.exe --clean-residue --dry-run
DRY-RUN：残留清理范围（不会修改任何内容）
  安装目录            : C:\Users\...\AppData\Local\Programs\DSHLauncher（目录已不存在）
  注册表卸载项        : HKCU\...\Uninstall\DSHLauncher → 将被删除
  桌面快捷方式        : 1 个将被删除
      - C:\Users\...\Desktop\DeepSeek Harness Launcher.lnk
  用户配置 / 运行期数据: **不处理**（本模式不做任何文件系统删除）
DRY-RUN：以上为完整影响范围；未改动任何内容。
```

真实执行（exit 0）后逐项核对：注册表卸载项 **已删除**、桌面快捷方式 **已删除**、
`%APPDATA%\DSHLauncher` 文件数 **5 → 5**、`%LOCALAPPDATA%\DSHLauncher` 文件数 **416 → 416**
（两处均零改动）；重复执行返回 `0` 并报告"没有注册表残留"。

### 10.3 文档数字与结论对齐

| 项 | 修正前 | 实测 |
|---|---|---|
| `--build-info` 修复标记数 | 15（本文 §4） | **16** |
| CHANGELOG 英文指标表 · 一致性项 | **147**（与中文的 165 互相矛盾） | **172** |
| 单元测试（README 中英 · CHANGELOG 中英 · 路线图 §15.5） | 145 | **149**（`dsh-uninstall` 17 → 21） |
| 一致性门禁项数（README 三处 · CHANGELOG · 路线图） | 165 | **175** |
| 本文 §5.4 的"当前机器处于已安装 5.0.0 LTS 状态" | — | 该结论**不再成立**（见 §10.1），已就地标注并指向本节 |

**另外更正一条上一轮的结论**：上一轮把"新布局下 example 同时存在于
`target/debug/build/<pkg>/<hash>/out/` 与 `target/debug/examples/`"当作实测结论。
本轮逐路径复核**无法证实**：本仓库 debug 树是旧布局（`build/<pkg>-<hash>/`，
`target\debug\build\dsh-core\` 根本不存在），`job_object_demo.exe` 只存在于
`target\debug\examples\`。可证实的同类证据是 **bin/lib**：新布局下
`target\release\build\dsh-app\<hash>\out\dsh_app.exe`（1,087,488 B）与 uplift 后的
`target\release\dsh-app.exe`（1,078,784 B）**不是同一个文件**（`fsutil hardlink list`
显示后者只与 `deps\dsh_app.exe` 同源）。因此上游反馈草稿
（[`CFT-FEEDBACK.md`](CFT-FEEDBACK.md)）改为只陈述**已核实**的 bin/lib 观察，
并把 example 的疑问明确写成"本仓库无法回答、需要上游文档说明"。

### 10.4 本轮门禁结果

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过（exit 0） |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | 通过（0 警告） |
| `cargo test --workspace --all-features --offline` | **149 passed / 0 failed**（`dsh-core` 97 · `dsh-ui` 13 · `dsh-app` 12 · `dsh-uninstall` 21 · `dsh-buildinfo` 6） |
| `pwsh -File tools/check-consistency.ps1` | `passed=175 failed=0 total=175` |
| `powershell -File tools/check-consistency.ps1`（**PS 5.1**） | 同上（与 PS 7 同结果） |
| `pwsh -File tools/gen-facts.ps1 -Check` | FACTS.json 与实测一致 |
| `pwsh -File tools/verify-version.ps1 -Exe .\DSHLauncher.exe -RequireInstaller` | 33 passed / 0 skipped / 0 failed |
| `pwsh -File tools/verify-install-layout-dryrun.ps1` | PASS（零改动） |
| `dsh-uninstall.exe --dry-run`（仓库根） | exit **2**（源码树护栏未回归） |

> 新增门禁断言 7 条：残留清理模式存在、帮助文本说明该模式、残留清理**零文件系统变更**、
> 注册表三值互相印证、护栏复用（标记/受保护路径/源码树）、支持 dry-run、
> 快捷方式匹配规则单一实现。门禁总数因此 165 → **175**。
>
> 本轮另产出：上游反馈草稿 [`CFT-FEEDBACK.md`](CFT-FEEDBACK.md)
> （两条：final-artifact/uplift 语义按 target 种类写进文档；提供打印**生效** build-dir 的官方途径）。

### 10.5 自检 E 段（失联检测）失败的真实原因：**测试侧**，不是产品侧

在**重建后的产物**上跑 `selftest.ps1`，E 段报三项失败（`adopt` / `detect` / `readopt`）。
逐条复核后确认：**产品行为正确，缺陷全部在测试脚本**。

| # | 现象 | 根因 | 修复 |
|---|---|---|---|
| 1 | 步骤 2「**未见接管日志**」 | `tools/verify-monitor.ps1` grep 的措辞是「已接管端口」，而产品现在打印的是「端口 P 上已有 dsh 服务在运行（PID …），**直接接管**」或「…**已直接接管**（会话未中断）」—— 该旧措辞在源码里**已不存在** ⇒ 这一步**在任何环境下都必然失败**。同类措辞误报此前已在 `tools/verify-token-navigation.ps1` 修过（见 CHANGELOG），本脚本漏了 | 判据改用产品实际措辞；并由门禁**交叉印证**（产品源码含「直接接管」+ 脚本按同一措辞判定），防止再次漂移 |
| 2 | 步骤 3「⚠ 端口仍被占用」、步骤 4「未检测到失联」、步骤 6「未自动重新接管」 | 脚本默认端口写死 **3099**，而本机 3099 **正是用户当前会话的端口**（PID 15256：启动时间早于本次自检，且全程存活）。脚本只检查"端口在听"就断言"外部 dsh 就绪"，于是它以为在测自己拉起的 PID 17292，实际监听者是 15256；它随后杀掉的 17292 从未绑上端口 ⇒ 后三步全是**误导性**失败（17292、5804 事后均已退出，15256 存活至今） | ① 默认改为**自动挑选空闲端口**（`-Port` 仍可指定）；② 新增 `Assert-Owner`：端口监听者必须**正是**本次启动的进程，否则明确报错；③ 准备阶段发现端口被占用就**报错退出**，绝不结束不是本测试启动的进程（旧实现会 `taskkill` 掉端口所有者 —— 在本机就是用户的活会话） |

**修复后实测**（自动挑选端口 54847）：

```text
  ✓ 外部 dsh 就绪（PID 17228）
  PASS: 已接管外部实例 ✓          ← 日志：端口 54847 上已有 dsh 服务在运行（PID 17228），直接接管。
  ✓ 外部 dsh 已退出，端口释放
  PASS: 检测到失联（约 12 秒）✓    ← 日志：服务 54847 已失联（外部实例已退出）
  ✓ 新 dsh 就绪（PID 8240）
  PASS: 自动重新接管（约 2 秒）✓   ← 日志：检测到端口 54847 重新有 dsh 服务在运行（PID 8240），已重新接管。
  ✅ 全部通过（exit 0）
```

同一次日志还印证了**上一轮端口修复确实生效**：进入时 `service.json` 记的是 3099/15256，
与配置端口 54847 不一致 ⇒ 打印 WARN 并**拒绝**接管，改走"端口上已有 dsh ⇒ 直接接管"分支
（`crates/dsh-app/src/service.rs` 的分支 2/分支 3）。

随后**完整 `selftest.ps1` 全量重跑**：A1 / A2 / B / C / D / E **全部 PASS**，
汇总 `✅ 全部通过`（exit 0）。用户配置 `%APPDATA%\DSHLauncher\settings.toml`
在测试前后**逐字节一致**（脚本自校验 + 人工复核），3099 上的活会话全程未受影响。

门禁相应新增 3 条断言（判据与产品措辞交叉印证 / 端口所有权前置校验 / 只结束自己的进程
且默认自动选端口），总数 172 → **175**。

---

## 11. 第五轮（工具链迁移：主工具链切到 nightly + build-dir Layout v2）验证记录

### 11.1 决策依据（先核实事实，再动手）

| 检查 | 实测 |
|---|---|
| 最新 stable | **`1.98.1`** —— `rustup check` 报 up to date，**stable 已无更新可升** |
| v2 布局在哪 | **只有 nightly**：scratch 工程实测 nightly 的 `target\debug\` = `build` / `examples` / `incremental`（**无 `deps\`、无 `.fingerprint\`**），`build\` 下按包名分桶；stable 仍产出 `deps\` + `.fingerprint\` |
| 上游状态 | v2 稳定化仍在上游推进（见 [`CFT-FEEDBACK.md`](CFT-FEEDBACK.md) 引用的 issue/PR），**尚未进入任何 stable 发行版** |
| 本仓库在 nightly 下能否编译 | `cargo check --workspace --all-targets --offline` **exit 0**（56s），**无需改任何源码** |
| nightly 组件 | 原本**缺 `clippy` / `rustfmt`**（门禁会直接报 not installed）⇒ 必须补装 |

结论：要"完整应用 v2"，主工具链只能是 nightly —— 于是用
[`rust-toolchain.toml`](../rust-toolchain.toml) 钉死 **`nightly-2026-09-10`**
（带日期以保证可复现）+ `components = ["clippy", "rustfmt"]` + `profile = "minimal"`。

### 11.2 迁移与实测

| 步骤 | 结果 |
|---|---|
| 安装钉死的 nightly | 新增工具链 `nightly-2026-09-10`：`rustc 1.100.0-nightly (a36d05efa 2026-09-09)`；`rustup show active-toolchain` 显示 `overridden by 'D:\DSHLauncher\rust-toolchain.toml'` |
| 清理旧布局缓存 | `clean.ps1 -Cache` 回收 **2,994.3 MB**（旧布局缓存对 v2 全部失效），6 项交付物保留 |
| 全量重建 | `build.ps1 release` + `build-setup.ps1` 通过；版本链路 **33/0/0** |
| **布局证据** | `target\release` 与 `target\debug` 顶层均为 `build / examples / incremental / *.d / *.pdb / *.rlib`；`Test-Path target\release\deps` = **False**、`target\release\.fingerprint` = **False**（debug 树同样 False/False） |
| `build\` 分桶 | 按包名：`anyhow, bitflags, bytes, cfg-if, cookie, …`（每包 `<哈希>\{out,fingerprint}`） |
| 产物尺寸 | 启动器 1,078,784 → **1,087,488 B**；卸载器 309,760 → **316,928 B**；安装包 1,618,432 → **1,641,472 B**（nightly 编译 ⇒ `FACTS.json` 已按实测重生成） |

**自清洁与门禁无需为 v2 改动**（逐条核实，非推断）：

* `clean.ps1` 的唯一不变式只是 **6 个 release 交付物**；删除逻辑是"除保留项外全删 + 遇到含保留
  项的目录就递归"（`Add-CachePlan`），因此对旧式 `<pkg>-<hash>`、v2 式 `<pkg>\<哈希>`、
  `deps\`、`.fingerprint\` **一律无感**。迁移前的 `-Cache -WhatIf` 实测：它把两种并存布局合并为
  一个 `target\release\build` 条目（431.8 MB = v2 340.3 + 旧式 91.6）处理；
* 全库无脚本硬编码内部布局（`check-consistency` 与 `verify-build-layout.ps1` 的静态断言持续强制）；
* `selftest.ps1` 的 example 路径取自 `cargo --message-format=json`，不猜目录。

### 11.3 迁移过程中发现并修掉的两个真实问题（都是"跑了才知道"）

| # | 问题 | 根因 | 修复 |
|---|---|---|---|
| 1 | 新 cargo 报 `unused_workspace_dependencies` 警告（`dsh-core` / `dsh-ui`） | `[workspace.dependencies]` 声明了这两项，但三个成员用的是**裸** `{ path = "../..." }`，没有任何成员以 `workspace = true` 引用 | 三个成员统一改为 `dsh-core.workspace = true` / `dsh-ui.workspace = true`（`Cargo.lock` **零变化**，语义等价）；新增门禁断言"workspace 依赖每项都必须被引用" |
| 2 | `verify-build-layout.ps1 -IncludeNightly` 把**交付物换成了另一个编译器**的产物 | 该步用 `cargo +nightly`（**滚动** nightly），而主工具链是**钉死的日期版**（rustc 提交号不同 ⇒ 代码生成不同）；它直接写默认 `target\` ⇒ `target\release` 被覆写。实测：卸载器 316,928 → **317,440 B**，而 `gen-facts -Check` **看不见**该漂移（只比尺寸/版本，不比字节） | 探针改用**独立 `CARGO_TARGET_DIR`**（`build-layout-probe-nightly`，跑完即删），`-Run` 结束时提示恢复步骤；新增门禁断言 + `.gitignore` 覆盖该探针目录 |

**修复验证**（重跑 `-Run -IncludeNightly`）：全部断言 PASS，且 `target\release\dsh-uninstall.exe`
回到 **316,928 B**（钉死版），探针目录无残留。

### 11.4 本轮门禁结果（工具链 = 钉死的 nightly + v2）

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过（exit 0；新 rustfmt **无格式漂移**） |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | 通过（0 警告；**且构建输出无 manifest 警告**） |
| `cargo test --workspace --all-features --offline` | **149 passed / 0 failed** |
| `pwsh -File tools/check-consistency.ps1` | `passed=179 failed=0 total=179` |
| `powershell -File tools/check-consistency.ps1`（PS 5.1） | 同上（与 PS 7 同结果） |
| `pwsh -File tools/gen-facts.ps1 -Check` | FACTS.json 与实测一致 |
| `pwsh -File tools/verify-version.ps1 -Exe .\DSHLauncher.exe -RequireInstaller` | **33 passed / 0 skipped / 0 failed** |
| `pwsh -File tools/verify-install-layout-dryrun.ps1` | PASS（零改动） |
| `pwsh -File tools/verify-build-layout.ps1 -Run -IncludeNightly` | **通过**（静态 4 项 + 三配置实测；搬迁探针 1178 个中间产物落到独立目录后即删） |
| `selftest.ps1`（A1/A2/B/C/D/E） | **全部 PASS** |

> 回退方法（**一条命令、无需改源码**）：`Remove-Item rust-toolchain.toml` —— 之后 `cargo` 立刻回到
> stable 1.98.1（旧布局）。完整步骤、升级钉死版本的检查清单、以及与上游稳定化的关系见
> [`OPS-RUNBOOK.md`](OPS-RUNBOOK.md) §8。

---

## 12. 上传前一致性核对（2026-09-12）

推送仓库前做全链路一致性核对（"新布局的安装包/卸载包是否都吃到最新更新、版本号是否同源"），
发现并修掉**两处**既有校验都覆盖不到的问题。

### 12.1 安装包内嵌资源陈旧（打包盲区）

| 链路 | 结果 |
|---|---|
| **安装包内嵌资源 ↔ 当前产物** | ❌ → ✅ **6 项逐字节一致**（修复前内嵌的 `README.md` 是旧版：41,763 B vs 46,086 B） |
| 根产物 ↔ `target\release` | ✅ 同一构建（哈希相同） |
| 产物 ↔ 源码 | ✅ 产物时间戳**晚于全部构建输入**（无陈旧） |
| 版本链路 | ✅ `Cargo.toml` / `Cargo.lock` 5.0.0 · 启动器/卸载器 5.0.0 · 安装包 5.0.0.0 · 标记 `version=<AppVersion> LTS` · 注册表 7 值同源 · `--version` = `DSHLauncher 5.0.0 LTS (release)` · `--build-info` 16 个修复标记 |
| 载荷清单 | ✅ 安装器 ↔ 卸载器逐项一致（6 项）；源码无陈旧版本字面量 |

**盲区**：`verify-version.ps1` 只验证"安装包内嵌了卸载器"这类**存在性**事实，`check-consistency.ps1`
只看源码文本 —— 没有任何校验比对**字节**。于是"给 README 加完「文档地图」却没重新打包"这种状态
让所有校验**全绿**，而用户装出来的 README 是旧的。

**修复**：新增 [`tools/verify-embedded.ps1`](../tools/verify-embedded.ps1)（PS 5.1 反射抽取 6 项
.NET 内嵌资源 + SHA256 逐字节比对；带 `-Root`/`-Setup` 以便正负用例，实测正例 0 / 负例 1 / 跳过 2），
挂进 `finish-release.ps1` **第 3b 步**；门禁新增 1 条断言（178 → **179**）。
**即时验证**：改完文档计数后（README 长度不变、内容已变）该工具**立刻报 FAIL**，
证明它抓的是字节而不是尺寸。

### 12.2 自检 C 段在"外部会话 + 无 cookie"环境下挂死（自检缺陷）

C 段直接用**用户真实配置**跑 `--ipc-probe`：端口上已有外部 dsh 时读不到 token、profile 里又没有
cookie ⇒ 内嵌页 401 ⇒ 认证自愈按设计弹窗询问 ⇒ 无头探针无人应答 ⇒ **挂满 180 s**（实测两次复现）。
改为与 E 段同套路：**临时空闲端口 + 备份/逐字节还原用户配置 + 按日志 PID 回收自建服务**；
并修正 `C-open` 断言（自建服务分支的日志是「已打开设置窗口。」，旧断言只认接管分支的
「已打开内嵌界面」，必然假失败）。

**修复后实测**：`selftest.ps1` **全部 PASS（exit 0）**，用户配置**逐字节还原**（140 → 140 字符），
3099 上的活动会话全程未受影响，且**无服务泄漏**（除 3099 外无 dsh 监听）。

### 12.3 本轮发布流程

`pwsh -File tools/finish-release.ps1`：**全部步骤通过**（含新增的 3b 与修正后的 10），
报告见 [`FINISH-REPORT.md`](FINISH-REPORT.md)。
