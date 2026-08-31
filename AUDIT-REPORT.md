# DSHLauncher 3.0 全面审查报告

> ⚠️ **修复状态（2026-09-01）**：本报告为审查基线。报告中的全部问题（C1-C4、M1-M18、第三部分 minor）
> 已在本轮修复中处理完毕：代码修复、文档修正、构建脚本修正、工程卫生清理均已完成，
> 并通过编译验证与自检（详见 git 提交信息）。本文件保留作为审查与修复的对照记录。

> 审查对象：D:\DSHLauncher 全部源码与文件（DSHLauncher.cs 3149 行、LanAccess.cs 613 行、lan-gateway.mjs 894 行、DSHLauncherSetup.cs 1094 行、README.md 845 行、构建/自检/图标/卸载脚本、.gitignore、LICENSE、assets）
> 审查方式：主导代理全量通读 + 5 个子代理分文件深度审查 + 构建验证 + 跨文件一致性比对（git diff / 资源名 / 端口 / 路径 / 命令）
> 验证基线：DSHLauncher.cs+LanAccess.cs 与 DSHLauncherSetup.cs 均可编译通过；运行中的 DSHLauncher.exe（PID 14348）与当前源码构建产物逐字节一致（160,768 B），即审查对象即发布形态。

---

## 一、必须修复（critical / 高危）

### C1. 清理归档会话时磁盘数据根本删不掉（LanAccess.cs:209-212）
`DeleteArchivedSessions` 内层正则 `"""session-[a-f0-9-]+"""`（字面含引号）**没有捕获组**，`im.Groups[1].Value` 恒为空串 → `ids` 全部是空串 → `ids.Contains(name)` 永远 false → **磁盘上的会话目录一个都删不掉**；但 L233 仍会把 workspace.json 的归档列表清空。用户以为"已彻底删除"，数据却完好留在磁盘；且 `deleted==0` 使调用方跳过服务重启，JSON 改动可能被 dsh 退出时回写覆盖，造成状态不一致。
**修复**：`ids.Add(im.Value.Trim('"'));`（`im.Value` 含引号，去引号后即为 sessionId）。

### C2. dsh 认证 token 与局域网 PIN 明文写入日志文件（DSHLauncher.cs:784, 1444, 1480）
`OnServerOutput` 把 dsh web 打印的完整行（含 `dsh web: http://127.0.0.1:3080/?token=...`）经 `Log` 落盘到 `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`；`Log("已自动生成访问 PIN：" + pin)` 与 `Log("局域网共享已开启: ...（PIN: " + pin + "）")` 也把 PIN 明文写日志。token 是会话认证凭证、PIN 是局域网访问凭证，日志长期保留（仅 2MB 裁剪）。
**修复**：写日志前脱敏（正则替换 `?token=[^&\s]+` 与 `PIN: \S+`），或 PIN/token 只显示在设置面板、不写日志文件。

### C3. 局域网网关速率限制可被 X-Forwarded-For 伪造绕过 → PIN 可被暴力破解（lan-gateway.mjs:134-139）
`clientIp()` 无条件信任客户端可伪造的 `X-Forwarded-For` 且取第一个值。网关直接面对局域网客户端（非可信代理），攻击者每次请求伪造不同 IP 即可绕过 PIN 登录限速（10 次/分/IP）、API 限速与整体限速。配合 6 位数字 PIN（10^6 组合），同 WiFi 攻击者可暴力破解后拿到全部会话/工作区数据。
**修复**：默认只使用 `socket.remoteAddress`；确需支持代理时用环境变量（如 `DSH_LAN_TRUST_PROXY=1`）显式开启并校验直连对端。

### C4. 安装包 DSHLauncherSetup.exe 过期——装出来是 v2.0（工程/发布级）
现有 `DSHLauncherSetup.exe`（2026-08-31 01:49）内嵌的 `DSHLauncher.exe` 仅 74,752 B（当前源码构建 160,768 B）、README 29,504 B（当前 56,865 B），不含 lan-gateway.mjs / whale-256.png 资源；程序集名 `DSHLauncherSetup-new` 证明它并非本仓库 build-setup.ps1 产出。用它安装将得到不含 v3.0 局域网功能的旧版。
**修复**：发布前重跑 `build-setup.ps1`（会自动先构建新启动器再打包），并把"产物与源码同步"纳入发布检查清单。

---

## 二、建议修复（major）

### M1. 设置窗口定时刷新覆盖用户正在输入的 PIN/端口（DSHLauncher.cs:2559-2560, 2513-2516）
`RefreshLanPanel` 每 1.5s 无条件把 `txtLanPin.Text = host.UiLanPin`、`txtLanPort.Text = host.UiLanPortText`。用户编辑这两个框时输入被持续覆盖；且 `txtLanPin.Leave`/`txtLanPort.Leave` 依赖框内文本与旧值比对，被覆盖后比对恒相等 → **用户改动静默丢失**。
**修复**：控件聚焦（`Focused`）时跳过该字段刷新。

### M2. WebView2 同进程复用同一 user-data-folder，二次初始化失败被静默吞掉（DSHLauncher.cs:2578-2580, 2769-2771）
设置窗口每次重建都用同一目录 `webview2-profile-settings` 创建环境，内嵌窗口用 `webview2-profile`。同一进程内对同一 user-data-folder 重复创建环境会失败，而失败在 `t.IsFaulted` 分支被静默 return → 重开设置后二维码空白、内嵌窗口卡"正在初始化"。
**修复**：缓存 `CoreWebView2Environment`（static）复用，或每次生成唯一 profile 目录名；失败时至少 `host.Log` 原因。

### M3. 清空 PIN 输入框不删 lan-pin.txt，旧 PIN 仍然生效（DSHLauncher.cs:1666-1674）
`trimmedPin.Length == 0` 时只置 `settings.LanPin = ""`，但 `LanAccess.EffectivePin` 仍从 lan-pin.txt 读出旧 PIN（来源还显示"启动器自动生成"）→ 用户以为已改回自动生成，实际访问密码没变，文案误导。
**修复**：清空时同步 `File.Delete(LanAccess.PinFilePath)` 并轮换会话密钥（与 RegenerateLanPin 一致）。

### M4. Harness 端口变更后局域网网关 DSH_TARGET 陈旧，手机端静默失联（DSHLauncher.cs:608-617, 1463, 1331-1334）
`CommitSettings` 改端口只重启 server；网关的 `DSH_TARGET` 在启动瞬间捕获，`MaybeStartLanGateway` 见网关存活即 return → 网关继续代理旧端口，LAN 全部 502 且无日志提示。
**修复**：端口变更且 LAN 开启时先 `StopLanGateway()` 再 `StartLanGateway()`。

### M5. 401 "重试"逻辑是空操作（lan-gateway.mjs:728-743）
`res.once('finish')` 在响应**已完整发给客户端后**才触发，`res.destroy()` 对已结束的响应无意义，请求从未重发（`retried` 恒 false）。dsh 重启令牌失效后，手机端首个请求必 401 并误跳登录页（用户以为 PIN 错了）。
**修复**：收到目标 401 时销毁当前代理响应并重发请求（限 1 次）；请求前先确认 `dshCookie` 存在。

### M6. WebSocket upgrade：目标返回非 101 时客户端永久挂起（lan-gateway.mjs:755-783）
`proxy` 无 `'response'` 监听，目标对 upgrade 请求返回 401/500 等普通响应时 `'upgrade'` 事件不触发 → 客户端 socket 不写响应也不关闭，手机端无限"连接中"。
**修复**：增加 `proxy.on('response')` 处理器（转发状态行+头部+body 后 destroy）并设超时。

### M7. Service Worker 缓存所有 GET，包括 SSE 流式响应（lan-gateway.mjs:205-214）
SSE 是 GET，会被 SW 拦截：`res.clone()` 对永不结束的流持续挂起（内存/缓存泄漏）；断网时 `caches.match` 回放不完整/过期的流。认证页面也被离线缓存。
**修复**：跳过 `/api/` 路径，并检查响应 `content-type` 含 `text/event-stream` 时不写缓存。

### M8. Engine.Resolve 无异常保护，可能在 UI 线程崩溃（DSHLauncher.cs:1113, 1356-1364）
`Resolve` 中 `Directory.GetDirectories`、PATH 遍历可能抛 `UnauthorizedAccessException`/`IOException`；`StartServer`/`StartLanGateway` 里的调用在 try 块外 → 托盘菜单/定时器回调中的未处理异常会崩溃。
**修复**：调用移入 try，失败转 `Log` + `SetState(Error)`。

### M9. 提权防火墙脚本：固定路径、不清理、TOCTOU（LanAccess.cs:485-548）
`firewall-add.ps1`/`firewall-remove.ps1` 写到固定可预测路径 `%APPDATA%\DSHLauncher\`，从不清理、执行前无完整性校验 → 同用户恶意进程可在 UAC 弹窗前替换脚本内容，用户点"是"后任意代码以管理员运行（用户态→管理员提权原语）。
**修复**：脚本末尾 `Remove-Item $MyInvocation.MyCommand.Path` 自删 + 随机文件名，或改用 `-Command` 内联。

### M10. 健康检查与规则检测误判（LanAccess.cs:122, 443-446）
- `IsGatewayRunning` 未设 `req.Proxy = null`，局域网 IP 探测会走系统代理（Clash/企业代理）→ 误判"已有网关在运行"导致局域网功能启动失败且提示误导。
- `HasRule` 用子串 `"enabled"/"启用"` 判断，已禁用规则（"Enabled: No"/"已启用: 否"）也命中 → 误判已有规则、跳过添加 → 防火墙未放行却显示"已配置"。
**修复**：`req.Proxy = null;`；规则存在性改匹配 `"Enabled: Yes"`/`"已启用: 是"` 或解析键值。

### M11. 客户端断连不传播到上游；上游请求无超时（lan-gateway.mjs:688/726/660/782, 166/668/704/759）
`req.pipe(proxy)` 后无 `aborted`/`close` 处理 → 手机端中途退出时 dsh 仍继续执行（LLM 生成空转）；`http.request` 无 timeout、`exchangeToken` 的 fetch 无 AbortSignal → dsh 挂起时网关连接/内存累积。
**修复**：`req.on('aborted', () => proxy.destroy())`、`res.on('close', ...)`；设 `timeout` 与 `AbortSignal.timeout`。

### M12. 升级安装时 taskkill /T 连带杀掉正在服务的 dsh web（DSHLauncherSetup.cs:517-518）
`StopLauncherInDir` 用 `taskkill /PID <pid> /T /F` 按进程树强杀。启动器的 dsh web 服务与 lan-gateway 都是其**子进程**，升级/覆盖安装时会把正在服务的 Harness 会话连同内存状态一并杀掉、浏览器端立即断开——与 build.ps1:13-15 明文设计（"结束启动器不要带 /T，dsh web 留在后台继续运行"）冲突，也与 L499/L553 注释"仅结束旧版启动器"不符。
**修复**：去掉 `/T` 只杀启动器本体（卸载脚本里的 `/T` 是合理的整树清理，保留）。

### M13. DoInstall 在 UI 线程同步执行整个安装（DSHLauncherSetup.cs:1033-1066）
6 个资源解压 + 8 次 reg.exe 子进程（每次最长 15s）全部在 UI 线程同步跑 → 窗口全程"未响应"、进度条冻结，用户易误判死机而强杀造成半截安装。
**修复**：仿照 `BtnDeployAllClick` 的后台线程模式（L979-1009），`BeginInvoke` 回 UI 更新。

### M14. 环境部署下载零容错（DSHLauncherSetup.cs:200-203, 305-307）
`WebClient` 默认超时仅 100s，Node MSI（约 35MB）弱网直接失败，无超时设置、无重试；临时文件名固定，并发/残留互相覆盖；下载内容无哈希校验。
**修复**：显式 `wc.Timeout`（300s）+ 退避重试 + 文件名随机化 + SHA256 校验（至少最小体积检查）。

### M15. Node 目录回退会选到最旧的补丁版；PATH 不刷新（DSHLauncherSetup.cs:258-264, 196-210）
nodejs.org nginx 目录按字典序排列，`Regex.Match` 取第一个匹配可能是通道内最旧补丁版（应取版本最大）；Node 装好后 `Detect()` 读当前进程 PATH，MSI/winget 写机器 PATH 不生效 → "部署成功但检测失败"。
**修复**：收集全部匹配按版本号取最大；从 `HKLM\...\Session Manager\Environment` 重读 PATH 合并后再 Detect。

### M16. 资源解压非原子（DSHLauncherSetup.cs:530-542）
直接 `File.Create` 覆盖写，中途失败留半截损坏 exe 且无回滚；`taskkill /F` 未 `WaitForExit` 就立即解压，仍可能报"正由另一进程使用"。
**修复**：先写 `*.tmp` 再 `File.Move` 覆盖；kill 后轮询等待退出。

### M17. 生成的 uninstall.cmd 带 UTF-8 BOM（DSHLauncherSetup.cs:568）
生成版 `new UTF8Encoding(true)` 写 BOM，而内容纯 ASCII；cmd.exe 在部分环境下首行 `ï»¿@echo off` 报错、`@echo off` 失效（会继续执行但闪错误行）。根目录 uninstall.cmd 无 BOM。
**修复**：`new UTF8Encoding(false)`；并把生成版已具备的 `'`/`%` 转义回灌根模板（根文件遇含 `'` 的路径会截断 PS 字符串）。

### M18. 其余安装包问题（DSHLauncherSetup.cs）
- 安装主流程异常处理缺口（L473-495：`Directory.CreateDirectory`、`WriteUninstallCmd`/`RegisterUninstall`/`CreateShortcut` 在 try 外；`ExtractResource` 缺资源抛裸 Exception 不被捕获）。
- `RegisterUninstall` 串行 8 次 reg.exe（改用 `RegistryKey` API 直写 + 补 `EstimatedSize`/`QuietUninstallString`）。
- `DetectOnly` 退出码忽略 WebView2（L457），与检测输出语义不一致。
- `--silent-install` 目录参数未拒绝盘符根目录、未 `GetFullPath` 归一化（L40-41）。
- `Directory.GetDirectories(p, "node-v*")` 取第一个即停，多版本可能选旧 Node（L89-93）。
- `StandardOutputEncoding` 为 .NET 4.5+ API（L345-346）：实际因 4.5+ in-place 升级可编译通过（已验证），但头注释"系统自带 csc v4.0.30319"定位应注明最低 .NET 4.5。
- setup.log 写在 exe 旁，只读目录时静默丢失（L404）——失败时回退 %TEMP%。
- 精简：`600000` 魔法数×5、注册键/快捷方式通配名重复、`UiLog` 与 `InstallLog` 重复（后者无截断）、`InstallRunning` 冗余包装、资源名清单与 build-setup.ps1 双处维护、头注释漏掉 3 个 WebView2 DLL 与 README。

---

## 三、精简优化与工程卫生（minor / 清理）

### 文档与文案
1. **README.md:103-104 重复句 + 编号错乱**（"5." 与 "4." 内容相同）——删除重复行。
2. **README:109 "3 级解析"** 实为 4 级（环境变量 → 启动器目录 .env → %APPDATA%.env → lan-pin.txt）——改措辞。
3. **README:154 "只显示 16 个顶级会话"** 与代码不符（无 16 上限）——删数字。
4. **README:60/102/518 "含备份"** 与代码不符（无备份逻辑）且与对话框"不可恢复"矛盾——改措辞。
5. **README:64/140-141 OLLAMA 措辞**：实际是"开启局域网后无条件为 dsh 进程设置"，不检测是否真的用 Ollama。
6. **README 目录结构遗漏** whale-256.png 与 uninstall.cmd。
7. **内置引导/日志引用不存在的 UI**（DSHLauncher.cs:277/958/999/1021/1857/2968/2973/2975/2977-2978）："一键启动"按钮（托盘实为"启动服务"）、"Node…"控件（设置窗口无此入口）、"取消勾选则改用 Edge 精简窗口"（LiteBrowser 复选框不存在）——统一改为托盘/设置的真实操作指引。
8. **GuideText 超长字面量**（约 40 行）可提取为资源文件。
9. **DSHLauncher.cs:1745 注释过时**："重启 dsh 服务让注册表变更生效"——实际改的是 workspace.json。
10. **Ollama 面板文案误导**（L2391-2395）：LAN 关闭时并未设置 OLLAMA_HOST，文案却恒显"已自动设置"。
11. **移动 UI "只读模式" footer 文案**（lan-gateway.mjs:311）与可发消息的实际行为不符——改"可继续对话 · 不能新建/切换/管理工作区"。

### 代码精简
12. **死设置**：`AutoStart`/`AutoOpen`/`LiteBrowser`（settings.ini 字段，无行为）——删除或恢复对应行为。
13. **死代码**：`OnStartClick`（未挂接）、`HideLauncherOnOpen`（空壳+3 调用）、`UiLanPinEffective`（未用）、`lanUrl`（只写不读）、`TryRemoveRuleElevated`（无调用方）、`RunCommand` 的 elevated 分支（三处调用均传 false）、DSHLauncher.cs:1446-1449 空 else-if（`"启动器自动生成"` 魔法字符串跨文件耦合）。
14. **重复逻辑**：`Env.Detect`（setup）与 `Engine.Resolve`（launcher）两份 node/npm/dsh 探测几乎相同；`WriteGateway` 与 `LoadWhaleIconB64` 资源查找+读取可抽公共辅助；`RefreshLanPanel`/`OnLogLine` 的 `InvokeRequired` 样板可抽 `SafeUi(Action)`。
15. **魔法值**：LanAccess 超时 900、DSHLauncher.cs:2565 `lp=3081`（与 Settings 默认重复）、lan-gateway 的 60000/4096/10000/3000/120000 等——提为命名常量。
16. **GeneratePin 用 `Random()`**（LanAccess.cs:315）——改 `RNGCryptoServiceProvider`（熵 20bit 的 6 位 PIN + 限速可绕过=双重弱点）。
17. **ParseEnvFile 不去引号/行内注释**（LanAccess.cs:260-269）：`DSH_LAN_PIN="123"` 会带引号导致认证失败。
18. **Service Worker 双 activate 监听可合并**；SW_VERSION 用 Math.random 与 crypto 混用；`readArchivedSessionIds` 无 TTL 缓存；`setInterval(hide,2000)` 常驻可首命中即停。
19. **esc() 未转义引号**（lan-gateway.mjs:315）→ data-key/data-title 属性注入面（self-XSS）——补 `&quot;`/`&#39;` 或用 dataset 赋值。
20. **请求体上限按块数计**（lan-gateway.mjs:588 `chunks.length > 256*1024`）——应累加字节数。
21. **cleanHeaders 未剥离 authorization**、`/__lan/health` 暴露 pid/port/host/target、错误页 e.message 未转义、绝对形式 request-target 未校验、登录体超限无响应、`/__lan/logout` 为 GET 无认证（登出 CSRF）、set-cookie 原样透传、dsh cookie 名 `dsh-auth-` 硬依赖——均为防御性改进。
22. **settings.ini 明文存 lanPin**（L205）且 UTF-8 无 BOM 手改易乱码（L131/206）——注释说明或调整。
23. **AppendLogFile 裁剪按字符数判断**（L723-729）：全中文 2MB≈70 万字符 < 100 万 → 永不裁剪——改按字节。
24. **Process 对象未 Dispose**（L870-891/1050-1059/1162-1172、L464）——句柄等 GC；**netstat ReadToEnd 无超时**（L386-387）；**probeReady 陈旧值竞态**（重启/换端口时可能误判新服务就绪，建议加代际号）；**AuthenticatedUrl 跨线程**（建议 volatile）；**单实例唤起线程竞态**（form.Handle 未创建时 BeginInvoke 抛异常 → 等待线程永久退出）。
25. **自动重启 3 次仍挂起进入 Error 时未停 LAN 网关**（L956-966）；**`if (!Visible)` 恒真**（L881，宿主常 Hide → 服务退出总弹气泡）；**LAN 端口变更冗余重启**（L1684+1694 两遍 StartLanGateway）。
26. **WebView2 初始化失败坏窗口驻留**（OnInitFailed 的 Close 被 TrayOnClose 取消 → embedded 复用坏窗口）；**ParseCssColor 不支持 #fff**（L2924）。
27. **selftest 改进**：`selftest.log` 写入失败二次抛出（L3123/3129）；`%TEMP%\dsh-launcher-selftest` 不清理（L3030-3031）；未验证 whale-256.png 资源与 token 捕获路径；selftest.ps1 不传播退出码（末行 Get-Content 恒 0）。

### 工程卫生
28. **根目录残留文件 "0"**（4B，内容 `-1\r\n`，未跟踪、未被 .gitignore 覆盖，全仓无写入来源，判定为开发期 `> 0` 重定向误产物）——删除；建议 .gitignore 加 `0`。
29. **v3.0 源码未提交**：LanAccess.cs、lan-gateway.mjs 未跟踪；DSHLauncher.cs/DSHLauncherSetup.cs/README.md/build.ps1 已改未提交——发布前提交。
30. **.gitignore 缺 `.env`**（代码明确支持仓库根 .env 存 DSH_LAN_PIN，会被 git 跟踪泄漏）；同时有死条目（`webview2-profile/`、`edge-profile/` 实际在 %LOCALAPPDATA%，`wv2-smoke.*` 无生产者）。
31. **whale-256.png / app.ico 为构建生成物却已入库**（make-icon.ps1 每次构建重新生成）——加入 .gitignore 或标注为受控资产。
32. **uninstall 缺口**：不清理防火墙规则 `DSHLauncher LAN <port>`、%APPDATA% 下 lan-pin/token/secret/.env、孤儿网关进程——补 netsh 清理（尽力而为）。
33. **根 uninstall.cmd 与生成逻辑双份维护**（BOM/转义已见差异）——加"由 WriteUninstallCmd 生成，勿手改"注释或删除模板。
34. **二维码依赖 jsdelivr/cdnjs CDN**（DSHLauncher.cs:1799/1818-1820）——离线仅文本兜底；与"零依赖"定位不符，可内联小型 qrcode 实现（唯一网络依赖）。
35. **lan-gateway.mjs 依赖全局 fetch（Node ≥ 18）**——绿色版旧 node 会直接报错，建议启动前检测版本并提示。
36. **selftest 不覆盖 LAN 网关端到端**（仅资源释放+IP 探测+PIN 解析，未实际启动网关验证 /__lan/health）。
37. **build-setup.ps1:15 冗余条件**（build.ps1 必已生成 app.ico）；**SettingsForm L2494 缩进异常**；**LanAccess.cs:5 注释矛盾**（"System.Drawing.dll(不需要)"）。

---

## 四、优先行动清单（建议按序执行）

1. 修复 C1（一行修复：`ids.Add(im.Value.Trim('"'));`）——清理归档会话功能当前完全失效。
2. 修复 C2（日志脱敏）、C3（clientIp 只用 remoteAddress）——两个安全 critical。
3. 重跑 `build-setup.ps1` 重建安装包（C4）；提交 v3.0 源码；删除根目录 "0" 文件；.gitignore 补 `.env`。
4. 修复 M1-M4（设置窗口覆盖输入 / WebView2 profile 复用 / PIN 清空 / 端口变更网关陈旧）——用户可感知的功能缺陷。
5. 修复 M5-M8、M9-M10（网关重试/WS 挂起/SW 缓存 SSE/Resolve 异常/提权脚本/代理误判）。
6. 按第三部分逐项清理（死代码、文案、魔法值、工程卫生）。

## 五、审查中发现做得好的地方

- 进程生命周期管理（启动/接管/停止/挂起自愈/端口争用清理）设计完整、注释清晰。
- PIN 比较为常数时间（sha256 + timingSafeEqual）、Cookie HMAC 签名 + 过期 + HttpOnly + SameSite=Strict、PIN 变更自动轮换密钥踢出所有设备。
- 网关只绑定具体局域网 IP、拒绝 0.0.0.0；防火墙规则限定 remoteip=localsubnet。
- 资源嵌入（lan-gateway.mjs / whale-256.png / app.ico）与构建脚本经核实完全一致；uninstall 生成逻辑一致。
- 移动端只读模式为"前端隐藏 + 后端 API 拦截"双重保障；SSE/WebSocket 透传与 PWA 注入实现专业。
