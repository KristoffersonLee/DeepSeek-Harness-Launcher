# DeepSeek Harness Launcher v4.0.0

> 此文件为 v4.0.0 GitHub Release 的发布说明（中英双语），可直接复制到 Release 页面正文。
> This file holds the v4.0.0 GitHub Release notes (bilingual) — copy it into the Release body.

## 更新说明 / What's New

### 中文

**🔒 安全修复**

- **防火墙规则卸载残留**：`netsh delete rule name=` 不支持通配符，导致卸载后防火墙规则泄漏。改用 `Get-NetFirewallRule` + `Remove-NetFirewallRule`，确保所有端口规则被彻底清理
- **WebSocket upgrade 数据丢失**：`proxyUpgrade()` 未转发客户端 `head` 数据，导致首帧 WebSocket 消息损坏或截断。添加 `psocket.write(head)` 正确转发
- **PowerShell 命令注入**：`RunFirewallElevated()` 中路径直接嵌入双引号字符串，用户名含 `$()` 可被解释为脚本块在 UAC 提权下执行。改用单引号 + `Replace("'", "''")` 转义
- **Token URL 泄露**：dsh 启动令牌通过 URL 查询参数发送，可能出现在服务器访问日志、代理日志中。改用 `X-DSH-Token` 自定义 Header 发送

**🔧 修复与优化**

- **全量代码审阅修复**（约 20 项）：
  - **probeReady 竞态条件**：引入代际号（generation counter），服务重启时递增，陈旧探测结果自动丢弃，避免 UI 误判新服务就绪
  - **UpgradeDsh 管道死锁**：npm install 输出远超 4KB 管道缓冲，改为异步排空 stdout/stderr + 5 分钟超时强制终止兜底，升级功能不再卡死
  - **升级回调进程崩溃**：窗体关闭后线程池线程调用 Invoke 抛 ObjectDisposedException 导致整个进程崩溃，补 IsDisposed 双重守卫
  - **cache-bust 硬编码 IP**：遗留开发数据 192.168.5.5 硬编码在提示页，改用运行时 HOST 变量
  - **WebView2 检测优化**：AllDirectories 递归扫描整个 EdgeWebView 目录（数十万文件）→ 只扫一级版本目录，检测从秒级降到毫秒级；添加 ProgramFiles 回退检查以兼容 ARM64/32 位系统
  - **下载退避重试**：注释承诺"退避重试"但实际无间隔，加入 1s/2s 真实退避
  - **Node 多版本选择一致性**：启动器 Engine.Resolve 取枚举首个 node-v* 目录（NTFS 不保证版本序），同步为安装包 Env.Detect 的最大版本逻辑
  - **卸载脚本增强**：添加防火墙规则清理 + %APPDATA% 凭据清理 + %LOCALAPPDATA% 日志/缓存清理，保留 settings.ini
  - **安装向导按钮逻辑**：btnNext 条件简化为 !deploying && !installing，修复安装中反而启用的逻辑错误
- **netsh 输出编码修复**：硬编码 `Encoding.UTF8` 导致中文 Windows（GBK 代码页）下"已启用""提升"等关键词匹配失败。改为 `Encoding.Default`（系统 ANSI 代码页）
- **上游超时客户端挂起**：`doProxy()` 和 `proxyUpgrade()` 的超时处理仅销毁上游连接，未向客户端返回错误响应，导致客户端永久挂起。添加 504/502 错误响应
- **未处理 Promise 拒绝**：`ensureDshCookie().then()` 无 `.catch()` 处理器，兑换失败时客户端 socket 挂起。添加 `.catch()` 返回 500 错误
- **Token 兑换竞态条件**：并发请求同时触发兑换时，后续调用立即返回 false 导致不必要的 502。改用共享 Promise 单飞模式，所有并发方等待同一次兑换结果
- **OS 位数检测**：`PROCESSOR_ARCHITECTURE` 返回当前进程位数（32 位进程在 64 位系统上返回 x86），导致下载错误的 MSI。改用 `Environment.Is64BitOperatingSystem`
- **FindPidOnPort 性能**：优先使用 `IPGlobalProperties.GetActiveTcpListeners()`（毫秒级响应），netstat 子进程作为回退方案
- **安装向导 Finish 逻辑**：`else if` 导致勾选"打开新手指引"时跳过"立即启动"。改为两个独立 `if`，两者均可执行

**🧹 冗余精简**

- **RunProcess 死代码**：移除 `RunProcess()` 中永不读取的 `StringBuilder buf` 及 `lock` 语句
- **ExtractResource 重复注释**：移除方法上方的重复注释
- **RunCommand 死分支**：移除从未执行的 `elevated` 分支（所有调用方传 `elevated=false`）
- **selftest.log**：删除生成产物，不再提交到仓库

**📋 一致性修正**

- **版本号统一**：v4.0.0（两处 AppVersion / 安装向导标题 / 注册表 DisplayVersion / README / CHANGELOG）
- **README 文档路径**：`docs/MAINTENANCE.zh.md` → `MAINTENANCE.zh.md`（与安装包实际部署位置一致）
- **README 版本号**：功能列表中 "v4.0" → "v4.0.0"，与项目其他版本引用统一
- **卸载行为**：与 README 承诺一致（保留 settings.ini，重装后配置不丢失）
- **uninstall.cmd 路径传递**：通过环境变量 `$env:UNINSTALL_DIR` 传递路径，避免路径含单引号时 PowerShell 解析失败

### English

**🔒 Security Fixes**

- **Firewall rule leak on uninstall**: `netsh delete rule name=` doesn't support wildcards, leaving stale inbound rules behind. Switched to `Get-NetFirewallRule` + `Remove-NetFirewallRule` for complete cleanup
- **WebSocket upgrade data loss**: `proxyUpgrade()` silently dropped the client's buffered frame data (`head`), corrupting or truncating the first WebSocket messages. Now forwards via `psocket.write(head)`
- **PowerShell command injection**: In `RunFirewallElevated()`, the path was embedded in a double-quoted string, allowing a malicious username containing `$()` to execute as a script block at UAC-elevated privilege. Switched to single-quote escaping with `Replace("'", "''")`
- **Token URL exposure**: The dsh one-time auth token was sent via URL query parameter, potentially appearing in server access logs and proxy logs. Switched to `X-DSH-Token` custom header

**🔧 Fixes & Improvements**

- **Full code audit fixes (~20 items)**:
  - **probeReady race condition**: introduced generation counter, incremented on service restart, stale probe results auto-discarded to prevent UI misjudgment
  - **UpgradeDsh pipe deadlock**: npm install output far exceeds 4KB pipe buffer; switched to async stdout/stderr draining + 5-min timeout kill fallback
  - **Upgrade callback crash**: Invoke after form close threw ObjectDisposedException crashing the process; added IsDisposed double guard
  - **cache-bust hardcoded IP**: leftover dev data 192.168.5.5 hardcoded in cache-bust page; replaced with runtime HOST variable
  - **WebView2 detection optimization**: AllDirectories recursive scan (hundreds of thousands of files) → single-level version directory scan, detection dropped from seconds to milliseconds; added `ProgramFiles` fallback for ARM64/32-bit compatibility
  - **Download backoff retry**: comment promised "backoff retry" but had no delay; added real 1s/2s backoff
  - **Node multi-version selection consistency**: launcher Engine.Resolve took first node-v* directory (NTFS doesn't guarantee version order); synced to installer Env.Detect max-version logic
  - **Uninstaller enhancement**: added firewall rule cleanup + %APPDATA% credential cleanup + %LOCALAPPDATA% log/cache cleanup; preserves settings.ini
  - **Installer wizard button logic**: simplified btnNext condition to !deploying && !installing; fixed logic error where button was enabled during install
- **netsh output encoding**: Hardcoded `Encoding.UTF8` broke Chinese keyword matching (e.g. "已启用", "提升") on Chinese Windows (GBK code page). Switched to `Encoding.Default` (system ANSI code page)
- **Upstream timeout hangs client**: The timeout handlers in `doProxy()` and `proxyUpgrade()` only destroyed the upstream connection without sending an error response, leaving clients hanging forever. Now returns 504/502 errors
- **Unhandled promise rejection**: `ensureDshCookie().then()` had no `.catch()` handler, leaving client sockets hanging when token exchange failed. Added `.catch()` returning 500 error
- **Token exchange race condition**: When concurrent requests triggered token exchange simultaneously, subsequent callers returned false immediately, causing spurious 502 errors. Switched to shared promise (single-flight) pattern — all concurrent callers await the same exchange result
- **OS bitness detection**: `PROCESSOR_ARCHITECTURE` returns the current process bitness (a 32-bit process on 64-bit Windows returns x86), causing the wrong MSI to download. Switched to `Environment.Is64BitOperatingSystem`
- **FindPidOnPort performance**: Now uses `IPGlobalProperties.GetActiveTcpListeners()` (ms-level response) first, with netstat subprocess as fallback
- **Installer Finish logic**: `else if` meant checking "show guide" skipped "launch app". Now two independent `if` statements, both can execute

**🧹 Redundancy Cleanup**

- **Dead code in RunProcess**: Removed `StringBuilder buf` and `lock` statements that were never read after process exit
- **Duplicate comment in ExtractResource**: Removed redundant comment above the method
- **Dead branch in RunCommand**: Removed never-executed `elevated` branch (all callers pass `elevated=false`)
- **selftest.log**: Removed generated artifact from repository

**📋 Consistency Fixes**

- **Version unified**: v4.0.0 (both AppVersion consts / installer wizard title / registry DisplayVersion / README / CHANGELOG)
- **README doc paths**: `docs/MAINTENANCE.zh.md` → `MAINTENANCE.zh.md` to match actual installer deployment layout
- **README version**: "v4.0" → "v4.0.0" in features list for consistency with all other version references
- **Uninstall behavior**: Matches README promise (preserves settings.ini, config survives reinstall)
- **uninstall.cmd path handling**: Passes path via environment variable `$env:UNINSTALL_DIR` to avoid PowerShell parsing issues with single quotes in paths

## 兼容性 / Compatibility

- dsh ≥ 0.1.1-rc.2（推荐 0.1.2-rc.1）；Windows 10/11 64 位；WebView2 运行时（可自动部署）
- dsh ≥ 0.1.1-rc.2 (0.1.2-rc.1 recommended); Windows 10/11 64-bit; WebView2 Runtime (auto-deployable)

## 资源 / Assets

- 安装包 / Installer：`DSHLauncherSetup.exe`（内嵌 WebView2 运行库、README 与中英双语维护手册 / embeds the WebView2 runtime, the README and the bilingual maintenance manuals）
- 启动器 / Launcher：`DSHLauncher.exe`（绿色版，需同目录 WebView2 三个 DLL / portable, requires three WebView2 DLLs alongside）
- 文档 / Docs：`README.md` + `MAINTENANCE.zh.md` / `MAINTENANCE.en.md` / `docs/CHANGELOG.md` / `docs/RELEASE_NOTES_v4.0.0.md`
