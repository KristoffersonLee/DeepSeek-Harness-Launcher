# DeepSeek Harness Launcher

一个为 **DeepSeek Harness** 打造的 Windows 桌面启动器：双击图标即自动启动 dsh web 服务，并在内嵌的 **WebView2** 窗口中直接显示 Harness —— 无需浏览器，像原生软件一样使用。

A Windows desktop launcher for **DeepSeek Harness**: double-click to automatically start the dsh web service and view Harness directly in an embedded **WebView2** window — no browser needed, used like a native app.

**作者 / Author: [KristoffersonLee](https://github.com/)** · v1.0.0

**[中文说明](#中文说明) | [English](#english)**

---

# 中文说明

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
- 📋 **设置窗口**：端口 / 工作目录 / 关闭时最小化到托盘 / 实时运行日志
- 📝 **日志文件**：`%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`（超 2MB 自动裁剪）
- 🗂️ **托盘菜单**：打开界面 / 刷新 / 浏览器打开 / 启动 / 停止 / 新手指引 / 打开日志目录 / 设置 / 关于 / 退出
- 🧩 **零依赖分发**：单文件安装包内嵌启动器与 WebView2 运行库，自动部署缺失环境（Node.js / dsh / WebView2 运行时）

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

## 卸载

运行安装目录里的 `uninstall.cmd`（或"设置 → 应用"里卸载），会清理文件、注册表项与桌面图标；
`%APPDATA%\DSHLauncher\settings.ini` 设置会保留，重装后不丢失。

## 目录结构

```
DSHLauncher.cs / DSHLauncherSetup.cs   源码（.NET Framework 4.x WinForms，C# 5）
build.ps1 / build-setup.ps1            构建脚本（系统自带 csc，零依赖编译）
make-icon.ps1 / app.ico / assets/      图标生成与资源（官方 DeepSeek 鲸鱼 LOGO）
lib/                                   WebView2 SDK 官方 DLL（MIT 许可）
selftest.ps1                           自检脚本
README.md / LICENSE                    本文件与许可证
```

## 许可证

[MIT License](LICENSE) © 2026 KristoffersonLee

---

# English

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
- 📋 **Settings window**: port / working directory / minimize-to-tray on close / live log
- 📝 **Log file**: `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` (auto-trimmed beyond 2 MB)
- 🗂️ **Tray menu**: open / refresh / open in browser / start / stop / guide / open log folder / settings / about / quit
- 🧩 **Zero-dependency distribution**: single-file installer embeds the launcher and WebView2 runtime, auto-deploys missing prerequisites (Node.js / dsh / WebView2 Runtime)

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
README.md / LICENSE                    this file and the license
```

## License

[MIT License](LICENSE) © 2026 KristoffersonLee
