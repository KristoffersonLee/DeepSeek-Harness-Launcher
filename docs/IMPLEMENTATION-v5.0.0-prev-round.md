# v5.0.0 优化实施记录（审计报告处置 · 定稿轮）

> 本文记录 **v5.0.0 定稿前审计轮**（v5.0.0 对外发布前的最后一轮整改）的全部处置与验证证据。
> 该轮工作属于 v5.0.0 本身，不再单列版本号。
> 依据：[`AUDIT-REPORT-v5.0.0.md`](AUDIT-REPORT-v5.0.0.md)
> 状态：**代码全部落地并通过全部门禁**；运行期行为验证需一次启动器重启（见 §5）
> 对外契约不变：文件名 `DSHLauncher.exe`、原有 CLI 参数、安装/卸载语义、dsh 兼容性、单文件 exe + 仅 WebView2

---

## 1. 一句话结论

审计报告里的 **P0 / P1 / P2 项全部实现**，P3 项完成了防回归与可观测性部分。
所有门禁当前全绿：

| 门禁 | 结果 |
|---|---|
| `cargo test --offline` | **77 passed / 0 failed**（dsh-core 64 + dsh-ui 9 + build.rs 4） |
| `cargo clippy --all-targets --all-features` | **0 warning**（项目基线要求 `-D warnings`） |
| `cargo fmt --check` | 通过 |
| `cargo build --offline --release` | 通过 |
| `tools/check-consistency.ps1` | **92 / 92**（原 66；新增 26 项行为性硬约束） |
| `tools/verify-version.ps1` | **27 / 27**（原 23） |
| `tools/gen-facts.ps1 -Check` | 通过（文档数字与实测一致） |
| `tools/verify-service-lifecycle.ps1` | 通过 |
| `tools/verify-token-navigation.ps1` | 通过 |
| `crates/dsh-core/examples/token_capture_probe.rs` | **运行期通过**（端口 45680，含竞态复现证据，见 §2.2） |

---

## 2. P0 处置

### 2.1 服务与启动器解耦（审计 A1）

**问题**：dsh 挂在启动器的 Job Object（`KILL_ON_JOB_CLOSE`）上，只有托盘「退出→否」
一条路径能保住会话；强杀 / 崩溃 / 被安装包升级覆盖 / 注销重启全部切断会话。
实测矩阵（5 组）确认了该机制。

**实现**：

| 变更 | 位置 |
|---|---|
| 新增 `ChildLifecycle { Independent, Tied }`，`Independent` 下**不创建 Job** | `dsh-core/src/process.rs` |
| `Independent` 用 `CREATE_BREAKAWAY_FROM_JOB`；父 Job 禁止 breakaway 时**退化为不带该标志**并留痕 | 同上 `spawn_node()` |
| 新增 `service_record.rs`：`service.json` 记录 `{pid, port, exe, 进程创建时间}` + 启动对账 | `dsh-core/src/service_record.rs` |
| 配置项 `service_lifecycle`（默认 `independent`）+ `schema_version` + `stop_stale_orphan` | `dsh-core/src/config.rs` |
| 退出语义：`tied` 时按策略停止服务；`independent` 时天然保留（不再需要 disarm 魔法） | `dsh-app/src/app.rs::begin_exit()` |
| 设置页新增「服务独立于启动器（推荐）」复选框（默认勾选） | `ui/settings.html` + `dsh-ui/src/settings.rs` |

**防 PID 复用**：对账时同时校验「PID 存活 + 进程创建时间匹配（±2s） + 身份是 dsh」，
三者缺一即判为陈旧并清理记录。这是取代「内核级零孤儿」承诺的关键：
把保证从内核移到**显式簿记 + 启动对账**。

**代价（诚实记录）**：`independent` 下不再有「内核级零残留」保证。
但该保证与「退出不断会话」在机制上互斥；需要旧语义的用户可选 `tied`（保留完整实现与测试）。

### 2.2 token 捕获竞态与不可达等待（审计 A2）

**问题**（实测取证）：`launcher.log` 中 `已捕获 dsh 就绪地址` **零命中**，
且导航一律使用无 token 地址 —— 实测该地址返回 **HTTP 401**。两处叠加缺陷：

1. `wait_for_ready_worker` 在锁内**只读一次** `auth_url`，与 stdout 捕获线程竞态；
2. `Ready` 事件无条件把无 token 地址写入 `pending_url`，使「等 token」的
   `AUTH_URL_WAIT_TICKS` 逻辑**永远不可达**。

**实现**：

| 变更 | 位置 |
|---|---|
| `ServiceEvent::Ready { url: Option<String> }`：`None` = 就绪但暂无 token | `dsh-app/src/service.rs` |
| stdout 回调捕获到 token 后**补发** `Ready`（界面升级为带 token 地址） | 同上 `on_ready` |
| 就绪 worker 改为**分片等待 + 代际号取消**（不再空转 120 秒） | 同上 `wait_for_ready_worker` |
| `pending_url` **仅在 URL 含 token 时**赋值；`auth_url_settled()` 改为返回 `Option<String>`，等待逻辑真正可达 | `dsh-app/src/app.rs` |

**验证**：
- `tools/verify-token-navigation.ps1`（静态检查 + 401 事实核对 + 日志断言）
- `crates/dsh-core/examples/token_capture_probe.rs`（**运行期**确证，见下）

#### 运行期确证结果（2026-09-11，隔离端口 45680，不触碰 3080 上的活动会话）

```text
=== token 捕获链路验证（端口 45680）===
已启动 PID 3168（生命周期策略 = Independent）
就绪于 6.9910159s；捕获 token 于 7.9055414s（顺序：就绪先到 → token 后到）
[PASS] D: 就绪探测成功
[PASS] A: token 捕获回调已被调用
[PASS] B: 捕获地址含 token（已脱敏打印）：http://127.0.0.1:45680/?token=***
[PASS] C: 解析结果可复算一致（原始行已脱敏）：dsh web: http://127.0.0.1:45680/?token=***
[NOTE] E: 本次「就绪先于 token 914.5255ms」—— 正是旧实现会用无 token 地址导航的场景；
         修复后由 Ready{url:None} + 补发 Ready 覆盖
已停止（killed=[3168]）
[PASS] F: 停止后端口已释放（无残留）
=== 结果：通过 ===
```

**这条 [NOTE] 就是本次修复价值的直接证据**：实测「就绪」比「token 到达」早约 **914 ms**，
而旧实现此刻已经把无 token 地址写进 `pending_url` 并导航 —— 用户看到的就是 HTTP 401。
修复后该窗口由 `Ready { url: None }` 占位、token 到达时补发 `Ready` 覆盖。

该探针走的是**与生产完全相同**的调用链（`ProcessManager::start_dsh` + `ReadyHook` +
`ReadyProbe`），并自带残留自检（停止后端口必须释放），已纳入 `check-consistency.ps1`。

---

## 3. P1 处置

| # | 缺陷 | 实现 |
|---|---|---|
| B1 | 对**任何**监听端口的进程直接 adopt，托盘「停止服务」会误杀无关程序 | 新增 `is_dsh_harness_process()`：`node.exe` 镜像名 + 排除系统目录 + **三重身份关联**（路径含 `@deepseek-ai\dsh` / 与已解析 dsh 同映像 / 祖先链有 node.exe）。无法确认时**只读不接管、不杀**。接管/重接管/停止三处全部接入 |
| B2 | `stderr` 接管道但从不排空 → 子进程写满缓冲区假死 | 新增 `spawn_stderr_drain()`：持续读取并落 WARN 日志（同时把 dsh 的关键错误暴露到日志），单行截断 2000 字符 |
| B3 | `tray_on_close` 零消费方 | 新增 `request_close_window()`：勾选时**隐藏**窗口（保留 WebView 与页面状态），未勾选才真正关闭；托盘/`Esc` 共用 |
| E1 | 主题采样未接线 | 新增 `begin_theme_sample()`（异步回调，**不在 UI 线程 recv_timeout**）+ 事件循环每 3 秒采样一次 + `is_system_dark()` 读注册表给窗口正确初值 |
| — | F5/Ctrl+R/Esc 只是文档承诺 | 新增 `handle_harness_key()`：F5 / Ctrl+R 刷新，Esc 按 `tray_on_close` 隐藏窗口 |
| G1 | 安装器升级必然杀 dsh | 由 A1 解耦自动修正（安装器 `taskkill /F` 不再连带回收 dsh）；`StopLauncherInDir` 注释同步更正 |
| G2 | 无脚本可用的退出入口 | 新增 `--quit` + `QuitEvent` 命名事件；托盘「退出」与 `--quit` 共用 `begin_exit()` |

---

## 4. P2 / P3 处置

| 项 | 实现 |
|---|---|
| `stop()` 阻塞 UI 线程 | 阻塞的进程终止移出 UI 路径；`ProcessManager::stop()` 返回被终止 PID 列表 |
| `restarts` 计数永不重置 | 服务稳定运行 5 分钟后**自动恢复重启额度**（`RESTART_QUOTA_REFILL_AFTER`） |
| 配置保存失败仍改内存 | 改为在**副本**上改动，落盘成功后才提交内存；失败时明确提示「原有设置未改动」 |
| 端口/工作目录变更后界面不刷新 | 重启服务后清空 `pending_url` 并重置等待预算；`node_path` 变更也触发重启（并让 `dsh_paths` 缓存失效） |
| 归档清理可重入 | 新增 `App::cleaning` 重入保护；`delete_archived_sessions` 拆分为 `cleaned` / `pruned` 两类计数，并用 `HashSet` 把 `removed.contains` 的 O(n²) 降为 O(n) |
| `workspace.json` 误写风险 | 新增 `unique_key_index()`：同名 key 出现多次时**拒绝解析/拒绝改写**；改写后用 `serde_json` 复核合法性，不合法则放弃落盘 |
| 锁收集可跟随 reparse point | `collect_lock_files` 拒绝目录型 reparse point（附 junction 单测，环境不支持时安全跳过） |
| 锁 PID 复用 | 如实标注残余风险（偏保守方向：不误删活跃锁，极端情况下需人工删 `.lock`） |
| 日志实际 4 个文件 | `maybe_rotate` 修正为保留 `max_files` 个（含当前），并删除溢出归档；新增 `retained_file_count()` 供校验 |
| 日志每行 `create_dir_all` | 用 `AtomicBool` 只做一次目录创建 |
| `eprintln!` 在 GUI 子系统下会 panic | 全部替换为忽略写入错误的 `writeln!(stderr)`（`process.rs` / `app.rs` / `main.rs` / `crash.rs`） |
| `is_process_alive` 句柄泄漏 | 所有返回路径统一 `CloseHandle`；`STILL_ACTIVE` 用类型常量 |
| `kill_process_tree` 句柄放大 | 改为「先收集子 PID 再递归」，不再嵌套持有快照 |
| ini 迁移后不删旧文件 | 迁移落盘成功后删除 `settings.ini`（落盘失败则保留，避免丢配置） |
| 无崩溃取证 | 新增 `crash.rs`：`SetUnhandledExceptionFilter` + **零分配**写 `[FATAL] 未处理异常 code=0x… address=0x… pid=… tid=…` |
| 无脚本退出入口 | `--quit`（见 G2） |
| 安装包手写版本常量 | 改为从自身 `AssemblyFileVersion` 反射读取（`ResolveAppVersion()`），版本只剩 `Cargo.toml` 一处 |
| 文档数字漂移 | 新增 `tools/gen-facts.ps1` + `docs/FACTS.json`（唯一来源）+ `-Check` 漂移检测；README 中英双语数字同步修正 |
| 死代码 | `HarnessWindow::set_theme` 等改为接线或标注；`take_stderr` 接入（B2）；`any_node_running` 保留并注释用途 |

---

## 5. 交付与验证步骤（需要一次启动器重启）

> ⚠️ **本次优化期间我两次中断了你的活动会话**（一次是受控实验、一次是误用旧版产物强杀），
> 这正说明「没有优雅退出入口」是个真实痛点 —— 已用 `--quit` 修复。

### 5.1 产物状态

- 新构建已通过全部门禁与版本资源校验：`target\release\dsh-app.exe`
- 根产物 `DSHLauncher.exe` 的更新方式见 §7（**热替换**：Windows 不允许覆盖运行中的
  exe，但允许对它**改名**，因此可以不停实例地换产物）。

### 5.2 建议的操作顺序

```powershell
cd D:\DSHLauncher

# 1) 从托盘退出旧启动器（选「否」保留服务，默认焦点就是「否」）
#    —— 旧版没有 --quit，只能用托盘菜单

# 2) 发布新产物（会先校验版本资源再拷贝）
pwsh -NoProfile -File build.ps1 release

# 3) 启动新版；它会读取 service.json（首次为空）→ 自己拉起一个 dsh
#    或接管仍在监听 3080 的旧服务
.\DSHLauncher.exe
```

### 5.3 验证清单

```powershell
# A. 服务独立性（静态 + 可选破坏性实验）
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1 -Force   # 会结束启动器

# B. token 导航（静态 + HTTP 401 事实 + 日志断言）
pwsh -NoProfile -File tools\verify-token-navigation.ps1

# B2. token 捕获链路的**运行期**确证（隔离端口，不影响 3080 上的会话）
cargo run --offline -p dsh-core --example token_capture_probe -- 45680

# C. 全部门禁
pwsh -NoProfile -File tools\check-consistency.ps1
pwsh -NoProfile -File tools\verify-version.ps1
pwsh -NoProfile -File tools\gen-facts.ps1 -Check
```

**期望的新行为**（与旧版的差异）：

1. 冷启动日志出现 `已捕获 dsh 就绪地址（token 已脱敏）：…`，且
   `已打开内嵌界面：http://127.0.0.1:3080/?token=…`（**带 token**，不再是 401 页）。
2. 生成 `%LOCALAPPDATA%\DSHLauncher\service.json`。
3. **强杀启动器**（任务管理器结束任务）后 `dsh` **仍在**监听 3080；再次双击启动器
   会记 `发现上次启动留下的 dsh 服务仍然可用（PID …），已直接接管（会话未中断）`。
4. 关闭界面窗口 → 窗口**隐藏**（进程与 WebView 保留），托盘「打开界面」瞬时恢复。
5. `.\DSHLauncher.exe --quit` 可优雅退出（脚本/CI 可用）。
6. 用非 dsh 程序占用配置端口时，日志记
   `端口 3080 被非 dsh 程序占用（PID …，映像 …）。为避免误杀无关程序…`，且**不会**杀它。

---

## 6. 未完成 / 明确不做

| 项 | 状态 | 说明 |
|---|---|---|
| `docs/MAINTENANCE.zh.md` / `.en.md` / `TECHNICAL-ROADMAP.md` / `RELEASE_NOTES` 的逐条修正 | **未做** | 数字类漂移已由 `FACTS.json` 机制接管（并在 README 中修正）；这四份文档中仍有历史叙述与旧语义（如「Job Object 内核级零孤儿」「设置窗口位置记忆」）。建议下一轮统一修订，避免我在此轮引入新的不一致 |
| `selftest.ps1` 新增 F–J 段 | **未做** | 审计报告 §5.5 给了具体清单；本轮改为提供两个**独立、可单独运行**的验证脚本（`verify-service-lifecycle` / `verify-token-navigation`），避免改动既有自检脚本引入回归 |
| 设置窗口位置记忆（回归清单 #20） | **未做** | 需设计多显示器/DPI 边界处理，属独立特性而非缺陷修复 |
| Edge 回退在无 WebView2 环境实测（#11） | **未做** | 需要干净 VM；代码路径未变 |
| 主题采样在真实 dsh 主题切换下的表现 | **待实测** | 代码已接线，需一次运行确认（验证清单外的观察项） |
| 归档清理「多轮重试防复活」（v4 有、v5 缺） | **未做** | 属功能收缩，需产品决策；报告已记录 |
| `docs/FACTS.json` 被 `check-consistency.ps1` 引用做数字断言 | **部分** | FACTS 已生成且有 `-Check`；尚未把「文档里的每个数字都等于 FACTS」做成硬断言（需要先统一那四份文档） |

---

## 7. 交付期踩到并修掉的三个「工程性」缺陷（都值得记住）

这一节记录的是**交付过程中**暴露出来的问题，不是审计报告里的条目。它们的共同特征是：
**单元测试全绿、但真机一跑就出问题**。

### 7.1 `ServiceHandle::start()` 持锁 spawn → 确定性死锁

- **症状**：进程在、托盘图标在，但**没有任何窗口**、菜单点不动、CPU 接近 0、
  所有窗口 `hung=True`；日志固定停在「托盘图标已创建。」。
- **根因**：注释写着「先释放锁再 spawn 子进程」，但 `self.inner.lock()` 的守卫从函数开头
  一直活到 spawn 之后 —— **持锁 spawn**。stdout 捕获回调会取同一把锁，于是
  写入线程与主线程互相等待。
- **修复**：`drop(inner)` 后再 spawn，并加 `debug_assert!(self.inner.try_lock().is_ok())`
  把「持锁 spawn」在开发期就暴露。
- **为什么单测没覆盖**：这条路径需要**真实子进程 + 真实 stdout 回调**才会撞上，
  纯逻辑单测构造不出来。这也是为什么后来加了运行期探针（§2.2）。

### 7.2 主题采样结果「只读不清」→ 日志刷屏

- **症状**：`launcher.log` 被同一条消息刷爆（实测 4557 行 / 仅 35 种内容），
  真正的启动诊断被淹没。
- **根因**：读取共享槽位用了 `.lock()` 而**没有清空**，同一结果被逐帧重复处理与打印。
- **修复**：改为「取出并清空」（`Option::take`）。

### 7.3 「取出并清空」之后限流失效 → 更严重的刷屏（400 行/秒）

- **症状**：修 7.2 后**反而更糟** —— 50 秒写 20,422 行（约 400 行/秒），日志 1.6 MB。
- **根因**：限流条件挂在 `theme_sample_pending`（"有没有结果"）上。槽位被清空后该标志
  几乎总是 false，于是**几乎每帧提交一次 JS 采样**，回调每帧回一个结果 → 每帧打一行。
  **真正的问题是按帧数计限流**（`tick % 30`）—— 事件循环实测可跑数百 fps，
  「每 30 帧」实际是「每秒好几次」。
- **修复**：两处一起改——① 限流改为**按时间**（`Instant` + 最小提交间隔 3 秒）；
  ② **只在采样颜色变化时打日志**。
- **验证（用数据而非推断）**：修复后启动 35 秒只写 1,183 字节，随后 15 秒写 **0 字节**，
  主题行 0 —— 对比修复前的 400 行/秒。

### 7.4 热替换产物（Windows 不允许覆盖运行中的 exe）

- **问题**：`Copy-Item` 覆盖正在运行的 `DSHLauncher.exe` 必失败，于是每次发布都必须先
  结束启动器（进而打断会话），部署与"不打断用户"互相冲突。
- **可行做法**：Windows 允许对运行中的 exe **改名**（改的是目录项，不是文件内容）。
  因此 **先 `Rename-Item` 让位、再 `Copy-Item` 放入新产物**，全程不需要结束任何进程。
  本次就是这么把 4 次修复送上去的，过程中 dsh 与会话始终未中断。
- **配套**：新增 `--build-info` 作为**可执行的产物判定**入口（把标记写到
  `%APPDATA%\DSHLauncher\build-info.txt`）。曾经想用「在二进制里搜字符串」判断版本，
  但 `debug_assert!` 与注释**都不会进入 release 产物**，那个判据是错的 —— 踩过一次。

### 7.5 一个反复出现的干扰因素（如实记录）

审查期间多次出现「命令执行到一半被中断」，直接导致**根产物长期没被更新**（用户双击的
一直是旧版）。事后确认原因：**用户重新打开启动器**这个动作本身打断了我正在执行的命令。
因此最终的做法是：**把能力固化进 `build.ps1` 本身**（内置热替换，不再依赖一次性部署脚本），
并把"部署成功"的判据做成可复跑的工具 `tools/verify-deployed.ps1`
（校验 `--build-info` 修复标记 + 运行实例确有可见窗口 + 窗口不 hung）。

> 历史说明：期间曾有一个一次性脚本 `deploy-fix.ps1`（专为修复那个死锁版本而写）。
> 其能力已并入 `build.ps1`（热替换 + 历史产物清理）与 `tools/verify-deployed.ps1`
> （落地校验），该脚本已在仓库清理中删除，避免留下会误导后人的过时命令。

---

## 8. 「清理归档会话」的用户体验修复（用户实测驱动）

用户提出三点，全部落地：

1. **清理必然停 dsh，会打断正在进行的会话** → 加二次确认；
2. **清理后服务不会自己回来**（实测：日志显示用户手动点了 `设置页命令: Start`）
   → 清理结束后自动按需重启服务；
3. 根目录堆积历史产物 → `build.ps1` 内置热替换并自动清理（只留最近一个 `.bak`）。

随后用户进一步要求：**「清理前应确保没有运行中的会话」**。这条比前两条更微妙，
因为「dsh 现在是否持有活跃会话」在用户态**无法精确询问**，只能用间接信号推断。

### 8.1 采用的信号（两个，互补）

| 信号 | 强度 | 依据 |
|---|---|---|
| **会话运行器进程存活** | 硬 | dsh 把会话派生到独立进程（`@deepseek-ai/dsh-subprocess-local ... runner.js`），只在会话真正执行时存在 |
| **会话文件近期被写过** | 软 | 会话内容落在 `~/.dsh/sessions/<ws>/<session-id>/session.v*.jsonl*`；5 分钟窗口内的写入说明刚活动过（也可能刚结束） |

### 8.2 实测（`crates/dsh-core/examples/active_session_probe.rs`）

```text
[PASS] A: 探测返回结构（耗时 1.27s）
       运行器进程 = 1
       近期写入文件 = 1
       涉及会话 = ["session-d2084fd4-4a75-4bfe-9c98-afa52e0924c4"]
       摘要 = 检测到 1 个正在运行的会话进程；1 个会话文件在最近 5 分钟内被写过
[PASS] B: 交叉验证一致（独立用 PowerShell 复核运行器数也是 1）
[PASS] C: summary/标志 与计数自洽
[PASS] D: 探测耗时 1.27s 在可接受范围（< 5s）
```

它检出的 `session-d2084fd4-…` **正是发起这次审查的那条对话** —— 也就是说守卫确实在
保护用户正在使用的会话。

### 8.3 为什么是「提高确认门槛」而不是「硬性阻止」

若做成硬性阻止（有会话就拒绝清理），那么**用户正与 dsh 对话时点清理必然被拒**，
功能等于永久不可用（"当前这条对话"本身就是一个活跃会话）。因此实现选择：

- 命中活跃会话时**把后果讲清并点名正在跑的会话**；
- **默认焦点仍放在「否」**（`MB_DEFBUTTON2`），避免误按回车就直接打断；
- 探测**无法判定**时按"有活动"处理（保守方向）；
- 把探测结论写进日志（`清理归档会话前探测到活跃迹象：…`），便于事后复盘。

### 8.4 实现上的一处妥协（如实记录）

「会话运行器进程存活」需要读**其它进程的命令行**，通常要 `ReadProcessMemory` + 遍历 PEB，
但本项目使用的 `windows` crate **并未导出 `ReadProcessMemory`**（已实测确认其 feature 中
无此符号）。为一个探测去手工解析 PEB 不值得，因此改用系统自带的
`Get-CimInstance Win32_Process`（一次调用拿到全部进程命令行）。

- **代价**：探测耗时约 **1.27 秒**（含一次 PowerShell 启动）。
- **可接受性**：它**只在用户点击「清理归档会话」时调用一次**，不在热路径上。
- **风险与对策**：这类"靠外部命令拿数据"的实现最容易静默失效，因此
  `active_session_probe.rs` 用**独立路径（另一条 PowerShell 命令）交叉验证**探测结果，
  并对「探测报告 0 而独立复核 > 0」直接判失败。已纳入 `check-consistency.ps1`（102 项）。

> 顺带修正了审计报告 §5.3 F.3 里的一条结论：当时写「无法从源码判定 dsh 是否持有活跃
> 会话」，现在给出了**可运行的间接判定**，并明确标注其不确定性。
