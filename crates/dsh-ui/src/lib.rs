//! dsh-ui —— 窗口 / 托盘 / 菜单 / IPC 组件。
//!
//! 提供可复用的界面组件，由 dsh-app 组装为完整应用。

pub mod guide;
pub mod harness;
pub mod icon;
pub mod settings;
pub mod theme;
pub mod tray;

pub use guide::{GuideError, GuideWindow, GUIDE_HTML};
pub use harness::{AuthState, HarnessError, HarnessWindow, ThemeSampleResult};
pub use settings::{SettingsCommand, SettingsError, SettingsInit, SettingsWindow};
pub use tray::{build_tray_icon, tray_event_from_id, TrayEvent};
pub use wry::WebContext;
// 说明：`theme` / `icon` 仍是 `pub mod`（本 crate 内部使用），但不再在 crate 根
// 重复导出内部函数——此前 `app_window_icon` / `is_dark_color` / `is_system_dark` /
// `set_dark_mode` / `set_titlebar_colors` / `pub use wry` 在本 crate 之外**零调用方**，
// 只会让"哪些是真正的对外 API"变得不可判定。

/// 创建共享的 WebView2 环境。
///
/// 两个要点：
/// 1. **显式指定用户数据目录**——否则 WebView2 在 exe 旁生成 `<exe>.WebView2\`，
///    破坏单文件分发，且在只读安装目录下会直接失败；
/// 2. **全进程只创建一份**——WebView2 不允许同一用户数据目录存在两个环境，
///    同时这也让内嵌窗口与设置窗口共享缓存，降低内存占用。
pub fn new_web_context() -> WebContext {
    WebContext::new(Some(dsh_core::webview_data_dir()))
}
