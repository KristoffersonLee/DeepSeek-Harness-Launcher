# DeepSeek Harness Launcher v3.0.0

> 此文件为 v3.0.0 GitHub Release 的发布说明（中英双语），可直接复制到 Release 页面正文。
> This file holds the v3.0.0 GitHub Release notes (bilingual) — copy it into the Release body.

## 更新说明 / What's New

### 中文

**🆕 新功能**

- **局域网共享与手机端专属 UI**：手机/平板在同一 WiFi 下扫码即可访问（默认关闭，行为与旧版一致）
  - 只绑定检测到的具体 IP（绝不绑定 0.0.0.0）；PIN 门禁（可自定义）+ 速率限制（防伪造绕过）+ 会话密钥轮换；防火墙限定 `remoteip=localsubnet`
- **全新独立移动端 UI**（非 dsh 原生）：会话列表按工作区分组折叠、完整聊天（历史 / 上滑加载更早 / 对话大纲跳转 / 底部输入栏）、只读模式（前端隐藏 + 网关 API 拦截双重保障）
- **归档会话彻底清理**：设置面板一键删除归档会话全部数据（不可恢复，重启生效）

**🔧 修复与优化**

- **全面审查修复**（详见 docs/AUDIT-REPORT.md）：归档清理正则失效、token/PIN 明文日志、网关限速可伪造绕过、WebView2 环境复用、PIN 自定义失效、升级安装误杀 dsh web、安装窗口假死、下载容错、Node 版本/PATH 刷新等
- **手机端修复**：subagent 注入消息不再冒充用户消息（大纲/聊天干净）；对话大纲独立分页（可加载到最早、跳转自动定位、时间正序显示）；分组名/会话标题两行完整显示
- **运行日志 UTF-8 修复**：中文不再乱码（dsh web 与网关进程显式 UTF-8 解码）
- **dsh 升级至 0.1.2-rc.1**：Session persistence API 内部变更（SessionHandle + session lock）不影响外部 JSON-RPC 契约，手机 UI 与网关零适配
- **文档重构**：README 精简为双语主文档；升级维护手册拆分 `docs/MAINTENANCE.zh.md` / `docs/MAINTENANCE.en.md`；新增 `docs/CHANGELOG.md`

### English

**🆕 New Features**

- **LAN sharing & standalone mobile UI**: phones/tablets on the same WiFi scan a QR code to access (off by default; binds only the concrete IP, never 0.0.0.0; customizable PIN gate + forgery-proof rate limiting + session-secret rotation; firewall scoped to `remoteip=localsubnet`)
- **Brand-new standalone mobile UI** (not the dsh native UI): collapsible grouped session list, full chat (history / load-earlier / outline navigation / composer), read-only mode (hidden in UI + blocked at the gateway API, double protection)
- **One-click archived-session purge** in Settings (deletes all archived data, not recoverable; takes effect after restart)

**🔧 Fixes & Improvements**

- Full audit fixes (archive purge, plaintext token/PIN logs, rate-limit bypass, WebView2 environment reuse, PIN customization, upgrade install killing dsh web, frozen installer UI, download resilience, Node version/PATH refresh, etc.)
- Mobile fixes: subagent-injected messages no longer masquerade as user messages; paginated standalone outline (load to earliest, auto-locate on jump, chronological order); full two-line group/session titles
- UTF-8 process-output fix (no more garbled Chinese in the log — explicit UTF-8 decoding for dsh web & gateway)
- dsh upgraded to 0.1.2-rc.1 (Session persistence API internal change (SessionHandle + session lock) does not affect external JSON-RPC contract — mobile UI and gateway need no adaptation)
- Docs rework: slim bilingual README; maintenance manual split into `docs/MAINTENANCE.zh.md` / `docs/MAINTENANCE.en.md`; new `docs/CHANGELOG.md`

## 兼容性 / Compatibility

- dsh ≥ 0.1.1-rc.2（推荐 0.1.2-rc.1）；Windows 10/11 64 位；WebView2 运行时（可自动部署）
- dsh ≥ 0.1.1-rc.2 (0.1.2-rc.1 recommended); Windows 10/11 64-bit; WebView2 Runtime (auto-deployable)

## 资源 / Assets

- 安装包 / Installer：`DSHLauncherSetup.exe`（内嵌 WebView2 运行库、README 与中英双语维护手册 / embeds the WebView2 runtime, the README and the bilingual maintenance manuals）
- 文档 / Docs：`README.md` + `docs/MAINTENANCE.zh.md` / `docs/MAINTENANCE.en.md` / `docs/CHANGELOG.md`
