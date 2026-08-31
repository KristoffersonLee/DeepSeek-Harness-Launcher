# DeepSeek Harness Launcher

一个为 **DeepSeek Harness** 打造的 Windows 桌面启动器：双击图标即自动启动 dsh web 服务，并在内嵌的 **WebView2** 窗口中直接显示 Harness —— 无需浏览器，像原生软件一样使用。

A Windows desktop launcher for **DeepSeek Harness**: double-click to automatically start the dsh web service and view Harness directly in an embedded **WebView2** window — no browser needed, used like a native app.

**作者 / Author: [KristoffersonLee](https://github.com/)** · v3.0.0

> ⚠️ **重要说明 / Important Notice**
>
> 本工具基于 **DeepSeek Harness 官方预览版** 构建。官方预览版仍处于快速迭代阶段，未来可能发布**破坏性更新**（包括端口 / 协议 / 接口、配置格式、工作目录结构等变化），届时本启动器可能无法兼容，**本项目也可能随之停止维护**。请知悉后再决定是否使用。
>
> This launcher is built on top of the **official preview release of DeepSeek Harness**. The official preview is still evolving rapidly and may introduce **breaking changes** in the future (including port / protocol / interface, config format, working-directory layout, etc.). In that case this launcher may become incompatible, and **this project may be discontinued**. Please be aware of this before using it.

**[中文说明](#中文说明) | [升级与维护手册（通用版）](#升级与维护手册通用版) | [English](#english) | [Upgrade & Maintenance Manual (English)](#upgrade--maintenance-manual-english)**

---

# 中文说明

## 目录
- [简介](#简介)
- [功能特性](#功能特性)
- [安装](#安装)
- [使用](#使用)
- [环境要求](#环境要求)
- [从源码构建](#从源码构建)
- [卸载](#卸载)
- [目录结构](#目录结构)
- [许可证](#许可证)
- [升级与维护手册（通用版）](#升级与维护手册通用版)
- [Upgrade & Maintenance Manual (English)](#upgrade--maintenance-manual-english)

## 简介

DeepSeek Harness 本身是网页应用（React 前端 + Node.js 服务），通常需要在浏览器中打开并忍受浏览器的内存开销与干扰。本启动器把 Harness 包装成一个标准桌面应用：

- **内嵌窗口**：用微软官方 WebView2 引擎把 Harness UI 直接渲染在应用窗口里（不启动 Edge、不经过你的主浏览器），并带标准菜单栏（设置 / 帮助 / 关于）。
- **零配置**：双击即用 —— 自动检测并启动 dsh web、自动打开界面、自动接管上次遗留的进程。
- **后台常驻**：点 ✕ 收进系统托盘，服务继续运行；托盘菜单可随时恢复、停止或彻底退出。

## 功能特性

- 🖥️ **内嵌 WebView2 界面**（无需浏览器）；WebView2 不可用时自动回退 Edge 精简窗口，再回退默认浏览器
- 🎨 **标题栏/菜单栏配色跟随 Harness 主题**（浅色/深色自动适配）
- ⚡ **一键启动**：双击即自动启动服务并打开界面
- 🔄 **自动接管**：识别并接管端口上已有的 Harness 进程（含上次遗留的孤儿进程）
- 🛡️ **自愈能力**：残留卡死进程自动清理、120 秒启动超时保护、运行中挂起自动重启（最多 3 次）
- 🔑 **token 认证适配（v2.0）**：自动捕获 `dsh web` 输出的一次性 token URL 并导航，兼容 dsh 0.1.2-alpha 起的强制认证
- 💤 **退出保留服务（v2.0）**：退出启动器默认保留 dsh web 后台运行，网页端不中断；下次打开自动接管
- 📋 **设置窗口**：端口 / 工作目录 / 关闭时最小化到托盘 / 实时运行日志
- 📝 **日志文件**：`%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`（超 2MB 自动裁剪）
- 🗂️ **托盘菜单**：打开界面 / 刷新 / 浏览器打开 / 启动 / 停止 / 新手指引 / 打开日志目录 / 设置 / 关于 / 退出
- 🧩 **零依赖分发**：单文件安装包内嵌启动器与 WebView2 运行库，自动部署缺失环境（Node.js / dsh / WebView2 运行时）
- 📡 **局域网共享与手机端（v3.0）**：手机/平板在同一 WiFi 下扫码即可访问；默认关闭，行为与旧版完全一致
  - 只绑定当前活跃的 WiFi/以太网**具体 IP**（绝不绑定 0.0.0.0），自动打印地址并生成二维码（qrcode.js）
  - **全新独立移动端 UI**（非 dsh 原生界面）：会话列表（按工作区分组折叠）+ 完整聊天（历史消息、上滑加载更早、对话大纲导航、底部输入栏）
  - **手机端只读模式**：不能新建会话 / 切换或添加工作区（前端隐藏 + 后端 API 拦截双重保障）
  - **会话列表智能过滤**：手机端只显示顶级用户会话（自动隐藏归档 / 子代理 / 空白会话，与桌面端一致）
  - **归档会话彻底清理**：设置面板一键删除归档会话的全部历史数据（聊天记录、文件引用，不可恢复；重启生效）
  - **PIN/Token 门禁**（HttpOnly Cookie，重生成 PIN 自动踢出所有设备）+ **速率限制** + 会话密钥轮换
  - 零依赖 Node 网关（内嵌资源），SSE / WebSocket 流式透传，PWA 增强（manifest / SW / 添加到主屏幕，SW 版本随机化自动刷新缓存）
  - Windows 防火墙自动放行（`remoteip=localsubnet`，仅局域网）；无管理员权限时给出可复制的手动命令
  - 开启局域网后自动为 dsh 进程设置 `OLLAMA_HOST=0.0.0.0`、`OLLAMA_ORIGINS=*`（使用 Ollama 本地推理时生效）

## 安装

方式一：运行 `DSHLauncherSetup.exe`（一键安装包）——自动检测环境，缺失时一键部署：
- Node.js → winget 安装 LTS（失败自动下载官方 MSI 静默安装）
- dsh → `npm install -g @deepseek-ai/dsh`（无写权限时自动改装到当前用户目录）
- **WebView2 运行时** → 自动下载微软官方 Evergreen 引导程序静默安装（内嵌界面依赖）

方式二：直接运行 `DSHLauncher.exe`（绿色版，需同目录的 WebView2 三个 DLL）。

## 使用

1. 双击启动器（或桌面快捷方式 **DeepSeek Harness Launcher**）。
2. 服务自动启动，内嵌窗口弹出显示 Harness 界面；控制面板不闪现，程序只以托盘图标存在。
3. 点 ✕ 收进托盘（勾选"关闭时最小化到托盘"时）；未勾选则询问是否停止服务后真正退出。
4. 菜单栏"设置"打开设置窗口；"帮助 → 使用文档"打开内置新手指引；"打开日志目录"直达日志文件夹。
5. （可选）设置窗口 →「局域网共享」→ 勾选"允许局域网访问"，确认提示后即可用手机扫码使用。

## 局域网共享（手机 / 平板扫码访问）

> 原理：**手机连接的是启动器内嵌的 Web 网关，而不是 WebView2 界面**。网关（`lan-gateway.mjs`，
> 零依赖 Node 实现，随启动器内嵌）把 `dsh web`（默认仅监听 127.0.0.1）安全地暴露给同一 WiFi。

### 开启步骤

1. 打开设置窗口 →「局域网共享」→ 勾选"允许局域网访问（默认关闭）"。
2. 首次开启会弹出确认框（请确保处于可信网络，如家庭 WiFi，不要在公共网络开启），确认后：
   - 自动检测活动网卡 IP（优先 WiFi/以太网，排除 VPN/虚拟网卡）并绑定该具体 IP；
   - **访问 PIN 可自定义**：在「局域网共享」面板的“访问密码 (PIN)”输入框填写自定义 PIN 并失焦即保存
     （也可通过环境变量 / `.env` 的 `DSH_LAN_PIN` 指定，禁止硬编码）；留空则自动生成 6 位 PIN；
   - 自动尝试添加防火墙规则（仅本地子网），无管理员权限时在面板显示可复制的手动命令；
   - 面板显示完整地址 `http://<IP>:<端口>/` 与二维码（qrcode.js 渲染）。
3. 手机连同一 WiFi，用相机 / 微信扫描二维码 → 输入 PIN → 进入**全新移动端专属 UI**（非 dsh 原生界面）：
   - **会话列表**：按工作区分组、可折叠；只显示顶级用户会话（自动隐藏归档 / 子代理 / 空白会话）；
   - **聊天**：历史消息加载、上滑加载更早、对话大纲（≡ 导航）跳转任意节点、底部输入栏发送消息；
   - **只读模式**：手机端不能新建会话 / 切换或添加工作区（界面隐藏 + 网关 API 拦截双重保障）；
   - 首次访问后写入 HttpOnly Cookie（30 天），后续免密；重新生成 PIN 会踢出所有设备；
   - 支持添加到主屏幕（PWA 增强，iOS 直接支持）；缓存由随机化 SW 版本自动刷新。
4. **彻底清理归档会话**：电脑端设置 →「清理归档会话」→ 删除归档会话的全部历史数据（聊天记录、文件引用，不可恢复；重启生效），手机端同步消失。
5. 关闭开关后，网关立即停止、防火墙规则清理，手机将**连接被拒绝**。

### 安全机制

- **默认关闭，显式开启**：开关默认关闭，关闭状态下手机无法访问，行为与升级前完全一致；
- **PIN 门禁**：首次访问必须输入 PIN（环境变量 → 启动器目录 `.env` → `%APPDATA%\.env` → lan-pin.txt 4 级解析，绝不硬编码），
  验证通过写入 `HttpOnly; SameSite=Strict` Cookie；
- **速率限制**：按来源 IP 滑窗计数（整体 / API / 登录分别限流，可调），超限返回 429；
- **最小暴露面**：网关只绑定检测到的具体局域网 IP，不绑定 0.0.0.0；防火墙规则限定 `remoteip=localsubnet`；
- **双层认证**：网关 PIN 之外，dsh 自身的启动令牌认证依然生效（网关自动兑换，手机无感知）。

### Windows 防火墙（自动 + 手动）

开启时自动执行（无需管理员时可能失败）：

```powershell
netsh advfirewall firewall add rule name="DSHLauncher LAN 3081" dir=in action=allow protocol=TCP localport=3081 remoteip=localsubnet
```

若自动配置失败（无管理员权限），在面板复制手动命令，或以管理员身份运行 PowerShell 执行：可用面板的
「以管理员身份配置防火墙」按钮（触发 UAC），或手动运行：

```powershell
# 放行（管理员 PowerShell）
netsh advfirewall firewall add rule name="DSHLauncher LAN 3081" dir=in action=allow protocol=TCP localport=3081 remoteip=localsubnet
# 关闭时清理
netsh advfirewall firewall delete rule name="DSHLauncher LAN 3081"
# 查看是否已存在
netsh advfirewall firewall show rule name="DSHLauncher LAN 3081"
```

> 💡 请把当前 WiFi 网络设置为「专用」网络（设置 → 网络和 Internet → 属性），`remoteip=localsubnet` 规则
> 配合专用网络配置可进一步降低暴露面。

### 使用 Ollama 本地推理

开启局域网后，启动器会自动为 dsh 进程设置 `OLLAMA_HOST=0.0.0.0`、`OLLAMA_ORIGINS=*`，
使模型接口可被局域网内的网关调用（无论是否真的使用 Ollama 都会设置，设置面板会提示已检测到 Ollama）。手动启动 Ollama 时：

```powershell
$env:OLLAMA_HOST = "0.0.0.0"
$env:OLLAMA_ORIGINS = "*"
ollama serve
```

### 手机端测试验证步骤

1. 电脑开启「局域网共享」，记下地址（如 `http://192.168.5.5:3081/`）与 PIN；
2. 手机连同一 WiFi，浏览器（或相机扫码）打开该地址 → 应出现 PIN 登录页；
3. 输入错误 PIN → 提示密码错误；输入正确 PIN → 自动进入**全新移动端专属 UI**；
4. 会话列表按工作区分组、可折叠，只显示顶级用户会话（无归档/子代理/空白；分组名与会话标题最多两行完整显示）；
5. 点任一会话 → 进入聊天：历史消息、上滑加载更早、≡ 大纲跳转对话节点、底部输入栏发送消息（消息自动滚动）；
6. 尝试新建会话 / 切换工作区 → 被网关拒绝（只读模式提示）；
7. 手机锁屏再开、旋转横屏 → 布局正常，输入框不被键盘遮挡；
8. iOS「添加到主屏幕」/ Android 菜单「添加到主屏幕」→ 以类原生窗口打开；
9. 电脑端设置「清理归档会话」→ 归档会话及其数据彻底删除，手机端同步消失；
10. 电脑端关闭「局域网访问」→ 手机刷新 → 连接被拒绝 / 超时（无法访问）；
11. 电脑端启动器**退出**后（保留服务）→ 手机刷新仍可访问（网关继续运行）；
12. 默认状态（开关关闭）→ 手机无法访问，电脑端行为与升级前完全一致。

## 环境要求

- Windows 10 / 11（64 位）
- Node.js LTS（缺失时安装包可自动部署）
- dsh（`npm install -g @deepseek-ai/dsh`，缺失时安装包可自动部署）
- **WebView2 运行时**（Win10/11 通常随 Edge 自带；缺失时安装包可自动部署，运行时会回退 Edge）

## 从源码构建

无需任何安装，Windows 自带 .NET Framework 编译器；WebView2 SDK DLL 已存放在 `lib\`：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1        # 构建 DSHLauncher.exe
powershell -NoProfile -ExecutionPolicy Bypass -File build-setup.ps1  # 构建安装包（含启动器与运行库）
```

自检（启动 → 就绪 → 停止 全链路）：

```powershell
.\selftest.ps1   # 或 DSHLauncher.exe --selftest
```

> ⚠️ 源码为 UTF-8（无 BOM）：构建脚本请用 **PowerShell 7（pwsh）** 执行；若用 Windows PowerShell 5.1，请以 `-File` 方式并确保控制台代码页为 65001（否则中文注释可能被按 ANSI 误读而报语法错）。

## 卸载

运行安装目录里的 `uninstall.cmd`（或"设置 → 应用"里卸载），会清理文件、注册表项与桌面图标；
`%APPDATA%\DSHLauncher\settings.ini` 设置会保留，重装后不丢失。

## 目录结构

```
DSHLauncher.cs / LanAccess.cs           启动器源码与局域网辅助层（.NET Framework 4.x WinForms，C# 5）
DSHLauncherSetup.cs                    一键安装包源码
lan-gateway.mjs                        局域网网关（零依赖 Node，构建时内嵌为资源）
build.ps1 / build-setup.ps1            构建脚本（系统自带 csc，零依赖编译；内嵌 lan-gateway.mjs 与图标）
make-icon.ps1 / app.ico / whale-256.png / assets/
                                        图标生成与资源（官方 DeepSeek 鲸鱼 LOGO；whale-256.png 内嵌为手机端 PWA 图标）
lib/                                   WebView2 SDK 官方 DLL（MIT 许可）
selftest.ps1                           自检脚本
uninstall.cmd                          卸载脚本（安装目录内的同名文件由安装包生成，本文件为仓库模板）
README.md / LICENSE                    本文件（含升级与维护手册）与许可证
```

> 说明：升级与维护手册已并入本 README（见下方「升级与维护手册（通用版）」），不再单独成文。

## 许可证

[MIT License](LICENSE) © 2026 KristoffersonLee

---

# 升级与维护手册（通用版）

> 本手册指导 **dsh**（`@deepseek-ai/dsh`，DeepSeek Harness 命令行/服务）与 **DSHLauncher** 的**安装、升级、验证、故障恢复**。
> 正文为**通用步骤**（不依赖具体电脑），文末附录保留**发布者机器的环境快照与升级历史**，供对照参考。
> 官方 dsh 仍处快速迭代（预览/rc/alpha），可能引入**破坏性更新**（端口/协议/接口、配置格式、工作目录等）。升级前务必阅读本手册第 3、4、7 节。

## 手册目录

1. [概述](#11-概述)
2. [环境要求与路径约定](#12-环境要求与路径约定)
3. [dsh 安装与升级](#13-dsh-安装与升级)
4. [升级后验证清单](#14-升级后验证清单)
5. [常见故障与修复](#15-常见故障与修复)
6. [启动器行为说明](#16-启动器行为说明)
7. [维护规范与防坑规则](#17-维护规范与防坑规则)
8. [附录 A：发布者本机环境快照（2026-08-31）](#18-附录-a发布者本机环境快照2026-08-31)
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
npm install -g "@deepseek-ai/dsh@<dsh-version>" --no-audit --no-fund --prefer-online `
  --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
```

| 参数 | 作用 |
|---|---|
| `@deepseek-ai/dsh@<版本>` | **必须显式写版本号**，避免装回旧标签指向的版本 |
| `--prefer-online` | 绕过本地已损坏的 npm 缓存 tarball |
| `--allow-scripts=...` | **npm 12 必需**：放行 koffi（FFI）、node-pty（终端）等原生模块构建脚本，否则装出「半成品」（第 1.5 节故障 B） |
| `--no-audit --no-fund` | 提速、免打扰 |

若前缀/权限异常：用系统 npm 显式指定前缀，例如
`"C:\Program Files\nodejs\npm.cmd" install -g "@deepseek-ai/dsh@<版本>" --prefix "<npm-prefix>" --no-audit --no-fund --prefer-online --allow-scripts=...`。
若默认 npm-cache 报 EPERM，追加 `--cache <可写目录>`。

**安装纪律**

- 让安装**完整跑完**（约 3–5 分钟），**不要中途 kill、不要同时杀进程**（曾因中断导致残留 worker 死锁与半成品目录）。
- 升级只改磁盘文件，运行中的 dsh web 不受影响；**升级后需重启启动器**才加载新版本。

### 1.4 升级后验证清单

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
- **修复**：用 `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` 完整重装（第 1.3 节），再按第 1.4 节核验 koffi 版本。**不要试图只替换单个 .node 文件**。

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

**故障 F：常见误报（不是问题）**

| 现象 | 说明 |
|---|---|
| `npm ls -g` 里 `UNMET OPTIONAL DEPENDENCY @img/sharp-*`（darwin/linux/freebsd） | Windows 本就不装，正常 |
| `EPERM` 写 `cordis.yml` | 通常是有另一实例占用端口/沙箱限制，非配置损坏 |
| 局域网共享打不开 / 手机 401 | 见「局域网共享」章节：检查开关、PIN、防火墙规则；`%LOCALAPPDATA%\DSHLauncher\logs\lan-gateway.log` 排障 |
| `npm warn cleanup Failed to remove .dsh-*` | 临时目录被运行中进程锁定，重启后可删 |

### 1.6 启动器行为说明

- **内嵌窗口**：WebView2 渲染 Harness，无需浏览器；不可用时自动回退 Edge 精简窗口。
- **托盘菜单**：打开界面 / 刷新 / 浏览器打开 / 启动服务 / 停止服务 / 新手指引 / 日志目录 / 设置 / 关于 / 退出。
- **token 认证适配（v2.0 起）**：启动器捕获 `dsh web` 输出行中的 `/?token=…` 并导航到带 token 地址；旧版本 dsh 无此输出时自动回退普通地址（兼容）。
- **服务生命周期（v2.0 起）**：
  - 启动器**退出默认保留 dsh web 后台运行**，网页端不中断；下次打开自动识别并接管；
  - 需停止服务：托盘「停止服务」，或关闭窗口提示时选"是"；
  - 点 ✕（勾选「关闭时最小化到托盘」）→ 隐藏到托盘，程序与服务继续常驻；
  - 接管依赖浏览器会话 cookie（默认 30 天有效）；cookie 过期后重开若遇 401，重启一次服务即可。
- **日志**：`%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`（超 2MB 自动裁剪）。

### 1.7 维护规范与防坑规则

1. **全局 npm 前缀必须与启动器使用的前缀一致**。执行前 `npm config get prefix` 确认；多 Node 环境（如其它工具的受管 Node）下 `npm` 可能指向不同前缀导致装错位置——用系统 npm + 显式 `--prefix` 最稳。
2. **npm 12 的 allow-scripts 策略会静默破坏原生模块**（故障 B 根因）：升级 dsh 必须带 `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs`，装完立即核验 koffi 版本。
3. **升级/重装后必须核验完整性**（第 1.4 节清单），尤其是 koffi 原生版本与 `dump-config`。
4. **不要一边装一边杀进程**；让安装完整跑完，确需中断时先确认残留进程。
5. **升级前评估破坏性变更**：阅读目标版本发布说明（速查见附录 B）；协议/配置变更（如 token 认证、APIProxy 移除）可能影响启动器与模型配置。
6. **版本显式化**：安装/升级必须写显式版本号，避免标签漂移。
7. **凭据安全**：API Key 存放于 `%USERPROFILE%\.dsh\.credentials.yaml`，**不要提交到仓库或写入文档**；升级 dsh 不改变凭据。

### 1.8 附录 A：发布者本机环境快照（2026-08-31）

> 以下为发布者机器（Windows，用户目录 `C:\Users\20183`）的记录，**供对照参考，非通用要求**。

**版本基线**

| 组件 | 版本 | 位置 |
|---|---|---|
| Node.js | v26.7.0 | `C:\Program Files\nodejs` |
| npm | 12.0.2 | 全局前缀 `C:\Users\20183\AppData\Roaming\npm` |
| pnpm | 11.22.0 | 同上 |
| @deepseek-ai/dsh | **0.1.2-alpha.2**（npm `alpha` 标签；latest/next = 0.1.1-rc.2） | `Roaming\npm\node_modules\@deepseek-ai\dsh` |
| Git | 2.55.0.4 | WinGet MinGit |
| Python | 3.13.15 | `C:\Users\20183\Local\Programs\Python\Python313` |
| DSHLauncher | v3.0.0（局域网共享 + 全新手机端 UI + 只读模式 + 归档清理） | `D:\DSHLauncher` |

**Agent 与模型 API 引用**

| 项 | 值 |
|---|---|
| provider | `deepseek-official` |
| BASE URL（OpenAI 格式） | `https://api.deepseek.com` |
| BASE URL（Anthropic 格式） | `https://api.deepseek.com/anthropic` |
| API Key 环境变量 | `DEEPSEEK_API_KEY` |
| 默认模型 | `deepseek-v4-flash`（reasoningEffort: high） |
| 默认输出上限 | DSH 默认 `256K`（官方支持最大 384K） |

导入模型目录（0.1.2-alpha.2 复核与 0.1.1-rc.2 一致）：

| 模型 id | 上下文 | 输出上限 | 输入模态 |
|---|---|---|---|
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

### 1.9 附录 B：版本跟踪与破坏性变更速查

| 版本 | 标签 | 关键点 |
|---|---|---|
| 0.1.1-rc.2 | latest/next | JSONL 会话后端默认压缩改 `zstd`（注意混编码崩溃，故障 C）；内置 DeepSeek 模型目录（flash/pro/vision-exp） |
| 0.1.2-alpha.1 | （GitHub，未上架 npm） | **APIProxy 移除 → @Remote 网关**；pi-ai 模型支持更新 + vLLM 思考预算；统一 `dsh` Profile 启动；WebFetch 默认开启（SSRF 防护） |
| 0.1.2-alpha.2 | alpha（npm） | 含 alpha.1 全部变更；**Web 界面强制一次性 token 认证**（故障 D，启动器已适配）；恢复 `SessionEvent.ignorable`；RemoteError 统一封装；Node 24 启动修复 |
| 启动器 v3.0.0 | — | 局域网共享与全新手机端专属 UI（非 dsh 原生）：会话分组折叠、聊天（历史/加载更早/大纲导航）、只读模式（前端隐藏 + 网关 API 拦截）、会话过滤（归档/子代理/空白）、归档会话一键彻底清理、PIN 轮换踢出所有设备、SW 随机化自动刷缓存 |

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

---

# English

## Table of Contents
- [Introduction](#introduction)
- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [Prerequisites](#prerequisites)
- [Building from Source](#building-from-source)
- [Uninstall](#uninstall)
- [Repository Structure](#repository-structure)
- [License](#license)
- [Upgrade & Maintenance Manual (English)](#upgrade--maintenance-manual-english)

## Introduction

DeepSeek Harness is a web application (React frontend + Node.js service) that is normally opened in a browser — with all the memory overhead and distractions that brings. This launcher wraps Harness into a standard desktop app:

- **Embedded window**: renders the Harness UI directly inside the app window using Microsoft's official **WebView2** engine — no Edge, no browser tab, with a standard menu bar (Settings / Help / About).
- **Zero configuration**: double-click and go — auto-detect and start the dsh web service, auto-open the interface, auto-adopt leftover processes.
- **Background resident**: clicking ✕ minimizes to the system tray while the service keeps running; the tray menu can restore, stop, or fully quit anytime.

## Features

- 🖥️ **Embedded WebView2 UI** (no browser); auto-falls back to a lightweight Edge window, then the default browser
- 🎨 **Title bar / menu bar colors follow the Harness theme** (light/dark auto-adapt)
- ⚡ **One-click start**: double-click to auto-start the service and open the UI
- 🔄 **Auto-adopt**: recognizes and takes over an existing Harness process on the port (including leftover orphans)
- 🛡️ **Self-healing**: cleans stuck residual processes, 120-second startup timeout, auto-restart on hang (max 3 times)
- 🔑 **Token-auth adaptation (v2.0)**: automatically captures the one-time token URL printed by `dsh web` (required since dsh 0.1.2-alpha)
- 💤 **Keep service on exit (v2.0)**: quitting the launcher keeps dsh web running in the background; the web page stays connected and is auto-adopted next launch
- 📋 **Settings window**: port / working directory / minimize-to-tray on close / live log
- 📝 **Log file**: `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` (auto-trimmed beyond 2 MB)
- 🗂️ **Tray menu**: open / refresh / open in browser / start / stop / guide / open log folder / settings / about / quit
- 🧩 **Zero-dependency distribution**: single-file installer embeds the launcher and WebView2 runtime, auto-deploys missing prerequisites (Node.js / dsh / WebView2 Runtime)
- 📡 **LAN sharing & mobile UI (v3.0)**: phones/tablets on the same WiFi scan a QR code — OFF by default, identical to the old behavior when disabled
  - binds only the detected active WiFi/Ethernet **specific IP** (never 0.0.0.0), prints the address and renders a QR code (qrcode.js)
  - **brand-new standalone mobile UI** (not the dsh native UI): session list grouped by workspace (collapsible) + full chat (history, load-earlier on scroll, conversation outline nav, bottom composer)
  - **mobile read-only mode**: no new sessions / no workspace switch or add (hidden in UI + blocked at the gateway API, double protection)
  - **smart session filtering**: mobile shows only top-level user sessions (archived/subagent/blank hidden, matching the desktop)
  - **one-click archived-session purge** in Settings: deletes all archived session data from disk (chat history & file refs, **not recoverable**; takes effect after service restart)
  - PIN/Token gate (HttpOnly cookie; regenerating the PIN revokes every device) + per-IP rate limiting + session-secret rotation
  - zero-dependency Node gateway (embedded resource), SSE/WebSocket passthrough, PWA (manifest/SW/add-to-homescreen, randomized SW version auto-flushes caches)
  - auto Windows Firewall rule scoped to `remoteip=localsubnet`; copyable manual commands when elevation is missing
  - when LAN is enabled, sets `OLLAMA_HOST=0.0.0.0` and `OLLAMA_ORIGINS=*` for the dsh process (for local Ollama inference)

## Installation

Option 1: run `DSHLauncherSetup.exe` (one-click installer) — it detects the environment and deploys whatever is missing:
- Node.js → winget LTS install (falls back to the official MSI silent install)
- dsh → `npm install -g @deepseek-ai/dsh` (falls back to the current-user prefix on permission errors)
- **WebView2 Runtime** → auto-downloads and silently installs Microsoft's official Evergreen bootstrapper (required by the embedded UI)

Option 2: run `DSHLauncher.exe` directly (portable; the three WebView2 DLLs must sit next to it).

## Usage

1. Launch the app (or the **DeepSeek Harness Launcher** desktop shortcut).
2. The service starts automatically and the embedded window shows Harness; no control panel flashes — the app lives in the tray.
3. Click ✕ to minimize to the tray (when "minimize to tray on close" is checked); otherwise you are asked whether to stop the service and quit for real.
4. The menu bar "Settings" opens the settings window; "Help → Documentation" opens the built-in guide; "Open Log Folder" jumps to the log directory.

## Prerequisites

- Windows 10 / 11 (64-bit)
- Node.js LTS (the installer can deploy it automatically)
- dsh (`npm install -g @deepseek-ai/dsh`, the installer can deploy it automatically)
- **WebView2 Runtime** (usually ships with Edge on Win10/11; the installer can deploy it automatically, and the app falls back to Edge if it is unavailable)

## Building from Source

No tooling installation needed — Windows ships the .NET Framework compiler; the WebView2 SDK DLLs are vendored in `lib\`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1        # build DSHLauncher.exe
powershell -NoProfile -ExecutionPolicy Bypass -File build-setup.ps1  # build the installer (launcher + runtime)
```

Self-test (start → ready → stop end-to-end):

```powershell
.\selftest.ps1   # or DSHLauncher.exe --selftest
```

> ⚠️ Sources are UTF-8 (no BOM): run the build scripts with **PowerShell 7 (pwsh)**; under Windows PowerShell 5.1 use `-File` with console codepage 65001 (otherwise the Chinese comments may be misread as ANSI and cause parse errors).

## Uninstall

Run `uninstall.cmd` in the install directory (or uninstall via Settings → Apps); it cleans up files, the registry entry and the desktop icon.
`%APPDATA%\DSHLauncher\settings.ini` is kept, so reinstalling preserves your settings.

## Repository Structure

```
DSHLauncher.cs / DSHLauncherSetup.cs   source (.NET Framework 4.x WinForms, C# 5)
build.ps1 / build-setup.ps1            build scripts (system csc, zero-dependency compilation)
make-icon.ps1 / app.ico / assets/      icon generation & assets (official DeepSeek whale logo)
lib/                                   official WebView2 SDK DLLs (MIT licensed)
selftest.ps1                           self-test script
README.md / LICENSE                    this file (includes the upgrade & maintenance manual) and the license
```

## License

[MIT License](LICENSE) © 2026 KristoffersonLee

---

# Upgrade & Maintenance Manual (English)

> This manual covers installing, upgrading, verifying, and troubleshooting **dsh** (`@deepseek-ai/dsh`, the DeepSeek Harness CLI/service) and **DSHLauncher**.
> The body is generic (machine-independent); the appendices keep the publisher machine's snapshot and upgrade history for reference.
> Official dsh is still under fast preview/rc/alpha iteration and may introduce **breaking changes** (port/protocol/interface, config format, working-directory layout, etc.). Read sections 3, 4 and 7 before upgrading.

## Manual TOC

1. [Overview](#11-overview)
2. [Requirements & Path Conventions](#12-requirements--path-conventions)
3. [Installing & Upgrading dsh](#13-installing--upgrading-dsh)
4. [Post-Upgrade Verification Checklist](#14-post-upgrade-verification-checklist)
5. [Common Failures & Fixes](#15-common-failures--fixes)
6. [Launcher Behavior](#16-launcher-behavior)
7. [Maintenance Rules & Pitfalls](#17-maintenance-rules--pitfalls)
8. [Appendix A: Publisher Machine Snapshot (2026-08-31)](#18-appendix-a-publisher-machine-snapshot-2026-08-31)
9. [Appendix B: Version Tracking & Breaking Changes](#19-appendix-b-version-tracking--breaking-changes)
10. [Appendix C: Publisher Upgrade History](#110-appendix-c-publisher-upgrade-history)

### 1.1 Overview

- **dsh**: DeepSeek Harness's Node.js service and CLI. The web UI listens at `http://127.0.0.1:3080/` by default. Installed globally via npm (`@deepseek-ai/dsh`).
- **DSHLauncher**: a Windows desktop shell. Double-click to auto-start `dsh web` and display Harness in an embedded **WebView2** window (no browser needed), with tray, settings, logging, auto-adopt, and self-healing.

### 1.2 Requirements & Path Conventions

**Requirements**

| Component | Requirement |
|---|---|
| OS | Windows 10 / 11 (64-bit) |
| Node.js | Official LTS or newer (DSHLauncher starts dsh with the system Node) |
| npm | Official latest; **npm 12 blocks un-whitelisted install/postinstall scripts by default** (see pitfall in 1.7) |
| WebView2 Runtime | Usually ships with Edge; the launcher auto-deploys it or falls back to Edge |

**Path conventions (generic)**

| Purpose | Path (generic) | Note |
|---|---|---|
| npm global prefix | `npm config get prefix` (usually `%APPDATA%\npm` on Windows) | dsh installs at `<prefix>\node_modules\@deepseek-ai\dsh` |
| dsh user data | `%USERPROFILE%\.dsh\` | sessions (`sessions\`), config (`settings.yaml`), credentials (`.credentials.yaml`), plugins (`profiles\`) |
| Launcher log | `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` | first stop when troubleshooting |
| Launcher settings | `%APPDATA%\DSHLauncher\settings.ini` | port / working dir / tray behavior |
| Web UI | `http://127.0.0.1:3080/` | default port; **requires a one-time token since 0.1.2-alpha** (see 1.6) |

> Conventions: `<npm-prefix>` is the output of `npm config get prefix`; `<dsh-version>` is the target version.

### 1.3 Installing & Upgrading dsh

> May be executed by an AI agent following this manual, or manually with the commands below. Both are equivalent.

**Determine the target version (always first)**

```powershell
npm view @deepseek-ai/dsh dist-tags --json   # latest / next / alpha tags
npm view @deepseek-ai/dsh versions --json    # all published versions
```

- The GitHub releases page (`deepseek-ai/deepseek-harness`) may publish before npm; **npm availability is authoritative**.
- alpha/rc are prereleases: read the release notes of the target version and assess **breaking changes** (see Appendix B).

**Install / upgrade command**

```powershell
# 0) Confirm the npm prefix (must match the one the launcher uses)
npm config get prefix

# 1) Install a specific version (explicit version + anti-corrupt cache + allow native build scripts; do not omit)
npm install -g "@deepseek-ai/dsh@<dsh-version>" --no-audit --no-fund --prefer-online `
  --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
```

| Flag | Purpose |
|---|---|
| `@deepseek-ai/dsh@<version>` | **Always pin an explicit version** to avoid tag drift |
| `--prefer-online` | bypass a corrupted local npm cache tarball |
| `--allow-scripts=...` | **required on npm 12**: allow native build scripts for koffi (FFI), node-pty (terminal), etc.; otherwise you get a half-installed package (Failure B in 1.5) |
| `--no-audit --no-fund` | faster, quieter |

If the prefix/permissions are wrong: use the system npm with an explicit prefix, e.g.
`"C:\Program Files\nodejs\npm.cmd" install -g "@deepseek-ai/dsh@<version>" --prefix "<npm-prefix>" --no-audit --no-fund --prefer-online --allow-scripts=...`.
If the default npm-cache reports EPERM, add `--cache <writable-directory>`.

**Install discipline**

- Let the install **finish completely** (about 3–5 minutes); **do not kill processes mid-install** (interruptions once caused deadlocked workers and half-built directories).
- Upgrading only changes files on disk; a running dsh web is unaffected. **Restart the launcher afterwards** to load the new version.

### 1.4 Post-Upgrade Verification Checklist

Verify **every item** after install/reinstall:

```powershell
# 1) Version
dsh --version

# 2) Usage (exercises bin.js -> dsh-app-boot -> commander/js-yaml loading chain)
dsh --help

# 3) Plugin config tree (validates YAML parsing and full plugin-tree loading; normally 500+ lines, no error/mismatch)
dsh --profile web --dump-config

# 4) koffi native version match (critical!)
node -e "const k=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"

# 5) node-pty loadable
node -e "const p=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/node-pty'); console.log(typeof p.spawn)"
```

| Check | Expected |
|---|---|
| `dsh --version` | matches the installed version |
| koffi | prints a version consistent with the JS wrapper (e.g. `3.1.6`); `Mismatched native Koffi modules` means a broken install |
| node-pty | prints `function` |
| `dump-config` | no `error` / `mismatch` / `failed to` |
| Key files | under `<npm-prefix>\node_modules\@deepseek-ai\dsh\`: `package.json`, `lib\bin.js`, `node_modules\commander\index.js`, `node_modules\js-yaml\dist\js-yaml.mjs`, `node_modules\@koromix\koffi-win32-x64\win32_x64\koffi.node` |
| Leftovers | `<npm-prefix>\node_modules\@deepseek-ai\` should normally contain only `dsh` (see Failure E in 1.5) |
| Web UI | embedded window renders correctly after launcher restart (token since 0.1.2-alpha, see 1.6) |

### 1.5 Common Failures & Fixes

**Failure A: Missing modules (js-yaml / commander)**

- **Symptom**: `dsh` reports a missing module; the launcher cannot start.
- **Cause**: interrupted install or wrong prefix, leaving a half-built directory.
- **Fix**: delete the install directory and reinstall completely (1.3), let it finish; if the prefix was wrong, reinstall with an explicit `--prefix`.

**Failure B: koffi native mismatch (Mismatched native Koffi modules)**

- **Symptom**: the launcher throws `Mismatched native Koffi modules` when loading the `subprocess`/`sandbox` plugins, exit code 1 — the launcher "won't open".
- **Cause**: npm 12's allow-scripts policy blocked koffi's build scripts → JS and native binaries out of sync.
- **Fix**: reinstall completely with `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` (1.3), then verify koffi (1.4). **Never try to replace a single .node file.**

**Failure C: Session encoding mismatch (uses .jsonl, but this backend is configured for compression "zstd")**

- **Symptom**: the launcher log reports `encodingMismatch`, exit code 1; `dsh --version` / `dump-config` work fine.
- **Cause**: since 0.1.1-rc.2 the session backend defaults to `compression: zstd`; a session directory that **mixes encodings** (plaintext `session.jsonl` alongside `session.jsonl.zstd`) crashes initialization. **Reinstalling does not help** (the default stays zstd).
- **Fix (unify the root to zstd)**:
  1. Locate conflicts: count plaintext vs zstd files under `%USERPROFILE%\.dsh\sessions`;
  2. For each conflicting directory: first decode the first zstd frame to confirm the session id matches and the full session is there (proving zstd is authoritative); **back up the plaintext `session.jsonl` OUTSIDE `.dsh`**, then delete the plaintext, keeping only `session.jsonl.zstd`;
  3. ⚠️ **Never back up inside `.dsh`** (dsh scans that tree as session roots, and a plaintext backup retriggers the same crash);
  4. Confirm zero plaintext files under `.dsh\sessions`, then restart the launcher.
- **Alternative**: if all sessions are plaintext and you want to keep them, append `- id: session-persistence-jsonl / config: { compression: none }` to `%USERPROFILE%\.dsh\profiles\web\cordis.patch.yml` (only when there are no zstd sessions).

**Failure D: One-time token auth on the Web UI since 0.1.2-alpha (401)**

- **Symptom**: `http://127.0.0.1:3080/` returns **401**; the embedded window shows `dsh web authentication required; reopen the URL printed by dsh web`; the log shows `dsh web: http://127.0.0.1:3080/?token=…` (different every launch).
- **Cause**: since 0.1.2-alpha, `dsh-client-connection` enforces **one-time token auth** on the Web UI (a new token per launch; after first visit it exchanges for a 30-day browser-session cookie). There is **no config switch to disable it**.
- **Fix**: use the token URL printed by `dsh web`; **DSHLauncher v2.0+ captures that URL automatically** — nothing manual needed. Upgrade old launchers.
- **Key points**: the token is one-time and changes every launch; the browser-session cookie lasts 30 days by default, so reopening the launcher auto-adopts (see 1.6) without re-authentication; if the cookie expires and you get 401, restart the service once.

**Failure E: Launcher won't open / unresponsive**

1. Stop the launcher and confirm the crashed process exited (`Get-NetTCPConnection -LocalPort 3080`); use `Get-CimInstance Win32_Process` to inspect command lines and **do not kill unrelated node processes** (e.g. other tools' MCP/agent).
2. Clean leftover temp directories:
   ```powershell
   Get-ChildItem "<npm-prefix>\node_modules\@deepseek-ai" -Force   # should normally contain only dsh
   Remove-Item "<leftover-path>" -Recurse -Force                    # e.g. .dsh-* (contains sharp DLLs)
   ```
   Leftovers locked by a running launcher can be deleted **after restarting the launcher**.
3. Delete the broken install (`Remove-Item "<npm-prefix>\node_modules\@deepseek-ai\dsh" -Recurse -Force`), reinstall completely (1.3), verify (1.4), then restart the launcher.

**Failure F: Common false alarms (not problems)**

| Phenomenon | Note |
|---|---|
| `UNMET OPTIONAL DEPENDENCY @img/sharp-*` (darwin/linux/freebsd) in `npm ls -g` | not installed on Windows; normal |
| `EPERM` writing `cordis.yml` | usually another instance holds the port / sandbox restrictions; not a corrupt config |
| `npm warn cleanup Failed to remove .dsh-*` | temp dir locked by a running process; deletable after restart |

### 1.6 Launcher Behavior

- **Embedded window**: WebView2 renders Harness (no browser); falls back to a lightweight Edge window when unavailable.
- **Tray menu**: open UI / refresh / open in browser / start service / stop service / guide / log folder / settings / about / quit.
- **Token-auth adaptation (v2.0+)**: the launcher captures the `/?token=…` URL from `dsh web` output and navigates to it; older dsh versions without that output fall back to the plain URL (compatible).
- **Service lifecycle (v2.0+)**:
  - Quitting the launcher **keeps dsh web running in the background by default** — the web page stays connected; the next launch auto-detects and adopts it;
  - To stop the service: tray "Stop service", or choose "Yes" in the close-window prompt;
  - Clicking ✕ (when "minimize to tray on close" is checked) hides to the tray; the app and service keep running;
  - Adoption relies on the browser-session cookie (30 days by default); if it expires and a relaunch gets 401, restart the service once.
- **Log**: `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` (auto-trimmed beyond 2 MB).

### 1.7 Maintenance Rules & Pitfalls

1. **The global npm prefix must match the one the launcher uses.** Confirm with `npm config get prefix` first; under multi-Node environments (e.g. another tool's managed Node) `npm` may point at a different prefix — the safest way is the system npm with an explicit `--prefix`.
2. **npm 12's allow-scripts policy silently breaks native modules** (root cause of Failure B): always install dsh with `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs`, then verify koffi immediately.
3. **Always verify integrity after upgrade/reinstall** (checklist in 1.4), especially the koffi native version and `dump-config`.
4. **Never install while killing processes**; let the install finish; if you must interrupt, check for leftover processes first.
5. **Assess breaking changes before upgrading**: read the target release notes (see Appendix B); protocol/config changes (e.g. token auth, APIProxy removal) may affect the launcher and model config.
6. **Pin explicit versions** to avoid tag drift.
7. **Credential safety**: API keys live in `%USERPROFILE%\.dsh\.credentials.yaml` — **never commit them to the repo or write them into documents**; upgrading dsh does not change credentials.

### 1.8 Appendix A: Publisher Machine Snapshot (2026-08-31)

> The following records the publisher machine (Windows, user dir `C:\Users\20183`) — **reference only, not generic requirements**.

**Version baseline**

| Component | Version | Location |
|---|---|---|
| Node.js | v26.7.0 | `C:\Program Files\nodejs` |
| npm | 12.0.2 | prefix `C:\Users\20183\AppData\Roaming\npm` |
| pnpm | 11.22.0 | same |
| @deepseek-ai/dsh | **0.1.2-alpha.2** (npm `alpha` tag; latest/next = 0.1.1-rc.2) | `Roaming\npm\node_modules\@deepseek-ai\dsh` |
| Git | 2.55.0.4 | WinGet MinGit |
| Python | 3.13.15 | `C:\Users\20183\Local\Programs\Python\Python313` |
| DSHLauncher | v3.0.0 (LAN sharing + standalone mobile UI + read-only mode + archive purge) | `D:\DSHLauncher` |

**Agent & model API references**

| Item | Value |
|---|---|
| provider | `deepseek-official` |
| BASE URL (OpenAI format) | `https://api.deepseek.com` |
| BASE URL (Anthropic format) | `https://api.deepseek.com/anthropic` |
| API key env var | `DEEPSEEK_API_KEY` |
| Default model | `deepseek-v4-flash` (reasoningEffort: high) |
| Default output cap | dsh default `256K` (official max 384K) |

Imported model catalog (re-verified on 0.1.2-alpha.2, identical to 0.1.1-rc.2):

| Model id | Context | Output cap | Input modalities |
|---|---|---|---|
| `deepseek-v4-flash` | 1M | 384K | text |
| `deepseek-v4-pro` | 1M | 384K | text |
| `deepseek-v4-flash-vision-exp` | 1M | 384K | text + image (experimental; not listed by `/list-models` but callable directly) |
| `LongCat-2.0` | 1M | — | text (custom provider: `https://api.longcat.chat/openai/v1`, `LONGCAT_API_KEY`) |

Credential / API key references (secrets live in `C:\Users\20183\.dsh\.credentials.yaml`, never committed):

| Env var | Purpose | Note |
|---|---|---|
| `DEEPSEEK_API_KEY` | deepseek-official (chat + web search) | required |
| `LONGCAT_API_KEY` | longcat (LongCat-2.0) | required when using LongCat |
| `ZHIPU_API_KEY` / `AGNES_API_KEY` | (reserved) | no provider configured, unused |

### 1.9 Appendix B: Version Tracking & Breaking Changes

| Version | Tag | Key points |
|---|---|---|
| 0.1.1-rc.2 | latest/next | JSONL session backend default compression changed to `zstd` (watch mixed-encoding crashes, Failure C); built-in DeepSeek model catalog (flash/pro/vision-exp) |
| 0.1.2-alpha.1 | (GitHub only, not on npm) | **APIProxy removed → @Remote gateway**; pi-ai model support updates + vLLM thinking budget; unified `dsh` Profile startup; WebFetch enabled by default (SSRF protection) |
| 0.1.2-alpha.2 | alpha (npm) | all alpha.1 changes; **one-time token auth on the Web UI** (Failure D, launcher adapted); restored `SessionEvent.ignorable`; unified RemoteError; Node 24 startup fix |

> Check with `npm view @deepseek-ai/dsh dist-tags`; release notes at `https://github.com/deepseek-ai/deepseek-harness/releases`.

### 1.10 Appendix C: Publisher Upgrade History

| Date | Action | Result |
|---|---|---|
| 2026-08-19 | full env check; npm 11.19→12.0.2; pnpm 11.22.0; fixed dsh rc.7 integrity (js-yaml.mjs/commander missing) | ✅ |
| 2026-08-20 | dsh rc.7 → rc.8 (`next` tag) | ⚠️ koffi native mismatch crash → fixed via `--prefer-online` + allowed-scripts reinstall (koffi 3.1.6 / 503-line plugin tree) |
| 2026-08-22 | dsh rc.8 → 0.1.1-rc.2 (built-in V4-Flash-Vision-Exp registration) | ✅ 14-item verification PASS (koffi 3.1.6 / 514-line plugin tree) |
| 2026-08-30 | re-check: npm latest/next = 0.1.1-rc.2; GitHub released 0.1.2-alpha.1 (not on npm yet) → hold | ✅ stayed on rc.2 |
| 2026-08-30 (fix) | fixed session-encoding mismatch crash (zstd/plaintext, Failure C); backups moved outside `.dsh` | ✅ |
| 2026-08-30 (p2) | workflow change: removed one-click upgrade script; "one sentence triggers agent per manual"; cleaned root | ✅ |
| 2026-08-31 | dsh 0.1.1-rc.2 → **0.1.2-alpha.2** (npm alpha tag); token-auth breaking change → launcher adaptation (Failure D) | ✅ |
| 2026-08-31 (fix) | launcher behavior fix: exit keeps dsh web running (web stays connected); adoption relies on 30-day cookie | ✅ |
| 2026-08-31 (p2) | version bumped to v2.0.0; manual merged into README (single document shipped); installer ships README; portable uninstaller | ✅ |
| 2026-09-01 | v2.0.0 → **v3.0.0**：局域网共享 + 全新手机端专属 UI（会话分组/聊天/只读模式/会话过滤/归档清理）；版本号一致性检查并更新 README | ✅ |
