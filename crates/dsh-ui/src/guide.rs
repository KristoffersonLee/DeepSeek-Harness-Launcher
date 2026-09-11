//! 新手指引窗口（静态 HTML + 内嵌 WebView2）。
//!
//! 为什么用窗口而不是 `MessageBoxW`：
//! - `ui/guide.html` 是随 exe 内嵌的资源（一致性校验也要求它存在），
//!   但 v5 早期版本改用 MessageBox 输出另一份**手写文本**，等于把同一份指引
//!   维护了两遍（文案一旦改动就会不一致），HTML 资源则完全没人用；
//! - MessageBox 不支持滚动、不能选中复制命令（`npm install -g @deepseek-ai/dsh`）、
//!   在长文案下会被系统截断。
//!
//! 因此这里让指引页回归为真正的窗口，文案唯一来源就是 `ui/guide.html`。
//! 托盘“新手指引”与 `--guide` 启动参数都走这里；WebView2 不可用时
//! 回退到 MessageBox 摘要（可用性底线）。

use tao::dpi::{LogicalSize, Size};
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder, WindowId};
use wry::{WebContext, WebViewBuilder};

/// 内嵌的新手指引 HTML（唯一文案来源）。
pub const GUIDE_HTML: &str = include_str!("../../../ui/guide.html");

/// 新手指引窗口。
pub struct GuideWindow {
    window: Window,
    #[allow(dead_code)] // 持有 WebView 才能维持其生命周期
    webview: wry::WebView,
}

impl GuideWindow {
    /// 创建指引窗口。
    ///
    /// `web_context` 与 Harness / 设置窗口共享（同一用户数据目录只能有一个环境）。
    pub fn new(
        window_target: &EventLoopWindowTarget<()>,
        web_context: &mut WebContext,
    ) -> Result<Self, GuideError> {
        let mut builder = WindowBuilder::new()
            .with_title(format!("{} 新手指引", dsh_core::APP_NAME))
            .with_inner_size(Size::Logical(LogicalSize::new(760.0, 640.0)))
            .with_min_inner_size(Size::Logical(LogicalSize::new(560.0, 420.0)));
        if let Some(icon) = crate::icon::app_window_icon() {
            builder = builder.with_window_icon(Some(icon));
        }
        let window = builder
            .build(window_target)
            .map_err(|e| GuideError::WindowCreation(e.to_string()))?;

        let webview = WebViewBuilder::new_with_web_context(web_context)
            .with_html(GUIDE_HTML.to_string())
            .build(&window)
            .map_err(|e| GuideError::WebViewCreation(e.to_string()))?;

        Ok(Self { window, webview })
    }

    /// 窗口 ID（用于在事件循环中匹配窗口事件）。
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// 显示并聚焦窗口。
    pub fn show(&self) {
        self.window.set_visible(true);
        self.window.set_focus();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GuideError {
    #[error("窗口创建失败：{0}")]
    WindowCreation(String),
    #[error("WebView2 创建失败：{0}")]
    WebViewCreation(String),
}
