# 升级与维护手册（通用版）

> 本文档为 **DSHLauncher** 仓库的维护手册（独立成文，随项目发布；安装包会把它部署到安装目录）。
> 指导 **dsh**（`@deepseek-ai/dsh`，DeepSeek Harness 命令行/服务）与 **DSHLauncher** 的**安装、升级、验证、故障恢复**。
> 正文为**通用步骤**（不依赖具体电脑），文末附录保留**发布者机器的环境快照与升级历史**，供对照参考。
> 官方 dsh 仍处快速迭代（预览/rc/alpha），可能引入**破坏性更新**（端口/协议/接口、配置格式、工作目录等）。升级前务必阅读本手册第 3、4、7 节。
> **启动器 v4.1.0 起已内置「修复模块」按钮和健康检查**，遇到原生模块问题时可优先使用启动器内置功能。
>
> ⚠️ **v5.0.0 LTS 起启动器已用 Rust 全量重写**（详见 [`RELEASE_NOTES_v5.0.0.md`](RELEASE_NOTES_v5.0.0.md) 与 [`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md)）。与手册相关的变化：
> - **局域网共享已彻底移除**（相关章节与故障条目不再适用）；
> - 运行时依赖由 .NET Framework + 旁挂 DLL 改为**仅 WebView2 Runtime**，产物为**单文件 exe**；
> - 构建脚本由 `csc.exe` 改为 `cargo build --offline`；
> - **「修复模块」按钮在 v5 已不再提供**（其 `dsh-core` 侧的升级/健康检查 API 亦作为死代码删除）——原生模块问题请按本手册 §1.5 的 `node-gyp rebuild` 手动步骤处理；
> - 新增**失联自愈**与**孤儿锁自动恢复**（见 §1.6）。
> - **发布标签为 5.0.0 LTS**：核对版本用 `DSHLauncher.exe --version`（输出 `DSHLauncher 5.0.0 LTS (release)`），
>   查看全部开关与退出码用 `--help`；Cargo 版本仍是合法 semver `5.0.0`（`LTS` 不写进版本号）。

## 手册目录

1. [概述](#11-概述)
2. [环境要求与路径约定](#12-环境要求与路径约定)
3. [dsh 安装与升级](#13-dsh-安装与升级)
4. [升级后验证清单](#14-升级后验证清单)
5. [常见故障与修复](#15-常见故障与修复)
6. [启动器行为说明](#16-启动器行为说明)
7. [维护规范与防坑规则](#17-维护规范与防坑规则)
8. [附录 A：发布者本机环境快照（2026-09-01）](#18-附录-a发布者本机环境快照2026-09-01)
9. [附录 B：版本跟踪与破坏性变更速查](#19-附录-b版本跟踪与破坏性变更速查)
10. [附录 C：发布者本机升级历史](#110-附录-c发布者本机升级历史)

### 1.1 概述

- **dsh**：DeepSeek Harness 的 Node.js 服务与命令行。Web 界面默认监听 `http://127.0.0.1:3080/`。通过 npm 全局安装（`@deepseek-ai/dsh`）。
- **DSHLauncher**：Windows 桌面壳。双击即自动启动 `dsh web`，用**内嵌 WebView2 窗口**显示 Harness（无需浏览器），并提供托盘、设置、日志、自动接管、自愈等能力。

### 1.2 环境要求与路径约定

**环境要求**

| 组件 | 要求 |
|---|---|
| 操作系统 | Windows 10 / 11（64 位） |
| Node.js | 官方 LTS 或更新（DSHLauncher 使用系统 Node 启动 dsh） |
| npm | 官方 latest；**注意 npm 12 起默认阻止未白名单包的 install/postinstall 脚本**（见第 1.7 节防坑） |
| WebView2 运行时 | 一般随 Edge 自带；缺失时启动器自动部署或回退 Edge |

**路径约定（通用）**

| 用途 | 路径（通用写法） | 说明 |
|---|---|---|
| npm 全局前缀 | `npm config get prefix` 查询（Windows 通常 `%APPDATA%\npm`） | dsh 装在此前缀的 `node_modules\@deepseek-ai\dsh` |
| dsh 用户数据 | `%USERPROFILE%\.dsh\` | 会话（`sessions\`）、配置（`settings.yaml`）、凭据（`.credentials.yaml`）、插件（`profiles\`） |
| 启动器日志 | `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` | 排障第一入口 |
| 启动器设置 | `%APPDATA%\DSHLauncher\settings.ini` | 端口/工作目录/托盘行为 |
| Web 界面 | `http://127.0.0.1:3080/` | 默认端口；**0.1.2-alpha 起需带一次性 token**（见第 1.6 节） |

> 术语约定：`<npm-prefix>` 指 `npm config get prefix` 的输出；`<dsh-version>` 指目标版本号。

### 1.3 dsh 安装与升级

> 可由 AI Agent 依据本手册自动执行，也可人工按下列命令操作。两种方式等价。

**确定目标版本（必须先做）**

```powershell
npm view @deepseek-ai/dsh dist-tags --json   # 查看 latest / next / alpha 标签
npm view @deepseek-ai/dsh versions --json    # 查看全部已发布版本
```

- GitHub 发布页（`deepseek-ai/deepseek-harness` releases）可能先于 npm 上架；**以 npm 实际可安装为准**。
- alpha/rc 为预发布：升级前阅读对应版本发布说明，评估**破坏性变更**（速查见附录 B）。

**安装 / 升级命令**

```powershell
# 0) 确认 npm 前缀（必须与启动器使用的全局前缀一致）
npm config get prefix

# 1) 安装指定版本（显式版本号 + 防损坏缓存 + 放行原生构建脚本，勿省略）
npm install -g "@deepseek-ai/dsh@<dsh-version>" --no-audit --no-fund --prefer-online --registry=https://registry.npmjs.org/ `
  --allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
```

| 参数 | 作用 |
|---|---|
| `@deepseek-ai/dsh@<版本>` | **必须显式写版本号**，避免装回旧标签指向的版本 |
| `--prefer-online` | 绕过本地已损坏的 npm 缓存 tarball |
| `--registry=https://registry.npmjs.org/` | **强制官方 registry**，不受本地 `.npmrc` 镜像源影响，确保版本查询和安装一致 |
| `--allow-scripts=...` | **npm 12 必需**：放行 fs-ext（文件锁）、koffi（FFI）、node-pty（终端）等原生模块构建脚本，否则装出「半成品」（第 1.5 节故障 B/G） |
| `--no-audit --no-fund` | 提速、免打扰 |

若前缀/权限异常：用系统 npm 显式指定前缀，例如
`"C:\Program Files\nodejs\npm.cmd" install -g "@deepseek-ai/dsh@<版本>" --prefix "<npm-prefix>" --no-audit --no-fund --prefer-online --allow-scripts=...`。
若默认 npm-cache 报 EPERM，追加 `--cache <可写目录>`。

**安装前准备（必须）**

- **必须先停止 dsh web 服务**，否则运行中的进程会锁定文件（如 `koffi.node`），导致安装失败（EBUSY）或产生半成品目录：
  ```powershell
  # 查找运行中的 dsh 进程
  Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' }
  # 停止（如果有）
  Stop-Process -Id <PID> -Force
  ```
- 让安装**完整跑完**（约 3–5 分钟），**不要中途 kill、不要同时杀进程**（曾因中断导致残留 worker 死锁与半成品目录）。
- 升级只改磁盘文件，运行中的 dsh web 不受影响；**升级后需重启启动器**才加载新版本。

**⚠️ 特殊情况：通过 DSH 内的 AI Agent 远程升级时**

当用户通过 DSH Web 界面与 AI Agent（如本 Agent）对话，要求 Agent 执行 dsh 升级时，会出现一个矛盾：
- Agent 运行在 dsh 进程内部
- 按手册要求，升级前必须先停止 dsh 进程
- 停止 dsh = 中断 Agent 自身的对话通道

**解决方法：**
1. Agent **不应自行执行** `Stop-Process` 停止 dsh，而应输出完整的升级命令供用户在外部终端手动执行。
2. 用户在 PowerShell / cmd 中运行停止 → 安装 → 验证 → 重启流程。
3. 升级完成后，Agent 可通过新版本的 dsh 继续对话。

示例输出模板：
```
由于我运行在 DSH 内部，无法停止自身进程。请在终端中手动执行：

# 1. 停止 dsh
Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force

# 2. 安装新版本
npm install -g "@deepseek-ai/dsh@<版本>" --no-audit --no-fund --prefer-online --registry=https://registry.npmjs.org/ --allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs

# 3. 验证
dsh --version

# 4. 重启 dsh
dsh web
```

**安装后清理**

- 安装失败后可能在 `%TEMP%` 残留 `dsh-broken-*`、`dsh-partial-*`、`dsh-spill-*`、`dsh-subprocess-*` 等目录（每个可达数百 MB），应手动清理：
  ```powershell
  Get-ChildItem "$env:TEMP" -Directory | Where-Object { $_.Name -like 'dsh-*' } | Remove-Item -Recurse -Force
  ```
- 安装失败后在 npm 缓存中可能残留损坏的 tarball，下次安装应加 `--prefer-online` 绕过缓存。

### 1.4 升级后验证清单

安装/重装后**逐项核验**，全部通过才算升级成功：

> ⚠️ **版本适配（0.1.5-alpha.1 起）**：fs-ext 是 0.1.3-alpha.2 引入的原生模块，**0.1.5-alpha.1 起被官方移除**（会话文件锁改用 `@deepseek-ai/node-addon-system`：POSIX `flock(2)` / Windows 内核信号量，无需本地编译）。目标版本 ≥ 0.1.5-alpha.1 时，下方第 4 项及表中 fs-ext 行**跳过不核验**——报 `Cannot find module 'fs-ext'` 属正常，勿按故障 G 处理；仅 0.1.3-alpha.2～0.1.4 需要核验 fs-ext。

```powershell
# 1) 版本
dsh --version

# 2) 用法（触发 bin.js → dsh-app-boot → commander/js-yaml 加载链）
dsh --help

# 3) 插件配置树（验证 YAML 解析与整棵插件树可加载，正常约 500+ 行，无 error/mismatch）
dsh --profile web --dump-config

# 4) fs-ext 原生模块可加载（关键！v4.1.0 起启动器内置健康检查会自动探测）
node -e "const f=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/fs-ext'); console.log(typeof f.flock)"

# 5) koffi 原生版本匹配（关键！）
node -e "const k=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"

# 6) node-pty 可加载
node -e "const p=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/node-pty'); console.log(typeof p.spawn)"
```

| 检查项 | 期望 |
|---|---|
| `dsh --version` | 与安装版本一致 |
| fs-ext | 输出 `function`；报 `Cannot find module 'fs_ext'` 即编译缺失 |
| koffi | 输出版本与 JS 包装一致（如 `3.1.6`）；报 `Mismatched native Koffi modules` 即安装损坏 |
| node-pty | 输出 `function` |
| `dump-config` | 无 `error` / `mismatch` / `failed to` |
| 关键文件 | `<npm-prefix>\node_modules\@deepseek-ai\dsh\` 下：`package.json`、`lib\bin.js`、`node_modules\commander\index.js`、`node_modules\js-yaml\dist\js-yaml.mjs`、`node_modules\fs-ext\build\Release\fs_ext.node`、`node_modules\@koromix\koffi-win32-x64\win32_x64\koffi.node` |
| 残留目录 | `<npm-prefix>\node_modules\@deepseek-ai\` 下正常应只有 `dsh`（见第 1.5 节故障 E） |
| Web 界面 | 重启启动器后内嵌窗正常显示（0.1.2-alpha 起带 token，见第 1.6 节） |

### 1.5 常见故障与修复

**故障 A：模块缺失（js-yaml / commander 文件缺失）**

- **现象**：`dsh` 命令报模块不存在；启动器无法启动。
- **根因**：安装被中断或装错前缀，留下半成品目录。
- **修复**：删除安装目录后完整重装（第 1.3 节），等它跑完；装错前缀的按 1.3 节显式 `--prefix` 重装。

**故障 B：koffi 原生二进制错位（Mismatched native Koffi modules）**

- **现象**：启动器加载 `subprocess`/`sandbox` 插件时抛 `Mismatched native Koffi modules`，退出码 1，表现为「启动器打不开」。
- **根因**：npm 12 的 allow-scripts 策略拦截了 koffi 等构建脚本 → JS 与原生二进制版本错位。
- **修复**：用 `--allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` 完整重装（第 1.3 节），再按第 1.4 节核验 koffi 版本。**不要试图只替换单个 .node 文件**。
- **启动器内置修复**：v4.1.0 起可在升级页点击「修复模块」按钮一键重编。

**故障 C：会话编码不匹配（uses .jsonl, but this backend is configured for compression "zstd"）**

- **现象**：启动器日志报 `encodingMismatch`，退出码 1；`dsh --version` / `dump-config` 正常。
- **根因**：0.1.1-rc.2 起会话后端默认 `compression: zstd`；若某个会话目录**混编码**（明文 `session.jsonl` 与 `session.jsonl.zstd` 并存），后端初始化即崩。**重装无效**（默认仍是 zstd）。
- **修复（统一 root 为 zstd）**：
  1. 定位冲突：在 `%USERPROFILE%\.dsh\sessions` 下统计明文与 zstd 数量；
  2. 对每个冲突目录：先解码 zstd 首帧确认会话 id 一致且含完整事件（证明 zstd 为权威数据），**将明文 `session.jsonl` 备份到 `.dsh` 目录之外**，然后删除明文，仅保留 `session.jsonl.zstd`；
  3. ⚠️ **备份切勿放在 `.dsh` 内部**（dsh 会把它当会话 root 扫描，明文备份再次触发同一崩溃）；
  4. 确认 `.dsh\sessions` 下明文数为 0，重启启动器。
- **备选**：若全部会话均为明文且想保留明文，可在 `%USERPROFILE%\.dsh\profiles\web\cordis.patch.yml` 追加 `- id: session-persistence-jsonl / config: { compression: none }`（仅当无任何 zstd 会话时可用）。

**故障 D：0.1.2-alpha 起 Web 界面要求一次性 token 认证（401）**

- **现象**：直接访问 `http://127.0.0.1:3080/` 返回 **401**；启动器内嵌窗显示 `dsh web authentication required; reopen the URL printed by dsh web`；日志可见 `dsh web: http://127.0.0.1:3080/?token=…`（每次启动不同）。
- **根因**：0.1.2-alpha 起 `dsh-client-connection` 对 Web 界面强制**一次性 token 认证**（每次启动生成新 token，访问一次后换取 30 天有效的浏览器会话 cookie），**无配置关闭开关**。
- **修复**：使用 `dsh web` 打印的带 token URL；**新版 DSHLauncher（v2.0 起）已自动捕获该 URL 并导航**，无需手工处理。旧版启动器请升级。
- **要点**：token 一次性、每次启动不同；浏览器会话 cookie 有效期默认 30 天，期间重开启动器自动接管（见第 1.6 节）无需重新认证；cookie 过期后 401 时重启一次服务即可。

**故障 E：启动器打不开 / 无响应**

1. 停止启动器，确认崩溃进程已退出（`Get-NetTCPConnection -LocalPort 3080`）；用 `Get-CimInstance Win32_Process` 看命令行，**勿误杀其它 node 进程**（如其它工具的 MCP/agent）。
2. 清理残留临时目录：
   ```powershell
   Get-ChildItem "<npm-prefix>\node_modules\@deepseek-ai" -Force   # 正常应只有 dsh
   Remove-Item "<残留目录路径>" -Recurse -Force                    # 如 .dsh-*（含 sharp DLL）
   ```
   被运行中进程锁定的残留，**重启启动器后即可删**。
3. 删除损坏安装（`Remove-Item "<npm-prefix>\node_modules\@deepseek-ai\dsh" -Recurse -Force`）后按第 1.3 节完整重装，按第 1.4 节核验，再重启启动器。

**故障 EBUSY：安装失败（文件被锁定）**

- **现象**：`npm install -g @deepseek-ai/dsh@<版本>` 报 `EBUSY: resource busy or locked` 或 `EEXIST: file already exists`，安装中断。
- **根因**：运行中的 dsh web 进程锁定了 `koffi.node` 等文件，npm 无法覆盖。
- **修复**：
  1. **先停止 dsh 进程**：
     ```powershell
     Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force
     ```
  2. 清理残留目录（`%TEMP%\dsh-broken-*`、`dsh-partial-*`、`dsh-spill-*`、`dsh-subprocess-*`）。
  3. 重新安装（加 `--prefer-online` 绕过缓存）：
     ```powershell
     npm install -g "@deepseek-ai/dsh@<版本>" --no-audit --no-fund --prefer-online --registry=https://registry.npmjs.org/ --allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
     ```
  4. 按第 1.4 节验证清单核验。

**故障 G：fs-ext 原生模块缺失（Cannot find module 'fs_ext'）——仅适用 0.1.3-alpha.2～0.1.4；0.1.5-alpha.1 起官方已移除 fs-ext，报该错误说明装错版本或安装损坏**

- **现象**：启动 dsh web 报 `Cannot find module 'fs_ext'` 或 `Module did not self-register: '...fs_ext.node'`；启动器日志可见 `Error: Cannot find module 'fs_ext'`。
- **根因（三层叠加）**：
  1. **编译从未发生**：npm 11.19.0+ 的 allow-scripts 白名单机制拦截了 fs-ext 的编译脚本 → `build/Release/fs_ext.node` 根本不存在。
  2. **ABI 不匹配**：重编时 PATH 中找到的 Node 版本与 dsh 实际运行的 Node 版本不同（如 Node 22 编出 ABI 127，dsh 跑在 Node 26 ABI 147 下）→ 无法加载。
  3. **Node 26 thin-LTO 编译坑**：Node 26 的 `common.gypi` 默认对 Windows 开启 clang 风格 thin-LTO（`-flto=thin`），MSVC 链接报 `LNK1117`。
- **修复**：
  1. **启动器内置修复**（v4.1.0 起）：升级页点击「修复模块」按钮，自动定位 npm 全局目录并 `node-gyp rebuild`。
  2. **手动修复**：
     ```powershell
     # 定位 fs-ext 目录
     $fsExt = "$(npm root -g)\node_modules\@deepseek-ai\dsh\node_modules\fs-ext"
     # 用 dsh 实际使用的 Node 版本重编
     cd $fsExt
     node-gyp rebuild
     ```
  3. **若仍报 LNK1117**：需修改 Node 26 头文件缓存（`%LOCALAPPDATA%\node-gyp\Cache\26.7.0\...\common.gypi`）中的 `enable_thin_lto=="true"` 条件后重编。
- **预防**：启动器 v4.1.0 起已在 `npm install -g` 命令中加入 `fs-ext` 到 allow-scripts 白名单，防止今后安装再次漏编译。

**故障 F：常见误报（不是问题）**

| 现象 | 说明 |
|---|---|
| `npm ls -g` 里 `UNMET OPTIONAL DEPENDENCY @img/sharp-*`（darwin/linux/freebsd） | Windows 本就不装，正常 |
| `EPERM` 写 `cordis.yml` | 通常是有另一实例占用端口/沙箱限制，非配置损坏 |
| `atomic-write: timed out waiting for the writer lock at …\.dsh\*.lock` | 上次 dsh 被**强杀**留下的孤儿锁（锁文件内容即持有者 PID）。**v5 启动器会在启动服务前自动清理**（仅当该 PID 确已退出，活跃锁绝不触碰）；手动处理见 §1.5 |
| `npm warn cleanup Failed to remove .dsh-*` | 临时目录被运行中进程锁定，重启后可删 |

### 1.6 启动器行为说明

> 以下为 **v5.0.0（Rust）** 的行为；与 v4 的差异已标注。

- **内嵌窗口**：WebView2 渲染 Harness，无需浏览器；不可用时自动回退 Edge 精简窗口，再回退默认浏览器。
- **托盘菜单**：打开界面 / 刷新 / 浏览器打开 / 启动服务 / 停止服务 / 新手指引 / 日志目录 / 设置 / 关于 / 退出。
- **token 认证适配（v2.0 起）**：启动器捕获 `dsh web` 输出行中的 `/?token=…` 并导航到带 token 地址；
  旧版本 dsh 无此输出、或接管**外部已启动**的实例时，回退到普通地址（依赖已持久化的登录 Cookie）。
- **服务生命周期（v2.0 起；v5.0.0 定稿轮语义变更，重要）**：
  - **默认「服务独立于启动器」**（`settings.toml` 的 `service_lifecycle = "independent"`）：
    dsh **不挂在启动器的 Job Object 上**，因此退出 / 崩溃 / 被安装包升级覆盖 / 注销重启
    **都不会**中断正在进行的会话；下次打开启动器通过 `%LOCALAPPDATA%\DSHLauncher\service.json`
    对账后**自动接管**。
  - 若需要「启动器一死就回收一切」的旧语义：设置页取消勾选「服务独立于启动器」
    （即 `service_lifecycle = "tied"`），此时 dsh 挂在 Job 上，强杀启动器由内核一并回收。
  - 需停止服务：托盘「停止服务」，或退出提示时选「是」；
  - 关闭功能窗口（或按 `Esc`）按设置页「关闭窗口时最小化到托盘」偏好处理：
    勾选时**隐藏**窗口（保留页面状态，托盘可秒开），未勾选时真正关闭该窗口；
  - 接管依赖浏览器会话 cookie（默认 30 天有效）；cookie 过期后重开若遇 401，重启一次服务即可。
- **孤儿进程策略（v5.0.0 定稿轮变更）**：默认 `independent` 下，**启动器退出不会回收 dsh**
  （这正是「退出不中断会话」的实现方式）。归属由 `service.json`（PID + 端口 + **进程创建时间**）
  记录并在下次启动时校验：创建时间比对用于防 PID 复用误接管。
  若要「内核级零残留」，把 `service_lifecycle` 改为 `tied`。
  - 若启动器自身是 dsh 的父进程，任务管理器可见层级；
  - 启动时若发现「有簿记但已不在监听端口」的残留（启动到一半被杀），按
    `stop_stale_orphan`（默认 `true`）结束它 —— 这类进程没有任何活跃会话。
- **失联自愈与自动重连（v5 新增）**：运行中每 1.5 秒探测；约 12 秒无响应判定失联并写日志。
  - **自有服务**退出 → 自动重启（最多 3 次）；
  - **接管的外部实例**退出（例如其 CMD 窗口被关闭——Windows 会向该控制台的所有进程发送
    `CTRL_CLOSE_EVENT`）→ 无法代为重启，但**持续看守该端口**；一旦重新出现服务即自动重新接管并让界面重连。
- **孤儿锁自动恢复（v5 新增）**：dsh 用 `wx` 排他创建的 `<file>.lock` 串行化写入，进程被强杀时锁会残留，
  导致下次启动报 `atomic-write: timed out waiting for the writer lock`。启动器在启动服务前读取锁文件中的
  **持有者 PID**，仅当该进程确实已退出才清理；内容不可解析时要求文件静置 5 秒（避开"创建后尚未写入"的竞态）。
- **配置**：`%APPDATA%\DSHLauncher\settings.toml`（TOML，强类型校验）。
  v4 的 `settings.ini` 会在首次启动时**自动迁移**（仅端口 / 工作目录 / Node 路径 / 托盘选项）并落盘；
  配置损坏时启动器会弹窗提示（含文件路径），不再静默降级。
- **日志**：`%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`。
  v5 改为 **append-only 滚动**（单文件 2 MB × 3 份），不再像 v4 那样超过阈值后每写一行都全量重写；
  日志会自动剥离 `?token=xxx`，凭据不落盘。
- **内置 WebView2 数据目录**：`%LOCALAPPDATA%\DSHLauncher\webview2-profile`
  （v5 显式指定，避免在 exe 旁生成 `<exe>.WebView2\`——那会破坏单文件分发，且在只读安装目录下会直接失败）。
- **命令行参数**：`--selftest`（隐藏自检：启动 → 就绪 → 停止）· `--settings`/`-s`（启动时打开设置）·
  `--guide`/`-g`（新手指引）· `--ipc-probe`（自检用：模拟一次设置页按钮点击，验证 IPC 往返）·
  `--quit`（**v5.0.0 定稿轮新增**：请求已在运行的实例优雅退出；发布/自动化脚本用它替代强杀，
  因为强杀会切断正在进行的会话）。
- **快捷键（v5.0.0 定稿轮起，内嵌窗口内）**：`F5` / `Ctrl+R` 刷新界面；`Esc` 按「关闭到托盘」偏好隐藏窗口。
- **标题栏跟随主题（v5.0.0 定稿轮起）**：每约 3 秒异步采样页面背景色并着色 DWM 标题栏（浅/深自动适配），
  采样在回调线程完成，不阻塞界面。
- **崩溃取证（v5.0.0 定稿轮起）**：release 使用 `panic = "abort"`，因此注册了
  `SetUnhandledExceptionFilter`：崩溃时在 `launcher.log` 留下一行
  `[FATAL] 未处理异常 code=0x… address=0x… pid=… tid=…`（无内存分配）。
- **v5 不提供（v4 有）**：设置页「修复模块」按钮（v5.0.0 LTS 起连 `dsh-core` 侧的升级/健康检查 API 也已删除，属有意的功能收缩）、设置窗口位置记忆。
  （重复启动**唤起已有窗口**已在 v5.0.0 实现：命名事件 `Local\DSHLauncher_Activate_v5`。）

### 1.7 维护规范与防坑规则

1. **全局 npm 前缀必须与启动器使用的前缀一致**。执行前 `npm config get prefix` 确认；多 Node 环境（如其它工具的受管 Node）下 `npm` 可能指向不同前缀导致装错位置——用系统 npm + 显式 `--prefix` 最稳。
2. **升级前必须停止 dsh 服务**（故障 EBUSY 根因）：运行中的进程会锁定 `koffi.node` 等文件，导致安装失败或产生半成品目录。执行 `Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force` 停止。
3. **npm 12 的 allow-scripts 策略会静默破坏原生模块**（故障 B/G 根因）：升级 dsh 必须带 `--allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs`，装完立即核验 koffi 和 fs-ext 版本。启动器 v4.1.0–v4.2.4 内置「修复模块」按钮可一键重编；**v5.0.0 不再提供该按钮**，其 `dsh-core` 侧的升级/健康检查 API（`version` / `dist_tags` / `upgrade` / `check_health`）也已作为**无调用方的死代码彻底删除**（v5.0.0 LTS 定稿轮），请按 §1.5 的**手动** `node-gyp rebuild` 步骤处理。
4. **升级/重装后必须核验完整性**（第 1.4 节清单），尤其是 koffi 原生版本与 `dump-config`。
5. **不要一边装一边杀进程**；让安装完整跑完，确需中断时先确认残留进程。
6. **升级前评估破坏性变更**：阅读目标版本发布说明（速查见附录 B）；协议/配置变更（如 token 认证、APIProxy 移除）可能影响启动器与模型配置。
7. **版本显式化**：安装/升级必须写显式版本号，避免标签漂移。
8. **凭据安全**：API Key 存放于 `%USERPROFILE%\.dsh\.credentials.yaml`，**不要提交到仓库或写入文档**；升级 dsh 不改变凭据。
9. **安装失败后的清理**：检查 `%TEMP%` 下的 `dsh-*` 残留目录并删除（每个可达数百 MB）；下次安装加 `--prefer-online` 绕过损坏的 npm 缓存。

### 1.8 附录 A：发布者本机环境快照（2026-09-09）

> 以下为发布者机器（Windows，用户目录 `C:\Users\20183`）的记录，**供对照参考，非通用要求**。

**版本基线**

| 组件 | 版本 | 位置 |
|---|---|---|
| Node.js | v26.7.0 | `C:\Program Files\nodejs` |
| npm | 12.0.2 | 全局前缀 `C:\Users\20183\AppData\Roaming\npm` |
| pnpm | 11.22.0 | 同上 |
| @deepseek-ai/dsh | **0.1.5-rc.1**（npm `latest`/`next` 标签；`alpha` = 0.1.5-alpha.2） | `Roaming\npm\node_modules\@deepseek-ai\dsh` |
| Git | 2.55.0.4 | WinGet MinGit |
| Python | 3.13.15 | `C:\Users\20183\Local\Programs\Python\Python313` |
| DSHLauncher | **v5.0.0**（Rust 全量重写：wry + tao、单文件 exe、局域网移除、失联自愈、孤儿锁恢复） | `D:\DSHLauncher` |

**Agent 与模型 API 引用**

| 项 | 值 |
|---|---|
| provider | `deepseek-official` |
| BASE URL（OpenAI 格式） | `https://api.deepseek.com` |
| BASE URL（Anthropic 格式） | `https://api.deepseek.com/anthropic` |
| API Key 环境变量 | `DEEPSEEK_API_KEY` |
| 默认模型 | `deepseek-flash`（provider `deepseek-official`，reasoningEffort: high；2026-09-10 随 0.1.5-rc.1 内置默认同步，实测网关已开放）；`LongCat-2.0`（provider `longcat`，凭据引用 `LONGCAT_API_KEY`）保留在子代理白名单 |
| BASE URL 覆盖环境变量（可选） | `DEEPSEEK_BASE_URL`（未设置时使用 `https://api.deepseek.com`） |
| 默认输出上限 | DSH 默认 `256K`（官方支持最大 384K；模型自身上限与显式请求值优先） |

导入模型目录（0.1.5-rc.1 复核：`dsh-llm-deepseek`（0.1.5-rc.1）建议目录现为**四个**条目——新增 `deepseek-flash`（DeepSeek-V41-Flash，文本 + 图片，声明 `systemPromptUpdate: in-history`）并作为该目录首个条目与**新会话内置默认模型**；另三个（`deepseek-v4-flash` / `deepseek-v4-pro` / `deepseek-v4-flash-vision-exp`）与 0.1.3-alpha.2 / 0.1.2-rc.1 一致，四条各 1M（1e6）上下文；pi-ai 维持 0.85.1；本机 `settings.yaml` 显式指定 `agent-default-model`，按官方「显式配置优先」规则继续生效；官方提示网关未开放该 id 前请求可能 `INVALID_REQUEST`——本机 2026-09-10 实测 `deepseek-flash` 直连 `api.deepseek.com` 返回 HTTP 200，已开放；模型探测增强（自定义 provider `models` 对象、Anthropic 原生列表、名称/上下文/最大输出 token 回填）不改变已导入/默认条目）：

| 模型 id | 上下文 | 输出上限 | 输入模态 |
|---|---|---|---|
| `deepseek-flash` | 1M | 256K（DSH 默认） | text + image（DeepSeek-V41-Flash；0.1.5-rc.1 新增，建议目录首个条目与内置默认；声明 `systemPromptUpdate: in-history`；网关未开放该 id 前请求可能 `INVALID_REQUEST`） |
| `deepseek-v4-flash` | 1M | 384K | text |
| `deepseek-v4-pro` | 1M | 384K | text |
| `deepseek-v4-flash-vision-exp` | 1M | 384K | text + image（实验模型，`/list-models` 不列出但可直接调用） |
| `LongCat-2.0` | 1M | — | text（自定义 provider：`https://api.longcat.chat/openai/v1`，`LONGCAT_API_KEY`） |

凭据 / API Key 引用（密钥本体在 `C:\Users\20183\.dsh\.credentials.yaml`，不入库）：

| 环境变量 | 用途 | 备注 |
|---|---|---|
| `DEEPSEEK_API_KEY` | deepseek-official（对话 + web 搜索） | 必需 |
| `LONGCAT_API_KEY` | longcat（LongCat-2.0） | 使用 LongCat 时必需 |
| `ZHIPU_API_KEY` / `AGNES_API_KEY` | （预留） | 无对应 provider 配置，未使用 |

> 凭据文档格式：`.credentials.yaml` `version: 1`（0.1.5-rc.1 未变更；四个引用均完好，密钥本体未改动）；provider 侧仍以 `apiKeyEnv`（`credential-ref`）按请求解析，无需迁移。

### 1.9 附录 B：版本跟踪与破坏性变更速查

| 版本 | 标签 | 关键点 |
|---|---|---|
| 0.1.1-rc.2 | latest/next | JSONL 会话后端默认压缩改 `zstd`（注意混编码崩溃，故障 C）；内置 DeepSeek 模型目录（flash/pro/vision-exp） |
| 0.1.2-alpha.1 | （GitHub，未上架 npm） | **APIProxy 移除 → @Remote 网关**；pi-ai 模型支持更新 + vLLM 思考预算；统一 `dsh` Profile 启动；WebFetch 默认开启（SSRF 防护） |
| 0.1.2-alpha.2 | alpha（npm） | 含 alpha.1 全部变更；**Web 界面强制一次性 token 认证**（故障 D，启动器已适配）；恢复 `SessionEvent.ignorable`；RemoteError 统一封装；Node 24 启动修复 |
| 0.1.2-alpha.3 | alpha（npm） | 长会话右侧导航/渲染内存优化、图片回显与投递修复、连接误判修复、窄视口定时计划修复；移除**可选** SQLite 持久化后端（zstd JSONL 不受影响）；**无事件结构 / API / token 认证契约变更**（手机端 UI 与局域网网关无需适配） |
| 0.1.2-alpha.4 | alpha（npm） | 父 Agent 与可持续子 Agent 通过 `send_message` 双向传递后续消息；自定义模型发现复用 Profile 请求头、模型目录支持搜索筛选；超长会话流式渲染/导航预览内存优化；Python SDK/Headless/ACP 默认启用 `web_fetch`；Web PTC Mode 默认移除通用 `workflow` 工具；`Session.events` 被内部按需读取 API（`seq`/`eventAt()`/`snapshotEvents()`）取代（**外部 JSON-RPC API 契约未变**：`session/list`/`session/page`/`session/prompt` 响应结构不变，手机端 UI 与局域网网关无需适配） |
| 0.1.2-alpha.5 | alpha（npm） | 修复从 0.1.1-rc.2 或 0.1.2-alpha.3 升级时启动失败或会话列表标题丢失的问题（**无 API 契约变更**） |
| 0.1.2-rc.1 | latest/next（npm） | **0.1.2 首个候选版本**，汇总自 0.1.1-rc.2 以来的全部变更；**破坏性变更**：Session persistence API 改为生命周期持有的 `SessionHandle`，`agentLoop.create()` 改为异步并新增 session 锁；Session format 升级至 v2（旧 v0/v1 日志迁移至当前格式）；Remote 网关统一远程调用 API 与异常分发（旧 APIProxy 已移除）；新增 Inspector 工具、Web Preview、连接状态显示、子代理模型选择、回合导航、图片回显/投递修复等（**JSON-RPC API 契约与 alpha.4 一致**：手机端 UI 与局域网网关无需适配） |
| 0.1.3-alpha.1 | （GitHub，未上架 npm；npm alpha 标签直接跳到 alpha.2） | **破坏性变更**：Session persistence API 改为生命周期持有的 `SessionHandle`，`agentLoop.create()` 改异步 + session 锁（同一 session 至多一个进程持有）；Session format 升级 v2（旧 v0/v1 日志按不可变相邻 generation 迁移）；新增通用文件上传（任意类型、与图片预览混排、进度/取消、会话切换续显）、出站请求遵循 `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`、`read_image` 顶层与 PTC 嵌套调用直接渲染图片、模型探测增强（自定义 provider `models` 对象 + Anthropic 原生模型列表 + 名称/上下文/最大输出 token 回填）；含一项已知性能回退（alpha.2 已修复）（**外部 JSON-RPC 契约是否变更经 alpha.2 核验**：见下行） |
| 0.1.3-alpha.2 | alpha（npm） | pi-ai → 0.85.1（支持更多新模型）；Web 顶栏「在应用中打开」工作区；可持续子代理支持消息排队/编辑/删除/单条或全部 Steer/停止；PTC 模式可展开查看命令及输出；修复 Web 断线自动恢复、聊天自动滚底、Windows Python SDK 启动崩溃、进程清理残留；长会话打开/恢复/续聊卡顿与内存占用优化、引用长会话时模型可按需读取预览未展示内容；**默认工具调整**：SDK/Headless/ACP 默认改用 read/write/edit 编辑文件（Web minimal、sdk-minimal 不变）；**自定义 persona 配置拆分为前缀与后缀**（旧配置及相关常量需适配）；普通 subprocess handle 移除 `pid`（终端 handle 不受影响；属字段删除，读取方容忍缺失即可）（**外部 JSON-RPC 契约未变**：发布说明无 `session/list`/`session/page`/`session/prompt` 等端点变更条目 → 手机端 UI 与局域网网关无需适配） |
| 0.1.5-alpha.1 | alpha（npm） | **会话格式升级至 V3**（历史会话恢复时生成新版日志并保留原文件；系统提示词纳入消息历史；旧 PTC 事件与 `code` 预设引用自动迁移；自定义日志读取器需适配、不支持降级读取）；**插件 Agent API 移除 `ctx.agent`**（调用方须显式传 Agent；持续子代理归属修正，不再参与根会话定时调度）；Inbox 改为类型接口（插件经 `agent.inbox` 读写，`hasPending`/`claim` 不再是公共接口）；新增实验性右侧 Sidebar（原 Detail 面板移除）、动态系统提示词（需模型显式声明支持）；**原生依赖变更：移除 fs-ext**（会话锁改用 `@deepseek-ai/node-addon-system`，POSIX `flock(2)` / Windows 内核信号量；无需再本地编译，§1.4 第 4 项自此跳过）；pi-ai 仍 0.85.1；Codex/Claude Code 可选子代理运行时升级（显式模型配置不变）；修复发送按钮/Enter 行为一致性、暂停目标须用户恢复、POSIX 绝对路径本地图片显示、空消息/空白队列编辑拒绝等（**外部 JSON-RPC 契约未变**：发布说明无 `session/list`/`session/page`/`session/prompt` 等端点变更条目 → 手机端 UI 与局域网网关无需适配） |
| 0.1.5-alpha.2 | alpha（npm） | 右侧 Sidebar 新增 Markdown / 代码高亮 / HTML / PDF / 图片等常见文档类型预览；模型可显式在会话中交付文件（Sidebar 预览、默认应用打开、文件管理器定位）；`/feedback` 支持提交明细反馈；修复：模型目录变化后 pi-ai 配置失效致模型设置入口消失（错误项保留诊断/修复入口）、自定义 provider Base URL 在模型发现/创建前校验规范化、Windows 原生文件夹选择器窗口层级、Composer 空白输入占位提示残留、工具筛选后子代理仍收到不可用工具指导、MCP 重复分页游标挂起、npm 安装依赖 fs-ext 本地编译等；设置界面本地化与无障碍增强；**Session 数据格式仍为 V3**（无新增迁移）；**Web 插件面板 API 调整**（插件经 `sidebar.panellist` 与 `main` 注册全局面板，原 `conversation` Slot 迁移至 `main` 的 `conversation` key）；**极简模式默认工具调整**（Web `minimal` 与 Python `sdk-minimal` 默认仅提供持久 shell，`str_replace_editor` 改为显式启用；持久 Bash 输出统一报告退出或超时状态）；实验性 Agent Teams 包可经 npm 安装（需显式添加 profile，默认不启用）；pi-ai 仍 0.85.1；fs-ext 保持移除状态（**外部 JSON-RPC 契约未变**：发布说明无 `session/list`/`session/page`/`session/prompt` 等端点变更条目 → 手机端 UI 与局域网网关无需适配；API/凭据/模型引用契约与 0.1.5-alpha.1 一致） |
| 0.1.5-rc.1 | latest/next（npm；alpha = 0.1.5-alpha.2） | **0.1.5 首个候选版本**，汇总自 0.1.2-rc.1 以来的全部变更；**模型引用变更**：新增 `DeepSeek-V41-Flash`（`deepseek-flash`，文本 + 图片、支持历史内系统提示词更新），成为内置建议目录首个条目与**新会话内置默认模型**（配置文件显式指定时以配置值为准；网关未开放该 id 前请求可能 `INVALID_REQUEST`）；模型探测支持自定义 provider `models` 对象与 Anthropic 原生列表（含名称/上下文/最大输出 token 回填）；Web 通用文件上传、右侧 Sidebar（多标签/分栏/全屏 + 文档预览）、子代理消息排队/编辑/Steer/停止、`/feedback`、顶栏「在应用中打开」、出站请求遵循 `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`；**会话格式 V3 + SessionHandle 生命周期 + 异步 `agentLoop.create()` + session 锁**（自 0.1.3-alpha.1 起）；**默认工具调整**（SDK/Headless/ACP 用 read/write/edit；Web `minimal` 与 `sdk-minimal` 仅持久 shell，`str_replace_editor` 显式启用）；**插件 API 调整**（移除 `ctx.agent`、Inbox 类型化、Web 面板 `sidebar.panellist`/`main`）；pi-ai 0.85.1；Codex 0.153.4 / Claude Code 2.1.263；fs-ext 保持移除；**凭据引用契约未变**（`DEEPSEEK_API_KEY`/`LONGCAT_API_KEY`，`.credentials.yaml` version 1）；**外部 JSON-RPC 契约未变** → 手机端 UI 与局域网网关无需适配 |
| 启动器 v3.0.0 | — | 局域网共享与全新手机端专属 UI（非 dsh 原生）：会话分组折叠、聊天（历史/加载更早/大纲导航）、只读模式（前端隐藏 + 网关 API 拦截）、会话过滤（归档/子代理/空白）、归档会话一键彻底清理、PIN 轮换踢出所有设备、SW 随机化自动刷缓存 |
| 启动器 v4.1.0 | — | 多通道版本选择（latest / alpha / next / rc / beta / dev 等 npm dist-tags 自由切换）；升级页「修复模块」一键重编 fs-ext / koffi / node-pty；升级后 `CheckDshHealth` 健康检查与状态栏原生模块探测；强制官方 registry；allow-scripts 白名单补全 fs-ext |
| 启动器 v4.2.0（RC） | — | 全量一致性治理与底层整改：版本号全局统一 4.2.0；「修复模块」改为嵌套 + 顶层双路径定位、按实际安装自动识别（修复 npm 12 嵌套布局下 koffi/node-pty 被误判「未安装」而空转的问题）；fs-ext 随 dsh 0.1.5-alpha.1 移除自动剔除（文件锁改用 `@deepseek-ai/node-addon-system`）；健康检查/提示文案/维护手册同步适配；CHANGELOG 版本导航与锚点整理；新增一致性校验工具 `tools/check-consistency.ps1` |
| 启动器 v4.2.1 | — | 修复「清理归档会话」恒删除 0 个（归档目录实为 44 字符 `session-<uuid>`，旧守卫仅接受 36 字符）；清理改为先停 dsh 服务再删除、仅移除删除成功的归档项，消除数据残留与列表丢失风险 |
| 启动器 v4.2.2 | — | 归档会话清理可靠性增强：清理前按端口停稳 dsh（含“未接管/外部实例”）并等待端口释放；自动清出数据已不存在的遗留归档标记并清理 `session_projcache` 投影缓存，避免运行中 dsh 回写 `workspace.json` 导致归档列表复活 |
| 启动器 v4.2.3 | — | 归档会话清理加固：停服前进程身份校验（非 dsh 占用即中止，防误杀）；停服判定改为“进程消失 + 端口可重新绑定”双条件；清理重启后归档列表复活自动二轮清理（≤3 轮）；普通终止无效时 UAC 提权一次；清理按钮忙碌保护 |
| 启动器 v4.2.4 | — | 设置窗口改为**独立顶层窗口**（独立任务栏项、可单独最小化/关闭、记忆位置，退出时一并关闭）；受限权限下新增“node 进程 + 配置端口所有者”等价身份判定（修复无法自动接管与清理报“PID 0”），并支持经 UAC 提权用 `Get-NetTCPConnection` 按端口强制停止 |
| **启动器 v5.0.0** | — | **Rust 全量重写**（wry + tao 直连，无 .NET / 无旁挂 DLL，单文件 exe）；**局域网共享彻底移除**（约 6,400 行 + 902 KB）；`GetExtendedTcpTable` 免管理员识别端口占用者；append-only 滚动日志；**新增失联自愈与自动重连、孤儿锁自动恢复、设置页 IPC、命令行参数**；修复启动自死锁、接管不开窗、WebView2 侧挂数据目录、GUI 子系统弹终端、标题栏无图标等缺陷；**定稿轮：服务与启动器解耦**（默认 `service_lifecycle = "independent"`：退出/崩溃/升级不再中断会话；`service.json` 簿记 + 启动对账，含防 PID 复用）；**修复 token 捕获竞态**（实测「就绪」早于「token 到达」约 914 ms，旧实现此刻已用无 token 地址导航 → HTTP 401）；**修复"启动器只剩托盘、界面永不出现"的持锁 spawn 死锁**；**非 dsh 端口占用只读不接管**（防误杀）；stderr 排空；`tray_on_close` 接线；`F5`/`Ctrl+R`/`Esc`；主题跟随接线（异步）；**「清理归档会话」加二次确认并在清理后自动重启服务**；**退出行为随「服务独立」设置变化：独立模式不弹窗、tied 模式每次询问**；`--quit` 优雅退出；`--build-info` 产物判定；崩溃取证；主题日志刷屏修复；`build.ps1` 支持热替换发布；文档数字单一来源。详见 [`RELEASE_NOTES_v5.0.0.md`](RELEASE_NOTES_v5.0.0.md)、[`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md) §13 与 [`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md) |

> 核对命令：`npm view @deepseek-ai/dsh dist-tags`；发布说明见 `https://github.com/deepseek-ai/deepseek-harness/releases`。

### 1.10 附录 C：发布者本机升级历史

| 日期 | 操作 | 结果 |
|---|---|---|
| 2026-08-19 | 全环境核对；npm 11.19→12.0.2；装 pnpm 11.22.0；dsh rc.7 完整性修复（js-yaml.mjs/commander 缺失） | ✅ |
| 2026-08-20 | dsh rc.7 → rc.8（`next` 标签） | ⚠️ koffi 原生错位崩溃 → `--prefer-online` + 放行脚本重装修复（koffi 3.1.6 / 插件树 503 行） |
| 2026-08-22 | dsh rc.8 → 0.1.1-rc.2（内置 V4-Flash-Vision-Exp 注册） | ✅ 14 项核验 PASS（koffi 3.1.6 / 插件树 514 行） |
| 2026-08-30 | 复核：npm latest/next = 0.1.1-rc.2；GitHub 发 0.1.2-alpha.1（当时未上架 npm）→ 决策暂缓 | ✅ 保持 rc.2 |
| 2026-08-30（补） | 修复会话编码不匹配崩溃（zstd/plaintext，故障 C）；备份移出 `.dsh` | ✅ |
| 2026-08-30（补2） | 流程变更：移除一键升级脚本，改「一句话触发 Agent 按本手册执行」；清理根目录 | ✅ |
| 2026-08-31 | dsh 0.1.1-rc.2 → **0.1.2-alpha.2**（npm alpha 标签）；token 认证破坏性变更 → 启动器适配（故障 D） | ✅ |
| 2026-08-31（补） | 启动器行为修复：退出默认保留 dsh web（网页不中断）；自动接管依赖 30 天 cookie | ✅ |
| 2026-08-31（补2） | 版本升至 v2.0.0；手册并入 README（单文档随发布）；安装包部署 README、卸载脚本通用化 | ✅ |
| 2026-09-01 | dsh 0.1.2-alpha.2 → **0.1.2-alpha.3**（npm alpha 标签）；核验：koffi 3.1.6 / node-pty 可加载 / 插件树 539 行无 error；确认事件结构、`dsh-auth-` cookie、token 契约无变更 → **手机端 UI 无需升级**；API 引用复核与附录 A 一致 | ✅ |
| 2026-09-01（补） | 启动器全面修复并发布 v3.0.0：审查问题修复（归档清理/日志脱敏/网关安全等）、手机端 UI 修复（subagent 注入过滤、大纲独立分页、正序显示、标题换行）、运行日志 UTF-8 修复、dsh 升级至 0.1.2-alpha.3；手册拆分独立成文（本文件） | ✅ |
| 2026-09-01（补2） | dsh 0.1.2-alpha.3 → **0.1.2-alpha.4**（npm alpha 标签）；核验：koffi 3.1.6 / node-pty 可加载 / 插件树 525 行无 error；确认 `Session.events` 内部 API 变更不影响外部 JSON-RPC 契约（`session/list`/`session/page`/`session/prompt` 响应结构不变）→ **手机端 UI 无需升级**；API 引用复核与附录 A 一致 | ✅ |
| 2026-09-04 | dsh 0.1.2-alpha.4 → **0.1.2-alpha.5**（npm alpha 标签）；核验：koffi 3.2.1 / node-pty 可加载 / 插件树无 error；确认无 API 契约变更 → **手机端 UI 无需升级** | ✅ |
| 2026-09-04（补） | 尝试升级 dsh 0.1.2-alpha.5 → **0.1.2-rc.1**，因运行中的 dsh 进程锁定 `koffi.node` 导致 EBUSY 安装失败；残留 `dsh-broken-*`、`dsh-partial-*` 等目录（约 409 MB）；按故障 EBUSY 流程修复：停止进程 → 清理残留 → 重装 alpha.4 | ⚠️ 已修复 |
| 2026-09-04（补2） | 清理安装残留：删除 `%TEMP%` 下 9 个 `dsh-*` 残留目录（共约 409 MB）；更新维护手册第 1.3 节（新增"安装前停止 dsh"强制步骤、安装后清理流程）和故障 EBUSY | ✅ |
| 2026-09-04（补3） | dsh 0.1.2-alpha.4 → **0.1.2-rc.1**（npm latest/next 标签）；核验：koffi 3.2.1 / node-pty 可加载 / 插件树无 error；确认 Session persistence API 内部变更（SessionHandle + session lock）不影响外部 JSON-RPC 契约 → **手机端 UI 无需升级**；API 引用复核与附录 A 一致 | ✅ |
| 2026-09-08 | dsh 0.1.2-rc.1 → **0.1.3-alpha.2**（npm alpha 标签；latest/next 仍为 0.1.2-rc.1，经启动器 v4.1.0 多通道升级）；核验：安装版本一致 / fs-ext 可加载（新增原生模块）/ koffi 3.2.1 / node-pty 可加载；本地核验 `dsh-llm-deepseek` 模型目录与 rc.1 一致（flash/pro/vision-exp，1M 上下文）；发布说明无 `session/list`/`session/page`/`session/prompt` 等外部 JSON-RPC 端点变更条目 → **手机端 UI 无需升级**；API 引用复核与附录 A 一致 | ✅ |
| 2026-09-08（补） | 同步文档：附录 A 版本基线 → 0.1.3-alpha.2、模型目录复核说明更新；附录 B 补充 0.1.3-alpha.2 与启动器 v4.1.0 条目；附录 C 补记本次升级 | ✅ |
| 2026-09-09 | dsh 0.1.3-alpha.2 → **0.1.5-alpha.1**（npm alpha 标签；latest/next 仍为 0.1.2-rc.1）；核验：安装版本一致 / koffi 3.2.1 / node-pty 可加载 / **fs-ext 已被官方移除（属正常，勿按故障 G 处理）**；pi-ai 维持 0.85.1；本地核验 `dsh-llm-deepseek` 模型目录不变（flash/pro/vision-exp，1M 上下文）；发布说明无 `session/list`/`session/page`/`session/prompt` 等外部 JSON-RPC 端点变更条目 → **手机端 UI 无需升级**；API 引用复核与附录 A 一致 | ✅ |
| 2026-09-09（补） | 同步文档：附录 A 版本基线 → 0.1.5-alpha.1、模型目录复核说明更新、fs-ext 移除的版本适配说明；附录 B 补充 0.1.5-alpha.1 条目；附录 C 补记本次升级 | ✅ |
| 2026-09-09（补2） | 发布 **v4.2.0（RC）**（上一内部开发期版本未发布，其条目并入本版）：全量治理整改落地（版本号全局统一 4.2.0；「修复模块」嵌套双路径定位修复 + fs-ext 移除适配；npm dist-tags JSON 解析器修复；健康检查/提示文案/维护手册/CHANGELOG 同步；新增一致性校验工具 `tools/check-consistency.ps1`）；dsh 保持 0.1.5-alpha.1，API 引用复核与附录 A 一致 | ✅ |
| 2026-09-09（补3） | 启动器发布 **v4.2.1**：打包「清理归档会话」修复（44 字符目录守卫 + 先停服务再删除 + 仅移除删除成功的归档项）；重建安装包并发布；dsh 保持 0.1.5-alpha.1，API 引用不变 | ✅ |
| 2026-09-09（补4） | 启动器发布 **v4.2.2**：打包「按端口停稳 dsh + 幽灵归档标记/投影缓存清理」修复；重建安装包并发布；dsh 保持 0.1.5-alpha.1 | ✅ |
| 2026-09-09（补5） | 启动器发布 **v4.2.3**：打包归档清理加固（停服身份校验防误杀 + 权威停服判定 + 复活自愈二轮清理 + UAC 提权兜底 + UI 忙碌保护）；重建安装包并发布；dsh 保持 0.1.5-alpha.1 | ✅ |
| 2026-09-09（补6） | dsh 0.1.5-alpha.1 → **0.1.5-alpha.2**（npm `alpha` 标签；latest/next 仍为 0.1.2-rc.1）；核验：安装版本一致 / pi-ai 仍 0.85.1 / 本地核验 `dsh-llm-deepseek`（0.1.5-alpha.2）模型目录不变（flash/pro/vision-exp，1M 上下文）；发布说明无 `session/list`/`session/page`/`session/prompt` 等外部 JSON-RPC 端点变更条目 → **手机端 UI 无需升级**；API/凭据引用复核与附录 A 一致 | ✅ |
| 2026-09-10（补7） | 启动器发布 **v4.2.4**：打包「设置窗口独立化 + 受限权限身份判定/提权按端口停服」；重建安装包并发布；dsh 打包时为 0.1.5-alpha.2（同日随后升级至 0.1.5-rc.1，见下条），API 引用复核与附录 A 一致 | ✅ |
| 2026-09-10（补8） | dsh 0.1.5-alpha.2 → **0.1.5-rc.1**（npm `latest`/`next` 标签，稳定/预览通道自此为 0.1.5 系列；`alpha` 仍为 0.1.5-alpha.2）；核验：安装版本一致（dsh web 进程启动晚于安装）/ pi-ai 仍 0.85.1 / `dsh-llm-deepseek`（0.1.5-rc.1）建议目录新增 `deepseek-flash`（V41 Flash，1M、text+image、`systemPromptUpdate: in-history`）且内置默认模型改为 `deepseek-flash`；本机 `agent-default-model` 显式配置为 `longcat/LongCat-2.0`，按官方「显式配置优先」规则继续生效（网关未开放 `deepseek-flash` 前其请求可能 `INVALID_REQUEST`）；**凭据引用复核**：`DEEPSEEK_API_KEY`/`LONGCAT_API_KEY` 等四个引用与 `.credentials.yaml` `version: 1` 均未变，无需迁移；**模型引用同步**：本机 `agent-default-model` 切回 `deepseek-official/deepseek-flash`（reasoningEffort: high），并把 `deepseek-flash` 加入子代理 `allowedModels`（`LongCat-2.0` 保留）；发布说明无 `session/list`/`session/page`/`session/prompt` 等外部 JSON-RPC 端点变更条目 → **手机端 UI 无需升级** | ✅ |
