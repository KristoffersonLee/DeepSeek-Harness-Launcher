//! 构建脚本：把 `app.ico` 与应用版本信息编译为 Win32 资源，产出真正的单文件 exe。
//!
//! 实现细节（`.rc` 生成、`rc.exe` 定位、SDK 架构筛选、版本号校验、失败即断构建）
//! 全部在 [`dsh_buildinfo`] —— 启动器与卸载器共用**唯一一份**实现，避免第二处事实来源。
//! 本文件只负责本产物的元数据与图标位置。

use std::path::PathBuf;

/// 本产物的资源块元数据。安装包（`DSHLauncherSetup.cs`）、卸载器与启动器必须完全一致。
const FILE_DESCRIPTION: &str = "DeepSeek Harness Launcher";
/// 安装包释放的就是这个文件名（`verify-version.ps1` 会校验它）。
const ORIGINAL_FILENAME: &str = "DSHLauncher.exe";

fn main() {
    let manifest_dir = PathBuf::from(env_or_die("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env_or_die("OUT_DIR"));

    // 图标位于仓库根：crates/dsh-app -> crates -> 根
    let icon_path = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join("app.ico"))
        .unwrap_or_else(|| manifest_dir.join("app.ico"));

    dsh_buildinfo::build(&dsh_buildinfo::Spec {
        manifest_dir,
        out_dir,
        icon_path,
        version: env!("CARGO_PKG_VERSION"),
        product_name: dsh_buildinfo::meta::PRODUCT_NAME,
        file_description: FILE_DESCRIPTION,
        company_name: dsh_buildinfo::meta::COMPANY_NAME,
        copyright: dsh_buildinfo::meta::COPYRIGHT,
        original_filename: ORIGINAL_FILENAME,
        base_name: "dsh_app",
        generated_label: "launcher",
        permit_missing: std::env::var_os("DSH_ALLOW_MISSING_VERSION_RESOURCE").is_some(),
    });
}

/// 读取必需的环境变量；缺失即构建失败（Cargo 保证这些变量一定存在）。
fn env_or_die(key: &str) -> String {
    match std::env::var(key) {
        Ok(v) => v,
        Err(_) => panic!("构建环境缺少 {key}（请通过 cargo 构建，不要直接编译 build.rs）"),
    }
}
