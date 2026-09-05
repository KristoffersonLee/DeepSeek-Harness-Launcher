# DeepSeek Harness Launcher

一个为 **DeepSeek Harness** 打造的 Windows 桌面启动器：双击图标即自动启动 dsh web 服务，并在内嵌的 **WebView2** 窗口中直接显示 Harness —— 无需浏览器，像原生软件一样使用。

A Windows desktop launcher for **DeepSeek Harness**: double-click to automatically start the dsh web service and view Harness directly in an embedded **WebView2** window — no browser needed, used like a native app.

**作者 / Author: [KristoffersonLee](https://github.com/)** · **v4.0.0**

> ⚠️ **重要说明 / Important Notice**
>
> 本工具基于 **DeepSeek Harness 官方预览版**构建。官方预览版仍处快速迭代，未来可能发布**破坏性更新**（端口/协议/接口、配置格式、工作目录结构等），届时本启动器可能无法兼容，**本项目也可能随之停止维护**。请知悉后再决定是否使用。
>
> This launcher is built on top of the **official preview release of DeepSeek Harness**, which is still evolving rapidly and may introduce **breaking changes** (port/protocol/interface, config format, working-directory layout, etc.). In that case this launcher may become incompatible, and **this project may be discontinued**. Please be aware of this before using it.

**[中文说明](#中文说明) | [English](#english)**

> 📖 升级、dsh 维护与故障排查见独立手册：**[中文维护手册](MAINTENANCE.zh.md)** · **[English Maintenance Manual](MAINTENANCE.en.md)**
>
> For upgrading dsh, maintenance and troubleshooting, see the standalone manuals: **[中文维护手册](MAINTENANCE.zh.md)** · **[English Maintenance Manual](MAINTENANCE.en.md)**

---

# 中文说明

## 目录

- [简介](#简介)
- [功能特性](#功能特性)
- [安装](#安装)
- [使用](#使用)
- [局域网共享（手机 / 平板扫码访问）](#局域网共享手机--平板扫码访问)
- [环境要求](#环境要求)
- [从源码构建](#从源码构建)
- [卸载](#卸载)
- [目录结构](#目录结构)
- [许可证](#许可证)

## 简介

DeepSeek Harness 本身是网页应用（React 前端 + Node.js 服务），通常需要在浏览器中打开并忍受浏览器的内存开销与干扰。本启动器把 Harness 包装成一个标准桌面应用：

- **内嵌窗口**：用微软官方 WebView2 引擎把 Harness UI 直接渲染在应用窗口里（不启动 Edge、不经过你的主浏览器），并带标准菜单栏（设置 / 帮助 / 关于）。
- **零配置**：双击即用 —— 自动检测并启动 dsh web、自动打开界面、自动接管上次遗留的进程。
- **后台常驻**：点 ✕ 收进系统托盘，服务继续运行；托盘菜单可随时恢复、停止或彻底退出。

## 功能特性

- 🖥️ **内嵌 WebView2 界面**（无需浏览器）；不可用时自动回退 Edge 精简窗口，再回退默认浏览器
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
  - 只绑定当前活跃的 WiFi/以太网**具体 IP**（绝不绑定 0.0.0.0），自动打印地址并生成二维码
  - **全新独立移动端 UI**：会话列表（按工作区分组折叠）+ 完整聊天（历史、上滑加载更早、对话大纲跳转、底部输入栏）
  - **手机端只读模式**：不能新建会话 / 切换或添加工作区（前端隐藏 + 后端 API 拦截双重保障）
  - **会话列表智能过滤**：只显示顶级用户会话（自动隐藏归档 / 子代理 / 空白会话）
  - **归档会话彻底清理**：设置面板一键删除归档会话的全部历史数据（不可恢复；重启生效）
  - **PIN/Token 门禁**（HttpOnly Cookie，重生成 PIN 自动踢出所有设备）+ **速率限制** + 会话密钥轮换
  - 零依赖 Node 网关（内嵌资源），SSE / WebSocket 流式透传，PWA 增强（manifest / SW / 添加到主屏幕）
  - Windows 防火墙自动放行（`remoteip=localsubnet`，仅局域网）；无管理员权限时给出可复制的手动命令
  - 开启局域网后为 dsh 进程设置 `OLLAMA_HOST=0.0.0.0`、`OLLAMA_ORIGINS=*`（若使用 Ollama 本地推理，局域网内的 Harness 网关即可调用模型接口）
- 🔍 **全量审阅修复（v4.0.0）**：probeReady 竞态条件（代际号）、UpgradeDsh 管道死锁（异步排空 + 5 分钟超时）、升级回调进程崩溃守卫、cache-bust 硬编码 IP、WebView2 检测优化（单层扫描）、下载退避重试、Node 多版本选择一致性、卸载脚本增强（防火墙规则 + %APPDATA% 清理 + settings.ini 保留）

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

> 原理：**手机连接的是启动器内嵌的 Web 网关，而不是 WebView2 界面**。网关（`lan-gateway.mjs`，零依赖 Node 实现，随启动器内嵌）把 `dsh web`（默认仅监听 127.0.0.1）安全地暴露给同一 WiFi。

### 开启步骤

1. 设置窗口 →「局域网共享」→ 勾选"允许局域网访问（默认关闭）"，按提示确认（请确保处于可信网络，不要在公共网络开启）。
2. 自动检测活动网卡 IP（优先 WiFi/以太网，排除 VPN/虚拟网卡）并绑定该具体 IP；**访问 PIN 可自定义**（在面板输入后失焦即保存，留空则自动生成 6 位；也可通过环境变量 / `.env` 的 `DSH_LAN_PIN` 指定）；自动尝试添加防火墙规则（仅本地子网）。
3. 手机连同一 WiFi，用相机 / 微信扫描二维码 → 输入 PIN → 进入**全新移动端专属 UI**（会话分组列表、聊天、只读模式、大纲导航、发送消息）。

### 安全机制

- **默认关闭，显式开启**：关闭状态下手机无法访问，行为与旧版完全一致；
- **PIN 门禁**：首次访问必须输入 PIN（环境变量 → 启动器目录 `.env` → `%APPDATA%\.env` → lan-pin.txt 4 级解析，绝不硬编码），验证通过写入 `HttpOnly; SameSite=Strict` Cookie；
- **速率限制**：按来源 IP 滑窗计数（整体 / API / 登录分别限流），超限返回 429；
- **最小暴露面**：网关只绑定检测到的具体局域网 IP，不绑定 0.0.0.0；防火墙规则限定 `remoteip=localsubnet`；
- **双层认证**：网关 PIN 之外，dsh 自身的启动令牌认证依然生效（网关自动兑换，手机无感知）。

### Windows 防火墙（自动 + 手动）

自动配置失败（无管理员权限）时，可复制面板手动命令，或运行：

```powershell
# 放行（管理员 PowerShell）
netsh advfirewall firewall add rule name="DSHLauncher LAN 3081" dir=in action=allow protocol=TCP localport=3081 remoteip=localsubnet
# 关闭时清理
netsh advfirewall firewall delete rule name="DSHLauncher LAN 3081"
```

> 💡 请把当前 WiFi 网络设置为「专用」网络，配合 `remoteip=localsubnet` 规则进一步降低暴露面。

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

运行安装目录里的 `uninstall.cmd`（或"设置 → 应用"里卸载），会清理安装文件、注册表项、桌面图标、防火墙规则、局域网凭据（PIN / 令牌 / 会话密钥 / `.env`）以及运行日志与内嵌浏览器缓存（`%LOCALAPPDATA%\DSHLauncher`）；
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
docs/MAINTENANCE.zh.md / MAINTENANCE.en.md
                                        升级与维护手册（中英双语，随安装包部署到安装目录）
selftest.ps1                           自检脚本
uninstall.cmd                          卸载脚本（安装目录内的同名文件由安装包生成，本文件为仓库模板）
README.md / LICENSE                    本文件与许可证
```

## 许可证

[MIT License](LICENSE) © 2026 KristoffersonLee

---

# English

## Table of Contents

- [Introduction](#introduction)
- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [LAN Sharing (Phone / Tablet Access)](#lan-sharing-phone--tablet-access)
- [Prerequisites](#prerequisites)
- [Building from Source](#building-from-source)
- [Uninstall](#uninstall)
- [Repository Structure](#repository-structure)
- [License](#license)

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
- 🗂️ **Tray menu**: open / refresh / open in browser / start / stop / guide / log folder / settings / about / quit
- 🧩 **Zero-dependency distribution**: single-file installer embeds the launcher and WebView2 runtime, auto-deploys missing prerequisites (Node.js / dsh / WebView2 Runtime)
- 📡 **LAN sharing & mobile UI (v3.0)**: phones/tablets on the same WiFi scan a QR code — OFF by default, identical to the old behavior when disabled
  - binds only the detected active WiFi/Ethernet **specific IP** (never 0.0.0.0), prints the address and renders a QR code
  - **brand-new standalone mobile UI**: session list grouped by workspace (collapsible) + full chat (history, load-earlier on scroll, outline navigation, bottom composer)
  - **mobile read-only mode**: no new sessions / no workspace switch or add (hidden in UI + blocked at the gateway API, double protection)
  - **smart session filtering**: shows only top-level user sessions (archived/subagent/blank hidden)
  - **one-click archived-session purge** in Settings: deletes all archived session data from disk (**not recoverable**; takes effect after service restart)
  - PIN/Token gate (HttpOnly cookie; regenerating the PIN revokes every device) + per-IP rate limiting + session-secret rotation
  - zero-dependency Node gateway (embedded resource), SSE/WebSocket passthrough, PWA (manifest/SW/add-to-homescreen)
  - auto Windows Firewall rule scoped to `remoteip=localsubnet`; copyable manual commands when elevation is missing
  - when LAN is enabled, sets `OLLAMA_HOST=0.0.0.0` and `OLLAMA_ORIGINS=*` for the dsh process (for local Ollama inference; allows the LAN gateway to reach the model API)
- 🔍 **Full audit fixes (v4.0.0)**: probeReady race condition (generation counter), UpgradeDsh pipe deadlock (async drain + 5-min timeout), upgrade callback crash guard, cache-bust hardcoded IP, WebView2 detection optimization (single-level scan), download backoff retry, Node multi-version selection consistency, uninstaller enhancement (firewall + %APPDATA% cleanup + settings.ini preserved)

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
5. (Optional) Settings → **LAN sharing** → enable "Allow LAN access", confirm the prompt, then scan the QR code with your phone.

## LAN Sharing (Phone / Tablet Access)

> How it works: **the phone talks to the launcher's embedded Web gateway, not the WebView2 UI**. The gateway (`lan-gateway.mjs`, a zero-dependency Node implementation embedded in the launcher) safely exposes `dsh web` (which listens on 127.0.0.1 only) to the same WiFi.

### Enable steps

1. Settings → **LAN sharing** → check "Allow LAN access (off by default)" and confirm (make sure you are on a trusted network; do not enable on public networks).
2. The active WiFi/Ethernet IP is auto-detected (VPN/virtual adapters excluded) and bound; **the PIN is customizable** (type it in the panel and it saves on focus loss; leave it empty for an auto-generated 6-digit PIN; `DSH_LAN_PIN` via environment / `.env` also works); a firewall rule scoped to the local subnet is attempted automatically.
3. On the phone (same WiFi) scan the QR code with the camera / WeChat → enter the PIN → enter the **standalone mobile UI** (grouped session list, chat, read-only mode, outline navigation, composer).

### Security model

- **Off by default, explicit opt-in**: phones cannot connect while the switch is off; behavior is identical to older versions.
- **PIN gate**: first visit requires the PIN (resolved from environment → launcher-dir `.env` → `%APPDATA%\.env` → lan-pin.txt, 4 levels, never hardcoded); success writes an `HttpOnly; SameSite=Strict` cookie.
- **Rate limiting**: sliding window per source IP (total / API / login separately); 429 when exceeded.
- **Minimal exposure**: the gateway binds only the detected concrete LAN IP, never 0.0.0.0; the firewall rule is scoped to `remoteip=localsubnet`.
- **Double authentication**: besides the gateway PIN, dsh's own one-time token auth still applies (the gateway exchanges it automatically; phones never notice).

### Windows Firewall (automatic + manual)

When the automatic rule fails (no admin rights), copy the manual command from the panel, or run:

```powershell
# Allow (admin PowerShell)
netsh advfirewall firewall add rule name="DSHLauncher LAN 3081" dir=in action=allow protocol=TCP localport=3081 remoteip=localsubnet
# Clean up when disabled
netsh advfirewall firewall delete rule name="DSHLauncher LAN 3081"
```

> 💡 Set the current WiFi network to "Private" and combine it with the `remoteip=localsubnet` rule to further reduce exposure.

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

Run `uninstall.cmd` in the install directory (or uninstall via Settings → Apps); it cleans up the installed files, the registry entry, the desktop icon, the firewall rule, LAN credentials (PIN / token / session secret / `.env`) and the logs & embedded-browser caches under `%LOCALAPPDATA%\DSHLauncher`.
`%APPDATA%\DSHLauncher\settings.ini` is kept, so reinstalling preserves your settings.

## Repository Structure

```
DSHLauncher.cs / LanAccess.cs           launcher source & LAN helper layer (.NET Framework 4.x WinForms, C# 5)
DSHLauncherSetup.cs                     one-click installer source
lan-gateway.mjs                         LAN gateway (zero-dependency Node, embedded at build time)
build.ps1 / build-setup.ps1             build scripts (system csc, zero-dependency compilation)
make-icon.ps1 / app.ico / whale-256.png / assets/
                                        icon generation & assets (official DeepSeek whale logo; whale-256.png is the embedded mobile PWA icon)
lib/                                    official WebView2 SDK DLLs (MIT licensed)
docs/MAINTENANCE.zh.md / MAINTENANCE.en.md
                                        upgrade & maintenance manual (bilingual, deployed to the install directory by the installer)
selftest.ps1                            self-test script
uninstall.cmd                           uninstall script (the installer generates its own copy; this file is the repo template)
README.md / LICENSE                     this file and the license
```

## License

[MIT License](LICENSE) © 2026 KristoffersonLee
