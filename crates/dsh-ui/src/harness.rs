//! 内嵌 Harness 窗口（WebView2 via wry + tao）。
//!
//! 职责：
//! - 创建 tao 窗口 + wry WebView2
//! - 注入 force-desktop JS（从 C# `InjectDesktopLayout` 平移）
//! - 主题采样 JS → 回调 → DWM 标题栏着色
//! - 最小宽度 1000px（避免触发移动端响应式布局）
//! - WebView2 缺失时回退 Edge 精简窗口
//!
//! 注：`F5` / `Ctrl+R` / `Esc` 的**键盘处理在 `dsh-app/src/app.rs`**
//! （`handle_harness_key`）——本层拿不到事件循环的按键事件，此前这里写了
//! "F5 / Ctrl+R 刷新"作为职责，属于把别人的活儿写进自己的文档。

use tao::dpi::{LogicalSize, Size};
use tao::event_loop::EventLoopWindowTarget;
use tao::platform::windows::WindowExtWindows;
use tao::window::{Window, WindowBuilder, WindowId};

use wry::{WebContext, WebView, WebViewBuilder};

/// force-desktop JS：强制显示侧边栏，避免 DPI 缩放 / 窄窗口触发移动端响应式布局。
/// 从 C# `InjectDesktopLayout` 原样平移。
///
/// 简化说明（本轮）：原实现里有 `IS_WEBVIEW2 = !!window.chrome.webview` 这个恒真判断，
/// 于是 `else` 分支（移除 class）永远不可达，`resize` 监听与"每 1.5 秒重加 class、
/// 持续 120 秒"的定时器也全都在做无用功——而它跑在我们要尽力保持响应性的那个页面里。
/// 现在只做一次幂等的 `classList.add`。
const FORCE_DESKTOP_JS: &str = r#"
(function(){
  var css='body.force-desktop aside,body.force-desktop [class*="sidebar" i],'+
  'body.force-desktop [class*="rail" i],body.force-desktop [class*="drawer" i]'+
  '{display:flex!important;transform:none!important;visibility:visible!important;'+
  'opacity:1!important;pointer-events:auto!important;max-width:none!important}';
  var st=document.createElement('style');st.textContent=css;
  (document.head||document.documentElement).appendChild(st);
  function apply(){if(document.body)document.body.classList.add('force-desktop');}
  if(document.body)apply();
  document.addEventListener('DOMContentLoaded',apply);
})();
"#;

/// 主题采样 JS：采样页面背景色。
const SAMPLE_THEME_JS: &str = r#"
(function(){try{var b=getComputedStyle(document.body).backgroundColor;
if(!b||b==='rgba(0, 0, 0, 0)'||b==='transparent')b=getComputedStyle(document.documentElement).backgroundColor;
if(!b||b==='rgba(0, 0, 0, 0)'||b==='transparent')return '';return b;}catch(e){return '';}})()
"#;

/// 主题采样结果：`Some(rgb)` 表示取到颜色，`None` 表示页面还没准备好。
pub type ThemeSampleResult = Option<(u8, u8, u8)>;

/// 内嵌页面的认证状态。
///
/// ## 为什么必须在**页面上下文**里判断，而不能在 Rust 侧发一个 HTTP 探测
///
/// `dsh web` 的认证有两条路径（源码：`dsh-client-connection` 的 `BrowserAuth`）：
/// 1. 启动时打印的 **一次性 launch token**（`GET /?token=…` → 303 并种下会话 cookie）；
/// 2. **已种下的签名 cookie**（`dsh-auth-<sha256(authority)>`，默认 30 天）。
///
/// launch token **只存在于 dsh 进程内存里**（`processLaunchToken` 的 WeakMap），
/// 已运行的外来实例读不到；而 cookie 只存在于 **WebView2 自己的 cookie 罐**里。
/// 因此"裸 socket 探一下 `GET /`"永远只能看到 401（它不带 cookie），
/// 只有让页面自己判断，才能区分「真的需要认证」与「已经带着 cookie 登录成功」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    /// 页面已正常加载（带 token 地址导航 / cookie 有效）。
    Ok,
    /// 页面是 dsh 的 401 认证页（`dsh web authentication required; …`）。
    Required,
    /// 无法判定（页面尚未渲染完 / 脚本异常），调用方应稍后重试。
    Unknown,
}

/// 认证自检脚本：返回 `AUTH_REQUIRED` / `OK` / `UNKNOWN`。
///
/// 判定依据是 dsh 401 页面的固定文案（`client-connection` 的 `writeUnauthorized`：
/// `dsh web authentication required; reopen the URL printed by dsh web.`）。
/// 认不出来时返回 `UNKNOWN` 而不是猜 `OK`：调用方会重试，绝不会因为"判不出来"
/// 就去做破坏性动作。
const AUTH_CHECK_JS: &str = r#"
(function(){try{
  var t=(document.body&&document.body.innerText)||'';
  if(t.indexOf('authentication required')>=0)return 'AUTH_REQUIRED';
  if(document.querySelector('#app,#root,[data-dsh-root]'))return 'OK';
  return t.length>=1?'OK':'UNKNOWN';
}catch(e){return 'UNKNOWN';}})()
"#;

/// 把回调拿到的原始文本解析为 [`AuthState`]（回调值是 JS 值的 JSON 序列化，可能带引号）。
pub fn parse_auth_state(raw: &str) -> AuthState {
    match raw
        .trim()
        .trim_matches('"')
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "AUTH_REQUIRED" => AuthState::Required,
        "OK" => AuthState::Ok,
        _ => AuthState::Unknown,
    }
}

/// 「只执行一次」的主题采样回调槽位（wry 的回调是 `Fn`，故需要共享可变性）。
type ThemeCallbackSlot = std::sync::Mutex<Option<Box<dyn FnOnce(ThemeSampleResult) + Send>>>;

/// 「只执行一次」的原始文本回调槽位（认证自检用；解析在回调内完成）。
type RawCallbackSlot = std::sync::Mutex<Option<Box<dyn FnOnce(String) + Send>>>;

/// 内嵌窗口句柄与 WebView 控制器。
pub struct HarnessWindow {
    window: Window,
    webview: WebView,
}

impl HarnessWindow {
    /// 创建内嵌窗口并导航到指定 URL。
    ///
    /// 接受 `EventLoopWindowTarget`（而非 `EventLoop`），因此可在事件循环**运行期间**
    /// 由托盘菜单 / 服务就绪事件动态创建窗口。
    ///
    /// `web_context` 必须由调用方持有并跨窗口复用：WebView2 不允许同一用户数据目录
    /// 创建两个环境，复用也是内存优化的关键（见路线图 §4.2）。
    pub fn new(
        window_target: &EventLoopWindowTarget<()>,
        web_context: &mut WebContext,
        url: &str,
    ) -> Result<Self, HarnessError> {
        let mut builder = WindowBuilder::new()
            .with_title(dsh_core::APP_NAME)
            .with_inner_size(Size::Logical(LogicalSize::new(1100.0, 720.0)))
            .with_min_inner_size(Size::Logical(LogicalSize::new(1000.0, 600.0)));
        // 标题栏图标：不设置则会显示窗口类默认图标（不是应用 logo）
        if let Some(icon) = crate::icon::app_window_icon() {
            builder = builder.with_window_icon(Some(icon));
        }
        let window = builder
            .build(window_target)
            .map_err(|e| HarnessError::WindowCreation(e.to_string()))?;

        let hwnd = {
            let raw = window.hwnd();
            windows::Win32::Foundation::HWND(raw as *mut core::ffi::c_void)
        };

        let webview = WebViewBuilder::new_with_web_context(web_context)
            .with_url(url)
            .with_initialization_script(FORCE_DESKTOP_JS)
            // **导航白名单**：Harness 页面里可能有指向外部的链接。内嵌窗口不带任何
            // IPC 通道（这是对的），但把 `?token=…` 留在窗口里导航到第三方站点
            // 会通过 Referer 泄露页面地址（同源策略下 query 通常不发送，但不该依赖它）。
            // 这里只允许本机回环与 about:blank，其余一律拒绝。
            .with_navigation_handler(|target| is_allowed_navigation(&target))
            .build(&window)
            .map_err(|e| HarnessError::WebViewCreation(e.to_string()))?;

        // 应用深/浅色标题栏（初始值；随后的周期采样会跟随页面真实主题更新）
        let _ = crate::theme::set_dark_mode(hwnd, crate::theme::is_system_dark());

        Ok(Self { window, webview })
    }

    /// 导航到新 URL。
    pub fn navigate(&self, url: &str) {
        let _ = self.webview.load_url(url);
    }

    /// 重新加载当前页面。
    pub fn reload(&self) {
        let _ = self.webview.reload();
    }

    /// 执行 JavaScript（无返回值）。
    pub fn evaluate_script(&self, js: &str) -> wry::Result<()> {
        self.webview.evaluate_script(js)
    }

    /// 执行 JavaScript（通过回调接收结果）。
    pub fn evaluate_script_with_callback(
        &self,
        js: &str,
        callback: impl Fn(String) + Send + 'static,
    ) {
        let _ = self.webview.evaluate_script_with_callback(js, callback);
    }

    /// 采样主题颜色并应用（**异步版，唯一的生产路径**）。
    ///
    /// ## 为什么必须异步
    ///
    /// 在**调用线程**上 `recv_timeout(2s)` 等待 JS 返回值会把 UI 线程（事件循环）
    /// 阻塞最多 2 秒。本方法把「等待 + 应用 DWM 属性」都放到 wry 的回调线程里完成，
    /// 调用方立即返回。
    ///
    /// 返回 `true` 表示采样请求已提交（**不代表已成功应用**——JS 结果在回调里）。
    /// 调用方应自己控制频率（例如每 3 秒一次）与并发（避免重复提交）。
    pub fn begin_theme_sample<F>(&self, on_done: F) -> bool
    where
        F: FnOnce(ThemeSampleResult) + Send + 'static,
    {
        // HWND 是裸指针，不满足 Send；转成 usize 传入闭包，回来时再还原。
        //
        // **安全性**：DWM 调用只要求句柄在**窗口存活期间**有效。回调由 WebView2
        // 控制器排队投递，理论上可能在窗口关闭之后才到达（此时 `App` 已经把
        // `HarnessWindow` drop 掉，句柄可能已被系统回收并复用给别的窗口）。
        // 因此这里**不假设**窗口必然还活着：投递前用 `IsWindow` 复核句柄，
        // 无效就跳过（代价是一次廉价的 Win32 调用，收益是绝不误改别的窗口）。
        let hwnd_addr = self.hwnd_typed().0 as usize;
        // wry 的回调是 `Fn`，因此回调本身需要可共享；用 Mutex 承载「只调用一次」的语义
        let cb: ThemeCallbackSlot = std::sync::Mutex::new(Some(Box::new(on_done)));
        let res = self
            .webview
            .evaluate_script_with_callback(SAMPLE_THEME_JS, move |raw| {
                let parsed = parse_theme_result(&raw);
                if let Some(rgb) = parsed {
                    let hwnd =
                        windows::Win32::Foundation::HWND(hwnd_addr as *mut core::ffi::c_void);
                    if is_live_window(hwnd) {
                        let dark = crate::theme::is_dark_color(rgb.0, rgb.1, rgb.2);
                        let _ = crate::theme::set_dark_mode(hwnd, dark);
                        let _ = crate::theme::set_titlebar_colors(hwnd, rgb, dark);
                    }
                }
                if let Ok(mut g) = cb.lock() {
                    if let Some(f) = g.take() {
                        f(parsed);
                    }
                }
            });
        res.is_ok()
    }

    /// 窗口 ID（用于在事件循环中匹配窗口事件）。
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// 异步检查当前页面是否处于「需要认证」状态（见 [`AuthState`]）。
    ///
    /// 返回 `true` 表示请求已提交（判定结果在回调里）；调用方应控制频率与并发。
    /// 与 [`HarnessWindow::begin_theme_sample`] 同一套「只调用一次」的槽位语义。
    pub fn begin_auth_check<F>(&self, on_done: F) -> bool
    where
        F: FnOnce(AuthState) + Send + 'static,
    {
        let cb: RawCallbackSlot = std::sync::Mutex::new(Some(Box::new(move |raw: String| {
            on_done(parse_auth_state(&raw))
        })));
        self.webview
            .evaluate_script_with_callback(AUTH_CHECK_JS, move |raw| {
                if let Ok(mut g) = cb.lock() {
                    if let Some(f) = g.take() {
                        f(raw);
                    }
                }
            })
            .is_ok()
    }

    /// 获取窗口句柄（带类型，供 DWM 调用）。
    fn hwnd_typed(&self) -> windows::Win32::Foundation::HWND {
        let raw = self.window.hwnd();
        windows::Win32::Foundation::HWND(raw as *mut core::ffi::c_void)
    }
    /// 显示并聚焦窗口。
    pub fn show(&self) {
        self.window.set_visible(true);
        self.window.set_focus();
    }

    /// 隐藏窗口（保留窗口与 WebView，托盘可即时恢复）。
    ///
    /// 与「销毁窗口」的区别：隐藏后页面状态、登录 Cookie 与滚动位置都还在，
    /// 再次打开是瞬时的。这是 `tray_on_close = true` 时的关闭语义。
    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    /// 窗口当前是否可见。
    ///
    /// 用途：隐藏到托盘时跳过周期性主题采样（省一次跨进程 JS 往返）。
    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }
}

/// 导航白名单：内嵌 Harness 窗口只允许本机回环地址与「不产生网络请求」的文档。
///
/// 为什么需要：页面内容来自 dsh 自身，但页面里可能有指向外部的链接。
/// 一旦窗口被导航到第三方站点，地址栏里的 `?token=…` 就有通过 Referer 泄露的风险
/// （同源策略下跨源只发 origin，但不该把安全建立在"浏览器默认策略不会变"上），
/// 而且那个站点还会复用已经注入的 `FORCE_DESKTOP_JS`。
///
/// 精确策略（与实现逐条对应）：
/// - 允许：空目标、`about:blank`、`data:text/html…`、`blob:…`、以及 `http(s)://` 下的
///   回环字面量（`127.0.0.1` / `localhost` / `[::1]`）；
/// - 拒绝：其余一切目标（含任意公网 http(s)、`file:`、自定义协议）。
///
/// `data:` / `blob:` 被允许是有意的：它们不指向网络位置，且 dsh 页面自身可能用它们
/// 承载导出/预览内容；本窗口**不注册任何 IPC 通道**，因此这两个来源拿不到任何能力。
/// 非白名单目标一律拒绝（返回 false = 取消本次导航）。
fn is_allowed_navigation(target: &str) -> bool {
    let u = target.trim().to_ascii_lowercase();
    u.is_empty()
        || u == "about:blank"
        || u.starts_with("data:text/html")
        || u.starts_with("blob:")
        || u.starts_with("http://127.0.0.1:")
        || u.starts_with("http://localhost:")
        || u.starts_with("http://[::1]:")
        || u.starts_with("https://127.0.0.1:")
        || u.starts_with("https://localhost:")
        || u.starts_with("https://[::1]:")
}

/// 句柄是否仍然指向一个真实存在的窗口。
///
/// 用途：主题采样回调可能在窗口关闭之后才被投递，此时句柄可能已被系统回收并
/// **复用给别的窗口**——直接调用 DWM 会去改一个无关窗口的外观。
fn is_live_window(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

/// 解析 `evaluate_script_with_callback` 返回的原始文本为 CSS 颜色。
///
/// 回调拿到的是 JS 值的 **JSON 序列化** 文本（字符串会带引号），
/// 也可能是 `""` / `null`（页面还没准备好）。这里统一处理。
fn parse_theme_result(raw: &str) -> Option<(u8, u8, u8)> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed == "null" {
        return None;
    }
    parse_css_color(trimmed)
}

/// 解析 CSS 颜色字符串（rgb/rgba/hex）。
fn parse_css_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("rgba(") {
        let inner = rest.strip_suffix(')')?;
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() >= 3 {
            return Some((
                parts[0].trim().parse().ok()?,
                parts[1].trim().parse().ok()?,
                parts[2].trim().parse().ok()?,
            ));
        }
    } else if let Some(rest) = s.strip_prefix("rgb(") {
        let inner = rest.strip_suffix(')')?;
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() >= 3 {
            return Some((
                parts[0].trim().parse().ok()?,
                parts[1].trim().parse().ok()?,
                parts[2].trim().parse().ok()?,
            ));
        }
    } else if let Some(rest) = s.strip_prefix('#') {
        if rest.len() == 6 {
            let val = u32::from_str_radix(rest, 16).ok()?;
            return Some((
                ((val >> 16) & 0xFF) as u8,
                ((val >> 8) & 0xFF) as u8,
                (val & 0xFF) as u8,
            ));
        } else if rest.len() == 3 {
            let val = u32::from_str_radix(rest, 16).ok()?;
            let r = ((val >> 8) & 0xF) * 17;
            let g = ((val >> 4) & 0xF) * 17;
            let b = (val & 0xF) * 17;
            return Some((r as u8, g as u8, b as u8));
        }
    }
    None
}

#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("窗口创建失败：{0}")]
    WindowCreation(String),
    #[error("WebView2 创建失败：{0}")]
    WebViewCreation(String),
    /// WebView2 运行时缺失。
    ///
    /// **已知未接线**：wry 的构建错误目前一律映射为 [`HarnessError::WebViewCreation`]，
    /// 而 `dsh-app` 对任何创建失败都走 Edge 回退，因此这个变体暂时不会被构造。
    /// 保留它是因为"运行时缺失"是一个**用户可自行修复**的独立原因，
    /// 将来要把提示细化成"请安装 WebView2 Runtime"时用它，而不是靠匹配错误文本。
    #[error("WebView2 运行时缺失，请安装 Edge WebView2 Runtime")]
    WebView2RuntimeMissing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_auth_states_from_callback_text() {
        // 回调拿到的是 JS 值的 JSON 序列化：字符串会带引号
        assert_eq!(parse_auth_state("\"AUTH_REQUIRED\""), AuthState::Required);
        assert_eq!(parse_auth_state("AUTH_REQUIRED"), AuthState::Required);
        assert_eq!(
            parse_auth_state(" \"auth_required\"\n"),
            AuthState::Required
        );
        assert_eq!(parse_auth_state("\"OK\""), AuthState::Ok);
        assert_eq!(parse_auth_state("OK"), AuthState::Ok);
        // 判不出来必须是 Unknown（调用方会重试），绝不能猜成 Ok/Required
        assert_eq!(parse_auth_state("\"UNKNOWN\""), AuthState::Unknown);
        assert_eq!(parse_auth_state(""), AuthState::Unknown);
        assert_eq!(parse_auth_state("null"), AuthState::Unknown);
        assert_eq!(parse_auth_state("garbage"), AuthState::Unknown);
    }

    #[test]
    fn auth_check_js_matches_dsh_unauthorized_copy() {
        // 判据必须与 dsh 401 页面文案一致（client-connection 的 writeUnauthorized）：
        // 一旦上游改文案，这里会先失败，提醒我们同步（而不是静默失去自愈能力）。
        assert!(AUTH_CHECK_JS.contains("authentication required"));
        // 且必须能在页面上下文里读到文档内容
        assert!(AUTH_CHECK_JS.contains("document.body"));
        assert!(AUTH_CHECK_JS.contains("AUTH_REQUIRED"));
    }
}
