//! 托盘菜单事件处理。
//!
//! 在 tao 事件循环中轮询 muda 的 MenuEvent，分发到对应的处理逻辑。

use muda::MenuEvent;

use dsh_ui::TrayEvent;

/// 检查并处理托盘菜单事件。
/// 在 tao 事件循环的每次迭代中调用。
///
/// ## 为什么要**在内部循环排空**
///
/// 调用方写的是 `while let Some(ev) = handle_tray_events() { … }`，其意图是"排空队列"。
/// 但旧实现只 `try_recv()` 一次：若取到的是**无法识别的菜单 id**，
/// `tray_event_from_id` 返回 `None`，函数就返回 `None`，
/// 调用方的 `while let` 立刻结束，**排在后面的合法事件要等下一帧（100ms）才被处理**。
/// 现在内部一直取到队列为空才返回，语义与调用方一致。
pub fn handle_tray_events() -> Option<TrayEvent> {
    loop {
        match MenuEvent::receiver().try_recv() {
            Ok(event) => {
                let id = &event.id().0;
                if let Some(tray_event) = dsh_ui::tray_event_from_id(id) {
                    return Some(tray_event);
                }
                // 未知 id：丢弃并继续取下一个，不要让它截断排空循环
            }
            // 队列为空（或通道断开）→ 本帧没有更多事件
            Err(_) => return None,
        }
    }
}
