//! dsh 环境解析，以及 `dsh web` 启动行的解析。
//!
//! 解析逻辑严格对齐 v4 `Engine.Resolve`：
//! 手动指定 → PATH → `%LOCALAPPDATA%\nodejs\node-v*`（取最高版本）→ `%ProgramFiles%\nodejs`。
//!
//! ## 本层只负责「找到 dsh」与「读懂它的启动行」
//!
//! 只有两件事：
//! 1. [`Dsh::resolve`] —— 定位 `node.exe` 与 `@deepseek-ai/dsh` 的 `bin.js`；
//! 2. [`parse_ready_url`] / [`looks_like_ready_line`] / [`is_loopback_url`] —— 解析
//!    `dsh web` 打到 stdout 的带 token 地址（界面能否打开全靠这一步）。
//!
//! ## 已删除的能力（v5.0.0 LTS 定稿轮）
//!
//! `version` / `dist_tags` / `upgrade` / `check_health` 四个方法**已彻底删除**，
//! 连同只服务于它们的 `is_safe_version`、`run_capture`、`resolve_npm` 与
//! `DshError` 的 5 个变体（`NpmNotFound` / `NpmFailed` / `InvalidVersion` /
//! `VersionUnavailable` / `NativeModuleMissing`）。
//!
//! 删除理由（此前一直以"公开库 API，不进调用图所以无害"为理由保留）：
//! - **它们是死代码**：从 v5 重写起就没有任何调用方，`dsh-app` 侧的包装层也早已删除；
//! - "不进调用图"只对体积成立，对**维护成本**不成立：它们是一整条 npm 调用链
//!   （定位 `npm.cmd`、拼 registry 参数、超时 + 进程树回收、JSON 解析），
//!   会持续跟着上游 npm 输出格式与本地环境漂移，却没有任何可执行的用户入口；
//! - 用户能力本身没有丢：升级 / 修复原生模块的手动步骤完整保留在
//!   `docs/MAINTENANCE.zh.md` §1.3–1.5（升级命令与 `node-gyp rebuild`），
//!   那是**唯一**被文档承诺、也被实际使用的路径。
//!
//! 原本由 `is_safe_version` 承担的安全属性（"绝不把外部返回值拼进命令行"）
//! 现在由**结构**保证，并由 `tools/check-consistency.ps1` 强制：本 crate 的
//! 生产代码里不存在任何 `cmd.exe` / shell 拼串调用，所有进程调用一律走参数数组。

use std::path::{Path, PathBuf};

pub struct Dsh {
    pub node: PathBuf,
    pub bin_js: PathBuf,
}

impl Dsh {
    /// 定位 node.exe 与 dsh 的 bin.js。
    pub fn resolve(node_path_override: Option<&str>) -> Result<Self, DshError> {
        let node = Self::resolve_node(node_path_override)?;
        let bin_js = Self::resolve_bin_js(&node)?;
        Ok(Self { node, bin_js })
    }

    fn resolve_node(override_path: Option<&str>) -> Result<PathBuf, DshError> {
        // 1) 手动指定（校验文件名必须为 node.exe，防止配置被篡改后启动任意程序）。
        //
        // **校验失败必须报错，不能静默回退 PATH**（v5.0.0 定稿轮修正）：
        // 用户配置了 `node_path` 就说明他想用那个 node（常见原因：多版本共存、
        // PATH 上的 node 版本不对）。旧实现在路径无效时**静默**改用 PATH 上的 node，
        // 用户以为自定路径生效了，实际跑的是另一个 node —— 这类"静默降级"是最难排查的。
        if let Some(p) = override_path {
            let p = p.trim();
            if !p.is_empty() {
                let path = PathBuf::from(p);
                let name_ok = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case("node.exe"))
                    .unwrap_or(false);
                if !name_ok {
                    return Err(DshError::InvalidNodePath(format!(
                        "{p}（文件名不是 node.exe）"
                    )));
                }
                if !path.is_file() {
                    return Err(DshError::InvalidNodePath(format!("{p}（文件不存在）")));
                }
                return Ok(path);
            }
        }

        // 2) PATH
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let cand = dir.join("node.exe");
                if cand.is_file() {
                    return Ok(cand);
                }
            }
        }

        // 3) %LOCALAPPDATA%\nodejs\node-v* —— 多版本共存时取最高版本
        if let Some(la) = dirs::data_local_dir() {
            let base = la.join("nodejs");
            if base.is_dir() {
                if let Some(best) = highest_node_version_dir(&base) {
                    return Ok(best);
                }
                let cand = base.join("node.exe");
                if cand.is_file() {
                    return Ok(cand);
                }
            }
        }

        // 4) %ProgramFiles%\nodejs\node.exe
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let cand = Path::new(&pf).join("nodejs").join("node.exe");
            if cand.is_file() {
                return Ok(cand);
            }
        }

        Err(DshError::NodeNotFound)
    }

    fn resolve_bin_js(node: &Path) -> Result<PathBuf, DshError> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(dir) = node.parent() {
            candidates.push(
                dir.join("node_modules")
                    .join("@deepseek-ai")
                    .join("dsh")
                    .join("lib")
                    .join("bin.js"),
            );
        }
        if let Some(roaming) = dirs::config_dir() {
            candidates.push(
                roaming
                    .join("npm")
                    .join("node_modules")
                    .join("@deepseek-ai")
                    .join("dsh")
                    .join("lib")
                    .join("bin.js"),
            );
        }
        candidates
            .into_iter()
            .find(|c| c.is_file())
            .ok_or(DshError::BinJsNotFound)
    }
}

/// 仅解析「已安装的 node.exe」，**不理会用户配置的覆盖路径**。
///
/// 用途：进程身份判定（[`crate::process::is_dsh_harness_process`]）。用户配置的
/// `node_path` 可以是任意路径，拿它做身份判定会把语义搞坏；这里只回答
/// 「本机常规安装位置上的 node.exe 是哪个」。
///
/// 找不到时返回 `None`（调用方应视为「无法确认」，而不是「不是」）。
///
/// ## 结果缓存
///
/// 身份判定会对**每个候选 PID** 调用本函数（`classify_dsh_identity` 的依据 3b），
/// 而每次解析都要遍历 PATH 上的每个目录做 `is_file()` 探测——在进程扫描/失联监测
/// 这类循环里是纯粹的重复 IO。node 的安装位置在**一次进程运行期间**不会变化，
/// 因此结果用 `OnceLock` 缓存。
///
/// 注意：**仅身份判定走缓存**。真正启动服务时仍用 [`Dsh::resolve`] 现算，
/// 因此"运行时新装了 Node"依然会被立刻发现。
pub fn resolve_installed_node() -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Dsh::resolve_node(None).ok()).clone()
}

/// 从 dsh web 的**启动输出**中解析带认证 token 的访问地址。
///
/// 为什么需要它：`dsh web` 的浏览器界面要求**一次性 token**，直接访问
/// `http://127.0.0.1:<port>/` 会返回 **HTTP 401**（实测）。token 只出现两处：
///   1. dsh 自动拉起浏览器时用的地址；
///   2. dsh 打到 stdout 的启动行（`printUrl` 默认为 true）：
///      `dsh web: http://127.0.0.1:3080/?token=xxxx`，若同机存在局域网地址还会追加
///      ` (LAN: http://…?token=…)`。
///
/// 因此启动器必须**读 dsh 的 stdout** 才能拿到可用地址；此前实现把 stdout 接成管道
/// 却从不读取，导致托盘“打开界面”/“在浏览器中打开”只能拿到无 token 的普通地址，
/// 用户看到的就是认证失败页。
///
/// 解析规则：匹配 `dsh web:` 前缀，取第一个 `http(s)://` 起的非空白串，
/// 并剥掉尾部的 `(LAN: …)`。返回 `None` 表示该行不是启动行（可能是普通日志）。
pub fn parse_ready_url(line: &str) -> Option<String> {
    let idx = line.find("dsh web:")?;
    let rest = line[idx + "dsh web:".len()..].trim_start();
    if !rest.starts_with("http") {
        return None;
    }
    // URL 以空白结束；同时丢弃同行的 " (LAN: ...)" 提示
    let url = rest.split_whitespace().next()?.trim_end_matches(',');
    if url.len() <= "http://".len() {
        return None;
    }
    Some(url.to_string())
}

/// 启动行的判定与解析（供子进程 stdout 读取线程使用）。
///
/// 同时识别 dsh 的“正在打开默认浏览器”提示行，便于在日志里说明该行为已被
/// `--no-open` 关闭（否则用户会以为是启动器打开了浏览器）。
pub fn is_browser_open_notice(line: &str) -> bool {
    line.contains("opening the default browser")
}

/// 该行「看起来应当是一条就绪地址行」，但 [`parse_ready_url`] 没能解析出地址。
///
/// ## 为什么需要它
///
/// `parse_ready_url` 依赖上游 dsh 的输出格式（`dsh web: <url>`）。上游一旦改格式
/// （换前缀、加颜色码、把地址挪到 stderr…），解析会**静默返回 `None`**：
/// 表现是「界面永远等不到 token → 用户看到 401 或空白页」，而日志里没有任何线索。
/// 这条判定让读取线程能明确留痕，把"静默失效"变成"可诊断的告警"。
pub fn looks_like_ready_line(line: &str) -> bool {
    line.contains("dsh web")
        || line.contains("127.0.0.1")
        || line.contains("localhost")
        || line.contains("token=")
}

/// 判断地址是否指向本机回环（dsh web 默认只绑 `127.0.0.1`）。
///
/// 用途：拿到的地址若不是回环，说明上游行为变了（例如默认绑 0.0.0.0 并只打印
/// 局域网地址）。这时**不能**把内嵌窗口导航过去（会把窗口指向非本机地址），
/// 调用方应改走普通回环地址并告警。
pub fn is_loopback_url(url: &str) -> bool {
    const PREFIXES: [&str; 4] = [
        "http://127.0.0.1:",
        "https://127.0.0.1:",
        "http://localhost:",
        "https://localhost:",
    ];
    PREFIXES.iter().any(|p| url.starts_with(p))
}

/// 在 `base` 下找 `node-v*` 目录中版本号最高的 node.exe。
fn highest_node_version_dir(base: &Path) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, (u32, u32, u32))> = None;
    let rd = std::fs::read_dir(base).ok()?;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(ver) = parse_node_version(&name) else {
            continue;
        };
        let cand = entry.path().join("node.exe");
        if !cand.is_file() {
            continue;
        }
        if best.as_ref().is_none_or(|(_, b)| &ver > b) {
            best = Some((cand, ver));
        }
    }
    best.map(|(p, _)| p)
}

/// 从 `node-v22.11.0` 之类的目录名解析版本号。
fn parse_node_version(name: &str) -> Option<(u32, u32, u32)> {
    let rest = name.strip_prefix("node-v")?;
    let mut it = rest.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    Some((a, b, c))
}

#[derive(Debug, thiserror::Error)]
pub enum DshError {
    #[error("未找到 node.exe。请安装 Node.js，或在设置中手动指定 node 路径。")]
    NodeNotFound,
    #[error("配置里的 node 路径不可用：{0}。请在设置页更正或清空该项（清空后自动探测）。")]
    InvalidNodePath(String),
    #[error("未找到 @deepseek-ai/dsh 的 bin.js。请确认已执行 npm install -g @deepseek-ai/dsh。")]
    BinJsNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_dir_version() {
        assert_eq!(parse_node_version("node-v22.11.0"), Some((22, 11, 0)));
        assert_eq!(parse_node_version("node-v18.0.0"), Some((18, 0, 0)));
        assert_eq!(parse_node_version("nodejs"), None);
        assert_eq!(parse_node_version("other-v1.2.3"), None);
    }

    /// 唯一临时目录：`<temp>\<tag>-<pid>-<seq>`。
    ///
    /// **为什么必须唯一**：此前用固定名 `<temp>\dsh_test_nodejs`，而 `cargo test`
    /// 会在同一台机器上并发/连续运行多个测试进程（本仓库还有 `--all-features`
    /// 的多 crate 轮次）。固定名会被另一个进程 `remove_dir_all` 掉，
    /// 表现为 `WriteAllBytes` 报 `NotFound`（实测：基线 `cargo test` 因此**整轮失败**，
    /// 而单独重跑 6 次全部通过——典型的环境相关 flaky）。
    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("{tag}_{}_{n}", std::process::id()))
    }

    #[test]
    fn picks_highest_version() {
        let dir = unique_temp_dir("dsh_test_nodejs");
        let _ = std::fs::remove_dir_all(&dir);
        for v in ["node-v18.0.0", "node-v22.11.0", "node-v20.5.1"] {
            let d = dir.join(v);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("node.exe"), b"").unwrap();
        }
        let best = highest_node_version_dir(&dir).unwrap();
        assert!(
            best.to_string_lossy().contains("node-v22.11.0"),
            "got {best:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignores_dirs_without_node_exe() {
        // 版本号最高的目录里没有 node.exe 时，应退而选下一个可用版本，
        // 而不是返回一个不存在的可执行文件路径。
        let dir = unique_temp_dir("dsh_test_nodejs_skip");
        let _ = std::fs::remove_dir_all(&dir);
        for v in ["node-v22.11.0", "node-v20.5.1"] {
            std::fs::create_dir_all(dir.join(v)).unwrap();
        }
        std::fs::write(dir.join("node-v20.5.1").join("node.exe"), b"").unwrap();
        let best = highest_node_version_dir(&dir).unwrap();
        assert!(
            best.to_string_lossy().contains("node-v20.5.1"),
            "got {best:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_node_override() {
        // 安全校验：文件名必须是 node.exe
        let bad = "C:\\Windows\\System32\\cmd.exe";
        let p = Path::new(bad);
        let ok = p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("node.exe"))
            .unwrap_or(false);
        assert!(!ok);
    }

    #[test]
    fn invalid_node_override_is_an_error_not_a_silent_fallback() {
        // 关键语义：指定了 node_path 但路径无效时**必须报错**，
        // 不能静默改用 PATH 上的 node（否则用户以为自定路径生效了）。
        let missing = r"C:\definitely\not\here\node.exe";
        match Dsh::resolve_node(Some(missing)) {
            Err(DshError::InvalidNodePath(m)) => assert!(m.contains("不存在"), "{m}"),
            other => panic!("应报 InvalidNodePath，实际 {other:?}"),
        }
        // 文件名不对同样是硬错误
        match Dsh::resolve_node(Some(r"C:\Windows\System32\cmd.exe")) {
            Err(DshError::InvalidNodePath(m)) => assert!(m.contains("node.exe"), "{m}"),
            other => panic!("应报 InvalidNodePath，实际 {other:?}"),
        }
        // 空白字符串等于"未指定"：走自动探测，绝不能被当成"非法路径"
        if let Err(e) = Dsh::resolve_node(Some("   ")) {
            assert!(
                !matches!(e, DshError::InvalidNodePath(_)),
                "空白 node_path 应视为未指定，实际 {e}"
            );
        }
    }

    #[test]
    fn detects_lines_that_look_like_ready_lines() {
        // 格式不符留痕：这些行本应能解析出地址，若解析失败说明上游格式变了
        assert!(looks_like_ready_line("dsh web: http://127.0.0.1:1/"));
        assert!(looks_like_ready_line("listening on 127.0.0.1:3080"));
        assert!(looks_like_ready_line("url with token=abc"));
        assert!(!looks_like_ready_line("some random log line"));
        // 与解析器的配合：能被解析的行当然也"像"就绪行
        let ok = "dsh web: http://127.0.0.1:3080/?token=x";
        assert!(parse_ready_url(ok).is_some());
        assert!(looks_like_ready_line(ok));
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_url("http://127.0.0.1:3080/?token=x"));
        assert!(is_loopback_url("http://localhost:3080/?token=x"));
        assert!(!is_loopback_url("http://192.168.1.5:3080/?token=x"));
        assert!(!is_loopback_url("http://evil.example:3080/?token=x"));
    }

    #[test]
    fn parses_ready_url_from_dsh_stdout() {
        // 实测 dsh 0.1.5-rc.1 的启动行（printUrl 默认 true）：
        //   dsh web: http://127.0.0.1:3080/?token=xxxx
        // 同机有局域网地址时还会追加 " (LAN: http://…?token=…)"
        let plain = "dsh web: http://127.0.0.1:3080/?token=abc123";
        assert_eq!(
            parse_ready_url(plain).as_deref(),
            Some("http://127.0.0.1:3080/?token=abc123")
        );

        let with_lan =
            "dsh web: http://127.0.0.1:3080/?token=abc123 (LAN: http://192.168.1.5:3080/?token=abc123)";
        assert_eq!(
            parse_ready_url(with_lan).as_deref(),
            Some("http://127.0.0.1:3080/?token=abc123"),
            "必须剥掉 (LAN: ...) 后缀"
        );

        // 端口可变、token 可含各类字符
        assert_eq!(
            parse_ready_url("dsh web: http://127.0.0.1:65535/?token=a-b_c.d").as_deref(),
            Some("http://127.0.0.1:65535/?token=a-b_c.d")
        );
    }

    #[test]
    fn ignores_non_ready_lines() {
        // 普通日志、stderr 混杂、空行都不能被误判为就绪地址
        for line in [
            "",
            "some random log",
            "dsh web: starting",
            "dsh web: (no url here)",
            "http://127.0.0.1:3080/?token=x", // 无前缀
            "dsh web: opening the default browser; pass --no-open to disable",
        ] {
            assert_eq!(parse_ready_url(line), None, "不应解析出地址：{line:?}");
        }
    }

    #[test]
    fn detects_browser_open_notice() {
        // 已传 --no-open 时不应出现；出现即说明上游行为变化，需要留痕
        assert!(is_browser_open_notice(
            "dsh web: opening the default browser; pass --no-open to disable"
        ));
        assert!(!is_browser_open_notice(
            "dsh web: http://127.0.0.1:3080/?token=x"
        ));
    }
}
