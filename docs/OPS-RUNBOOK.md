# 发布/维护操作手册（OPS）— DSHLauncher

> 面向「本机发布与验证」的操作规程。记录 2026-09-10 v5.0.0 收尾审计中**实际踩到的环境级事故**、
> 恢复步骤与预防规则。故障现象、根因与处置均已复现或可复现，不是推测性建议。

> 📜 **历史记录说明（重要）**：本手册正文中的 `5.0.1` 是 **v5.0.0 定稿期间使用过的内部迭代标识**，
> 该迭代**从未对外发布**，其全部内容已并入 **v5.0.0**。下文**有意保留** `5.0.1` 字样，因为本节
> 记录的是当时的真实现场与日志（改动这些历史叙述会破坏事故复盘的可信度）；阅读时请把它理解为
> 「v5.0.0 定稿前审计轮」，对应 [`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md) 所载工作。

---

## 1. 事故记录：反复强杀 GUI 进程导致 Windows 无法再创建进程

### 1.1 现象

在同一个会话里连续执行「启动 → 采样 → `Stop-Process -Force`」的启动器内存基线测量后，
**所有新进程都无法启动**，表现为：

```text
subprocess-local: Windows Job runner exited with exit code 3221225794 before proving its managed range empty
Error: subprocess-local: Windows Job runner exited with exit code 3221225794 ...
```

- `3221225794 = 0xC0000142 = STATUS_DLL_INIT_FAILED`（`STATUS_DLL_INIT_FAILED` 的十进制形式）。
- 连 `cmd /c echo`、`powershell -Command` 这类最小命令也失败 → 不是项目代码问题，而是
  **Windows 进程/会话资源耗尽**（桌面堆 desktop heap、句柄或 job object 名额）。
- 失效范围：本机所有新的进程创建（含 IDE、终端、构建）。已存在的进程继续运行
  （本次事故中 `node`（dsh web）仍在 `127.0.0.1:3080` 服务，故 GUI 未中断）。

### 1.2 根因

DSHLauncher 是 **GUI + WebView2** 进程：

1. 每次启动会创建 tao 窗口 + WebView2 运行时的**多个子进程**（`msedgewebview2.exe`）。
2. 测量脚本用 `Stop-Process -Force` 杀掉启动器 → 它是 Job Object 的持有者，
   但 `-Force` 走的是 `TerminateProcess`，**不走优雅退出路径**，WebView2 子进程与
   DWM/输入法挂接的会话对象不会立即释放。
3. 短时间内重复「启动 6 个 WebView2 子进程 → 强杀 → 再启动」多轮，桌面堆与会话对象累积，
   最终窗口站/桌面无法再为新进程初始化 DLL → `0xC0000142`。

### 1.3 恢复步骤（按顺序，通常第 3 步即可）

```powershell
# 1) 结束所有残留的本项目 GUI 进程与其 WebView2 宿主
Get-Process DSHLauncher -ErrorAction SilentlyContinue | Stop-Process -Force
Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Where-Object { $_.CommandLine -like '*DSHLauncher*' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force }

# 2) 等待资源回收（会话对象释放不是瞬时的）
Start-Sleep -Seconds 10

# 3) 复测进程创建
cmd /c "echo env-probe-ok"          # 出现 env-probe-ok 即已恢复
```

若第 3 步仍失败：**注销当前用户 → 重新登录**；仍失败则**重启 Windows**。
（桌面堆属于窗口站资源，只有会话销毁/重建才能彻底回收。）

> 上面的第 1、2、3 步已内置到 `tools/finish-release.ps1` 的「0. 环境健康检查」与
> 「1. 清理残留进程」两节：脚本会先探测、清理，再继续发布流程。

### 1.4 预防规则（发布/基准测量时必须遵守）

| # | 规则 | 原因 |
|---|---|---|
| 1 | **测量启动器内存基线时，优先复用已在运行的实例**（`Get-Process` 直接采样），不要为了「干净启动」反复重启 | 每次重启都会重建一整套 WebView2 子进程 |
| 2 | 必须重启时，**每轮之间至少间隔 5 秒**，并确认 `msedgewebview2` 子进程数已回落 | 给会话对象释放留出时间 |
| 3 | 优先用**优雅退出**（托盘「退出」/ 关闭窗口）而不是 `Stop-Process -Force` | Job Object 与 WebView2 能正常回收 |
| 4 | 强杀后立刻清点 `Get-Process msedgewebview2`，异常增长时先执行 §1.3 | 及时止损，避免累积到 `0xC0000142` |
| 5 | 一轮发布验证中，**GUI 进程启动次数控制在个位数** | 桌面堆预算有限 |
| 6 | 需要多次基线采样时，改用 `--selftest`（无窗口、无 WebView2）测服务链路，用**单次** GUI 启动测内存 | 把开销大的操作降到一次 |

> **v5.0.1（历史内部迭代标识，即 v5.0.0 定稿轮；见文首「历史记录说明」）起的变化**：默认 `service_lifecycle = "independent"`，dsh **不再挂在启动器的
> Job 上**。因此上表第 3 条的理由（Job Object 回收）不再适用于 dsh —— 强杀启动器
> **不会**回收 dsh。要回收请显式用托盘「停止服务」或 `--quit`（`tied` 模式下仍随启动器回收）。

---

## 1b. 事故记录：启动器只剩托盘、没有任何窗口（确定性死锁）

### 1b.1 现象

用户报「启动器像一个后台进程，打不开设置和其他任何东西」。实测现场：

- `DSHLauncher.exe` 进程在、托盘图标在（窗口类 `tray_icon_app` 存在）；
- **没有任何可见窗口**；所有已创建窗口 `IsHungAppWindow = True`；
- **CPU 4 秒增量 = 0 ms**（不是慢，是**死锁**）；
- 日志固定停在 `托盘图标已创建。` 之后（`进入事件循环。` 永不出现）。

### 1b.2 根因（历史内部迭代 `5.0.1` 已修，即 v5.0.0 定稿轮；见文首「历史记录说明」）

`crates/dsh-app/src/service.rs` 的 `ServiceHandle::start()` 里，注释写着
「先释放锁再 spawn 子进程」，但 `self.inner.lock()` 的守卫**从函数开头一直活到 spawn 之后**
—— 即**持锁 spawn**。dsh 子进程 stdout 一有输出，读取线程上的 token 捕获回调
（取同一把 `Arc<Mutex<..>>`）就阻塞，而主线程 spawn 返回后又去重新取锁 ⇒ 互相等待。

**为什么单测抓不到**：需要"真实子进程 + 真实 stdout 回调"才会撞上。

**为什么旧版更早没暴露**：捕获回调在旧实现里不会补发事件，触发时机更晚；v5.0.1（即 v5.0.0
定稿轮，见文首说明）让回调在拿到 token 时**补发 `Ready`**，于是回调更早、更稳定地撞上这把锁。

### 1b.3 恢复步骤

```powershell
# 1) 结束卡死的启动器（它已经什么都做不了，结束不损失能力）
Get-CimInstance Win32_Process -Filter "ParentProcessId=<卡死PID>" |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Stop-Process -Id <卡死PID> -Force -ErrorAction SilentlyContinue

# 2) 换成修复版产物（无需覆盖运行中的 exe，见 §1c）
pwsh -NoProfile -File D:\DSHLauncher\build.ps1 release
#    build.ps1 已内置热替换：直连覆盖失败时自动"改名让位 + 放入新产物"，
#    并清理历史产物（只保留最近一个 .bak 作回退点）。

# 3) 启动并**用窗口/CPU/标记判定**，而不是只看进程在不在
$p = Start-Process D:\DSHLauncher\DSHLauncher.exe -PassThru
Start-Sleep 20
pwsh -NoProfile -File D:\DSHLauncher\tools\verify-deployed.ps1
#    ↑ 校验三件事：根产物自报修复标记（--build-info）／确有可见窗口／窗口不 hung
Get-Content "$env:LOCALAPPDATA\DSHLauncher\logs\launcher.log" -Tail 20   # 应见「进入事件循环。」
```

### 1b.4 预防规则

| # | 规则 | 原因 |
|---|---|---|
| 1 | **任何 `spawn` 子进程之前必须显式释放 ServiceInner 锁** | 子进程的 stdout 回调会取同一把锁；持锁 spawn = 死锁 |
| 2 | 保留 `debug_assert!(self.inner.try_lock().is_ok())` 的断言 | 把"持锁 spawn"在开发期暴露，而不是等用户报"只剩托盘" |
| 3 | 判定"启动成功"必须看**窗口 + CPU**，不能只看进程存在 | 死锁时进程、托盘图标都在，但没有任何窗口 |
| 4 | 任何"回调里取锁"的设计都要问一句：**调用链上有没有人正持着它？** | 本仓库 13.3 #1 的自死锁是同一类错误 |

---

## 1c. 操作记录：不停实例热替换启动器产物

### 1c.1 问题

Windows **不允许覆盖正在运行**的 exe。于是"发布新产物"与"不结束启动器"看似不可兼得：

```text
Copy-Item : The process cannot access the file '...\DSHLauncher.exe'
            because it is being used by another process.
```

而结束启动器会（在 v5.0.0 语义下）切断正在进行的 dsh 会话。

### 1c.2 可行做法：先改名，再放入

Windows **允许对运行中的 exe 改名**（改的是目录项，不是文件内容），因此：

```powershell
cd D:\DSHLauncher
# 1) 旧产物改名让位（正在运行的进程不受影响，继续用旧映像）
Rename-Item .\DSHLauncher.exe ('DSHLauncher.exe.bak-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
# 2) 放入新产物
Copy-Item .\target\release\dsh-app.exe .\DSHLauncher.exe -Force
> 路径说明（构建目录布局）：`target\release\` 是**最终产物**目录，Cargo 承诺其布局不变。
> 中间产物（`deps/`、`.fingerprint/`、按包名分桶的 `build/<包名>/<哈希>/`、`incremental/`）
> 在 Cargo 1.91+ 可用 `CARGO_BUILD_BUILD_DIR` / `[build] build-dir` 整块搬出 `target\`，
> Cargo 1.100 起的新布局也会改变它们的组织方式 —— **任何脚本都不应依赖这些路径**。
> 自查与三配置实测：`pwsh -File tools\verify-build-layout.ps1 -Run -IncludeNightly`。
# 3) 校验落地的是新版（见 §1c.3），不需要结束任何进程
```

**下一步**：用户下次自己重启启动器时新版生效。因为默认 `independent`，那次重启
**不会**中断 dsh 会话（会走 `service.json` 对账 + 接管）。

### 1c.3 判定产物真伪：**不要搜索二进制字符串**

本轮踩过这个坑：曾用「在 exe 里搜 `debug_assert` 的文案」判断是不是修复版，
结论是错的 —— **`debug_assert!` 与注释都不会进入 release 产物**。正确做法是
用可执行入口：

```powershell
Remove-Item "$env:APPDATA\DSHLauncher\build-info.txt" -Force -ErrorAction SilentlyContinue
Start-Process .\DSHLauncher.exe -ArgumentList '--build-info' -Wait -WindowStyle Hidden
Get-Content "$env:APPDATA\DSHLauncher\build-info.txt"
# 期望：fixes=...,deadlock-fix-service-start-lock-scope,... 等标记齐全
```

> `--build-info` 是 GUI 子系统进程，`println!` 没有控制台可写，所以它**总是**写一个
> 标记文件；脚本以文件为准。

### 1c.4 预防规则

| # | 规则 | 原因 |
|---|---|---|
| 1 | 发布/验证脚本**禁止**为了覆盖 exe 而强杀启动器 | 强杀会切断会话；改名热替换可以完全避免 |
| 2 | 判定版本用 `--build-info`，不要搜字符串 | `debug_assert!`/注释不进 release 产物 |
| 3 | 热替换后清理历史 `.bak` | 避免根目录堆积多个历史产物 |
| 4 | 需要"新产物立即生效"时，用 `.\DSHLauncher.exe --quit` 优雅退出后再启动 | 有脚本可用的优雅退出入口，无需强杀 |

---

## 2. 依赖安全审计（本机网络限制下的可靠路径）

`cargo audit` 需要 `git clone https://github.com/RustSec/advisory-db.git`。
**本机网络放行 crates.io 与 api.osv.dev，但不达 github.com**，因此该命令会失败：

```text
error: couldn't fetch advisory database: git operation failed: An IO error occurred when talking to the server
```

改用仓库自带脚本（查 OSV，RustSec 数据已镜像进 OSV；覆盖范围等价，只是不走 git 协议）：

```powershell
# 全量 lockfile（264 包）
pwsh -NoProfile -File tools\osv-audit.ps1

# 只扫直接依赖（快）
pwsh -NoProfile -File tools\osv-audit.ps1 -OnlyDirect
```

**结论口径（重要）**：`Cargo.lock` 是 **universal** 的——它为所有平台记录依赖，
不等于「本次构建用到的包」。本项目：

| 范围 | 包数 | 说明 |
|---|---|---|
| `Cargo.lock` 全平台解析 | 264 | 含 Linux/macOS 专用依赖 |
| **Windows 目标实际编译** | **104** | 用 `cargo tree --target x86_64-pc-windows-msvc` 统计 |

当前 3 条 advisory 记录（`glib 0.18.5` ×2、`proc-macro-error 1.0.4`）**全部只存在于 Linux 的
GTK 栈**（`tao` 默认 feature 在 Linux 拉入 `gtk → glib → glib-macros → proc-macro-error`），
Windows 构建图中不存在，已用 `cargo tree -i <crate> --target x86_64-pc-windows-msvc` 验证。

判定是否为「真实影响」的通用流程：

```powershell
# 1) 该包是否在当前目标构建图里？（NOT PRESENT = 不参与构建，不是真实风险）
cargo tree --offline -i <crate> --target x86_64-pc-windows-msvc -e normal,build,dev

# 2) 若存在，看它怎么进来的
cargo tree --offline -i <crate> --target x86_64-pc-windows-msvc
```

> `cargo deny` / `cargo bloat` / `cargo udeps` 同样需要网络或 nightly 工具链，本机未安装；
> 未使用依赖的排查用等价手段完成：对每个依赖做全项目标识符检索（`dirs`/`serde`/`serde_json`/
> `toml`/`wry` 在 `dsh-app` 中命中 0 次 → 已从 `Cargo.toml` 移除）。

---

## 4. 发布流程（v5.0.0 起）

```powershell
cd D:\DSHLauncher

# 一次跑完：环境检查 → 清理残留 → 发布 → 安装包 → fmt/clippy/test
#           → 一致性与版本链路校验 → 内存/启动基线 → 端到端自检 → 依赖审计
pwsh -NoProfile -ExecutionPolicy Bypass -File tools\finish-release.ps1

# 只想快速重建产物
pwsh -NoProfile -ExecutionPolicy Bypass -File build.ps1 release   # 启动器
pwsh -NoProfile -ExecutionPolicy Bypass -File build-setup.ps1     # 安装包
```

报告写入 `docs\FINISH-REPORT.md`。常用开关：

| 开关 | 用途 |
|---|---|
| `-SkipAuditInstalls` | 离线环境跳过 `cargo-audit` / `cargo-machete` 安装 |
| `-SkipE2E` | 跳过 `selftest.ps1`（需要独占单实例互斥体） |
| `-MeasureSeconds N` | 内存采样时长（默认 10 秒） |

### 4.1 发布前必须满足

1. **没有正在运行的启动器**——Windows 不允许覆盖运行中的 exe。
   `build.ps1` 会在拷贝前校验 `target\release\dsh-app.exe` 的版本资源，
   拷贝失败时给出明确提示并**拒绝发布不合格产物**（根目录保留的会是上一版）。
2. `tools/verify-version.ps1` 必须全绿（版本链路：Cargo.toml → exe 资源 → 安装包 → 卸载器）。
3. `tools/check-consistency.ps1` 必须全绿。

### 4.2 已知的「必须两个文件」约定

- 卸载器 = **原生 exe** `dsh-uninstall.exe`（v5.0.0 LTS 起取代脚本卸载器）：无 PowerShell/执行策略依赖，可签名、可带版本资源；开关 `--purge/--silent/--dry-run`（兼容 `/purge` 等 Windows 风格）。
  **不要把逻辑写回批处理**：cmd.exe 的 `%*` 展开、引号内管道、`chcp 65001` 下的非 ASCII
  会让脚本以 `... was unexpected at this time.` 中止，卸载项与安装目录双双残留
  （v5.0.0 前实测如此）。
- 生成的两个文件都以 **ASCII + CRLF + 无 BOM** 写出。

### 4.3 发布到 GitHub（tag + Release；风格必须与既有版本一致）

既有 10 个版本（v1.0.0–v4.2.4）的统一风格：**annotated tag**（消息 `Release version <x.y.z>`）
+ **Release 标题 = tag 名** + **正文 = `docs/RELEASE_NOTES_v<版本>.md` 的内容** +
**只挂一个资源 `DSHLauncherSetup.exe`** + 标记为 Latest。

```powershell
$repo = 'KristoffersonLee/DeepSeek-Harness-Launcher'

# 1) annotated tag（消息风格与旧版一致）
git tag -a v5.0.0 -m "Release version 5.0.0"
git push origin main          # 首次推送"重建后的历史"时必须 --force；之后正常推
git push origin v5.0.0

# 2) Release：正文取发布说明，资源挂安装包
gh release create v5.0.0 --repo $repo --title v5.0.0 --latest `
  --notes-file docs\RELEASE_NOTES_v5.0.0.md DSHLauncherSetup.exe
```

⚠️ **正文里的仓库相对链接必须改写成绝对 URL**：Release 页面没有仓库上下文，
`[CHANGELOG.md](CHANGELOG.md)` 这类链接在 Release 页会 404
（`docs/RELEASE_NOTES_v5.0.0.md` 里就有 4 个）。做法是**只在发布时**生成一份改写副本，
仓库内的文件保持相对链接（符合文档约定）：

```powershell
$base = "https://github.com/$repo/blob/main/docs/"
$note = Get-Content docs\RELEASE_NOTES_v5.0.0.md -Encoding UTF8 -Raw
$fixed = [regex]::Replace($note, '\]\(([A-Za-z0-9._\-]+\.md)\)', ('](' + $base + '$1)'))
[IO.File]::WriteAllText("$env:TEMP\body.md", $fixed, (New-Object Text.UTF8Encoding($false)))
gh release edit v5.0.0 --repo $repo --notes-file "$env:TEMP\body.md"
```

**发布后 30 秒核对**（必须逐条对上，否则说明上传的不是验证过的产物）：

```powershell
gh release view v5.0.0 --repo $repo --json assets -q '.assets[]|{name,size,digest}'
(Get-FileHash .\DSHLauncherSetup.exe -Algorithm SHA256).Hash   # 必须等于上面的 digest（小写）
git ls-remote origin refs/heads/main refs/tags                 # SHA 必须等于本地 git rev-parse
```

> **已发布的 tag 不要移动**：发版之后若有纯文档提交，正常 `git push origin main` 即可，
> 让 main 走在 tag 前面（v5.0.0 即如此：tag 指向交付提交 `557d466`，其后可继续有文档提交）。

---

## 5. 版本号修改 checklist（改版本时只有一处需要改）

1. 改 `Cargo.toml` 的 `[workspace.package] version`。
2. 改 `DSHLauncherSetup.cs` 的 `AppVersion`（与上者必须一致）。
3. 改 `docs/CHANGELOG.md`：新增 `## vX.Y.Z` 段落 + 版本导航锚点。
4. 新增 `docs/RELEASE_NOTES_vX.Y.Z.md`，标题必须是 `# DeepSeek Harness Launcher vX.Y.Z`。
5. `docs/MAINTENANCE.zh.md` / `.en.md` 提及新版本号。
6. 其余全部自动跟随：
   - exe 版本资源 ← `CARGO_PKG_VERSION`（`crates/dsh-app/build.rs`）
   - 安装包版本资源 ← `tools/gen-setup-version.ps1`（从 `Cargo.toml` 解析）
   - 注册表 `DisplayVersion` ← `Program.AppVersion`
7. 跑 `tools\verify-version.ps1` 与 `tools\check-consistency.ps1` 复核。

---

## 6. 回滚与残留处理

- 本项目**不创建备份、快照、分支或标签**（见任务硬性约束），因此不存在代码侧回滚点；
  发布前请确保工作树状态已确认。
- **构建缓存（`target\`）的回收**：它是缓存不是交付物，`cargo` 从不回收旧产物，实测会随
  构建/验证轮次堆到 **3.5 GB**（`debug` 2.6 GB 里 `incremental` 一项就占 1.0 GB）。回收方式：

  ```powershell
  pwsh -NoProfile -File tools\clean.ps1          # 只报告构成（默认不删任何东西）
  pwsh -NoProfile -File tools\clean.ps1 -Cache   # 清缓存并保留 release 产物（推荐，校验链不受影响）
  pwsh -NoProfile -File tools\clean.ps1 -All     # 全清；之后必须先 cargo build --release --locked
                                                 # 再跑 gen-facts -Check（FACTS 记录的是 release 产物）
  ```

  ⚠️ **`-All` 之后不要直接发版**：`docs/FACTS.json` 的 `artifacts.launcher_build` 记录的是
  `target\release\dsh-app.exe` 的字节数，删掉它会让 `tools/gen-facts.ps1 -Check` 报事实漂移。
- 卸载默认**保留** `%APPDATA%\DSHLauncher\settings.toml`；需要彻底清理时用
  `dsh-uninstall.exe --purge`。
- 卸载是"两阶段"的：主进程删完自己的文件后，把自身复制到 `%TEMP%` 并重入，由副本递归删除
  安装目录。因此**命令返回后目录可能仍存在约 1～2 秒**，属于预期行为，不要误判为残留。
- **`%TEMP%` 会留一个约 300 KB 的副本直到重启**：Windows 不允许运行中的镜像删除自己
  （实测对自身路径取 `DELETE` 权限被拒），只能用"下次重启时删除"；每次卸载会顺手清理
  历史遗留的副本。这是平台限制，已写入 README 与 `--help`。
- 升级安装会清理**上一版遗留的载荷**（`ObsoletePayloads`：旧的 `uninstall.cmd`/`uninstall.ps1`），
  避免新旧两套卸载器并存。

- **安装目录已被删除（悬空残留）**：此时三条正规入口全部失效 —— 安装目录里的 `dsh-uninstall.exe`
  随目录消失、源码树里的副本被源码树护栏拒绝（exit 2）、`--deferred-pass <dir>` 因缺少安装标记
  被拒（exit 2），于是「设置 → 应用」的卸载按钮只会报找不到可执行文件。用**残留清理模式**：

  ```powershell
  .\dsh-uninstall.exe --clean-residue --dry-run   # 先看会清什么（零改动）
  .\dsh-uninstall.exe --clean-residue             # 清注册表卸载项 + 桌面快捷方式
  ```

  它**不删除任何文件或目录**（不碰 `%APPDATA%` 用户配置与 `%LOCALAPPDATA%` 运行期数据）；
  准入要求注册表 `InstallLocation` / `UninstallString` / `DisplayIcon` 三者互相印证，且安装标记
  **已不存在**（目录仍完整时明确拒绝，要求走正规卸载）。实测：真实悬空状态下 dry-run 零改动 →
  执行后两处残留被清、`%APPDATA%` 与 `%LOCALAPPDATA%` 文件数逐个不变、重复执行为 no-op。

- **历史与标签（本轮清理后的现状，务必知情）**：v1–v4 的全部内容已从 git 历史中清除 ——
  仓库现在是**单一提交**（`839fca3`），9 个旧 tag（v1.0.0…v4.2.4）已删除，旧对象已 `gc` 回收。含义：

  * 上面那句「本项目不创建备份、快照、分支或标签 ⇒ 不存在代码侧回滚点」已从**约定**变成
    **物理事实**：旧版本内容在本机已不可取回；远端 GitHub 上仍留有一份，
    **在你执行 force-push 之前，那是唯一的退路**；
  * 被清除的 tag → 提交映射、旧文件清单，记录在那个单提交的**提交信息**里（便于日后审计）；
  * 联网后补做（本机 github.com 不可达，实测 `git ls-remote` 在 21s 超时）：

    ```powershell
    git push origin --delete v1.0.0 v2.0.0 v3.0.0 v4.0.0 v4.2.0 v4.2.1 v4.2.2 v4.2.3 v4.2.4
    git push --force origin main
    git fetch --prune --tags origin
    ```

  * 其他机器上的旧克隆仍各自留有旧历史，需要各自重新克隆。

- 发布时若 `D:\DSHLauncher\DSHLauncher.exe` 被运行中的实例锁定，可用**同卷重命名**绕过：
  运行中的映像被以「删除共享」方式锁定——`Rename-Item` 可行，而 `Copy-Item` 覆盖不可行。
  先改名旧产物、再放入新产物即可，**运行中的进程与其托管服务不受影响**。
- 报告与临时产物：`tools/finish-release.ps1` 写 `docs\FINISH-REPORT.md`；
  `target\setup-assemblyinfo.cs` 与 `DSHLauncher.exe.replaced-old` 属可重建/临时文件，不要提交。

---

## 7. 事故记录：批量替换中文/全角字符把源文件截断为 0 字节（v5.0.0 定稿轮，已恢复）

### 7.1 现象

`crates/dsh-app/src/service.rs` 与 `crates/dsh-core/src/process.rs` 在一次批量文本替换后
**内容消失（各剩 2 字节）**，`cargo check` 报大量 `unresolved import`。
两个文件当时都处于**未跟踪状态**，`git` 里没有历史，`git checkout` 无法恢复。

### 7.2 根因（两步叠加，都要记住）

1. **在 `pwsh -Command` 里用含中文/全角字符的字面量做批量替换**：命令行在传递过程中把
   全角括号 `（` 变成了 `v`，于是全文件的 `（` 被替换成了 `v`（第一次只损坏内容，不致命）。
2. **PowerShell 重载解析失败后继续写文件**：`[regex]::Replace($t, $pattern, [char]0xFF08)`
   把 `[char]` 传给了需要 `MatchEvaluator` 的重载 → 抛异常 → 结果变量 `$t2` 保持 `$null` →
   紧接着的 `[IO.File]::WriteAllText($path, $t2)` **把 $null 当空字符串写入**，文件被截断。

### 7.3 恢复步骤（本次实际用过，可复用）

```powershell
# 0) 确认损坏范围（哪些文件 0/2 字节、哪些只是内容被误替换）
Get-ChildItem -Recurse -File -Include *.rs -Path crates | Where-Object { $_.Length -lt 50 } |
    ForEach-Object { "$($_.Length)  $($_.FullName)" }

# 1) 从 DSH 会话记录里取回「我读过的那份完整内容」+「我全部的 edit 调用参数」
#    路径：~/.dsh/sessions/<workspace>/<session-id>/session.v3.jsonl.zstd
#    注意：它是 seekable-zstd（实测 832 个串接帧），用 zstdDecompressSync 只会解出第一帧，
#    必须按帧魔数 28 B5 2F FD 切分逐帧解压（Node 22+/26 的 zlib 即可）。
# 2) 剥掉 read 工具输出里的 `NNN: ` 显示前缀，还原基线文件
# 3) 按记录顺序重放该文件的每一次 edit（old_string/new_string）
#    —— 注意：用 pwsh 做的批量替换**不在** edit 记录里，必须手工补回
# 4) 恢复后立刻跑：cargo fmt --check / clippy -D warnings / cargo test / check-consistency / gen-facts -Check
```

本次恢复结果：`process.rs` 4/4 edit 命中；`service.rs` 17/19 命中（2 处手工补回）；
恢复后 **fmt/clippy/test(97)/一致性(115)/FACTS(29 项事实) 全绿**。

### 7.4 预防规则（必须遵守）

| # | 规则 | 原因 |
|---|---|---|
| 1 | **不要在 `pwsh -Command` 里用含中文/全角字符的字面量做批量替换**；改用 `edit` 工具，或把替换脚本**写成 UTF-8 文件**再执行 | 命令行传递会损坏多字节字符（本次实测把 `（` 变成 `v`） |
| 2 | 批量写回前**必须校验待写内容非空**（`if ([string]::IsNullOrEmpty($t2)) { throw }`） | `[IO.File]::WriteAllText(path, $null)` **静默截断**目标文件，不报错 |
| 3 | 批量改写前先 `Copy-Item` 一份到仓库外（`%TEMP%`） | 未入库的目录没有 `git` 兜底，本次能恢复纯属侥幸（会话记录恰好保留了完整快照） |
| 4 | 优先把未跟踪的源码 `git add` | 有历史才有回滚；当前 `crates/`、`ui/` 与部分 `docs/`、`tools/` 仍未入库 |
| 5 | 恢复脚本一律放 `%TEMP%`，用完即删 | 仓库里不得残留一次性分析脚本（本次已确认无残留） |
| 6 | 每次大范围改写后立刻跑 Rust 三件套 + `check-consistency` | 内容类损坏（全角→ASCII、注释被吞）**编译不会报错**，只有断言才发现 |

### 7.5 事故记录：在仓库根目录跑卸载器 → 源码树产物被删（v5.0.0 定稿轮实测，已恢复）

**现象**：用户在 `D:\DSHLauncher` 下执行卸载步骤后，根目录的
`DSHLauncher.exe` / `dsh-uninstall.exe` / `app.ico` / `README.md` / 维护手册 全部消失，
`%LOCALAPPDATA%\DSHLauncher`（日志 + WebView2 profile + `service.json`）也被删除。

**根因**：
1. 模板 `uninstall.ps1` 的"所有权证明"是「目录里有 `DSHLauncher.exe`」——
   **仓库根目录恰好满足**（`build.ps1` 会把产物放到根目录），于是模板把源码树当成了安装副本；
2. 运维清单把它写成了 `.\uninstall.cmd`（仓库根路径），等于引导用户踩坑。

**修复**：模板现在**必须**同时满足「存在 `.dsllauncher-install` 标记」且
「没有 `Cargo.toml`/`crates`」才允许删除任何东西；否则打印说明并 `exit 1`。
验证安装/卸载时**只对安装目录里的副本操作**：
`& "$env:LOCALAPPDATA\Programs\DSHLauncher\dsh-uninstall.exe"`。

**预防规则（追加）**：

| # | 规则 | 原因 |
|---|---|---|
| 7 | 卸载/清理类脚本必须先证明"我面对的是安装副本"，**不能只看目录里有没有我们的 exe** | 仓库根目录同样有我们的 exe |
| 8 | 验证安装/卸载时，命令的目标路径必须是**安装目录**，不要把仓库根当安装目录 | 一次误操作会删掉 5 个源文件 + 全部运行期数据 |
| 9 | `build.rs` 这类构建脚本对"可选资产"的 `rerun-if-changed` 必须**无条件声明**，缺失则直接让构建失败 | 实测：图标缺失时构建过一次后，图标补回来也不会重跑 build.rs → exe 静默丢掉图标资源（−22 KB） |

### 7.6 本轮其他操作记录
- **发布方式**：全程使用 `build.ps1 release` 的**热替换**（`Rename-Item` 让位 → `Copy-Item` 放入），
  本轮多次重发布**未结束任何进程**，活动会话未中断。
- **未执行的破坏性验证**（按用户要求）：`selftest.ps1`、`verify-service-lifecycle.ps1 -Force`、
  安装/卸载互逆矩阵 —— 命令清单见 `docs/IMPLEMENTATION-v5.0.0.md` §7。
- **`osv-audit.ps1` 语义已改**：不再"见 advisory 即红"，而是逐包用
  `cargo tree -i <crate> --target x86_64-pc-windows-msvc` 复核是否进入 Windows 构建图；
  当前 3 条记录（glib×2、proc-macro-error）全在图外 → `exit 0`。
  **改 GUI crate 的 feature 时必须重跑**：一旦某个包进入 Windows 图，它会立刻变红。

### 7.7 事故记录：清理"旧版残留"时误删 make-icon 的数据源（已完整恢复）

- **现象**：清理仓库里的 v1–v4 遗留资产时删掉了 `assets/whale-path.txt`（依据是"全库零引用"）。
  随后 `tools/finish-release.ps1` 的第 2、3 步报 `build.ps1 退出码 1` / `build-setup.ps1 退出码 1`，
  而该脚本只记录退出码、**拿不到子进程的输出**，所以现场看不出原因。
- **根因（两处叠加）**：
  1. **判据错**：核查引用时用了 `Select-String -SimpleMatch -Pattern 'assets|whale'`。
     `-SimpleMatch` 是**字面串**匹配，`|` 不会被当作交替符 —— 实际是在搜 `assets|whale`
     这个字符串本身，必然零命中，于是得到"零引用"的**假阴性**。真实情况是
     `make-icon.ps1` 第 13–14 行把它当作**唯一数据源**（缺失即 `throw`），而 `build.ps1`
     第一步就是调用 `make-icon.ps1`。
  2. **删除前没有跑门禁**：门禁当时**没有**断言该文件存在，因此删完没有任何红灯。
- **恢复步骤（两条经验都可复用）**：
  1. 文件已随历史重写被 `git gc` 回收、远端又不可达 ⇒ 从 **DSH 会话记录**恢复：
     `%USERPROFILE%\.dsh\sessions\<workspace>\<session>\session.v3.jsonl.zstd` 是
     seekable-zstd（**串接帧**）；按帧魔数 `28 B5 2F FD` 切分后逐帧调用
     `zlib.zstdDecompressSync`（Node 22+/26 内置，直接 `zstdDecompressSync` 只能解第一帧），
     再从解出的 JSONL 里取出该行原文。
  2. **用确定性产物反向验证**：`make-icon.ps1` 对同一输入必然产出同样的 `app.ico`，
     因此判据是"用恢复的原文重建 `app.ico`，与现存文件 **SHA256 相同**"——
     这等价于证明恢复内容与原始文件**逐字节一致**（本次实测：3450 B，SHA256 `A2EFB5AF…` 一致）。
- **预防规则（已落地）**：
  * 门禁新增断言：`assets/whale-path.txt` 必须存在（`make-icon.ps1` 的唯一输入）；
  * **禁止用 `-SimpleMatch` 核查"是否存在引用"** —— 它不做正则交替，会静默给出假阴性；
    改用默认正则 `-Pattern 'a|b'` 或 `-Pattern @('a','b')`；
  * 删除任何"疑似无用"的文件之前，**先跑一遍门禁**：本轮的漏网正是因为它当时没覆盖该文件。

---

## 8. 工具链契约：钉死的 nightly（build-dir Layout v2）与回退到 stable

### 8.1 现状（唯一来源：`rust-toolchain.toml`）

| 项 | 值 |
|---|---|
| 主工具链 | `nightly-2026-09-10`：`rustc 1.100.0-nightly (a36d05efa 2026-09-09)` / `cargo 1.100.0-nightly` |
| 组件 | `clippy` + `rustfmt`（门禁需要；`profile = "minimal"` 只装必要部分） |
| 为什么是 nightly | Cargo 的 **build-dir 新布局（Layout v2）目前只在 nightly 提供** —— stable 1.98.1（当前最新 stable）仍是旧布局（`deps\` + `.fingerprint\`）。要"完整应用 v2"，主工具链就只能是 nightly |
| 为什么钉死**日期** | nightly 每天前进；工具链版本会改变产物字节、`rustfmt` 的输出与 `clippy` 的规则集（直接影响 `-D warnings` 门禁）。固定日期，`cargo build` 才是可复现的 |
| 生效判定 | `rustup show active-toolchain` 必须显示 `... (overridden by 'D:\DSHLauncher\rust-toolchain.toml')` |
| 布局判定 | `target\<profile>\` 顶层**没有** `deps\` 与 `.fingerprint\`；中间产物在 `target\<profile>\build\<包名>\<哈希>\{out,fingerprint}\` |

### 8.2 回退到 stable（一条命令）

```powershell
Remove-Item rust-toolchain.toml                # 之后 cargo 立刻回到 stable 1.98.1（旧布局）
pwsh -NoProfile -File tools\clean.ps1 -Cache   # 布局切换会让旧缓存失效，顺手回收
```

回退**不需要改任何源码**：仓库里没有任何脚本依赖构建目录的内部布局（`check-consistency`
与 `verify-build-layout.ps1` 会持续强制这一点）。单个命令也可以临时覆盖，例如
`cargo +stable build --release --offline`（覆盖机制已实测生效）。

### 8.3 升级钉死的版本（每轮升级必须做的四步）

```powershell
rustup toolchain install nightly-YYYY-MM-DD --profile minimal --component clippy,rustfmt
# 1) 改 rust-toolchain.toml 的 channel
# 2) cargo fmt --all -- --check       # 新 rustfmt 可能要求重新格式化 → 有 diff 就 cargo fmt --all
# 3) cargo clippy --workspace --all-targets --all-features -- -D warnings   # 新 clippy 可能引入新 lint
# 4) cargo test / tools\check-consistency.ps1 / tools\gen-facts.ps1 -Check / selftest.ps1
```

**为什么第 2、3 步不能省**：工具链升级最典型的两种"沉默失败"就是格式化漂移与新 lint。
它们放着不管，门禁会在下一轮才变红 —— 那时改动已经混在一起，很难归因。

### 8.4 与上游稳定化的关系

v2 的稳定化仍在上游推进（issue/PR 见 [`CFT-FEEDBACK.md`](CFT-FEEDBACK.md)）。一旦它在某个
**stable** 版本落地，本仓库应当把 `rust-toolchain.toml` 换成那个 stable（或直接删除该文件、
回到默认 stable），并重跑 §8.3 的四步。届时"必须用 nightly"的唯一理由就消失了 ——
这一点写在这里，免得后来者以为它是本项目的固有设计。
