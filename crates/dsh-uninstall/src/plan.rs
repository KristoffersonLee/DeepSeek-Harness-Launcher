//! 卸载计划：安装目录、载荷清单、标记，以及"哪些文件属于我们"。
//!
//! 这里的常量与判定都是**纯数据/纯函数**，因此可以在不碰文件系统的前提下单测
//! （旧脚本的同类逻辑只能在真实卸载时才能验证，这是脚本形态的一个实质劣势）。

use std::path::{Path, PathBuf};

use crate::guards;

/// 安装标记文件名。**只有它存在时才会执行删除** —— 这是区分"安装副本"与
/// "源码树/便携目录"的唯一可靠依据（v5.0.0 实测：只看"有 DSHLauncher.exe"会把源码树删掉）。
pub const MARKER_NAME: &str = ".dsllauncher-install";

/// 事务化安装的暂存后缀（安装器先把每个载荷写成 `<name>.tmp` 再落位）。
pub const TMP_SUFFIX: &str = ".tmp";

/// 卸载器自身的文件名（阶段一跳过它，交给阶段二）。
pub const SELF_NAME: &str = "dsh-uninstall.exe";

/// **上一版（v5.0.0 之前）遗留的脚本卸载器**：升级安装后可能仍留在安装目录里。
///
/// 它们必须被当作"我们的文件"处理，理由有两条：
/// 1. 不删 ⇒ 卸载后残留两个脚本（而它们指向的 `uninstall.ps1` 逻辑清单里**没有**
///    `dsh-uninstall.exe`，用户若误点旧脚本会留下卸载器）；
/// 2. 不认 ⇒ 会被 [`crate::guards::foreign_entries`] 当成"第三方文件" ⇒ 安装目录被保留
///    ⇒ 违背"卸载零残留"。
///
/// 与 `DSHLauncherSetup.cs` 的 `ObsoletePayloads` 必须逐项一致（门禁会比对）。
pub const LEGACY_PAYLOADS: &[&str] = &["uninstall.cmd", "uninstall.ps1"];

/// 本安装包释放的文件（**不含**标记文件）。
///
/// ⚠️ 必须与 `DSHLauncherSetup.cs` 的 `Payloads` 数组完全一致 ——
/// `tools/check-consistency.ps1` 会逐项比对两处清单，防止"安装器写什么、卸载器清什么"漂移。
pub const PAYLOADS: &[&str] = &[
    "DSHLauncher.exe",
    "dsh-uninstall.exe",
    "app.ico",
    "README.md",
    "MAINTENANCE.zh.md",
    "MAINTENANCE.en.md",
];

/// 卸载上下文（阶段一/阶段二共用）。
#[derive(Debug, Clone)]
pub struct Context {
    /// 安装目录（本程序所在目录，或阶段二里经二次校验的目标）。
    pub install_dir: PathBuf,
    /// 标记文件路径。
    pub marker: PathBuf,
    /// 是否连 `%APPDATA%\DSHLauncher` 一起删。
    pub purge: bool,
    /// 只打印问题。
    pub silent: bool,
    /// 只报告不改动。
    pub dry_run: bool,
    /// 本程序自身的路径。
    pub self_exe: PathBuf,
}

impl Context {
    /// 用安装目录与命令行参数构造上下文，并立即执行**准入校验**。
    pub fn new(install_dir: PathBuf, args: &crate::cli::Args) -> Result<Self, String> {
        let self_exe = std::env::current_exe().map_err(|e| format!("无法确定本程序路径：{e}"))?;
        let own = normalize(&install_dir);
        let own_dir = self_exe
            .parent()
            .map(normalize)
            .unwrap_or_else(|| own.clone());

        // 阶段二（从 %TEMP% 重入）删除的是**参数给的目标目录**；阶段一删除的是"本程序所在目录"。
        //
        // ⚠️ 这里曾经写成"参数必须等于本程序所在目录"——那在阶段二里永远不成立
        // （副本住在 %TEMP%，目标是安装目录），实测直接导致"卸载后安装目录仍在"。
        // 真正需要拦的是"目标 = 本程序所在目录"：那等于让副本递归删除 %TEMP% 自己所在的目录。
        let target = if args.deferred_pass {
            let want = args
                .deferred_dir
                .as_ref()
                .ok_or_else(|| "第二阶段缺少目标目录（--deferred-pass <dir>）".to_string())?;
            let want_full = normalize(Path::new(want));
            if guards::same_path(&want_full, &own_dir) {
                return Err(format!(
                    "第二阶段目标目录与本程序所在目录相同，拒绝执行：{want_full:?}"
                ));
            }
            want_full
        } else {
            own
        };
        let marker = target.join(MARKER_NAME);
        let install_dir = target;

        let ctx = Self {
            install_dir,
            marker,
            purge: args.purge,
            silent: args.silent,
            dry_run: args.dry_run,
            self_exe,
        };
        ctx.check_admission()?;
        Ok(ctx)
    }

    /// 准入校验：**任何删除动作之前**必须通过。
    pub fn check_admission(&self) -> Result<(), String> {
        if !self.install_dir.is_absolute() {
            return Err(format!(
                "安装目录不是绝对路径，拒绝执行：{:?}",
                self.install_dir
            ));
        }
        if guards::is_source_tree(&self.install_dir) {
            return Err(format!(
                "这是源码树/开发目录（存在 Cargo.toml 或 crates\\），不是安装副本 —— 已拒绝执行。\n\
                 目录：{}\n\
                 要卸载真正安装的副本，请运行**安装目录里**的 {}（例如 %LOCALAPPDATA%\\Programs\\{}）。",
                self.install_dir.display(),
                SELF_NAME,
                dsh_core::APP_NAME
            ));
        }
        if !self.marker.is_file() {
            return Err(format!(
                "未找到安装标记 {} —— 为安全起见拒绝执行任何删除。\n目录：{}",
                self.marker.display(),
                self.install_dir.display()
            ));
        }
        // 标记**内容**也必须是我们写的：空文件/伪造文件不足以证明目录归属。
        //
        // P0 修复：这里此前只接受 `dsh_core::APP_NAME`（= `DSHLauncher`），而安装器
        // `BuildMarkerText()` 写的是**产品名** `DeepSeek Harness Launcher` —— 两者互不包含，
        // 于是**每一台真实安装都会被自己的卸载器拒绝**（退出码 2，什么都不删）：
        // 注册表卸载项、桌面快捷方式、载荷文件与 `%LOCALAPPDATA%` 运行期数据全部残留，
        // 用户只能手工清理。现在接受任一等价标识，并由门禁交叉校验安装器写入的产品名。
        let content = std::fs::read_to_string(&self.marker).unwrap_or_default();
        if !marker_is_ours(&content) {
            return Err(format!(
                "安装标记 {} 的内容不属于 {}（内容：{:?}）—— 拒绝执行任何删除。",
                self.marker.display(),
                dsh_core::PRODUCT_NAME,
                content.chars().take(40).collect::<String>()
            ));
        }
        Ok(())
    }

    /// 目录本身是否允许**递归**删除（标记存在 + 不是受保护路径）。
    pub fn may_delete_dir_itself(&self) -> bool {
        self.marker.is_file() && !guards::is_protected_root(&self.install_dir)
    }

    /// 当前是否运行在安装目录**之外**（= 阶段二：被复制到 `%TEMP%` 后重入）。
    ///
    /// 阶段一里安装目录中的 `dsh-uninstall.exe` 正在运行，Windows 不允许删除运行中的镜像，
    /// 因此它必须留到阶段二；阶段二运行的是 `%TEMP%` 里的副本，可以安全删除它。
    pub fn is_deferred_pass(&self) -> bool {
        !guards::same_path(&self.self_exe, &self.install_dir.join(SELF_NAME))
    }

    /// 计划删除的文件清单（含 `.tmp` 暂存残留）。
    ///
    /// 阶段一里会跳过**自身的文件名**（运行中的镜像）；标记文件不在此列，由专门步骤处理。
    pub fn files_to_remove(&self) -> Vec<PathBuf> {
        let skip_self = !self.is_deferred_pass();
        let mut out = Vec::new();
        // 本版载荷 + 上一版遗留的脚本卸载器（升级安装后可能还在）
        for name in PAYLOADS.iter().chain(LEGACY_PAYLOADS.iter()) {
            if skip_self && *name == SELF_NAME {
                continue;
            }
            out.push(self.install_dir.join(name));
            out.push(self.install_dir.join(format!("{name}{TMP_SUFFIX}")));
        }
        out
    }

    /// 面向用户的目录名（日志/报告用）。
    pub fn display(&self) -> String {
        self.install_dir.display().to_string()
    }
}

/// 安装标记的**内容**是否证明这个目录是我们的。
///
/// 接受内部标识（`DSHLauncher`）或产品名（`DeepSeek Harness Launcher`）任一出现：
/// 前者是安装器升级后的写法（新增 `product=DSHLauncher` 行），后者是 5.0.0 安装器实际写入的首行。
/// 两者都必须接受 —— 否则升级前的安装将**无法卸载**（P0）。
fn marker_is_ours(content: &str) -> bool {
    content.contains(dsh_core::APP_NAME) || content.contains(dsh_core::PRODUCT_NAME)
}

/// 规范化路径用于比较（**唯一实现在 guards::normalize**，此处仅转发）。
///
/// 卸载器里不允许存在第二份路径规范化逻辑：两份实现一旦分叉，
/// 「准入校验用的路径」与「比较用的路径」就会对不上。
pub use crate::guards::normalize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_list_is_exactly_the_documented_set() {
        // 与 DSHLauncherSetup.cs 的 Payloads 逐项一致（门禁还会做同样的比对，
        // 这里再锁一层：卸载器自己必须知道"我们到底装了什么"）。
        assert_eq!(
            PAYLOADS,
            [
                "DSHLauncher.exe",
                "dsh-uninstall.exe",
                "app.ico",
                "README.md",
                "MAINTENANCE.zh.md",
                "MAINTENANCE.en.md",
            ]
        );
    }

    fn ctx(self_exe: &str, deferred: bool) -> Context {
        let dir = PathBuf::from(r"C:\app");
        Context {
            install_dir: dir.clone(),
            marker: dir.join(MARKER_NAME),
            purge: false,
            silent: true,
            dry_run: true,
            self_exe: PathBuf::from(if deferred {
                r"C:\Users\u\AppData\Local\Temp\dsh-uninstall-abc.exe"
            } else {
                self_exe
            }),
        }
    }

    #[test]
    fn phase_one_skips_its_own_running_image() {
        let c = ctx(r"C:\app\dsh-uninstall.exe", false);
        let files = c.files_to_remove();
        assert!(!c.is_deferred_pass(), "在安装目录里运行 = 阶段一");
        assert!(
            !files.iter().any(|p| p.ends_with(SELF_NAME)),
            "阶段一不得删除正在运行的自身镜像：{files:?}"
        );
        // 其它载荷（含 .tmp 暂存残留）与上一版遗留脚本都要清
        assert_eq!(
            files.len(),
            (PAYLOADS.len() - 1 + LEGACY_PAYLOADS.len()) * 2
        );
        assert!(files.contains(&PathBuf::from(r"C:\app\DSHLauncher.exe")));
        assert!(files.contains(&PathBuf::from(r"C:\app\DSHLauncher.exe.tmp")));
        assert!(
            !files
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == MARKER_NAME)),
            "标记文件由专门步骤处理，不能混进载荷清单"
        );
    }

    #[test]
    fn phase_two_also_removes_the_installed_uninstaller() {
        let c = ctx(r"C:\app\dsh-uninstall.exe", true);
        let files = c.files_to_remove();
        assert!(c.is_deferred_pass(), "从 %TEMP% 运行 = 阶段二");
        assert_eq!(files.len(), (PAYLOADS.len() + LEGACY_PAYLOADS.len()) * 2);
        assert!(files.contains(&PathBuf::from(r"C:\app\dsh-uninstall.exe")));
        // 升级安装留下的旧脚本卸载器也必须清掉（否则"零残留"不成立）
        assert!(files.contains(&PathBuf::from(r"C:\app\uninstall.ps1")));
        assert!(files.contains(&PathBuf::from(r"C:\app\uninstall.cmd")));
    }

    /// P0 回归：**安装器真实写入的标记文本**必须被接受。
    ///
    /// 安装器写的是产品名（`DeepSeek Harness Launcher`），而内部标识是 `DSHLauncher`；
    /// 两者互不包含，旧实现只认后者 ⇒ 真实安装**永远无法卸载**（退出码 2）。
    #[test]
    fn accepts_the_marker_text_the_installer_actually_writes() {
        let real = "DeepSeek Harness Launcher\r\nversion=5.0.0 LTS\r\ninstalled=2026-09-11 04:00:00\r\ninstall_dir=C:\\Users\\u\\AppData\\Local\\Programs\\DSHLauncher\r\n";
        assert!(
            marker_is_ours(real),
            "安装器写的产品名标记必须被接受，否则卸载器会拒绝执行任何删除"
        );
        // 升级后的写法（显式内部标识）同样必须被接受
        assert!(marker_is_ours("DSHLauncher\r\nversion=5.0.0 LTS\r\n"));
        assert!(marker_is_ours("product=DSHLauncher\n"));
        // 空内容 / 无关内容必须拒绝（不能因为放宽就失去护栏）
        assert!(!marker_is_ours(""));
        assert!(!marker_is_ours("some other product"));
        assert!(!marker_is_ours("version=5.0.0 LTS\n"));
    }

    #[test]
    fn normalize_strips_trailing_separators() {
        assert_eq!(normalize(Path::new(r"C:\a\b\")), PathBuf::from(r"C:\a\b"));
        assert_eq!(normalize(Path::new(r"C:\a\b")), PathBuf::from(r"C:\a\b"));
    }
}
