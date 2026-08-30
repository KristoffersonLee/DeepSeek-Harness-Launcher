# DeepSeek Harness Launcher 升级与维护手册

**版本**：v1.0（随 DSHLauncher 发布） ｜ **更新日期**：2026-08-31 ｜ **适用范围**：任何运行 Windows 10/11（64 位）并安装 DSHLauncher 的电脑

> 本手册指导 **dsh**（`@deepseek-ai/dsh`，DeepSeek Harness 命令行/服务）与 **DSHLauncher**（内嵌 WebView2 的桌面启动器）的
> **安装、升级、验证、故障恢复**。正文为**通用步骤**（不依赖具体电脑），文末附录保留**发布者机器的环境快照与升级历史**，供对照参考。
>
> 官方 dsh 仍处快速迭代（预览/rc/alpha），可能引入**破坏性更新**（端口/协议/接口、配置格式、工作目录等）。升级前务必阅读本手册第 3、4、7 节。

---

## 目录

1. [概述](#1-概述)
2. [环境要求与路径约定](#2-环境要求与路径约定)
3. [dsh 安装与升级](#3-dsh-安装与升级)
4. [升级后验证清单](#4-升级后验证清单)
5. [常见故障与修复](#5-常见故障与修复)
6. [启动器行为说明](#6-启动器行为说明)
7. [维护规范与防坑规则](#7-维护规范与防坑规则)
8. [附录 A：发布者本机环境快照（2026-08-31）](#8-附录-a发布者本机环境快照2026-08-31)
9. [附录 B：版本跟踪与破坏性变更速查](#9-附录-b版本跟踪与破坏性变更速查)
10. [附录 C：发布者本机升级历史](#10-附录-c发布者本机升级历史)
11. [发布与许可证](#11-发布与许可证)

---

## 1. 概述

- **dsh**：DeepSeek Harness 的 Node.js 服务与命令行。Web 界面默认监听 `http://127.0.0.1:3080/`。通过 npm 全局安装（`@deepseek-ai/dsh`）。
- **DSHLauncher**：Windows 桌面壳。双击即自动启动 `dsh web`，用**内嵌 WebView2 窗口**显示 Harness（无需浏览器），并提供托盘、设置、日志、自动接管、自愈等能力。
- **本手册的价值**：dsh 官方预览迭代快、曾出现破坏安装的原生模块问题与协议级变更（token 认证等）。按本手册第 3、4 节操作可避免「升级后启动器打不开/报错」。

## 2. 环境要求与路径约定

### 2.1 环境要求

| 组件 | 要求 |
|---|---|
| 操作系统 | Windows 10 / 11（64 位） |
| Node.js | 官方 LTS 或更新（DSHLauncher 使用系统 Node 启动 dsh） |
| npm | 官方 latest；**注意 npm 12 起默认阻止未白名单包的 install/postinstall 脚本**（见第 7 节防坑） |
| WebView2 运行时 | 一般随 Edge 自带；缺失时启动器自动部署或回退 Edge |

### 2.2 路径约定（通用）

| 用途 | 路径（通用写法） | 说明 |
|---|---|---|
| npm 全局前缀 | `npm config get prefix` 查询（Windows 通常 `%APPDATA%\npm`） | dsh 装在此前缀的 `node_modules\@deepseek-ai\dsh` |
| dsh 用户数据 | `%USERPROFILE%\.dsh\` | 会话（`sessions\`）、配置（`settings.yaml`）、凭据（`.credentials.yaml`）、插件（`profiles\`） |
| 启动器日志 | `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` | 排障第一入口 |
| 启动器设置 | `%APPDATA%\DSHLauncher\settings.ini` | 端口/工作目录/托盘行为 |
| Web 界面 | `http://127.0.0.1:3080/` | 默认端口；**0.1.2-alpha 起需带一次性 token**（见第 6 节） |

> 术语约定：下文中 `<npm-prefix>` 指 `npm config get prefix` 的输出；`<dsh-version>` 指目标版本号。

## 3. dsh 安装与升级

> 可由 AI Agent 依据本手册自动执行，也可人工按下列命令操作。两种方式等价。

### 3.1 确定目标版本（必须先做）

```powershell
npm view @deepseek-ai/dsh dist-tags --json   # 查看 latest / next / alpha 标签
npm view @deepseek-ai/dsh versions --json    # 查看全部已发布版本
```

- GitHub 发布页（`deepseek-ai/deepseek-harness` releases）可能先于 npm 上架；**以 npm 实际可安装为准**。
- alpha/rc 为预发布：升级前阅读对应版本发布说明，评估**破坏性变更**（速查见附录 B）。

### 3.2 安装 / 升级命令

```powershell
# 0) 确认 npm 前缀（必须与启动器使用的全局前缀一致）
npm config get prefix

# 1) 安装指定版本（显式版本号 + 防损坏缓存 + 放行原生构建脚本，勿省略）
npm install -g "@deepseek-ai/dsh@<dsh-version>" --no-audit --no-fund --prefer-online `
  --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
```

参数说明：

| 参数 | 作用 |
|---|---|
| `@deepseek-ai/dsh@<版本>` | **必须显式写版本号**，避免装回旧标签指向的版本 |
| `--prefer-online` | 绕过本地已损坏的 npm 缓存 tarball |
| `--allow-scripts=...` | **npm 12 必需**：放行 koffi（FFI）、node-pty（终端）等原生模块构建脚本，否则装出「半成品」（第 5 节故障 B） |
| `--no-audit --no-fund` | 提速、免打扰 |

若前缀/权限异常：用系统 npm 显式指定前缀，例如
`"C:\Program Files\nodejs\npm.cmd" install -g "@deepseek-ai/dsh@<版本>" --prefix "<npm-prefix>" --no-audit --no-fund --prefer-online --allow-scripts=...`。
若默认 npm-cache 报 EPERM，追加 `--cache <可写目录>`。

### 3.3 安装纪律

- **让安装完整跑完**（约 3–5 分钟），**不要中途 kill、不要同时杀进程**（曾因中断导致残留 worker 死锁与半成品目录）。
- 升级只改磁盘文件，运行中的 dsh web 不受影响；**升级后需重启启动器**才加载新版本。

## 4. 升级后验证清单

安装/重装后**逐项核验**，全部通过才算升级成功：

```powershell
# 1) 版本
dsh --version

# 2) 用法（触发 bin.js → dsh-app-boot → commander/js-yaml 加载链）
dsh --help

# 3) 插件配置树（验证 YAML 解析与整棵插件树可加载，正常约 500+ 行，无 error/mismatch）
dsh --profile web --dump-config

# 4) koffi 原生版本匹配（关键！）
node -e "const k=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"

# 5) node-pty 可加载
node -e "const p=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/node-pty'); console.log(typeof p.spawn)"
```

| 检查项 | 期望 |
|---|---|
| `dsh --version` | 与安装版本一致 |
| koffi | 输出版本与 JS 包装一致（如 `3.1.6`）；报 `Mismatched native Koffi modules` 即安装损坏 |
| node-pty | 输出 `function` |
| `dump-config` | 无 `error` / `mismatch` / `failed to` |
| 关键文件 | `<npm-prefix>\node_modules\@deepseek-ai\dsh\` 下：`package.json`、`lib\bin.js`、`node_modules\commander\index.js`、`node_modules\js-yaml\dist\js-yaml.mjs`、`node_modules\@koromix\koffi-win32-x64\win32_x64\koffi.node` |
| 残留目录 | `<npm-prefix>\node_modules\@deepseek-ai\` 下正常应只有 `dsh`（见第 5 节故障 E） |
| Web 界面 | 重启启动器后内嵌窗正常显示（0.1.2-alpha 起带 token，见第 6 节） |

## 5. 常见故障与修复

### 故障 A：模块缺失（js-yaml / commander 文件缺失）

- **现象**：`dsh` 命令报模块不存在；启动器无法启动。
- **根因**：安装被中断或装错前缀，留下半成品目录。
- **修复**：删除安装目录后完整重装（第 3 节），等它跑完；装错前缀的按第 3.2 节显式 `--prefix` 重装。

### 故障 B：koffi 原生二进制错位（Mismatched native Koffi modules）

- **现象**：启动器加载 `subprocess`/`sandbox` 插件时抛 `Mismatched native Koffi modules`，退出码 1，表现为「启动器打不开」。
- **根因**：npm 12 的 allow-scripts 策略拦截了 koffi 等构建脚本 → JS 与原生二进制版本错位。
- **修复**：用 `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` 完整重装（第 3.2 节），再按第 4 节核验 koffi 版本。**不要试图只替换单个 .node 文件**。

### 故障 C：会话编码不匹配（uses .jsonl, but this backend is configured for compression "zstd"）

- **现象**：启动器日志报 `encodingMismatch`，退出码 1；`dsh --version` / `dump-config` 正常。
- **根因**：0.1.1-rc.2 起会话后端默认 `compression: zstd`；若某个会话目录**混编码**（明文 `session.jsonl` 与 `session.jsonl.zstd` 并存），后端初始化即崩。**重装无效**（默认仍是 zstd）。
- **修复（统一 root 为 zstd）**：
  1. 定位冲突：在 `%USERPROFILE%\.dsh\sessions` 下统计明文与 zstd 数量；
  2. 对每个冲突目录：先解码 zstd 首帧确认会话 id 一致且含完整事件（证明 zstd 为权威数据），
     **将明文 `session.jsonl` 备份到 `.dsh` 目录之外**，然后删除明文，仅保留 `session.jsonl.zstd`；
  3. ⚠️ **备份切勿放在 `.dsh` 内部**（dsh 会把它当会话 root 扫描，明文备份再次触发同一崩溃）；
  4. 确认 `.dsh\sessions` 下明文数为 0，重启启动器。
- **备选**：若全部会话均为明文且想保留明文，可在 `%USERPROFILE%\.dsh\profiles\web\cordis.patch.yml` 追加
  `- id: session-persistence-jsonl / config: { compression: none }`（仅当无任何 zstd 会话时可用）。

### 故障 D：0.1.2-alpha 起 Web 界面要求一次性 token 认证（401）

- **现象**：直接访问 `http://127.0.0.1:3080/` 返回 **401**；启动器内嵌窗显示
  `dsh web authentication required; reopen the URL printed by dsh web`；日志可见 `dsh web: http://127.0.0.1:3080/?token=…`（每次启动不同）。
- **根因**：0.1.2-alpha 起 `dsh-client-connection` 对 Web 界面强制**一次性 token 认证**（每次启动生成新 token，访问一次后换取 30 天有效的浏览器会话 cookie），**无配置关闭开关**。
- **修复**：使用 `dsh web` 打印的带 token URL；**新版 DSHLauncher（2026-08-31 起）已自动捕获该 URL 并导航**，无需手工处理。旧版启动器请升级。
- **要点**：token 一次性、每次启动不同；浏览器会话 cookie 有效期默认 30 天，期间重开启动器自动接管（见第 6 节）无需重新认证；cookie 过期后 401 时重启一次服务即可。

### 故障 E：启动器打不开 / 无响应

1. 停止启动器，确认崩溃进程已退出（`Get-NetTCPConnection -LocalPort 3080`）；用 `Get-CimInstance Win32_Process` 看命令行，**勿误杀其它 node 进程**（如其它工具的 MCP/agent）。
2. 清理残留临时目录：
   ```powershell
   Get-ChildItem "<npm-prefix>\node_modules\@deepseek-ai" -Force   # 正常应只有 dsh
   Remove-Item "<残留目录路径>" -Recurse -Force                    # 如 .dsh-*（含 sharp DLL）
   ```
   被运行中进程锁定的残留，**重启启动器后即可删**。
3. 删除损坏安装（`Remove-Item "<npm-prefix>\node_modules\@deepseek-ai\dsh" -Recurse -Force`）后按第 3 节完整重装，按第 4 节核验，再重启启动器。

### 故障 F：常见误报（不是问题）

| 现象 | 说明 |
|---|---|
| `npm ls -g` 里 `UNMET OPTIONAL DEPENDENCY @img/sharp-*`（darwin/linux/freebsd） | Windows 本就不装，正常 |
| `EPERM` 写 `cordis.yml` | 通常是有另一实例占用端口/沙箱限制，非配置损坏 |
| `npm warn cleanup Failed to remove .dsh-*` | 临时目录被运行中进程锁定，重启后可删 |

## 6. 启动器行为说明

- **内嵌窗口**：WebView2 渲染 Harness，无需浏览器；不可用时自动回退 Edge 精简窗口。
- **托盘菜单**：打开界面 / 刷新 / 浏览器打开 / 启动服务 / 停止服务 / 新手指引 / 日志目录 / 设置 / 关于 / 退出。
- **token 认证适配（2026-08-31 起）**：启动器捕获 `dsh web` 输出行中的 `/?token=…` 并导航到带 token 地址；旧版本 dsh 无此输出时自动回退普通地址（兼容）。
- **服务生命周期（2026-08-31 起）**：
  - 启动器**退出默认保留 dsh web 后台运行**，网页端不中断；下次打开自动识别并接管；
  - 需停止服务：托盘「停止服务」，或关闭窗口提示时选“是”；
  - 点 ✕（勾选「关闭时最小化到托盘」）→ 隐藏到托盘，程序与服务继续常驻；
  - 接管依赖浏览器会话 cookie（默认 30 天有效）；cookie 过期后重开若遇 401，重启一次服务即可。
- **日志**：`%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`（超 2MB 自动裁剪）。

## 7. 维护规范与防坑规则

1. **全局 npm 前缀必须与启动器使用的前缀一致**。执行前 `npm config get prefix` 确认；多 Node 环境（如其它工具的受管 Node）下 `npm` 可能指向不同前缀导致装错位置——用系统 npm + 显式 `--prefix` 最稳。
2. **npm 12 的 allow-scripts 策略会静默破坏原生模块**（故障 B 根因）：升级 dsh 必须带 `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs`，装完立即核验 koffi 版本。
3. **升级/重装后必须核验完整性**（第 4 节清单），尤其是 koffi 原生版本与 `dump-config`。
4. **不要一边装一边杀进程**；让安装完整跑完，确需中断时先确认残留进程。
5. **升级前评估破坏性变更**：阅读目标版本发布说明（速查见附录 B）；协议/配置变更（如 token 认证、APIProxy 移除）可能影响启动器与模型配置。
6. **版本显式化**：安装/升级必须写显式版本号，避免标签漂移。
7. **凭据安全**：API Key 存放于 `%USERPROFILE%\.dsh\.credentials.yaml`，**不要提交到仓库或写入文档**；升级 dsh 不改变凭据。

## 8. 附录 A：发布者本机环境快照（2026-08-31）

> 以下为发布者机器（Windows，用户目录 `C:\Users\20183`）的记录，**供对照参考，非通用要求**。

### A.1 版本基线

| 组件 | 版本 | 位置 |
|---|---|---|
| Node.js | v26.7.0 | `C:\Program Files\nodejs` |
| npm | 12.0.2 | 全局前缀 `C:\Users\20183\AppData\Roaming\npm` |
| pnpm | 11.22.0 | 同上 |
| @deepseek-ai/dsh | **0.1.2-alpha.2**（npm `alpha` 标签；latest/next = 0.1.1-rc.2） | `Roaming\npm\node_modules\@deepseek-ai\dsh` |
| Git | 2.55.0.4 | WinGet MinGit |
| Python | 3.13.15 | `C:\Users\20183\Local\Programs\Python\Python313` |
| DSHLauncher | v1.0.0（含 token 认证适配与退出保留服务） | `D:\DSHLauncher` |

### A.2 Agent 与模型 API 引用

**Agent 连接引用**

| 项 | 值 |
|---|---|
| provider | `deepseek-official` |
| BASE URL（OpenAI 格式） | `https://api.deepseek.com` |
| BASE URL（Anthropic 格式） | `https://api.deepseek.com/anthropic` |
| API Key 环境变量 | `DEEPSEEK_API_KEY` |
| 默认模型 | `deepseek-v4-flash`（reasoningEffort: high） |
| 默认输出上限 | DSH 默认 `256K`（官方支持最大 384K） |

**导入模型目录**（0.1.2-alpha.2 复核与 0.1.1-rc.2 一致）

| 模型 id | 上下文 | 输出上限 | 输入模态 |
|---|---|---|---|
| `deepseek-v4-flash` | 1M | 384K | text |
| `deepseek-v4-pro` | 1M | 384K | text |
| `deepseek-v4-flash-vision-exp` | 1M | 384K | text + image（实验模型，`/list-models` 不列出但可直接调用） |
| `LongCat-2.0` | 1M | — | text（自定义 provider：`https://api.longcat.chat/openai/v1`，`LONGCAT_API_KEY`） |

**凭据 / API Key 引用**（密钥本体在 `C:\Users\20183\.dsh\.credentials.yaml`，不入库）

| 环境变量 | 用途 | 备注 |
|---|---|---|
| `DEEPSEEK_API_KEY` | deepseek-official（对话 + web 搜索） | 必需 |
| `LONGCAT_API_KEY` | longcat（LongCat-2.0） | 使用 LongCat 时必需 |
| `ZHIPU_API_KEY` / `AGNES_API_KEY` | （预留） | 无对应 provider 配置，未使用 |

## 9. 附录 B：版本跟踪与破坏性变更速查

| 版本 | 标签 | 关键点 |
|---|---|---|
| 0.1.1-rc.2 | latest/next | JSONL 会话后端默认压缩改 `zstd`（注意混编码崩溃，故障 C）；内置 DeepSeek 模型目录（flash/pro/vision-exp） |
| 0.1.2-alpha.1 | （GitHub，未上架 npm） | **APIProxy 移除 → @Remote 网关**；pi-ai 模型支持更新 + vLLM 思考预算；统一 `dsh` Profile 启动；WebFetch 默认开启（SSRF 防护） |
| 0.1.2-alpha.2 | alpha（npm） | 含 alpha.1 全部变更；**Web 界面强制一次性 token 认证**（故障 D，启动器已适配）；恢复 `SessionEvent.ignorable`；RemoteError 统一封装；Node 24 启动修复 |

> 核对命令：`npm view @deepseek-ai/dsh dist-tags`；发布说明见 `https://github.com/deepseek-ai/deepseek-harness/releases`。

## 10. 附录 C：发布者本机升级历史

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
| 2026-08-31（补2） | 手册重写为通用正式版（随启动器发布）；本机记录移入附录 | ✅ |

## 11. 发布与许可证

- 本手册随 **DSHLauncher** 一同发布，位于启动器安装目录（`DSH-升级维护手册.md`）。
- 项目采用 [MIT License](LICENSE)。**注意**：`dsh` 为 DeepSeek Harness 官方产品（"DeepSeek Harness"为注册商标，使用需遵循其品牌规范）；本手册与启动器均为社区维护，与官方无隶属关系。
- 官方预览版可能发布破坏性更新，本启动器与手册可能随之调整；使用前请阅读 [README.md](README.md) 中的稳定性声明。
