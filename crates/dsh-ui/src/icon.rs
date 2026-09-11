//! 应用图标加载（窗口标题栏用）。
//!
//! 直接解析内嵌的 `app.ico`：tao 创建窗口时若不显式设置图标，
//! Windows 会用窗口类默认图标，表现为标题栏左上角不显示应用 logo。

/// 从内嵌的 `app.ico` 创建 tao 窗口图标。
///
/// 取 32×32 条目：标题栏在 100% DPI 下即 32px，高 DPI 由系统缩放。
pub fn app_window_icon() -> Option<tao::window::Icon> {
    let img = dsh_core::icon::parse_ico(include_bytes!("../../../app.ico"), 32)?;
    tao::window::Icon::from_rgba(img.rgba, img.width, img.height).ok()
}
