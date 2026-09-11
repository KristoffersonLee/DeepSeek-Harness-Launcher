//! 安全护栏（纯函数，全部有单测）。
//!
//! 这些规则逐条对应旧 PowerShell 卸载器里被实测过的坑，重写时一条都不能丢：
//!
//! 1. **源码树识别**：仓库根同时存在 `DSHLauncher.exe` / `app.ico` / `README.md` / `uninstall.*`，
//!    仅凭"有 DSHLauncher.exe"会把源码树当成安装副本 → v5.0.0 实测误删过一次；
//! 2. **受保护路径**：盘符根（`D:` 这种"裸盘符"尤其危险，`GetFullPath('D:')` 会解析成
//!    "D: 上的当前目录"）、Windows / Program Files / ProgramData / 用户目录 / TEMP 一律拒绝；
//! 3. **外来文件保护**：目录里若有不属于本安装包的东西，只删自己的文件，保留目录本身。

use std::path::{Path, PathBuf};

/// 路径规范化（绝对化 + 去尾分隔符）—— **卸载器内的唯一实现**。
///
/// `plan::normalize` 直接转发到这里。此前两处各写了一份逐字相同的实现，任何一处
/// 改动都会让「准入校验用的路径」与「比较用的路径」悄悄分叉；对一个以「拒绝删除受保护
/// 路径」为核心职责的卸载器来说，这类分叉的后果是灾难性的。
pub fn normalize(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    let s = abs
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    PathBuf::from(if s.is_empty() {
        abs.to_string_lossy().to_string()
    } else {
        s
    })
}

/// 两个路径是否指向同一位置（Windows 大小写不敏感）。
pub fn same_path(a: &Path, b: &Path) -> bool {
    normalize(a)
        .to_string_lossy()
        .eq_ignore_ascii_case(&normalize(b).to_string_lossy())
}

/// 是否是"源码树/开发目录"特征（`Cargo.toml` 或 `crates\`）。
pub fn is_source_tree(dir: &Path) -> bool {
    dir.join("Cargo.toml").exists() || dir.join("crates").is_dir()
}

/// 是否受保护路径（**拒绝**递归删除）。
///
/// 覆盖：裸盘符（`D:`）、任何盘的根、以及常见系统/用户目录本身。
pub fn is_protected_root(dir: &Path) -> bool {
    let p = normalize(dir);
    let s = p.to_string_lossy().to_string();

    // 裸盘符：'D:' 这种写法在 Windows 上表示"D: 盘的当前目录"，必须单独拦
    if s.len() == 2 && s.ends_with(':') && s.as_bytes()[0].is_ascii_alphabetic() {
        return true;
    }

    for key in [
        "WINDIR",
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
    ] {
        if let Some(v) = std::env::var_os(key) {
            let base = normalize(Path::new(&v));
            if !base.to_string_lossy().is_empty() && same_path(&base, &p) {
                return true;
            }
        }
    }
    false
}

/// 目录里是否存在**不属于本安装包**的项目（存在则保留目录本身）。
///
/// * `payloads` —— 本版载荷；
/// * `legacy` —— 上一版遗留、但仍由我们负责清理的文件（脚本卸载器）；
/// * `marker` —— 安装标记：属于我们，但由专门步骤删除，因此不计入"外来文件"。
pub fn foreign_entries(
    dir: &Path,
    payloads: &[&str],
    legacy: &[&str],
    marker: &str,
) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut foreign = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == marker {
            continue;
        }
        let ours = payloads.iter().chain(legacy.iter()).any(|p| {
            name.eq_ignore_ascii_case(p) || name.eq_ignore_ascii_case(&format!("{p}.tmp"))
        });
        if !ours {
            foreign.push(name);
        }
    }
    foreign.sort();
    foreign
}

// ---------------------------------------------------------------------------
// 残留清理（`--clean-residue`）的准入判定
//
// 背景：正规卸载的安装目录来自"本程序所在目录"。安装目录一旦被删除，注册表卸载项与
// 桌面快捷方式就会指向不存在的文件（**悬空残留**），而所有正规入口都被护栏拒绝：
// 安装目录里的卸载器随目录消失、源码树里的副本被源码树护栏拒绝、`--deferred-pass`
// 被"缺少安装标记"拒绝。结果是用户没有任何受支持的手段清掉那两处残留。
//
// 因此新增一个**只清磁盘外残留**的模式。它会删除东西，所以准入必须比正规卸载更严：
// 不靠"目录里有什么"证明归属（目录已经没了），而靠**注册表三个值互相印证**。
// ---------------------------------------------------------------------------

/// 从 `UninstallString` / `DisplayIcon` 形式的字符串里取出可执行文件路径。
///
/// 实际会被解析到的三种形态：
/// * `"C:\...\dsh-uninstall.exe"`（带引号）；
/// * `"C:\...\dsh-uninstall.exe" --silent`（带引号 + 参数）；
/// * `C:\...\app.ico,0`（图标路径 + 资源序号）。
pub fn parse_exe_spec(spec: &str) -> Option<PathBuf> {
    let s = spec.trim();
    if s.is_empty() {
        return None;
    }
    // 引号形态：取到下一个引号为止（其后的都是命令行参数）；无引号：取到第一个空白为止
    let head = match s.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => s.split_whitespace().next().unwrap_or(""),
    };
    // 去掉图标序号后缀（`...,0`）。只认"逗号 + 纯数字"，避免误伤合法路径里的逗号。
    let head = match head.rfind(',') {
        Some(i)
            if !head[i + 1..].is_empty() && head[i + 1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            &head[..i]
        }
        _ => head,
    };
    let t = head.trim();
    if t.is_empty() {
        None
    } else {
        Some(PathBuf::from(t))
    }
}

/// 文件名是否属于本安装包载荷（大小写不敏感）。
pub fn is_our_payload_name(name: &str) -> bool {
    crate::plan::PAYLOADS
        .iter()
        .any(|p| name.eq_ignore_ascii_case(p))
}

/// 由注册表卸载项的三个值推定残留清理的目标安装目录（纯函数）。
///
/// 三者必须**互相印证**，否则拒绝：`InstallLocation` 给出目录，`UninstallString`
/// 或 `DisplayIcon` 给出的可执行文件必须位于该目录下、且文件名属于本安装包载荷。
/// 这样即使注册表里存在同名/伪造的键（别的程序、或被人改过的键），也不会被我们清理。
pub fn residue_target_of(
    install_location: &str,
    uninstall_string: &str,
    display_icon: &str,
) -> Result<PathBuf, String> {
    let loc = install_location.trim();
    if loc.is_empty() {
        return Err(format!(
            "注册表卸载项缺少 InstallLocation，无法确定残留范围 —— 拒绝执行。\n\
             请手工检查 HKCU\\{}。",
            crate::platform::UNINSTALL_KEY
        ));
    }
    let dir = normalize(Path::new(loc));
    if !dir.is_absolute() {
        return Err(format!(
            "注册表里的 InstallLocation 不是绝对路径，拒绝执行：{loc}"
        ));
    }

    // UninstallString 优先，其次 DisplayIcon：任一能印证即可（不同版本写的值不完全一样）
    let matched = [uninstall_string, display_icon].iter().any(|spec| {
        let Some(exe) = parse_exe_spec(spec) else {
            return false;
        };
        let exe = normalize(&exe);
        let (Some(parent), Some(name)) = (exe.parent(), exe.file_name()) else {
            return false;
        };
        same_path(parent, &dir) && is_our_payload_name(&name.to_string_lossy())
    });
    if !matched {
        return Err(format!(
            "注册表卸载项与 InstallLocation 不一致（{} 下没有本安装包的载荷可执行文件）——\n\
             这不是本程序留下的残留，拒绝清理。\nInstallLocation={loc}",
            dir.display()
        ));
    }
    Ok(dir)
}

/// 残留清理的**目录级**准入：受保护路径 / 源码树 / 仍存在安装标记 一律拒绝。
///
/// 第三条最关键：安装目录仍然完整（标记在）时，用户应当运行安装目录里的卸载器走正规
/// 卸载流程（那条路会一并清掉载荷文件与运行期数据）。残留清理只处理"目录已经没了"
/// 这一种情形，绝不能变成绕过正规流程的捷径。
pub fn residue_dir_allowed(dir: &Path) -> Result<(), String> {
    if is_protected_root(dir) {
        return Err(format!(
            "残留清理的目标是受保护路径，拒绝执行：{}",
            dir.display()
        ));
    }
    if is_source_tree(dir) {
        return Err(format!(
            "残留清理的目标是源码树/开发目录（存在 Cargo.toml 或 crates\\），拒绝执行：{}",
            dir.display()
        ));
    }
    let marker = dir.join(crate::plan::MARKER_NAME);
    if marker.is_file() {
        return Err(format!(
            "安装标记仍然存在（{}）—— 安装目录是完整的，请运行安装目录里的 {} 走正规卸载。\n\
             残留清理只处理“安装目录已被删除、只剩注册表项/快捷方式”的情形。",
            marker.display(),
            crate::plan::SELF_NAME
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_source_tree_by_manifest_or_crates_dir() {
        let tmp = std::env::temp_dir().join(format!("dsh_guard_src_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(!is_source_tree(&tmp), "空目录不是源码树");
        std::fs::write(tmp.join("Cargo.toml"), b"[workspace]").unwrap();
        assert!(is_source_tree(&tmp), "有 Cargo.toml 即源码树");
        std::fs::remove_file(tmp.join("Cargo.toml")).unwrap();
        std::fs::create_dir_all(tmp.join("crates")).unwrap();
        assert!(is_source_tree(&tmp), "有 crates\\ 即源码树");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refuses_protected_roots() {
        // 裸盘符与盘符根（旧脚本的 'D:' vs 'D:\' 漏洞就是在这里修的）
        assert!(is_protected_root(Path::new(r"D:")));
        assert!(is_protected_root(Path::new(r"D:\")));
        assert!(is_protected_root(Path::new(r"C:\")));
        // 系统/用户目录本身
        for key in ["WINDIR", "LOCALAPPDATA", "APPDATA", "USERPROFILE", "TEMP"] {
            if let Some(v) = std::env::var_os(key) {
                let p = PathBuf::from(v);
                if !p.to_string_lossy().is_empty() {
                    assert!(
                        is_protected_root(&p),
                        "{key} 本身必须是受保护路径：{}",
                        p.display()
                    );
                }
            }
        }
        // 正常安装目录不受保护
        assert!(!is_protected_root(Path::new(
            r"C:\Users\someone\AppData\Local\Programs\DSHLauncher"
        )));
    }

    #[test]
    fn same_path_is_case_insensitive_and_separator_insensitive() {
        assert!(same_path(Path::new(r"C:\A\B"), Path::new(r"c:\a\b\")));
        assert!(!same_path(Path::new(r"C:\A\B"), Path::new(r"C:\A\BC")));
    }

    #[test]
    fn foreign_entries_ignore_our_files_and_the_marker() {
        let tmp = std::env::temp_dir().join(format!("dsh_guard_foreign_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join("user-data")).unwrap();
        for f in [
            "DSHLauncher.exe",
            "app.ico",
            "DSHLauncher.exe.tmp",
            MARKER_FOR_TEST,
        ] {
            std::fs::write(tmp.join(f), b"x").unwrap();
        }
        let foreign = foreign_entries(
            &tmp,
            super::super::plan::PAYLOADS,
            super::super::plan::LEGACY_PAYLOADS,
            MARKER_FOR_TEST,
        );
        assert_eq!(foreign, vec!["user-data".to_string()]);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    const MARKER_FOR_TEST: &str = super::super::plan::MARKER_NAME;

    // ---- 残留清理（--clean-residue）的准入 ----

    #[test]
    fn parses_exe_specs_from_registry_forms() {
        // 带引号（UninstallString 的实际写法）
        assert_eq!(
            parse_exe_spec(r#""C:\a\b\dsh-uninstall.exe""#),
            Some(PathBuf::from(r"C:\a\b\dsh-uninstall.exe"))
        );
        // 带引号 + 参数（QuietUninstallString）
        assert_eq!(
            parse_exe_spec(r#""C:\a\b\dsh-uninstall.exe" --silent"#),
            Some(PathBuf::from(r"C:\a\b\dsh-uninstall.exe"))
        );
        // 无引号 + 参数
        assert_eq!(
            parse_exe_spec(r"C:\a\b\dsh-uninstall.exe /quiet"),
            Some(PathBuf::from(r"C:\a\b\dsh-uninstall.exe"))
        );
        // 图标路径 + 资源序号（DisplayIcon 的惯用写法）
        assert_eq!(
            parse_exe_spec(r"C:\a\b\app.ico,0"),
            Some(PathBuf::from(r"C:\a\b\app.ico"))
        );
        // 空/纯空白 → None（不能返回空路径）
        assert_eq!(parse_exe_spec(""), None);
        assert_eq!(parse_exe_spec("   "), None);
        // 目录名里带逗号但后面不是纯数字：不得被当成图标序号切掉
        assert_eq!(
            parse_exe_spec(r"C:\a,b\dsh-uninstall.exe"),
            Some(PathBuf::from(r"C:\a,b\dsh-uninstall.exe"))
        );
    }

    #[test]
    fn residue_target_requires_the_registry_values_to_agree() {
        let dir = r"C:\Users\u\AppData\Local\Programs\DSHLauncher";
        let uninstall = format!(r#""{dir}\dsh-uninstall.exe""#);
        // 正常形态：三个值互相印证
        assert_eq!(
            residue_target_of(dir, &uninstall, "").unwrap(),
            PathBuf::from(dir)
        );
        // 只靠 DisplayIcon 印证也可以
        assert_eq!(
            residue_target_of(dir, "", &format!(r"{dir}\DSHLauncher.exe")).unwrap(),
            PathBuf::from(dir)
        );
        // 缺少 InstallLocation ⇒ 无法确定范围，拒绝
        assert!(residue_target_of("", &uninstall, "").is_err());
        // 相对路径 ⇒ 拒绝（规范化后不是绝对路径）
        assert!(residue_target_of(r"Programs\DSHLauncher", &uninstall, "").is_err());
        // 可执行文件不在 InstallLocation 下 ⇒ 不是我们的残留
        assert!(residue_target_of(dir, r#""C:\other\dsh-uninstall.exe""#, "").is_err());
        // 文件名不是本安装包载荷 ⇒ 拒绝（避免清理同名/伪造的键）
        assert!(residue_target_of(dir, &format!(r#""{dir}\evil.exe""#), "").is_err());
    }

    #[test]
    fn residue_dir_allowed_refuses_intact_install_and_unsafe_dirs() {
        // 受保护路径（用户目录本身）直接拒绝
        if let Some(home) = std::env::var_os("USERPROFILE") {
            assert!(residue_dir_allowed(Path::new(&home)).is_err());
        }
        // 源码树拒绝
        let src = std::env::temp_dir().join(format!("dsh_residue_src_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Cargo.toml"), b"[workspace]").unwrap();
        assert!(residue_dir_allowed(&src).is_err());
        let _ = std::fs::remove_dir_all(&src);

        // 目录已不存在（残留状态）⇒ 允许
        let gone = std::env::temp_dir().join(format!("dsh_residue_gone_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&gone);
        assert!(
            residue_dir_allowed(&gone).is_ok(),
            "目录不存在时应当允许清理残留"
        );

        // 安装标记仍在 ⇒ 拒绝（必须走正规卸载）
        std::fs::create_dir_all(&gone).unwrap();
        std::fs::write(
            gone.join(super::super::plan::MARKER_NAME),
            b"DeepSeek Harness Launcher\n",
        )
        .unwrap();
        let err = residue_dir_allowed(&gone).unwrap_err();
        assert!(err.contains("安装标记"), "{err}");
        let _ = std::fs::remove_dir_all(&gone);
    }
}
