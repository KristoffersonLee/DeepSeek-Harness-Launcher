//! 原生对话框（MessageBoxW）。
//!
//! 用于退出确认等需要用户决策的场景，避免引入额外 GUI 依赖。

use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDNO, IDYES, MB_DEFBUTTON1, MB_DEFBUTTON2, MB_ICONINFORMATION, MB_ICONQUESTION,
    MB_ICONWARNING, MB_OK, MB_YESNO, MB_YESNOCANCEL,
};

/// 用户在退出确认中的选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitChoice {
    /// 同时停止服务后退出
    StopServiceAndExit,
    /// 保留服务后台运行后退出（v2.0 语义）
    KeepServiceAndExit,
    /// 取消退出
    Cancel,
}

/// 询问「退出启动器时要不要一起停止服务」。
///
/// 语义与 v2.0/v4 一致，但**文案随生命周期策略变化**：
/// - `tied_service = false`（默认 `independent`）：dsh 独立于启动器，退出启动器
///   **本来就不会**影响它。此处只是给用户一个「顺手停掉」的机会。
/// - `tied_service = true`（用户显式选择 `tied`）：退出启动器就会回收 dsh，
///   必须讲清这一点。
///
/// **默认焦点在「否」（保留服务）**：dsh web 是**会话型**服务——正在对话/跑任务的
/// 会话就挂在它上面，停掉即断开。而 `MB_YESNOCANCEL` 默认焦点是「是」，
/// 用户回车或误点就会停服务，表现为“每次退出启动器都强行断开 dsh 连接”。
pub fn confirm_service_stop(
    has_own_service: bool,
    has_adopted: bool,
    tied_service: bool,
) -> ExitChoice {
    let names = match (has_own_service, has_adopted) {
        (true, true) => "服务、外部实例",
        (true, false) => "服务",
        (false, true) => "外部实例",
        (false, false) => "服务",
    };
    let keep_line = if tied_service {
        "【否】（默认，推荐）后台继续运行：本次会解除「随启动器退出而停止」的绑定，\n\
         正在进行的会话与任务不会中断，下次打开启动器会自动重新接管。"
    } else {
        "【否】（默认，推荐）后台继续运行：服务本就独立于启动器，\n\
         正在进行的会话与任务不会中断，下次打开启动器会自动重新接管。"
    };
    let text = format!(
        "DSH Harness 正在运行（{names}）。\n\n\
         退出启动器时，这个服务要一起停止吗？\n\n\
         {keep_line}\n\n\
         【是】停止服务并退出：正在进行的会话与任务会被立即中断。\n\n\
         【取消】不退出，继续使用。"
    );
    let result = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(dsh_core::APP_NAME),
            // MB_DEFBUTTON2 = 默认焦点落在「否」→ 保留服务，避免误回车停掉会话
            MB_YESNOCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2,
        )
    };
    match result {
        IDYES => ExitChoice::StopServiceAndExit,
        IDNO => ExitChoice::KeepServiceAndExit,
        _ => ExitChoice::Cancel, // IDCANCEL 或直接关闭对话框
    }
}

/// 信息提示框。
pub fn info(message: &str, title: &str) {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// 警告提示框。
pub fn warn(message: &str, title: &str) {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_OK | MB_ICONWARNING,
        );
    }
}

/// 危险操作的二次确认（默认焦点在**取消**）。
///
/// ## 为什么需要它
///
/// 「清理归档会话」按设计**必须先停止 dsh 服务**（否则会话目录被占用、删除失败并留下
/// 不一致状态）。但这条副作用此前只写在界面的小字提示里，用户点下去会**突然发现正在
/// 进行的会话断了**——实测就发生过一次，用户以为是启动器自身的缺陷。
///
/// 因此：把后果、范围、以及"服务之后可以再启动"一起说清楚，并把默认焦点放在**取消**上
/// （`MB_DEFBUTTON2`），避免回车误确认。
///
/// 返回 `true` = 用户明确确认；`false` = 取消或直接关闭对话框。
pub fn confirm_dangerous(message: &str, title: &str) -> bool {
    let result = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    result == IDYES
}

/// 询问「是否重启 dsh 服务以恢复内嵌界面的认证」。
///
/// ## 为什么会产生这个询问
///
/// `dsh web` 的浏览器认证只有两条路：启动时打印的**一次性 launch token**（种下会话
/// cookie），或**已种下的签名 cookie**。launch token 只存在于 dsh 进程内存里，因此
/// 「接管别处启动的服务」时启动器读不到它；cookie 又不存在（全新安装 / WebView2
/// profile 被清理 / 超过 30 天 / 端口变化）时，内嵌界面只能显示
/// `dsh web authentication required`。
///
/// 唯一可靠的恢复方式是**让启动器自己拉起 dsh**（它才能读到带 token 的地址）。
///
/// **默认焦点在「是」**：此刻界面已经不可用，重启是"恢复可用"的动作；而且历史会话与
/// 文件都在磁盘上，重启只中断**正在进行**的任务。因此这里与"退出时是否停服务"那类
/// 默认保守的对话框不同，默认选恢复。
pub fn confirm_restart_for_auth(activity: &str) -> bool {
    let text = format!(
        "内嵌界面无法认证，当前显示的是 dsh 的「需要认证」提示页。\n\n\
         原因：正在运行的 dsh 服务不是本启动器启动的，启动器读不到它启动时打印的\n\
         一次性 token；而本机 WebView2 配置里也没有可用的登录 cookie。\n\n\
         活跃会话探测：{activity}\n\n\
         是否**重启 dsh 服务**以便启动器取得新的 token 并自动打开界面？\n\n\
         · 重启会中断正在进行的任务；历史会话与文件都在磁盘上，不会丢失。\n\
         · 重启后的服务由启动器托管，界面会自动刷新为正常页面。\n\
         · 若选择「否」，界面保持不可用；稍后可在托盘菜单执行「停止服务」→「启动服务」。"
    );
    let result = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(format!("{} · 界面需要认证", dsh_core::APP_NAME)),
            // MB_DEFBUTTON1：默认焦点在「是」（界面此刻已不可用，恢复是首要动作）
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON1,
        )
    };
    result == IDYES
}

/// 关于对话框。
///
/// `tied_service` 决定「进程回收」那一行怎么写：默认的 `independent` 模式下
/// dsh **根本没有挂在启动器的 Job 上**（用 `CREATE_BREAKAWAY_FROM_JOB` 脱离），
/// 强杀启动器也**不会**回收它。此前这里无条件写「Windows Job Object（强杀启动器
/// 不留孤儿进程）」——在默认配置下是**错的**，属于对用户的错误承诺。
pub fn about(version: &str, tied_service: bool) {
    let lifecycle = if tied_service {
        "进程回收：Windows Job Object（tied 模式：启动器被强杀时由内核回收 dsh）"
    } else {
        "服务生命周期：独立于启动器（默认）——退出/崩溃/升级启动器都不会中断会话，\n\
         服务归属由 service.json 簿记 + 下次启动对账保证（可选 tied 模式改用 Job Object）"
    };
    let text = format!(
        "{} v{} {}（{} 长期支持版）\n\n\
         内嵌 WebView2 桌面启动器：自动启动 / 接管 dsh web，托盘常驻，无需浏览器。\n\n\
         构建：Rust（wry + tao 直连，无 .NET 依赖，MSRV 1.82）\n\
         {lifecycle}",
        dsh_core::APP_NAME,
        version,
        crate::cli::RELEASE_CHANNEL,
        crate::cli::RELEASE_CHANNEL
    );
    info(&text, "关于");
}

/// 新手指引回退对话框（**仅在 WebView2 创建失败时使用**）。
///
/// 正常路径是 [`dsh_ui::GuideWindow`] —— 它直接渲染 `ui/guide.html`。
/// 这里不再手写第二份文案：旧实现把同一份指引用 MessageBox 又写了一遍，
/// 两处文案必然漂移。现在统一从 HTML 提取纯文本，保证只有**一个文案来源**。
pub fn show_guide_fallback() {
    let text = dsh_ui::GUIDE_HTML;
    let plain = html_to_plain_text(text);
    info(&plain, "新手指引");
}

/// 极简 HTML → 纯文本：去掉 `<script>`/`<style>`、标签，压缩空行。
///
/// 只服务于上面的回退路径，不追求通用性，因此不引入 HTML 解析依赖。
///
/// **两处必须小心的地方**（此前各有一个真实缺陷）：
/// 1. 实体解码**顺序**：必须先解 `&lt;`/`&gt;`/`&quot;`，最后才解 `&amp;`。
///    反过来的话 `&amp;lt;` 会先变成 `&lt;`、再被解码成 `<` —— 双重解码，
///    把用户文本里本来只是"字面量"的内容变成了标签形态（注入/显示错乱）。
/// 2. 块未闭合：`strip_blocks` 遇到没有 `</script>` 的输入会**丢弃剩余全部内容**。
///    那时应当给出固定摘要，而不是把对话框变成空白（用户会以为功能坏了）。
fn html_to_plain_text(html: &str) -> String {
    let (without_blocks, truncated) = strip_blocks(html);

    let mut out = String::with_capacity(without_blocks.len() / 2);
    let mut in_tag = false;
    for ch in without_blocks.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                // 标签边界视作分隔符，避免 `<p>a</p><p>b</p>` 粘成 `ab`
                out.push('\n');
            }
            _ if in_tag => {}
            _ => out.push(ch),
        }
    }

    // 逐行 trim、丢弃空行
    let mut cleaned = String::with_capacity(out.len());
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        cleaned.push_str(t);
        cleaned.push('\n');
    }
    // 去掉常见 HTML 实体（**顺序**：先具体实体，最后才是 &amp;）
    let mut text = cleaned
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string();
    if truncated {
        text.push_str(
            "\n\n（指引内容解析不完整：内嵌 HTML 结构异常，已改用简化提示）\n\
             可在托盘菜单 →「新手指引」重试；若持续失败请查看维护手册。",
        );
    }
    if text.is_empty() {
        text = "新手指引暂时不可用：请从托盘菜单重试，或查看 docs/MAINTENANCE.zh.md。".to_string();
    }
    text
}

/// 删除 `<script ...>...</script>` 与 `<style ...>...</style>` 整块（大小写不敏感）。
///
/// 返回 `(处理后的文本, 是否因块未闭合而被截断)`。
fn strip_blocks(html: &str) -> (String, bool) {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0usize;
    let mut truncated = false;
    while cursor < html.len() {
        // 找最近的 <script / <style 起点
        let next = ["<script", "<style"]
            .iter()
            .filter_map(|t| lower[cursor..].find(t).map(|i| (cursor + i, *t)))
            .min_by_key(|(i, _)| *i);
        let Some((open, tag)) = next else {
            out.push_str(&html[cursor..]);
            break;
        };
        out.push_str(&html[cursor..open]);
        let close_tag = format!("</{}>", &tag[1..]);
        match lower[open..].find(&close_tag) {
            Some(rel_close) => cursor = open + rel_close + close_tag.len(),
            None => {
                // 未闭合：丢弃剩余内容，但**必须让调用方知道**（见 html_to_plain_text）
                truncated = true;
                break;
            }
        }
    }
    (out, truncated)
}
