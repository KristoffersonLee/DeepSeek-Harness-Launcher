# Changelog / 更新日志

## v4.0.0 (2026-09-05)

### 🔧 修复与优化 / Fixes & Improvements

- **全面审阅修复**：probeReady 竞态条件（代际号）、UpgradeDsh 管道死锁（异步排空）、升级回调进程崩溃守卫（IsDisposed）、cache-bust 硬编码 IP、WebView2 检测优化（单层扫描）、下载退避重试、Node 多版本选择一致性、卸载脚本增强（防火墙规则 + %APPDATA% 清理）
  - Full audit fixes: probeReady race (generation counter), UpgradeDsh pipe deadlock (async drain), upgrade callback crash guard (IsDisposed), cache-bust hardcoded IP, WebView2 detection optimization (single-level scan), download backoff retry, Node multi-version selection consistency, uninstaller enhancement (firewall + %APPDATA% cleanup)
- **一致性修正**：版本号统一为 v4.0.0；卸载行为与 README 一致（保留 settings.ini）；安装包/卸载器同步
  - Consistency: version unified to v4.0.0; uninstall behavior matches README (preserves settings.ini); installer/uninstaller sync

## v3.0.0 (2026-09-01)

### 🆕 新功能 / New Features

- **局域网共享与手机端专属 UI**：手机/平板在同一 WiFi 下扫码即可访问（默认关闭，行为与旧版一致）
  - LAN sharing & standalone mobile UI: phones/tablets on the same WiFi scan a QR code to access (off by default, identical to old behavior when disabled)
- **全新独立移动端 UI**（非 dsh 原生）：会话列表按工作区分组折叠、完整聊天（历史、上滑加载更早、对话大纲跳转、底部输入栏）、只读模式（前端隐藏 + 网关 API 拦截双重保障）
  - Brand-new standalone mobile UI (not the dsh native UI): collapsible grouped session list, full chat (history, load-earlier, outline navigation, composer), read-only mode (hidden in UI + blocked at the gateway API)
- **PIN/Token 门禁 + 安全加固**：HttpOnly Cookie、速率限制（防 X-Forwarded-For 伪造）、会话密钥轮换、只绑定具体 IP 绝不绑定 0.0.0.0、防火墙限定 `remoteip=localsubnet`
  - PIN/Token gate + security hardening: HttpOnly cookie, rate limiting (forgery-proof), session-secret rotation, binds only the concrete IP (never 0.0.0.0), firewall scoped to `remoteip=localsubnet`
- **归档会话彻底清理**：设置面板一键删除归档会话全部数据（不可恢复，重启生效）
  - One-click archived-session purge in Settings (not recoverable; takes effect after restart)

### 🔧 修复与优化 / Fixes & Improvements

- **全面审查修复**（详见 docs/AUDIT-REPORT.md）：归档清理正则失效、token/PIN 明文日志、网关限速绕过、WebView2 环境复用、PIN 自定义失效、升级安装误杀 dsh web、安装窗口假死、下载容错、Node 版本/PATH 等
  - Full audit fixes (see docs/AUDIT-REPORT.md): archive-purge regex, plaintext token/PIN logs, rate-limit bypass, WebView2 env reuse, PIN customization, upgrade killing dsh web, frozen installer UI, download resilience, Node version/PATH, etc.
- **手机端修复**：subagent 注入消息不再冒充用户消息（大纲/聊天干净）；对话大纲独立分页（可加载到最早、跳转自动定位）；大纲时间正序显示；分组名/会话标题两行完整显示
  - Mobile fixes: subagent-injected messages no longer masquerade as user messages; paginated standalone outline (load to earliest, auto-locate on jump); chronological outline order; full two-line group/session titles
- **运行日志 UTF-8 修复**：中文不再乱码（dsh web 与网关进程显式 UTF-8 解码）
  - UTF-8 process-output fix: no more garbled Chinese in the log (explicit UTF-8 decoding for dsh web & gateway)
- **dsh 升级至 0.1.2-rc.1**：Session persistence API 内部变更（SessionHandle + session lock）不影响外部 JSON-RPC 契约，手机 UI 与网关零适配
  - dsh upgraded to 0.1.2-rc.1: Session persistence API internal change (SessionHandle + session lock) does not affect external JSON-RPC contract — mobile UI and gateway need no adaptation
- **最终审查修复**：401 重试不再丢失响应、改 PIN 后网关强制重启（旧 PIN/Cookie 立即失效）、大纲"加载更早"可达且节点去重、SSE 长静默不再被超时掐断、速率限制数值校验、日志脱敏全覆盖等 18 项
  - Final audit fixes: 401 retry no longer loses the response, gateway hard-restart on PIN change (old PIN/cookies invalidated immediately), outline load-earlier reachable with dedup, SSE no longer cut by idle timeout, rate-limit value validation, full log redaction coverage, and more (18 items)
- **文档重构**：README 精简为双语主文档，升级维护手册拆分至 docs/MAINTENANCE.zh.md / MAINTENANCE.en.md
  - Docs rework: README slimmed to a bilingual main doc; the maintenance manual moved to docs/MAINTENANCE.zh.md / MAINTENANCE.en.md

## v2.0.0 (2026-08-31)

- **token 认证适配**：自动捕获 `dsh web` 的一次性 token URL（dsh 0.1.2-alpha 强制认证）
  - Token-auth adaptation: auto-captures the one-time token URL from `dsh web` (mandatory since dsh 0.1.2-alpha)
- **退出保留服务**：退出启动器默认保留 dsh web 后台运行，网页端不中断；下次打开自动接管
  - Keep service on exit: quitting keeps dsh web running; the web page stays connected and is auto-adopted next launch
- **手册并入 README**（单文档随发布）；安装包部署 README；卸载脚本通用化（任意安装位置可用）
  - Manual merged into README (single doc ships with release); installer ships README; portable uninstaller

## v1.0.0 (2026-08-14)

- **首个发布**：内嵌 WebView2 桌面启动器（无需浏览器）、自动启动/接管/自愈、托盘常驻、零依赖构建（系统 csc）
  - First release: embedded WebView2 desktop launcher (no browser), auto start/adopt/self-heal, tray resident, zero-dependency build (system csc)
