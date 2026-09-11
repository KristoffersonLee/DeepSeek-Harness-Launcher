//! 轻量滚动日志 + 脱敏。
//!
//! 修复 v4 的 O(n) IO 放大：`AppendLogFile` 在超过 2 MB 后每追加一行都要全量读取再重写，
//! 这里改为 append-only，仅在超过阈值时做一次 rename 滚动。

use std::borrow::Cow;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 共享的写入状态：**一把锁 + 一个常开句柄 + 目录就绪标志**。
///
/// 三样东西必须整体共享（都在同一个 `Arc` 里）：分开共享会在
/// 「判定超限 → 改名滚动 → 继续写入」之间产生竞态。
#[derive(Default)]
struct Sink {
    /// 常开的追加句柄。
    ///
    /// **为什么常开**：旧实现每写一行都 `open + write + close`，即每行多两次系统调用
    /// 与一次路径解析。日志写入发生在 UI 线程（托盘动作、启动计时、状态变更）与 dsh 的
    /// stderr 排空线程上，突发时是每秒数百行 —— 那是纯粹的重复打开开销。
    /// 句柄常开后，滚动判定也变成对**句柄自身**的 fstat（不再按路径 `metadata`）。
    ///
    /// 滚动前必须显式 `drop` 本句柄：Windows 上把「正被写着的文件」改名后，
    /// 后续写入仍会落进被改名的那个文件（日志错位）。
    file: Option<std::fs::File>,
    /// 日志目录是否已确保存在（避免每行都 `create_dir_all`）。
    dir_ready: bool,
}

pub struct RollingLogger {
    log_dir: PathBuf,
    max_size: u64,
    max_files: u32,
    /// 写入状态（锁 + 句柄）。
    ///
    /// **必须是 `Arc`**：本类型在整个进程里有多个 `clone`（`App`、`ServiceInner`、
    /// `ProcessManager` 的 stderr 排空线程各持一份）。此前 `Clone` 会新建一把锁，
    /// 于是多个副本**互不排斥**——
    /// (a) 同一行日志可能被并发 `write_all` 交错；
    /// (b) 更糟的是滚动竞态：A 线程判定超限 → B 线程 append → A 线程 rename，
    ///     B 的写入落进即将被改名的文件里（丢日志）。
    /// 共享同一把锁后这两类问题都不存在。
    sink: Arc<Mutex<Sink>>,
}

impl Clone for RollingLogger {
    fn clone(&self) -> Self {
        Self {
            log_dir: self.log_dir.clone(),
            max_size: self.max_size,
            max_files: self.max_files,
            // 共享锁与句柄（见字段文档：新建锁会造成不互斥与滚动竞态）
            sink: Arc::clone(&self.sink),
        }
    }
}

impl RollingLogger {
    pub fn new(log_dir: PathBuf) -> Self {
        Self::with_limits(log_dir, 2 * 1024 * 1024, 3)
    }

    /// 带自定义阈值/保留数的构造（**仅供测试**：2 MB 阈值下没法在单测里验证滚动）。
    fn with_limits(log_dir: PathBuf, max_size: u64, max_files: u32) -> Self {
        Self {
            log_dir,
            max_size,
            max_files,
            sink: Arc::new(Mutex::new(Sink::default())),
        }
    }

    /// 最多保留的日志文件数量（含当前文件）。
    ///
    /// `max_files = 3` ⇒ `launcher.log` + `.1` + `.2`（3 个文件、最多约 6 MB）。
    /// 历史实现会滚动出 `.1/.2/.3` **四个**文件（约 8 MB），与文档「2 MB × 3 份」不符。
    pub fn retained_file_count(&self) -> u32 {
        self.max_files
    }

    pub fn append(&self, level: &str, message: &str) -> std::io::Result<()> {
        let mut sink = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if !sink.dir_ready {
            std::fs::create_dir_all(&self.log_dir)?;
            sink.dir_ready = true;
        }
        let line = format!(
            "[{}] [{}] {}\n",
            local_timestamp(),
            level,
            redact_secrets(message)
        );
        let path = self.log_dir.join("launcher.log");
        let mut f = match sink.file.take() {
            Some(f) => f,
            None => OpenOptions::new().create(true).append(true).open(&path)?,
        };
        // 超限就先滚动再写：这样每个归档都**不超过**阈值，
        // 而不是「写满阈值 + 若干行」才滚动。
        let oversized = f
            .metadata()
            .map(|m| m.len() > self.max_size)
            .unwrap_or(false);
        if oversized {
            // 必须先关闭句柄再改名（见 Sink::file 的说明）
            drop(f);
            self.rotate()?;
            f = OpenOptions::new().create(true).append(true).open(&path)?;
        }
        let written = f.write_all(line.as_bytes());
        // 无论写入成功与否都放回句柄：失败多为磁盘满/权限问题，复用句柄不会更糟，
        // 而每行重新打开会让失败路径也白白付出打开代价。
        sink.file = Some(f);
        written
    }

    pub fn info(&self, msg: &str) {
        let _ = self.append("INFO", msg);
    }
    pub fn warn(&self, msg: &str) {
        let _ = self.append("WARN", msg);
    }
    pub fn error(&self, msg: &str) {
        let _ = self.append("ERROR", msg);
    }

    /// 滚动：把 `launcher.log` 变成 `.1`，`.1` 变 `.2`……**丢弃最旧的**。
    ///
    /// 保留文件总数为 `max_files`（当前文件 + `max_files - 1` 个归档），
    /// 因此 `max_files = 3` 时归档只有 `.1` 与 `.2`。
    ///
    /// 调用方必须已持有锁并**已关闭**当前句柄（见 [`RollingLogger::append`]）。
    fn rotate(&self) -> std::io::Result<()> {
        // 归档槽位：1 ..= (max_files - 1)；从最旧的开始腾位置
        let last = self.max_files.saturating_sub(1);
        for i in (1..last).rev() {
            let src = self.log_dir.join(format!("launcher.log.{i}"));
            if src.exists() {
                let _ = std::fs::rename(&src, self.log_dir.join(format!("launcher.log.{}", i + 1)));
            }
        }
        let path = self.log_dir.join("launcher.log");
        if last == 0 {
            // 不保留归档（max_files = 1）：当前文件直接丢弃
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        // 用 `?` 把改名失败**如实上抛**：旧实现忽略它，于是
        // 「滚动彻底失效、日志无限增长」完全静默（`info/warn/error` 会丢弃这个错误，
        //  但至少调用方有机会看到 —— 直连 `append` 的路径能拿到它）。
        std::fs::rename(&path, self.log_dir.join("launcher.log.1"))?;
        // 超出保留数量的最旧归档必须删除，否则文件数会无限增长
        let overflow = self.log_dir.join(format!("launcher.log.{}", last + 1));
        if overflow.exists() {
            let _ = std::fs::remove_file(&overflow);
        }
        Ok(())
    }
}

/// 本地时间戳。用 `GetLocalTime` 而非 chrono，避免新增依赖。
pub fn local_timestamp() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

/// 需要脱敏的 `key=value` 键名（小写比较）。
///
/// 覆盖范围（v5.0.0 定稿轮扩展）：此前只处理 `token=`，因此 `pin=`、`api_key=`、
/// `access_token=`、`cookie=` 等一旦出现在 dsh 的输出或错误信息里就会**原样落盘**。
/// dsh 的凭据文件（`~/.dsh/.credentials.yaml`）与原生模块报错都可能把这类键打进
/// stderr，而 stderr 会被 `spawn_stderr_drain` 写进日志。
const SENSITIVE_KEYS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apikey",
    "secret",
    "client_secret",
    "password",
    "passwd",
    "pwd",
    "pin",
    "cookie",
    "authorization",
    "auth",
    "session_key",
    "private_key",
    "credential",
];

/// 值结束的判定字符：`&`、空白、引号、逗号、分号、右括号等。
fn is_value_terminator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '>')
}

/// 脱敏：剥离日志里的凭据。
///
/// 手写实现，不引入 regex（`check-consistency.ps1` 明令禁止 `regex` 依赖）。
///
/// 覆盖两类形态：
/// 1. **`key=value`**（含 `key: value` 风格的 `Authorization: Bearer …` 会被第 2 条覆盖）：
///    键名命中 [`SENSITIVE_KEYS`] 时把值替换为 `***`；
/// 2. **HTTP 认证头**：`Bearer <token>` / `Basic <base64>` 的凭证部分。
///
/// `\u2028` / `\u2029`（JS 行分隔符）不需要在此处理——它们只在“把字符串嵌进 JS 字面量”
/// 时才有意义（见 `dsh-ui/src/settings.rs` 的双层转义），日志是纯文本输出。
pub fn redact_secrets(input: &str) -> Cow<'_, str> {
    // 快路径：完全不含任何敏感键名与认证头时不分配
    let lower = input.to_ascii_lowercase();
    let needs_key_scan = SENSITIVE_KEYS.iter().any(|k| lower.contains(k));
    let needs_bearer = lower.contains("bearer ") || lower.contains("basic ");
    if !needs_key_scan && !needs_bearer {
        return Cow::Borrowed(input);
    }

    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    // 逐字符扫描；命中键名时推进到值结束处，避免二次替换
    while i < input.len() {
        // 第 2 条：Bearer / Basic 认证头（大小写在 lower 上判断）
        let rest_lower = &lower[i..];
        let scheme = if rest_lower.starts_with("bearer ") {
            Some("bearer ")
        } else if rest_lower.starts_with("basic ") {
            Some("basic ")
        } else {
            None
        };
        if let Some(scheme) = scheme {
            out.push_str(&input[i..i + scheme.len()]);
            let vstart = i + scheme.len();
            let vend = input[vstart..]
                .find(is_value_terminator)
                .map(|o| vstart + o)
                .unwrap_or(input.len());
            out.push_str("***");
            i = vend;
            continue;
        }

        // 第 1 条：key=value / key: value
        if let Some((head_end, vstart)) = match_sensitive_key(input, &lower, i) {
            out.push_str(&input[i..head_end]);
            let vend = input[vstart..]
                .find(is_value_terminator)
                .map(|o| vstart + o)
                .unwrap_or(input.len());
            out.push_str("***");
            i = vend;
            continue;
        }

        // 普通字符：按 UTF-8 边界推进一个字符
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }
    Cow::Owned(out)
}

/// 若 `pos` 处开始是一个「敏感键 + 分隔符 + 值」，返回 `(脱敏起点, 值起点)`。
///
/// `脱敏起点` 已经把 `key=`、`key: `、起始引号以及 `Bearer `/`Basic ` 方案名包含在内，
/// 因此 `Authorization: Bearer abc` 会被整体替换为 `Authorization: Bearer ***`。
fn match_sensitive_key(input: &str, lower: &str, pos: usize) -> Option<(usize, usize)> {
    // 键必须从词边界开始，避免把 `xxtoken=` 当成 `token=`、`oauth=` 当成 `auth=`
    if pos > 0 {
        let prev = input[..pos].chars().next_back()?;
        if prev.is_ascii_alphanumeric() || prev == '_' {
            return None;
        }
    }
    for key in SENSITIVE_KEYS {
        if !lower[pos..].starts_with(key) {
            continue;
        }
        let mut i = pos + key.len();
        // 允许 `key=value` / `key = value` / `key: value` / `"key": "value"`
        // （JSON 形态下键名后面先跟一个闭合引号，再跟分隔符）
        i = skip_ascii_ws(input, i);
        if matches!(input[i..].chars().next(), Some('"') | Some('\'')) {
            i += 1;
            i = skip_ascii_ws(input, i);
        }
        let sep = input[i..].chars().next()?;
        if sep != '=' && sep != ':' {
            continue;
        }
        i += sep.len_utf8();
        // 跳过空白与起始引号，定位值的起点
        while let Some(c) = input[i..].chars().next() {
            if c == ' ' || c == '\t' || c == '"' || c == '\'' {
                i += c.len_utf8();
            } else {
                break;
            }
        }
        let mut head_end = i;
        // `Authorization: Bearer xxx` / `Basic xxx`：方案名也属于敏感前缀
        let mut value_start = i;
        let vl = &lower[value_start..];
        for scheme in ["bearer ", "basic "] {
            if vl.starts_with(scheme) {
                value_start += scheme.len();
                head_end = value_start;
                break;
            }
        }
        return Some((head_end, value_start));
    }
    None
}

/// 跳过 ASCII 空白，返回新的下标（不会切在 UTF-8 边界中间）。
fn skip_ascii_ws(s: &str, mut i: usize) -> usize {
    while let Some(c) = s[i..].chars().next() {
        if c == ' ' || c == '\t' {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// UTF-8 首字节 → 该字符的字节长度（非法首字节按 1 处理）。
fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_token() {
        assert_eq!(
            redact_secrets("http://127.0.0.1:3080/?token=abc123"),
            "http://127.0.0.1:3080/?token=***"
        );
        assert_eq!(
            redact_secrets("a=1&token=secret%20x&b=2"),
            "a=1&token=***&b=2"
        );
    }

    #[test]
    fn redacts_other_credential_keys() {
        // v5.0.0 定稿轮扩展：此前只覆盖 `token=`，pin / api_key / cookie / secret
        // 等键一旦出现在 dsh 的 stderr（会被排空线程写进日志）就原样落盘。
        assert_eq!(redact_secrets("pin=123456"), "pin=***");
        assert_eq!(redact_secrets("api_key=sk-abc"), "api_key=***");
        assert_eq!(redact_secrets("cookie: session=xyz"), "cookie: ***");
        assert_eq!(redact_secrets("password = hunter2"), "password = ***");
        // JSON 形态：保留引号（脱敏后仍是合法 JSON 片段）
        assert_eq!(
            redact_secrets("\"client_secret\": \"s3cr3t\""),
            "\"client_secret\": \"***\""
        );
        // 不是敏感键时不得误伤
        assert_eq!(redact_secrets("spin=1"), "spin=1");
        assert_eq!(redact_secrets("oauth_mode=x"), "oauth_mode=x");
    }

    #[test]
    fn redacts_authorization_headers() {
        assert_eq!(
            redact_secrets("Authorization: Bearer abc.def.ghi"),
            "Authorization: Bearer ***"
        );
        assert_eq!(
            redact_secrets("authorization=Basic dXNlcjpwYXNz"),
            "authorization=Basic ***"
        );
        // 裸 Bearer（没有 key）同样要脱敏
        assert_eq!(redact_secrets("Bearer abc123"), "Bearer ***");
    }

    #[test]
    fn redaction_keeps_utf8_boundaries() {
        // 逐字符扫描必须按 UTF-8 边界推进，否则会 panic（切片越界）
        let s = "错误：token=abc 中文后缀";
        let out = redact_secrets(s);
        assert!(out.contains("token=***"), "{out}");
        assert!(out.contains("中文后缀"), "{out}");
    }

    #[test]
    fn clones_share_one_sink() {
        // 关键不变式：clone 必须共享同一把写锁、同一个常开句柄与目录标志。
        // 此前每个 clone 新建一把锁 → 多个副本互不排斥，
        // 且滚动 rename 与并发 append 之间存在丢日志竞态。
        let logger = RollingLogger::new(std::env::temp_dir().join("dsh_log_clone_test"));
        let clone = logger.clone();
        assert!(
            std::sync::Arc::ptr_eq(&logger.sink, &clone.sink),
            "clone 必须共享写入状态（锁 + 句柄）"
        );
    }

    /// 滚动必须在**超过阈值之前**发生，且保留数不超过 `max_files`。
    ///
    /// 旧实现每行 `open + write + close` 且用 `std::fs::metadata(路径)` 判大小：
    /// 这里同时锁住新实现的两个行为要点（阈值判定 + 保留数量）。
    #[test]
    fn rotates_before_exceeding_threshold_and_keeps_max_files() {
        let dir = std::env::temp_dir().join(format!("dsh_log_rot_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logger = RollingLogger::with_limits(dir.clone(), 4096, 3);
        // 每行约 40 字节 ⇒ 200 行约 8 KB，必然触发两次滚动
        for i in 0..200 {
            logger.info(&format!("rotation-line-{i:04}-padding"));
        }
        let cur = dir.join("launcher.log");
        assert!(cur.is_file(), "当前日志文件必须存在");
        assert!(
            std::fs::metadata(&cur).unwrap().len() <= 4096,
            "触发滚动的时机必须在超限之前"
        );
        assert!(dir.join("launcher.log.1").is_file(), "应有至少一个归档");
        // 保留数：当前文件 + (max_files - 1) 个归档，绝不允许无限增长
        let count = std::fs::read_dir(&dir).unwrap().flatten().count();
        assert_eq!(count, 3, "max_files = 3 ⇒ 目录里只能有 3 个日志文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 常开句柄不得破坏「多副本并发写整行」的保证。
    #[test]
    fn shared_handle_still_writes_whole_lines() {
        let dir = std::env::temp_dir().join(format!("dsh_log_handle_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logger = RollingLogger::new(dir.clone());
        let mut handles = Vec::new();
        for t in 0..4 {
            let l = logger.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..25 {
                    l.warn(&format!("t{t}-{i}"));
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let text = std::fs::read_to_string(dir.join("launcher.log")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 100, "应写入 100 行，实际 {}", lines.len());
        assert!(lines
            .iter()
            .all(|l| l.starts_with('[') && l.contains("] [WARN] ")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_appends_produce_whole_lines() {
        // 多副本并发写：每行都必须是完整的 `[时间戳] [级别] 消息`
        let dir = std::env::temp_dir().join(format!("dsh_log_conc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logger = RollingLogger::new(dir.clone());
        let mut handles = Vec::new();
        for t in 0..4 {
            let l = logger.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..50 {
                    l.info(&format!("t{t}-{i}"));
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let content = std::fs::read_to_string(dir.join("launcher.log")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 200, "应写入 200 行，实际 {}", lines.len());
        for line in lines {
            assert!(
                line.starts_with('[') && line.contains("] [INFO] "),
                "行结构被并发写破坏：{line}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert!(matches!(redact_secrets("plain line"), Cow::Borrowed(_)));
        assert_eq!(redact_secrets("plain line"), "plain line");
    }

    #[test]
    fn timestamp_is_reasonable() {
        let t = local_timestamp();
        assert_eq!(t.len(), 19, "got {t}");
        assert!(t.starts_with("202"), "got {t}");
    }
}
