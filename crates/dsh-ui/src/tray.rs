//! 托盘图标 + 菜单。
//!
//! 使用 tray-icon + muda 原生菜单。

use muda::{Menu, MenuItem, PredefinedMenuItem};

/// 托盘菜单回调事件。
#[derive(Debug, Clone, PartialEq)]
pub enum TrayEvent {
    Open,
    Refresh,
    OpenInBrowser,
    Start,
    Stop,
    Guide,
    OpenLogDir,
    Settings,
    About,
    Exit,
}

/// 托盘菜单项定义：(事件 ID, 显示文本)。
///
/// 集中成表，避免菜单项与 [`tray_event_from_id`] 两处手写字符串漂移
/// （旧实现里 10 个 `MenuItem::with_id` 各自重复一段 `map_err` 转换）。
const ITEMS: &[(&str, &str)] = &[
    ("open", "打开界面"),
    ("refresh", "刷新界面"),
    ("open_browser", "在浏览器中打开界面"),
    ("start", "启动服务"),
    ("stop", "停止服务"),
    ("guide", "新手指引"),
    ("log_dir", "打开日志目录"),
    ("settings", "设置…"),
    ("about", "关于"),
    ("exit", "退出"),
];

/// 构建托盘图标与菜单。
pub fn build_tray_icon(
    icon: tray_icon::Icon,
) -> Result<tray_icon::TrayIcon, Box<dyn std::error::Error>> {
    let menu = Menu::new();
    // 分隔线位置：三个功能组（界面 / 服务 / 设置与退出）
    const SEPARATOR_AFTER: &[&str] = &["open_browser", "stop", "settings"];

    for (id, label) in ITEMS {
        menu.append(&MenuItem::with_id(*id, *label, true, None))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if SEPARATOR_AFTER.contains(id) {
            menu.append(&PredefinedMenuItem::separator())
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
    }

    let tray = tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(dsh_core::APP_NAME)
        .with_icon(icon)
        .build()?;

    Ok(tray)
}

/// 从菜单事件 ID 解析事件类型。
pub fn tray_event_from_id(id: &str) -> Option<TrayEvent> {
    match id {
        "open" => Some(TrayEvent::Open),
        "refresh" => Some(TrayEvent::Refresh),
        "open_browser" => Some(TrayEvent::OpenInBrowser),
        "start" => Some(TrayEvent::Start),
        "stop" => Some(TrayEvent::Stop),
        "guide" => Some(TrayEvent::Guide),
        "log_dir" => Some(TrayEvent::OpenLogDir),
        "settings" => Some(TrayEvent::Settings),
        "about" => Some(TrayEvent::About),
        "exit" => Some(TrayEvent::Exit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_item_id_is_mapped() {
        // 菜单项与实际事件映射必须一一对应：漏掉一个就会变成“点了没反应”
        for (id, label) in ITEMS {
            assert!(
                tray_event_from_id(id).is_some(),
                "菜单项 {id}（{label}）没有对应事件"
            );
        }
        assert_eq!(ITEMS.len(), 10, "菜单项数量变化需同步文档与自检脚本");
    }

    #[test]
    fn unknown_id_is_ignored() {
        assert_eq!(tray_event_from_id("nope"), None);
        assert_eq!(tray_event_from_id(""), None);
    }
}
