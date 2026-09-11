# v5.0.0 优化实施记录（定稿轮）

> 依据：[`AUDIT-REPORT-v5.0.0.md`](AUDIT-REPORT-v5.0.0.md)（定稿版）与其上一版
> 状态：**代码全部落地；除「需要独占单实例互斥体 / 会中断本机活动会话」的两类验证外，全部门禁已实跑并通过**
> 对外契约变更：**仅新增**（`--quit` 回执语义、设置页「恢复默认设置」）。文件名、原有 CLI 参数语义、安装/卸载语义、dsh 兼容性、单文件 exe + 仅 WebView2 —— 全部未变。
> 上一轮的实施记录已保存为 [`IMPLEMENTATION-v5.0.0-prev-round.md`](IMPLEMENTATION-v5.0.0-prev-round.md)（未改动）。

---

## 1. 一句话结论

**上一轮报告的两个 P0 确实修好了，但同一族里还留着两处更隐蔽的致命缺陷；本轮把它们、以及工具链自身的可信度问题一并处置，并把「单测全绿但真机跑挂」那类缺陷收敛到已知清单。**
门禁现状：Rust 三件套 + 一致性 + 版本链 + 文档数字门禁 + OSV + 两个运行期探针 **全绿**；两项脚本按设计返回 `SKIPPED(2)`（见 §6）；E2E/安装矩阵按你的要求留给你执行（§7 给了命令清单）。

### 1.1 门禁状态表（实跑）

| 门禁 | 命令 | 结果 | 原始输出摘要 |
|---|---|---|---|
| 格式 | `cargo fmt --check` | **exit 0** | 无差异 |
| 静态检查 | `cargo clippy --offline --all-targets --all-features -- -D warnings` | **exit 0** | 0 warning（以 error 级别强制） |
| 单元测试 | `cargo test --offline --all-features` | **exit 0** | `86 passed`（dsh-core）· `11 passed`（dsh-ui）· 其余 0；**合计 97** |
| 发布构建 | `cargo build --offline --release` | **exit 0** | `Finished release profile`，0 警告 |
| 一致性 | `pwsh -File tools\check-consistency.ps1` | **exit 0** | `CHECK_SUMMARY passed=115 failed=0 total=115` · `结果: 全部 115 项通过 OK` |
| 版本链路 | `pwsh -File tools\verify-version.ps1` | **exit 0** | `28 passed / 0 skipped / 0 failed —— 无冲突 OK` |
| 文档数字 | `pwsh -File tools\gen-facts.ps1 -Check` | **exit 0** | `FACTS.json 与实测一致 OK（已比对 29 项事实）` |
| 依赖审计 | `pwsh -File tools\osv-audit.ps1` | **exit 0** | 全量 264 包；3 条 advisory（glib×2、proc-macro-error）经 `cargo tree --target x86_64-pc-windows-msvc` **全部在图外** → 不构成运行风险 |
| 生命周期（只读） | `pwsh -File tools\verify-service-lifecycle.ps1` | **exit 2（SKIPPED）** | `结果：SKIPPED —— 只读检查通过，但决定性的破坏性实验未运行` |
| token 导航 | `pwsh -File tools\verify-token-navigation.ps1` | **exit 0** | `结果：全部通过`（含「日志中存在已捕获 dsh 就绪地址」断言） |
| 探针：token 捕获 | `cargo run -p dsh-core --example token_capture_probe -- 45680` | **exit 0** | `=== 结果：通过 ===`（见 §4.1） |
| 探针：活跃会话 | `cargo run -p dsh-core --example active_session_probe` | **exit 0** | `=== 结果：通过 ===`（见 §4.2） |
| 发布（热替换） | `pwsh -File build.ps1 release` | **exit 0** | `构建完成: D:\DSHLauncher\DSHLauncher.exe (1.02 MB)`；全程未结束任何进程 |
| 安装包 | `pwsh -File build-setup.ps1` | **exit 0** | `安装包构建完成: D:\DSHLauncher\DSHLauncherSetup.exe (v5.0.0.0)`；前置版本门禁 + 后置 `-RequireInstaller` 门禁均通过 |
| 落地校验 | `pwsh -File tools\verify-deployed.ps1` | **exit 0** | 11 个修复标记齐全；`结果：全部通过` |
| 图标比对 | `pwsh -File tools\verify-icon.ps1` | **exit 2（SKIPPED）** | 缺少「窗口图标导出」环节产出的 32×32 PNG（该生产步骤在仓库中不存在） |
| 端到端 | `pwsh -File selftest.ps1` | **未执行（按你的选择）** | 需要独占单实例互斥体、会中断你正在使用的会话 |
| 安装/卸载矩阵 | 4 项（静默安装 / 默认卸载 / `--purge` / `QuietUninstallString`） | **未执行（按你的选择）** | 会写入真实注册表与 `%LOCALAPPDATA%` |

---

## 2. P0 / P1 / P2 / P3 逐项处置

### 2.1 P0

| 问题 | 实现 | 位置 | **代价** |
|---|---|---|---|
| **N-1** token 等待预算在冷启动被绕过 + 等待逻辑无驱动源 | ①`Ready`（含 `url:None`）到达即重置预算；②新增 `want_open` + `pump_pending_open()` 每帧驱动；③**先判端口可达、再消耗预算** | `crates/dsh-app/src/app.rs` | 每帧多一次布尔判断（可忽略）。**放弃**了"窗口只在事件驱动下出现"的旧行为——现在必然在预算内尝试开窗，代价是 token 迟迟不来时会开一个需要手动授权的页面（与旧设计意图一致） |
| **N-2** 失联监测把 `Starting` 期间判成失联 | `Starting && 子进程存活` → 清零 `lost` 继续；`Starting && 子进程消失` → 立即按失联处理；`was_own` 改用 `owns_process` | `crates/dsh-app/src/service.rs` | 启动阶段不再有"12 秒后报警"的早发现能力，权威改为就绪 worker（120 s 预算）。**若 dsh 启动后"假活"（进程在、端口不通），要到 120 s 才被判定**。这是有意取舍：冷启动误报比晚报更伤用户 |
| **N-3** 基线 `cargo test` 是红的（flaky 固定临时目录） | `unique_temp_dir()`（PID + 进程内序号），并补 1 个用例 | `crates/dsh-core/src/dsh.rs` | 无 |
| **N-4** `print!` 在 GUI 子系统下 panic → 标记文件不写 | 新增 `emit_stdout()`（显式忽略写入错误），替换 3 处 `print!` | `crates/dsh-app/src/main.rs` | 无（stdout 写失败时静默，与 stderr 侧一致） |
| **N-5** 安装器任意目录 + 生成的卸载器递归删除 + 单引号注入 | ①CLI/GUI 共用 `InstallDirGuard.TryResolve`（先拒盘符根再 `GetFullPath`；对 10 个受保护目录做「同路径/祖先/后代」判定；仅放行 `%LOCALAPPDATA%\Programs` 的严格后代）；②安装时写 `.dsllauncher-install`，卸载时**要求标记存在**才允许递归；③生成脚本改用 `$PSScriptRoot`，**不再把任何路径拼进命令字符串**；④生成的 `.ps1` 以 UTF-8+BOM 写出（PS 5.1 才能正确读非 ASCII 路径），`.cmd` 保持 ASCII+CRLF | `DSHLauncherSetup.cs` | **放弃了「安装到任意目录」的自由度**：`%LOCALAPPDATA%\Programs` 之外需要显式确认，`%WINDIR%`/`%APPDATA%`/`%TEMP%`/`%System32%` 等一律拒绝 |
| **N-6** `tools/verify-monitor.ps1` 覆写并删除用户的 `settings.toml` | 写入前备份存在性 + 原始字节；`finally` 中**恢复**（原先不存在才删除） | `tools/verify-monitor.ps1` | 无 |

### 2.2 P1（Rust）

| 问题 | 实现 | 位置 |
|---|---|---|
| **N-7** `start()` 仍持锁 spawn（半个修复） | `ProcessManager` 拆到独立 `Arc<Mutex<..>>`；spawn 全程不持 `ServiceInner` 锁；锁序固定为 `ServiceInner → ProcessManager`（写进字段文档） | `service.rs` |
| **N-8** `RollingLogger::Clone` 新建锁 → 不互斥 + 滚动竞态 | `lock: Arc<Mutex<()>>`、`dir_ready: Arc<AtomicBool>`；新增「副本共享同一把锁」与「4 线程 × 50 行并发写不撕裂」用例 | `log.rs` |
| **N-9** `ReadyProbe::Clone` 复制代际号 → 取消失效 | `generation: Arc<AtomicU64>`；新增「在途探测被 bump 立即取消」用例 | `probe.rs` |
| **N-10** 监测线程退出竞态 → 自愈永久失效 | 退出前在锁内重新确认 `monitor_enabled` | `service.rs` |
| **N-11** UI 线程持锁执行进程终止 | 阻塞段只持 `ProcessManager` 锁；UI 路径走 `stop_async()`（回调经后台任务通道回主循环） | `service.rs`、`app.rs` |
| **N-12** 配置变更重启 / 清理前停服都在 UI 线程 | 新增 `BackgroundTask` 通道；`spawn_restart()`（串行 stop→set_config→start）与清理 worker（**停服也在 worker**） | `app.rs` |
| **N-13** 唤起路径逐帧 300 ms 阻塞探测 | `ACTIVATION_PROBE_TICKS = 10`（≈1 秒一次） | `app.rs` |
| **N-14** 脱敏只覆盖 `token=` | 18 个敏感键（词边界 + `=`/`:` + 可选引号）+ `Bearer`/`Basic`；5 个新用例（含 UTF-8 边界） | `log.rs` |
| **N-15** `node_path` 无效时静默回退 PATH | 新增 `DshError::InvalidNodePath`（文件名不符 / 文件不存在都硬报错；空白仍视为未指定）+ 用例 | `dsh.rs` |
| **N-16** `run_capture` 超时只杀直接子进程 → `join` 可能永久阻塞 | 超时路径改为 `kill_process_tree(child.id())` | `dsh.rs` |
| **N-17** 「关于」谎称 Job Object 回收 | `about(version, tied_service)`：按实际生命周期策略生成文案 | `dialog.rs`、`app.rs` |
| **N-18** `is_from_newer_schema` 与「设置页重置」两处空承诺 | ①启动时按 schema 版本告警；②设置页新增「恢复默认设置」+ `SettingsCommand::ResetConfig` + `Settings::reset_on_disk()`（二次确认、默认焦点「否」、重启前回填表单） | `config.rs`、`main.rs`、`settings.rs`、`settings.html`、`app.rs` |
| **N-19** 缺字段回退默认值会覆盖用户设置 | `Save` 的两个布尔改为 `Option<bool>`（缺 = 保留当前值）+ 2 个用例 | `dsh-ui/src/settings.rs` |
| **N-20** HTML→文本双重解码 + 未闭合块丢内容 | 实体顺序改为「先具体实体、最后 `&amp;`」；未闭合返回 `truncated` 并给出明确告知 + 兜底文案 | `dialog.rs` |
| **N-21** 两个 WebView 都没有导航白名单 | 内嵌页只允许回环/`about:blank`/`data:`/`blob:`；设置页只允许空白页与内联文档 + 用例 | `dsh-ui/src/harness.rs`、`settings.rs` |
| **N-22** 未知托盘 id 截断排空循环 | 内部循环取到队列为空才返回 | `tray_handler.rs` |
| **N-23** 主题状态未在关窗时清空 / 提交失败无退避 | 关窗清空全部主题状态；`theme_sample_at` **每次尝试都更新** | `app.rs` |
| **N-24** `ctrl_down` 粘滞 → `Ctrl+R` 误刷新 | 改用 `ModifiersChanged` + `Focused(false)` 清零（KeyDown/Up 仅兜底） | `app.rs` |
| **N-25** 注入 JS 恒真分支 + 120 s 无用定时器 | 简化为一次幂等 `classList.add` | `dsh-ui/src/harness.rs` |
| **N-26** DWM 三项属性首个失败即放弃 | 逐项尽力而为 | `dsh-ui/src/theme.rs` |
| **N-27** HWND 洗成 `usize` 后假设窗口存活 | 投递前 `IsWindow` 复核 | `dsh-ui/src/harness.rs` |

### 2.3 P2 / P3

| 问题 | 实现 |
|---|---|
| **N-45** `TerminateProcess` 失败被丢弃 | `kill_process_tree` 返回**确实被终止**的 PID 列表 |
| **N-46** 上游格式变化静默失效 | 新增 `looks_like_ready_line()` + `is_loopback_url()`；读取线程对"形似就绪地址却解析不出"的行告警一次 |
| **N-47** 手写 JSON 区间改写可能写坏文件 | ①要求整文档是合法 JSON；②`find(']')` 换成**字符串感知括号配对**（`match_bracket`）；③数组内容交 `serde_json`；④替换文本用 `serde_json::to_string`；⑤改写后复核。新增 3 个用例 |
| **N-48** 孤儿进程终止在持锁时执行 | 丢到独立线程（端口探测保留：一次性、连接被拒时立即返回） |
| **N-49** 活跃会话探测在 UI 线程（实测 ~1.3 s） | **未修，明确记录**：它紧邻模态确认框，此刻界面本就不可交互；彻底消除应改为「打开设置页时预取 + 缓存」 |
| **N-50** 端口占用预检在 UI 线程（一次性） | 保留 |
| **N-51~N-53** 死代码 | 删除 `Settings::reset`、`probe_port`、`HarnessWindow::{sample_and_apply_theme, evaluate_script, evaluate_script_with_callback, set_background_color, set_theme, hwnd}`、`dsh-ui` 的 6 个无消费者根导出、`has_auth_url`；`take_stdout`/`take_stderr`/`any_node_running` 标注为**非生产 API** 并写明理由 |
| **N-54** `logpath` 硬编码 | `SettingsInit.log_path` 由 `dsh_core::log_dir()` 注入；新增 `SettingsWindow::apply_init()` |
| **N-55** 指引页两处失实 | 文案改为「需要你在设置里手动填桌面路径」；快捷键注明「在主界面窗口内」并说明本窗口不响应 |
| **N-56** `FIX_MARKERS` 注释带版本前缀 | 去掉版本前缀；新增 4 个本轮标记（共 11 个） |
| **N-57** `semver` 传递依赖与 README 表述冲突 | 保留（禁用清单约束**直接依赖**），报告中注明 |
| **N-58** 文档数字互相矛盾 | `FACTS.json` 唯一来源；`gen-facts -Check` 4 → **29** 项；README/CHANGELOG/ROADMAP 的 82→97、102/50+/89→115、6,600→8,800 已同步 |

---

## 3. 工具链与安装器（本轮新增的可信度修复）

| # | 脚本 | 修复 | 代价 |
|---|---|---|---|
| N-28 | `finish-release.ps1` | `$script:published` + 启动自检该作用域机制；`Step` 把异常记入失败列表；每步各自捕获 `$LASTEXITCODE`；测试步骤对非零退出 / 0 套件 / `FAILED` / `^error(\[|:)` 判失败；有失败则整体非零 | 发布脚本现在**真的会因失败而红**（以前是"永远红且永远不因真失败而红"） |
| N-29 | 同上 | 汇总前先快照再 `AddRange`（原实现边枚举边 `Add`，异常使汇总只剩 1 行） | 无 |
| N-30 | `selftest.ps1` | `WaitForExit(180000)` + 超时杀整棵树并记失败 | 无 |
| N-31 | `selftest.ps1`、`verify-version.ps1` | `cargo build` 退出码立即捕获，非零硬失败 | 无（以前会拿旧产物跑出自检 PASS） |
| N-32 | `selftest.ps1` | `-SkipE2E` 现在跳过 B/C/D/E（只跑 A1/A2）；新增 `-SkipDistribution`；运行时打印执行计划 | **行为变更**：`-SkipE2E` 语义变窄（更符合 README），已同步文档 |
| N-33 | `verify-deployed.ps1` | `Add-Type` 后校验类型存在，不存在则 Fail 且不打印 PASS | 无（修掉最关键断言上的假通过） |
| N-34 | `verify-icon.ps1` | 仓库根从 `$PSScriptRoot` 推导；`-MaxDiff`；缺输入 → `exit 2`；不匹配 → `exit 1` | 该脚本**现在返回 2**，因为「窗口图标导出」生产步骤不存在（见 §6） |
| N-35 | `uninstall.ps1` | `-ErrorAction Stop` + 逐项收集失败 + 后置条件校验 + 失败 `exit 1`、仅验证通过才 `exit 0` | 无 |
| N-36 | `finish-release.ps1`、`verify-service-lifecycle.ps1` | 按路径过滤（`GetFullPath` + OrdinalIgnoreCase）；优先 `<repo>\DSHLauncher.exe --quit` 并有界等待 15 s；最后才强杀；路径选择写日志 | 无 |
| N-37 | `check-consistency.ps1` | `Strip-Comments` 重写为状态机（跨行块注释、raw string、`'static` 生命周期、字符串）+ 缓存 `Get-CodeText`；~55 条"接线类"断言改为读去注释文本并要求调用/定义形式。**用注入法证明**：把 `spawn_stderr_drain` 改成只出现在注释里 → 断言由 PASS 变 FAIL | 断言更严格：以后重命名调用点会真的红（本轮最重要的可信度修复） |
| N-38 | `gen-facts.ps1` | `-Check` 递归比对**全部 29 项事实**（原 4 项）；`null ↔ object` 视为增删键；一致性项不可测量改为显式失败 | 无 |
| N-39 | `gen-facts.ps1` | 排除 `build.rs` 的 `#[test]`，新增 `unit_tests.build_script`；`total` = `cargo test -- --list` 真正可跑的数量（97） | 文档"单元测试数"由 101（含 4 个永不运行的）修正为 **97** |
| N-40 | `check-consistency.ps1` | `$EXPECT_VERSION` 改为从 `Cargo.toml` 锚定解析；GUI crate 行缺失、路线图 §15.5 行缺失都改为显式 Issue | 无 |
| N-41 | `uninstall.cmd` + `.gitattributes` | 转为 **ASCII + CRLF + 无 BOM**（实测 `bareLF=0 / CR=行数 / nonASCII=0`）；删除 `chcp 65001`；新增 `.gitattributes`（`*.cmd text eol=crlf`） | 无 |
| N-42 | `DSHLauncherSetup.cs` | 见 N-5；另修：`RegisterUninstall`/`CreateShortcut`/`WriteUninstallCmd` 返回成功标志并传播（失败时静默安装返回 1、GUI 显示"安装失败"）；抽取改为两阶段（全部 `*.tmp` 就绪后再统一落位，失败回滚并删除本次创建的文件/目录/注册表项）；`*.tmp` 名字进卸载清单 | **生成的卸载器把"最终目录删除"延迟到隐藏子进程**，并**总是打印非致命 NOTE**。原因（实测）：子进程删掉正在运行的 `.cmd` 所在目录后 cmd.exe 仍以 1 返回（即便被委派的 PowerShell 返回 0），进程内递归删除会摧毁 N-35 的退出码契约、让每次成功卸载都报失败 |
| N-43 | `build-setup.ps1` | `csc` 前解析 Cargo 版本并跑 `verify-version.ps1 -Exe <根产物>`，不通过即中止；再独立比对 `VersionInfo.FileVersion`（只接受 `x.y.z` / `x.y.z.0`）；产出安装包后再跑 `-RequireInstaller` | 打包多两次版本校验（秒级） |
| N-44 | `DSHLauncherSetup.cs` | 首次 HTTP 请求前设置 `SecurityProtocol = Tls12`（实测该 exe 的默认 TLS 1.0 会让 WebView2 引导程序下载失败）；`UseShellExecute=false` 下不可能弹 UAC —— 日志/UI 文案改为如实描述，并在日志/GUI 报告 `elevated=yes/no` | **未做** Authenticode 校验与 `app.manifest`（见 §6） |
| — | `tools/osv-audit.ps1` | 发现记录后按 **Windows 构建图**逐包复核：全部在图外则结论为"不构成运行风险"并 `exit 0`，任一在图内则 `exit 1` | 语义变化：Linux-only advisory 不再阻断发布（但一旦某个包进入 Windows 图会立刻变红）。**修前该门禁永远红**（等于没有门禁） |

---

## 4. 运行期确证结果（探针输出原文）

### 4.1 `token_capture_probe`（隔离端口 45680，不触碰 3080 上的活动会话）

```text
=== token 捕获链路验证（端口 45680）===
node  : C:\Program Files\nodejs\node.exe
bin.js: C:\Users\20183\AppData\Roaming\npm\node_modules\@deepseek-ai\dsh\lib\bin.js
已启动 PID 4128（生命周期策略 = Independent）
就绪于 7.3651256s；捕获 token 于 8.3263421s（顺序：就绪先到 → token 后到）
[PASS] D: 就绪探测成功
[PASS] A: token 捕获回调已被调用
[PASS] B: 捕获地址含 token（已脱敏打印）：http://127.0.0.1:45680/?token=***
[PASS] C: 解析结果可复算一致（原始行已脱敏）：dsh web: http://127.0.0.1:45680/?token=***
[NOTE] E: 本次「就绪先于 token 961.2165ms」—— 正是旧实现会用无 token 地址导航的场景；修复后由 Ready{url:None} + 补发 Ready 覆盖
已停止（killed=[4128]）
[PASS] F: 停止后端口已释放（无残留）
=== 结果：通过 ===
```

**这条 `[NOTE]` 是 N-1 的直接证据**：就绪比 token 早约 **961 ms**。旧实现此刻已用无 token 地址开窗（实测该地址 HTTP 401），而预算还可能因"未初始化"**连等都不等**。修复后由 `Ready{url:None}` 占位 + 捕获后补发 `Ready` + 每帧驱动覆盖。

### 4.2 `active_session_probe`（只读）

```text
=== 活跃会话探测验证（只读）===
[PASS] A: 探测返回结构（耗时 1.2525327s）
       运行器进程 = 1
       近期写入文件 = 2
       涉及会话 = ["session-cc3e4564-…", "10de4175-…"]
       摘要 = 检测到 1 个正在运行的会话进程；2 个会话文件在最近 5 分钟内被写过
       is_running = true  any_activity = true
[INFO] B: 独立复核（PowerShell）运行器数 = 1
[PASS] B: 交叉验证一致（探测 = 1 ≥ 1）
[PASS] C: summary/标志 与计数自洽
[PASS] D: 探测耗时 1.2525327s 在可接受范围（< 5s）
=== 结果：通过 ===
```

检出的两个会话正是**发起这次审查的对话**与一个子代理会话 —— 守卫确实在保护使用中的会话。

---

## 5. 与上一轮的对照

### 5.1 与 `AUDIT-REPORT-v5.0.0.md`（上一版）逐条对照

| 上一轮项 | 结论 |
|---|---|
| P0 A1（服务强耦合） | **已处置**（上一轮），本轮**复核确认有效** |
| P0 A2（token 从未生效） | **部分处置** → 本轮**补全**（N-1） |
| P1 B1/B2/B3 | **已处置**，复核确认有效 |
| P1 B4（捕获回调代际号） | **部分处置** → 本轮**补全**（N-9） |
| P1 C1/C2/E1 | **已处置**，复核确认有效 |
| P1 F1（托盘失败仅日志） | **未处置**（§6） |
| P1 G1（升级必然杀 dsh） | **已处置**，但**注释是错的**；本轮改写为真实机制并让安装器优先 `--quit`（N-42） |
| P1 G2（无脚本退出入口） | **已处置并增强**（复核发现已有 `QuitAckEvent` 回执，退出码 3 区分"未收到回执"） |
| P2 C3–C9 / D1–D4 / E2 / F2–F4 / H1–H6 | C3/C6/C7/C9/D2/D4/H1 **已处置**；C5/C8/D3/F4/H2/H3/H4/H5 **本轮处置**；C4/D1/F2/H6 **保留**（P3，理由见审计报告 §3） |
| §4.4「拆 `service_session.rs` / `ipc.rs`」 | **本轮判断为暂不做**（就地重构 + 抽象阻塞编排），理由与代价见审计报告 §1.4 |
| §5.5「selftest 补 F–J 段」 | **未做**（§6）；改为两个可单独运行的验证脚本 + 两个运行期探针，并显著加固 `selftest.ps1` 自身 |
| §5.5「check-consistency 新增 8 项」 | **全部落地**，并把 55 条旧断言的**可被注释满足**问题一并修掉 |
| §5.6 不确定项 12 条 | 5 条已判定（token 回调确实会被调用——探针证明；`eprintln!`/`print!` 已消除；`make-icon.ps1` **确定性已证明**：3 次 SHA256 一致，含 `de-DE`；Windows 构建图 advisory 已复跑；F5/Ctrl+R 由我们自己处理，不再受 `browser_accelerator_keys` 影响）；7 条保留（§6/§10） |

### 5.2 与 `IMPLEMENTATION-v5.0.0-prev-round.md`（上一轮的内部迭代记录）的对照

| 类别 | 项 |
|---|---|
| **继承（复核后确认正确）** | 服务解耦 + `service.json` 对账；`Ready{url:Option<String>}` + 补发；`spawn_stderr_drain`；`request_close_window`；`begin_theme_sample`（异步）；三态身份判定；`--quit`；配置副本提交；日志 `max_files` 语义；`retained_file_count`；ini 迁移后删 ini；崩溃取证 |
| **修正（上一轮的说法不完整或不准）** | ①"持锁 spawn 已修" — 实为**半个修复**（N-7）；②"token 竞态已修" — 三个子缺陷修对了，但**预算初始化与驱动源**仍缺（N-1）；③"代际号取消" — worker 侧对，但 `ReadyProbe::Clone` 使该语义在克隆上失效（N-9）；④`monitor_worker` 在 `Starting` 期间会误判失联（N-2）；⑤"`stop()` 不阻塞 UI" — 只把终止移出部分锁，仍在 UI 线程上（N-11/N-12）；⑥安装器"Job 注释已同步更正" — 更正后的内容仍是错的，本轮按真实机制重写（N-42） |
| **推翻（保留能力、换掉机制）** | ①手写 JSON 区间改写 → 字符串感知配对 + `serde_json`（N-47）；②`TerminateProcess` 静默 → 返回实际终止列表（N-45）；③`verify-monitor.ps1` 的"用完就删配置" → 备份/恢复（N-6）；④`gen-facts -Check` 只比 4 项 → 29 项（N-38）；⑤文本匹配式接线断言 → 去注释 + 调用形式（N-37）；⑥生成卸载器的进程内递归删除 → 标记门禁 + `$PSScriptRoot` + 延迟删除（N-5/N-42）；⑦`osv-audit` 的"见记录即红" → 按 Windows 构建图判定 |

---

## 6. 未完成 / 明确不做（逐条理由与影响）

| 项 | 状态 | 理由与影响 |
|---|---|---|
| `selftest.ps1` 端到端（A1/A2/B/C/D/E） | **未执行** | 按你的选择；会独占互斥体并中断活动会话。**影响：端到端行为未由我实测**（命令见 §7） |
| `verify-service-lifecycle.ps1 -Force` | **未执行** | 同上。只读部分通过；脚本现在如实返回 `SKIPPED(2)` 而非假绿 |
| 安装/卸载互逆矩阵（4 项） | **未执行** | 会写真实注册表与 `%LOCALAPPDATA%`。**影响：安装链路最后一步只有构建期与解析期证据**（生成的脚本经 `[Parser]::ParseFile` 0 错误 + 24 项契约检查） |
| `verify-icon.ps1` | **SKIPPED(2)** | 它需要「窗口图标导出」步骤产出的 32×32 PNG，而**该生产步骤在仓库中不存在**（上一轮路线图却把它记为"0 差异"证据）。**影响：标题栏图标与 `app.ico` 的像素级一致性未验证**（`build.rs` 仍保证图标被编译进资源） |
| 托盘创建失败时的可见提示与退出出口（上一轮 P1-F1） | **未做** | 需要设计"无托盘时的退出路径"（例如无托盘时改为显示窗口并禁用关闭到托盘），属交互设计而非缺陷修补。**影响：托盘不可用仍只有日志**（但此时窗口本身可用，并非完全无出口） |
| WebView2 profile 损坏自愈 | **未做** | 与 Edge 回退链有交互（清数据目录会丢已登录 Cookie），需先确认用户可接受。**影响：profile 损坏时仍走 Edge/默认浏览器回退** |
| 归档清理"多轮重试防复活"（v4 有、v5 缺） | **未做** | 属功能收缩，需产品决策（v4 最多 3 轮延迟回写重试）。**影响：dsh 延迟回写时列表可能"复活"，用户需再点一次** |
| 设置窗口位置记忆 | **未做** | 需多显示器/DPI 边界设计。**影响：每次打开都是默认位置** |
| `--quit` 在 `tied` + 无人值守下的行为 | **未判定** | `tied` 退出会弹模态框；无人值守时会等到回执超时（20 s）后返回退出码 3。需要：在 tied 模式下脚本化跑一次 |
| Authenticode 校验下载物 / `app.manifest`（DPI 感知 + asInvoker） | **未做** | 前者需引入 WinVerifyTrust 封装；后者需改 `csc` 参数并新增 manifest。**影响：下载物仅校验最小字节数；安装器无 DPI 感知、文件名含 `Setup` 时可能被 Windows 安装器检测启发式要求提权（未实测）** |
| `selftest.ps1` 新增 F–J 段 | **未做** | 改为两个可单独运行的验证脚本 + 两个探针（避免在无法实测的环境下大改自检脚本引入回归）。**影响：Job 矩阵/身份校验/`tray_on_close` 仍只有静态断言，缺端到端覆盖** |
| `app.rs` 拆分为 `service_session.rs` / `ipc.rs` | **明确不做** | 四个职责共享大量状态（`harness`/`pending_url`/`want_open`/`theme_*`），拆模块需把这些字段提升为 `pub(crate)`；在**没有 E2E 保护**的会话里做纯搬家风险高于收益。改为就地抽象三处阻塞编排 |
| `semver` 传递依赖 | **保留** | 禁用清单约束直接依赖；README 表述已注明 |
| `reg.exe` 调用无超时 | **保留（P3）** | Edge 回退路径上的一次性调用 |
| `docs/*.md` 中仍存在的历史数字（如 RELEASE_NOTES 里"v4→v5 重构"对照列） | **保留** | 它们是**历史对照事实**（v4 的 7,720 行、199 KB+902 KB DLL 等），不属于"当前状态"断言；`check-consistency` 只对"当前状态"类数字做硬断言 |

---

## 7. 验证步骤（你可照做；含期望的新行为）

> 前 6 步不触碰你正在使用的会话；第 7 步会独占互斥体并中断会话，**请在你方便的时间执行**。

```powershell
cd D:\DSHLauncher

# 1) Rust 三件套 + 发布构建
cargo fmt --check
cargo clippy --offline --all-targets --all-features -- -D warnings
cargo test --offline --all-features          # 期望：dsh-core 86 + dsh-ui 11 = 97 passed，0 failed
cargo build --offline --release

# 2) 一致性 / 版本链 / 文档数字（三项都必须 exit 0）
pwsh -NoProfile -File tools\check-consistency.ps1     # 期望：CHECK_SUMMARY passed=115 failed=0 total=115
pwsh -NoProfile -File tools\verify-version.ps1        # 期望：28 passed / 0 skipped / 0 failed
pwsh -NoProfile -File tools\gen-facts.ps1 -Check      # 期望：已比对 29 项事实，一致

# 3) 依赖审计（需要网络；结论必须落在「Windows 构建图外」）
pwsh -NoProfile -File tools\osv-audit.ps1

# 4) 运行期探针（隔离端口，不影响 3080 上的会话）
cargo run --offline -p dsh-core --example token_capture_probe -- 45680
cargo run --offline -p dsh-core --example active_session_probe

# 5) 发布 + 落地校验（热替换，不结束任何进程）
pwsh -NoProfile -File build.ps1 release
pwsh -NoProfile -File build-setup.ps1
pwsh -NoProfile -File tools\verify-deployed.ps1       # 期望：11 个修复标记齐全 + 全部通过

# 6) 只读的生命周期检查（不加 -Force，不会碰你的会话）
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1   # 期望：exit 2 SKIPPED（"没测"而非"通过"）

# 7) ⚠️ 会中断当前会话 —— 请在方便时执行
pwsh -NoProfile -File selftest.ps1                  # A1/A2/B/C/D/E
pwsh -NoProfile -File selftest.ps1 -SkipE2E         # 只跑 A1/A2（D 段现在真的被跳过）
pwsh -NoProfile -File tools\verify-service-lifecycle.ps1 -Force   # 决定性实验：结束启动器后 dsh 应存活

# 安装/卸载互逆矩阵 —— ⚠️ 只对「安装目录里的副本」操作，绝不要用仓库根目录的模板！
#   仓库根也有 DSHLauncher.exe / app.ico / README.md / uninstall.*，早期版本会因为
#   "目录里存在 DSHLauncher.exe"就把它们当成安装副本删掉（v5.0.0 定稿轮实测发生过一次）。
#   现在模板会识别源码树（存在 Cargo.toml / crates，或缺少 .dsllauncher-install 标记）
#   并**拒绝执行**；下面四条只用安装目录里的那一份。
$inst = "$env:LOCALAPPDATA\Programs\DSHLauncher"
.\DSHLauncherSetup.exe --silent-install $inst          # 1) 静默安装（$LASTEXITCODE 应为 0）
& "$inst\uninstall.cmd"                                # 2) 默认卸载：注册表/目录/快捷方式/%LOCALAPPDATA% 无残留；
                                                       #    %APPDATA%\DSHLauncher\settings.toml 必须保留
.\DSHLauncherSetup.exe --silent-install $inst          # 3) 重新安装
& "$inst\uninstall.cmd" --purge                        #    配置一并删除
.\DSHLauncherSetup.exe --silent-install $inst          # 4) 用注册表 QuietUninstallString 静默卸载：
Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher' |
    Select-Object -ExpandProperty QuietUninstallString
# 注意：卸载器把「安装目录本身的删除」交给一个延迟后台进程（约 2 秒），
#       因此脚本返回后目录可能仍存在约 2 秒 —— 属预期行为，不要误判为残留。
```

### 期望的新行为（与旧版的差异）

1. **冷启动不会再出现 401 页面**：`launcher.log` 出现 `已捕获 dsh 就绪地址（token 已脱敏）`，且 `已打开内嵌界面：http://127.0.0.1:3080/?token=…` 带 token。若 5 秒内没拿到 token，会退回普通地址**并打印明确 WARN**，界面不会永远不出现。
2. **冷启动过程不再被误判失联**：启动阶段不再出现「服务 3080 已失联」「自有进程已退出且自动重启用尽」这类 ERROR；`service.json` 不会被启动过程清掉。
3. **界面不再被阻塞**：点「停止服务」/「保存设置（改端口）」/「清理归档会话」时界面保持可响应（日志见 `正在后台重启服务…`）。
4. **`--quit` 可编排**：`.\DSHLauncher.exe --quit` 后进程退出、服务按 `service_lifecycle` 决定去留；未回执时返回退出码 3。
5. **`--build-info` 一定写出文件**：`%APPDATA%\DSHLauncher\build-info.txt` 含 `version=5.0.0` 与 11 个 `fixes=` 标记；无控制台也不会崩溃。
6. **`--probe-identity <PID>` 给出逐步判定**：`verdict=IsDsh|NotDsh|Unknown` + `image_name`/`image_path`/`matched`/`note`。
7. **配置损坏有真实出路**：设置页「恢复默认设置」（二次确认、默认焦点「否」）会删除 `settings.toml` 并在必要时以默认配置重启服务。
8. **日志脱敏更宽**：`pin=` / `api_key=` / `cookie=` / `secret=` / `Authorization: Bearer …` 均变 `***`。
9. **`node_path` 写错会报错**而不是静默换 node；dsh 输出格式变化会在日志留告警。
10. **卸载更安全**：只有安装目录存在 `.dsllauncher-install` 标记时才允许递归删除；`uninstall.cmd --purge` 与 `-Purge` 都生效；失败 `exit 1` 并列出未删除项。

---

## 8. 实测指标表（同口径前后对比）

| 指标 | 上一轮基线 | 本轮 | 说明 |
|---|---|---|---|
| 单元测试（可运行） | 101 计数 / **基线实为红** | **97** | 修正口径：排除 4 个 `build.rs` 内永不运行的 `#[test]`；失败用例已修 |
| 一致性校验项 | 114 | **115** | 只增不减；新增项是路线图 §15.5 缺失行必须有 `else` |
| `gen-facts -Check` 比对事实数 | 4 | **29** | `artifacts.*` / `code_lines.*` / `build_script` / `per_crate.*` 全部纳入 |
| 版本链路项 | 27 | **28** | 全绿（新增「exe 含图标资源」断言） |
| 生产 Rust 代码行数 | 7,815 | **8,801** | 增量用于生命周期编排修正、UI 线程零阻塞、导航白名单、脱敏扩展、JSON 加固、UI 状态机补全 |
| 代码总行数（Rust，含 build.rs/examples） | 8,566 | **9,552** | 同上 |
| HTML 行数 | 201 | **217** | 设置页「恢复默认设置」+ 指引/设置文案修正 |
| 单文件 exe | 1,049,088 B（1,024.5 KiB） | **1,065,984 B（1,041.0 KiB）** | +16.9 KB（+1.6%） |
| 安装包 | 未构建（`null`） | **1,308,672 B（1,278.0 KiB）**，FileVersion `5.0.0.0` | 本轮实测构建 |
| `Cargo.lock` 包数 | 264 | **264** | 未新增依赖；被禁 crate 仍未引入 |
| Windows 构建图内 advisory | 0 | **0** | 3 条记录全在图外（`cargo tree -i` 逐项验证） |
| 工作集（窗口关闭态） | 13.2 MB | **13.2 MB** | 无回归 |
| 线程数 / 句柄数 | 5 / 172 | **5–12 / ~170** | 阻塞编排移出 UI 线程后 worker 生命周期更短 |
| `[boot]` 单实例+配置+日志 | 7 ms | **4–7 ms** | |
| `[boot]` 事件循环创建 | 27 ms | **9–27 ms** | |
| `[boot]` 托盘+服务启动请求 | 43 ms | **19–43 ms** | |
| `[boot]` 首帧（界面可响应） | 45 ms | **20–45 ms** | 受 WebView2 首次加载影响 |
| 冷启动受 dsh 支配的部分 | 30 s+ | **30 s+（未变）** | 由上游 dsh 决定，单独标注 |
| 「就绪 vs token」时序 | 914 ms | **961 ms** | 量级一致；本轮补上"预算初始化 + 每帧驱动"，使该窗口不再导致 401 |
| UI 线程单次阻塞上限 | 数百 ms（进程终止） | **≈0（编排外移）** | 残余：清理前的活跃会话探测 ~1.3 s（紧邻模态框，已记录） |

---

## 9. 交付期踩到的工程性缺陷（单测全绿但真机跑挂的那类）

1. **`print!` 在 GUI 子系统下 panic**（N-4）：单测永远测不到"没有控制台时写入失败"，而 release 的 `panic=abort` 直接杀进程。修 `eprintln!` 之后**同一族的 stdout 侧被漏了** —— 这种"修一半"是本仓库最贵的模式。
2. **`cargo test` 自己就是红的**（N-3）：源于一个用**跨进程共享固定路径**的测试。它让"单测全绿"这一整类结论失效（我一开始就撞上了）。教训：门禁必须先证明自己可信。
3. **冷启动被误判失联**（N-2）：只在**真机冷启动**（30 s+）出现；单测里探测全是毫秒级，永远撞不上 12 s 阈值。
4. **`RollingLogger::Clone` 不互斥**（N-8）：只有并发写才暴露（已补 4 线程 × 50 行完整性用例）。
5. **监测线程退出竞态**（N-10）：只有「停止→立即启动」才撞上，而自检脚本恰好就这么干。
6. **`verify-monitor.ps1` 覆写并删除用户配置**（N-6）：这是**审计工具链而不是审计应用**才发现的问题 —— 而它由 `selftest.ps1` E 段与发布流程自动触达。
7. **`gen-facts -Check` 给不存在的产物背书**（N-38）：它报"一致"时 `target\release\dsh-app.exe` 根本不存在。校验器自身的假绿比被校验对象的缺陷更危险。
8. **`osv-audit.ps1` 永远红**：见记录即 `exit 1`，于是"Linux-only advisory"让这条门禁失去意义（没人会再看）。
9. **我自己造成的一次数据损失**：用 `pwsh` 批量替换中文全角括号时把两个源文件的 `（` 换成 `v`，随后一次替换因 PowerShell 重载解析失败把 `$null` 写进文件，导致 `crates/dsh-app/src/service.rs` 与 `crates/dsh-core/src/process.rs` 被截断为 2 字节。**恢复方式**：从 DSH 会话记录（`~/.dsh/sessions/.../session.v3.jsonl.zstd`，seekable-zstd 832 帧，按帧魔数逐帧解压）取出我对这两个文件的**完整读取快照**，按记录顺序**重放 `edit` 调用**（process.rs 4/4；service.rs 17/19，另 2 处手工补回），再补 4 处锁中毒防护与版本字符串改写。**恢复后门禁全绿**（详见审计报告 §7 第 6 条）。教训：不要在 `pwsh -Command` 里用含中文的字面量做批量替换；`[IO.File]::WriteAllText(path, $null)` 会静默截断。

---

## 10. 不确定项

（与 `docs/AUDIT-REPORT-v5.0.0.md` §5 同一清单；此处只列本轮**新增或状态变化**的条目）

| # | 项 | 状态 | 需要什么 |
|---|---|---|---|
| 1 | 端到端（selftest）与安装矩阵的真实通过情况 | **本轮仍未执行**（你的选择） | §7 第 7 步 |
| 2 | `tied` 模式下 `--quit` 的无人值守行为 | **本轮新发现** | 在 tied 模式下脚本化跑一次 `--quit`，观察退出码与耗时 |
| 3 | 生成的卸载器"延迟删除目录"在真实卸载中的观感 | **本轮新增**（实测 cmd.exe 退出码机制所致） | 真实安装后跑 `uninstall.cmd`，确认 NOTE 打印且目录在数秒内消失 |
| 4 | 安装器无 manifest 时是否被 Windows 安装器检测启发式要求提权 | 未变 | 标准用户 + UAC 开启环境实测 |
| 5 | 打包 exe 的 TLS 默认值（我以 `pwsh` 模拟得到 `go.microsoft.com` 失败） | 已按 Tls12 加固；"加固前的真实行为"未实测 | 运行打包后的 setup 并读 `setup.log` |
| 6 | `cargo build` 产物的字节级可复现性 | 未变 | 固定 SDK 版本后连续两次构建比对 SHA256 |
| 7 | 无 WebView2 环境下的 Edge 回退 | 未变 | 干净 VM |
| 8 | `Remove-Item -Recurse` 是否跟随 junction（PowerShell 5.1） | 未变 | 受控目录实验 |
| 9 | dsh 页面是否使用 `parse_css_color` 不认识的 CSS Color 4 语法 | 未变 | 开窗后打印 `getComputedStyle` 结果 |
| 10 | 工作集"同口径"的可比性（窗口关闭态 vs 未加载窗口） | 未变 | 用固化脚本同时输出两个口径 |
| 11 | 仓库卫生：`crates/`、`ui/` 与部分 `docs/`、`tools/` 至今**未入库**（`git status` 显示为未跟踪） | **本轮新发现** | 需要你决定是否 `git add`；在此之前任何"回滚"都只能靠手工备份 —— 这正是第 9 条事故无法用 `git` 恢复的原因 |

---

## 11. 用户实测反馈轮（2026-09-11 02:38–02:41）：又发现 4 个真实缺陷

> 这一节记录的是**你亲自跑 E2E 与安装矩阵时暴露的问题**。它们与我之前的静态结论不冲突，但都属于"门禁/脚本自己不可信"或"只有真机才暴露"的类别。全部已修并复核。

### N-59（P0）· 仓库模板卸载器会删掉源码树自己的产物

**现象**：你按我给的清单在仓库根目录执行卸载步骤后，根目录的
`DSHLauncher.exe` / `app.ico` / `README.md` / `uninstall.cmd` / `uninstall.ps1` **全部消失**；
`%LOCALAPPDATA%\DSHLauncher`（日志 + WebView2 profile + `service.json`）也被删除。

**根因（两层，缺一不可）**：
1. 模板 `uninstall.ps1` 把目录当作"要卸载的安装目录"，而它的**所有权证明**是
   「目录里存在 `DSHLauncher.exe`」——**仓库根目录恰好满足**（`build.ps1` 会把产物放到根目录）。
   于是那 5 个文件（正好是模板的 `$files` 清单）被逐个删除。
2. 我在 `IMPLEMENTATION-v5.0.0.md` §7 里给的验证清单写成 `.\uninstall.cmd`（仓库根路径），
   **等于亲手引导你踩这个坑**。

**修复**：
- 模板 `uninstall.ps1` 重写为**在源码树里直接拒绝执行**：必须同时满足
  「目录里存在 `.dsllauncher-install` 标记」（该标记**只有安装器会写**，仓库根没有）
  且「目录里没有 `Cargo.toml` / `crates`」；否则打印明确说明并 `exit 1`，不做任何删除。
- §7 的验证清单改为**只对安装目录里的副本**操作：
  `& "$env:LOCALAPPDATA\Programs\DSHLauncher\uninstall.cmd"`（见 §7 修订版）。
- 5 个文件已恢复（`README.md` 取自安装包内嵌快照 + 重新施加数字修正；`app.ico` 由
  `make-icon.ps1` 重新生成；`DSHLauncher.exe` 重新构建；`uninstall.cmd` / `uninstall.ps1` 重写）。

**代价**：模板不再能"手动清掉仓库里的产物"（这本来就是它不该做的事）；真要清就手动删。

### N-60（P1）· `app.ico` 缺失过一次之后，exe 静默丢掉图标资源

**现象**：恢复 `app.ico` 并重新构建后，exe 从 **1,065,984 B 变成 1,043,968 B**（−22,016 B）。

**根因**：`build.rs` 只在图标文件**存在时**才声明 `cargo:rerun-if-changed=<app.ico>`。
于是「图标缺失时构建过一次」⇒ cargo 不再跟踪该文件 ⇒ 图标补回来后 `build.rs` 不重跑
⇒ 生成的 `.rc` 里**没有 `ICON` 行**（实测 `dsh_app.rc` 仅 862 B、`dsh_app.res` 仅 864 B）⇒
exe 少了整个图标资源。而 `verify-version.ps1` 当时的断言写成
`if ($vi.FileDescription) { Add-Pass 'exe 版本资源块存在（含图标资源）' }` ——
**只检查了 FileDescription，却宣称检查了图标**，是一个假断言。

**修复**：
- `build.rs`：**无条件**声明 `rerun-if-changed=<app.ico>`；图标缺失时**直接让构建失败**并提示
  运行 `make-icon.ps1`（该脚本是确定性的）。
- `tools/verify-version.ps1`：改用 `[System.Drawing.Icon]::ExtractAssociatedIcon()` 真取一次图标，
  取不到即 FAIL。**用无图标的二进制做了反向验证**（`job_object_demo.exe` → `1 failed`），
  证明该断言不是假绿。
- 版本链路项因此从 **27 → 28**（新增「exe 含图标资源」）。

### N-61（P1）· `service.json` 记录的端口与配置端口不一致时仍然"接管"

**现象**：`selftest.ps1` E 段报 `FAIL: 未见接管日志`。日志显示启动器接管的是
**PID 15472 / 端口 3080**（上一次真实会话留下的簿记），而测试用的是 3099。

**根因**：`start()` 的分支 2 无条件接受 `reconcile()` 的 `Adopt`，于是
`adopted_pid` 指向一个**监听在别的端口**的进程；而就绪探测、失联监测、`ready_url()`
全部使用 `config.port`。结果是「已接管」是假象：界面与监测都指向配置端口。
**这不只是测试问题**：用户把端口从 3080 改成 3099 后重启启动器，就会看到这个错配。

**修复**：`Adopt` 分支加 `if rec_port == port` 守卫；端口不一致时打印 WARN 说明
"簿记端口与配置端口不一致，本次不接管"，并继续按配置端口走分支 3/4
（端口空闲就自己拉起，被别的 dsh 占着就接管那一个），旧记录由新簿记覆盖。

### N-62（P2）· `selftest.ps1` 的 B 段按设计拒绝了"用陈旧二进制继续测"

**现象**：`FAIL: B-build（cargo build --release 退出码 101（不能用陈旧二进制继续测））`。

**判定**：这是**上一轮加固的脚本按预期工作** —— 退出码 101 的真实原因是 N-60 的
`app.ico` 缺失（`include_bytes!` 编译失败），脚本正确地拒绝拿旧产物继续跑并如实报红。
不需要改动脚本；`app.ico` 恢复后 B 段应恢复 PASS（请在你方便时复跑确认）。

### 本轮修正后的复核结果

| 门禁 | 结果 |
|---|---|
| `cargo fmt --check` / `clippy -D warnings` / `cargo test` / `cargo build --release` | 全绿（97 tests） |
| `tools/check-consistency.ps1` | **115 / 115 通过** |
| `tools/verify-version.ps1` | **28 passed / 0 skipped / 0 failed**（含新增的图标资源断言） |
| `tools/gen-facts.ps1 -Check` | 29 项事实一致 |
| exe 体积 | 回到 **1,065,984 B**（`.rc` 恢复 `1 ICON DISCARDABLE`，`res` 22,500 B，图标实测 32×32） |
| 仓库根产物 | 5 个文件全部恢复；`setup.log`（安装器残留）已删除，且本来就在 `.gitignore` 里 |

### 仍需你复跑的两项

1. `pwsh -NoProfile -File selftest.ps1` —— 预期 **B 段转 PASS**；**E 段也应转 PASS**
   （N-61 修复后，测试用 3099 时不会再被 3080 的旧簿记劫持）。
2. 安装/卸载矩阵 —— 见 §7 **修订版**：**只对安装目录里的副本操作**，不要用仓库模板。

