# DSHLauncher 5.0.0 LTS — 最终深度审查轮实施记录

> 本文档是 **5.0.0 LTS 发布后第三轮无禁区深度审查**（架构 / 缺陷 / 性能 / 内存 / 冗余 / 安全 /
> 可观测性 / 发布工程）的逐条处置记录，格式为「问题 → 方案 → 结果」。
>
> - 问题清单与分级（P0/P1/P2/P3）见本文 §1。
> - 验证证据见 [`RELEASE-VERIFICATION-v5.0.0-LTS.md`](RELEASE-VERIFICATION-v5.0.0-LTS.md)。
> - 指标数字以 [`FACTS.json`](FACTS.json) 为准（由 `tools/gen-facts.ps1` 实测生成）。
> - 版本不变：`5.0.0`（合法 semver），`LTS` 是发布标签。

## 1. 审计报告摘要（P0 / P1 / P2 / P3）

基线：`cargo check` / `cargo clippy --all-targets --all-features -D warnings` / `cargo test --all-features` /
`cargo fmt --check` 全部通过，一致性门禁 164/165 项通过（本轮新增的 18 条断言见 §2）。
因此本轮的价值不在「修红」，而在于**找出静态检查与既有测试都覆盖不到的真实缺陷**。

| 级别 | 问题 | 处置 |
|---|---|---|
| **P0** | 关闭到托盘的 Harness 窗口**无法恢复**（默认行为；托盘「打开界面」、双击图标唤起全部无效） | 已修复 + 门禁断言（§2.1） |
| **P0** | **卸载器拒绝卸载任何真实安装**：安装器写的安装标记首行是产品名，卸载器却要求内部标识（两者互不包含）⇒ 退出码 2，什么都不删 | 已修复 + 三方交叉校验 + 模拟安装 dry-run 校验脚本（§2.1） |
| **P1** | README 承诺的构建入口 `powershell -File build.ps1` 在 ANSI 非 UTF-8 代码页（本机 gb2312）下**无法解析** | 已修复（BOM）+ 门禁断言（§2.6） |
| **P1** | 一致性/校验脚本在 Windows PowerShell 5.1 下 **35 项假失败**（`Get-Content` 按代码页解码 UTF-8） | 已修复（固定 UTF-8 默认）+ 门禁断言（§2.6） |
| **P1** | 进程枚举被反复全表快照：身份判定最坏 8 次、卸载器每个 PID 一次、进程树每个节点一次 | 已重构为一次性 `ProcessTable`（§2.3） |
| **P1** | UI 线程上执行 `Get-CimInstance` 活跃会话探测（实测约 1.3 秒） | 已移到 worker 线程（§2.3） |
| **P1** | `--version` / `--help` 的输出在 stdout 被重定向时**被静默丢弃**（`AttachConsole` 覆盖了标准句柄） | 已修复 + 门禁断言（§2.1） |
| **P2** | 旧版 `settings.ini` 迁移结果**绕过** `validate()`（`port=0` 会被采信） | 已修复（§2.1） |
| **P2** | 「恢复默认设置」不清理 `settings.ini`，重置后旧配置会被重新导入 | 已修复（§2.1） |
| **P2** | `run_capture_within` 在拿不到 stdout 时**泄漏常驻子进程** | 已修复（§2.1） |
| **P2** | 进程创建时间只保留整秒，削弱「防 PID 复用」判定 | 已修复为 100ns 精度（§2.1） |
| **P2** | 日志每行 `open + write + close` + 路径 `metadata` 判滚动 | 已改为常开句柄 + 句柄 fstat（§2.3） |
| **P2** | 归档清理对每个归档 id 重复枚举 sessions 根目录 | 已改为一次性枚举（§2.3） |
| **P2** | 崩溃诊断行可能被静默截断（256 字节缓冲偏紧） | 已扩到 384 字节 + 单测（§2.1） |
| **P3** | 死代码：`DshError::Io`（零构造点）、`ProcessManager::take_stdout/take_stderr`（零调用方且是陷阱 API） | 已删除（§2.4） |
| **P3** | 重复实现：`guards::full` 与 `plan::normalize` 逐字重复；`is_protected_root` 有不可达分支 | 已合并/删除（§2.4） |
| **P3** | 测试假覆盖：旧版 ini 迁移测试抄了一份解析循环，生产路径从未被执行 | 已抽出 `parse_ini` 并直接测它（§2.4） |
| **P3** | Edge 回退路径派生 `reg.exe` 解析文本 | 已改用 `RegGetValueW`（§2.3） |
| **P3** | `lib.rs` 目录回退逻辑两份写法不一致 | 已合并为 `dir_or_current`（§2.4） |
| **P3** | `redact_secrets` 快路径每次分配小写副本 | **评估后不改**（§2.3 说明理由） |
| **P3** | 导航白名单文档与实现不符（文档称只允许回环，实现还允许 `data:`/`blob:`） | 已对齐文档并说明理由（§2.5） |

## 2. 关键变更（问题 → 方案 → 结果）

### 2.1 缺陷修复

#### P0 — 隐藏到托盘的窗口无法恢复

- **问题**：`App::pump_pending_open` 的首个判断是 `if !self.want_open || self.harness.is_some() { self.want_open = false; return; }`。
  这意味着只要窗口对象还在（关闭到托盘时 `harness` **不会**被置空），函数就立刻返回，
  于是 `open_harness` 中「已存在则 `h.show()`」的分支**永远不可达**。
  托盘「打开界面」、第二实例唤起（双击图标）两条路径都只设置 `want_open = true` 后交给该泵，
  因此两者都表现为「点了/双击了没反应」，而日志还写着「已激活内嵌窗口」。
  `tray_on_close` 默认为 `true`，所以这是**默认路径**上的功能缺失。
- **方案**：
  1. `open_harness` 对已存在窗口**无条件** `show()`（地址未就绪时也显示，并在日志里说明「地址尚未就绪，稍后自动导航」）；地址就绪时一并 `navigate`。
  2. `pump_pending_open` 去掉 `harness.is_some()` 短路，只按 `want_open` 驱动。
  3. 新增 `App::navigate_existing_harness()`：服务就绪/重启后的**补导航**只改地址、不改可见性，避免服务恢复时把用户刻意隐藏的窗口弹到前台。
  4. `ServiceEvent::Ready` 与 `BackgroundTask::Restart` 改为：窗口已存在 ⇒ 就地导航；否则才排入「打开待办」。
- **结果**：关闭到托盘（关闭按钮 / Esc）与托盘打开、双击唤起形成闭环；`tools/check-consistency.ps1` 新增断言锁死该回归（泵不得出现 `harness.is_some` 短路，且 `open_harness` 必须保留「已存在 ⇒ 显示」分支）。

#### P2 — 旧版 ini 迁移绕过校验 / 重置不清理旧 ini

- **问题**：`Settings::load()` 在 `settings.toml` 不存在时走 `migrate_from_ini()`，成功即 `return Ok(migrated)`，
  **跳过了 `validate()`**；而 TOML 路径是校验的。一份 `port=0` 的旧 ini 会让启动器用 0 端口拉起服务。
  另外 `reset_on_disk()` 只删 `settings.toml`：迁移失败留下的旧 ini 会在下次启动被重新导入，用户「恢复默认设置」后依旧起不来。
- **方案**：迁移结果与 TOML 路径共用同一套 `validate()`（失败时报 `ConfigError::Corrupted` 并**保留** ini 供用户处理）；
  `reset_on_disk()` 同时删除两个配置文件；新增 `remove_if_exists()`（幂等，删除失败如实上抛，不静默）。
- **结果**：非法旧配置不再静默生效；重置语义完整。新增单测：`migrated_settings_are_validated`、`remove_if_exists_is_idempotent_and_strict`。

#### P2 — 有界外部命令探测的进程泄漏

- **问题**：`run_capture_within()` 里 `let mut stdout = child.stdout.take()?;` —— `?` 直接返回时 `Child` 被丢弃，
  而 `std::process::Child` 的 `Drop` **不会**终止子进程，于是异常分支会留下一个无人看管的 PowerShell。
- **方案**：改为 `let Some(mut stdout) = child.stdout.take() else { kill + kill_process_tree + wait; return None };`。
- **结果**：任何异常分支都不再泄漏常驻子进程。

#### P0 — 卸载器拒绝卸载任何真实安装

- **问题**：安装器 `DSHLauncherSetup.cs::BuildMarkerText()` 写入的安装标记以 `Program.AppName` 开头，
  而 `AppName = "DeepSeek Harness Launcher"`；卸载器 `plan.rs::check_admission()` 却用
  `content.contains(dsh_core::APP_NAME)` 校验，`APP_NAME = "DSHLauncher"`。
  两者**互不包含**（`DeepSeek Harness Launcher` 里没有子串 `DSHLauncher`），
  于是**每一台真实安装都会被自己的卸载器拒绝**：退出码 2、什么都不删，
  注册表卸载项、桌面快捷方式、载荷文件与 `%LOCALAPPDATA%` 运行期数据全部残留，用户只能手工清理。
- **为什么此前没被发现**：卸载器的准入路径只有在「安装标记存在且内容匹配」时才会继续，
  而仓库根目录是**源码树**（按设计必须被拒绝），所以任何在仓库里跑 dry-run 的验证都只会看到
  「这是源码树」这一条分支 —— 准入通过的路径**从未被执行过**。
- **方案**：
  1. 卸载器新增纯函数 `marker_is_ours(content)`：接受**内部标识或产品名**任一等价标识；
  2. `dsh-core` 新增常量 `PRODUCT_NAME`，作为产品名的单一来源；
  3. 安装器在标记里显式追加一行 `product=DSHLauncher`，使新安装的标记不再依赖两处字符串的巧合；
  4. 新增 `tools/verify-install-layout-dryrun.ps1`：在 `%TEMP%` 搭一个与真实安装同构的目录，
     只跑 `--dry-run`（零改动），逐项断言准入通过、载荷清单完整、dry-run 没有改动任何内容；
  5. 门禁新增三方交叉校验（Rust `PRODUCT_NAME` ↔ 卸载器判定函数 ↔ C# `AppName`）与该脚本的存在性断言。
- **结果**：模拟安装目录的 dry-run 由 **exit 2（拒绝）** 变为 **PASS**：6 项载荷、`.tmp` 暂存残留
  与上一版遗留脚本（`uninstall.cmd` / `uninstall.ps1`）全部被列出，且目录内容零改动。
  新增单测 `accepts_the_marker_text_the_installer_actually_writes`。

#### P1 — `--version` / `--help` 输出被静默丢弃

- **问题**：release 是 Windows **GUI 子系统**，`emit_console_text()` 无条件调用 `AttachConsole(ATTACH_PARENT_PROCESS)`
  以「借用父进程控制台」。但该调用会把进程的标准句柄指向控制台，于是**调用方重定向到文件或管道的 stdout 被丢弃**：
  实测 `DSHLauncher.exe --version > ver.txt` 得到 **0 字节**，在终端上也看不到任何输出。
  受影响的正是在 README 与安装验证里被反复使用的版本核对入口（`--build-info` 也丢 stdout，只是它还有标记文件兜底）。
  反证：不调用该函数的 `--probe-identity` 输出正常。
- **方案**：新增 `has_stdout_handle()`，**先判定进程是否已有可写的 stdout 句柄**：
  有（终端直接运行、管道、文件重定向）⇒ 直接写、尊重重定向；只有完全没有句柄（资源管理器双击）才 `AttachConsole`。
- **结果**：`--version` 输出 `DSHLauncher 5.0.0 LTS (release)`；`--version > file` 得到 32 字节；`--help` / `--build-info` 同样恢复。
  门禁新增断言：`emit_console_text` 必须先判定句柄再决定是否借用控制台。

#### P2 — 进程创建时间精度 / 崩溃诊断行截断

- **问题**：`filetime_to_system_time()` 只取整秒；与 `service_record.rs` 的 2 秒容差叠加后，「同一秒内 PID 被复用」无法区分。
  崩溃取证的 `line` 缓冲为 256 字节，而中文 FATAL 行实测约 200 字节，余量过小。
- **方案**：`FILETIME` 保留 100ns（`Duration::new(secs, nanos)` + `checked_add` 防回绕）；缓冲扩到 384 字节。
- **结果**：新增单测 `filetime_keeps_sub_second_precision`（断言亚秒值并验证「早于 UNIX 纪元返回 None」）、
  `fatal_line_fits_without_truncation`、`fatal_line_handles_extreme_values`。

### 2.2 逻辑修正

- `ServiceHandle::monitor_worker` 的「非活跃状态看守端口」条件由 `!matches!(state, Stopped)` 改为显式的 `matches!(state, Error)`，
  并注释说明为何 `Stopping` 不可能出现在该分支（`stop()` 在同一临界区同时置 `monitor_enabled = false` 与 `state = Stopping`）。
  这样未来若有人改动 `stop()` 的加锁范围，行为会立刻暴露，而不是悄悄去「接管一个正在被终止的进程」。
- 新增 `ServiceState::is_active()` 回归测试：`Stopping` / `Stopped` / `Error` 均不算活跃（否则「用户主动停止」会被失联监测误报为「失联」并自动重启）。
- 新增 `ServiceHandle` 生命周期策略测试：`tied` ⇒ `stops_service_on_exit() == true`，`independent` ⇒ `false`。

### 2.3 性能与内存

#### 进程枚举：从「每次查询一次全表快照」到「一次成表」

- **问题**：`classify_dsh_identity` 会依次调用 `process_image_name`（一次全表枚举）与 `is_descendant_of_node`（每级祖先一次，最多 6 次），
  最坏 8 次；`kill_process_tree` 是**每个节点**一次；卸载器 `stop_launcher_in` 对**每个 PID** 调 `process_image_name`，
  机器上 300 个进程就是 300 次全表枚举。这是纯粹的重复系统调用与内存拷贝。
- **方案**：新增 `dsh_core::ProcessTable`（一次 `CreateToolhelp32Snapshot` → `entries: Vec<ProcEntry>`，提供 `image_name` / `parent_of` / `pids` / `child_pids_of` / `child_links`）；
  `classify_dsh_identity` 只捕获一次并交给 `classify_dsh_identity_in(&table, pid)`；
  `is_descendant_of_node(&table, pid)` 在同一份快照上追溯（深度上限 `ANCESTOR_MAX_DEPTH`）；
  `kill_process_tree` 改为**每层一次**快照（保留「收集过程中新派生的子孙也能被纳入」这一特性）+ 终止后**一次**补偿扫描卷积新派生的子进程；
  卸载器改用 `ProcessTable::capture()` 后按 PID 查表。
- **结果**：身份判定 8 → 1 次；卸载器 N → 1 次；进程树 O(节点数) → O(树深)+1。新增单测 `process_table_is_queryable`、`table_driven_identity_keeps_three_state_semantics` 与门禁断言。

#### 日志：常开句柄 + 句柄 fstat 判滚动

- **问题**：`RollingLogger::append` 每行都 `OpenOptions::open` + `write` + 关闭，并在写完后用 `std::fs::metadata(路径)` 判断是否需要滚动。
  日志写入发生在 UI 线程（托盘动作、启动计时、状态变更）与 dsh 的 stderr 排空线程上，突发时每秒数百行。
- **方案**：把「锁 + 句柄 + 目录就绪标志」合并为一个共享 `Sink`（`Arc<Mutex<Sink>>`，`Clone` 共享）；
  句柄常开，滚动判定改为对**句柄自身** `fstat`；需要滚动时先 `drop` 句柄再改名（Windows 上把正被写着的文件改名会让后续写入落到被改名的文件里），随后重新打开；
  阈值判定改为「先滚动再写」，使每个归档都不超过阈值。
- **结果**：每行省 2 次系统调用与 1 次路径解析。新增单测：`clones_share_one_sink`、`rotates_before_exceeding_threshold_and_keeps_max_files`、
  `shared_handle_still_writes_whole_lines`（并发 4 线程 × 25 行，断言 100 行且每行结构完整）。

#### 其他

- 归档清理：一次性枚举 workspace 目录（N 个归档从 N 次目录枚举降到 1 次）。
- Edge 回退：`reg.exe query` + 文本解析改为 `RegGetValueW`（含 `WOW6432Node` 视图），少一个子进程。
- 活跃会话探测移出 UI 线程（见下）。
- **评估后不改**：`redact_secrets` 的快路径每次仍会 `to_ascii_lowercase()` 分配一份副本。
  曾尝试改为免分配的逐键大小写折叠扫描，但那会把每行的字符比较从 1 次线性扫描抬到 18 次（CPU 明显劣化），
  而日志行的分配开销远小于此。这是一个**有意保留的取舍**，在此记录以免后来者重复评估。

### 2.4 冗余清理与源码精简

- 删除 `DshError::Io`：全仓无任何构造点（`Dsh::resolve*` 不做 IO）。
- 删除 `ProcessManager::take_stdout/take_stderr`：零调用方，且文档已自认是「取走后子进程会在管道写满时永久阻塞」的陷阱 API。
- 合并 `guards::full` 与 `plan::normalize`：两份逐字重复的路径规范化实现（卸载器的「准入校验」与「比较」一旦分叉，后果是误删受保护路径）。唯一实现落在 `guards::normalize`，`plan` 只做 `pub use` 转发。
- 删除 `guards::is_protected_root` 中不可达的第三个分支（`len == 2 && bytes[1] == ':'` 被前一个分支完全覆盖）。
- 测试假覆盖修复：`parses_legacy_ini_shape` 曾把解析循环**抄了一遍**，生产代码路径从未被执行；现抽出 `Settings::parse_ini(raw)` 并直接测它。
- `lib.rs`：`app_data_dir` / `local_data_dir` 各自写了一份「取不到就退化为当前目录」，合并为 `dir_or_current`。
- `dsh-uninstall/steps.rs`：载荷清单只计算一次（删除与复核共用），避免两次分配。

### 2.5 安全与可观测性

- `HarnessWindow` 的导航白名单文档与实现对齐：明确允许 `about:blank` / `data:text/html` / `blob:` / 回环字面量（`127.0.0.1`、`localhost`、`[::1]`），其余一律拒绝；
  并说明 `data:` / `blob:` 属于有意的白名单项（这两类不指向网络位置，且该窗口不注册任何 IPC 通道，拿不到任何能力）。
- 崩溃取证、身份判定、进程表、日志滚动、配置迁移均有单测（共新增 11 个）。
- 日志脱敏范围、`--probe-identity`、`--build-info` 等既有可观测性能力保持不变（本轮未削弱任何诊断入口）。

### 2.6 发布工程

#### P1 — 中文 Windows 上构建脚本无法解析

- **问题**：`build.ps1` / `build-setup.ps1` / `make-icon.ps1` / `selftest.ps1` / `tools/*.ps1` 都是 **UTF-8 无 BOM**，
  而 README 承诺的入口是 `powershell -ExecutionPolicy Bypass -File build.ps1`（Windows PowerShell **5.1**）。
  5.1 在没有 BOM 时按 **ANSI 代码页**（本机 gb2312）解码脚本自身，中文注释被解码成乱码并吞掉引号/反斜杠，
  报出 `Missing closing ')'` / `Unexpected token` —— 在中文/日文/韩文 Windows 上**根本构建不了**。
- **方案**：给全部含非 ASCII 的 `.ps1` 加 UTF-8 BOM（PS 5.1 据此按 UTF-8 解码；PS 7 不受影响）。
- **结果**：`powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1 release` 与 `build-setup.ps1` 实测通过（见发布验证记录）。
  门禁新增断言：含非 ASCII 的 `.ps1` 必须带 BOM。

#### P1 — 校验脚本在 PS 5.1 下的 35 项假失败

- **问题**：BOM 只影响脚本**自身**的解析；脚本用 `Get-Content` 读源码/文档时，PS 5.1 仍按代码页解码，
  于是 UTF-8 的中文源码变成乱码、所有中文断言假失败（实测 35 项），`FACTS.json` 甚至无法解析。
- **方案**：每个含非 ASCII 的 `.ps1` 顶部固定 `$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'`。
- **结果**：`tools/check-consistency.ps1` 在 **PS 5.1 与 PS 7 下均 165/165 通过**。门禁新增对应断言。

#### 构建目录布局（Cargo build-dir v2 / CFT）兼容性

- **背景**：Cargo 的 *Call for Testing: Build Dir Layout v2* 把构建目录改为「按包名 + 构建单元哈希」
  组织（`deps/`、`.fingerprint/` 消失，中间产物落到 `build/<包名>/<哈希>/{fingerprint,out}`），
  **Cargo 1.100 起已稳定并成为默认**；Cargo 1.91 起（稳定版可用）还能用 `CARGO_BUILD_BUILD_DIR`
  把中间产物整块搬出 `target/`。CFT 明确承诺**不变**的只有「最终产物在 `target/<profile>/` 内的布局」。
- **审计**（逐文件核对 + 全局扫描）：本仓库只依赖被承诺不变的两条最终产物路径
  （`target/release/dsh-app.exe`、`target/release/dsh-uninstall.exe`）；
  两个 `build.rs` 都从 `CARGO_MANIFEST_DIR` 反推仓库根，**没有**从 `OUT_DIR` 反推 target 目录
  （即 CFT 的 Issue #13663 那一类不适用）；没有依赖 `CARGO_BIN_EXE_*` 或 `[[test]]` 路径推断。
- **发现并修复的 4 处真实缺口**（都属于「现在的工具链依赖了未承诺的内部细节」）：
  1. `tools/clean.ps1` 只清 `target\`：一旦用 `CARGO_BUILD_BUILD_DIR` 搬迁，**最大的一块缓存会被漏掉**
     （实测：`target\` 只剩 7.6 MB，而搬迁目录 538 MB）。现在会解析生效的构建目录（env 优先，
     其次 `.cargo/config.toml` 的 `[build] build-dir`），报告、清理计划与「下一步指引」都跟着它走。
  2. `tools/finish-release.ps1` 的 6b 段只统计 `target\` ⇒ 搬迁后少报。现在两者都统计并分别记录。
  3. `selftest.ps1` 硬编码 `target\debug\examples\job_object_demo.exe`。现在从 cargo 的
     `--message-format=json` 里取 `executable`（权威且与布局无关，同时消除了「陈旧二进制假 PASS」），
     即 CFT 建议的「不要猜产物路径，问 cargo」。
  4. `.gitignore` 未覆盖被搬迁的构建目录（搬迁后会被当成未跟踪文件）。现补 `/build/` 与
     `/build-layout-probe/`。
- **新增验证器 `tools/verify-build-layout.ps1`**：静态断言（脚本是否硬编码 `deps`/`.fingerprint`/
  中间 `examples`；example 路径是否来自 cargo；`.gitignore` 与 `clean.ps1` 是否已适配）
  ＋ `-Run` 的三配置实测（默认 / `CARGO_BUILD_BUILD_DIR` / nightly 新布局），每步断言最终产物存在
  并跑版本链路校验。退出码 0/1。
- **实测结果**（本机 `cargo 1.98.1` + `cargo 1.100.0-nightly (2026-09-04)`，三次 release 全量构建）：
  默认布局 ✅；`CARGO_BUILD_BUILD_DIR=<root>\build-layout-probe` ✅（1086 个中间产物落到该目录、
  最终产物仍在 `target\release\`、`clean.ps1 -Cache` 计划包含它）；nightly 新布局 ✅
  （`target\release` 顶层出现按包名分桶的 `build`，最终产物路径不变）。
- **门禁新增 4 条断言**：脚本不得硬编码构建目录内部布局；`.gitignore` 必须覆盖被搬迁的构建目录；
  `clean.ps1` 必须解析生效的构建目录；`verify-build-layout.ps1` 必须存在（共 164 项）。
#### 自清洁工具重写（tools/clean.ps1）

- **问题**（三条，都实测过）：
  1. 只清 `target\`：仓库根的热替换副本 / `*.old-*` / `*.pubtmp` / `setup*.log` / `selftest.log`
     与 `%TEMP%` 里的自检残留（单测临时目录、卸载器副本、早期探针）**无人回收**；
  2. 清理清单是**子目录白名单**（`debug`、`release\deps`、`release\build`、`release\examples`、
     `release\.fingerprint`）：cargo 新增的 `target\tmp` / `doc` / `package` / 新 profile 会静默漏掉；
  3. 删不掉就报失败：热替换副本 `DSHLauncher.exe.bak-*` **是运行中实例的映像**，Windows 语义下
     不可删除 —— 于是清理经常以退出码 1 结束；而且它不知道清理是否破坏了 `docs/FACTS.json`。
- **方案**：重写为四区域 + 可组合开关，并把两条硬约束写进代码：
  * 区域：`target\`（`-Cache` / `-All`）、仓库根（`-Repo`）、`%TEMP%`（`-Temp`，白名单制）、
    运行期数据（只报告、**永不删除** —— 那是卸载器的职责）；
  * `-Cache` 改为「**除 release 交付物外全部清理**」：对 cargo 未来的目录天然免疫；
  * `%TEMP%` 白名单**显式排除** `dsh-spill-*` 与 `dsh-subprocess-*`（DSH 运行时正在使用），
    未识别的 `dsh*` 项只列出、不动作；
  * 前置校验：release 交付物缺失时拒绝 `-Cache` 并给出正确顺序（先构建再清缓存），退出码 2；
  * 后置自校验：读 `docs/FACTS.json` 比对 `artifacts.launcher_build` 字节数，并列出「清缓存后
    无需重建即可跑」的门禁清单；
  * 被占用项降级为「跳过」（含运行实例判定），只有 `target\` 内的删除失败才计为失败。
- **结果**：`-Cache` 实测回收 3.1 MB，且 `check-consistency`（160/160）/ `gen-facts -Check` /
  `verify-version`（33/0/0）/ 模拟安装 dry-run / `--version` 全部仍绿；`-Repo -Temp` 清掉历史残留
  （含早期轮次留下的 300 KB 探针）；`-WhatIf` 实测零改动。门禁新增 6 条断言覆盖上述契约。
#### 文档数字一致性

- `tools/gen-facts.ps1` 重新实测并写回 `docs/FACTS.json`；README（中/英）/ CHANGELOG（中/英）/ 技术路线图 §15.5 / 发布说明里的单元测试数、一致性项数、代码行数、产物体积已同步。
- `tools/gen-facts.ps1 -Check`：**FACTS.json 与实测一致 OK（已比对 31 项事实）**。

## 3. 受影响文件

**修改（Rust）**：`crates/dsh-core/src/process.rs`、`log.rs`、`config.rs`、`dsh.rs`、`lib.rs`、`maintenance.rs`、
`crates/dsh-app/src/main.rs`（`--version`/`--help` 输出可达性修复）、`app.rs`、`service.rs`、`browser.rs`、`crash.rs`、`crates/dsh-ui/src/harness.rs`、
`crates/dsh-uninstall/src/guards.rs`、`plan.rs`（`marker_is_ours`）、`platform.rs`、`steps.rs`；
`DSHLauncherSetup.cs`（安装标记追加 `product=` 行）。

**修改（工程/脚本）**：`tools/check-consistency.ps1`（147 → **165** 项断言，另为全部脚本固定 `Get-Content` 编码）、
全部 `.ps1`（UTF-8 BOM + `Get-Content` 编码默认值）、`.gitignore`（热替换产物）、
`README.md`、`docs/CHANGELOG.md`、`docs/RELEASE_NOTES_v5.0.0.md`、`docs/TECHNICAL-ROADMAP.md`、`docs/FACTS.json`。

**新增**：`docs/IMPLEMENTATION-v5.0.0-lts-final.md`（本文）、`tools/verify-install-layout-dryrun.ps1`（模拟安装目录 dry-run 校验）。

**未删除任何用户可见能力**：`--selftest` / `--settings` / `--guide` / `--ipc-probe` / `--quit` / `--build-info` /
`--probe-identity` / `--version` / `--help` 全部保留；安装包与卸载器的载荷清单、注册表项、快捷方式命名均未变。

## 4. 假设、平台限制与未执行项

| 项 | 说明 |
|---|---|
| 目标平台 | 仅 Windows（`x86_64-pc-windows-msvc`）：本仓库全部实现都建立在 Win32 / WebView2 之上，`Cargo.toml` 的 GUI 依赖刻意关闭默认 feature 以避免 Linux 侧拉入 GTK 栈 |
| 交叉目标 | 未执行：本机只安装了 `x86_64-pc-windows-msvc`，且项目本身不支持其它平台 |
| `cargo deny` | **未执行**：工具未安装且环境离线（`cargo install` 不可用）。已用仓库自带的 `tools/osv-audit.ps1`（OSV 通道，与 RustSec 同源数据）替代 |
| `cargo audit` | **未执行**：`advisory-db` 需要 `git clone https://github.com/RustSec/advisory-db`，本环境无法访问 GitHub（脚本已内置该替代路径并记录原因） |
| `cargo machete` / `cargo udeps` | **未执行**（工具缺失/离线）。仓库的 `check-consistency.ps1` 内置等价的离线依赖使用检查（对每个 crate 比对声明与源码引用），本轮 5 个 crate 全部通过 |
| 真实安装/卸载 | 本轮执行了 `dsh-uninstall.exe --dry-run`（零改动的影响范围报告）与版本链路校验；**未在真实用户环境上执行破坏性卸载**（会删除用户配置与运行期数据）。安装包已重新编译并通过 `verify-version.ps1 -RequireInstaller` 的 33 项断言 |
| `--selftest` 端到端 | **未执行**：本机已有启动器实例持有单实例互斥体，`--selftest` 按设计在此时返回 **2**（避免把「没执行验证」误判为通过）。非侵入式入口（`--version` / `--help` / `--build-info` / `--probe-identity`）已全部实测 |
| 运行中实例 | 发布产物通过「改名热替换」落地：**当前正在运行的实例仍是旧映像**，新版在下次启动时生效（这是仓库既有的、为避免中断会话而设计的行为） |
| 内存基线 | 未重新测量工作集：需要反复启动 GUI 进程，且与本轮改动无直接因果关系（本轮内存侧改动是「去掉每行日志的临时分配」与「减少快照拷贝」） |

## 5. 迁移说明

- **无破坏性变更**：CLI 参数、配置文件格式（`settings.toml`，`schema_version = 2`）、`service.json` 格式（`version = 1`）、
  注册表项（`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher`）、快捷方式命名、载荷清单均未改变。
- 唯一的行为变化是**修复**：关闭到托盘的窗口现在可以通过托盘「打开界面」或再次双击图标恢复（此前完全无效）。
- 旧版 `settings.ini` 若含非法值（例如 `port=0`），现在会在启动时明确报错并回退默认配置，而不是静默用非法值启动；
  「恢复默认设置」现在会同时清理 `settings.ini`。

## 6. 一致性核对表（5.0.0 LTS）

| 位置 | 期望 | 实测 |
|---|---|---|
| workspace `Cargo.toml` `[workspace.package] version` | `5.0.0` | ✅ `5.0.0` |
| 各成员 crate `version.workspace = true` | 继承 | ✅ 全部继承（无手写版本） |
| `--version` 输出 | `DSHLauncher 5.0.0 LTS (release)` | ✅ |
| `--build-info` `version=` | `5.0.0` | ✅ |
| exe 版本资源 `FileVersion` / `ProductVersion` | `5.0.0` | ✅ |
| 卸载器版本资源 | `5.0.0`（`OriginalFilename=dsh-uninstall.exe`） | ✅ |
| 安装包 `FileVersion` | `5.0.0.0`（反射自 exe，无第二处手写版本） | ✅ |
| 注册表 `DisplayVersion` | `5.0.0` | ✅（安装器绑定 `AppVersion`） |
| README / CHANGELOG（中英）/ 路线图 / 发布说明 | 数字与 `FACTS.json` 一致 | ✅ `gen-facts -Check` 全绿 |
| `LTS` 语义 | 仅作发布标签，不进入 semver | ✅ 门禁断言 |
