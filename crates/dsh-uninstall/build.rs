//! 构建脚本：给卸载器 exe 注入版本资源与图标（与启动器同一份实现，见 `dsh_buildinfo`）。
//!
//! 为什么卸载器也要版本资源：它是**随安装包发布、并留在用户机器上的可执行文件**（还是
//! "应用和功能"里的卸载入口）。没有 FileVersion/ProductName 的话，属性页空白、AV/审计
//! 也拿不到可核对的元数据 —— 这正是 v5.0.0 之前启动器踩过的坑。

use std::path::PathBuf;

/// 与启动器/安装包一致的元数据（`verify-version.ps1` 会交叉校验）。
const FILE_DESCRIPTION: &str = "DeepSeek Harness Launcher Uninstaller";
const ORIGINAL_FILENAME: &str = "dsh-uninstall.exe";

fn main() {
    let manifest_dir = PathBuf::from(env_or_die("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env_or_die("OUT_DIR"));

    // crates/dsh-uninstall -> crates -> 仓库根
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
        base_name: "dsh_uninstall",
        generated_label: "uninstaller",
        permit_missing: std::env::var_os("DSH_ALLOW_MISSING_VERSION_RESOURCE").is_some(),
    });
}

fn env_or_die(key: &str) -> String {
    match std::env::var(key) {
        Ok(v) => v,
        Err(_) => panic!("构建环境缺少 {key}（请通过 cargo 构建，不要直接编译 build.rs）"),
    }
}
