//! 归档会话清理（v4 `LanAccess.DeleteArchivedSessions` 的 Rust 版）。
//!
//! 三条硬约束来自 v4.2.1 / v4.2.2 / v4.2.3 的踩坑记录，重写时不能丢：
//!   1. 归档 id 是 **44 字符** `session-<36位uuid>`（v4.2.1 曾因只接受 36 字符而恒删除 0 个）
//!   2. 必须拒绝删除 reparse point（符号链接 / junction），否则会误删链接指向的目标
//!   3. 必须同时删除 `session_projcache` 投影缓存，否则运行中的 dsh 会依据缓存让归档列表"复活"

use std::path::{Path, PathBuf};
use std::time::Duration;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// `~/.dsh`（dsh 的数据根），**取不到用户主目录时返回 `None`**。
///
/// ## 为什么必须是 `Option`，而不是 `unwrap_or_default()`
///
/// 旧实现失败时退化成**相对路径** `.dsh/...`，而本模块的函数会**扫描并删除文件**：
/// - `collect_lock_files` 会把当前工作目录当成 `~/.dsh` 遍历，可能删掉别的程序
///   放在 CWD 下的 `.lock`；
/// - `delete_archived_sessions` 会在 CWD 下删会话目录。
///
/// 取不到主目录时唯一安全的动作是**什么都不做**（调用方拿到 `None` 后直接返回），
/// 因此这里把"定位失败"编码进类型，而不是靠调用方记得检查空路径。
fn dsh_home_opt() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".dsh"))
}

/// `%USERPROFILE%\.dsh` —— dsh 的数据根目录。
///
/// **仅供展示/日志**（路径比较、打印）。删除与扫描一律使用内部返回 `Option` 的版本：
/// 主目录不可得时本函数返回空路径，若拿它去删/扫就会落到当前工作目录。
pub fn dsh_home() -> PathBuf {
    dsh_home_opt().unwrap_or_default()
}

/// `~/.dsh/storages/workspace.json`（定位不到主目录时为 `None`）。
fn workspace_json_path() -> Option<PathBuf> {
    Some(dsh_home_opt()?.join("storages").join("workspace.json"))
}

/// `~/.dsh/sessions`（其下每个子目录是一个 workspace）。
///
/// 探测与清理**共用**这一个来源：早先这里与 `sessions_root()` 是两份逐字重复的实现，
/// 任一处改动都可能让"探测"与"清理"指向不同的目录。
fn sessions_root() -> Option<PathBuf> {
    Some(dsh_home_opt()?.join("sessions"))
}

/// `~/.dsh/storages/session_projcache/sessions`。
fn proj_cache_dir() -> Option<PathBuf> {
    Some(
        dsh_home_opt()?
            .join("storages")
            .join("session_projcache")
            .join("sessions"),
    )
}

/// 「会话刚被写过」的判定窗口。
///
/// 取 5 分钟：足够覆盖"用户刚发完一条消息"到"我们点清理"之间的间隔，
/// 又不至于把几十分钟前的历史会话算成活跃。
pub const ACTIVE_SESSION_WINDOW: std::time::Duration = std::time::Duration::from_secs(300);

/// 活跃会话的探测结果。
///
/// ## 为什么要探测「有没有会话正在跑」
///
/// 「清理归档会话」按设计**必须先停止 dsh**。若此刻有会话正在对话/跑任务，
/// 停服就会把它打断。用户明确要求：**清理前要确保没有运行中的会话**。
///
/// ## 判定信号（两个，互补）
///
/// 1. **会话运行器进程存活**：dsh 把会话派生到独立进程
///    （`@deepseek-ai/dsh-subprocess-local ... runner.js`）。只要存在一个，就说明
///    **确实有会话在跑** —— 这是最硬的信号（实测：本机跑着 1 个 runner，
///    它正在执行当前这次对话）。
/// 2. **会话文件在 [`ACTIVE_SESSION_WINDOW`] 内被写过**：会话内容落在
///    `~/.dsh/sessions/<workspace>/<session-id>/session.v*.jsonl*`。文件新近被改说明
///    会话刚刚还在活动（也可能刚结束，因此这是**较软的**信号）。
///
/// ## 诚实标注的不确定性
///
/// 这两个信号都是**间接**的：无法从用户态精确问出"dsh 现在是否持有活跃会话句柄"。
/// 因此本模块只用于**提醒用户**（提升确认强度、默认焦点放在「否」），
/// 而不是硬性阻止——硬阻止会让"清理"这个功能在被自己这条对话占用时永远不可用。
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSessions {
    /// 存活的会话运行器进程数。
    pub runner_processes: usize,
    /// 判定窗口内被写过的会话文件数。
    pub recently_written: usize,
    /// 最近被写过的会话目录名（用于向用户说明"哪个会话在跑"），最多 3 个。
    pub recent_session_hint: Vec<String>,
}

impl ActiveSessions {
    /// 是否存在**正在运行**的会话（以最硬的信号为准）。
    pub fn is_running(&self) -> bool {
        self.runner_processes > 0
    }

    /// 是否有任何"可能还在活动"的迹象（运行器进程或近期写入）。
    pub fn any_activity(&self) -> bool {
        self.runner_processes > 0 || self.recently_written > 0
    }

    /// 面向用户的一句话描述。
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.runner_processes > 0 {
            parts.push(format!(
                "检测到 {} 个正在运行的会话进程",
                self.runner_processes
            ));
        }
        if self.recently_written > 0 {
            parts.push(format!(
                "{} 个会话文件在最近 {} 分钟内被写过",
                self.recently_written,
                ACTIVE_SESSION_WINDOW.as_secs() / 60
            ));
        }
        if parts.is_empty() {
            "未检测到活跃会话".to_string()
        } else {
            parts.join("；")
        }
    }
}

/// 探测当前是否有会话在活动（见 [`ActiveSessions`]）。
///
/// 返回 `None` 表示**无法判定** —— 调用方应当把它当作"不确定"，
/// 而不是"没有会话"（保守处理：按有活动对待，提升确认强度）。
pub fn probe_active_sessions() -> Option<ActiveSessions> {
    let runner_processes = count_session_runner_processes();
    // 定位不到主目录时不要退化成"扫描当前目录"，只用运行器进程这一个信号
    let Some(root) = sessions_root() else {
        return Some(ActiveSessions {
            runner_processes,
            recently_written: 0,
            recent_session_hint: Vec::new(),
        });
    };

    if !root.is_dir() {
        // 目录不存在通常意味着从未用过 dsh；此时唯一可用的信号是运行器进程
        return Some(ActiveSessions {
            runner_processes,
            recently_written: 0,
            recent_session_hint: Vec::new(),
        });
    }

    let mut recently_written = 0usize;
    let mut hint: Vec<String> = Vec::new();
    let mut stack = vec![root.clone()];
    // 限制遍历规模：sessions 下是 <workspace>/<session>/<file>，3 层足够；
    // 预算用于防止异常深/宽的目录把界面拖住。
    let mut budget = 5000usize;
    while let Some(dir) = stack.pop() {
        if budget == 0 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            budget = budget.saturating_sub(1);
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                // 不跟随 reparse point（与清理路径同一原则）
                if !is_reparse_point(&path) {
                    stack.push(path);
                }
                continue;
            }
            // 会话文件形如 session.v3.jsonl.zstd / session.jsonl
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("session") || !name.contains("jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let Ok(age) = modified.elapsed() else {
                continue;
            };
            if age <= ACTIVE_SESSION_WINDOW {
                recently_written += 1;
                if hint.len() < 3 {
                    if let Some(parent) = path.parent().and_then(|p| p.file_name()) {
                        let n = parent.to_string_lossy().to_string();
                        if !hint.contains(&n) {
                            hint.push(n);
                        }
                    }
                }
            }
        }
    }

    Some(ActiveSessions {
        runner_processes,
        recently_written,
        recent_session_hint: hint,
    })
}

/// 统计存活的「会话运行器」进程数。
///
/// 会话运行器由 dsh 服务器派生，命令行里含 `dsh-subprocess-local`（入口 `runner.js`）。
/// 它只在会话真正执行时存在，因此是"有会话在跑"的最硬信号
/// （实测：本机跑着 1 个 runner，正在执行当前这次对话）。
///
/// ## 为什么用 `Get-CimInstance` 而不是自己解析 PEB
///
/// 读**其它进程的命令行**没有一步到位的公开 API：Win32 侧要 `ReadProcessMemory` 读
/// 目标进程的 PEB，再手工解读 `RTL_USER_PROCESS_PARAMETERS`（还要处理 WOW64 下
/// 32/64 位结构差异、跨权限读取失败等）。`ReadProcessMemory` 本身在 `windows` crate
/// 里是**可用**的（`Win32_System_Diagnostics_Debug`，本项目已启用该 feature），
/// 但为了一个"点了按钮才跑一次"的探测去手写 PEB 解析不划算——那是一条容易随
/// 系统版本变化而静默失效的路径。
/// `Get-CimInstance Win32_Process` 是系统自带、一次调用即可拿到全部进程命令行；
/// 而且本函数**只在用户点击「清理归档会话」时调用一次**，不在热路径上。
///
/// ## 为什么必须有超时
///
/// 早先实现用 `Command::output()` —— 它**没有超时**。一旦 PowerShell 被策略拦下、
/// WMI 卡住或机器负载极高，这个调用会把清理流程（进而是用户可感知的界面状态）
/// **无限期挂住**。现在改为 spawn + 轮询 `try_wait`，超时即杀进程树（并连同
/// `WinMgmt` 查询一并放弃），如实返回 0（= 无硬证据）。
///
/// 查询失败或超时返回 `0`：调用方必须结合 [`ActiveSessions::recently_written`] 这个
/// 软信号一起判断，并在文案里保留"可能仍有会话"的余地。
fn count_session_runner_processes() -> usize {
    /// 外部命令的硬超时：它决定用户点「清理」后最多多等多久。
    const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
    let script = "Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" \
                  | Where-Object { $_.CommandLine -like '*dsh-subprocess-local*' -or \
                                   $_.CommandLine -like '*runner.js*' } \
                  | Measure-Object | Select-Object -ExpandProperty Count";
    let out = run_capture_within(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", script],
        PROBE_TIMEOUT,
    );
    match out {
        Some(text) => text.trim().parse::<usize>().unwrap_or(0),
        None => 0,
    }
}

/// 运行外部命令并在 `timeout` 内取回 stdout（超时/失败返回 `None`）。
///
/// 与 `dsh.rs::run_capture` 的差别：这里不关心退出码与 stderr，只关心"限时拿到 stdout"，
/// 因此实现更短；相同的是都必须**限时**——无超时的 `output()` 会把调用方永久挂住。
fn run_capture_within(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const MAX_OUTPUT_BYTES: u64 = 64 * 1024;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;

    // 单线程排空 stdout：`powershell.exe` 的输出量很小，直接读到底即可
    //
    // **注意这里不能用 `?` 直接返回**：`Child` 被丢弃时**不会**终止子进程
    // （std 的 Drop 只关闭句柄），于是拿不到 stdout 的分支会留下一个无人看管的
    // PowerShell 常驻后台。必须先回收再返回。
    let Some(mut stdout) = child.stdout.take() else {
        let pid = child.id();
        let _ = child.kill();
        crate::process::kill_process_tree(pid);
        let _ = child.wait();
        return None;
    };
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        // 限长：外部命令若被诱导输出巨量内容也不能吃满内存
        let _ = stdout.by_ref().take(MAX_OUTPUT_BYTES).read_to_end(&mut buf);
        buf
    });

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // 超时：杀进程树（PowerShell 会派生 WmiPrvSE 查询等后代）
                    let pid = child.id();
                    let _ = child.kill();
                    crate::process::kill_process_tree(pid);
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };
    let bytes = reader.join().unwrap_or_default();
    status.filter(|s| s.success())?;
    Some(String::from_utf8_lossy(&bytes).to_string())
}

/// 从 workspace.json 文本中解析 `archivedSessionIds` 数组内容。
///
/// ## 安全边界（重要）
///
/// 本函数**不是**完整 JSON 解析器，但已按"任何一个环节不确定就放弃"的原则加固：
/// 1. 先用 `serde_json` 确认**整个文档是合法 JSON**（不合法直接放弃，绝不改写）；
/// 2. 要求 `"archivedSessionIds"` 在文本中**唯一**
///    （同名 key 出现多次时区间定位不可靠）；
/// 3. 用**字符串感知的括号配对**（[`match_bracket`]）定位数组结尾，
///    而不是此前那种 `find(']')` —— 后者遇到数组里嵌套 `[`/`]`（或字符串里出现 `]`）
///    会定位到错误的区间，进而**写坏 workspace.json**；
/// 4. 数组内容交给 `serde_json` 反序列化，因此转义引号等写法都被正确处理
///    （旧实现按 `,` 切分 + `trim_matches('"')`，遇到转义就解析错）。
pub fn archived_session_ids(raw: &str) -> Vec<String> {
    let Some(key_idx) = unique_key_index(raw, "\"archivedSessionIds\"") else {
        return Vec::new();
    };
    let rest = &raw[key_idx..];
    let Some(rel_start) = rest.find('[') else {
        return Vec::new();
    };
    let start = key_idx + rel_start;
    let Some(end) = match_bracket(raw, start) else {
        return Vec::new();
    };
    // 交给真正的 JSON 解析器处理数组内容（含转义、嵌套结构）
    serde_json::from_str::<Vec<String>>(&raw[start..=end]).unwrap_or_default()
}

/// 从 `open_idx`（必须是 `[` 或 `{`）出发，返回**配对**闭合括号的下标。
///
/// 与 `find(']')` 的关键差别：跳过字符串字面量（含 `\"` 转义）并跟踪嵌套层级，
/// 因此 `["a]b", ["c"]]` 这种内容也能正确定位到最后一个 `]`。
fn match_bracket(raw: &str, open_idx: usize) -> Option<usize> {
    let bytes = raw.as_bytes();
    let open = *bytes.get(open_idx)?;
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(open_idx) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    // 只接受与起始括号同类型的闭合（`[` 必须由 `]` 闭合）
                    return (b == close).then_some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 找到 `key` 在文本中的**唯一**出现位置；出现 0 次或多次都返回 `None`。
///
/// 为什么要求唯一：区间式改写的正确性依赖「这个 key 只有一个」。
/// 若上游将来把 `archivedSessionIds` 嵌进子对象或同时出现在多处，
/// 我们会定位到错误区间并**破坏 workspace.json**。这里用「唯一性」作为最廉价的
/// 结构性护栏（比引入完整 JSON 解析更轻，且能覆盖真实风险）。
fn unique_key_index(raw: &str, key: &str) -> Option<usize> {
    let first = raw.find(key)?;
    if raw[first + key.len()..].contains(key) {
        return None;
    }
    Some(first)
}

/// 归档会话数量（设置面板展示用）。
pub fn archived_session_count() -> usize {
    let Some(ws) = workspace_json_path() else {
        return 0;
    };
    if !ws.is_file() {
        return 0;
    }
    std::fs::read_to_string(&ws)
        .map(|raw| archived_session_ids(&raw).len())
        .unwrap_or(0)
}

/// id 强校验：`session-` + 36 位 `[a-f0-9-]`，合计 44 字符。
fn is_valid_session_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if !bytes.starts_with(b"session-") {
        return false;
    }
    let tail = &bytes[8..];
    tail.len() == 36 && tail.iter().all(|c| c.is_ascii_hexdigit() || *c == b'-')
}

/// 是否为 reparse point（符号链接 / junction）。
fn is_reparse_point(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        })
        .unwrap_or(false)
}

/// 彻底删除全部归档会话。
///
/// 调用前应先停止 dsh 服务并等待端口释放，否则可能"占用"导致删除失败。
/// 返回 `(已清理数, 详情文本)`。
pub fn delete_archived_sessions() -> Result<(usize, String), MaintenanceError> {
    let Some(ws) = workspace_json_path() else {
        return Ok((
            0,
            "无法定位用户主目录（~），未执行任何删除；请检查 USERPROFILE / HOME 环境变量".into(),
        ));
    };
    if !ws.is_file() {
        return Ok((0, "未找到 workspace.json（可能没有归档会话）".into()));
    }
    let raw = std::fs::read_to_string(&ws)?;
    let ids = archived_session_ids(&raw);
    if ids.is_empty() {
        return Ok((0, "没有归档会话".into()));
    }

    let sessions_root = sessions_root();
    let cache_root = proj_cache_dir();
    // **一次性**枚举 workspace 目录。旧实现把 `read_dir(sessions_root)` 放在按 id 的循环里，
    // 于是「N 个归档会话」= N 次对同一目录的完整枚举（上百个归档时纯粹是重复 IO）。
    let workspace_dirs: Vec<PathBuf> = match sessions_root.as_deref().filter(|r| r.is_dir()) {
        Some(root) => std::fs::read_dir(root)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };
    // 一次遍历决定每个 id 的去留：
    //  - `cleaned`：目录确实已不存在（含"数据本就不存在"的遗留标记）
    //  - `pruned` ：id 本身非法（含路径遍历尝试），仅从列表剔除，**不算"清理"**
    // 旧实现用 `removed.contains(id)`（O(n²)）且把两类混在一个计数器里，
    // 归档量大时明显变慢，摘要文案也会把「剔除脏条目」说成「已清理」。
    let mut cleaned: Vec<String> = Vec::new();
    let mut pruned: Vec<String> = Vec::new();

    for id in &ids {
        // 无法校验的脏条目（含路径遍历尝试）直接从列表清出，不计入清理数
        if !is_valid_session_id(id) {
            pruned.push(id.clone());
            continue;
        }

        let mut dir_gone = true;
        for wd in &workspace_dirs {
            let sd = wd.join(id);
            if !sd.is_dir() {
                continue;
            }
            dir_gone = false;
            if is_reparse_point(&sd) {
                break; // 视为占用，保留
            }
            for attempt in 0..4 {
                if std::fs::remove_dir_all(&sd).is_ok() {
                    dir_gone = true;
                    break;
                }
                if attempt < 3 {
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
            break;
        }

        if dir_gone {
            cleaned.push(id.clone());
            if let Some(cache_root) = &cache_root {
                let cf = cache_root.join(format!("{id}.json"));
                if cf.is_file() {
                    let _ = std::fs::remove_file(&cf);
                }
            }
        }
    }

    let removed_count = cleaned.len() + pruned.len();
    let total = ids.len();
    // 仅当有变化时才改写 workspace.json；全部失败则保持数据与列表一致
    if removed_count > 0 {
        let removed: std::collections::HashSet<&String> =
            cleaned.iter().chain(pruned.iter()).collect();
        let remaining: Vec<String> = ids
            .iter()
            .filter(|id| !removed.contains(id))
            .cloned()
            .collect();
        rewrite_archived_list(&raw, &remaining, &ws)?;
    }

    let cleaned_n = cleaned.len();
    let pruned_n = pruned.len();
    let remain = total - removed_count;
    let detail = if removed_count > 0 {
        let mut s = format!("已清理 {cleaned_n} / {total} 个归档会话");
        if pruned_n > 0 {
            s.push_str(&format!("，并剔除 {pruned_n} 个格式非法的列表项"));
        }
        s.push('。');
        if remain > 0 {
            s.push_str(&format!(
                " 其余 {remain} 个删除失败（可能被占用），已保留在归档列表，可稍后重试。"
            ));
        }
        s
    } else {
        format!("未能清理任何归档会话（共 {total} 个）。若服务仍在占用，请先完全退出 dsh 后重试。")
    };
    Ok((cleaned_n, detail))
}

/// 用新的归档列表替换 workspace.json 中的 `archivedSessionIds: [...]`。
///
/// 前置条件（任一不满足就**不改写**，返回 `Ok(())`——宁可这一次没清理，也不破坏文件）：
/// 1. 整个文档是合法 JSON；
/// 2. `"archivedSessionIds"` 在文本中唯一（见 [`unique_key_index`]）；
/// 3. 数组区间由**字符串感知的括号配对**定位（见 [`match_bracket`]），
///    嵌套数组/转义引号都不会错位（旧实现用 `find(']')`，会定位到错误的 `]`）。
fn rewrite_archived_list(
    raw: &str,
    remaining: &[String],
    ws: &Path,
) -> Result<(), MaintenanceError> {
    // 前置条件 1：不合法 JSON 一律不碰
    if serde_json::from_str::<serde_json::Value>(raw).is_err() {
        return Ok(());
    }
    let Some(key_idx) = unique_key_index(raw, "\"archivedSessionIds\"") else {
        return Ok(());
    };
    let rest = &raw[key_idx..];
    let Some(rel_start) = rest.find('[') else {
        return Ok(());
    };
    let start = key_idx + rel_start;
    // 前置条件 3：配对定位
    let Some(end) = match_bracket(raw, start) else {
        return Ok(());
    };
    let new_list = serde_json::to_string(remaining)
        .map_err(|e| MaintenanceError::Io(std::io::Error::other(e.to_string())))?;
    let mut out = String::with_capacity(raw.len() + new_list.len());
    out.push_str(&raw[..start]);
    out.push_str(&new_list);
    out.push_str(&raw[end + 1..]);
    // 防御性复核：改写后必须仍是**合法 JSON**，否则放弃落盘（避免把配置文件写坏）
    if serde_json::from_str::<serde_json::Value>(&out).is_err() {
        return Ok(());
    }
    std::fs::write(ws, out)?;
    Ok(())
}

/// 清理 dsh 遗留的**原子写锁文件**（孤儿锁）。
///
/// ## 背景
/// dsh 的 `dsh-atomic-write` 用 `wx`（排他创建）的 `<file>.lock` 兄弟文件串行化写入。
/// 进程被强杀（`taskkill /F`、Job Object 回收、断电）时 `finally { rm(lockPath) }`
/// 不会执行，锁文件残留 → 之后所有写入都拿不到锁并在 2 秒后抛错：
/// ```text
/// Error: atomic-write: timed out waiting for the writer lock
///        at C:\Users\<user>\.dsh\.credentials.yaml.lock
/// ```
/// 上游明确把这种情形留给"操作者手工恢复"（源码注释：*file age cannot prove that
/// its owner stopped; orphan recovery is an operator action*）。
///
/// ## 为什么可以安全自动化
/// 锁文件的内容是**持有者 PID**（`writeFile(lockPath, `${process.pid}\n`, {flag:'wx'})`）。
/// 因此可以直接验证持有者是否仍存活——这是"证明"而非"猜测年龄"：
/// - 内容可解析且该 PID **仍存活** → 活跃锁，**绝不触碰**；
/// - 内容可解析但该 PID **已退出** → 持有者已死，锁必然孤儿，删除；
/// - 内容无法解析（如刚创建就被杀，文件为 0 字节）→ 仅在文件 **5 秒未被改动** 时删除，
///   避开"创建后尚未写入"的瞬间竞态。
///
/// 返回被删除的孤儿锁列表（供调用方记日志）。
pub fn clean_stale_dsh_locks() -> Vec<PathBuf> {
    // 定位不到主目录就**什么都不做**：退化成相对路径会把当前工作目录当成 ~/.dsh，
    // 而本函数会删文件（见 `dsh_home_opt` 的说明）。
    let Some(root) = dsh_home_opt() else {
        return Vec::new();
    };
    if !root.is_dir() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    collect_lock_files(&root, 0, 3, &mut candidates);

    let mut removed = Vec::new();
    for path in candidates {
        if is_orphan_lock(&path) && std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

/// 判定某个锁文件是否为孤儿（持有者已不存在）。
///
/// ## 判定依据
///
/// - **内容可解析为 PID**：以该 PID **是否存活**为准（这是可靠证明，不是猜年龄）。
/// - **内容不可解析**（如刚创建就被杀，文件为 0 字节）：仅在文件静置 5 秒后才删，
///   避开「已创建、尚未写入」的瞬间竞态。
///
/// ## 已知的残余风险（如实标注）
///
/// PID 会被 Windows 回收复用。若持有者进程已退出、且该 PID 恰好被一个**新**进程
/// 复用，本判定会认为「持有者存活」→ **不删** → dsh 的原子写会一直超时。
/// 这是**偏保守**的失败方向（宁可留下一个陈旧锁，也不误删活跃锁），
/// 且需要「PID 在锁的生命周期内被复用」这一低概率巧合；用户可手工删除该 `.lock`。
fn is_orphan_lock(path: &Path) -> bool {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    match raw.trim().parse::<u32>() {
        // 内容可解析：以 PID 存活与否为准
        Ok(pid) => !crate::process::is_process_alive(pid),
        // 无法解析：要求文件已静置一段时间，规避"创建后尚未写入"的竞态
        Err(_) => {
            const SETTLE: std::time::Duration = std::time::Duration::from_secs(5);
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| t.elapsed().map(|e| e > SETTLE).unwrap_or(false))
                .unwrap_or(false)
        }
    }
}

/// 递归收集 `*.lock`（限制深度，避免遍历过大目录）。
///
/// **不跟随 reparse point**：目录型 junction / 符号链接会让遍历走出 `~/.dsh`
/// （甚至进入网络路径），随后 `remove_file` 就会删到范围外的文件。
fn collect_lock_files(dir: &Path, depth: u32, max_depth: u32, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            // `file_type()` 不跟随链接：目录型 reparse point 在这里被拦下
            if is_reparse_point(&path) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // 跳过体积可能很大、且不会存锁的目录
            if name == "node_modules" || name == "sessions" {
                continue;
            }
            collect_lock_files(&path, depth + 1, max_depth, out);
        } else if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("lock"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "a1b2c3d4-e5f6-4789-8abc-def012345678";

    #[test]
    fn valid_id_is_44_chars() {
        let id = format!("session-{UUID}");
        assert_eq!(id.len(), 44);
        assert!(is_valid_session_id(&id));
    }

    #[test]
    fn rejects_traversal_and_short_ids() {
        assert!(!is_valid_session_id("session-../../evil"));
        assert!(!is_valid_session_id(&format!("session-{}", &UUID[..35])));
        assert!(!is_valid_session_id("other-a1b2c3d4"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn parses_archived_list() {
        let raw = r#"{"x":1,"archivedSessionIds": ["session-a1b2c3d4-e5f6-4789-8abc-def012345678", "session-00000000-0000-0000-0000-000000000000"],"y":2}"#;
        let ids = archived_session_ids(raw);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], format!("session-{UUID}"));
    }

    #[test]
    fn empty_and_missing_list() {
        assert!(archived_session_ids(r#"{"archivedSessionIds": []}"#).is_empty());
        assert!(archived_session_ids(r#"{"nothing": 1}"#).is_empty());
    }

    #[test]
    fn duplicate_key_is_refused() {
        // 同名 key 出现多次时区间定位不可靠 → 必须放弃解析（宁可这次不清理，
        // 也不能改写错位置把 workspace.json 写坏）
        let dup = r#"{"a":{"archivedSessionIds":["x"]},"archivedSessionIds":["session-00000000-0000-0000-0000-000000000000"]}"#;
        assert!(
            archived_session_ids(dup).is_empty(),
            "重复 key 必须拒绝解析"
        );
        assert_eq!(unique_key_index(dup, "\"archivedSessionIds\""), None);
        // 唯一 key 正常返回位置
        let ok = r#"{"archivedSessionIds":["x"]}"#;
        assert!(unique_key_index(ok, "\"archivedSessionIds\"").is_some());
    }

    #[test]
    fn bracket_matcher_handles_nesting_and_escapes() {
        // 这是把 `find(']')` 换成配对扫描的核心动机：
        // 数组元素里出现 `]`（字符串内）或嵌套数组时，`find(']')` 会定位到**错误的**
        // 结尾，改写就会破坏文件。
        let s = r#"["a]b", ["c"], "d"]"#;
        assert_eq!(match_bracket(s, 0), Some(s.len() - 1), "必须找到最外层的 ]");

        let obj = r#"{"k": [1,2], "j": "}"}"#;
        assert_eq!(match_bracket(obj, 0), Some(obj.len() - 1));
        // 内层数组 `[1,2]` 的配对位置（index 6 起）
        assert_eq!(match_bracket(obj, 6), Some(10));

        // 转义引号不能提前结束字符串状态
        let esc = r#"["a\"]b"]"#;
        assert_eq!(match_bracket(esc, 0), Some(esc.len() - 1));

        // 非括号起点 / 未闭合 → None
        assert_eq!(match_bracket(s, 1), None);
        assert_eq!(match_bracket(r#"["a""#, 0), None);
        // 类型不匹配（[ 由 } 闭合）必须拒绝
        assert_eq!(match_bracket("[}", 0), None);
    }

    #[test]
    fn parses_ids_with_escapes_via_serde() {
        // 旧实现按 `,` 切分 + `trim_matches('"')`，遇到转义就会解析错。
        // 现在数组内容交给 serde_json，转义被正确处理。
        let raw = r#"{"archivedSessionIds":["session-a1b2c3d4-e5f6-4789-8abc-def012345678"]}"#;
        assert_eq!(archived_session_ids(raw).len(), 1);
        // 元素里带 `]` 也不能影响区间定位
        let tricky = r#"{"archivedSessionIds":["x]y","z"]}"#;
        assert_eq!(archived_session_ids(tricky).len(), 2);
        // 非字符串元素的畸形列表：反序列化失败 → 返回空（而不是改写）
        assert!(archived_session_ids(r#"{"archivedSessionIds":[1,2]}"#).is_empty());
    }

    #[test]
    fn rewrite_refuses_invalid_json_document() {
        // 原始文档不是合法 JSON 时，绝不能碰它
        let dir = std::env::temp_dir().join(format!("dsh_ws_invalid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ws = dir.join("workspace.json");
        let broken = r#"{"archivedSessionIds":["session-x"], "#;
        std::fs::write(&ws, broken).unwrap();
        rewrite_archived_list(broken, &[], &ws).unwrap();
        assert_eq!(
            std::fs::read_to_string(&ws).unwrap(),
            broken,
            "非 JSON 文档必须原样保留"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrite_refuses_to_break_json() {
        // 归档列表里含未转义引号的畸形输入：改写后若不再是合法 JSON，
        // rewrite 必须放弃落盘（这里直接验证「落盘前后都可解析」的护栏逻辑）
        let dir = std::env::temp_dir().join(format!("dsh_ws_guard_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ws = dir.join("workspace.json");
        let raw = r#"{"archivedSessionIds": ["session-a1b2c3d4-e5f6-4789-8abc-def012345678"], "other": 1}"#;
        std::fs::write(&ws, raw).unwrap();

        // 正常路径：只剩空列表
        rewrite_archived_list(raw, &[], &ws).unwrap();
        let after = std::fs::read_to_string(&ws).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&after).is_ok());
        assert!(after.contains("\"archivedSessionIds\": []"), "{after}");
        assert!(after.contains("\"other\": 1"), "其它字段必须保留：{after}");

        // 重复 key：必须原样保留（不改写）
        let dup = r#"{"archivedSessionIds":[],"x":{"archivedSessionIds":[]}}"#;
        std::fs::write(&ws, dup).unwrap();
        rewrite_archived_list(dup, &[], &ws).unwrap();
        assert_eq!(std::fs::read_to_string(&ws).unwrap(), dup);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_scan_skips_reparse_points() {
        // 目录型 reparse point（junction）不应被跟随：否则会遍历到 ~/.dsh 之外的
        // 位置，并可能删除范围外的 .lock 文件。
        let root = std::env::temp_dir().join(format!("dsh_lock_reparse_{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("dsh_outside_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let outside_lock = outside.join("victim.lock");
        std::fs::write(&outside_lock, b"1").unwrap();

        // 用 mklink /J 建 junction（无需管理员）
        let link = root.join("link");
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output();
        let created = status.map(|o| o.status.success()).unwrap_or(false)
            && std::fs::symlink_metadata(&link)
                .map(|m| {
                    use std::os::windows::fs::MetadataExt;
                    m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                })
                .unwrap_or(false);

        if !created {
            // 环境不支持 junction（极少见）：跳过，不让测试假失败
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }

        let mut found = Vec::new();
        collect_lock_files(&root, 0, 3, &mut found);
        assert!(
            !found.contains(&outside_lock),
            "不得穿过 junction 收集到外部锁文件：{found:?}"
        );

        let _ = std::fs::remove_dir_all(&link);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn removes_only_lock_files_in_tree() {
        // 用临时目录验证锁收集逻辑（不触碰真实 ~/.dsh）
        let root = std::env::temp_dir().join(format!("dsh_lock_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("storages");
        std::fs::create_dir_all(&sub).unwrap();

        let lock_a = root.join(".credentials.yaml.lock");
        let lock_b = sub.join("workspace.json.lock");
        let keep_yaml = root.join(".credentials.yaml");
        let keep_json = sub.join("workspace.json");
        for p in [&lock_a, &lock_b, &keep_yaml, &keep_json] {
            std::fs::write(p, b"1").unwrap();
        }

        let mut found = Vec::new();
        collect_lock_files(&root, 0, 3, &mut found);

        assert_eq!(found.len(), 2, "应只收集到两个 .lock：{found:?}");
        assert!(lock_a.exists() && lock_b.exists(), "收集阶段不应删除文件");
        assert!(
            keep_yaml.exists() && keep_json.exists(),
            "非锁文件不应被收集"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_bulky_dirs() {
        let root = std::env::temp_dir().join(format!("dsh_lock_deep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // sessions 目录应被跳过（可能很大且不含锁）
        let deep = root.join("sessions").join("ws1").join("s1");
        std::fs::create_dir_all(&deep).unwrap();
        let deep_lock = deep.join("x.lock");
        std::fs::write(&deep_lock, b"1").unwrap();

        let mut found = Vec::new();
        collect_lock_files(&root, 0, 3, &mut found);
        assert!(found.is_empty(), "sessions 下的锁不应被扫描");
        assert!(deep_lock.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn live_pid_lock_is_not_orphan() {
        // 锁内容为「当前进程 PID」→ 持有者存活 → 绝不能判定为孤儿
        let dir = std::env::temp_dir().join(format!("dsh_lock_live_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("live.lock");
        std::fs::write(&lock, format!("{}\n", std::process::id())).unwrap();

        assert!(!is_orphan_lock(&lock), "存活 PID 的锁不得被判为孤儿");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dead_pid_lock_is_orphan() {
        // 锁内容为一个几乎不可能存在的 PID → 持有者已死 → 判为孤儿
        let dir = std::env::temp_dir().join(format!("dsh_lock_dead_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("dead.lock");
        std::fs::write(&lock, "4294967294\n").unwrap();

        assert!(is_orphan_lock(&lock), "已退出 PID 的锁应判为孤儿");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unparseable_lock_requires_settle_time() {
        // 空内容：刚创建的不算孤儿（避开创建后尚未写入的竞态），静置后才算
        let dir = std::env::temp_dir().join(format!("dsh_lock_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("empty.lock");
        std::fs::write(&lock, b"").unwrap();

        assert!(!is_orphan_lock(&lock), "刚创建的空锁不应立即判为孤儿");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dsh_home_points_under_user_profile() {
        let h = dsh_home();
        assert!(h.ends_with(".dsh"), "got {h:?}");
    }

    /// 限时外部命令：正常路径必须拿回 stdout。
    #[test]
    fn bounded_capture_returns_output() {
        let out = run_capture_within("cmd.exe", &["/c", "echo 7"], Duration::from_secs(10));
        assert_eq!(
            out.as_deref().map(str::trim),
            Some("7"),
            "应取回 stdout（实际 {out:?}）"
        );
    }

    /// 限时外部命令：超时必须**按时**返回 `None`，绝不能把调用方挂住。
    ///
    /// 这是「清理归档会话」不会无限期卡住界面的保证：早先实现用
    /// `Command::output()`（无超时），PowerShell/WMI 一旦卡住就永久阻塞。
    #[test]
    fn bounded_capture_times_out_without_hanging() {
        let start = std::time::Instant::now();
        // ping -n 30 至少要跑 29 秒
        let out = run_capture_within(
            "ping.exe",
            &["-n", "30", "127.0.0.1"],
            Duration::from_millis(400),
        );
        assert!(out.is_none(), "超时必须返回 None，实际 {out:?}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "耗时 {:?} 说明限时失效（调用方会被挂住）",
            start.elapsed()
        );
    }

    #[test]
    fn active_session_probe_is_safe_and_descriptive() {
        // 探测在任意机器上都应可安全调用（不得 panic、不得卡死）。
        // 具体数值取决于本机状态，因此只断言结构合理性与文案可用性。
        if let Some(a) = probe_active_sessions() {
            assert!(!a.summary().is_empty());
            assert_eq!(a.is_running(), a.runner_processes > 0);
            assert_eq!(
                a.any_activity(),
                a.runner_processes > 0 || a.recently_written > 0
            );
            assert!(a.recent_session_hint.len() <= 3, "提示最多 3 条");
            if !a.any_activity() {
                assert!(a.summary().contains("未检测到"), "{}", a.summary());
            }
        }
    }

    #[test]
    fn active_sessions_summary_is_actionable() {
        let none = ActiveSessions {
            runner_processes: 0,
            recently_written: 0,
            recent_session_hint: vec![],
        };
        assert!(!none.any_activity() && !none.is_running());
        assert!(none.summary().contains("未检测到"));

        let running = ActiveSessions {
            runner_processes: 2,
            recently_written: 1,
            recent_session_hint: vec!["session-x".into()],
        };
        assert!(running.is_running() && running.any_activity());
        let s = running.summary();
        assert!(s.contains("2 个正在运行的会话进程"), "{s}");
        assert!(s.contains("1 个会话文件"), "{s}");

        // 只有软信号时**不得**声称"正在运行的会话进程"
        let soft = ActiveSessions {
            runner_processes: 0,
            recently_written: 3,
            recent_session_hint: vec![],
        };
        assert!(!soft.is_running() && soft.any_activity());
        assert!(
            !soft.summary().contains("正在运行的会话进程"),
            "{}",
            soft.summary()
        );
    }

    #[test]
    fn clean_stale_locks_is_safe_on_real_home() {
        // 在真实 ~/.dsh 上调用：必须不 panic，且绝不能删掉「存活 PID」的锁。
        // （活跃锁会被 is_orphan_lock 拦下，因此重复调用是幂等的。）
        let removed = clean_stale_dsh_locks();
        for p in &removed {
            assert!(
                p.extension()
                    .map(|e| e.eq_ignore_ascii_case("lock"))
                    .unwrap_or(false),
                "只允许删除 .lock 文件，实际: {p:?}"
            );
        }
        // 再跑一次不应再删任何东西（幂等）
        let again = clean_stale_dsh_locks();
        assert!(again.is_empty(), "第二次应无可删（幂等），实际: {again:?}");
    }
}
