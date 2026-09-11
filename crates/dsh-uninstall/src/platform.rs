//! Windows 平台细节：自身目录/路径、注册表卸载项、桌面快捷方式位置、进程结束、自删除调度。
//!
//! 全部走 `windows` crate + `std`，不引入第三方依赖。

use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

use dsh_core::APP_NAME;

/// 卸载注册表项（与安装器 `Program.UninstallKey` 一致，位于 HKCU）。
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher";

/// 本程序所在目录（等价于旧脚本的 `$PSScriptRoot`）。
pub fn own_dir() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| std::io::Error::other("可执行文件没有父目录"))
}

/// `%APPDATA%\DSHLauncher`（用户配置，默认保留）。
pub fn roaming_dir() -> PathBuf {
    dirs_join("APPDATA", APP_NAME)
}

/// `%LOCALAPPDATA%\DSHLauncher`（日志 / WebView2 profile / service.json，总是清理）。
///
/// 与 `dsh_core::local_data_dir()` 同源：运行期数据属于 LOCALAPPDATA，用户配置属于 APPDATA。
pub fn local_dir() -> PathBuf {
    dsh_core::local_data_dir()
}

fn dirs_join(env_key: &str, leaf: &str) -> PathBuf {
    std::env::var_os(env_key)
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(leaf)
}

// ---------------------------------------------------------------------------
// 桌面快捷方式：必须跟随 OneDrive 重定向
//
// 旧脚本用 `[Environment]::GetFolderPath('Desktop')`，它解析的是 shell 已知文件夹
// （OneDrive 重定向后的真实桌面）。Rust 侧不引入 Shell/COM 依赖，改为读同一个来源：
// `HKCU\...\Explorer\User Shell Folders\Desktop`（REG_EXPAND_SZ，可能含 %USERPROFILE%），
// 再退化为 `%USERPROFILE%\Desktop`。公共桌面同理读 HKLM。
// ---------------------------------------------------------------------------
pub fn desktop_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = user_shell_folder(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
        "Desktop",
    ) {
        out.push(p);
    } else if let Some(profile) = std::env::var_os("USERPROFILE") {
        out.push(PathBuf::from(profile).join("Desktop"));
    }
    if let Some(p) = user_shell_folder(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
        "Common Desktop",
    ) {
        out.push(p);
    } else if let Some(public) = std::env::var_os("PUBLIC") {
        out.push(PathBuf::from(public).join("Desktop"));
    }
    out.retain(|p| p.is_dir());
    out.sort();
    out.dedup();
    out
}

/// 读一个 `REG_EXPAND_SZ` / `REG_SZ` 值并展开环境变量；读不到返回 `None`。
fn user_shell_folder(root: HKEY, subkey: &str, value: &str) -> Option<PathBuf> {
    let sub: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let raw = String::from_utf16_lossy(&buf[..end]);
    // REG_EXPAND_SZ 里的 %USERPROFILE% 必须展开（RegGetValueW 不会替我们展开）
    let expanded = expand_env(&raw);
    if expanded.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(expanded))
    }
}

/// 展开 `%VAR%`（只用 std 的环境变量，避免引入依赖）。
fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(rel_end) = s[i + 1..].find('%') {
                let name = &s[i + 1..i + 1 + rel_end];
                if !name.is_empty() && !name.contains('\\') {
                    if let Some(v) = std::env::var_os(name) {
                        out.push_str(&v.to_string_lossy());
                        i += rel_end + 2;
                        continue;
                    }
                }
            }
        }
        // 按 UTF-8 边界推进一个字符
        let ch = s[i..].chars().next().unwrap_or('%');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 桌面上的启动器快捷方式（新旧两种命名），**只列出、不删除**。
///
/// 与 [`remove_desktop_shortcuts`] 共用同一条命名规则：dry-run 报告与真实删除若各写一份
/// 匹配逻辑，就会出现"dry-run 说有 2 个、真删时删了 1 个"这类静默漂移。
pub fn desktop_shortcuts() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in desktop_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            if shortcut_name_matches(&entry.file_name().to_string_lossy()) {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

/// 文件名是否是我们的快捷方式（`DSH Harness*.lnk` / `DeepSeek Harness Launcher*.lnk`）。
fn shortcut_name_matches(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.starts_with("dsh harness") || lower.starts_with("deepseek harness launcher"))
        && lower.ends_with(".lnk")
}

/// 删除桌面上的启动器快捷方式（新旧两种命名），返回被删掉的路径。
pub fn remove_desktop_shortcuts() -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for path in desktop_shortcuts() {
        if std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

/// 读取卸载注册表项里的一个字符串值（`REG_SZ` / `REG_EXPAND_SZ`，命令式环境变量会展开）。
///
/// 返回 `None` = 键或值不存在 / 类型不符。**只读**：不创建、不修改任何键。
/// 残留清理模式靠它读 `InstallLocation` / `UninstallString` / `DisplayIcon` 来确认
/// "这个键确实是我们写的、且安装目录在哪"。
pub fn read_uninstall_value(name: &str) -> Option<String> {
    let sub: Vec<u16> = UNINSTALL_KEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let value: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let raw = String::from_utf16_lossy(&buf[..end]);
    let expanded = expand_env(&raw);
    if expanded.trim().is_empty() {
        None
    } else {
        Some(expanded)
    }
}

/// 卸载注册表项是否仍然存在。
///
/// **必须用 `RegOpenKeyExW` 查"键"，不能用 `RegGetValueW` 查"值"**：
/// 后者需要一个值名，传 `null` 只会去读该键的**默认值**，而我们的键没有默认值 ——
/// 于是"键存在"会被误判成"不存在"（实测：dry-run 报告 `注册表卸载项 → 不存在`，
/// 而实际上键就在那里；`remove_uninstall_key` 的后置校验也会因此误报失败）。
pub fn uninstall_key_exists() -> bool {
    let sub: Vec<u16> = UNINSTALL_KEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut h = HKEY::default();
        let r = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            None,
            KEY_READ,
            &mut h,
        );
        if r == ERROR_SUCCESS {
            let _ = RegCloseKey(h);
            true
        } else {
            false
        }
    }
}

/// 删除卸载注册表项（递归）。返回是否成功。
pub fn remove_uninstall_key() -> bool {
    let sub: Vec<u16> = UNINSTALL_KEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr())) };
    r == ERROR_SUCCESS || !uninstall_key_exists()
}

/// 结束**从 `dir` 启动**的启动器进程（及其进程树），返回被结束的 PID。
///
/// 只认映像路径落在 `dir` 下的实例：绝不误杀用户装在别处的启动器
/// （旧脚本同样是按路径比较，v4.2.x 的审计把这个错误修过）。
pub fn stop_launcher_in(dir: &Path) -> Vec<u32> {
    let mut killed = Vec::new();
    let me = std::process::id();
    // 单次快照 + 按 PID 查映像名。旧实现是「遍历全部 PID，每个再枚举一次全表」
    // （机器上 300 个进程 ⇒ 300 次全表枚举，是卸载里最慢的一段）。
    let Some(table) = dsh_core::ProcessTable::capture() else {
        // 拿不到进程表 ⇒ 无法确认任何进程的身份 ⇒ 什么都不杀（保守方向正确）
        return killed;
    };
    for pid in table.pids() {
        if pid == me {
            continue;
        }
        let Some(image) = table.image_name(pid) else {
            continue;
        };
        if !image.eq_ignore_ascii_case("DSHLauncher.exe") {
            continue;
        }
        // 映像路径可能因权限读取不到：此时**不杀**（宁可留一个进程，也不误杀）
        let Some(path) = dsh_core::process::process_image_path(pid) else {
            continue;
        };
        let Some(parent) = path.parent() else {
            continue;
        };
        if !super::guards::same_path(parent, dir) {
            continue;
        }
        let tree = dsh_core::process::kill_process_tree(pid);
        killed.extend(tree);
    }
    killed.sort_unstable();
    killed.dedup();
    killed
}

/// 把 `src` 复制到 `dst`（同卷或跨卷都由 std 处理），返回是否成功。
pub fn copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dst).map(|_| ())
}

/// 让"运行中的本程序"在**下次重启时**被系统删除。
///
/// ## 为什么不能"退出即删除"（实测结论，别再试了）
///
/// 阶段二运行的是 `%TEMP%` 里的副本，它自己就是"正在运行的镜像"。实测用
/// `CreateFileW(自己的路径, DELETE, …, FILE_FLAG_DELETE_ON_CLOSE)` **必然失败**：
/// 系统返回 `ERROR_ACCESS_DENIED（拒绝访问）` —— 可执行镜像被映射为 image section 后，
/// 不允许对它取 `DELETE` 权限（否则运行中的程序可以把自己从磁盘上抹掉）。
/// 允许的只有**改名**（`build.ps1` 的热替换正是靠这个），改名之后依然要有人来删。
///
/// 因此 Windows 上唯一可行的路径是"标记为下次重启时删除"：
/// 系统在重启早期（还没有任何镜像被映射时）替我们删掉它。
/// 代价是 `%TEMP%` 里会留一个约 300 KB 的副本直到重启 —— 这是**平台限制**，不是遗漏；
/// 为降低影响，[`sweep_stale_temp_copies`] 会在每次卸载时顺手清掉历史遗留的副本。
pub fn schedule_delete_on_reboot(path: &Path) {
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        );
    }
}

/// 清理 `%TEMP%` 里**历史遗留**的卸载器副本（不含当前正在运行的那一份）。
///
/// 用途：上一次卸载留下的副本要等到重启才会消失；本次卸载顺手把它们删掉，
/// 避免 `%TEMP%` 里越积越多。只匹配本程序自己的命名模式，且删除失败一律忽略
/// （文件可能仍被占用，或者根本不是我们的）。
pub fn sweep_stale_temp_copies(current: &Path) -> usize {
    let temp = std::env::temp_dir();
    let Ok(rd) = std::fs::read_dir(&temp) else {
        return 0;
    };
    let mut removed = 0;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if !lower.starts_with("dsh-uninstall-") || !lower.ends_with(".exe") {
            continue;
        }
        let path = entry.path();
        if super::guards::same_path(&path, current) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_environment_variables() {
        let profile = std::env::var("USERPROFILE").unwrap_or_default();
        if !profile.is_empty() {
            let expanded = expand_env(r"%USERPROFILE%\Desktop");
            assert_eq!(expanded, format!("{profile}\\Desktop"));
        }
        // 未知变量原样保留（不吞字符），且不丢非 ASCII
        assert_eq!(expand_env("100%% 中文"), "100%% 中文");
        assert_eq!(expand_env("%NOT_SET_VAR_XYZ%"), "%NOT_SET_VAR_XYZ%");
    }

    #[test]
    fn desktop_dirs_are_existing_absolute_paths() {
        // 结果取决于机器，但只要是"存在"的就必须是绝对路径（供删除前比较使用）
        for d in desktop_dirs() {
            assert!(d.is_absolute(), "{d:?}");
            assert!(d.is_dir(), "{d:?}");
        }
    }

    #[test]
    fn uninstall_key_string_matches_the_installer() {
        // 与 DSHLauncherSetup.cs 的 UninstallKey 一致（HKCU\Software\...\Uninstall\DSHLauncher）
        assert_eq!(
            UNINSTALL_KEY,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher"
        );
    }
}
