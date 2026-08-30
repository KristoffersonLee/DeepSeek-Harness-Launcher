# DeepSeek Harness Launcher 升级与维护手册

> 本机核对日期：2026-08-22（dsh 升级至 0.1.1-rc.2 后更新）；2026-08-30 复核：npm latest/next = 0.1.1-rc.2、GitHub 发 v0.1.2-alpha.1（当时未上架 npm，决策暂缓）；2026-08-31：npm 上架 0.1.2-alpha.2 → 升级 dsh 并适配启动器 token 认证（见第九节）；升级/维护为一句话触发 Agent 按本手册执行（一键脚本已移除）
> 目的：记录正确的升级/验证/修复方法，避免重蹈「升级后启动器崩溃」事故。
> 已记录事故：① 2026-08-19 rc.7 模块缺失（js-yaml/commander 文件缺失）；② 2026-08-20 rc.8 koffi 原生二进制错位（Mismatched native Koffi modules）。

## 一、本机环境版本基线（全部为最新）

| 组件 | 版本 | 位置 | 状态 |
|---|---|---|---|
| Node.js | v26.7.0（官方最新） | `C:\Program Files\nodejs` | ✅ |
| npm | 12.0.2（官方 latest） | 全局前缀 `C:\Users\20183\AppData\Roaming\npm` | ✅ |
| pnpm | 11.22.0（官方 latest） | 全局前缀同上（`dsh plugin` 必需） | ✅ |
| @deepseek-ai/dsh | **0.1.2-alpha.2**（npm `alpha` 标签；latest/next 仍为 0.1.1-rc.2） | `Roaming\npm\node_modules\@deepseek-ai\dsh` | ✅ 2026-08-31 升级核验通过（koffi 3.1.6 / node-pty / 插件树 / 模型目录一致） |
| Git | 2.55.0.4（winget 无更新） | WinGet MinGit | ✅ |
| Python | 3.13.15（3.13 系最新补丁） | `C:\Users\20183\Local\Programs\Python\Python313` | ✅ |
| DSHLauncher | v1.0.0（本地 git 与 origin/main 同步；2026-08-31 适配 dsh 0.1.2-alpha 的 token 认证，见第九节） | `D:\DSHLauncher` | ✅ |

> ⚠️ 安装/升级必须显式写版本号（标签可能随发布变化）：
> `npm install -g @deepseek-ai/dsh@0.1.2-alpha.2`（或用 `npm view @deepseek-ai/dsh dist-tags` 确认当前标签），避免装回旧标签指向的版本。
>
> 📌 版本跟踪（2026-08-31）：npm 渠道 `latest`/`next` = 0.1.1-rc.2，`alpha` = **0.1.2-alpha.2**（08-30 GitHub 发布并上架 npm，本机已升级）。**破坏性变更已确认**：dsh web **强制一次性 token 认证**（`/?token=…`，无关闭开关）→ DSHLauncher 已适配（见第九节）；另含 APIProxy 移除 → @Remote 网关、pi-ai 模型支持更新 + vLLM 思考预算、统一 dsh Profile 启动。升级前先 `npm view @deepseek-ai/dsh dist-tags` 确认上架情况；一句话触发 Agent 升级（按第三节流程）。

## 一补、Agent 与模型 API 引用（2026-08-22 更新）

### Agent 连接引用

| 项 | 值 |
|---|---|
| provider | `deepseek-official`（DeepSeek 官方） |
| BASE URL（OpenAI 格式） | `https://api.deepseek.com` |
| BASE URL（Anthropic 格式） | `https://api.deepseek.com/anthropic` |
| API Key 环境变量 | `DEEPSEEK_API_KEY`（credentials 服务） |
| 默认模型（agent-default-model） | `deepseek-v4-flash`（保持原默认；reasoningEffort: high） |
| 默认输出上限 | DSH 默认 `256K`（未覆盖；官方支持最大 384K） |

### 导入模型目录（llm.models 实测 2026-08-22）

| 模型 id | 显示名/官方版本 | 上下文 | 输出上限 | 输入模态 | 官方价格（元/百万 tokens，空闲时段 输入/输出） |
|---|---|---|---|---|---|
| `deepseek-v4-flash` | DeepSeek-V4-Flash-0731 | 1M | 384K | text | 1.5 / 4.5 |
| `deepseek-v4-pro` | DeepSeek-V4-Pro-0813 | 1M | 384K | text | 4.5 / 13.5 |
| `deepseek-v4-flash-vision-exp` | DeepSeek-V4-Flash-Vision-Exp | 1M | 384K | text + image | 1.5 / 4.5（图片按 token 计费，单图 ≤384 tokens） |
| `LongCat-2.0` | 龙猫 LongCat（自定义 provider） | 1M | — | text | 自定义：`https://api.longcat.chat/openai/v1`，`LONGCAT_API_KEY` |

> 说明：
> - `deepseek-v4-flash-vision-exp` 为实验模型，官方 `/list-models` 不列出，但设置 `model='deepseek-v4-flash-vision-exp'` 可直接调用（官方新闻 news260821 确认）。
> - 三个 DeepSeek 模型的目录由 `dsh-llm-deepseek`（0.1.1-rc.2）官方默认提供；settings.yaml **未覆盖任何模型配置**（保持官方注册含 imagePixelBudget/imageMaxBytes，maxTokens 为 DSH 默认 256K）。
> - 峰值时段（周一至五 9:00-12:00、14:00-18:00）价格为空闲时段 2 倍。
> - 2026-08-31 于 0.1.2-alpha.2 复核：deepseek 模型目录（flash/pro/vision-exp）、baseURL、1M/256K、LongCat 配置、会话 zstd 默认均与本节一致，**无变化**。

## 二、最重要的防坑规则（两次事故的教训）

1. **全局 npm 前缀必须是 `C:\Users\20183\AppData\Roaming\npm`**。
   - 启动器（DSHLauncher）用**系统 Node 26**，全局包只认这个位置。
   - ⚠️ 在 WorkBuddy/CodeBuddy 的终端里，`npm` 可能指向**受管 Node 22**（前缀在 `C:\Users\20183\.workbuddy\binaries\node\...`），此时 `npm install -g` 会装错位置。
   - 执行前先确认：`npm config get prefix` 必须输出 `C:\Users\20183\AppData\Roaming\npm`。
   - 更稳的做法：直接用系统 npm 并显式指定前缀——
     `"C:\Program Files\nodejs\npm.cmd" install -g <pkg> --prefix "C:/Users/20183/AppData/Roaming/npm" --no-audit --no-fund`。

2. **⚠️ npm 12 的 allow-scripts 安全策略会静默破坏原生模块（本次 rc.8 事故根因）**。
   - npm 12 默认阻止未白名单包的 `install`/`postinstall` 脚本。升级 dsh 时会拦下：
     `koffi`（FFI 原生库）、`node-pty`（终端）、`@deepseek-ai/dsh-subprocess-local`（spawn helper）、`@google/genai`、`protobufjs`。
   - 后果：koffi 的预编译二进制版本错位（JS 3.1.6 ↔ 原生 3.1.5）→ 启动器在加载 `subprocess`/`sandbox` 插件时抛
     `Mismatched native Koffi modules` 直接退出（code 1），**表现为「启动器打不开」**。
   - 正确做法：装完立即执行原生模块验证（见下），若发现被拦，用
     `npm install -g @deepseek-ai/dsh@0.1.0-rc.8 --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` 补跑脚本。

3. **升级/重装后必须核验完整性**（含原生模块）：
   - `dsh --version` → 应输出 `0.1.0-rc.8`
   - `dsh --help` → 应正常打印用法（触发 bin.js → dsh-app-boot → commander/js-yaml 加载链）
   - `dsh --profile web --dump-config` → 应打印完整插件配置树（验证 js-yaml YAML 解析与整棵插件树可加载，约 500 行）
   - **koffi 原生版本匹配**（关键！本次事故点）：
     ```powershell
     node -e "const k=require('C:/Users/20183/AppData/Roaming/npm/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"
     ```
     必须输出 `3.1.6`（与 JS 包装一致）；若报 `Mismatched native Koffi modules` 即安装损坏。
   - **node-pty 可加载**：
     ```powershell
     node -e "const p=require('C:/Users/20183/AppData/Roaming/npm/node_modules/@deepseek-ai/dsh/node_modules/node-pty'); console.log(typeof p.spawn)"
     ```
     应输出 `function`。
   - 关键文件抽查（以下路径必须都存在）：
     ```
     Roaming\npm\node_modules\@deepseek-ai\dsh\package.json
     Roaming\npm\node_modules\@deepseek-ai\dsh\lib\bin.js
     Roaming\npm\node_modules\@deepseek-ai\dsh\node_modules\commander\index.js
     Roaming\npm\node_modules\@deepseek-ai\dsh\node_modules\js-yaml\dist\js-yaml.mjs
     Roaming\npm\node_modules\@deepseek-ai\dsh\node_modules\@koromix\koffi-win32-x64\win32_x64\koffi.node
     ```
   - 浏览器访问 `http://127.0.0.1:3080/` 应返回页面（HTTP 200）。

4. **不要一边装一边杀进程**。事故①恶化的直接原因：安装中途被 kill，残留 npm worker 死锁、留下半成品目录。要让安装完整跑完；确需中断时，先 `tasklist` 确认残留 node 进程再处理。

5. **清理残留临时目录**。npm 安装后可能留下 `Roaming\npm\node_modules\@deepseek-ai\.dsh-*` 临时目录（内含 sharp 等 DLL）。若被运行中的启动器进程锁定无法删除，**重启启动器后即可删**：
   ```powershell
   Get-ChildItem "C:\Users\20183\AppData\Roaming\npm\node_modules\@deepseek-ai" -Force  # 正常应只有 dsh
   Remove-Item "<残留目录路径>" -Recurse -Force
   ```

## 三、正确的升级命令（按序执行）

> 🚀 **首选方式（2026-08-30 起）：一句话触发，Agent 按本手册直接升级，不依赖脚本。**
> 只需对 DSH 说「更新 DSH（可指定版本，如 0.1.2-alpha.1）」，Agent 自动按以下流程执行：
> 1. **核对 registry**：`npm view @deepseek-ai/dsh dist-tags` / `versions`，确认目标版本已上架（alpha/rc 发布可能滞后于 GitHub）；
> 2. **安装**（系统 npm + 显式前缀 + 显式版本 + 放行原生构建脚本 + 防损坏缓存）：
>    ```powershell
>    "C:\Program Files\nodejs\npm.cmd" install -g @deepseek-ai/dsh@<版本> `
>      --prefix "C:/Users/20183/AppData/Roaming/npm" --no-audit --no-fund --prefer-online `
>      --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
>    ```
> 3. **全面核验**（第二节第 3 条）：`dsh --version`、关键文件抽查、koffi 原生版本 3.1.6、node-pty 可加载、`dsh --profile web --dump-config` 无 error/mismatch、清理残留 `.dsh-*`；
> 4. **回写手册**：更新「一、版本基线」「六、升级历史」，必要时更新「一补、API 引用」与版本跟踪备注；
> 5. **提示重启**：升级只改磁盘文件，运行中的 GUI 需重启启动器（DSHLauncher）才加载新版本；升级中不得中断或误杀 node 进程（第二节第 4 条）。
>
> 📌 说明（2026-08-30 更新）：一键脚本 `update-dsh.ps1` 已移除（升级不再依赖脚本），统一走上方「首选方式」——一句话触发 Agent 按本手册执行；下方手动命令为等价操作，供排查/人工核验使用。

手动命令（等价操作，供排查用）：

```powershell
# 0) 确认前缀正确（必须输出 Roaming\npm）
npm config get prefix

# 1) 升级 dsh 到 rc.8（显式版本 + 防损坏缓存 + 放行原生构建脚本）
"C:\Program Files\nodejs\npm.cmd" install -g @deepseek-ai/dsh@0.1.1-rc.2 `
  --prefix "C:/Users/20183/AppData/Roaming/npm" --no-audit --no-fund --prefer-online `
  --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs

# 2) 核验（见第二节第 3 条，重点跑 koffi 版本检查）
dsh --version
node -e "const k=require('C:/Users/20183/AppData/Roaming/npm/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"
dsh --profile web --dump-config
```

> 说明：
> - `--prefer-online`：绕过本地已损坏的 npm 缓存 tarball（本次 koffi 缓存损坏的来源之一）。
> - 若沙箱/环境对默认 npm-cache 报 EPERM，追加 `--cache C:/tmp/npmcache` 换可写缓存目录。

## 四、故障恢复（若启动器打不开/报错）

> ⚠️ **先读第八节**：若日志含 `uses .jsonl, but this backend is configured for compression "zstd"`（会话编码不匹配），这是 **0.1.1-rc.2 的 zstd 默认压缩与旧明文会话冲突**，**第四节的「重装 dsh」步骤修不了它**（重装后默认仍是 zstd，混编码文件还在就会继续崩）。请按第八节统一会话编码，勿盲目重装。

1. 停止启动器，确认崩溃进程已退出（端口 3080 空闲：`Get-NetTCPConnection -LocalPort 3080`）。
   ⚠️ 不要误杀其他 node 进程（如 WorkBuddy 自身的 MCP/agent 进程）——用 `Get-CimInstance Win32_Process` 看命令行确认身份。
2. 删除损坏安装与残留临时目录：
   ```powershell
   Remove-Item "C:\Users\20183\AppData\Roaming\npm\node_modules\@deepseek-ai\dsh" -Recurse -Force
   Get-ChildItem "C:\Users\20183\AppData\Roaming\npm\node_modules\@deepseek-ai" -Force  # 清理 .dsh-* 残留
   ```
3. 按第三节命令完整重装，**等它跑完**（约 3-5 分钟，勿中断）。
4. 按第二节第 3 条全面核验（**koffi 版本 3.1.6、node-pty 可加载、dump-config 完整**）后重启启动器。

## 五、常见误报说明

- `npm ls -g` 里的 `UNMET OPTIONAL DEPENDENCY @img/sharp-*`（darwin/linux/freebsd 平台二进制）——Windows 上本就不装，**不是问题**。
- `EPERM` 写 `cordis.yml`——通常是有另一个 dsh web 实例正在运行（端口被占）或沙箱权限限制，不是配置损坏。
- `npm warn cleanup Failed to remove .dsh-*`——临时目录被运行中进程锁定，重启启动器后可删，不影响运行。
- koffi 加载报 `Mismatched native Koffi modules`——**真问题**，按第四节重装（不要试图只替换单个 .node 文件）。

## 六、升级历史

| 日期 | 操作 | 结果 |
|---|---|---|
| 2026-08-19 | 全环境核对；npm 11.19→12.0.2；装 pnpm 11.22.0；dsh rc.7 完整性修复（js-yaml.mjs/commander 缺失） | ✅ 正常 |
| 2026-08-20 | dsh rc.7 → rc.8（`next` 标签，显式版本安装） | ⚠️ 首次安装 koffi 原生错位致启动器崩溃 → 系统 npm + `--prefer-online` + 独立 cache 完整重装 + 放行脚本后修复，验证 koffi 3.1.6 / 插件树 503 行无错 |
| 2026-08-22 | dsh rc.8 → 0.1.1-rc.2（`next` = `latest`；官方内置 V4-Flash-Vision-Exp 注册，多模态 API 上线后首个含视觉模型的版本） | ✅ update-dsh.ps1 升级 + 14 项核验 PASS（koffi 3.1.6 / 插件树 514 行 / node-pty 正常）；残留 `.dsh-DFKhUDXu`（sharp DLL 被运行中实例锁定，手册第五节误报说明）待重启启动器后清理 |
| 2026-08-22（补） | 确认 dsh 0.1.1-rc.2 已含官方模型目录（flash/pro/vision-exp）；**默认模型保持 `deepseek-v4-flash` 不变**；手册新增「Agent 与模型 API 引用」节 | ✅ llm.models 实测 4 模型完整、failures 0；settings.yaml 无任何模型覆盖 |
| 2026-08-30 | 复核 dsh：npm registry `latest` = `next` = 0.1.1-rc.2（官方/npmmirror/jsdelivr 三源确认，**0.1.2-alpha.1 未上架 npm**）；**GitHub 官方发布页 08-27 已发布 v0.1.2-alpha.1（alpha 预发布，含破坏性变更：移除 APIProxy、pi-ai 模型支持更新等）→ 决策：暂不升级，保持 0.1.1-rc.2**；update-dsh.ps1 14 项验证全 PASS（koffi 3.1.6 / 插件树 514 行 / node-pty / 关键文件 / 无残留）；运行中 Agent 进程（PID 13428）加载 0.1.1-rc.2；导入模型 API 引用与「一补」节记录核验一致 | ✅ 保持 0.1.1-rc.2 |

| 2026-08-30（补） | 修复会话编码不匹配崩溃：0.1.1-rc.2 的 `dsh-session-persistence-jsonl` 后端默认 `compression: zstd`，某会话目录混含明文 `session.jsonl`（188B header 残片）与 `session.jsonl.zstd`（436KB 完整会话）致 `encodingMismatch` 启动即崩（退出码 1）；删明文残片、备份移出 `.dsh`、统一 root 为 zstd；`dsh web` 启动验证监听 3080 无 `encodingMismatch` | ✅ 修复并写入手册第八节（与 rc.7/rc.8 事故无关，勿重装） |
| 2026-08-30（补2） | 流程变更：升级/维护不再依赖一键脚本 `update-dsh.ps1`（**已删除**）；清除根目录无用文件（`update-dsh.ps1`、`dsh-session-encoding-fix.zip`、`.workbuddy\`）；`.gitignore` 增加 `.workbuddy/`；手册第三节改为「一句话触发 Agent 按手册执行」 | ✅ 目录与手册全面一致；下次更新只需一句话 |
| 2026-08-31 | dsh 0.1.1-rc.2 → **0.1.2-alpha.2**（npm `alpha` 标签，08-30 上架）；按第三节流程安装 + 全面核验 PASS（koffi 3.1.6 / node-pty / 插件树 / 模型目录与「一补」一致 / 会话 zstd 默认一致）；**破坏性变更：dsh web 强制一次性 token 认证**（无 token 401）→ DSHLauncher v1.0.0 内嵌窗 401，启动器适配（捕获 `dsh web: …/?token=` 并导航，见第九节），重建 exe 正式名替换（旧版备份 `DSHLauncher.exe.old`） | ✅ 升级完成，重启启动器后内嵌窗正常 |

## 七、rc.8 官方发布要点（与本机相关）

来源：<https://github.com/deepseek-ai/deepseek-harness/releases>（2026-08-20 官方确认）

- **多模态增强**：DeepSeek 模型适配器可配置原生图片请求；`/goal`、`/plan` 命令支持图文输入；`@` 菜单可引用文件与会话。
- **子代理**：Claude Code 与 Codex 可作为 Profile Bundle 按需安装；Codex 支持非交互权限模式与多命名实例。
- **Windows PTY**：极简模式预设默认支持**持久 PowerShell 会话**（`dsh-tool-pwsh-persistent`；win32 上 bash 系自动禁用，POSIX 反之）——本机已确认 rc.8 的 `minimal` preset 含此配置。
- **问题修复**：图片过大/历史图片载荷导致请求失败；取消流式生成后回复前缀丢失；自定义 OpenAI 兼容网关调用与推理内容回传。
- **体验优化**：`web_search` 支持并发查询；子代理 reportDelivery 及时反馈；本地 `dsh web` 自动打开浏览器；大历史会话分叉性能优化。
- **⚠️ SQLite 后端数据结构不兼容**：本机不受影响——会话主存储为 JSONL（`session-persistence-jsonl`），`session-query-sqlite` 为内存模式（`path: ':memory:'`、`openAt: never`），无磁盘 SQLite 数据需要迁移。
- **品牌**："DeepSeek Harness"为注册商标，使用需遵循品牌规范。

## 八、新增故障：会话编码不匹配导致启动器崩溃（zstd/plaintext）

> 发生日期：2026-08-30；环境 dsh 0.1.1-rc.2。**与 rc.7（模块缺失）/ rc.8（koffi 错位）两次事故无关，切勿盲目重装**——重装后默认仍是 zstd，混编码文件还在会继续崩。

### 现象
启动器日志报（退出码 1）：
`Error: ... session artifact "C:\Users\20183\.dsh\sessions\...\<id>\session.jsonl" uses .jsonl, but this backend is configured for compression "zstd"; use a separate root or select the matching compression mode`
调用栈落在 `dsh-session-persistence-jsonl/lib/index.js` 的 `encodingMismatch` / `checkRootEncoding`。

### 根因
dsh 0.1.1-rc.2 的 `dsh-session-persistence-jsonl` 后端**默认 `compression: zstd`**（`DEFAULT_COMPRESSION = "zstd"`，见 `node_modules/@deepseek-ai/dsh-session-persistence-jsonl/lib/index.js:733`；`session-persistence-jsonl` 后端 config 在 `cordis.patch.yml` 里默认只含 `root`，未显式设 compression，故回退 zstd）。后端在 `[cordis.init]` 阶段扫描每个会话目录的"对立编码"文件（`checkRootEncoding`，index.js:1402）：配置 zstd 时发现 `session.jsonl`、或配置 none 时发现 `session.jsonl.zstd`，即抛 `encodingMismatch` 退出。

⚠️ 本机实测为**混编码会话目录**：某会话目录同时含 188 字节明文 `session.jsonl`（仅 header 残片，无事件数据）与 436KB 的 `session.jsonl.zstd`（完整会话，解码出 798 帧 / 132 万字符、会话 id 一致）。此时**不能"转换"，应直接删掉明文残片**——完整数据已在 zstd 中。

### 修复（让 root 统一为 zstd，匹配默认）
1. 确认冲突范围：`find "$APPDATA/.dsh/sessions" -name "*.jsonl"`（明文数量）、`find ... -name "*.jsonl.zstd"`（zstd 数量）。
2. 对每个冲突的明文 `session.jsonl`：
   - 先解码同目录 `session.jsonl.zstd` 的 header（`zstdDecompressSync` 解首帧），确认会话 id 一致且含完整事件，证明 zstd 为权威数据；
   - **备份该明文文件到 `.dsh` 目录之外**（如 `C:\Users\20183\dsh_session_backup_<id>\`）；
   - 删除明文 `session.jsonl`，仅保留 `session.jsonl.zstd`。
3. ⚠️ **备份切勿放在 `.dsh\sessions` 内部**——dsh 会把它当作会话 root 一并扫描，备份里的明文碎片会再次触发同一崩溃（本次修复曾踩此坑：备份误放 `.dsh\sessions\__backup_*` 导致崩溃依旧）。
4. 确认 `.dsh\sessions` 下再无明文：``find "$APPDATA/.dsh/sessions" -name "*.jsonl"`` 应为 0。

### 验证
- `dsh --profile web --dump-config` 应正常输出，且 `session-persistence-jsonl` 后端 config 仅含 `root`（compression 回退 zstd 即符合预期，无 `encodingMismatch` 即通过）；
- 或直接拉起验证（与启动器一致）：`cd C:\Users\20183\Desktop && dsh web`，日志出现 `dsh web: http://127.0.0.1:3080` 且无 `encodingMismatch` / `plugin tree failed to load` 即修复成功；
- 重启 DSHLauncher 即可正常加载。

### 备选（本机不适用）
若全部会话均为明文、想保留明文历史并改默认，可在 `C:\Users\20183\.dsh\profiles\web\cordis.patch.yml` 追加 `- id: session-persistence-jsonl / config: { compression: none }`，使后端读明文、新会话也写明文。但**本机已有 121 个 zstd 会话，改 none 反而会让这些 zstd 触发 mismatch**，故本机必须用"统一为 zstd"方案，不可用此备选。

## 九、新增故障：0.1.2-alpha 起 dsh web 强制一次性 token 认证（启动器适配）

> 发生日期：2026-08-31；环境 dsh 0.1.2-alpha.2。

### 现象
DSHLauncher 内嵌窗口（WebView2）显示 `dsh web authentication required; reopen the URL printed by dsh web`；直接访问 `http://127.0.0.1:3080/` 返回 **401**，带 `/?token=…` 返回 **200**。启动器日志可见 `dsh web: http://127.0.0.1:3080/?token=…`（每次启动 token 不同）。

### 根因
0.1.2-alpha 起 `dsh-client-connection` 对 Web 界面启用一次性 token 认证（browser-session 签名 cookie + 每次启动生成新 token），**无配置关闭开关**（`--trusted-host` 仅作用于 /api 浏览器信任围栏，与页面认证无关）。DSHLauncher v1.0.0 固定打开无 token 的 `http://127.0.0.1:3080/` → 401。

### 修复（DSHLauncher 适配，2026-08-31）
1. `DSHLauncher.cs` 三处改动：
   - 新增字段 `AuthenticatedUrl`（默认 null）；
   - `OnServerOutput`：捕获 dsh web 输出行 `dsh web: http…/?token=…`（去掉 ` (LAN: …)` 后缀）写入 `AuthenticatedUrl`；
   - `OpenBrowser()`：优先使用 `AuthenticatedUrl` 导航，null 时回退普通地址（兼容旧版本）。
2. 重建：`build.ps1`（**注意：文件为 UTF-8 无 BOM，须用 pwsh 7 执行，或 `csc /codepage:65001` 直接编译；Windows PowerShell 5.1 按 ANSI 误读中文会报语法错**）。
3. 正式名替换：先备份 `DSHLauncher.exe.old` 再覆盖 `DSHLauncher.exe`；若正式名 exe 正被运行占用，可先重命名运行中 exe、拷入新文件（免杀进程）。

### 验证
- 带 token HTTP 200 / 无 token HTTP 401（符合预期）；
- 重启启动器后内嵌窗正常显示 Harness。

### 说明（服务生命周期）
启动器退出会同时停止其拉起的 dsh web（托盘设置可改「退出时是否同时停止服务」）；点 ✕ 最小化到托盘则服务常驻、网页端不中断。
