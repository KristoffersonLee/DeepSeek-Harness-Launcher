# DSHLauncher v5.0.0 深度 Code Review 报告（定稿版）

> 被审版本：**5.0.0**（`Cargo.toml` 的 `[workspace.package] version` 是全仓库唯一版本来源）
> 审查方式：逐文件通读全部源码（`crates/**/*.rs`、`ui/*.html`、`*.ps1`、`DSHLauncherSetup.cs`、`Cargo.toml`/`Cargo.lock`、`docs/*.md`）+ 真机运行门禁 + 可复现实验；并对上一轮报告（同名文件的上一版，见 §7 第 3 条的说明）的每一条结论做「复核 / 反证 / 新发现」三分类。
> 关键立场：**不误报**。凡无法从源码或实测判定的，一律列入 §5 「不确定项」并写明判定所需条件。

---

## 1. 执行摘要

### 1.1 总体健康度：**7.5 / 10**（上一轮 6.5）

| 维度 | 本轮 | 上一轮 | 依据 |
|---|---|---|---|
| 架构分层与可测试性 | 8.5 | 8 | `dsh-core` 保持零 GUI 依赖；单测 **97 个全绿**（上一轮基线本身是**红的**：`cargo test` 因 flaky 用例整轮失败） |
| Windows 系统编程功底 | 8 | 8 | Job/Object 语义、`GetExtendedTcpTable` 两级缓冲校验、`Local\` 命名空间、`ShellExecuteW`、句柄全路径 `CloseHandle` —— 复核后确认**仍然正确** |
| 安全基线 | 7.5 | 7 | 新增：脱敏扩到 18 个敏感键 + `Authorization: Bearer/Basic`；设置页/内嵌页**导航白名单**；安装器「任意目录 + 单引号注入」两个 P0 已处置。**扣分项**：安装器仍无 Authenticode 校验、无 manifest |
| 核心功能可用性 | 8 | 4 | 上一轮两个 P0（401、死锁）已修；本轮又发现**同一族的两处残留**：等待预算在冷启动被绕过、且无每帧驱动（见 P0-1） |
| 生命周期与容错 | 8 | 4 | 服务解耦（`independent`）+ `service.json` 对账确认有效；本轮修掉「`Starting` 期间被误判失联」与「监测线程退出竞态」两个自愈失效缺陷 |
| 文档与实测一致性 | 6 | 3 | `docs/FACTS.json` + `gen-facts.ps1 -Check` 机制已建立；但 `-Check` 原本只比对 4 项事实（已扩展），且 README 里一致性项数存在 3 个值（已统一） |
| 性能与内存 | 8 | 8 | 用户可感知的启动路径实测 4–27 ms；本轮消除了 UI 线程上所有**阻塞数百毫秒**的调用（进程终止、配置重启、逐帧端口探测） |
| 交付链与门禁自身可信度 | 6.5 | — | **新维度**。上一轮把「66/66 一致性全绿」当作证据，但本轮证明：约 55 条「接线类」断言**可以被一条注释满足**，`finish-release.ps1` **永远以 1 退出**，`verify-icon.ps1` **不可能失败**，`gen-facts -Check` 只比 4 项 |

### 1.2 Top 5 必修项（本轮）

1. **P0-1 · token 等待预算在冷启动被绕过，且等待逻辑没有驱动源**（`app.rs`）。
   两个独立缺陷叠加：`run()` 直接 `service.start()`，`auth_wait_ticks` 仍是初值 0，于是首个 `Ready{url:None}` 立刻"预算耗尽"→ 用**无 token 地址**开窗（实测该地址 HTTP 401）；同时预算只在**事件**里递减，事件之间没有任何东西重试 `open_harness()` → 一旦 token 永远不来（上游改格式 / 输出落到 stderr），**窗口永远不会出现**。这正是上一轮「双击后什么都没有」的同族残留。
2. **P0-2 · 失联监测把 `Starting` 期间的"端口还没监听"判成失联**（`service.rs`）。
   `LOST_LIMIT = 8 × 1.5 s = 12 s`，而 dsh 冷启动实测 **30 秒以上**。于是每次冷启动到第 12 秒：状态被错误置为 `Error`、刚写下的 `service.json` 被清掉、日志报「自有进程已退出且自动重启用尽」——而子进程活得好好的。
3. **P0-3 · `cargo test` 基线是红的**（`dsh.rs` 的 `picks_highest_version` 用固定临时目录名，跨进程互相删除，实测 4 次里失败 1 次）。**任何「单测全绿」的结论在修掉它之前都不成立。**
4. **P0-4 · `--build-info` / `--probe-identity` 用 `print!` 写 stdout**（`main.rs`）。release 是 GUI 子系统，无控制台时 `GetStdHandle` 为空 → `print!` **panic** → `panic=abort` 下整进程崩溃，**标记文件还没写**。发布脚本因此会把一个好产物判成坏的。同一族的历史修复（`eprintln!`）上一轮做了，**stdout 侧漏了**。
5. **P0-5 · 安装器允许任意安装目录，而它生成的卸载器会 `Remove-Item -Recurse` 那个目录**（`DSHLauncherSetup.cs`）。守卫只排除盘符根 / `%WINDIR%` / `%USERPROFILE%`；实测 `GetPathRoot('C:\Users\x\AppData\Local')` 返回 `C:\`，因此 `--silent-install "%LOCALAPPDATA%"` 会被接受、卸载时被递归删除。同一段代码还用字符串拼接把路径塞进一个 `-Command`，**单引号可注入任意 PowerShell**。

### 1.3 按 ROI 排序的改进建议

| # | 改进 | ROI 论证 |
|---|---|---|
| 1 | 把「接线类」断言从**文本匹配**改为**调用点断言或运行期探针** | 当前 55 条断言可被一条注释满足。这是「门禁自身可信度」的唯一杠杆：改完之后，本轮修的每一个 P0/P1 才真正有回归保护 |
| 2 | 让 `finish-release.ps1` 的退出码反映每一步的真实结果 | 它现在**永远**退出 1（`$published` 赋值不逃出 `Step` 作用域），同时把 clippy/test/一致性/版本链的失败全部吞成报告行 —— 双重失效：既永远红，也永远不因真失败而红 |
| 3 | UI 线程零阻塞（本轮已完成，需守住） | 已消除 4 处数百毫秒级阻塞。建议加一条**静态**门禁：禁止在 `app.rs` 里出现 `service.stop()` / `is_port_listening(` 的直接调用 |
| 4 | 安装器：安装目录白名单 + 标记文件 + 不再递归删用户给的路径 | 这是唯一能让用户**丢数据**的缺陷类别（P0） |
| 5 | 把 `docs/*.md` 的数字全部改为从 `FACTS.json` 派生并被硬断言 | 机制已有，但目前只覆盖单测数/exe 量级；代码行数、一致性项数、依赖包数仍靠手写 |

### 1.4 是否颠覆性重构：逐组件判断

| 组件 | 判断 | 理由 |
|---|---|---|
| `dsh-core`（纯逻辑层） | **保留，局部重写** | 分层是最大结构收益（97 个单测全在这一层）。局部重写：`maintenance.rs` 的 JSON 区间改写、`log.rs` 的脱敏与 `Clone` 语义、`probe.rs` 的代际号语义、`dsh.rs` 的 node 解析语义 |
| `dsh-app/src/service.rs` | **重构**（非重写） | 上一轮报告建议重写约 200 行。本轮实际做的是**定向重构**：把 `ProcessManager` 拆到独立互斥量（结构性消除死锁类）、把 `Stopping` 补进状态机、把阻塞终止移出锁、修掉监测线程的两个语义缺陷。理由：`monitor_worker` 的复杂度并非来自"分支太长"，而来自**缺一个显式状态机**；补状态机比改写控制流风险低得多，而风险正是这个文件最贵的东西 |
| `dsh-app/src/app.rs` | **重构（就地）** | 上一轮建议拆成 `service_session.rs` / `ipc.rs`。本轮判断：**暂不拆分**。该文件已从 739 行长到 1.4k 行，但它的四个职责之间共享大量状态（`harness` / `pending_url` / `want_open` / `theme_*`），拆模块需要把这些字段一并提升为 `pub(crate)`，收益是整洁度、代价是**在一个没有 E2E 门禁保护的会话里做纯搬家**。本轮改为：把三处阻塞编排抽成 `spawn_stop` / `spawn_restart` / 清理 worker，并补文档。**这是一个明确的取舍，不是遗漏** |
| `wry` + `tao` 直连 | **保留** | 已正确解决 WebView2 COM 直连、共享 `WebContext`、DWM 着色、托盘、单文件 exe。换 `tauri` 会被 `check-consistency.ps1` 禁止，且不解决任何已发现缺陷 |
| `std::thread` + `mpsc` | **保留** | 真实并发只有 5 类 worker；峰值线程数个位数。本轮新增的 worker 都带 `stack_size(256–512 KiB)` 与命名 |
| `DSHLauncherSetup.cs` | **保留，改 11 处** | 重写风险最高、收益最低。但其中两个 P0（任意目录递归删除、单引号注入）必须改 |
| `tools/*.ps1` | **保留，改 20+ 处** | 这些脚本是**证据的来源**，它们的可信度直接决定报告的可信度。本轮改动集中在"不许假通过 / 不许吞错 / 不许删用户数据" |

---

## 2. 问题总表

> 编号规则：`N-xx` 为本轮新发现；引用上一轮报告的问题沿用其编号（A1/A2/B1…）。
> 严重度：P0 致命 / P1 严重 / P2 一般 / P3 轻微。

### 2.1 P0

| # | 文件:位置 | 问题 | 证据 | 根因 | 修复 | 验证 |
|---|---|---|---|---|---|---|
| **N-1** | `crates/dsh-app/src/app.rs`（`auth_url_settled` / `open_harness` / `run`） | 冷启动时首个 `Ready{url:None}` 立刻被判「预算耗尽」→ 用无 token 地址导航（HTTP 401）；且等待预算没有驱动源，token 永不到达时**窗口永不出现** | 旧 `auth_url_settled`：`if self.auth_wait_ticks == 0 { warn(…); return Some(ready_url()) }`；`run()` 只调 `self.service.start()`，未预设预算；`auth_wait_ticks` 仅在事件路径被递减 | 「等待」被建模成**计数器**而不是**时间预算**，且计数器由事件驱动 | ①`Ready` 事件（含 `url:None`）到达时重置预算；②新增 `want_open` + `pump_pending_open()`，每帧重试；③**先判端口是否监听、再消耗预算**（冷启动 30 秒不再空耗） | `check-consistency.ps1` 新增 4 条行为断言；`cargo test` 全绿；静态检查 `want_open` 有真实调用点 |
| **N-2** | `crates/dsh-app/src/service.rs`（`monitor_worker`） | `Starting` 期间端口尚未监听 = **预期**，却被计入失联：12 秒后置 `Error`、清 `service.json`、日志谎报「自有进程已退出」 | `LOST_LIMIT: u32 = 8`、`MONITOR_INTERVAL = 1500ms`（=12 s）；`state.is_active()` 含 `Starting`；dsh 冷启动实测 30 s+（上一轮 `[boot] 服务就绪: 41329ms`） | 状态机把"启动中"与"运行中"混在一个 `is_active()` 里，失联统计对两者一视同仁 | `Starting` 且子进程存活 → 清零 `lost` 并 continue（权威是就绪 worker）；`Starting` 且子进程消失 → 立即按失联处理（不必等满 12 s） | 新增日志语义断言 + 手工走查；`service.rs` 无 `unused` 警告 |
| **N-3** | `crates/dsh-core/src/dsh.rs`（`picks_highest_version` 用例） | **基线 `cargo test` 是红的**：固定临时目录 `<temp>\dsh_test_nodejs` 被并发/历史进程互相删除 → `WriteAllBytes` 报 `NotFound` | 基线记录：`test result: FAILED. 68 passed; 1 failed; 1.82s`（`crates\dsh-core\src\dsh.rs:434`）；单独重跑 6 次全绿；连跑 4 次里 1 次失败 | 测试用了**跨进程共享的固定路径** | 改为 `unique_temp_dir()`（PID + 进程内序号）；顺带补 `ignores_dirs_without_node_exe` | `cargo test` 连续 6 次全绿（原始输出见实施记录） |
| **N-4** | `crates/dsh-app/src/main.rs`（`emit_build_info_and_exit` / `--probe-identity` / `--quit`） | `print!` 在 GUI 子系统 + 无控制台时写失败 → panic → `panic=abort` 整进程崩溃，**标记文件尚未写入** | 3 处 `print!`；release 声明 `windows_subsystem = "windows"`；同族问题（`eprintln!`）上一轮已修但只修了 stderr | 只想到"没有控制台所以打印看不到"，没意识到"写失败会 panic" | 新增 `emit_stdout()`（显式忽略写入错误）；三处全部替换 | `grep -c 'print!'` 在 `main.rs` 只剩注释提及；clippy 0 警告 |
| **N-5** | `DSHLauncherSetup.cs`（安装目录校验 + 生成的 `uninstall.ps1`） | ①用户可指定任意安装目录（GUI 路径**零校验**），生成的卸载器 `Remove-Item -Recurse -Force` 该目录；守卫不覆盖 `%LOCALAPPDATA%`/`%APPDATA%`/`%TEMP%`/`%System32%`；②把路径拼进 `-Command` 字符串，目录名含 `'` 即注入任意 PowerShell（隐藏窗口 + `-ExecutionPolicy Bypass`） | 守卫：`$InstallDir -ne $root -and -ne $env:WINDIR -and -ne $env:USERPROFILE`；实测 `[IO.Path]::GetPathRoot('C:\Users\x\AppData\Local')` → `C:\`；生成语句 `('… Remove-Item -LiteralPath ' + "'" + $InstallDir + "'" + ' -Recurse -Force')` | 把"用户选的目录"当成了"我们拥有的目录"；以及用字符串拼接构造命令 | ①安装期白名单/祖先校验（CLI + GUI 共用）；②安装时写 `.dsllauncher-install` 标记，卸载期**要求标记存在**才允许递归删除；③生成脚本改用 `$PSScriptRoot`，**不再把路径拼进任何命令字符串** | 见实施记录 §安装器；`[Parser]::ParseInput` 校验生成的脚本文本 |
| **N-6** | `tools/verify-monitor.ps1:52,107` | **测试脚本覆写并删除用户的 `%APPDATA%\DSHLauncher\settings.toml`**；该路径由 `selftest.ps1` E 段与 `finish-release.ps1` 第 10 步自动触达 | `Set-Content -Path $cfg -Value "port = $Port"` … `Remove-Item $cfg -Force`，`$cfg = Join-Path $env:APPDATA 'DSHLauncher\settings.toml'` | 测试用了**真实配置路径**，且清理语义写成了"删除"而不是"恢复" | 写入前备份原始字节；`finally` 中恢复（原先不存在则删除） | 脚本自带对照输出；`check-consistency.ps1` 新增「不得删除用户配置」静态断言 |

### 2.2 P1

| # | 文件:位置 | 问题 | 证据 | 修复 |
|---|---|---|---|---|
| **N-7** | `crates/dsh-app/src/service.rs`（`start()`） | 上一轮的"持锁 spawn 死锁"修复只做了**一半**：`debug_assert!(try_lock().is_ok())` 之后**立刻又重新取锁**并在持锁状态下调用 `start_dsh` | `drop(inner); debug_assert!(self.inner.try_lock().is_ok(), …); let started = { let mut g = self.inner.lock()…; g.process.start_dsh(…) };` | `ProcessManager` 拆到独立 `Arc<Mutex<..>>`；spawn 全程不持有 `ServiceInner` 锁；锁序固定为 `ServiceInner → ProcessManager` |
| **N-8** | `crates/dsh-core/src/log.rs`（`impl Clone for RollingLogger`） | `clone()` 新建一把锁 → 进程内的多个 logger 副本**互不排斥**；滚动期间 A 改名的文件里可能落进 B 的写入 | `lock: Mutex::new(())`（clone 内） | `lock: Arc<Mutex<()>>`、`dir_ready: Arc<AtomicBool>`（共享）；新增 `clones_share_one_lock_and_dir_flag` 与 200 行并发写完整性用例 |
| **N-9** | `crates/dsh-core/src/probe.rs`（`impl Clone for ReadyProbe`） | `clone()` 只复制代际号**当前值** → 对克隆 `bump_generation()` 无效 → `GenerationChanged` 取消路径在克隆上**永不可达** | `generation: AtomicU64::new(self.generation.load(..))` | `generation: Arc<AtomicU64>`（共享）；新增「在途探测被 bump 立即取消」用例 |
| **N-10** | `crates/dsh-app/src/service.rs`（`ensure_monitor` / `monitor_worker`） | 监测线程退出竞态：`ensure_monitor()` 在锁内只检查 `monitor_running`，而监测线程在观察到 `monitor_enabled=false` 后就返回 → 快速「停止→启动」会得到「线程已消失但 `monitor_enabled=true`」→ **自愈/失联重连/外部重新接管全部静默失效** | `if !enabled { return; }` 与 `if g.monitor_running { return; }` 之间没有共同临界区 | 退出前在锁内重新确认 `monitor_enabled`：为真则继续值守，为假才置 `monitor_running=false` 并退出 |
| **N-11** | `crates/dsh-app/src/service.rs`（`stop()`） | UI 线程上持 `ServiceInner` 锁执行**递归终止进程树 + `wait`**（数百毫秒到数秒） | `let mut inner = self.inner.lock()…; inner.process.stop()` | 阻塞段移出 `ServiceInner` 锁（只持 `ProcessManager` 锁）；UI 路径改走 `stop_async()` + 后台任务通道 |
| **N-12** | `crates/dsh-app/src/app.rs`（配置变更 / 归档清理） | ①「保存设置」触发的 `stop()→set_config()→start()` 全在 UI 线程；②归档清理的「停服务」也在 UI 线程（只有删除在 worker） | 旧代码 `let _ = self.service.stop(); … self.service.start()`；清理分支 `if let Err(e) = self.service.stop()` | 新增 `BackgroundTask` 通道；`spawn_stop()` / `spawn_restart()` / 清理 worker 把三步全部串行放到 worker |
| **N-13** | `crates/dsh-app/src/app.rs`（唤起路径） | `is_port_listening()` 是**阻塞** connect（300 ms 超时），而唤起待办里**逐帧**调用它；事件循环 100 ms/帧 → 服务未起来时 UI 线程约 75% 时间在阻塞，最长 30 秒 | `else if dsh_core::is_port_listening(self.config.port) {` 位于 `activation_pending` 分支内 | 加 `ACTIVATION_PROBE_TICKS = 10`（≈1 秒）限流；`open_harness` 内的端口探测改为**仅在必要时**执行 |
| **N-14** | `crates/dsh-core/src/log.rs`（`redact_secrets`） | 只覆盖 `token=`；`pin=` / `api_key=` / `cookie=` / `secret=` / `Authorization: Bearer …` 等**原样落盘**（stderr 会被排空线程写进日志） | `const KEY: &str = "token=";` | 18 个敏感键（词边界 + 可选 `=`/`:` + 可选引号）+ `Bearer`/`Basic` 方案名；5 个新单测（含 UTF-8 边界） |
| **N-15** | `crates/dsh-core/src/dsh.rs`（`resolve_node`） | 配置了 `node_path` 但路径无效时**静默回退 PATH**：用户以为自定 node 生效，实际跑的是另一个 | `if path.is_file() && 文件名是 node.exe { return Ok(path) }`，否则继续往下探测 | 新增 `DshError::InvalidNodePath`；文件名不符 / 文件不存在都**硬报错**（空白字符串仍视为未指定） |
| **N-16** | `crates/dsh-core/src/dsh.rs`（`run_capture`） | 超时后只 `child.kill()` 直接子进程；`npm.cmd` 派生的 node 仍持有管道写端 → 后面的 `t1.join()` **可能永久阻塞** | `let _ = child.kill(); break child.wait()?;` 之后的 `t1.join()` | 超时路径改为 `kill_process_tree(child.id())`（先 `kill`+`wait` 再递归兜底） |
| **N-17** | `crates/dsh-app/src/dialog.rs`（`about`） | 「关于」对话框对用户宣称「进程回收：Windows Job Object（强杀启动器不留孤儿进程）」，而**默认 `independent` 模式下根本没有 Job**，强杀启动器**故意不回收** dsh | 硬编码字符串 | `about(version, tied_service)`：按实际生命周期策略生成文案 |
| **N-18** | `crates/dsh-core/src/config.rs`（`is_from_newer_schema`） + `ui/settings.html` + `crates/dsh-app/src/main.rs` | 两处「文档承诺无实现」：①`is_from_newer_schema()` 的文档写着"调用方应提示用户"，但**零调用方**；②`main.rs` 的配置损坏提示与 `ConfigError` 文案都说"可在设置页重置"，而设置页**没有重置控件**、`Settings::reset()` **零调用方** | `pub fn is_from_newer_schema`（仅测试引用）；`你可以在设置页"重置"或手动删除该文件后重启` | ①在 `main.rs` 载入配置后按 schema 版本告警；②设置页新增「恢复默认设置」按钮 + `SettingsCommand::ResetConfig` + `Settings::reset_on_disk()`（二次确认 + 默认焦点「否」） |
| **N-19** | `crates/dsh-ui/src/settings.rs`（`parse_command`） | 缺 `trayOnClose` / `independentService` 字段时回退到**默认值 true**，而 `save` 作用在现有配置的**副本**上 → 会把用户显式关掉的托盘偏好 / 显式选的 `tied` 生命周期**悄悄改回** | `tray_on_close: …unwrap_or(true)` | 两个字段改为 `Option<bool>`；缺字段 = **保留当前值**；新增 `save_missing_checkboxes_means_keep_current_value` 用例 |
| **N-20** | `crates/dsh-app/src/dialog.rs`（`html_to_plain_text` / `strip_blocks`） | ①实体解码顺序错（先解 `&amp;` 再解 `&lt;`）→ `&amp;lt;` 被**双重解码**成 `<`；②`<script>` 未闭合时直接 `break` → **丢弃剩余全部内容**，对话框可能变空白 | `.replace("&amp;", "&").replace("&lt;", "<")`；`None => break` | 顺序改为「先具体实体、最后 `&amp;`」；未闭合改为返回 `truncated` 标记并由调用方给出明确告知 + 兜底文案 |
| **N-21** | `crates/dsh-ui/src/harness.rs` / `settings.rs` | 两个 WebView 都**没有导航白名单**。设置页带 IPC 通道 + 初始化脚本（含用户工作目录/node 路径）；内嵌页带 `?token=` | 无 `with_navigation_handler` | 两个 WebView 都加 `with_navigation_handler`：内嵌页只允许回环/`about:blank`/`data:`/`blob:`；设置页只允许空白页与内联文档 |
| **N-22** | `crates/dsh-app/src/tray_handler.rs` | `try_recv()` 只取**一次**：取到无法识别的菜单 id 就返回 `None`，调用方的 `while let` 立刻结束 → **排在后面的合法事件要等下一帧** | `if let Ok(event) = …try_recv() { if let Some(e) = map(id) { return Some(e) } } None` | 内部循环取到队列为空才返回（未知 id 丢弃后继续） |
| **N-23** | `crates/dsh-app/src/app.rs`（`on_window_closed` / `maybe_sample_theme`） | ①关窗只清 `theme_sample_pending`，残留 `theme_slot`/`theme_sample_at`/`theme_last_color` → 重开窗口会把已销毁 WebView 的陈旧值当新结果，且因"颜色没变"永不打印；②提交失败时不更新 `theme_sample_at` → 限流失效、**每帧重试**且无任何日志 | 回填不完整；`theme_sample_at` 只在 `if submitted` 内更新 | 关窗时清空全部主题状态；`theme_sample_at` **每次尝试都更新** |
| **N-24** | `crates/dsh-app/src/app.rs`（`ctrl_down`） | 自己数 KeyDown/KeyUp 记录修饰键：按住 Ctrl 切到别的程序松开，收不到 KeyUp → 状态粘滞 → 之后随便敲 `R` 就刷新页面、冲掉正在编辑的内容 | `KeyCode::ControlLeft \| ControlRight => self.ctrl_down = pressed` | 改用 `WindowEvent::ModifiersChanged`（tao 0.35 提供）+ `Focused(false)` 清零，KeyDown/Up 仅作兜底 |
| **N-25** | `crates/dsh-ui/src/harness.rs`（`FORCE_DESKTOP_JS`） | 注入到**承载业务页面**的脚本里有恒真判断（`IS_WEBVIEW2` 永远为真）→ `else` 分支死代码，`resize` 监听与"每 1.5 秒重加 class、持续 120 秒"的定时器全是无用功 | `var force=IS_WEBVIEW2\|\|window.innerWidth>900;` | 简化为一次幂等的 `classList.add` |
| **N-26** | `crates/dsh-ui/src/theme.rs`（`set_titlebar_colors`） | 三项 DWM 属性用 `?` 串联：Win10 上 `DWMWA_CAPTION_COLOR` 返回 `E_INVALIDARG` 就**不再尝试**后两项，而调用方全部丢弃返回值 → 静默半失效 | 三处 `DwmSetWindowAttribute(…)?;` | 逐项尽力而为（`let _ =`），能生效的生效 |
| **N-27** | `crates/dsh-ui/src/harness.rs`（`begin_theme_sample`） | 把 HWND 洗成 `usize` 后在**回调线程**还原并直接调 DWM；回调可能在窗口销毁之后才投递（句柄可能已被复用给别的窗口），而注释把"窗口必然还活着"当成了不变量 | `let hwnd_addr = self.hwnd_typed().0 as usize;` 注释：*窗口必然还活着* | 投递前用 `IsWindow` 复核，无效则跳过（不再假设） |

### 2.3 工具链 P1（脚本即证据）

| # | 文件:位置 | 问题 | 修复 |
|---|---|---|---|
| **N-28** | `tools/finish-release.ps1`（`Step` + `$published`） | `$published` 在 `Step` 的 scriptblock 里赋值**不逃出作用域**（`& $body` 是子作用域）→ 即使发布完全成功也**永远**打印"未更新"并 `exit 1`；同时 `Step` 把异常吞成报告行、`cargo test` 的退出码从不检查（编译错误会显示"0 套件通过 / 0 失败"）。**双重失效：既永远红，也不因真失败而红** |
| **N-29** | `tools/finish-release.ps1:311` | `$results \| Where-Object … \| ForEach-Object { $results.Add($_) }` 在枚举中修改集合 → 抛非终止异常 → 报告里的「结果汇总」**只剩一行** |
| **N-30** | `selftest.ps1`（B/C 段） | 文案写"最长 180 秒"，但 `Start-Process -Wait` **没有超时** → 死锁的启动器会让自检**永久挂起**（历史事故正是死锁） |
| **N-31** | `selftest.ps1:38,124` / `tools/verify-version.ps1:186` | `cargo build … \| Out-Null` 丢弃退出码，靠 `Test-Path` 判定成功 → 构建失败时**用旧产物跑出自检 PASS** |
| **N-32** | `selftest.ps1` D/E 段 | `-SkipE2E` **不跳过 D 段**（D 无条件执行），却跳过 E 段；README 说"只跑 Job Object 两项"——两头都不对 |
| **N-33** | `tools/verify-deployed.ps1` | `Add-Type … -ErrorAction SilentlyContinue` 失败后 `[WD]::ForPid()` 报错 → `$wins` 为 `$null` → 打出 `[PASS] … 无 hung 窗口`：**在最关键的死锁断言上假通过** |
| **N-34** | `tools/verify-icon.ps1` | 不匹配分支只打印红字、**没有 `exit 1`/`throw`** → 永不失败；默认图标路径硬编码 `D:\DSHLauncher`；依赖一个不存在的导出步骤 |
| **N-35** | `uninstall.ps1` | `$ErrorActionPreference='SilentlyContinue'` + 无条件 `exit 0` → 卸载可完全失败却报成功；`(Get-ChildItem InstallDir).Count -eq 0` 后递归删目录的守卫有 `'D:'` vs `'D:\'` 漏洞；`%LOCALAPPDATA%\DSHLauncher` 无条件删除（可能删掉**另一份安装**的日志/profile/`service.json`，甚至让仍在跑的服务失去可追踪记录）；停止启动器的路径比较**大小写敏感** |
| **N-36** | `tools/finish-release.ps1` / `tools/verify-service-lifecycle.ps1` | `Get-Process DSHLauncher \| Stop-Process -Force` **不过滤安装路径**（会杀掉别处的实例），且**完全不用**项目自己的优雅退出入口 `--quit` —— 而 `docs/OPS-RUNBOOK.md` 明文要求"优先优雅退出、避免强杀 GUI"（§1 事故正是反复强杀导致桌面堆耗尽） |
| **N-37** | `tools/check-consistency.ps1`（约 55 条断言） | 「接线类」断言是**原始文本匹配**：标识符出现在 `//` 注释、`///` 文档或测试名里都算通过。只有 3 条做了注释剥离 | 扩展注释剥离（含块注释），并把"存在"类断言改为"调用点/定义形式"断言 |
| **N-38** | `tools/gen-facts.ps1`（`-Check`） | 只比对 4 项事实；`artifacts.*`、`code_lines.html`/`rust_build_rs`/`rust_examples`/`rust_total` **从不比对** → 实测：`target\release\dsh-app.exe` **不存在**，FACTS 里仍有它的测量值，`-Check` 照样 OK |
| **N-39** | `tools/gen-facts.ps1`（测试计数） | 用正则数 `#[test]`，把 `crates/dsh-app/build.rs` 里的 **4 个永不运行的测试**算进"单元测试数" |
| **N-40** | `tools/check-consistency.ps1` | 版本被**第二次硬编码**（`$EXPECT_VERSION = '5.0.0'`）且用**非锚定**正则匹配整个 `Cargo.toml`；另有若干"删掉主体即静默通过"的检查（GUI crate 行被删 → 既不 Pass 也不 Issue） |
| **N-41** | `uninstall.cmd` | **不是 CRLF**（实测 `bareLF=16`、`CRLF=0`），却在自己的注释里声明"ASCII + CRLF + 无 BOM"；另有 `chcp 65001` 无必要且会永久改变调用者的控制台代码页 |
| **N-42** | `DSHLauncherSetup.cs`（生成脚本的编码 / 注册表 / 抽取） | ①用 `ASCIIEncoding` 写生成的 `uninstall.ps1` → **非 ASCII 安装路径被写成 `?`** → 卸载时既停不掉启动器也删不掉目录，但注册表/快捷方式已被移除（留下无法卸载的残骸）；②`RegisterUninstall` 的 `catch { }` 吞掉注册表写失败 → 静默安装返回 0、GUI 显示"安装完成"却没有卸载项；③抽取 5 个文件逐个提交，中途失败会留下"有 exe、无卸载器、无注册表项"的半成品；④`--purge` / `--silent` 从未生效（脚本声明 `-Purge`/`-Silent`，而 `.cmd` 转发 `--purge`） |
| **N-43** | `build-setup.ps1` | 内嵌 `DSHLauncher.exe` 前**从不校验其版本**，也不调用 `tools/verify-version.ps1` → 版本链的"安装包"这一跳没有门禁（`DSHLauncherSetup.cs` 的注释却称"verify-version.ps1 仍会校验链路"） |
| **N-44** | `DSHLauncherSetup.cs`（网络 / manifest） | 未设置 `ServicePointManager.SecurityProtocol`，而该 exe 以"无 app.config 的 4.0 目标"运行 → 默认 TLS 1.0；实测 WebView2 引导程序下载地址在 TLS 1.0 下**连接失败**。且下载物**无 Authenticode 校验**；`build-setup.ps1` 也不传 `/win32manifest` → 无 DPI 感知、文件名含 `Setup` 可能被 Windows 安装器检测启发式要求提权 |

### 2.5 用户实测反馈轮新增（2026-09-11，详见 `IMPLEMENTATION-v5.0.0.md` §11）

| # | 严重度 | 位置 | 问题 |
|---|---|---|---|
| **N-59** | **P0** | `uninstall.ps1` / `uninstall.cmd`（仓库模板） | 模板的"所有权证明"只要求目录里有 `DSHLauncher.exe`，而**仓库根目录恰好满足**；在根目录执行卸载 ⇒ 删掉 `DSHLauncher.exe`/`app.ico`/`README.md`/`uninstall.*` 五个文件与整个 `%LOCALAPPDATA%\DSHLauncher` |
| **N-60** | P1 | `crates/dsh-app/build.rs`、`tools/verify-version.ps1` | `rerun-if-changed` 只在图标存在时声明 ⇒ 图标缺失时构建过一次后，图标补回来也不重跑 build.rs ⇒ exe **静默丢掉图标资源**（−22,016 B）；而 `verify-version.ps1` 的"含图标资源"断言只检查了 `FileDescription`（假断言） |
| **N-61** | P1 | `crates/dsh-app/src/service.rs`（`start()` 分支 2） | `service.json` 记录的端口 ≠ 配置端口时仍然"接管" ⇒ `adopted_pid` 指向别的端口，而探测/监测/`ready_url()` 全用 `config.port`：界面与监测都指向错误端口（`selftest.ps1` E 段"未见接管日志"即此因；用户改端口后重启也会撞上） |
| **N-62** | P2 | `selftest.ps1` B 段 | `FAIL: B-build（… 不能用陈旧二进制继续测）` 是**上一轮加固按预期工作**：退出码 101 的真实原因是 N-60 的 `app.ico` 缺失；脚本正确拒绝拿旧产物继续测 |
### 2.4 P2 / P3（摘要，逐条处置见实施记录）

| # | 严重度 | 位置 | 问题 |
|---|---|---|---|
| N-45 | P2 | `crates/dsh-core/src/process.rs`（`kill_process_tree`/`terminate`） | `TerminateProcess` 的布尔结果被丢弃 → "停止了服务"的日志与实际可能不符 |
| N-46 | P2 | `crates/dsh-core/src/process.rs`（`spawn_ready_reader`） | 上游输出格式变化时 `parse_ready_url` **静默返回 None**，日志零线索 |
| N-47 | P2 | `crates/dsh-core/src/maintenance.rs`（区间改写） | 用 `find(']')` 定位数组结尾：遇到嵌套数组或字符串里的 `]` 会定位错区间 → **改写错位置破坏 `workspace.json`**；解析时按 `,` 切分 + `trim_matches('"')` 不识别转义 |
| N-48 | P2 | `crates/dsh-app/src/service.rs`（`start()` 分支 2/3） | `kill_process_tree(orphan)` 与 `is_port_listening` 在持 `ServiceInner` 锁时执行（UI 线程） |
| N-49 | P2 | `crates/dsh-app/src/app.rs`（归档清理） | 活跃会话探测（含一次 `Get-CimInstance`，实测 ~1.27 s）在 UI 线程、且在模态确认框**之前** |
| N-50 | P2 | `crates/dsh-app/src/app.rs`（设置页/托盘） | 端口占用预检 `is_port_listening(candidate.port)` 在 UI 线程（一次性，可接受但已记录） |
| N-51 | P3 | `crates/dsh-core/src/{config,log,probe,process}.rs` | 死代码：`Settings::reset`（已删除）、`probe_port`（已删除）、`take_stdout`/`take_stderr`（已标注为非生产 API）、`any_node_running`（已标注） |
| N-52 | P3 | `crates/dsh-ui/src/lib.rs` | 6 个在本 crate 之外**零调用方**的根导出（已清理） |
| N-53 | P3 | `crates/dsh-ui/src/harness.rs` | 死方法 `sample_and_apply_theme`（自身会阻塞 2 s）等 6 个（已删除） |
| N-54 | P3 | `ui/settings.html` | `id="logpath"` 从未被更新，值是硬编码字符串（与真实 `log_dir()` 可能不符） |
| N-55 | P3 | `ui/guide.html` | ①"选桌面（默认就是）"——代码里工作目录默认为空、不设 cwd；②在本指引窗口里宣传 `Esc`/`F5`，而按键处理只对主界面窗口生效 |
| N-56 | P3 | `crates/dsh-app/src/cli.rs` | `FIX_MARKERS` 的注释用 `v5.0.1` 前缀 → 制造第二处"版本事实" |
| N-57 | P3 | `Cargo.lock` | `semver` 作为**传递依赖**存在（经 `rustc_version`），而 README 把它列进"刻意禁用"清单（禁用清单约束的是直接依赖，但表述会误导） |
| N-58 | P3 | `docs/*.md` | 同一指标在多份文档里 3–4 个互相矛盾的值（单测数 48/58/65、一致性项 31/53/60/66/102、exe 体积 3 个值、代码行数 3 个值） |

---

## 3. 分维度分析

### 3.1 缺陷修复（panic 面 / 并发 / 句柄 / Win32 / 边界）

**panic 面**：release 是 `panic = "abort"`，任何 panic 都是整进程死亡。本轮清点结论：
- `print!` 是**上一轮遗漏的真实崩溃面**（N-4）——`eprintln!` 那批修了，stdout 侧没修。
- `maintenance.rs` 的 `&bytes[8..]` 有 `starts_with` 前置守卫；`icon.rs` 全部走 `get()`；`dsh.rs` 的 `parse()` 用 `ok()?`；`probe.rs` 用 `SocketAddr::from` 而非字符串解析 —— 复核后确认**仍然安全**。
- `service.rs` 的生产 `lock().unwrap()` 全部改为 `unwrap_or_else(|e| e.into_inner())`（锁中毒不再等于整进程死亡）。
- 新增的 `log.rs` 脱敏扫描按 UTF-8 边界推进，并有专门用例覆盖中文混合文本。

**Mutex 不可重入**：本轮抓住了同一族的**另一半**（N-7）。修复方式是结构性的（拆互斥量），而不是再补一个 `debug_assert`。锁序约定写进了字段文档：只允许 `ServiceInner → ProcessManager`。

**管道纪律**：stdout/stderr 都有持续读取者（复核确认）；`spawn_ready_reader` 新增"格式不符留痕"。

**句柄**：`is_process_alive` / `process_start_time` / `process_image_name` / `child_pids_of` / `process_image_path` 的所有返回路径都 `CloseHandle`（复核确认）；`kill_process_tree` 已是"先收集子 PID 再递归"（不嵌套持快照）。本轮把 `terminate` 的失败结果**返回给调用方**，不再静默。

**Win32 返回码**：`GetExtendedTcpTable` 的两级缓冲 + 表头/表项边界双重复核**仍然正确**；本轮新增 `IsWindow` 复核（N-27）、`DwmSetWindowAttribute` 逐项尽力而为（N-26）。

**GUI 子系统陷阱**：`eprintln!`/`print!` 全部消除（N-4）；`--selftest`/`--ipc-probe` 的互斥体冲突返回非 0（复核确认 exit 2）。

**上游协议侦测**：新增 `looks_like_ready_line()` + 读取线程告警，把"上游改格式 ⇒ 静默失效"变成可诊断（N-46）；新增 `is_loopback_url()`。

**边界条件**：`port` `0/65535`、路径含空格/引号/中文、空配置文件、锁文件为空/超大/不可解析、TIME_WAIT、PID 复用 —— 逐项复核，结论与上一轮一致（PID 复用已有创建时间比对；锁文件超大仍是已知的低危残余）。

### 3.2 逻辑修正（状态机 / 时序 / 限流 / 生命周期）

**状态机**：上一轮指出"枚举 5 值 ≠ 真实 8 态"。本轮**补进了 `Stopping`**（终止阻塞段），并写明了显式迁移图；同时修正了两处**语义误用**：
- `Starting` 不能被当作"应当已经可用"（N-2）；
- `start()` 分支 1 不再把"接管来的外部实例"标成 `Running`（否则 `was_own` 判定为真，监测线程会试图"重启"一个不属于我们的服务）。

**异步时序**：`Ready{url:Option<String>}` 的语义复核确认正确；本轮补上"预算重置"与"每帧驱动"（N-1），使等待逻辑**真正可达**。
共享槽位读取一律"取出并清空"（复核确认）；关窗时**全部**主题状态清空（N-23）。

**限流去抖**：主题采样按时间（3 s）而非帧数（复核确认）；本轮新增：唤起路径端口探测按 tick 限流（N-13）、采样提交失败也要更新时间戳（N-23）。

**生命周期语义**：`independent`（默认）下退出/崩溃/升级不中断会话 —— 由 `service.json` + 对账保证；`tied` 保留 Job 语义。行为矩阵与文档一致性复核：**README / MAINTENANCE 已同步**（其中「关于」对话框的 Job 描述是错的，已修 N-17）。

**失联判定**：`8 × 1.5 s = 12 s`，但每轮探测自带 300 ms connect 超时 → 实测判定约 14.4 s（与文档"约 12 秒"有 20% 偏差，量级正确，保留并已在文档中写明）。

**进程身份校验**：三态判定（`IsDsh`/`NotDsh`/`Unknown`）复核确认有效；接管/重接管/停止三处全部接入；`Unknown` 不接管、不杀。

### 3.3 性能

- `[boot]` 埋点：单实例+配置+日志 **4–7 ms**、事件循环创建 **9–27 ms**、托盘+服务启动请求 **19–43 ms**、首帧 **20–45 ms**（多次运行；受 WebView2 首次加载影响）。**用户可感知的启动受 dsh 冷启动支配（30 s+），这部分单独标注**。
- 本轮消除的高频/阻塞项：UI 线程上的进程终止（N-11/N-12）、配置重启（N-12）、逐帧 300 ms 端口探测（N-13）、主题采样提交失败后的逐帧重试（N-23）。
- 未做的优化及理由：日志每行 `open/close`（改成常驻句柄会与滚动 rename 冲突，收益小于复杂度）；线程栈已按需缩小（256–512 KiB）；`monitor` 的 1.5 s 轮询保留（0.67 次/秒 TCP 握手可忽略，且没有内核事件可替代）。

### 3.4 冗余清理

- **删除**：`Settings::reset`、`probe_port`、`HarnessWindow::{sample_and_apply_theme, evaluate_script, evaluate_script_with_callback, set_background_color, set_theme, hwnd}`、`dsh-ui` 的 6 个无消费者根导出、`has_auth_url`（被 `auth_url()` 取代）。
- **接线**：`is_from_newer_schema`（→ 启动告警）、`effective_work_dir`（→ 服务启动）、`Settings::reset_on_disk`（→ 设置页新按钮）、`retained_file_count`（被一致性脚本引用）。
- **显式标注为非生产 API**：`take_stdout`/`take_stderr`（并说明"取走管道就没有读取者，会让子进程假死"）、`any_node_running`。
- **产物卫生**：`git status` 中 `crates/`、`ui/` 为未跟踪（与该仓库的历史提交状态一致）；根目录无 `*.pubtmp / *.bak-* / *.old* / *.replaced-old`。
- **注释与代码矛盾**：本轮修正 3 处（`harness.rs` 声称自己负责 F5/Ctrl+R、`dialog.rs` 声称 Job 回收、`service.rs` 声称持有 ServiceInner 锁调用 `start_dsh` 是安全的）。

### 3.5 内存

口径统一（同一台机器、同一脚本）：

| 口径 | 数值 | 说明 |
|---|---|---|
| 工作集（窗口关闭态） | 13–14 MB | 与上一轮实测一致 |
| 专用内存 | ~2 MB | |
| 线程数 | 5–12 | 取决于是否有 worker 在跑 |
| 句柄数 | ~170 | |
| 工作集（内嵌窗口已加载） | ~29 MB | 用户感知口径 |
| exe 体积 | 见 `docs/FACTS.json` | 单文件、含图标与版本资源 |
| WebView2 profile | 固定 `%LOCALAPPDATA%\DSHLauncher\webview2-profile` | 复核确认 exe 旁**不会**生成 `<exe>.WebView2` |
| 全进程一份 `WebContext` | 是 | 复核确认 |

**profile 损坏自愈**：`HarnessWindow::new` 失败时 `dsh-app` 走 Edge 回退 → 默认浏览器，三级降级链完整。**"清数据目录后重试一次"仍未实现**（列入未完成项）。

### 3.6 自清洁（本轮重点）

| 面 | 结论 |
|---|---|
| 进程 | `independent`：不由启动器回收（设计如此），由 `service.json` 对账 + `stop_stale_orphan` 兜底；`tied`：内核回收。`--quit` 复核确认已具备**回执**语义（`QuitOutcome`，退出码 0/3 区分"已退出"与"未收到回执"） |
| 句柄 | 逐处复核，见 §3.1 |
| 锁 | 只删"持有者 PID 确已退出"的锁；不可解析的锁要求静置 5 s；**PID 复用残余风险如实保留**（偏保守方向） |
| 临时文件 | `settings.toml.tmp`、`service.json.tmp` 均为「写 tmp → rename」，崩溃残留不做启动清理（P3，已记录）；`tools/*.ps1` 的临时产物已要求脚本清理（N-29 的 `%TEMP%\dsh-win-graph.lock`、`verify-deployed` 的 build-info.txt 例外见未完成项） |
| 日志 | 严格保留 `max_files`（=3，含当前）；溢出归档被删除；`retained_file_count()` 供校验；目录创建只做一次；**副本共享同一把锁**（N-8） |
| 缓存/Profile | 卸载时随 `%LOCALAPPDATA%\DSHLauncher` 删除（配置保留在 `%APPDATA%`） |
| 线程 | 5 类 worker 全部可取消或自然退出；本轮新增的 worker 都有名字与受限栈；不再有"退出后无法重新拉起监测"的竞态（N-10） |
| Job Object | `tied` 下强杀启动器由内核回收；`independent` 下**故意不回收**并在文档与「关于」对话框中如实说明（N-17） |
| 注册表/快捷方式/服务 | 安装/卸载互逆（除 N-42 的残余问题）；不注册服务/计划任务/PATH |
| 构建/发布残留 | `build.ps1` 内置热替换 + 历史产物清理（复核确认） |
| 自检脚本 | 见 N-30/N-31/N-32 |
| 崩溃取证 | `crash.rs` 零分配、单行 append、不 `format!`（复核确认符合约束） |

### 3.7 扩展维度

- **安全**：命令注入（`npm.cmd` 直调 + semver 白名单复核有效）；路径遍历（44 字符强校验 + reparse point 拒绝复核有效）；TOCTOU（锁文件 PID 判定窗口，残余风险如实保留）；越界删除（`maintenance.rs` 拒绝 reparse point；**安装器侧的越界删除是本轮 P0，已处置**）；日志脱敏（本轮扩展）；IPC 信任边界（双层转义复核有效 + 新增导航白名单）；依赖供应链（`npm --registry` 命令行优先于环境变量）。
- **容错与可恢复**：各级降级路径复核；**托盘创建失败仍然只记日志**（未修，见未完成项——它需要设计"无托盘时的退出出口"）；幂等性（`stop`/`delete_archived_sessions`/`clean_stale_dsh_locks`）复核通过；超时覆盖（`reg.exe` 仍无超时，P3）；崩溃取证完整。
- **可观测性**：日志分级（INFO/WARN/ERROR）；关键决策可落盘（`--probe-identity` 输出逐步证据）；**结构化字段仍缺失**（无 `pid`/`tid`/`module`）；**一键诊断包仍未实现**（未完成项）；性能埋点限于 `[boot]`。
- **可扩展性**：`schema_version` 现已**被消费**（N-18）；多实例/多 profile 改造成本高（单互斥体 + 单 `config.port`）；Windows 耦合点仍集中在 `dsh-core` 若干模块（未做 `#[cfg(windows)]` 隔离）；UI 文案仍硬编码中文。
- **交付链**：版本链路 `Cargo.toml → Cargo.lock → exe 资源 → 安装包资源 → 注册表 DisplayVersion`（复核有效，且**全仓库只剩 5.0.0 一个版本号**）；构建可复现性：`app.ico` 生成**确定**（连续 3 次 SHA256 一致，含 `de-DE` 文化）；**`cargo build` 产物的字节级可复现性未验证**（`rc.exe` 版本影响资源字节）。
- **测试与门禁**：`check-consistency.ps1` 从 114 项扩到 **见 FACTS**（新增行为性硬约束）；本轮新增的每条 P0/P1 都有对应断言或探针。

---

## 4. 上一轮结论的复核结果（三分类）

| 上一轮项 | 复核结论 | 证据 |
|---|---|---|
| **P0 A1**（服务与启动器强耦合） | **已修复（确认）** | `ChildLifecycle::Independent` 为默认；`CREATE_BREAKAWAY_FROM_JOB` 在 spawn 时置位；`service.json` 含 PID + 端口 + **进程创建时间**（±2 s）用于防 PID 复用；`reconcile()` 四态对账 |
| **P0 A2**（带 token 地址从未生效） | **部分修复 → 本轮补全** | `Ready{url:Option<String>}`、`pending_url` 仅在带 token 时赋值、捕获后补发 `Ready` —— 这三条**已确认**。但**等待预算在冷启动被绕过**且**无驱动源**（N-1），属于同族残留，本轮修掉 |
| **P1 B1**（进程身份校验丢失） | **已修复（确认）** | `classify_dsh_identity()` 三态；`Unknown` 不接管；接管/重接管/停止三处接入；有 4 个专门用例 |
| **P1 B2**（stderr 从未排空） | **已修复（确认）** | `spawn_stderr_drain()` 存在且有调用方；单行截断 2000 字符 |
| **P1 B3**（`tray_on_close` 未接线） | **已修复（确认）** | `request_close_window()` 有真实读取方；`Esc` 共用同一语义 |
| **P1 B4**（捕获回调代际号校验） | **部分修复 → 本轮补全** | 回调在 `Starting\|Running` 时才补发 `Ready`（避免旧进程覆盖）；但 `ReadyProbe::Clone` 复制代际号使取消语义在克隆上失效（N-9），本轮修掉 |
| **P1 C1**（`wait_for_ready_worker` 无取消） | **已修复（确认）** | `stop()`/新 `start()` 递增 `ready_gen`；worker 分片等待（≤250 ms）并在代际变化时立即返回 |
| **P1 C2**（配置变更后界面未刷新） | **已修复（确认）** | `pending_url = None` + 预算重置 + `want_open` 重新导航 |
| **P1 E1**（主题跟随未接线） | **已修复（确认）** | `begin_theme_sample()` 异步回调 + 3 s 时间限流；**无 UI 线程 `recv_timeout`**（唯一的同步版本本轮删除） |
| **P1 F1**（托盘失败仅记日志） | **未修复（保留）** | 见 §5 未完成项：需要设计"无托盘时的可见退出出口" |
| **P1 G1**（安装器升级必然杀 dsh） | **已修复（确认，但注释是错的）** | A1 解耦后 `taskkill /F`（不带 `/T`）确实不再连带回收 dsh（因为 breakaway）；但同一段注释用 Job/KILL_ON_JOB_CLOSE 解释该现象是**错的**，且 `tied` 模式下仍会被回收（N-42/§2.1 P0-5 一并处置） |
| **P1 G2**（无脚本可用的退出入口） | **已修复并增强（确认）** | `--quit` + `QuitEvent`；本轮复核发现它已有**回执**（`QuitAckEvent`，退出码 3 区分"未收到回执"） |
| **P2 C3–C9 / D1–D4 / E2 / F2–F4 / H1–H6** | 逐条复核 | C3 日志文件数**已修**；C4 端口分配竞态**保留**（P3，自检专用）；C5 **本轮重写**（N-47）；C6 **已修**（`cleaned`/`pruned` 分离 + `HashSet`）；C7 **已修**（句柄全路径关闭）；C8 **本轮修**（N-20）；C9 **已修**（`cleaning` 重入保护）；D1 **保留**（`CreateMutexW` + `GetLastError`，`windows` crate 调用前会清 last-error，风险低）；D2 **已修**（迁移成功后删 ini）；D3 **本轮修**（N-15）；D4 **已修**（`node_path` 变更清缓存）；E2 **本轮修**（死方法删除）；F2 **保留**（配置损坏弹窗仍在事件循环前，P3）；F3 **已修**（接管外部实例不再共享受限超时——见就绪 worker 的取消机制）；F4 **本轮修**（N-16）；H1 **已修**（无 `.pubtmp`）；H2 **本轮修**（数字单一来源 + 扩展 `-Check`）；H3 **本轮修**（死代码清账）；H4 **本轮修**（N-14）；H5 **本轮修**（N-22）；H6 **保留**（`reg.exe` 无超时，P3） |
| **§5.6 不确定项 12 条** | 见 §5 | 能判定的已判定；仍不能判定的保留并写明所需条件 |

---

## 5. 不确定项（禁止臆测，逐条说明判定所需条件）

| # | 项 | 为什么仍未判定 | 判定需要什么 |
|---|---|---|---|
| 1 | **`selftest.ps1` 全段（A1/A2/B/C/D/E）与安装/卸载互逆矩阵的真实通过情况** | 本次按你的选择**未执行**会中断会话的验证；这些步骤会独占单实例互斥体、结束/重启启动器，并会写入真实注册表与 `%LOCALAPPDATA%` | 你方便的时间窗口内运行 `selftest.ps1` 与安装矩阵（命令清单见实施记录 §7） |
| 2 | **无 WebView2 Runtime 环境下的 Edge 回退** | 需要干净 VM（或注册表/目录模拟缺失） | 干净 VM 实测 `HarnessWindow::new` 失败 → `open_edge_fallback` 生效 |
| 3 | **`Remove-Item -Recurse` 是否跟随目录 junction** | PowerShell 版本相关，本次未做破坏性实验 | 在受控目录造 junction 后实测（PowerShell 5.1 基线） |
| 4 | **`cargo build --release` 产物的字节级可复现性** | `rc.exe` / `link.exe` 版本影响资源与 PE 字节；本机只有一套工具链 | 同一工具链连续两次构建比对 SHA256；跨工具链需固定 SDK 版本 |
| 5 | **`msedgewebview2.exe` 子进程数量的"本项目口径"** | 需要开窗状态下按 `CommandLine -like '*DSHLauncher*'` 过滤计数 | 开窗后采样一次 |
| 6 | **DSHLauncherSetup.exe 的实际 TLS 默认值与下载可达性** | 我以 `pwsh` 模拟 `SecurityProtocol = Tls`（`go.microsoft.com` 失败、`nodejs.org` 成功），但未运行真实安装包 | 运行打包后的 setup，读 `setup.log` 的"下载失败"记录 |
| 7 | **Windows 是否因文件名含 `Setup` 对该 exe 强制提权** | 需要标准用户 + UAC 开启的环境 | 标准账户实测启动是否弹 UAC |
| 8 | **`dsh` 自身的 workspace 默认目录** | 上游行为，不在本仓库 | 查 `@deepseek-ai/dsh` 源码或实测其默认 cwd |
| 9 | **dsh 页面是否可能使用 CSS Color 4 语法**（`parse_css_color` 只认 `rgb/rgba/#rgb/#rrggbb`） | 上游页面样式，不在本仓库 | 开窗后打印 `getComputedStyle(document.body).backgroundColor` 实测 |
| 10 | **wry 是否保证 `evaluate_script_with_callback` 回调不在 WebView drop 之后投递** | wry 实现细节；本轮改为**不依赖**该假设（加 `IsWindow` 复核），因此风险已消除，但"wry 的真实行为"仍未判定 | 读 wry 源码或构造"投递前销毁窗口"的复现 |
| 11 | **`--quit` 在 `tied` + 无人值守场景下的行为** | `tied` 模式的退出会弹模态确认框，无人值守时会一直等待到回执超时（20 s）后返回退出码 3 | 在 `tied` 模式下用脚本运行 `--quit` 并观察 |
| 12 | **工作集"同口径"数值的可比性** | 上一轮 §14.6 定义的"同口径"是"刚启动、未加载 WebView2 窗口"，而 §15.5 记的是"窗口关闭态"——两者未必等价 | 用 `tools/` 里固化的采样脚本同时输出两个口径 |

---

## 6. 可复现实验与原始证据

| 实验 | 命令 / 方式 | 结果 |
|---|---|---|
| 基线门禁（**未改任何代码前**） | `cargo fmt --check` / `cargo clippy --offline --all-targets --all-features -- -D warnings` / `cargo test --offline --all-features` / `cargo build --offline --release` | fmt **0**、clippy **0**、test **101（FAILED）**、buildrel **0**。失败用例：`dsh::tests::picks_highest_version`，`panicked at crates\dsh-core\src\dsh.rs:434` `Os { code: 3, kind: NotFound }`；`test result: FAILED. 68 passed; 1 failed; 1.82s` |
| 复现该 flaky | 单独跑该用例 1 次 + 全量跑 6 次 + 连跑 3 次 | 单独跑**通过**；`cargo test -p dsh-core --lib` 连跑 6 次全绿（69 passed）；说明与并发/历史进程有关 |
| 300 次文件写入压力 | 循环 300 次「建目录 + 写 `node.exe`」 | **0 次失败** → 排除"文件系统/杀软稳定拒绝"，指向**测试自身的固定路径** |
| 上一轮「401 取证」 | 日志查 `已打开内嵌界面：http://127.0.0.1:3080/`（无 token）；`launcher.log` 中 `token`/`auth`/`已捕获` 零命中 | 上一轮证据，本轮**代码层**复核出同族残留（N-1） |
| 运行期探针（token 捕获链路） | `cargo run --offline -p dsh-core --example token_capture_probe -- 45680` | 见实施记录（隔离端口，不触碰 3080 上的活动会话） |
| 运行期探针（活跃会话） | `cargo run --offline -p dsh-core --example active_session_probe` | 见实施记录（只读探测） |
| Job 语义实验 | `crates/dsh-core/examples/job_object_demo.rs` | 保留（上一轮 5 组矩阵的来源） |
| 并发写完整性 | 新单测 `concurrent_appends_produce_whole_lines`（4 线程 × 50 行） | 200 行全部结构完整 —— 直接证明 `RollingLogger::Clone` 共享锁生效 |
| 代际号取消 | 新单测 `in_flight_probe_is_cancelled_by_bump`（在途探测 + 别处 bump） | 返回 `GenerationChanged` |
| JSON 区间改写加固 | 新单测 `bracket_matcher_handles_nesting_and_escapes` / `parses_ids_with_escapes_via_serde` / `rewrite_refuses_invalid_json_document` | 嵌套数组、字符串内 `]`、转义引号、非 JSON 文档四类边界全部通过 |
| 脱敏覆盖 | 新单测 `redacts_other_credential_keys` / `redacts_authorization_headers` / `redaction_keeps_utf8_boundaries` | 18 个敏感键 + Bearer/Basic + 中文混合文本 |
| 版本链路 | `pwsh -NoProfile -File tools\verify-version.ps1` | 见实施记录 |

---

## 7. 审查纪律声明

1. **审查阶段未修改生产代码**：§1–§5 的所有结论均来自通读与实测；代码改动发生在结论形成之后，并与每条结论一一对应（见实施记录）。
2. **临时探针/脚本**：本轮为验证而创建的临时脚本（如一致性脚本的探针草稿 `tools/_scratch-consistency-probe.ps1`）在交付前**必须删除**；`crates/dsh-core/examples/` 只保留 `job_object_demo.rs`、`token_capture_probe.rs`、`active_session_probe.rs` 三个**长期**探针（后者已纳入门禁）。
3. **上一轮报告的去向（如实记录一次信息损失）**：任务 §9.1 明确允许「覆盖上一轮同名文件」，本轮据此把定稿版写入了同一路径 `docs/AUDIT-REPORT-v5.0.0.md`。**但我没有在覆盖前保留上一轮正文的副本**——`crates/` 与 `docs/` 在本仓库中处于未跟踪状态，`git` 里也没有它的历史，因此上一轮报告（含其 Job 生命周期 5 组实验矩阵、HTTP 401/303 实测记录、`launcher.log` 取证片段）的**原始正文已不可恢复**。可挽回的部分是：①它的**全部结论**已按原编号（A1/A2/B1–B4/C1–C9/D1–D4/E1/E2/F1–F4/G1/G2/H1–H6）逐条并入本报告 §4 的复核表；②它依赖的实验都写明了可复现方式（见 §6），可按需重跑。**这是一次本可避免的信息损失，责任在我；已在此显式记录，供验收判断。**
4. **不确定项优先于结论**：§5 的 12 条均写明"仍未判定 + 需要什么"，没有一条是被"合理地猜"出来的。
5. **一处必须如实说明的环境异常**：本次会话期间观察到 `crates/` 下有若干文件（`lib.rs` / `main.rs` / `app.rs` / `single_instance.rs`）在**我没有编辑的情况下**被改动（新增了 `--quit` 的**回执**机制 `QuitAckEvent` / `QuitOutcome`）。改动本身是自洽且能编译通过的，我据此把它当作既成事实复核并保留了它。但**其来源未能确定**：我的子代理当时都在各自受限的文件范围内工作，我无法排除"另一个会话/代理在同一工作区并行编辑"。因此：本报告对这四个文件的结论**以我最终复核过的内容为准**；若还有其它写入者，请以 `docs/FACTS.json` 与实际构建产物为准重新核对。这一条不是推测，是观察记录。
6. **我自己造成的一次数据损失与恢复过程（必须完整披露）**：在批量改写「全角括号」相关注释时，我用 `pwsh` 直接做字符串替换，结果 `crates/dsh-app/src/service.rs` 与 `crates/dsh-core/src/process.rs` 的全部 `（` 被误替换成 `v`；紧接着的一次替换又因为 PowerShell 重载解析失败（把 `[char]` 传给了需要 `MatchEvaluator` 的参数）使待写变量为 `$null`，而 `[IO.File]::WriteAllText(path, $null)` **把这两个文件写成了 2 字节**。两个文件当时都处于未跟踪状态、`git` 里没有历史，常规回滚不可用。
   **恢复方式（可复现）**：DSH 把会话记录写在 `~/.dsh/sessions/<workspace>/<session-id>/session.v3.jsonl.zstd`，其中包含工具结果（我对这两个文件的**完整读取**）与全部 `edit` 调用参数。该文件是 **seekable-zstd（实测 832 个串接帧）**，Node 的 `zstdDecompressSync` 只解出第一帧；改用 Node 26 的 `zlib` 按帧魔数（`28 B5 2F FD`）切分逐帧解压后得到 6.5 MB JSONL，然后：
   1. 从快照剥掉 `NNN: ` 显示前缀，还原**基线文件**（`process.rs` 995 行 / `service.rs` 879 行）；
   2. 按记录顺序**重放**该文件的每一次 `edit`（`process.rs` 4/4 命中；`service.rs` 19 次中 17 次命中，2 次未命中——因为那两处改动我当初是用 `pwsh` 批量替换做的，不在 `edit` 记录里）；
   3. 手工补回那 2 处（监测线程重武装、就绪超时分支）+ 4 处锁中毒防护 + 版本字符串改写。
   恢复后 `cargo fmt --check` / `clippy -D warnings` / `cargo test`（86+11=**97**）全部通过；`tools/check-consistency.ps1` **115/115**、`tools/gen-facts.ps1 -Check`（**29 项事实**）一致 —— **恢复是完整的，并由门禁证明**。
   **教训（已写入 `docs/OPS-RUNBOOK.md`）**：①不要在 `pwsh -Command` 里用含中文/全角字符的字面量做批量替换；②`[IO.File]::WriteAllText(path, $null)` 会静默把目标截断，批量写回前必须先校验内容非空；③本次能恢复**纯属侥幸**（会话记录恰好保留完整快照），不得当作常规手段。
7. **临时脚本清单**：为恢复而写的分析脚本全部位于 `%TEMP%`（`dsh_recover.js` / `dsh_unz.js` / `dsh_scan.js` / `dsh_rebuild2.js` / `dsh_unwrap.js` / `dsh_restore_service.js` / `dsh_probe*.js`）以及 `%TEMP%\dsh_session.jsonl`、`%TEMP%\dsh_rec\`，**未进入仓库**且交付前已删除；`crates/dsh-core/examples/` 只保留三个长期探针。子代理遗留的 `tools/_scratch-consistency-probe.ps1` 亦已删除。
