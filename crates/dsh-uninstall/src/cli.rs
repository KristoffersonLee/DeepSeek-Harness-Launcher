//! 命令行解析（与旧脚本的开关**向后兼容**，并接受 Windows/MDM 风格写法）。
//!
//! 兼容矩阵（`uninstall.cmd` 时代的所有用法都必须继续可用）：
//!
//! | 语义 | 旧脚本 | 本程序 |
//! |---|---|---|
//! | 保留用户配置（默认） | — | — |
//! | 连用户配置一起删 | `-Purge` / `--purge` | 同左，另接受 `/purge` `/p` |
//! | 只打印问题 | `-Silent` / `--silent` | 同左，另接受 `/quiet` `/q` `/s` |
//! | 只报告不改动 | `-DryRun` / `--dry-run` | 同左，另接受 `/dryrun` `/whatif` |
//! | 只清"磁盘外"残留 | — | `--clean-residue`（见 [`Args::clean_residue`]） |
//!
//! 另外有一个**内部**开关 `--deferred-pass <dir>`：主流程删完自己的文件后，会把自身复制到
//! `%TEMP%` 并用它重入，由第二阶段的进程删除安装目录本身（运行中的镜像无法删除自己）。

/// 解析后的命令行参数。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// 连 `%APPDATA%\DSHLauncher`（用户配置）一起删。
    pub purge: bool,
    /// 只打印问题（失败项与最终结论），用于 `QuietUninstallString`。
    pub silent: bool,
    /// 只报告影响范围，不改动任何状态。
    pub dry_run: bool,
    /// 只清理"磁盘外"残留（注册表卸载项 + 桌面快捷方式），**不删除任何文件或目录**。
    ///
    /// 用途：安装目录已被删除（手工删除 / 磁盘清理 / 安全软件隔离）后，正规卸载入口
    /// 会全部被护栏拒绝 —— 安装目录里的 `dsh-uninstall.exe` 已随目录消失，源码树里的
    /// 副本被源码树护栏拒绝，`--deferred-pass <dir>` 被"缺少安装标记"拒绝。于是
    /// 注册表卸载项与桌面快捷方式成为**永远清不掉的悬空残留**（实测：设置 → 应用里的
    /// 卸载按钮只会报找不到可执行文件）。本开关补上这条路，且把影响面收窄到最小。
    pub clean_residue: bool,
    /// 打印用法并退出。
    pub help: bool,
    /// 内部：第二阶段重入（删除安装目录本身）。
    pub deferred_pass: bool,
    /// 内部：阶段二的目标目录（仅当 `deferred_pass` 时有效，且会二次校验）。
    pub deferred_dir: Option<String>,
}

impl Args {
    /// 从参数序列解析（不读取进程参数，便于单测）。
    pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Self {
        let argv: Vec<String> = argv.into_iter().collect();
        let mut a = Args::default();
        let mut i = 0;
        while i < argv.len() {
            let raw = argv[i].as_str();
            // 归一化：去掉前导 `-` / `--` / `/`，转小写，去掉分隔用的 `-`/`_`
            let t: String = raw
                .trim_start_matches(['-', '/'])
                .to_ascii_lowercase()
                .replace(['-', '_'], "");
            match t.as_str() {
                "purge" | "p" => a.purge = true,
                "silent" | "quiet" | "q" | "s" => a.silent = true,
                "dryrun" | "whatif" | "n" => a.dry_run = true,
                "cleanresidue" | "residue" => a.clean_residue = true,
                "help" | "h" | "?" => a.help = true,
                // `--deferred-pass <dir>`：内部开关，**必须**带目标目录。
                // 没有目录就不知道要删哪里 —— 此时按用法错误处理（打印帮助），
                // 绝不"猜一个目录"去递归删除。
                "deferredpass" | "deferred" => {
                    a.deferred_pass = true;
                    match argv.get(i + 1).filter(|v| !v.starts_with(['-', '/'])) {
                        Some(v) => {
                            a.deferred_dir = Some(v.clone());
                            i += 1;
                        }
                        None => a.help = true,
                    }
                }
                "deferreddir" => match argv.get(i + 1) {
                    Some(v) => {
                        a.deferred_dir = Some(v.clone());
                        a.deferred_pass = true;
                        i += 1;
                    }
                    None => a.help = true,
                },
                // 未知参数既不报错也不改变语义（旧脚本对未知参数是忽略的，保持兼容）
                _ => {}
            }
            i += 1;
        }
        a
    }
}

/// 打印用法与退出码契约。
pub fn print_help() {
    println!(
        "{name} 卸载器 {version}\n\
         \n\
         用法: dsh-uninstall.exe [开关]\n\
         \n\
         开关:\n\
         \x20 (无)                        卸载（默认**保留** %APPDATA%\\{name} 用户配置）\n\
         \x20 --purge, /purge             连用户配置一起删除\n\
         \x20 --silent, /quiet            只打印问题（QuietUninstallString 使用）\n\
         \x20 --dry-run, /whatif          只报告影响范围，不修改任何状态\n\
         \x20 --clean-residue            只清理“磁盘外”残留（注册表卸载项 + 桌面快捷方式）：\n\
         \x20                            用于安装目录已被删除、正规卸载入口失效的情况。\n\
         \x20                            可与 --dry-run 组合；**不删除任何文件或目录**。\n\
         \x20 --help, /?                  打印本帮助\n\
         \n\
         退出码: 0 = 成功（含“本来就干净”） / 1 = 至少一步失败 / 2 = 被防护规则拒绝\n\
         \x20       （缺安装标记 / 受保护路径 / 源码树 / 残留清理准入不通过）\n\
         \n\
         说明: 删除安装目录本身需要先退出本程序（Windows 不允许运行中的镜像自删），
         因此主流程会把自身复制到 %TEMP% 并由副本完成删除；该副本由系统在**下次重启时**清理。
         每次卸载还会顺手清掉 %TEMP% 里历史遗留的副本。

         防护: 只有安装目录里存在安装标记 {marker} 时才会执行删除；\n\
         受保护路径（盘符根 / 系统目录 / 用户目录）拒绝递归删除；\n\
         目录里若有不属于本安装包的文件，只删自己的文件并保留目录。\n",
        name = dsh_core::APP_NAME,
        version = env!("CARGO_PKG_VERSION"),
        marker = crate::plan::MARKER_NAME,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(list: &[&str]) -> Args {
        Args::parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_keep_user_config_and_are_not_silent() {
        let a = parse(&[]);
        assert!(!a.purge, "默认必须保留用户配置");
        assert!(!a.silent && !a.dry_run && !a.help && !a.deferred_pass);
        assert!(!a.clean_residue, "默认必须走正规卸载，而不是残留清理");
        assert_eq!(a.deferred_dir, None);
    }

    #[test]
    fn clean_residue_accepts_unix_and_windows_style_switches() {
        for v in [
            "--clean-residue",
            "--cleanresidue",
            "-CleanResidue",
            "/cleanresidue",
            "--residue",
        ] {
            assert!(parse(&[v]).clean_residue, "{v} 应开启残留清理模式");
        }
        // 残留清理只是**开关**，不得因此顺带打开 purge / dry-run 等语义
        let a = parse(&["--clean-residue"]);
        assert!(!a.purge && !a.dry_run && !a.silent);
        // 与 dry-run 可组合（只报告，不改动）
        assert!(parse(&["--clean-residue", "--dry-run"]).dry_run);
    }

    #[test]
    fn accepts_legacy_and_windows_style_switches() {
        // 旧脚本用法（必须继续可用）
        for v in ["--purge", "-purge", "-Purge"] {
            assert!(parse(&[v]).purge, "{v} 应开启 purge");
        }
        for v in ["--silent", "-silent", "-Silent"] {
            assert!(parse(&[v]).silent, "{v} 应开启 silent");
        }
        for v in ["--dry-run", "--dryrun", "-DryRun"] {
            assert!(parse(&[v]).dry_run, "{v} 应开启 dry-run");
        }
        // Windows / MDM 风格
        assert!(parse(&["/purge"]).purge);
        assert!(parse(&["/quiet"]).silent);
        assert!(parse(&["/q"]).silent);
        assert!(parse(&["/dryrun"]).dry_run);
        assert!(parse(&["/whatif"]).dry_run);
        assert!(parse(&["/?"]).help);
    }

    #[test]
    fn combinations_are_independent() {
        let a = parse(&["--purge", "--silent", "--dry-run"]);
        assert!(a.purge && a.silent && a.dry_run);
        // dry-run 与 purge 组合：仍然"只报告"，绝不能因为 purge 就动手
        assert!(a.dry_run);
    }

    #[test]
    fn deferred_pass_takes_a_directory_and_reports_usage_when_missing() {
        let a = parse(&["--deferred-pass", r"C:\x\DSHLauncher"]);
        assert!(a.deferred_pass);
        assert_eq!(a.deferred_dir.as_deref(), Some(r"C:\x\DSHLauncher"));
        // 缺值 → 视为用法错误（打印帮助），绝不猜一个目录去删
        assert!(parse(&["--deferred-pass"]).help);
    }

    #[test]
    fn unknown_switches_are_ignored_like_the_old_script() {
        let a = parse(&["--nope", "junk", "/x"]);
        assert!(!a.purge && !a.silent && !a.dry_run && !a.help);
    }
}
