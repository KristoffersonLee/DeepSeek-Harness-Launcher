//! 浏览器打开 / Edge 精简窗口回退。
//!
//! 对应路线图回归清单 #10：**WebView2 运行时缺失时回退 Edge 精简窗口**。
//! 这是可用性底线——丢掉会导致部分机器彻底打不开 Harness，因此不属于冗余。
//!
//! 所有对外启动都用 `ShellExecuteW` 或**带独立参数**的 `Command`：
//! 旧实现用 `cmd /c start "" <url>` 打开默认浏览器，URL 会被 cmd.exe 二次解析，
//! 且会闪一个控制台窗口；`ShellExecuteW` 由 Shell 直接处理协议关联，
//! 不存在命令行解析、也不需要中间进程。

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use dsh_core::RollingLogger;

/// `CREATE_NO_WINDOW`：避免 GUI 子系统进程拉出控制台黑框。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 用系统默认浏览器打开 URL（ShellExecuteW，无 shell 解析）。
pub fn open_default_browser(url: &str) {
    let _ = shell_execute(url);
}

/// 打开日志 / 数据目录（资源管理器）。
pub fn open_directory(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    // 目录走 ShellExecuteW 而不是 `explorer.exe <path>`：
    // 后者在路径含 `&` 等字符时同样依赖命令行解析，且会多一个常驻进程。
    let _ = shell_execute(&dir.to_string_lossy());
}

/// 调用 `ShellExecuteW(NULL, "open", target, NULL, NULL, SW_SHOWNORMAL)`。
fn shell_execute(target: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = HSTRING::from("open");
    let file = HSTRING::from(target);
    unsafe {
        // 返回值 > 32 才表示成功（见 ShellExecuteW 文档）
        let rc = ShellExecuteW(None, &verb, &file, None, None, SW_SHOWNORMAL);
        if rc.0 as usize > 32 {
            Ok(())
        } else {
            Err(format!("ShellExecuteW 返回 {}", rc.0 as usize))
        }
    }
}

/// Edge 精简窗口回退。
///
/// 与 v4 `OpenEdgeFallback` 一致：`--app` 模式 + 独立用户目录 + 省内存参数。
/// 返回 `true` 表示已用 Edge 打开；`false` 表示已回退到默认浏览器。
pub fn open_edge_fallback(url: &str, logger: &RollingLogger) -> bool {
    let Some(edge) = find_edge_path() else {
        logger.warn("未找到 Edge，改用默认浏览器打开。");
        open_default_browser(url);
        return false;
    };

    let profile = dsh_core::edge_profile_dir();
    let _ = std::fs::create_dir_all(&profile);

    let mut cmd = Command::new(&edge);
    cmd.arg(format!("--app={url}"))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--disable-breakpad",
            "--disable-background-mode",
            "--disable-features=msEdgeSidebarV2,msEdgeShoppingAssistant,msEdgeTranslate",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .creation_flags(CREATE_NO_WINDOW);

    match cmd.spawn() {
        Ok(_) => {
            logger.info(&format!("已在 Edge 精简窗口中打开 {url}"));
            true
        }
        Err(e) => {
            logger.warn(&format!("Edge 启动失败（{e}），改用默认浏览器。"));
            open_default_browser(url);
            false
        }
    }
}

/// 定位 msedge.exe：标准安装目录 → 注册表 App Paths → PATH。
fn find_edge_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(base) = std::env::var_os(key) {
            candidates.push(
                PathBuf::from(base)
                    .join("Microsoft")
                    .join("Edge")
                    .join("Application")
                    .join("msedge.exe"),
            );
        }
    }
    for c in &candidates {
        if c.is_file() {
            return Some(c.clone());
        }
    }

    // 注册表 App Paths
    if let Some(p) = edge_from_registry() {
        return Some(p);
    }

    // PATH
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join("msedge.exe");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// 从 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe` 读取默认值。
///
/// **直接调 Win32 注册表 API**，不再 `reg.exe query` + 解析文本：
/// 旧的实现每次都要额外派生一个控制台子进程（`CREATE_NO_WINDOW` 只挡住窗口，
/// 挡不住进程创建与命令行解析），既慢又依赖 `reg.exe` 的输出格式；
/// 而我们本来就启用了 `Win32_System_Registry` feature（`dsh-ui` 用它读系统主题）。
fn edge_from_registry() -> Option<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };

    // 32 位安装的 Edge 在 WOW6432Node 下；两个位置都试，顺序与旧实现一致（先原生视图）
    const SUBKEYS: [&str; 2] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
    ];
    for subkey in SUBKEYS {
        let sub: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = [0u16; 512];
        let mut size = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(sub.as_ptr()),
                PCWSTR::null(), // 默认值（旧实现读的也是 /ve）
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        if status != windows::Win32::Foundation::ERROR_SUCCESS {
            continue;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        if end == 0 {
            continue;
        }
        let value = String::from_utf16_lossy(&buf[..end]);
        let p = PathBuf::from(value.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    None
}
