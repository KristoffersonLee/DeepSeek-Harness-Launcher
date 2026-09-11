//! 设置窗口（静态 HTML + wry IPC）。
//!
//! 页面通过 `window.ipc.postMessage(JSON)` 把用户操作发给 Rust 侧，
//! Rust 侧解析为 [`SettingsCommand`] 后用 mpsc 交给主循环处理；
//! 主循环处理完再调用 [`SettingsWindow::set_status`] 把结果回写到页面。
//!
//! HTML 源文件位于 `ui/settings.html`。

use std::sync::mpsc::Sender;

use tao::dpi::{LogicalSize, Size};
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder, WindowId};
use wry::{WebContext, WebViewBuilder};

/// 当前配置快照，用于打开设置页时回填表单。
#[derive(Debug, Clone, Default)]
pub struct SettingsInit {
    pub port: u16,
    pub work_dir: String,
    pub node_path: String,
    pub tray_on_close: bool,
    /// 服务是否独立于启动器（`true` = 默认的 independent 模式）。
    ///
    /// 这里用 `bool` 而非 `dsh_core::ServiceLifecycle`，是为了让 `dsh-ui` 不依赖
    /// `dsh-core` 的领域类型（保持界面层与逻辑层解耦）。
    pub independent_service: bool,
    /// 日志文件的**真实**路径（由 `dsh_core::log_dir()` 派生）。
    ///
    /// 此前页面里这个值是一段硬编码的 `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`：
    /// 当 `dirs::data_local_dir()` 取不到（会退化到当前目录）时，页面会显示一个
    /// 与实际位置不符的路径。现在由 Rust 侧注入唯一真值。
    pub log_path: String,
}

/// 设置页发来的命令。
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCommand {
    /// 保存常规设置
    Save {
        port: String,
        work_dir: String,
        node_path: String,
        /// 「关闭主界面窗口时最小化到托盘」。
        ///
        /// **`Option` 而不是 `bool`**：缺字段时必须保留用户当前值，而不是回退到
        /// 「默认 true」。命令是作用在**现有配置的副本**上的，用 `unwrap_or(true)`
        /// 会把用户显式关掉的偏好悄悄改回开启（旧页面 / 部分字段消息都会踩到）。
        tray_on_close: Option<bool>,
        /// 服务独立于启动器（取消勾选 = 服务随启动器退出而停止）。同样保留当前值。
        independent_service: Option<bool>,
    },
    /// 启动服务
    Start,
    /// 停止服务
    Stop,
    /// 打开日志目录
    OpenLogDir,
    /// 清理归档会话
    CleanArchived,
    /// 恢复默认设置（删除 `settings.toml`，供配置损坏时自救）
    ResetConfig,
}

/// 设置窗口。
pub struct SettingsWindow {
    window: Window,
    webview: wry::WebView,
}

impl SettingsWindow {
    /// 创建设置窗口。
    ///
    /// `web_context` 与内嵌 Harness 窗口共享同一份（同一用户数据目录只能有一个环境）。
    pub fn new(
        window_target: &EventLoopWindowTarget<()>,
        web_context: &mut WebContext,
        init: &SettingsInit,
        tx: Sender<SettingsCommand>,
    ) -> Result<Self, SettingsError> {
        let mut builder = WindowBuilder::new()
            .with_title(format!("{} 设置", dsh_core::APP_NAME))
            .with_inner_size(Size::Logical(LogicalSize::new(720.0, 620.0)))
            .with_min_inner_size(Size::Logical(LogicalSize::new(660.0, 560.0)));
        if let Some(icon) = crate::icon::app_window_icon() {
            builder = builder.with_window_icon(Some(icon));
        }
        let window = builder
            .build(window_target)
            .map_err(|e| SettingsError::WindowCreation(e.to_string()))?;

        let html = include_str!("../../../ui/settings.html");

        let webview = WebViewBuilder::new_with_web_context(web_context)
            .with_html(html.to_string())
            .with_initialization_script(init_script(init))
            // **导航白名单**：本页是内联文档（`with_html`），任何导航都意味着
            // 「IPC 通道 + 初始化脚本（含用户的工作目录/node 路径）被带到一个新文档」。
            // 页面自身没有任何链接/表单，因此白名单只允许它自己的初始文档。
            .with_navigation_handler(|url| is_allowed_navigation(&url))
            .with_ipc_handler(move |request| {
                if let Some(cmd) = parse_command(request.body()) {
                    let _ = tx.send(cmd);
                }
            })
            .build(&window)
            .map_err(|e| SettingsError::WebViewCreation(e.to_string()))?;

        Ok(Self { window, webview })
    }

    /// 重新回填表单（「恢复默认设置」后刷新界面用）。
    pub fn apply_init(&self, init: &SettingsInit) {
        let _ = self.webview.evaluate_script(&init_script(init));
        let _ = self
            .webview
            .evaluate_script("window.__applyInit && window.__applyInit();");
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

    /// 把状态文本回写到页面（调用页面里的 `__setStatus`）。
    ///
    /// **必须双层转义**：状态文本里会出现用户可控内容（工作目录、node 路径、
    /// 端口、错误消息）。旧实现把 `"text": "..."` 形式的 JSON 片段直接拼进
    /// JavaScript 源码，文本中只要有 `"` 或 `</script>` 就能跳出字符串、
    /// 在页面里执行任意脚本。这里先用 `serde_json` 序列化成 JSON，
    /// 再把整个 JSON 当作 **JS 字符串字面量** 序列化一次，注入面彻底关闭。
    pub fn set_status(&self, text: &str, is_error: bool) {
        let payload = serde_json::json!({ "text": text, "error": is_error }).to_string();
        let literal = js_string_literal(&payload);
        let js = format!(
            "(function(){{try{{var p=JSON.parse({literal});\
             window.__setStatus && window.__setStatus(p);}}catch(e){{}}}})();"
        );
        let _ = self.webview.evaluate_script(&js);
    }

    /// 在页面上下文中执行脚本（用于自检探针）。
    pub fn evaluate(&self, js: &str) -> wry::Result<()> {
        self.webview.evaluate_script(js)
    }
}

/// 把 JSON 文本编码为**可安全内联进 HTML/JS 的字符串字面量**。
///
/// 除 `serde_json` 的标准转义外，额外把 `<` / `>` 转成 `\u003c` / `\u003e`：
/// 设置页是通过 `with_html` 内联的文档，若用户目录里出现 `</script>`，
/// 浏览器会提前闭合脚本块，其后的内容就变成 HTML——这是经典注入点。
/// `\u003c` 在 JS 里解析回 `<`，语义不变但不可能闭合标签。
fn js_string_literal(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

/// 允许导航到的地址白名单。
///
/// 页面用 `with_html` 内联，其文档 URL 是实现相关的（wry 在 Windows 上用
/// `about:blank` 承载内联 HTML）。这里采取「只允许空白页与内联 data 文档」的策略：
/// 任何 http/https 导航（例如页面里将来误加的链接、或 dsh 页面反向注入）都会被拒绝，
/// 从纵深防御角度保证 **IPC 通道与初始化脚本永远不会被带到第三方文档**。
fn is_allowed_navigation(url: &str) -> bool {
    let u = url.trim();
    u.is_empty()
        || u.eq_ignore_ascii_case("about:blank")
        || u.starts_with("data:text/html")
        || u.starts_with("blob:")
}

/// 生成回填表单的初始化脚本。
///
/// 与 [`SettingsWindow::set_status`] 同一套双层转义规则：配置里的
/// `work_dir` / `node_path` 是用户可控文本，直接拼进 JS 源码会有注入风险。
fn init_script(init: &SettingsInit) -> String {
    let payload = serde_json::json!({
        "port": init.port,
        "work_dir": init.work_dir,
        "node_path": init.node_path,
        "tray_on_close": init.tray_on_close,
        "independent_service": init.independent_service,
        "log_path": init.log_path,
    })
    .to_string();
    let literal = js_string_literal(&payload);
    format!("window.__INIT__ = JSON.parse({literal}); window.__applyInit && window.__applyInit();")
}

/// 解析页面发来的 JSON 命令。无法识别时返回 `None`（静默忽略，不 panic）。
fn parse_command(body: &str) -> Option<SettingsCommand> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let action = v.get("action")?.as_str()?;
    Some(match action {
        "save" => SettingsCommand::Save {
            port: v
                .get("port")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            work_dir: v
                .get("workDir")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            node_path: v
                .get("nodePath")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            // 缺字段 → `None` = **保留用户当前值**（不是回退默认值）
            tray_on_close: v.get("trayOnClose").and_then(|x| x.as_bool()),
            independent_service: v.get("independentService").and_then(|x| x.as_bool()),
        },
        "start" => SettingsCommand::Start,
        "stop" => SettingsCommand::Stop,
        "openLogDir" => SettingsCommand::OpenLogDir,
        "cleanArchived" => SettingsCommand::CleanArchived,
        "resetConfig" => SettingsCommand::ResetConfig,
        _ => return None,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("窗口创建失败：{0}")]
    WindowCreation(String),
    #[error("WebView2 创建失败：{0}")]
    WebViewCreation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_save_command() {
        let body = r#"{"action":"save","port":"3090","workDir":"D:\\ws","nodePath":"C:\\n\\node.exe","trayOnClose":false,"independentService":false}"#;
        assert_eq!(
            parse_command(body),
            Some(SettingsCommand::Save {
                port: "3090".into(),
                work_dir: "D:\\ws".into(),
                node_path: "C:\\n\\node.exe".into(),
                tray_on_close: Some(false),
                independent_service: Some(false),
            })
        );
    }

    #[test]
    fn save_missing_checkboxes_means_keep_current_value() {
        // **关键语义**：缺字段必须是 `None`（= 保留用户当前值），
        // 不能回退成"默认 true"——`save` 作用在现有配置的**副本**上，
        // 回退默认值会把用户显式关掉的托盘偏好 / 显式选择的 tied 生命周期
        // 悄悄改回开启（旧页面或未来的部分更新都会踩到）。
        let parsed = parse_command(r#"{"action":"save","port":"3080"}"#);
        match parsed {
            Some(SettingsCommand::Save {
                tray_on_close,
                independent_service,
                ..
            }) => {
                assert_eq!(
                    tray_on_close, None,
                    "缺 trayOnClose 时必须表示「保留当前值」"
                );
                assert_eq!(
                    independent_service, None,
                    "缺 independentService 时必须表示「保留当前值」"
                );
            }
            other => panic!("应解析为 Save，实际 {other:?}"),
        }
    }

    #[test]
    fn parses_reset_config() {
        assert_eq!(
            parse_command(r#"{"action":"resetConfig"}"#),
            Some(SettingsCommand::ResetConfig)
        );
    }

    #[test]
    fn navigation_whitelist_blocks_remote_pages() {
        // 纵深防御：内联设置页只允许自己的文档；任何 http(s) 导航都要被拒绝，
        // 否则 IPC 通道与初始化脚本（含用户的工作目录/node 路径）会被带到第三方文档。
        assert!(is_allowed_navigation("about:blank"));
        assert!(is_allowed_navigation(""));
        assert!(is_allowed_navigation("data:text/html,<p>x</p>"));
        assert!(!is_allowed_navigation("http://127.0.0.1:3080/"));
        assert!(!is_allowed_navigation("https://evil.example/"));
        assert!(!is_allowed_navigation("file:///C:/Windows/System32/"));
    }

    #[test]
    fn parses_simple_actions() {
        assert_eq!(
            parse_command(r#"{"action":"start"}"#),
            Some(SettingsCommand::Start)
        );
        assert_eq!(
            parse_command(r#"{"action":"stop"}"#),
            Some(SettingsCommand::Stop)
        );
        assert_eq!(
            parse_command(r#"{"action":"openLogDir"}"#),
            Some(SettingsCommand::OpenLogDir)
        );
        assert_eq!(
            parse_command(r#"{"action":"cleanArchived"}"#),
            Some(SettingsCommand::CleanArchived)
        );
    }

    #[test]
    fn rejects_unknown_or_malformed() {
        // 未知 action / 缺字段 / 非法 JSON / 空串都必须安全返回 None（不能 panic）
        assert_eq!(parse_command(r#"{"action":"nope"}"#), None);
        assert_eq!(parse_command(r#"{}"#), None);
        assert_eq!(parse_command("not json"), None);
        assert_eq!(parse_command(""), None);
        assert_eq!(parse_command(r#"{"action":123}"#), None);
    }

    #[test]
    fn save_command_tolerates_missing_fields() {
        // 页面只发了 action 时不应丢弃整个命令，而是用空值兜底
        let parsed = parse_command(r#"{"action":"save"}"#);
        assert!(matches!(parsed, Some(SettingsCommand::Save { .. })));
        if let Some(SettingsCommand::Save { port, .. }) = parsed {
            assert!(port.is_empty());
        }
    }

    #[test]
    fn init_script_embeds_values() {
        let init = SettingsInit {
            port: 3099,
            work_dir: "D:\\ws".into(),
            node_path: String::new(),
            tray_on_close: false,
            independent_service: true,
            log_path: "C:\\logs\\launcher.log".into(),
        };
        let js = init_script(&init);
        assert!(js.contains("window.__INIT__"));
        assert!(js.contains("JSON.parse("));
        assert!(js.contains("3099"));
        assert!(js.contains("tray_on_close"));
        assert!(js.contains("independent_service"));
    }

    #[test]
    fn init_script_escapes_script_breaking_paths() {
        // 旧实现直接拼 `window.__INIT__ = {...}`：路径里的引号/换行/闭合标签
        // 都会破坏 JS 语法甚至形成注入点。双层转义后必须仍是一段合法脚本。
        let init = SettingsInit {
            port: 3080,
            work_dir: "D:\\a\"b</script>\n'; alert(1);//".into(),
            node_path: "C:\\x\\node.exe".into(),
            tray_on_close: true,
            independent_service: true,
            log_path: "C:\\logs\\launcher.log".into(),
        };
        let js = init_script(&init);
        assert!(!js.contains("</script>"), "{js}");
        assert!(
            !js.contains("alert(1)") || js.contains("\\\""),
            "脚本片段必须被转义：{js}"
        );
        // 关标签必须以 \u003c 形式出现，浏览器不会提前闭合脚本块
        assert!(js.contains("\\u003c"), "尖括号应转义为 \\u003c：{js}");
    }
}
