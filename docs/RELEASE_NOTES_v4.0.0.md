# DeepSeek Harness Launcher v4.0.0

> 此文件为 v4.0.0 GitHub Release 的发布说明（中英双语），可直接复制到 Release 页面正文。
> This file holds the v4.0.0 GitHub Release notes (bilingual) — copy it into the Release body.

## 更新说明 / What's New

### 中文

**🔧 修复与优化**

- **全量代码审阅修复**（约 20 项）：
  - **probeReady 竞态条件**：引入代际号（generation counter），服务重启时递增，陈旧探测结果自动丢弃，避免 UI 误判新服务就绪
  - **UpgradeDsh 管道死锁**：npm install 输出远超 4KB 管道缓冲，改为异步排空 stdout/stderr + 5 分钟超时强制终止兜底，升级功能不再卡死
  - **升级回调进程崩溃**：窗体关闭后线程池线程调用 Invoke 抛 ObjectDisposedException 导致整个进程崩溃，补 IsDisposed 双重守卫
  - **cache-bust 硬编码 IP**：遗留开发数据 192.168.5.5 硬编码在提示页，改用运行时 HOST 变量
  - **WebView2 检测优化**：AllDirectories 递归扫描整个 EdgeWebView 目录（数十万文件）→ 只扫一级版本目录，检测从秒级降到毫秒级
  - **下载退避重试**：注释承诺"退避重试"但实际无间隔，加入 1s/2s 真实退避
  - **Node 多版本选择一致性**：启动器 Engine.Resolve 取枚举首个 node-v* 目录（NTFS 不保证版本序），同步为安装包 Env.Detect 的最大版本逻辑
  - **卸载脚本增强**：添加防火墙规则清理（DSHLauncher LAN *）+ %APPDATA% 凭据清理 + %LOCALAPPDATA% 日志/缓存清理，保留 settings.ini
  - **安装向导按钮逻辑**：btnNext 条件简化为 !deploying && !installing，修复安装中反而启用的逻辑错误
- **一致性修正**：
  - 版本号统一升级为 v4.0.0（两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG）
  - 卸载行为与 README 承诺一致（保留 settings.ini，重装后配置不丢失）
  - 仓库 uninstall.cmd 模板与 WriteUninstallCmd 生成逻辑完全同步
- **冗余精简**：删除遗留产物 dump-config.txt 与 selftest.log；清理错位/失准注释

### English

**🔧 Fixes & Improvements**

- **Full code audit fixes (~20 items)**:
  - **probeReady race condition**: introduced generation counter, incremented on service restart, stale probe results auto-discarded to prevent UI misjudgment
  - **UpgradeDsh pipe deadlock**: npm install output far exceeds 4KB pipe buffer; switched to async stdout/stderr draining + 5-min timeout kill fallback
  - **Upgrade callback crash**: Invoke after form close threw ObjectDisposedException crashing the process; added IsDisposed double guard
  - **cache-bust hardcoded IP**: leftover dev data 192.168.5.5 hardcoded in cache-bust page; replaced with runtime HOST variable
  - **WebView2 detection optimization**: AllDirectories recursive scan (hundreds of thousands of files) → single-level version directory scan, detection dropped from seconds to milliseconds
  - **Download backoff retry**: comment promised "backoff retry" but had no delay; added real 1s/2s backoff
  - **Node multi-version selection consistency**: launcher Engine.Resolve took first node-v* directory (NTFS doesn't guarantee version order); synced to installer Env.Detect max-version logic
  - **Uninstaller enhancement**: added firewall rule cleanup (DSHLauncher LAN *) + %APPDATA% credential cleanup + %LOCALAPPDATA% log/cache cleanup; preserves settings.ini
  - **Installer wizard button logic**: simplified btnNext condition to !deploying && !installing; fixed logic error where button was enabled during install
- **Consistency fixes**:
  - Version unified to v4.0.0 (both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG)
  - Uninstall behavior matches README promise (preserves settings.ini, config survives reinstall)
  - Repo uninstall.cmd template fully synced with WriteUninstallCmd generation logic
- **Redundancy cleanup**: removed leftover dump-config.txt and selftest.log; cleaned up misplaced/inaccurate comments

## 兼容性 / Compatibility

- dsh ≥ 0.1.1-rc.2（推荐 0.1.2-rc.1）；Windows 10/11 64 位；WebView2 运行时（可自动部署）
- dsh ≥ 0.1.1-rc.2 (0.1.2-rc.1 recommended); Windows 10/11 64-bit; WebView2 Runtime (auto-deployable)

## 资源 / Assets

- 安装包 / Installer：`DSHLauncherSetup.exe`（内嵌 WebView2 运行库、README 与中英双语维护手册 / embeds the WebView2 runtime, the README and the bilingual maintenance manuals）
- 文档 / Docs：`README.md` + `docs/MAINTENANCE.zh.md` / `docs/MAINTENANCE.en.md` / `docs/CHANGELOG.md` / `docs/RELEASE_NOTES_v4.0.0.md`
