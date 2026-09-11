//! 应用图标加载（托盘用）。
//!
//! 优先级：
//! 1. 内嵌在 exe 里的 `app.ico` 字节（`include_bytes!`）——单文件分发时唯一可靠来源，
//!    也是唯一能精确控制像素的来源；
//! 2. exe 内嵌的 Win32 图标资源（`Icon::from_resource`）；
//! 3. 同目录 `app.ico`（开发期）；
//! 4. 1×1 占位像素（保证托盘仍能创建，不因缺图标而崩溃）。

/// 内嵌的图标字节（构建时固化的 `app.ico`）。
const EMBEDDED_ICO: &[u8] = include_bytes!("../../../app.ico");

/// 加载托盘图标。
pub fn load_app_icon() -> Option<tray_icon::Icon> {
    // 1) 内嵌字节：解析 32×32 条目（托盘图标推荐尺寸）
    if let Some(img) = dsh_core::icon::parse_ico(EMBEDDED_ICO, 32) {
        if let Ok(icon) = tray_icon::Icon::from_rgba(img.rgba, img.width, img.height) {
            return Some(icon);
        }
    }

    // 2) Win32 资源（build.rs 用 rc.exe 写入的 ID 1）
    if let Ok(icon) = tray_icon::Icon::from_resource(1, None) {
        return Some(icon);
    }

    // 3) 同目录 app.ico（开发期 / 便携版）
    for candidate in icon_candidates() {
        if candidate.is_file() {
            if let Ok(icon) = tray_icon::Icon::from_path(&candidate, None) {
                return Some(icon);
            }
        }
    }

    // 4) 占位
    tray_icon::Icon::from_rgba(vec![0x2F, 0x6F, 0xED, 0xFF], 1, 1).ok()
}

/// 候选图标路径：exe 同目录 → 仓库根（开发期）。
fn icon_candidates() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("app.ico"));
            // 开发期：target/<profile> → 仓库根
            if let Some(root) = dir.parent().and_then(|p| p.parent()) {
                v.push(root.join("app.ico"));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        v.push(cwd.join("app.ico"));
    }
    v
}
