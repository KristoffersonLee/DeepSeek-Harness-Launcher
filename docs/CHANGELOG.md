# Changelog / 更新日志

> 📖 **版本导航 / Version Navigator：** [v5.0.0](#v500-2026-09-10) · [v4.2.4](#v424-2026-09-10) · [v4.2.3](#v423-2026-09-09) · [v4.2.2](#v422-2026-09-09) · [v4.2.1](#v421-2026-09-09) · [v4.2.0](#v420-2026-09-09) · [v4.1.0](#v410-2026-09-08) · [v4.0.0](#v400-2026-09-05) · [v3.0.0](#v300-2026-09-01) · [v2.0.0](#v200-2026-08-31) · [v1.0.0](#v100-2026-08-14)

---

<a name="v500-lts-toolchain-2026-09-12"></a>
## v5.0.0 LTS · 工具链迁移轮（2026-09-12）：主工具链切到 nightly + build-dir Layout v2

> 🏷 **版本不变**：`5.0.0`；`LTS` 仍是发布标签。本轮只改**构建工具链与缓存布局**，
> 不改任何用户可见行为 —— 产物版本资源、`--version`、`--help`、自检 A1–E 全部照旧通过。

| 项 | 变更 |
|---|---|
| 主工具链 | `stable 1.98.1` → **`nightly-2026-09-10`**（由 [`rust-toolchain.toml`](../rust-toolchain.toml) 钉死，含 clippy/rustfmt） |
| 构建缓存布局 | 旧布局（`deps\` + `.fingerprint\`）→ **Layout v2**（`target\<profile>\build\<包名>\<哈希>\{out,fingerprint}\`） |
| 为什么必须 nightly | v2 **目前只在 nightly 提供** —— `1.98.1`（`rustup check` 确认的当前最新 stable）仍是旧布局；上游稳定化仍在推进（见 `CFT-FEEDBACK.md`） |
| 为什么钉死**日期** | nightly 每天前进；工具链版本会改变产物字节、`rustfmt` 输出与 `clippy` 规则集（后者直接影响 `-D warnings` 门禁） |
| 缓存回收 | 迁移时 `clean.ps1 -Cache` 回收 **2,994.3 MB**（旧布局缓存对 v2 全部失效） |
| 产物尺寸 | 启动器 1,078,784 → **1,087,488 B**；卸载器 309,760 → **316,928 B**；安装包 1,618,432 → **1,641,472 B**（nightly 编译；`FACTS.json` 已按实测重生成） |
| 回退 | `Remove-Item rust-toolchain.toml` —— 一条命令回到 stable 旧布局，**无需改任何源码**；完整步骤见 [OPS-RUNBOOK §8](OPS-RUNBOOK.md) |

**门禁与自清洁工具无需为 v2 改动**（已逐条核实，不是推断）：

* `clean.ps1` 的唯一不变式只是 **6 个 release 交付物**，删除逻辑是"除保留项外全删 +
  遇到含保留项的目录就递归"，因此对 `deps\`/`.fingerprint\`/旧式 `<pkg>-<hash>`/v2 式
  `<pkg>\<哈希>` **一律无感**（`-Cache -WhatIf` 实测把两种布局合并为一个 `build` 条目处理）；
* 没有任何脚本硬编码构建目录内部布局（`check-consistency` 与 `verify-build-layout.ps1` 持续强制）；
* `selftest.ps1` 的 example 路径本来就取自 `cargo --message-format=json`，不猜目录。

**工具链升级顺带修掉的一处依赖卫生问题**：新 cargo 会对"在 `[workspace.dependencies]` 中声明、
却没有任何成员引用"的条目发出 `unused_workspace_dependencies` 警告 —— 实测报出 `dsh-core` /
`dsh-ui` 两项（三个成员当时用的是裸 `{ path = "../..." }`）。已统一改为 `workspace = true`
（`Cargo.lock` **零变化**，语义等价），并新增门禁断言钉死，防止再次漂移。

**v2 生效后的工具同步复核（本轮第二遍，因此门禁 177 → 178）**：把主工具链切过去之后，
又重新审了一遍"自清洁与其他工具是否绑死了旧布局"。结论：全库无脚本依赖构建目录的内部布局
（静态扫描 + 逐工具实跑双证），只有**一处**需要同步 —— `clean.ps1 -Cache` 的保留清单漏了
`CACHEDIR.TAG`：实测 cargo 在 stable / v2 / `CARGO_BUILD_BUILD_DIR` 三种配置下**都会**写它，
但 cargo **只在创建构建目录时**写、之后不补写，于是被 `-Cache` 删掉后本仓库的 `target\` 真的
丢了该标记（1.9 GB 缓存不再被备份软件跳过，`legacyBuildDirs` 的识别判据也会在这类目录上失效）。
已把它加进保留清单并用 cargo 自己产出的字节恢复标记；另新增门禁断言
「`assets/whale-path.txt`（make-icon 的唯一输入）必须存在」—— 起因是本轮清理"旧版遗留资产"时
我用 `Select-String -SimpleMatch` 误判该文件零引用而误删，导致 `build.ps1`/`build-setup.ps1`
双双退出码 1；已从 DSH 会话记录恢复并用"确定性重建的 `app.ico` SHA256 一致"证明逐字节等价，
事故记录与预防规则见 [`OPS-RUNBOOK.md`](OPS-RUNBOOK.md) §7.7。

**本轮验证**：`fmt --check` 0 · `clippy -D warnings` 0（**且构建输出无 manifest 警告**）·
单测 **149/149** · 一致性门禁 **178/178**（PS 5.1 与 PS 7 同结果）· 版本链路 **33/0/0** ·
`gen-facts -Check` 一致 · 模拟安装 dry-run PASS · `verify-build-layout.ps1 -Run -IncludeNightly` 通过 ·
`selftest.ps1` A1/A2/B/C/D/E 全部 PASS · `finish-release.ps1` **14 步全通过**（含 cargo-audit 与
cargo-machete，见 [`FINISH-REPORT.md`](FINISH-REPORT.md)）。

---

<a name="v500-lts-final-2026-09-11"></a>
## v5.0.0 LTS · 最终深度审查轮（2026-09-11）

> 🏷 **版本不变**：`5.0.0`（合法 semver）；`LTS` 仍是发布标签。本条目记录 **5.0.0 LTS 发布后的
> 第三轮无禁区深度审查**（架构 / 缺陷 / 性能 / 内存 / 冗余 / 安全 / 可观测性 / 发布工程）的落地结果。
> 📄 逐条处置记录见 **[最终轮实施记录](IMPLEMENTATION-v5.0.0-lts-final.md)**；
> 验证证据见 **[发布验证记录](RELEASE-VERIFICATION-v5.0.0-LTS.md)**；数字以 **[FACTS.json](FACTS.json)** 为准。

### 🔴 缺陷修复（P0/P1）

| 问题 | 方案 | 结果 |
|---|---|---|
| **关闭到托盘的窗口再也回不来**（P0）：`pump_pending_open` 在 `harness.is_some()` 时直接返回，使 `open_harness` 里「显示已存在窗口」的分支**永不可达** —— 托盘「打开界面」与双击图标的唤起都毫无反应（日志却写着「已激活内嵌窗口」）。默认 `tray_on_close = true`，因此这是**默认路径**。 | `open_harness` 对已存在窗口**无条件显示并聚焦**（地址未就绪时也显示）；`pump_pending_open` 不再短路；`Ready`/重启走新增的 `navigate_existing_harness`（只导航不弹窗，避免服务恢复时抢焦点）。 | 关闭到托盘与托盘打开/双击唤起闭环；新增门禁断言锁死回归。 |
| **旧版 `settings.ini` 迁移绕过校验**：迁移结果直接返回，`port=0` 这类非法值会一路走到 `--port 0` 启动服务。 | 迁移结果与 TOML 路径走**同一套** `validate()`；失败即明确报错（并保留 ini 不删）。 | 非法旧配置不再产生「服务起不来且日志无线索」。 |
| **「恢复默认设置」不清理旧 ini**：删除 `settings.toml` 后，陈旧的 `settings.ini` 会在下次启动被重新导入，用户点了重置却「没生效」。 | `reset_on_disk` 同时删除 `settings.toml` 与 `settings.ini`，且失败不再静默（`remove_if_exists` 如实上抛）。 | 重置语义完整；新增单测。 |
| **外部命令探测可能泄漏常驻进程**：`run_capture_within` 用 `child.stdout.take()?` 早退，`Child` 被丢弃时**不会**终止子进程。 | 拿不到管道时先 `kill` + `kill_process_tree` + `wait` 再返回。 | 不再留下无人看管的 PowerShell。 |
| **进程创建时间只保留整秒**：与「防 PID 复用」的 2 秒容差叠加后，同一秒内复用的 PID 无法区分。 | `FILETIME → SystemTime` 保留 100ns 精度（换算为 `Duration::new(secs, nanos)`，用 `checked_add` 防回绕）。 | 判定精度提升；新增单测（含「早于 UNIX 纪元返回 None」）。 |
| **卸载器拒绝卸载任何真实安装**（P0）：安装器 `BuildMarkerText()` 写入的标记首行是**产品名** `DeepSeek Harness Launcher`，而卸载器要求标记内容含**内部标识** `DSHLauncher` —— 两者互不包含，于是每一台真实安装都被自己的卸载器拒绝（退出码 2，什么都不删）：注册表卸载项、桌面快捷方式、载荷文件与 `%LOCALAPPDATA%` 运行期数据全部残留。 | 卸载器改判 `marker_is_ours()`（接受内部标识**或**产品名）；安装器在标记里显式追加 `product=DSHLauncher` 行；门禁新增三方交叉校验（Rust 常量 ↔ 卸载器判定 ↔ C# `AppName`）。 | 模拟安装目录 dry-run 实测通过（此前 exit 2）；真实安装可正常卸载。 |
| **崩溃诊断行可能被静默截断**：256 字节栈缓冲装不下约 200 字节的中文行加 pid/tid 时，`Writer` 静默丢弃尾部。 | 缓冲扩到 384 字节并新增单测断言「未填满且以换行结尾」。 | 崩溃取证信息完整。 |
| **`--version` / `--help` 输出被静默丢弃**（P1）：GUI 子系统下 `emit_console_text` 无条件 `AttachConsole(ATTACH_PARENT_PROCESS)`，把标准句柄指向控制台，于是「重定向到文件/管道」的 stdout 全部丢失 —— 实测 `DSHLauncher.exe --version > ver.txt` 得到 **0 字节**，正是安装包与 CI 核对版本的那条命令。 | 先判定进程是否已有可写的 stdout 句柄（`has_stdout_handle()`），有则直接写、尊重重定向；只有完全没有句柄（资源管理器双击）才借用父控制台。 | `--version` 输出 `DSHLauncher 5.0.0 LTS (release)`；重定向到文件 32 字节；新增门禁断言。 |

### ⚡ 性能与内存

| 问题 | 方案 | 结果 |
|---|---|---|
| **进程枚举被反复全表快照**：身份判定最坏 8 次；卸载器对**每个 PID** 各枚举一次（300 进程即 300 次全表枚举）；进程树回收每个节点一次。 | 新增 `ProcessTable`（一次 toolhelp 枚举后按 PID 查映像名/父进程/子进程）；身份判定、进程树回收、卸载器共用同一份快照；进程树改为**每层一次**快照加终止后一次补偿扫描。 | 快照次数从 O(进程数×查询) 降到 1 / O(树深)；新增单测与门禁断言。 |
| **日志每行 open + write + close**：UI 线程（托盘/启动计时/状态）与 stderr 排空线程上都是重复的打开与路径解析。 | `Sink` 持有**常开句柄**；滚动判定改为对句柄 fstat；滚动前显式关闭句柄再改名（Windows 语义），阈值判定改为「先滚动再写」。 | 每行省 2 次系统调用与 1 次路径解析；新增滚动与并发整行的单测。 |
| **归档清理对每个归档 id 重新枚举 sessions 根目录**。 | 一次性收集 workspace 目录后复用。 | N 个归档从 N 次目录枚举降到 1 次。 |
| **Edge 回退路径派生 `reg.exe`** 查询注册表再解析文本。 | 改用 `RegGetValueW`（Win32 注册表 API，已启用 feature），并补 `WOW6432Node` 视图。 | 少一个子进程与一次文本解析。 |
| **UI 线程上探测活跃会话**（`Get-CimInstance` 实测约 1.3 秒）。 | 新增 `BackgroundTask::CleanupPrecheck`：探测（活跃会话加归档数）移到 worker，确认框仍在主线程。 | 「点清理」不再冻结界面一秒以上。 |
| **`redact_secrets` 快路径仍会分配一份小写副本**。 | 已评估：改用免分配的大小写折叠扫描会把每行常数从 1 次线性扫描抬到 18 次（CPU 反而劣化），故**保留原实现**并在此说明取舍。 | 维持现状（有意为之）。 |

### 🧹 冗余与源码精简

- 删除 `DshError::Io`（全仓零构造点）、`ProcessManager::take_stdout/take_stderr`（零调用方，且是「取走 stdout 即子进程假死」的陷阱 API）。
- 合并 `guards::full` 与 `plan::normalize` 两份**逐字重复**的路径规范化实现（卸载器的「准入校验」与「比较」绝不允许分叉）；删除 `is_protected_root` 中不可达的第三个分支。
- 测试假覆盖修复：旧版 ini 迁移测试**把解析循环抄了一遍**，生产代码那条路径从未被执行；现抽出 `Settings::parse_ini` 并直接测它（含大写键名、CRLF、非法端口、空内容）。
- `lib.rs` 的目录回退逻辑合并为 `dir_or_current`（配置目录与运行期目录共用同一套退化语义）。
- `steps.rs` 的载荷清单只计算一次（删除与复核共用）。

### 🛡 安全与可观测性

- `HarnessWindow` 导航白名单的文档与实现对齐（明确允许 `about:blank` / `data:text/html` / `blob:` / 回环字面量，其余拒绝），并说明该窗口不注册 IPC，因此这两类来源拿不到任何能力。
- 崩溃取证、身份判定、生命周期策略新增单测；`ServiceState::is_active` 明确排除 `Stopping`（否则「用户主动停止」会被失联监测误报成「失联」并自动重启），并注释说明该分支为何只可能是 `Error`。

### 🚀 发布工程（本轮最重要的可用性修复）

| 问题 | 方案 | 结果 |
|---|---|---|
| **README 承诺的构建入口在中文 Windows 上跑不起来**（P1）：`powershell -ExecutionPolicy Bypass -File build.ps1` 在 ANSI 代码页为 gb2312 的机器上，会把 UTF-8 的中文注释按 GB2312 解码、吞掉引号与反斜杠，直接抛 `Missing closing` 与 `Unexpected token` 解析错误。 | 全部 `.ps1` 加 **UTF-8 BOM**（PS 5.1 据此按 UTF-8 解码；PS 7 不受影响）。 | `powershell -File build.ps1 release` 与 `build-setup.ps1` 实测通过；新增门禁断言。 |
| **校验脚本在 PS 5.1 下产生 35 项假失败**：`Get-Content` 默认按代码页解码，读 UTF-8 源码/文档得到乱码，所有中文断言失败。 | 每个含非 ASCII 的 `.ps1` 固定 `$PSDefaultParameterValues['Get-Content:Encoding'] = 'UTF8'`。 | `check-consistency.ps1` 在 **PS 5.1 与 PS 7 下均通过**（当轮为 153/153；当前总数以 `FACTS.json` 为准）；新增门禁断言。 |
| 文档数字与实测漂移。 | `tools/gen-facts.ps1` 重新实测并同步 README / CHANGELOG / 路线图 / 发布说明的数字。 | `gen-facts.ps1 -Check` 全绿（31 项事实）。 |
| **自清洁工具覆盖面不足且会误报失败**：只认 `target`（仓库根残留与 `%TEMP%` 自检残留无人管）、清理清单是**子目录白名单**（漏 `tmp`/`doc`/`package`/新 profile）、删不掉被占用副本时报失败、且不知道清理是否破坏了 `FACTS.json`。 | 重写 `tools/clean.ps1`：四区域（target / 仓库根 / %TEMP% / 运行期数据只报）+ 可组合开关（`-Cache` / `-All` / `-Repo` / `-Temp`）+ **除 release 交付物外全部清理**（对未来 cargo 目录免疫）+ `%TEMP%` 白名单制并排除 `dsh-spill-*`/`dsh-subprocess-*` + 清完**自校验 FACTS** + 被占用项降级为「跳过」。门禁 6 条断言覆盖上述契约。 | 实测 `-Cache` 回收 3.1 MB 且 `FACTS` / `verify-version` / `check-consistency` 仍全绿；`-Repo -Temp` 清掉历史残留（含早期轮次的 300 KB 探针）；`-WhatIf` 零改动。 |
| **不兼容 Cargo 的构建目录新布局（CFT: Build Dir Layout v2）**：`clean.ps1` 只清 `target\`（Cargo 1.91+ 可用 `CARGO_BUILD_BUILD_DIR` 把中间产物整块搬走，实测 538 MB 会被漏掉）；`finish-release.ps1` 的缓存体积只统计 `target\`（少报最大一块）；`selftest.ps1` 硬编码 `target\debug\examples\...`；`.gitignore` 未覆盖被搬迁的构建目录。 | 四区域自清洁改为**解析生效的构建目录**（env + `.cargo/config.toml`）；`finish-release` 同时统计两者；`selftest` 的 example 路径改由 cargo `--message-format=json` 报告取得；`.gitignore` 补 `/build/`、`/build-layout-probe/`；新增 `tools/verify-build-layout.ps1`（静态断言 + 三配置实测）与 4 条门禁断言。 | 实测三种配置全绿：默认布局 / `CARGO_BUILD_BUILD_DIR`（1086 个中间产物搬到该目录，`clean.ps1 -Cache` 计划包含它）/ nightly 新布局（`target\release` 顶层出现按包名分桶的 `build`）；最终产物路径三者一致。 |

### 🩹 卸载残留清理（安装目录被删除后的悬空状态）

| 问题 | 根因 | 处置 |
|---|---|---|
| 安装目录被删除后，注册表卸载项与桌面快捷方式**永远清不掉**：两者都指向不存在的文件，「设置 → 应用」里的卸载按钮只会报找不到可执行文件。 | 卸载器的安装目录来自**它自身所在的目录**；目录没了就没有正规入口 —— 源码树里的副本被源码树护栏拒绝（实测 exit 2），`--deferred-pass <dir>` 被"缺少安装标记"拒绝（实测 exit 2）。 | 新增 `--clean-residue`（残留清理模式）：只清"磁盘外"的两处残留（注册表项 + 桌面快捷方式），**不做任何文件系统变更**；准入要求注册表 `InstallLocation` / `UninstallString` / `DisplayIcon` 三者互相印证，且安装标记**已不存在**（安装目录仍完整时明确拒绝并引导走正规卸载）；受保护路径与源码树同样拒绝。 |
| 文档数字漂移：`--build-info` 实际输出 **16** 个修复标记，发布验证记录写 15；CHANGELOG 英文指标表把一致性项写成 **147**（中文为 165）。 | 手写数字没有门禁覆盖（`gen-facts.ps1 -Check` 只比对源码实测，不读文档）。 | 修正为实测值；README 中英、CHANGELOG 中英、路线图 §15.5 的数字统一以 `FACTS.json` 对齐。 |
| 上一轮把"新布局下 example 同时存在于 `build/<pkg>/<hash>/out/` 与 `examples/`"当作实测结论，本轮逐路径复核**无法证实**（本仓库 debug 树是旧布局，`target\debug\build\dsh-core\` 根本不存在）。 | 该结论未逐路径复核。 | 改为只陈述已核实的 **bin/lib** 观察：`build/dsh-app/<hash>/out/dsh_app.exe`（1,087,488 B）与 uplift 后的 `target\release\dsh-app.exe`（1,078,784 B）**不是同一个文件**（`fsutil hardlink list` 为证）；example 的疑问明确写成"本仓库无法回答、需上游文档说明"（见 `docs/CFT-FEEDBACK.md`）。 |
| 自检 **E 段**（失联检测）报 `adopt` / `detect` / `readopt` 三项失败，看起来像产品故障。 | `tools/verify-monitor.ps1` grep 的措辞「已接管端口」在产品里**已不存在**（现在打印「直接接管」）⇒ 该步**必然**失败；且它默认用端口 **3099**（本机正是**用户当前会话**的端口），又只检查"端口在听"就断言"外部 dsh 就绪" ⇒ 自测的 dsh 从未绑上端口，后三步全是**误导性**失败。 | 判据改为产品实际措辞，并由门禁**交叉印证**（产品源码含「直接接管」+ 脚本按同一措辞判定）；默认端口改为**自动挑选空闲端口**；新增 `Assert-Owner`（端口监听者必须是本次启动的进程）；端口被占用即报错退出，且**绝不结束不是自己启动的进程**（旧实现会 `taskkill` 掉端口所有者）。复核确认**产品侧行为正确**；修复后 E 段与完整 `selftest.ps1` 全部通过。 |

残留清理实测（真实悬空状态：安装目录已不存在、注册表项与快捷方式仍在）：`--clean-residue
--dry-run` 正确列出影响范围且**零改动**；真实执行后注册表项与快捷方式被移除，而
`%APPDATA%\DSHLauncher`（5 个文件）与 `%LOCALAPPDATA%\DSHLauncher`（416 个文件）**逐个不变**；
重复执行返回 `0` 并报告"没有注册表残留"。门禁新增 7 条断言（模式存在、帮助文本、零文件系统
变更、三值印证、护栏复用、dry-run、快捷方式匹配规则单一实现）。

**验证摘要**（详细见发布验证记录）：单元测试 **149**（`dsh-core` 97 · `dsh-ui` 13 · `dsh-app` 12 · `dsh-uninstall` 21 · `dsh-buildinfo` 6）；一致性门禁 **178/178**（PS 5.1 与 PS 7 同结果）；`clippy --all-targets --all-features -D warnings` **0 警告**；`cargo fmt --check` **通过**；依赖审计（OSV 通道）**Windows 构建图内 0 漏洞**（`glib` / `proc-macro-error` 均仅在非 Windows 依赖图中）。

---

<a name="v500-2026-09-10"></a>
## v5.0.0 (2026-09-10)

> 📌 **[中文](#v500-中文)** · **[English](#v500-english)** · 📄 详细说明见 **[发布说明](RELEASE_NOTES_v5.0.0.md)** / See the **[release notes](RELEASE_NOTES_v5.0.0.md)**
> 🏷 **发布标签：5.0.0 LTS（长期支持）** —— Cargo 版本仍是合法 semver `5.0.0`；"LTS" 只作发布标签，见 `--version` 输出、文档、安装向导与制品名。
> 🏷 **Release label: 5.0.0 LTS** — the Cargo version stays the legal semver `5.0.0`; "LTS" is a release label only (see `--version`, docs, installer wizard, artifact names).
> 📄 定稿轮完整处置记录见 **[实施记录](IMPLEMENTATION-v5.0.0.md)**；问题清单见 **[深度审查报告](AUDIT-REPORT-v5.0.0.md)** / Full finalisation-round record: **[implementation record](IMPLEMENTATION-v5.0.0.md)**; findings: **[audit report](AUDIT-REPORT-v5.0.0.md)**

> ⚠️ **从零开始的全量重写**：C# WinForms → **Rust**，架构、依赖与构建链全部更换。本版包含**定稿前审计轮**的全部整改（原内部迭代标签已并入 v5.0.0）。

<a name="v500-中文"></a>
### 中文更新说明

#### 🎯 重构

- **Rust 全量重写**：`dsh-core`（零 GUI 依赖纯逻辑）/ `dsh-ui`（窗口·托盘·主题）/ `dsh-app`（唯一入口）
- **技术栈**：`wry`（WebView2 COM 直连）+ `tao` + `tray-icon` / `muda` + `windows` + `std::thread` + `mpsc`
- **刻意不引入**：`tauri` / `tokio` / `regex` / `chrono` / `semver` / `tracing-*`（依赖精简 + 离线可构建，由校验工具强制）
- **构建链**：`csc.exe` → `cargo build --offline`；`Cargo.lock` 入库；release 启用 `opt-level=z` + `lto` + `panic=abort` + `strip`

#### 🗑 破坏性变更：局域网共享彻底移除

- 删除 `lan-gateway.mjs`(1098 行) / `LanAccess.cs`(680 行) / `DSHLauncher.cs`(4275 行) / `whale-256.png` / 3 个旁挂 DLL —— **约 6,400 行 + 902 KB**
- 同时移除 PIN/令牌/会话密钥、防火墙规则与 UAC 提权路径、Ollama 暴露、二维码与移动端 UI
- **入站监听面归零**（仅 `127.0.0.1`）；不再需要 .NET Framework 与旁挂 DLL

#### 🆕 新功能

- **失联自愈与自动重连**：每 1.5 秒探测，约 12 秒判定失联；自有服务自动重启（≤3 次），接管实例持续看守并在端口恢复后**自动重新接管 + 界面重连**
- **孤儿锁自动恢复**：读取 `<file>.lock` 中的**持有者 PID**，仅当该进程确已退出才清理（活跃锁绝不触碰）
- **设置页 IPC**：保存 / 启动 / 停止 / 打开日志目录 / 清理归档会话，含配置回填与状态回显
- **命令行参数**：`--selftest` · `--settings`/`-s` · `--guide`/`-g` · `--ipc-probe`
- **原生指引与关于对话框**

定稿轮新增：

- **`--quit`**：请求已在运行的实例**优雅退出**（发布/自动化脚本用它替代强杀 —— 强杀会切断会话）。
- **`--build-info`**：打印构建与修复标记，并写入 `%APPDATA%\DSHLauncher\build-info.txt`。这是一个**可执行的产物判定**入口；用"在二进制里搜字符串"判断版本是错的 —— `debug_assert!` 与注释都不会进入 release 产物（本轮踩过）。
- **`--probe-identity <PID>`**：诊断用 —— 打印启动器对某个 PID 的**完整身份判定过程**（镜像名 / 映像路径 / 是否系统目录 / 命中的判定依据 / 无法判定的原因），并写一份 `identity-probe.txt`。存在的意义：身份判定是"防误杀"的关键闸门，一旦误判会导致**接管失败 + 服务停掉后起不来**，而这类失败在日志里只有一行结论、看不出卡在哪一步。
- **崩溃取证**：`panic = "abort"` 下注册 `SetUnhandledExceptionFilter`，崩溃时在日志留下一行 `[FATAL] 未处理异常 code=0x… address=0x… pid=… tid=…`（零内存分配）。
- 设置页新增「服务独立于启动器（推荐）」开关。

#### 🔧 修复（实现期发现，均为真实缺陷）

- **启动器启动即卡死**：`ServiceHandle` 持锁时重复加锁（`Mutex` 不可重入）→ **自死锁**；改为免加锁辅助函数
- **接管已有服务时不打开界面**：接管分支未发 `Ready` → 双击后界面永不出现
- **WebView2 在 exe 旁生成数据目录**：破坏单文件分发，且**只读安装目录下会直接失败**；现固定到 `%LOCALAPPDATA%`
- **设置窗口按钮全是死的**：此前无 IPC，点击只改文字
- **双击出现终端窗口**：Rust 默认 CONSOLE 子系统；release 现声明 `windows_subsystem = "windows"`
- **窗口标题栏不显示 logo**：现解析内嵌 `app.ico` 设置窗口图标
- **dsh 孤儿锁导致启动失败**（`atomic-write: timed out waiting for the writer lock`）：现按 PID 存活性安全清理
- **配置迁移不落盘** / **配置损坏静默降级** / **单实例测试依赖外部状态** / **重复的 `single_instance` 实现**
- **测试假通过**：GUI 子系统下 PowerShell `&` 不等待进程，`$LASTEXITCODE` 残留 → 改用 `Start-Process -Wait -PassThru`；`--selftest`/`--ipc-probe` 互斥体冲突时返回 **2**（原为 0）

#### 🔴 定稿轮：严重修复（两个 P0）

- **启动器只剩托盘、界面永不出现（确定性死锁）**：`ServiceHandle::start()` 的注释写着"先释放锁再 spawn 子进程"，但 `self.inner.lock()` 的守卫从函数开头一直活到 spawn 之后 —— **持锁 spawn**。dsh 子进程 stdout 一有输出，捕获回调就阻塞在同一把锁上，与主线程互等。症状：进程在、托盘图标在，但**没有任何窗口**、菜单点不动、CPU 接近 0、日志固定停在「托盘图标已创建。」。已改为 `drop(lock)` 后再 spawn，并加 `debug_assert!(try_lock().is_ok())` 让这类错误在开发期就暴露。
- **带 token 的地址从未生效 → 界面是 HTTP 401 认证失败页**：`Ready` 事件无条件把**无 token** 地址写进 `pending_url`，使「等 token 再导航」的逻辑**永远不可达**；就绪 worker 又只在锁内读一次 `auth_url`，与 stdout 捕获线程存在竞态。现改为 `Ready { url: Option<String> }`（`None` = 就绪但暂无 token）、**只有带 token 才写入 `pending_url`**、捕获到 token 后**补发** `Ready`。**运行期实测**：本次「就绪」比「token 到达」早约 **914 ms** —— 正是旧实现必然用无 token 地址导航的窗口。

#### 🔗 定稿轮：破坏性变更 —— 服务与启动器解耦

> ⚠️ **行为语义变更**：dsh 服务默认**独立于启动器**。重写初版中「退出启动器」会切断正在进行的会话（只有托盘「退出 → 否」能保住）；现在退出 / 崩溃 / 被安装包升级覆盖 / 注销重启**都不会**中断会话。

- 默认 `service_lifecycle = "independent"`：dsh **不挂在启动器的 Job Object 上**（`CREATE_BREAKAWAY_FROM_JOB`），退出 / 崩溃 / 升级都不中断会话。
- 归属改由 `%LOCALAPPDATA%\DSHLauncher\service.json` 簿记（PID + 端口 + **进程创建时间**）+ 启动时对账保证；**创建时间比对**用于防止 PID 复用被误当成自己的服务。
- 需要「启动器一死就回收一切」的旧语义：设置页取消勾选「服务独立于启动器」（即 `tied`）；内核级零残留的实现与测试均保留。
- 配置新增 `schema_version` / `service_lifecycle` / `stop_stale_orphan`（缺失时取默认值，旧配置文件可直接沿用）。

#### 🔧 定稿轮：其他修复

- **可能误杀无关程序**：接管/停止前不再只看"谁占着端口"，而是做 dsh 身份校验（`node.exe` 镜像名 + 排除系统目录 + 路径 / 映像 / 祖先链三重关联）；**无法确认就只读不接管、不杀**（v4 有该校验，重写初版丢失）。
- **stderr 管道从不排空**：dsh 写满管道缓冲区后会永久阻塞（服务假死、被误判失联重启）。现持续读取并落 WARN 日志（单行截断 2000 字符）。
- **`tray_on_close` 完全没接线**：界面承诺「关闭窗口时最小化到托盘」，实现却只是销毁窗口。现按偏好**隐藏**（保留 WebView 与页面状态）或真正关闭。
- **`F5` / `Ctrl+R` / `Esc` 只是文档承诺**：`ui/guide.html` 里写着的快捷键此前无任何实现。现已实现（`Esc` 按 `tray_on_close` 偏好隐藏）。
- **标题栏不跟随主题**：`sample_and_apply_theme` 恒返回 `None`。现新增 `begin_theme_sample()`（**异步回调**，绝不在 UI 线程 `recv_timeout`），窗口创建时用系统深浅色给正确初值。
- **日志被主题消息刷爆**：采样结果"只读不清"导致同一结果逐帧重复打印（4557 行 / 仅 35 种内容）；改成"取出并清空"后又因**按帧数限流**（事件循环实测数百 fps）恶化到 **400 行/秒**。现改为**按时间限流（3 秒）+ 只在颜色变化时打日志**。
- **归档清理 O(n²) 且把"剔除脏条目"计为"已清理"**：改用 `HashSet` + 分类计数；`workspace.json` 改写前检查 key 唯一性、改写后复核 JSON 合法性（防区间错位写坏文件）。
- **锁文件收集会跟随 junction / 符号链接**（可越界删除 `~/.dsh` 之外的文件）：现拒绝 reparse point。
- **日志实际滚动出 4 个文件**（文档称 3 份）：现严格保留 `max_files` 个并删除溢出归档。
- **「清理归档会话」会静默停掉正在服务的 dsh**：该操作按设计必须先停服务，但后果只写在界面小字里，用户点下去会突然发现会话断了（实测误判为启动器缺陷）；且清理后**不把服务拉回来**，用户会停在"服务已停、界面打不开"的状态，只能手动点「启动服务」。现新增**二次确认**（明示"停服务 / 会话中断 / 数据不可恢复"，默认焦点在「否」），并在清理结束后**自动按需重启服务**。
- **清理前不检查是否有会话在跑**：清理必然先停 dsh，若此刻有会话正在对话/跑任务就会被**打断**。现在点击清理时先**探测活跃会话**，命中则把确认文案升级为警告并**点名正在跑的会话**（默认焦点仍在「否」）。探测用两个信号：**会话运行器进程存活**（`dsh-subprocess-local` / `runner.js`，硬信号）与**会话文件在最近 5 分钟内被写过**（软信号）。实测：在本机准确检出 1 个运行器进程，并点出正是当前这条对话（`session-d2084fd4-…`）。
  - **诚实标注**：这两个信号都是**间接**的（用户态无法精确询问"dsh 是否持有活跃会话"），因此实现选择「明确告知 + 提高确认门槛」而非硬性阻止 —— 硬阻止会让"清理"在你正与它对话时永远不可用。探测无法判定时按"有活动"处理。
- **`build.ps1` 发布必须退出启动器**：直接覆盖正在运行的 exe 必失败，于是发布与"不打断用户"不可兼得。现内置**热替换**（先改名让位再放入），并在发布后清理历史产物（`*.bak-*` / `*.old-*` / `*.pubtmp`，只保留最近一个 `.bak` 作回退点）。
- **勾了「服务独立于启动器」却每次退出都弹确认框**：该选项因此**形同虚设**（用户会以为没生效）。现在退出行为**随该设置变化**：
  - **勾选（默认 `independent`）**：退出**不弹窗**，服务继续在后台运行（日志留一行说明；想停服务请用托盘「停止服务」或设置页）。
  - **取消勾选（`tied`）**：退出**每次询问** —— 因为此时退出会连带停掉服务，那次确认是必要的（默认焦点仍在「否」）。
  - 设计理由：**弹窗的价值在于提醒一次「意外的或不可逆的后果」**。独立模式下退出对正在进行的会话无害，没什么可提醒的；每次都问只会让用户烦，还会让人误以为该设置没生效。
- **取消勾选「服务独立于启动器」会立即打断正在进行的会话**（本轮实测踩到）：当时保存该设置会先 `stop()` 再重启服务，而 Job 归属是**进程创建时**决定的、无法事后改变，于是重启还失败了（端口上残留着正在退出的旧进程）→ 服务进入 `Error` → 连"退出时要不要停服务"都没有可询问的对象（表现为**取消勾选后退出没有弹窗**）。
  - 现在语义：**切换策略只记录、不重启服务**（`independent` / `tied` 的差别只作用于**以后新拉起**的进程），并在状态栏与日志说明"下次服务重启后生效"。
  - 顺带修好一处**误导性诊断**：端口占用者身份判定新增**三态** —— `IsDsh` / `NotDsh` / `Unknown`。旧实现把"读不到进程信息"也报成「被非 dsh 程序占用」，导致日志自相矛盾（前一秒还在正常接管那个 PID，后一秒说它不是 dsh）。现在无法判定时如实说"无法读取该进程信息（可能正在退出或权限受限）"。
- 其他：`eprintln!` 在 GUI 子系统下写失败会 panic（`panic=abort` ⇒ 整进程崩溃）→ 全部改为忽略错误的 `writeln!`；`is_process_alive` 句柄泄漏；`kill_process_tree` 嵌套持有 N+1 个快照句柄；`stop()` 持锁执行进程终止会阻塞 UI；自动重启次数永不重置（现稳定运行 5 分钟后恢复额度）；配置保存失败仍已改内存（现副本改动 + 落盘成功才提交）；ini 迁移后不删旧文件（删除 toml 会"复活"旧配置）。

#### 🏷 发布工程轮：LTS 标签 + `--version` 入口 + 运行期加固

> 本轮把版本统一为 **5.0.0 LTS**（Cargo 仍是合法 semver `5.0.0`，"LTS" 只作
> **发布标签**出现在 `--version` 输出、文档、安装向导与制品名中），并补齐发布/运行期
> 的最后一批缺陷。

- **新增 `--version` / `-V` 与 `--help` / `-h`**：安装包与 CI 终于能自动核对版本
  （`DSHLauncher.exe --version` → `DSHLauncher 5.0.0 LTS (release)`）。此前这两个开关
  被当成"普通启动"：抢单实例互斥体、开窗、甚至唤起已有实例，于是"装完核对版本"无法自动化。
  二者都在单实例检查**之前**处理，绝不触碰互斥体/窗口。
- **启动服务整体移出 UI 线程**：`ServiceHandle::start()` 里的对账（进程存活 + 创建时间 +
  身份判定）、端口探测（300 ms connect）、遗留锁清理（遍历 `~/.dsh`）与 spawn 都是阻塞调用，
  此前点托盘「启动服务」会让界面卡住；现在与「停止服务」同款走 worker 线程 + 后台任务通道。
- **端口可达性探测集中限流**：`is_port_listening` 是阻塞 connect，而「打开界面」的每帧泵
  与唤起待办都在 100 ms 事件循环里调用它 —— 服务未起来时 UI 线程约 75% 时间卡在 connect 上。
  现在统一走 `port_reachable()`（`READY_PROBE_INTERVAL` = 900 ms 内的结果复用），
  且「服务尚未就绪」**只记一次日志**（此前每 100 ms 一行，120 秒可刷出上千行）。
- **端口变更预检改为非阻塞**：改用 `is_port_free_to_bind`（一次 bind，无 300 ms 超时），
  权威判定仍在 `start()` 分支 3（含身份校验与可操作错误）。
- **修复并发双重启动竞态**：分支 1 的存活检查与真正 spawn 之间释放过锁，失联自愈线程或
  用户连点会各自拉起一个 dsh（抢同一端口）。现在在**持有 `ProcessManager` 锁**的临界区内
  spawn 前复查存活，已运行则记为 `AlreadyRunning` 而不重复拉起。
- **修复「启动中收到停止请求」的状态覆盖**：spawn 期间用户点「停止服务」时，旧实现会留下
  一个无人监视、UI 停在 `Starting` 的 dsh（直到 120 秒就绪超时才被回收）。现在**停止优先**：
  立即回收刚启动的进程并回到 `Stopped`。
- **进程存活判定改用 `WaitForSingleObject(0)`**：退出码 `259` 与 `STILL_ACTIVE` 哨兵值同值，
  旧实现会把 `exit(259)` 的**已退出**进程永久判为存活（连带 `service.json` 被判有效、
  残留回收被跳过、孤儿 `.lock` 永不被清理）。新增回归用例。
- **进程树终止改为迭代 + 去重**：不再递归（`dsh-stop` 线程只有 256 KB 栈；深树或环状
  父子关系会栈溢出），并限制单次回收的节点预算。
- **TCP 监听表读写对齐安全**：`GetExtendedTcpTable` 的缓冲改用 `Vec<u32>` 承载
  （`MIB_TCPTABLE_OWNER_PID` 要求 4 字节对齐，`Vec<u8>` 只保证 1），并修正表项起点偏移。
- **外部命令探测限时**：`Get-CimInstance` 探测（活跃会话）改用有超时的执行器 —
  超时即杀进程树并如实返回 0。此前 `Command::output()` 无超时，PowerShell/WMI 一卡就
  把清理流程与界面状态永久挂住。
- **诊断信息落到日志文件**：stdout 读取线程的两条告警（浏览器移交未关掉 / 上游输出格式变了）
  此前只写 stderr —— release 是 GUI 子系统、stderr 句柄可能无效，等于把最关键的线索丢掉。
- **`~` 定位失败不再退化成相对路径**：`~/.dsh` 相关的"扫描 + 删除"函数改为 `Option` 语义，
  取不到主目录时**什么都不做**（旧实现会退化成 `.dsh/...` 即当前工作目录，可能删到 CWD 下
  别的程序的 `.lock`）。
- **删除重复实现与未使用依赖**：`sessions_dir()` / `sessions_root()` 两份逐字重复合为一处；
  `dsh-ui` 移除从未使用的 `serde`；`classify_dsh_identity` 的 node 解析加 `OnceLock` 缓存
  （身份判定会对每个 PID 重扫 PATH）。新增 `rust-version = "1.82"`（MSRV，与所用 API 对齐）。
- **修复 `service.json` 存错目录（卸载残留 + 验证脚本假通过）**：簿记此前写在
  `%APPDATA%\DSHLauncher`（用户配置目录，**卸载默认保留**），而本模块文档、README、
  维护手册、发布说明、卸载器与 `tools/verify-service-lifecycle.ps1` **全部**按
  `%LOCALAPPDATA%\DSHLauncher\service.json` 约定。实测后果有两条：① 卸载后
  `%APPDATA%` 下仍残留这份运行期状态；② 验证脚本永远读不到记录，只能一直打印
  "尚无 service.json"，接管断言失去意义。现改为写在 `%LOCALAPPDATA%`（与日志/profile 同级，
  卸载器整目录清理），并在首次读取时删除旧位置的残留文件。
- **彻底删除死代码 API**：`dsh-core::dsh` 的 `version` / `dist_tags` / `upgrade` /
  `check_health`（连同一整条 npm 调用链：`is_safe_version` 白名单、`run_capture`
  超时+进程树回收、`resolve_npm`，以及 `DshError` 的 5 个专用变体）。它们从 v5 重写起就
  **没有任何调用方**，此前以"公开库 API、不进调用图所以无害"为由保留——那对体积成立，
  对维护成本不成立：这条链路会持续跟着上游 npm 输出格式与本地环境漂移，却没有任何用户入口。
  用户能力没有丢：升级与修复原生模块的手动步骤完整保留在维护手册 §1.3–1.5；
  原本由 `is_safe_version` 承担的安全属性改由结构保证并由门禁强制
  （**生产代码不含任何 `cmd.exe` / shell 拼串调用**）。
- **修复服务启动路径的自死锁（P0，实测复现）**：`ServiceHandle::start()` 在 spawn 之前
  **重复 `lock()` 了同一个 `std::sync::Mutex`**（不可重入）——同一线程第二次加锁即永久阻塞。
  因为死锁发生在持有 `ServiceInner` 锁的状态下，**后续任何服务操作（状态查询、设置页保存、
  停止服务）都会一起卡死**；表现为「启动服务后界面正常但服务永远起不来」。
  正常使用中长期没暴露：端口上总有上次留下的 dsh，`start()` 走"接管"分支就返回了；
  **只有端口上没有服务时**（全新环境 / 手动停服后重启 / 换端口）才会走到 spawn 分支。
  已改为从句柄直接取 `ProcessManager`（不再二次加锁），并补上**启动阶段计时埋点**
  （`[start] 服务对账 / 端口探测 / 解析 dsh 路径 / 清理遗留锁 / spawn 子进程`）——
  正是这组埋点在一次非默认端口启动中把故障定位到"清理遗留锁之后 5 分钟无日志"。
  另加 `start()` 加锁次数断言（必须恰好 2 次）与可选的真实 spawn 回归测试
  （`DSH_TEST_SPAWN=1` 开启，默认跳过以免在开发机上启动服务）。
- **新增「内嵌界面需要认证」自愈**（用户实测故障：界面停在
  `dsh web authentication required; reopen the URL printed by dsh web.`）：
  `dsh web` 的认证只有两条路——启动时打印的**一次性 launch token**（只存在于 dsh 进程内存），
  或**已种下的签名 cookie**（30 天，存在于 WebView2 自己的 cookie 罐）。
  接管"别处启动"的服务时启动器读不到 token；cookie 又不存在（全新安装 / profile 被清理 /
  超期 / 端口变化）时界面必然 401，而旧实现对此**毫无觉察**（照样把 401 页当界面打开）。
  现在窗口打开/每次导航后会在**页面上下文**里自检（裸 socket 探测区分不了"已有 cookie"），
  命中则：无活跃会话 → 自动重启服务（启动器自己拉起 ⇒ 能读到带 token 的地址 ⇒ 种下 cookie）；
  有活跃会话 → 明确询问（默认「是」，说清会中断进行中的任务、历史会话不丢）。
- **一致性门禁扩到 165 项**：新增发布工程硬约束（`--version/--help` 存在、LTS 与 semver 分离、
  启动不在 UI 线程、端口探测单点限流、外部命令限时、存活判定不依赖退出码哨兵、
  进程树不递归、`start()` 加锁次数、认证自检与自愈接线、死代码 API 不得复活、
  生产代码无 shell 拼串）以及**离线版 `cargo machete` 等价检查**（每个直接依赖必须被本 crate 源码引用）。

#### 🧹 自清洁轮：构建缓存回收（仓库根 3.5 GB → 11 MB）

- **问题**：仓库根目录实测 **3,556 MB**，其中 `target\` 占 **3,551 MB** —— 而它是**构建缓存、
  不是交付物**（`.gitignore` 早已排除）。构成：`target\debug` 2,629 MB（deps 1,292 + **incremental 1,030**
  + build 125 + examples 87 + pdb 55）、`target\release` 922 MB（deps 824 + build 88）。
  `cargo` **从不回收**旧产物（换 feature / profile / 工具链都是新增而非替换），因此随每轮
  构建与验证单调膨胀 —— 与「自清洁」目标直接冲突。
- **新增 `tools/clean.ps1`**（自清洁工具，**默认只报告、不删任何东西**）：
  `-Cache` 清 debug profile + 依赖缓存并**保留** `target\release\dsh-app.exe`（FACTS 与
  `verify-version.ps1` 的校验输入）；`-All` 等价 `cargo clean`；`-WhatIf` 先看将删什么。
  脚本自带"保留项被误删即失败"的复核。
- **实测结果**：`-Cache` 回收 **3,544.9 MB**，`target\` 3,550.8 MB → **5.8 MB**，
  仓库根 **3,556 MB → 11.3 MB**；且从**清理后的树冷构建**完全离线可用：
  `cargo build --release --locked` **89.7s / exit 0**、`cargo test --workspace --all-features`
  **67.1s / 111 项全通过**；清理后 `verify-version` 28/0/0、`gen-facts -Check`、`check-consistency` 全部照常通过。
- **防止复发**：`tools/finish-release.ps1` 关闭增量编译（`CARGO_INCREMENTAL=0`，把一轮验证的
  增量产物压到 0）并把 `target\` 体积写进收尾报告（新增「6b. 构建缓存体积」一行）；
  一致性门禁新增 5 条断言（清理工具存在、默认只报告、保留 release 产物、`/target/` 与 `*.pdb` 已被忽略）；
  README（中英）与 `docs/OPS-RUNBOOK.md` §6 写明回收命令与"-All 之后不要直接发版"的坑。

#### 🛠 卸载器改为原生 exe（取代脚本卸载器）

- **问题**：卸载器是 `uninstall.cmd` → `powershell.exe -ExecutionPolicy Bypass -File uninstall.ps1`。
  命令行 `-ExecutionPolicy Bypass` **只覆盖本机设置，覆盖不了组策略**（`MachinePolicy`/`UserPolicy`
  设为 AllSigned/Restricted 时脚本仍被拒），AppLocker/WDAC 也能直接封锁脚本执行 ⇒
  加固过的机器上用户**根本卸载不掉**；脚本版延迟删目录还要**再依赖一次 PowerShell**
  （模板里甚至要往 `%TEMP%` 写临时脚本）；`cmd → powershell -Bypass → Remove-Item -Recurse`
  也正是 AV/EDR 最敏感的行为模式；且脚本无法做 Authenticode 签名获得同等信任。
- **方案**：新增 `crates/dsh-uninstall`（原生 `dsh-uninstall.exe`，约 300 KB，仅依赖 `dsh-core` + `windows`，
  无第三方依赖、无脚本引擎），安装包内嵌它并把 `UninstallString`/`QuietUninstallString` 指向它；
  仓库里的 `uninstall.cmd`/`uninstall.ps1` **删除**（单一实现，不留第二份逻辑）。
  开关向后兼容并额外接受 Windows 风格：`--purge/--silent/--dry-run` + `/purge /quiet /whatif`；
  退出码 `0` 成功 / `1` 部分失败 / `2` 被防护规则拒绝。自删除改为"复制自身到 `%TEMP%` 后重入"。
- **护栏逐条对齐（并补上单测，共 16 个）**：安装标记 `.dsllauncher-install` 必需**且内容必须属于本应用**；
  受保护路径（盘符根/系统目录/用户目录）拒绝递归删除；目录里有第三方文件则只删自己的文件并保留目录；
  只结束**从本目录启动**的启动器实例（按映像路径比较，读不到路径就不杀）；`--dry-run` 零改动。
- **顺带修掉升级路径的漏洞**：安装器新增 `ObsoletePayloads`，升级时清除上一版遗留的
  `uninstall.cmd`/`uninstall.ps1`（实测：只覆盖本版载荷会留下**两套卸载器**，而旧脚本的清理清单里
  没有 `dsh-uninstall.exe`，用户点到旧脚本就会残留；新卸载器也会把旧脚本误判成"第三方文件"而保留目录）。
- **结构性防漂移**：一致性门禁新增 8 条断言（内嵌/注册表指向/脚本不得复活/护栏齐全/只杀本目录实例/
  dry-run 无副作用/两阶段/载荷清单两侧一致）+ `verify-version.ps1` 新增 5 条（卸载器
  FileVersion/ProductVersion/OriginalFilename/ProductName/图标 + 安装包内嵌与注册表指向），
  版本链路校验从 28 项增至 **33 项**。
- **实测（真实安装 → dry-run → 卸载 → 零残留 → 重装）**：dry-run 逐项报告且零改动；
  真实卸载后**安装目录、注册表项、桌面快捷方式、`%LOCALAPPDATA%\DSHLauncher` 全部消失，
  `%APPDATA%` 用户配置保留**。过程中还修掉两个只有真机跑才会暴露的缺陷：
  ① 注册表存在性用 `RegGetValueW` 查**默认值**（键没有默认值 ⇒ 把"存在"误报成"不存在"），
  改用 `RegOpenKeyExW` 查**键**；② 阶段二把"目标目录"误判成"本程序所在目录"（副本住在 `%TEMP%`），
  导致**安装目录删不掉** —— 真正的护栏应是"目标 ≠ 本程序所在目录"。
- **平台限制（如实记录）**：Windows 不允许运行中的镜像删除自己（实测对自身取 `DELETE` 权限被拒），
  因此 `%TEMP%` 里会留一个约 300 KB 的副本直到**下次重启**由系统清理；每次卸载会顺手清扫历史副本。
  已写入 README 与 `--help`。

#### ✅ 验证

- `cargo test`：**108 个单元测试**全绿
- `selftest.ps1`：**A1**（强杀回收）/ **A2**（优雅退出保留服务）/ **B**（端到端）/ **C**（设置页 IPC + 开窗）/ **D**（单文件分发）/ **E**（失联检测与自动重新接管）**全通过**
- `tools/check-consistency.ps1`：**31 项**硬约束通过
- 零编译警告

定稿轮验证与工具（新增）：

- 新增 `tools/verify-service-lifecycle.ps1`（含 `-Force` 判决性实验）、`tools/verify-token-navigation.ps1`、`crates/dsh-core/examples/token_capture_probe.rs`（**运行期**验证 token 捕获链路，含竞态复现）。
- 新增 `crates/dsh-core/examples/active_session_probe.rs`：**运行期**验证活跃会话探测，并用**独立路径交叉验证**（两条不同的 PowerShell 查询），防止"靠外部命令取数"的实现静默失效。
- 新增 `tools/gen-facts.ps1` + `docs/FACTS.json`：文档数字的**唯一来源**，`-Check` 检测漂移（此前同一指标在 5 份文档里有 3–4 个互相矛盾的值）。
- `tools/check-consistency.ps1` 新增**文档数字断言**：直接校验 README / CHANGELOG（中英）/ 路线图 §15 引用的单元测试数、一致性项数、exe 体积量级与 `FACTS.json` 一致 —— `gen-facts -Check` 只能发现"源码变了"，发现不了"文档没跟着改"。**首次运行即抓出 3 处真实漂移**（README 体积量级四舍五入不一致、英文 CHANGELOG 缺实测表、数字停留在旧值）。
- 修正 `tools/verify-token-navigation.ps1` 的一处**误报**：它只识别旧措辞「已接管端口」，把"经 `service.json` 对账接管"判成失败。现改为按"是否**曾经**捕获过 token"判定，与措辞解耦 —— 捕获链路从未生效才算真异常。
- `tools/check-consistency.ps1` 新增的都是**行为性**硬约束（接线、默认值、必需 API、护栏）—— 这类缺陷的特征正是"单元测试全绿但功能没接上"。
- 单元测试与一致性项的具体数量以 [`FACTS.json`](FACTS.json) 为准（由 `tools/gen-facts.ps1` 实测生成）；`cargo clippy --all-targets --all-features` 0 警告。

#### 📊 实测指标

**重写基线（v4 → v5）**

| 指标 | v4 | v5 | 变化 |
|---|---|---|---|
| 工作集 | 65,520 K | **23.4 MB** | -64% |
| 产物 | 199 KB + 902 KB 旁挂 | **单文件 934.5 KB** | — |
| 代码量 | 7,720 行 | **4,089 行** | -47% |
| 单元测试 | 0 | **48** | — |

**定稿轮与发布工程轮（同口径实测）**

| 指标 | 重构初版 | 当前（5.0.0 LTS） |
|---|---|---|
| 单元测试 | 65 | **149** |
| 一致性校验项 | 66 | **178** |
| 版本链路项 | 23 | **28** |
| 启动器工作集（窗口关闭态） | 23.9 MB（另一口径） | **13.2 MB**（同口径实测） |
| 冷启动「就绪 vs token」 | 未测（缺陷未暴露） | **就绪早 914 ms**，由 `Ready{url:None}` + 补发覆盖 |

> 数字的权威来源是 [`FACTS.json`](FACTS.json)（由 `tools/gen-facts.ps1` 从源码与产物实测生成）。

<a name="v500-english"></a>
### English Release Notes

#### 🎯 Rewrite

- **Full Rust rewrite**: `dsh-core` (pure logic, zero GUI deps) / `dsh-ui` (windows, tray, theme) / `dsh-app` (single entry)
- **Stack**: `wry` (WebView2 COM) + `tao` + `tray-icon` / `muda` + `windows` + `std::thread` + `mpsc`
- **Deliberately excluded**: `tauri` / `tokio` / `regex` / `chrono` / `semver` / `tracing-*` (enforced by the consistency checker)
- **Build chain**: `csc.exe` → `cargo build --offline` with a committed `Cargo.lock`

#### 🗑 Breaking change: LAN sharing fully removed

- ~6,400 lines and 902 KB deleted (`lan-gateway.mjs`, `LanAccess.cs`, the C# launcher, `whale-256.png`, three side-by-side DLLs)
- Also removed: PIN/token/session-secret credentials, firewall rules, the UAC elevation path, Ollama exposure, QR code and mobile UI
- **Inbound attack surface is zero**; .NET Framework and the side-by-side DLLs are no longer required

#### 🆕 New features

- **Loss detection & auto-reconnect**: probed every 1.5 s, declared lost in ~12 s; own service auto-restarted (≤3), adopted instances watched and **re-adopted with UI reconnect**
- **Orphan lock recovery**: reads the **owner PID** from `<file>.lock` and removes it only when that process is really gone
- **Settings-page IPC**, **CLI flags** (`--selftest`, `--settings`/`-s`, `--guide`/`-g`, `--ipc-probe`), **native guide/about dialogs**

Added in the finalisation round:

- **`--quit`**: asks the running instance to exit gracefully (use it in release/automation scripts instead of force-killing, which cuts sessions).
- **`--build-info`**: prints build/fix markers and writes `%APPDATA%\DSHLauncher\build-info.txt` — an **executable** artifact check. Grepping the binary is wrong: `debug_assert!` and comments never reach a release build.
- **`--probe-identity <PID>`**: a diagnostic that prints the launcher's **full identity decision** for a PID (image name / image path / system-directory check / which rule matched / why it could not decide) and writes `identity-probe.txt`. Rationale: identity is the "no collateral kill" gate — a wrong verdict means **adoption fails and the service cannot be restarted**, yet the log shows only a one-line conclusion with no clue where it stopped.
- **Crash forensics**: under `panic = "abort"` a `SetUnhandledExceptionFilter` writes one `[FATAL] unhandled exception code=0x… address=0x… pid=… tid=…` line into the log (zero allocation).
- Settings gained **"service is independent of the launcher"** (checked by default).

#### 🔧 Fixes (real defects found during implementation)

- **Launcher self-deadlock on startup** (non-reentrant `Mutex` re-lock)
- **No window when adopting an existing service** (missing `Ready` event)
- **WebView2 created a data folder next to the exe** (broke single-file distribution; failed outright in read-only installs)
- **Settings buttons were dead** (no IPC)
- **Stray console window** (release now declares `windows_subsystem = "windows"`)
- **Missing title-bar logo** (window icon now parsed from the embedded `app.ico`)
- **dsh orphan lock** breaking the next start (`atomic-write: timed out waiting for the writer lock`)
- **Test false-positives** under the GUI subsystem; `--selftest` / `--ipc-probe` now exit **2** on mutex conflict

#### 🔴 Finalisation round: critical fixes (two P0s)

- **Launcher stayed a tray-only background process with no window (deterministic deadlock)**: `ServiceHandle::start()` claimed to release the lock before spawning, but the `MutexGuard` lived until after `start_dsh` — it **spawned while holding the lock**. As soon as the child wrote to stdout the capture callback blocked on the same lock, deadlocking against the main thread. Symptom: process and tray icon alive, **no window**, unresponsive menus, ~0 CPU, log frozen after "tray icon created". Fixed by dropping the guard before spawning, plus `debug_assert!(try_lock().is_ok())` so the mistake surfaces during development.
- **The token URL never took effect → the embedded window showed an HTTP 401 page**: `Ready` unconditionally stored a **token-less** URL in `pending_url`, making the "wait for the token" logic **unreachable**; the readiness worker also read `auth_url` once while holding the lock, racing the stdout capture thread. Now `Ready { url: Option<String> }` (`None` = ready but no token yet), `pending_url` is only set for token-bearing URLs, and `Ready` is re-emitted once the token arrives. **Measured at runtime**: readiness preceded the token by ~**914 ms** — exactly the window where the old code navigated to a 401 page.

#### 🔗 Finalisation round — breaking change: the service is decoupled from the launcher

> ⚠️ **Behavioural change**: the dsh service is now **independent of the launcher** by default. In the initial rewrite, "quit the launcher" cut the running session (only the tray "Exit → No" path preserved it); now quitting / crashing / being replaced by an installer / signing out **never** interrupts a session.

- Default `service_lifecycle = "independent"`: dsh is **not** in the launcher's Job Object (`CREATE_BREAKAWAY_FROM_JOB`), so quitting / crashing / being upgraded never interrupts a session.
- Ownership is recorded in `%LOCALAPPDATA%\DSHLauncher\service.json` (PID + port + **process creation time**) and reconciled at startup; the creation-time comparison prevents a recycled PID from being mistaken for the service.
- Prefer the old "kill everything when the launcher dies" behaviour? Uncheck *service is independent of the launcher* in Settings (`tied`); the kernel-level zero-residue implementation and its tests remain.
- New config keys: `schema_version` / `service_lifecycle` / `stop_stale_orphan` (old config files keep working through defaults).

#### 🔧 Finalisation round: other fixes

- **Could kill unrelated programs**: adoption/stop no longer trusts "whoever owns the port" — it verifies dsh identity (`node.exe` image + system-directory exclusion + path / image / ancestor-chain checks) and **refuses to adopt or kill when unconfirmed** (v4 had this check; the initial rewrite lost it).
- **stderr pipe never drained**: dsh would block forever once the pipe buffer filled (service appeared hung and got restarted). Now drained continuously into the WARN log (2000-char line cap).
- **`tray_on_close` had no consumer**: the UI promised "closing the window minimizes to tray" while the code destroyed the window. Now it **hides** (preserving the WebView and page state) or really closes, per the preference.
- **`F5` / `Ctrl+R` / `Esc` were documentation-only**: the shortcuts advertised in `ui/guide.html` were unimplemented. Implemented (`Esc` hides per the `tray_on_close` preference).
- **Title bar did not follow the theme**: `sample_and_apply_theme` always returned `None`. Added `begin_theme_sample()` (**async callback**, never `recv_timeout` on the UI thread) plus a correct initial value from the system light/dark preference.
- **Log flooded by theme messages**: the sample slot was read without clearing, re-printing the same result every frame (4557 lines / 35 distinct values); after switching to take-and-clear, **per-frame-count rate limiting** (the event loop measured hundreds of fps) made it worse at **400 lines/s**. Now rate-limited **by time (3 s)** and logged **only when the colour changes**.
- **Archived-session cleanup was O(n²)** and counted "pruned malformed entries" as "cleaned": now `HashSet` + separate counters; `workspace.json` rewrites check key uniqueness first and re-validate the JSON afterwards.
- **Lock-file scan followed junctions/symlinks** (could delete outside `~/.dsh`): reparse points are now refused.
- **Log rotation produced 4 files** (docs said 3): now keeps exactly `max_files` and deletes overflow archives.
- **"Clean archived sessions" silently stopped the serving dsh**: the operation must stop the service by design, but the consequence was only in small print — users suddenly saw their session drop (measured; misattributed to a launcher defect), and the service was **not restored** afterwards, leaving "service stopped, UI unreachable" until they manually pressed *Start service*. Now there is a **second confirmation** (spelling out "service stops / session is interrupted / data is unrecoverable", default focus on *No*) and the service is **automatically restarted** when needed after cleanup.
- **Cleanup did not check for running sessions**: it always stops dsh first, so a conversation or task in progress would be **interrupted**. Clicking cleanup now **probes for active sessions** first and, when found, escalates the confirmation to a warning that **names the running session** (default focus stays on *No*). Two signals are used: a live **session runner process** (`dsh-subprocess-local` / `runner.js` — the hard signal) and **session files written within the last 5 minutes** (the soft signal). Measured on this machine: it correctly detected one runner and identified it as the very conversation being reviewed (`session-d2084fd4-…`).
  - **Stated honestly**: both signals are **indirect** (user mode cannot ask dsh whether it holds a live session), so the implementation chooses "inform clearly + raise the confirmation bar" rather than a hard block — a hard block would make cleanup permanently unusable while you are talking to it. When the probe cannot decide, it is treated as "activity present".
- **`build.ps1` required quitting the launcher**: overwriting a running exe always fails, so publishing and not-interrupting-the-user were mutually exclusive. It now performs the **hot swap** (rename aside, then copy) and cleans up historical artifacts afterwards (`*.bak-*` / `*.old-*` / `*.pubtmp`, keeping only the newest `.bak` as a rollback point).
- **Unchecking "service is independent of the launcher" interrupted the running session** (hit during this round's testing): saving that setting would `stop()` and restart the service, but job membership is fixed **at process creation** and cannot be changed afterwards — so the restart failed too (a dying process still held the port) → the service ended in `Error` → there was nothing left to ask about on exit (which is why **no prompt appeared after unchecking**).
  - New semantics: **switching the policy is recorded only and never restarts the service**; `independent` vs `tied` affects **future spawns only**, with the status bar and log explaining that it takes effect on the next service restart.
  - Also fixed a **misleading diagnostic**: the port-owner identity check is now tri-state (`IsDsh` / `NotDsh` / `Unknown`). The old code reported "held by a non-dsh program" even when it simply could not read the process, producing self-contradictory logs (the same PID was adopted successfully one second earlier). It now says "cannot read that process (it may be exiting, or access is restricted)".
- Also: `eprintln!` could panic under the GUI subsystem (`panic=abort` ⇒ the whole process dies) → replaced with fallible `writeln!`; `is_process_alive` handle leak; `kill_process_tree` held N+1 snapshot handles; `stop()` blocked the UI while holding the lock; the auto-restart quota never refilled (now refills after 5 minutes of healthy uptime); a failed config save still mutated memory (now copy-then-commit); the legacy `.ini` was never deleted (deleting the TOML silently resurrected old settings).
- **Ticking "service is independent of the launcher" still prompted on every exit**: the option was therefore **effectively inert** (users would think it had not taken effect). Exit behaviour now follows that setting:
  - **Checked (default, `independent`)**: exit **does not prompt**; the service keeps running in the background (one log line explains; stop it from the tray or Settings).
  - **Unchecked (`tied`)**: exit **prompts every time** — because exiting really does stop the service, so that confirmation is warranted (default focus stays on *No*).
  - Rationale: **a prompt is only worth showing for an unexpected or irreversible consequence**. Under the independent mode, quitting is harmless to the running session, so there is nothing to warn about; asking every time is noise and makes the setting look broken.

#### 🏷 Release-engineering round: LTS label, `--version` entry point, runtime hardening

> This round unifies the version as **5.0.0 LTS** — Cargo stays on the legal semver `5.0.0`,
> while "LTS" is a **release label** surfaced in `--version`, the docs, the installer wizard and
> artifact names — and closes the last batch of release/runtime defects.

- **New `--version` / `-V` and `--help` / `-h`**: installers and CI can finally verify the version
  automatically (`DSHLauncher.exe --version` → `DSHLauncher 5.0.0 LTS (release)`). These switches
  used to be treated as a normal launch (grabbing the single-instance mutex, opening a window, even
  activating an existing instance), so "check the version after installing" could not be automated.
  Both are handled **before** the single-instance check and never touch the mutex or any window.
- **Service start moved off the UI thread**: `ServiceHandle::start()` chains blocking work
  (reconciliation with process liveness / start-time / identity probes, a 300 ms port connect,
  a `~/.dsh` lock sweep, and the spawn itself). Clicking "start service" used to freeze the UI;
  it now runs on a worker thread like "stop service", reporting back through the background-task channel.
- **Central, throttled port probing**: `is_port_listening` is a blocking connect, yet the per-frame
  "open the UI" pump and the activation backlog both called it inside the 100 ms event loop, leaving
  the UI thread blocked ~75% of the time while the service was starting. Everything now goes through
  `port_reachable()` (results reused for `READY_PROBE_INTERVAL` = 900 ms), and "service not ready yet"
  is logged **once** instead of every 100 ms (thousands of lines over a 120 s cold start).
- **Non-blocking port-change pre-check**: switched to `is_port_free_to_bind` (a single bind call, no
  300 ms connect timeout); the authoritative decision remains `start()`'s branch 3.
- **Fixed a concurrent double-start race**: the liveness check in branch 1 and the actual spawn were
  separated by a lock release, so the health-monitor thread or a double click could each spawn a dsh
  (fighting over the same port). The liveness check is now repeated inside the `ProcessManager`
  critical section and reports `AlreadyRunning` instead of spawning a second process.
- **Fixed "stop requested while starting" state clobbering**: a stop during spawn used to leave an
  unmonitored dsh with the UI stuck in `Starting` until the 120 s ready timeout. **Stop now wins**:
  the freshly spawned process is reclaimed immediately and the state returns to `Stopped`.
- **Process liveness via `WaitForSingleObject(0)`**: exit code `259` equals the `STILL_ACTIVE` sentinel,
  so the old check reported `exit(259)` processes as alive forever (which also kept `service.json`
  considered valid, skipped stale-process reaping, and left orphan `.lock` files in place). Regression test added.
- **Iterative process-tree termination** (with de-duplication) instead of recursion — the `dsh-stop`
  thread only has a 256 KB stack, and deep or cyclic parent/child links could overflow it — plus a
  node budget per sweep.
- **Alignment-safe TCP listener table access**: the `GetExtendedTcpTable` buffer is now backed by
  `Vec<u32>` (`MIB_TCPTABLE_OWNER_PID` needs 4-byte alignment; `Vec<u8>` only guarantees 1), and the
  entry start offset is correct.
- **Bounded external-command probes**: the active-session `Get-CimInstance` probe now runs under a
  hard timeout — on expiry the process tree is killed and the probe honestly reports 0. The previous
  `Command::output()` had no timeout, so a stuck PowerShell/WMI call would hang the cleanup flow and
  the UI status indefinitely.
- **Reader diagnostics now reach the log file**: the two warnings emitted by the stdout reader thread
  (browser hand-off still enabled / upstream output shape changed) were stderr-only, and release builds
  are GUI-subsystem where stderr may be an invalid handle — i.e. the most valuable clues were dropped.
- **`~` resolution failure no longer degrades to a relative path**: the "scan + delete" helpers under
  `~/.dsh` are now `Option`-based and do **nothing** when the home directory cannot be resolved
  (the old fallback became `.dsh/...` relative to the current directory, which could delete another
  program's `.lock` files under the CWD).
- **Removed duplicated code and an unused dependency**: `sessions_dir()` / `sessions_root()` were
  byte-identical duplicates and are now one helper; `dsh-ui` dropped its never-used `serde` dependency;
  node resolution inside `classify_dsh_identity` is now `OnceLock`-cached (identity checks rescanned
  PATH for every candidate PID). Added `rust-version = "1.82"` (MSRV, matching the APIs in use).
- **Fixed `service.json` being written to the wrong directory** (uninstall residue + a
  verification script that could never pass): the bookkeeping used `%APPDATA%\DSHLauncher`
  (the user-config directory, **preserved by uninstall**), while this module's docs, the README,
  the maintenance manuals, the release notes, the uninstaller and `tools/verify-service-lifecycle.ps1`
  all expect `%LOCALAPPDATA%\DSHLauncher\service.json`. Two measured consequences: (1) the runtime
  record survived uninstall inside `%APPDATA%`; (2) the verifier could never read it and kept printing
  "no service.json yet", making its adoption assertion meaningless. It now lives under
  `%LOCALAPPDATA%` (next to the logs and profiles, cleaned by the uninstaller), and the stale file at
  the old location is removed on first read.
- **Deleted the dead-code APIs outright**: `dsh-core::dsh`'s `version` / `dist_tags` / `upgrade` /
  `check_health`, together with the whole npm call chain they needed (`is_safe_version` allow-list,
  `run_capture` with its timeout + process-tree reaping, `resolve_npm`, and five `DshError` variants).
  They had **no caller at all**, and the old justification ("public library API, not in the call graph,
  therefore harmless") holds for binary size but not for maintenance cost: that chain keeps drifting
  with upstream npm output and the local environment while having no user entry point whatsoever.
  No user capability is lost — the manual upgrade / native-module repair steps are documented in the
  maintenance manual §1.3–1.5 — and the security property `is_safe_version` used to carry is now
  enforced structurally by the gate (**no `cmd.exe`/shell string concatenation in production code**).
- **Fixed a self-deadlock in the service-start path (P0, reproduced live)**: `ServiceHandle::start()`
  **locked the same `std::sync::Mutex` twice** (it is not reentrant) before spawning — the second
  `lock()` from the same thread blocks forever. Because the deadlock happened *while holding* the
  `ServiceInner` lock, **every later service operation (state reads, saving settings, stopping the
  service) blocked with it**; the visible symptom was "the UI is fine but the service never starts".
  It stayed hidden because a dsh from a previous session is normally already on the port and `start()`
  returns early through the adopt branch — **only a port with no service** (fresh machine, manual stop
  then start, changed port) reaches the spawn branch. It now takes the `ProcessManager` handle from the
  guard it already holds, and the start path gained **phase timing marks**
  (`[start] reconcile / port probe / resolve dsh path / sweep locks / spawn`): those marks are what
  localised the fault to "no log at all for five minutes after the lock sweep" during a non-default-port
  start. Added a lock-count assertion for `start()` (exactly two) and an opt-in real-spawn regression
  test (`DSH_TEST_SPAWN=1`; skipped by default so `cargo test` never starts a service on a dev machine).
- **New "embedded UI requires authentication" self-heal** (a real user-visible failure: the window sat on
  `dsh web authentication required; reopen the URL printed by dsh web.`): `dsh web` authenticates in only
  two ways — the **one-time launch token** printed at startup (which lives only in the dsh process's
  memory) or an already-minted **signed cookie** (30 days, stored in WebView2's own cookie jar). When the
  launcher adopts a service started elsewhere it cannot read the token, and when the cookie is missing
  (fresh install, wiped profile, expired, changed port) the page is a guaranteed 401 — which the old code
  did not notice at all (it happily presented the 401 page as the UI). The window now self-checks **in the
  page context** after opening and after every navigation (a bare socket probe cannot tell "I already have
  the cookie"), and on a hit: no active session → restart the service automatically (the launcher then owns
  it, reads the token URL and mints the cookie); an active session → ask explicitly (default "Yes",
  spelling out that in-flight tasks are interrupted while history stays on disk).
- **Consistency gate grown to 165 checks**: the release-engineering invariants above (`--version/--help`
  present, LTS kept out of the semver version, service start off the UI thread, single throttled port
  probe, bounded external commands, liveness not based on the exit-code sentinel, non-recursive process
  trees, `start()` lock count, auth self-check/self-heal wiring, deleted APIs must not come back,
  no shell string concatenation in production code) plus an **offline `cargo machete` equivalent**
  (every direct dependency must be referenced by its own crate's sources).

#### 🧹 Self-cleaning round: build-cache reclaim (repo root 3.5 GB → 11 MB)

- **Problem**: the repository root measured **3,556 MB**, of which `target\` was **3,551 MB** — a
  **build cache, not a deliverable** (already excluded by `.gitignore`). Breakdown: `target\debug`
  2,629 MB (deps 1,292 + **incremental 1,030** + build 125 + examples 87 + pdb 55) and
  `target\release` 922 MB (deps 824 + build 88). Cargo **never reclaims** old artifacts (features,
  profiles and toolchains only ever add), so it grew monotonically with every build/verification
  round — directly at odds with the self-cleaning goal.
- **New `tools/clean.ps1`** (report-only by default — it deletes nothing unless asked): `-Cache`
  drops the debug profile and dependency caches while **keeping** `target\release\dsh-app.exe`
  (the validation input for FACTS and `verify-version.ps1`); `-All` is a `cargo clean` equivalent;
  `-WhatIf` previews. The script itself fails if a kept artifact went missing.
- **Measured**: `-Cache` reclaimed **3,544.9 MB**, taking `target\` from 3,550.8 MB to **5.8 MB** and
  the repo root from **3,556 MB to 11.3 MB**; a **cold build from the cleaned tree** is still fully
  offline — `cargo build --release --locked` **89.7 s / exit 0**, `cargo test --workspace
  --all-features` **67.1 s / 111 tests green** — and afterwards `verify-version` (28/0/0),
  `gen-facts -Check` and `check-consistency` all still pass.
- **Prevention**: `tools/finish-release.ps1` now disables incremental compilation
  (`CARGO_INCREMENTAL=0`, cutting a verification round's incremental output to zero) and records the
  `target\` size in its report (new "6b. build-cache size" line); the consistency gate gained five
  assertions (cleanup tool present, report-only default, release artifact kept, `/target/` and `*.pdb`
  ignored); the README (zh + en) and `docs/OPS-RUNBOOK.md` §6 document the commands and the
  "don't ship right after `-All`" trap.

#### 🛠 Uninstaller rewritten as a native exe (replacing the script pair)

- **Problem**: uninstall ran `uninstall.cmd` → `powershell.exe -ExecutionPolicy Bypass -File uninstall.ps1`.
  A command-line `-ExecutionPolicy Bypass` **only overrides the local setting, not Group Policy**
  (`MachinePolicy`/`UserPolicy` = AllSigned/Restricted still refuse the script), and AppLocker/WDAC can
  block script execution outright — so on a hardened machine the user **cannot uninstall at all**.
  The scripted deferred directory removal needed **PowerShell a second time** (the template even wrote a
  temp script into `%TEMP%`), the `cmd → powershell -Bypass → Remove-Item -Recurse` shape is exactly what
  AV/EDR watch for, and a script cannot earn the same trust through Authenticode signing.
- **Fix**: new `crates/dsh-uninstall` producing a native `dsh-uninstall.exe` (~300 KB, depending only on
  `dsh-core` + `windows`, no third-party crates, no script engine). The installer embeds it and points
  `UninstallString`/`QuietUninstallString` at it; the repository's `uninstall.cmd`/`uninstall.ps1` are
  **deleted** (one implementation, no second copy of the logic). Switches stay backwards compatible and
  additionally accept Windows-style spellings: `--purge/--silent/--dry-run` plus `/purge /quiet /whatif`;
  exit codes `0` success / `1` partial failure / `2` refused by a guard. Self-removal is now
  "copy itself to `%TEMP%` and re-enter".
- **Guards aligned one by one (with 16 unit tests)**: the `.dsllauncher-install` marker is required **and
  its content must belong to this app**; protected roots (volume roots, system/user directories) are never
  deleted recursively; a directory containing third-party files keeps only our files removed and the
  directory itself; only launcher instances **started from this directory** are stopped (image-path
  comparison; nothing is killed when the path cannot be read); `--dry-run` changes nothing.
- **Upgrade-path hole fixed along the way**: the installer gained `ObsoletePayloads` and now deletes the
  previous version's `uninstall.cmd`/`uninstall.ps1`. Measured: overwriting only the current payloads left
  **two uninstallers** in place — and the old script's cleanup list does not contain `dsh-uninstall.exe`
  (a user running the stale script would leave it behind), while the new uninstaller would treat those
  legacy scripts as "foreign" and keep the directory.
- **Structural drift protection**: the consistency gate gained 8 assertions (embedded / registry target /
  scripts must not come back / guards present / only this directory's instance is stopped / dry-run has no
  side effects / two-phase removal / payload lists match on both sides) and `verify-version.ps1` gained 5
  (uninstaller FileVersion / ProductVersion / OriginalFilename / ProductName / icon, plus embedded and
  registry wiring) — the version chain grew from 28 to **33** checks.
- **Measured (real install → dry-run → uninstall → zero residue → reinstall)**: dry-run reports every item
  and changes nothing; a real uninstall removes the **install directory, registry entry, desktop shortcut
  and `%LOCALAPPDATA%\DSHLauncher`** while **preserving `%APPDATA%` user config**. The live run also caught
  two defects that only a real machine exposes: (1) key existence was probed with `RegGetValueW` on the
  **default value** (the key has none, so "present" was reported as "absent") — now `RegOpenKeyExW` probes
  the key; (2) phase two mistook "target directory" for "the directory this program lives in" (the copy
  lives in `%TEMP%`), so the install directory was never removed — the correct guard is
  "target ≠ own directory".
- **Platform limitation (stated plainly)**: Windows forbids a running image from deleting itself (measured:
  `DELETE` access to one's own image is denied), so one ~300 KB copy stays in `%TEMP%` until the **next
  reboot**, when the OS removes it; each uninstall also sweeps stale copies. Documented in the README and
  in `--help`.

#### ✅ Verification

- `cargo test`: **108 unit tests**, all green
- `selftest.ps1`: **A1** (force-kill reaping) / **A2** (graceful exit keeps the service) / **B** (end-to-end) / **C** (settings-page IPC + window) / **D** (single-file distribution) / **E** (loss detection and automatic re-adoption) — **all passed**
- `tools/check-consistency.ps1`: **31** behavioural invariants passed
- Zero compiler warnings

Finalisation round — verification & tooling (new):

- New `tools/verify-service-lifecycle.ps1` (with a decisive `-Force` experiment), `tools/verify-token-navigation.ps1`, and `crates/dsh-core/examples/token_capture_probe.rs` (runtime verification of the capture path, including the race reproduction).
- New `crates/dsh-core/examples/active_session_probe.rs`: runtime verification of the active-session probe, **cross-checked through an independent path** (two different PowerShell queries) so an external-command-based implementation cannot fail silently.
- New `tools/gen-facts.ps1` + `docs/FACTS.json`: the **single source** for documented numbers, with drift detection (`-Check`). Previously one metric carried 3–4 contradictory values across five documents.
- `tools/check-consistency.ps1` now asserts the **documented numbers** themselves: the unit-test count, consistency-check count and exe size magnitude quoted in README / CHANGELOG (zh + en) / roadmap §15 must match `FACTS.json` — `gen-facts -Check` only notices "the sources changed", not "the docs did not follow". **Its first run caught three real drifts** (README size rounding, a missing English results table, and counts left at old values).
- Fixed a **false positive** in `tools/verify-token-navigation.ps1`: it only recognised the old wording "已接管端口" and flagged adoption via `service.json` reconciliation as a failure. It now decides on "was a token **ever** captured", decoupled from wording — only a capture path that never worked is a real defect.
- `tools/check-consistency.ps1` — every addition is a **behavioural** invariant (wiring, defaults, required APIs, guardrails): precisely the class of defect that passes unit tests while being unwired.
- Exact test / consistency counts live in [`FACTS.json`](FACTS.json) (generated from the sources by `tools/gen-facts.ps1`); `cargo clippy --all-targets --all-features` reports 0 warnings.

#### 📊 Measured

**Rewrite baseline (v4 → v5)**

| Metric | v4 | v5 | Change |
|---|---|---|---|
| Working set | 65,520 K | **23.4 MB** | -64% |
| Artifact | 199 KB + 902 KB DLLs | **single file 934.5 KB** | — |
| Code size | 7,720 lines | **4,089 lines** | -47% |
| Unit tests | 0 | **48** | — |

**Finalisation + release-engineering rounds (same basis)**

| Metric | Initial rewrite | Finalisation round |
|---|---|---|
| Unit tests | 65 | **149** |
| Consistency checks | 66 | **178** |
| Version-chain checks | 23 | **28** |
| Launcher working set (window closed) | 23.9 MB (a different measurement basis) | **13.2 MB** (same basis) |
| Cold-start "ready vs token" ordering | not measured (defect not yet exposed) | ready arrives **914 ms earlier** → covered by `Ready{url:None}` + re-emit |

> The authoritative source for these numbers is [`FACTS.json`](FACTS.json), generated from the
> sources and artifacts by `tools/gen-facts.ps1`.

---

<a name="v424-2026-09-10"></a>
## v4.2.4 (2026-09-10)

> 📌 **[中文](#v424-中文)** · **[English](#v424-english)**

<a name="v424-中文"></a>
### 中文更新说明

#### 🆕 新功能

- **设置窗口独立化**：设置改为**独立顶层窗口**——拥有独立任务栏项，可单独最小化/关闭，并自动记住、沿用上次位置与大小；不再与被隐藏的宿主窗口或 Harness 主窗口共用同一个窗口外观
- 退出启动器时**一并关闭设置窗口**，避免残留窗口阻塞进程退出

#### 🔧 修复与优化

- **受限权限下的身份判定修复（根因）**：非提权运行时 WMI 读不到其它进程命令行（实测命中 0），导致启动器既**无法自动接管**自己的 dsh 服务、清理时也识别不到占用者并报“无法停止 / PID 0”；新增 `IsDshHarnessOnPort`——“**node 进程 + 正是配置端口的所有者**”作为等价身份，并用于接管判定（3 处）与清理停服
- **多路径识别占用者**：`ListDshWebPids`（WMI 枚举）+ `FindPidOnPortDetailed`（netstat 及失败原因诊断），候选 PID 逐一终止
- **提权按端口强制停止**：普通权限仍失败时，经 UAC 提权用 `Get-NetTCPConnection -LocalPort` 自行识别并 taskkill，规避普通权限下的识别限制
- **dsh 基线同步至 0.1.5-alpha.2**：维护手册附录 A/B/C 同步（模型目录不变：flash / pro / vision-exp，1M 上下文；pi-ai 仍 0.85.1；**Session 数据格式仍为 V3**；外部 JSON-RPC 契约未变 → 手机端与局域网网关无需适配）

#### 📋 一致性修正

- **版本号统一 v4.2.4**：两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG / 发布说明 / 维护手册

<a name="v424-english"></a>
### English Release Notes

#### 🆕 New Features

- **Standalone settings window**: settings is now a **true top-level window** with its own taskbar entry — it can be minimized/closed independently and remembers its last position and size; it no longer shares the same window appearance as the hidden host or the Harness window
- The settings window is **closed together with the launcher on exit**, so no leftover window can block process exit

#### 🔧 Fixes & Improvements

- **Identity detection fixed under a limited token (root cause)**: when running non-elevated, WMI cannot read other processes' command lines (measured 0 hits), so the launcher could neither **auto-adopt** its own dsh service nor identify the port owner during cleanup ("cannot stop / PID 0"); new `IsDshHarnessOnPort` treats "**node process + owner of the configured port**" as an equivalent identity and is used for adoption (3 call sites) and cleanup
- **Multi-path port owner discovery**: `ListDshWebPids` (WMI enumeration) + `FindPidOnPortDetailed` (netstat plus failure diagnostics) with per-candidate termination
- **Elevated port-based force stop**: if a normal kill still fails, one UAC-elevated `Get-NetTCPConnection -LocalPort` lookup + taskkill is used, bypassing the limited-token restrictions
- **dsh baseline synced to 0.1.5-alpha.2**: maintenance-manual Appendices A/B/C updated (model catalog unchanged: flash / pro / vision-exp, 1M context; pi-ai still 0.85.1; **Session data format remains V3**; external JSON-RPC contract unchanged → mobile UI and LAN gateway need no adaptation)

#### 📋 Consistency Fixes

- **Version unified to v4.2.4**: both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG / release notes / maintenance manuals

---

<a name="v423-2026-09-09"></a>
## v4.2.3 (2026-09-09)

> 📌 **[中文](#v423-中文)** · **[English](#v423-english)**

<a name="v423-中文"></a>
### 中文更新说明

#### 🔧 修复与优化

- **停服进程身份校验（防误杀）**：仅终止“确认是 dsh 服务”的进程（命令行特征 `bin.js web`/`@deepseek-ai\dsh`，或本启动器已知句柄）；端口若被非 dsh 程序占用则**中止清理**，防止误杀无关进程
- **停服判定改为权威双条件**：仅当“目标进程对象消失 **且** 端口可重新绑定”才算停稳，修复 netstat 偶发漏报导致“已释放”假阳性（旧 dsh 未停、持续回写 `workspace.json` 使归档列表反复复活的问题）
- **清理后复活自愈**：重启后留观察窗口，若归档列表被延迟回写复活则**自动第二轮清理**（最多三轮）
- **普通终止无效时 UAC 提权终止一次**；仍失败则中止并明确提示（不误报、不改列表）
- **UI 忙碌保护**：清理为长阻塞操作，点击期间禁用按钮并显示忙碌光标

#### 📋 一致性修正

- **版本号统一 v4.2.3**：两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG / 发布说明 / 维护手册

<a name="v423-english"></a>
### English Release Notes

#### 🔧 Fixes & Improvements

- **Process identity check before killing**: only processes confirmed as dsh (command line matches `bin.js web`/`@deepseek-ai\dsh`, or the launcher's own known handle) are terminated; if a non-dsh program holds the port, cleanup **aborts** instead of killing an unrelated process
- **Authoritative stop verdict**: "stopped" now requires **both** the target process object being gone **and** the port being rebindable — fixing false "port released" results from flaky netstat (the old dsh that was never actually stopped kept rewriting `workspace.json` and resurrecting the archive list)
- **Post-restart resurrection healing**: after restart an observation window is kept; if the archive list is resurrected by a delayed rewrite, cleanup **auto-runs a second round** (up to three)
- **UAC escalation once** when a normal kill is ineffective; if it still fails, cleanup aborts with a clear message (no false success, no list mutation)
- **UI busy protection**: cleanup is a long blocking operation; the button is disabled with a wait cursor while it runs

#### 📋 Consistency Fixes

- **Version unified to v4.2.3**: both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG / release notes / maintenance manuals

---

<a name="v422-2026-09-09"></a>
## v4.2.2 (2026-09-09)

> 📌 **[中文](#v422-中文)** · **[English](#v422-english)**

<a name="v422-中文"></a>
### 中文更新说明

#### 🔧 修复与优化

- **归档会话清理“占用”失败与列表复活修复（根因）**：清理前改为**按端口停稳 dsh 服务**（定位监听端口进程杀树，含“未接管/外部实例”），并**等待端口真正释放**后再删除——此前运行中的 dsh 与删除并发导致“占用”失败，并会周期回写 `workspace.json` 复活归档标记
- **遗留归档标记与投影缓存清理**：目录已不存在的归档 id 直接清出列表；同时删除 `session_projcache/sessions/<id>.json`，防止服务重启后列表复活
- **删除重试**：目录删除失败自动重试，仅保留真正失败的 id 供稍后重试

#### 📋 一致性修正

- **版本号统一 v4.2.2**：两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG / 发布说明 / 维护手册

<a name="v422-english"></a>
### English Release Notes

#### 🔧 Fixes & Improvements

- **"Clean archived sessions" "in use" failures and list resurrection fixed (root cause)**: cleanup now **stops dsh by port** (locates the listener process, kills its tree, including non-adopted/external instances) and **waits until the port is actually released** before deleting — the live dsh previously raced the deletion (→ "in use" failures) and periodically rewrote `workspace.json`, resurrecting archive markers
- **Leftover markers and projection cache purged**: archived ids whose dirs no longer exist are removed outright; matching `session_projcache/sessions/<id>.json` entries are deleted too, so the list can no longer be resurrected after a restart
- **Delete retries**: directory deletion retries automatically; only truly failing ids stay in the list for a later retry

#### 📋 Consistency Fixes

- **Version unified to v4.2.2**: both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG / release notes / maintenance manuals

---

<a name="v421-2026-09-09"></a>
## v4.2.1 (2026-09-09)

> 📌 **[中文](#v421-中文)** · **[English](#v421-english)**

<a name="v421-中文"></a>
### 中文更新说明

#### 🔧 修复与优化

- **修复“清理归档会话”恒删除 0 个的问题（根因）**：dsh 0.1.5-alpha.1 的归档会话目录与 `archivedSessionIds` 均为 44 字符 `session-<36位uuid>`，而删除守卫 `^[a-f0-9-]{36}$` 只接受 36 字符 → 所有归档目录被拒、永不删除；现改为 `^(session-)?[a-f0-9-]{36}$`（保留 reparse-point 与防遍历检查）
- **清理时序与一致性修复**：清理前先停止 dsh 服务（消除会话文件句柄/锁占用与并发写窗口）；仅从归档列表移除“确实删除成功”的 id，全部删除失败时不改写 `workspace.json`——不再产生“磁盘数据残留 + 归档列表被清空”的孤儿状态

#### 📋 一致性修正

- **版本号统一 v4.2.1**：两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG / 发布说明 / 维护手册

<a name="v421-english"></a>
### English Release Notes

#### 🔧 Fixes & Improvements

- **"Clean archived sessions" always deleted 0 (root cause)**: archived session dirs and `archivedSessionIds` are 44-char `session-<36-hex-uuid>` under dsh 0.1.5-alpha.1, while the deletion guard `^[a-f0-9-]{36}$` accepted only 36 chars → every archived dir was rejected and nothing was ever deleted; the guard is now `^(session-)?[a-f0-9-]{36}$` (reparse-point and traversal checks kept)
- **Cleanup ordering & consistency**: the dsh service is stopped before deletion (removes file-handle/lock contention and the concurrent-write window); only ids that were actually deleted are removed from the archive list, and `workspace.json` is left untouched when nothing was deleted — no more orphaned on-disk data with a cleared list

#### 📋 Consistency Fixes

- **Version unified to v4.2.1**: both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG / release notes / maintenance manuals

---

<a name="v420-2026-09-09"></a>
## v4.2.0 (2026-09-09) — RC

> 📌 **[中文](#v420-中文)** · **[English](#v420-english)**

<a name="v420-中文"></a>
### 中文更新说明

#### 🔄 架构与一致性治理

- **版本号全局统一 v4.2.0**：两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG / 发布说明 / 维护手册同步
- **更新日志连贯化**：整理 CHANGELOG 版本导航与锚点（导航指向缺失段落的漂移已修复）；上一开发周期内完成、尚未单独发布的解析器修复等条目已并入本版「修复与优化」
- **新增一致性校验工具** `tools/check-consistency.ps1`：一键扫描版本号漂移、fs-ext 残留引用、中英维护手册关键锚点与版本基线一致性

#### 🔧 修复与优化

- **「修复模块」原生模块定位修复**：npm 12 全局安装把依赖嵌套于 `@deepseek-ai/dsh\node_modules`，旧实现只探测全局顶层目录 → koffi / node-pty 恒被判「未安装，跳过」，修复模块实际空转；现改为嵌套 + 顶层双路径探测，按实际存在目录重建
- **fs-ext 移除适配**：dsh 0.1.5-alpha.1 起官方移除 fs-ext（文件锁改用 `@deepseek-ai/node-addon-system`）；健康检查失败指引、「修复模块」按钮提示与确认框、维护手册去旧化——fs-ext 缺失不再误判为故障 G，安装旧版（≤0.1.4）时仍会自动重建
- **dsh 基线复核**：本机 dsh = 0.1.5-alpha.1（npm alpha 标签；latest/next = 0.1.2-rc.1），API/凭据/模型引用契约不变（详见维护手册附录）
- **npm dist-tags JSON 解析器修复**：替换脆弱的逐字符手写解析器为正则表达式，修复 npm 返回数组格式 `[{...}]` 时无法解析的问题；现在可正确检测 npm 推送的所有版本（包括 0.1.5-alpha.1）；兼容紧凑/格式化/数组/对象四种 JSON 形式

#### 📋 一致性修正

- **修复「修复模块」空转**（v4.1.0 功能在 npm 12 嵌套布局下失效，本次为根因修复）

<a name="v420-english"></a>
### English Release Notes

#### 🔄 Architecture & Consistency Governance

- **Version unified to v4.2.0 project-wide**: both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG / release notes / maintenance manuals
- **Changelog made contiguous**: version navigator/anchors tidied up (drift pointing at a missing section fixed); parser-fix and other items completed in the previous development cycle but not released standalone are folded into the "Fixes & Improvements" section of this release
- **New consistency checker** `tools/check-consistency.ps1`: scans for version drift, leftover fs-ext references, and zh/en manual anchor/baseline mismatches

#### 🔧 Fixes & Improvements

- **"Fix Modules" module-location fix**: npm 12 global installs nest dependencies under `@deepseek-ai/dsh\node_modules`, while the old code probed only the top-level global directory → koffi / node-pty were always judged "not installed, skipped", making the button a no-op; now probes both the nested and top-level layouts and rebuilds wherever the module actually lives
- **fs-ext removal adaptation**: upstream removed fs-ext in dsh 0.1.5-alpha.1 (file locks now use `@deepseek-ai/node-addon-system`); health-check failure guidance, "Fix Modules" tooltips/confirm dialog and the maintenance manuals were de-referenced — a missing fs-ext no longer triggers Failure G, while installing legacy versions (≤0.1.4) still rebuilds it automatically
- **dsh baseline re-check**: local dsh = 0.1.5-alpha.1 (npm `alpha` tag; latest/next = 0.1.2-rc.1), API/credential/model-reference contract unchanged (see maintenance-manual appendix)
- **npm dist-tags JSON parser fix**: replaced the fragile character-by-character hand-written parser with a regex-based approach, fixing the issue where npm returning array format `[{...}]` could not be parsed; now correctly detects all versions published to npm (including 0.1.5-alpha.1); compatible with compact/formatted/array/object JSON forms

#### 📋 Consistency Fixes

- **Fixed "Fix Modules" no-op** (v4.1.0 feature was dead under the npm 12 nested layout — root-cause fix)

---

<a name="v410-2026-09-08"></a>
## v4.1.0 (2026-09-08)

> 📌 **[中文](#v410-中文)** · **[English](#v410-english)**

<a name="v410-中文"></a>
### 中文更新说明

#### 🆕 新功能

- **多通道版本选择**：升级页下拉框列出所有 npm dist-tags（latest / alpha / next / rc / beta / dev 等），可自由选择安装预览版或稳定版通道
- **「修复模块」按钮**：一键重编 fs-ext / koffi / node-pty 三个原生模块，自动定位 npm 全局目录并逐个 `node-gyp rebuild`，修复 ABI 不匹配或编译缺失
- **升级健康检查**：版本验证通过后、重启服务前新增 `CheckDshHealth` 探测，检测原生模块是否正常加载，异常时给出明确指引

#### 🔧 修复与优化

- **allow-scripts 白名单补全**：`npm install -g` 命令加入 `fs-ext` 到白名单（`fs-ext,koffi,node-pty,...`），防止 fs-ext 编译脚本被拦截导致 `Cannot find module 'fs_ext'`
- **强制官方 registry**：所有 npm 命令（`npm view` / `npm install`）统一添加 `--registry=https://registry.npmjs.org/`，不受本地 `.npmrc` 镜像源影响
- **当前版本健康提示**：版本检测附加原生模块健康状态，异常时显示 `⚠ 原生模块异常` 并引导点击「修复模块」
- **检查更新后静默探测**：检查更新后后台自动探测原生模块健康，异常时在状态栏提示
- **升级失败指引优化**：健康检查失败时给出明确指引：`请点击「修复模块」按钮重新编译 fs-ext/koffi/node-pty`
- **dsh 升级至 0.1.3-alpha.2**（npm alpha 标签；latest/next 仍为 0.1.2-rc.1）：经多通道功能从 rc.1 升级；核验 fs-ext 可加载 / koffi 3.2.1 / node-pty 可加载、模型目录与 rc.1 一致；外部 JSON-RPC 契约未变（`session/list`、`session/page`、`session/prompt` 结构不变）→ 手机 UI 与网关零适配

#### 🔒 安全修复

- **nodePath 文件名校验**：`settings.ini` 中 `nodePath` 现在必须指向 `node.exe`，防止 settings.ini 被篡改后启动任意程序
- **归档会话删除 reparse point 检查**：删除归档会话前检查目录是否为符号链接/junction，防止误删链接目标目录；同时校验 sessionId 格式（仅允许 [a-f0-9-]）
- **OLLAMA_ORIGINS 限制**：开启局域网时 `OLLAMA_ORIGINS` 从 `*` 限制为 `http://127.0.0.1:{port}`（dsh web 来源），防止任意来源跨域调用 Ollama API

#### 🧹 冗余精简

- **RunCommand 废弃参数清除**：移除 `RunCommand()` 中从未使用的 `elevated` 参数及 3 处调用方的 `false` 实参
- **FindPidOnPort 死代码清除**：删除 IPGlobalProperties 分支（遍历结果被完全丢弃，PID 100% 回退 netstat），直接调用 netstat
- **FindPidOnPortByNetstat 合并**：改为转发调用 FindPidOnPort，消除重复的 netstat 逻辑
- **废弃兼容注释清除**：删除 Settings.Load 中描述不存在历史兼容的注释
- **.gitignore 简化**：`*.old`/`*.old2`/`*.old3`/`*.old4` 合并为 `*.old*`

#### 📋 一致性修正

- **allow-scripts 白名单统一**：启动器代码与 MAINTENANCE 文档白名单保持一致（均包含 fs-ext）
- **版本号统一**：v4.1.0（两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG）
- **README 版本号**：功能列表中 "v2.0"/"v3.0" → "v2.0.0"/"v3.0.0"，全项目 semver 三段式统一
- **模板 uninstall.cmd 单引号修复**：改用环境变量 `$env:KILL_DIR` 传递路径，与 WriteUninstallCmd 生成版行为一致
- **netsh 编码注释修正**：修正 RunCommand 中自相矛盾的 "UTF-8" 注释，改为准确的 "系统 ANSI 代码页"

<a name="v410-english"></a>
### English Release Notes

#### 🆕 New Features

- **Multi-channel version selection**: upgrade page dropdown lists all npm dist-tags (latest / alpha / next / rc / beta / dev, etc.), allowing free choice of preview or stable channels
- **"Fix Modules" button**: one-click rebuild of fs-ext / koffi / node-pty native modules; auto-locates npm global directory and runs `node-gyp rebuild` for each, fixing ABI mismatch or missing compilation
- **Post-upgrade health check**: new `CheckDshHealth` probe after version verification but before service restart; detects whether native modules load correctly, with clear guidance on failure

#### 🔧 Fixes & Improvements

- **allow-scripts whitelist completed**: added `fs-ext` to the whitelist (`fs-ext,koffi,node-pty,...`) in `npm install -g` command, preventing fs-ext build script interception that caused `Cannot find module 'fs_ext'`
- **Enforce official registry**: all npm commands (`npm view` / `npm install`) now include `--registry=https://registry.npmjs.org/`, immune to local `.npmrc` mirror config
- **Current version health indicator**: version check now appends native module health status; shows `⚠ Native module error` on failure with guidance to click "Fix Modules"
- **Silent health probe after update check**: background native module health probe after checking for updates; shows status bar warning on failure
- **Improved upgrade failure guidance**: health check failure now shows clear instructions: `Click "Fix Modules" to rebuild fs-ext/koffi/node-pty`
- **dsh upgraded to 0.1.3-alpha.2** (npm alpha tag; latest/next still 0.1.2-rc.1): upgraded from rc.1 via the multi-channel feature; verified fs-ext loadable / koffi 3.2.1 / node-pty loadable and the model catalog identical to rc.1; external JSON-RPC contract unchanged (`session/list`, `session/page`, `session/prompt` structures intact) — mobile UI and gateway need no adaptation

#### 🔒 Security Fixes

- **nodePath filename validation**: `nodePath` in `settings.ini` must now point to `node.exe`, preventing arbitrary program launch if settings.ini is tampered with
- **Archive session deletion reparse point check**: checks whether a directory is a symlink/junction before deleting archived sessions, preventing accidental deletion of link targets; also validates sessionId format (only `[a-f0-9-]` allowed)
- **OLLAMA_ORIGINS restriction**: when LAN sharing is enabled, `OLLAMA_ORIGINS` is now restricted to `http://127.0.0.1:{port}` (dsh web origin) instead of `*`, preventing arbitrary origins from cross-origin access to the Ollama API

#### 🧹 Redundancy Cleanup

- **Dead parameter in RunCommand**: removed the never-used `elevated` parameter from `RunCommand()` and the `false` arguments at all 3 call sites
- **Dead code in FindPidOnPort**: removed the IPGlobalProperties branch (its traversal result was completely discarded; PID lookup always fell back to netstat); now calls netstat directly
- **FindPidOnPortByNetstat merged**: now forwards to FindPidOnPort, eliminating duplicate netstat logic
- **Obsolete compatibility comment removed**: deleted the comment in Settings.Load describing non-existent historical compatibility
- **.gitignore simplified**: merged `*.old`/`*.old2`/`*.old3`/`*.old4` into `*.old*`

#### 📋 Consistency Fixes

- **allow-scripts whitelist unified**: launcher code and MAINTENANCE doc whitelist now consistent (both include fs-ext)
- **Version unified**: v4.1.0 (both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG)
- **README version**: "v2.0"/"v3.0" → "v2.0.0"/"v3.0.0" in features list, semver three-segment format unified across the project
- **Template uninstall.cmd single-quote fix**: now uses environment variable `$env:KILL_DIR` to pass the path, matching the behavior of the WriteUninstallCmd-generated version
- **netsh encoding comment fixed**: corrected the self-contradictory "UTF-8" comment in RunCommand to accurately state "system ANSI code page"

---

<a name="v400-2026-09-05"></a>
## v4.0.0 (2026-09-05)

> 📌 **[中文](#v400-中文)** · **[English](#v400-english)**

<a name="v400-中文"></a>
### 中文更新说明

#### 🔒 安全修复

- **防火墙规则卸载残留**：`netsh delete rule name=` 不支持通配符导致卸载后防火墙规则泄漏，改用 `Get-NetFirewallRule` + `Remove-NetFirewallRule`
- **WebSocket upgrade 数据丢失**：`proxyUpgrade()` 未转发客户端 `head` 数据导致首帧消息损坏，添加 `psocket.write(head)` 转发
- **PowerShell 命令注入**：`RunFirewallElevated()` 路径含 `$()` 可被解释为脚本块在 UAC 提权下执行，改用单引号 + 转义
- **Token URL 泄露**：dsh 启动令牌通过 URL 查询参数发送可能出现在服务器日志中，改用 `X-DSH-Token` Header

#### 🔧 修复与优化

- **全面审阅修复**：probeReady 竞态条件（代际号）、UpgradeDsh 管道死锁（异步排空）、升级回调进程崩溃守卫（IsDisposed）、cache-bust 硬编码 IP、WebView2 检测优化（单层扫描）、下载退避重试、Node 多版本选择一致性、卸载脚本增强（防火墙规则 + %APPDATA% 清理）
- **netsh 输出编码**：硬编码 UTF-8 导致中文 Windows 下"已启用"等关键词匹配失败，改为 `Encoding.Default`
- **上游超时客户端挂起**：`doProxy()`/`proxyUpgrade()` 超时仅销毁连接未返回错误，添加 504/502 响应
- **未处理 Promise 拒绝**：`ensureDshCookie().then()` 无 `.catch()` 导致客户端挂起，添加错误处理
- **Token 兑换竞态条件**：并发调用方立即返回 false 导致不必要的 502，改用共享 Promise 单飞模式
- **NodeMsiArch 位数检测**：`PROCESSOR_ARCHITECTURE` 返回进程位数而非 OS 位数，改用 `Is64BitOperatingSystem`
- **WebView2 检测**：仅检查 `ProgramFilesX86`，添加 `ProgramFiles` 回退以兼容 ARM64/32 位系统
- **FindPidOnPort 性能**：优先使用 `IPGlobalProperties.GetActiveTcpListeners()`（毫秒级），netstat 作为回退
- **安装向导 Finish 逻辑**：`else if` 导致勾选"新手指引"时跳过"立即启动"，改为两个独立 `if`

#### 🧹 冗余精简

- **RunProcess 死代码**：移除永不读取的 `StringBuilder buf` 及 `lock` 语句
- **ExtractResource 重复注释**：移除方法上方的冗余注释
- **RunCommand 死分支**：移除从未执行的 `elevated` 分支
- **selftest.log**：删除生成产物

#### 📋 一致性修正

- **版本号统一**：v4.0.0（两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG）
- **README 文档路径**：`docs/MAINTENANCE.zh.md` → `MAINTENANCE.zh.md`（与安装包实际部署位置一致）
- **README 版本号**：功能列表中 "v4.0" → "v4.0.0"
- **卸载行为**：与 README 承诺一致（保留 settings.ini，重装后配置不丢失）
- **uninstall.cmd 路径传递**：通过环境变量传递路径，避免单引号解析失败

<a name="v400-english"></a>
### English Release Notes

#### 🔒 Security Fixes

- **Firewall rule leak on uninstall**: `netsh delete rule name=` doesn't support wildcards, leaving stale inbound rules; switched to `Get-NetFirewallRule` + `Remove-NetFirewallRule`
- **WebSocket upgrade data loss**: `proxyUpgrade()` silently dropped the client's buffered frame data, corrupting the first WebSocket messages; now forwards via `psocket.write(head)`
- **PowerShell command injection**: a malicious username containing `$()` could execute as a script block at UAC-elevated privilege; switched to single-quote escaping
- **Token URL exposure**: dsh one-time auth token sent via query parameter could appear in server logs; switched to `X-DSH-Token` header

#### 🔧 Fixes & Improvements

- **Full audit fixes**: probeReady race (generation counter), UpgradeDsh pipe deadlock (async drain), upgrade callback crash guard (IsDisposed), cache-bust hardcoded IP, WebView2 detection optimization (single-level scan), download backoff retry, Node multi-version selection consistency, uninstaller enhancement (firewall + %APPDATA% cleanup)
- **netsh output encoding**: hardcoded UTF-8 broke Chinese keyword matching on Chinese Windows; switched to `Encoding.Default`
- **Upstream timeout hangs client**: timeout handler only destroyed the connection without sending an error response; now returns 504/502
- **Unhandled promise rejection**: `ensureDshCookie().then()` had no `.catch()`, leaving client sockets hanging; added error handling
- **Token exchange race condition**: concurrent callers returned false immediately, causing spurious 502 errors; switched to shared promise (single-flight) pattern
- **OS bitness detection**: `PROCESSOR_ARCHITECTURE` returns process bitness, not OS; switched to `Is64BitOperatingSystem`
- **WebView2 detection**: only checked `ProgramFilesX86`; added `ProgramFiles` fallback for ARM64/32-bit compatibility
- **FindPidOnPort performance**: now uses `IPGlobalProperties.GetActiveTcpListeners()` (ms-level) first, with netstat as fallback
- **Installer Finish logic**: `else if` meant checking "show guide" skipped "launch app"; now two independent `if` statements

#### 🧹 Redundancy Cleanup

- **Dead code**: removed `StringBuilder buf` and `lock` statements that were never read
- **Duplicate comment**: removed redundant comment above `ExtractResource`
- **Dead branch**: removed never-executed `elevated` branch from `RunCommand`
- **selftest.log**: removed generated artifact from repository

#### 📋 Consistency Fixes

- **Version unified**: v4.0.0 (both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG)
- **README doc paths**: `docs/MAINTENANCE.zh.md` → `MAINTENANCE.zh.md` to match actual installer deployment layout
- **README version**: "v4.0" → "v4.0.0" in features list
- **Uninstall behavior**: matches README promise (preserves settings.ini, config survives reinstall)
- **uninstall.cmd path handling**: passes path via environment variable to avoid single-quote parsing issues

---

<a name="v300-2026-09-01"></a>
## v3.0.0 (2026-09-01)

> 📌 **[中文](#v300-中文)** · **[English](#v300-english)**

<a name="v300-中文"></a>
### 中文更新说明

#### 🆕 新功能

- **局域网共享与手机端专属 UI**：手机/平板在同一 WiFi 下扫码即可访问（默认关闭，行为与旧版一致）
- **全新独立移动端 UI**（非 dsh 原生）：会话列表按工作区分组折叠、完整聊天（历史、上滑加载更早、对话大纲跳转、底部输入栏）、只读模式（前端隐藏 + 网关 API 拦截双重保障）
- **PIN/Token 门禁 + 安全加固**：HttpOnly Cookie、速率限制（防 X-Forwarded-For 伪造）、会话密钥轮换、只绑定具体 IP 绝不绑定 0.0.0.0、防火墙限定 `remoteip=localsubnet`
- **归档会话彻底清理**：设置面板一键删除归档会话全部数据（不可恢复，重启生效）

#### 🔧 修复与优化

- **全面审查修复**：归档清理正则失效、token/PIN 明文日志、网关限速绕过、WebView2 环境复用、PIN 自定义失效、升级安装误杀 dsh web、安装窗口假死、下载容错、Node 版本/PATH 等
- **手机端修复**：subagent 注入消息不再冒充用户消息（大纲/聊天干净）；对话大纲独立分页（可加载到最早、跳转自动定位）；大纲时间正序显示；分组名/会话标题两行完整显示
- **运行日志 UTF-8 修复**：中文不再乱码（dsh web 与网关进程显式 UTF-8 解码）
- **dsh 升级至 0.1.2-rc.1**：Session persistence API 内部变更（SessionHandle + session lock）不影响外部 JSON-RPC 契约，手机 UI 与网关零适配
- **最终审查修复**：401 重试不再丢失响应、改 PIN 后网关强制重启（旧 PIN/Cookie 立即失效）、大纲"加载更早"可达且节点去重、SSE 长静默不再被超时掐断、速率限制数值校验、日志脱敏全覆盖等 18 项
- **文档重构**：README 精简为双语主文档，升级维护手册拆分至 docs/MAINTENANCE.zh.md / MAINTENANCE.en.md

<a name="v300-english"></a>
### English Release Notes

#### 🆕 New Features

- **LAN sharing & standalone mobile UI**: phones/tablets on the same WiFi scan a QR code to access (off by default, identical to old behavior when disabled)
- **Brand-new standalone mobile UI** (not the dsh native UI): collapsible grouped session list, full chat (history, load-earlier, outline navigation, composer), read-only mode (hidden in UI + blocked at the gateway API)
- **PIN/Token gate + security hardening**: HttpOnly cookie, rate limiting (forgery-proof), session-secret rotation, binds only the concrete IP (never 0.0.0.0), firewall scoped to `remoteip=localsubnet`
- **One-click archived-session purge** in Settings (not recoverable; takes effect after restart)

#### 🔧 Fixes & Improvements

- Full audit fixes: archive-purge regex, plaintext token/PIN logs, rate-limit bypass, WebView2 env reuse, PIN customization, upgrade killing dsh web, frozen installer UI, download resilience, Node version/PATH, etc.
- Mobile fixes: subagent-injected messages no longer masquerade as user messages; paginated standalone outline (load to earliest, auto-locate on jump); chronological outline order; full two-line group/session titles
- UTF-8 process-output fix: no more garbled Chinese in the log (explicit UTF-8 decoding for dsh web & gateway)
- dsh upgraded to 0.1.2-rc.1: Session persistence API internal change (SessionHandle + session lock) does not affect external JSON-RPC contract — mobile UI and gateway need no adaptation
- Final audit fixes: 401 retry no longer loses the response, gateway hard-restart on PIN change (old PIN/cookies invalidated immediately), outline load-earlier reachable with dedup, SSE no longer cut by idle timeout, rate-limit value validation, full log redaction coverage, and more (18 items)
- Docs rework: README slimmed to a bilingual main doc; the maintenance manual moved to docs/MAINTENANCE.zh.md / MAINTENANCE.en.md

---

<a name="v200-2026-08-31"></a>
## v2.0.0 (2026-08-31)

> 📌 **[中文](#v200-中文)** · **[English](#v200-english)**

<a name="v200-中文"></a>
### 中文更新说明

#### 🆕 新功能

- **token 认证适配**：自动捕获 `dsh web` 的一次性 token URL（dsh 0.1.2-alpha 强制认证）
- **退出保留服务**：退出启动器默认保留 dsh web 后台运行，网页端不中断；下次打开自动接管

#### 🔧 修复与优化

- **手册并入 README**（单文档随发布）；安装包部署 README；卸载脚本通用化（任意安装位置可用）

<a name="v200-english"></a>
### English Release Notes

#### 🆕 New Features

- **Token-auth adaptation**: auto-captures the one-time token URL from `dsh web` (mandatory since dsh 0.1.2-alpha)
- **Keep service on exit**: quitting keeps dsh web running; the web page stays connected and is auto-adopted next launch

#### 🔧 Fixes & Improvements

- Manual merged into README (single doc ships with release); installer ships README; portable uninstaller

---

<a name="v100-2026-08-14"></a>
## v1.0.0 (2026-08-14)

> 📌 **[中文](#v100-中文)** · **[English](#v100-english)**

<a name="v100-中文"></a>
### 中文更新说明

#### 🆕 新功能

- **首个发布**：内嵌 WebView2 桌面启动器（无需浏览器）、自动启动/接管/自愈、托盘常驻、零依赖构建（系统 csc）

<a name="v100-english"></a>
### English Release Notes

#### 🆕 New Features

- **First release**: embedded WebView2 desktop launcher (no browser), auto start/adopt/self-heal, tray resident, zero-dependency build (system csc)
