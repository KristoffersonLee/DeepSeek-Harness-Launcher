//! 主应用循环：组装 dsh-core + dsh-ui，运行 tao 事件循环。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tao::event::{ElementState, Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::keyboard::KeyCode;
use tao::window::WindowId;

use dsh_core::{ActivationEvent, QuitEvent, RollingLogger, Settings, SingleInstance};

use crate::service::{ServiceEvent, ServiceHandle};
use crate::tray_handler::handle_tray_events;

/// 归档清理在后台线程完成后回传的结果。
type CleanResult = Result<(usize, String), String>;

/// 后台任务回传结果。
///
/// ## 为什么统一成一个通道
///
/// 凡「会阻塞数百毫秒以上、又必须改状态」的动作都放到 worker 线程，结果回到主循环：
/// - **进程终止**（递归杀进程树 + `wait`）—— UI 线程做这个会把界面冻住；
/// - **归档清理**（停服 + 每会话最多 4×300 ms 重试）；
/// - **配置变更触发的服务重启**（stop → set_config → start 必须串行）。
///
/// 以前只有归档清理走通道，`stop()` 与配置重启直接在 UI 线程里跑，
/// 于是「点停止服务 / 点保存」都会把界面冻住数百毫秒到数秒。
#[derive(Debug)]
enum BackgroundTask {
    /// 归档清理结束（`Ok((已清理数, 详情))` / `Err(说明)`）
    Cleanup(CleanResult),
    /// 「保存设置 / 恢复默认设置」触发的服务重启
    Restart(Result<(), String>),
    /// 托盘 / 设置页的「停止服务」
    Stop(Result<(), String>),
    /// 托盘 / 设置页 / 启动阶段的「启动服务」
    Start(Result<(), String>),
    /// 认证自愈前的「活跃会话」探测结果（在 worker 线程完成，见 [`App::maybe_check_auth`]）
    AuthActivity(Option<dsh_core::ActiveSessions>),
    /// 「清理归档会话」的前置探测结果（活跃会话 + 当前归档数）。
    ///
    /// **必须在 worker 线程完成**：探测里有一次 `Get-CimInstance`（实测 ~1.3 秒，
    /// 上限 5 秒），放在 UI 线程上就是「点了清理按钮后界面卡住一秒多」。
    /// 确认框仍在主线程弹（模态对话框本来就要求 UI 线程），但**探测不再阻塞它**。
    CleanupPrecheck {
        running: bool,
        active: Option<dsh_core::ActiveSessions>,
        count: usize,
    },
}

/// 内嵌界面处于「需要认证」状态时的恢复策略。
///
/// 抽成纯函数是为了可单测：真正的动作（重启服务）会中断会话，判定逻辑必须被锁住。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthRecovery {
    /// 没有活跃会话 ⇒ 直接自动重启（不打扰用户）
    AutoRestart,
    /// 有活跃会话（或无法判定）⇒ 必须显式询问
    AskUser,
}

/// 依据活跃会话探测结果决定恢复策略。
///
/// `None`（探测失败/无法判定）按**有活跃会话**处理：宁可多问一次，也不静默打断任务。
fn auth_recovery_action(activity: Option<&dsh_core::ActiveSessions>) -> AuthRecovery {
    match activity {
        Some(a) if !a.any_activity() => AuthRecovery::AutoRestart,
        _ => AuthRecovery::AskUser,
    }
}

/// 事件循环空闲时的等待间隔。
///
/// 事件循环不能用 `ControlFlow::Wait`：托盘菜单、服务事件都靠轮询 mpsc 通道，
/// 没有窗口事件时 `Wait` 会永久休眠。100ms 是响应速度与唤醒次数的折中
/// （托盘点击 ≤100ms 生效，空闲时约 10 次/秒）。
const IDLE_TICK: Duration = Duration::from_millis(100);

/// 唤起请求的最长等待（tick 数）：服务 30 秒仍不就绪则放弃，避免永久挂起。
const ACTIVATION_MAX_WAIT_TICKS: u64 = 300;

/// 「端口是否已可连接」探测的最小间隔。
///
/// `is_port_listening` 是一次**阻塞** connect（单次最长 300 ms）。它有两类调用方：
/// 唤起待办、以及「打开界面」的每帧泵——两者都在事件循环（100 ms/帧）里。
/// 逐帧探测会让 UI 线程有约 75% 的时间卡在这个调用上（服务未起来时尤其明显，
/// 而冷启动实测要 30 秒以上）。这里按**时间**限流，一次探测的结果在间隔内复用。
const READY_PROBE_INTERVAL: Duration = Duration::from_millis(900);

/// 等待「带 token 的 dsh 地址」被捕获的最长 tick 数（100ms/tick ⇒ 约 5 秒）。
///
/// dsh 打印 `dsh web: http://…?token=…` 通常在 1 秒内，5 秒足够；
/// 超时后退回普通地址开窗（用户可能需要在页面里手动授权），但不会永久卡住。
const AUTH_URL_WAIT_TICKS: u32 = 50;

/// 主题采样的**最小提交间隔**。
///
/// 为什么按时间而不是按帧数：帧率不可控（实测事件循环可跑到数百 fps），按帧计数
/// 会把「每 30 帧」变成「每秒好几次」，配合回调就成了日志刷屏。
const THEME_SAMPLE_INTERVAL: Duration = Duration::from_secs(3);

/// 主题采样回调的超时：超过它仍未拿到结果就解除 pending，避免永久停采。
const THEME_SAMPLE_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

/// 内嵌窗口打开/导航后，多久开始做「页面是否需要认证」的自检。
///
/// 必须等页面真正渲染出文档：导航刚提交时 `document.body` 还是空的，
/// 自检会得到 `Unknown`（会重试，但白白多跑一轮）。
const AUTH_CHECK_DELAY: Duration = Duration::from_millis(1500);

/// 同一次导航内，认证自检的最大尝试次数（间隔 [`AUTH_CHECK_DELAY`]）。
///
/// 上限存在的意义：页面永远渲染不出来时不能无限重试（每轮都要一次 JS 往返）。
const AUTH_CHECK_MAX_ATTEMPTS: u32 = 6;

/// 认证自检回调的超时（超过即视为本轮无结论，计入尝试次数）。
const AUTH_CHECK_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

/// 启动阶段计时器（由 `main.rs` 传入，与其共用同一套 `[boot]` 日志格式）。
///
/// 单独定义而不跨模块共享，是为了让 `app.rs` 不依赖 `main.rs` 的内部类型；
/// 0 点统一为 `main` 入口。
#[derive(Clone, Copy)]
pub struct BootTimer(std::time::Instant);

impl BootTimer {
    /// 以 `main` 入口为 0 点新建计时器。
    pub fn start() -> Self {
        Self(std::time::Instant::now())
    }

    pub fn mark(&self, logger: &RollingLogger, label: &str) {
        let line = format!("[boot] {label}: {}ms", self.0.elapsed().as_millis());
        // 注意：不能只用 eprintln! —— release 是 Windows GUI 子系统，stderr 句柄可能
        // 无效，而 eprintln! 在写入失败时会 panic（panic=abort ⇒ 整进程崩溃）。
        {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "{line}");
        }
        logger.info(&line);
    }
}

/// 判断 URL 是否带 dsh 的一次性认证 token。
///
/// 只有带 token 的地址才算「可用于导航」——无 token 访问返回 HTTP 401（已实测）。
fn contains_token(url: &str) -> bool {
    url.contains("token=")
}

/// 应用主状态。
struct App {
    config: Settings,
    logger: RollingLogger,
    service: ServiceHandle,
    tx: mpsc::Sender<ServiceEvent>,
    rx: mpsc::Receiver<ServiceEvent>,
    /// 内嵌 Harness 窗口（懒创建；关闭后置空，可再次打开）
    harness: Option<dsh_ui::HarnessWindow>,
    /// 设置窗口（懒创建）
    settings: Option<dsh_ui::SettingsWindow>,
    /// 新手指引窗口（懒创建；渲染 ui/guide.html）
    guide: Option<dsh_ui::GuideWindow>,
    /// 最近一次待导航的 URL（窗口尚未创建时暂存）
    pending_url: Option<String>,
    /// 共享的 WebView2 环境（全进程一份，见 dsh_ui::new_web_context）
    web_context: dsh_ui::WebContext,
    /// 设置页命令通道（接收端）
    settings_rx: mpsc::Receiver<dsh_ui::SettingsCommand>,
    /// 设置页命令通道（发送端，创建窗口时交给页面）
    settings_tx: mpsc::Sender<dsh_ui::SettingsCommand>,
    /// 后台任务结果回传通道
    task_tx: mpsc::Sender<BackgroundTask>,
    task_rx: mpsc::Receiver<BackgroundTask>,
    /// 单实例锁：必须活到进程退出（Drop 会释放内核对象）
    _single: SingleInstance,
    /// 唤起事件：第二个实例双击图标时用它把窗口显示到前台
    activation: ActivationEvent,
    /// 退出请求事件：`DSHLauncher.exe --quit` 用它请求本实例优雅退出
    quit_event: QuitEvent,
    /// 退出**回执**事件：收尾完成（或用户取消退出）时置位，
    /// 让 `--quit` 的请求方能区分「已退出」与「用户拒绝了退出」。
    quit_ack: dsh_core::QuitAckEvent,
    /// 收到唤起信号但当时还没有窗口（服务尚未就绪）→ 等首个窗口出现后再唤起
    activation_pending: bool,
    /// 唤起待办的过期 tick（防止服务始终起不来时永久挂起）
    activation_deadline: u64,
    /// 等待 token 地址已消耗的 tick 数（见 [`AUTH_URL_WAIT_TICKS`]）
    auth_wait_ticks: u32,
    /// 归档清理是否正在进行（IPC 重入保护：清理是长阻塞操作，连点会并发起线程）
    cleaning: bool,
    /// 归档清理的**前置探测**是否在途（同上：探测要跑外部命令，连点会并发起线程）
    cleanup_prechecking: bool,
    /// 归档清理前服务是否在运行 —— 清理完成后据此自动把服务拉回来
    cleanup_restore_service: bool,
    /// Ctrl 是否按下（用于 Ctrl+R 刷新；tao 的 KeyEvent 不携带修饰键状态）
    ctrl_down: bool,
    /// 是否已提交一次主题采样但结果尚未回来（避免并发堆积 JS 调用）
    theme_sample_pending: bool,
    /// 主题采样结果的共享槽位（回调线程写入，主循环读出）
    theme_slot: Option<ThemeSlot>,
    /// 「首次采样未取到背景色」是否已说明过（避免逐帧刷屏）
    theme_warned_no_color: bool,
    /// 唤醒请求：**是否希望界面处于打开状态**。
    ///
    /// 这是「等待 token 再导航」逻辑的**驱动源**。历史缺陷：等待预算
    /// （[`AUTH_URL_WAIT_TICKS`]）只在 `Ready` / 托盘点击等**事件**里被消耗，
    /// 而事件之间没有任何东西重新驱动 `open_harness()`——
    /// 于是若 dsh 没有打印带 token 的地址（上游改格式、或 token 行落在 stderr），
    /// 预算永远走不完，**窗口永远不会出现**（用户看到的就是"双击后什么都没有"）。
    /// 现在每帧在 `want_open` 为真时重试一次，使预算变成真正的**时间**预算。
    want_open: bool,
    /// 最近一次成功应用到标题栏的颜色（**只在变化时打日志**）
    theme_last_color: Option<(u8, u8, u8)>,
    /// 最近一次**提交**采样的时刻（按时间限流的依据）
    theme_sample_at: Option<std::time::Instant>,
    /// 最近一次端口可达性探测的时刻（`None` = 从未探测）
    port_probe_at: Option<std::time::Instant>,
    /// 最近一次端口可达性探测的结果（见 [`READY_PROBE_INTERVAL`]）
    port_probe_ok: bool,
    /// 「服务尚未就绪」是否已经记过日志（避免每帧刷一条）
    not_ready_logged: bool,
    /// 认证自检结果的共享槽位（回调线程写入，主循环读出）
    auth_slot: Option<AuthSlot>,
    /// 是否已提交一次认证自检但结果尚未回来
    auth_check_pending: bool,
    /// 本轮自检的提交时刻（超时保护）
    auth_check_at: Option<std::time::Instant>,
    /// 本轮自检已尝试次数（见 [`AUTH_CHECK_MAX_ATTEMPTS`]）
    auth_check_attempts: u32,
    /// 「界面已认证」是否已记过日志（每轮导航只记一次）
    auth_ok_logged: bool,
    /// 本轮导航是否已经就「需要认证」做过处置（避免重复弹窗/重复重启）
    auth_recovery_offered: bool,
}

/// 认证自检结果的共享槽位类型（`None` = 回调还没到）。
type AuthSlot = Arc<Mutex<Option<dsh_ui::AuthState>>>;

/// 主题采样结果的共享槽位类型（`None` = 回调还没到，`Some(x)` = 已到）。
type ThemeSlot = Arc<Mutex<Option<dsh_ui::ThemeSampleResult>>>;

/// 启动参数（把三个布尔开关收进一个结构体）。
///
/// 为什么分组：`App::run` 原本是 `(open_settings, open_guide, ipc_probe, boot)`，
/// 加上 `App::new` 已经接收的 config/logger/single/activation，参数个数会触发
/// `clippy::too_many_arguments`；而且这三个开关总是从同一处（命令行）一起流转，
/// 打包成一个 `Copy` 结构体语义更清晰，也避免将来再加开关时继续膨胀签名。
#[derive(Debug, Default, Clone, Copy)]
pub struct StartupOptions {
    /// `--settings`：启动时打开设置窗口
    pub open_settings: bool,
    /// `--guide`：启动时打开新手指引窗口
    pub open_guide: bool,
    /// `--ipc-probe`：自检用，模拟一次设置页按钮点击
    pub ipc_probe: bool,
}

impl App {
    pub fn new(
        config: Settings,
        logger: RollingLogger,
        single: SingleInstance,
        activation: ActivationEvent,
        quit_event: QuitEvent,
        quit_ack: dsh_core::QuitAckEvent,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let service = ServiceHandle::new(config.clone(), logger.clone());
        let (settings_tx, settings_rx) = mpsc::channel();
        let (task_tx, task_rx) = mpsc::channel();

        Self {
            config,
            logger,
            service,
            tx,
            rx,
            harness: None,
            settings: None,
            guide: None,
            pending_url: None,
            web_context: dsh_ui::new_web_context(),
            settings_rx,
            settings_tx,
            task_tx,
            task_rx,
            _single: single,
            activation,
            quit_event,
            quit_ack,
            activation_pending: false,
            activation_deadline: 0,
            auth_wait_ticks: 0,
            cleaning: false,
            cleanup_prechecking: false,
            cleanup_restore_service: false,
            ctrl_down: false,
            theme_sample_pending: false,
            theme_slot: None,
            theme_warned_no_color: false,
            theme_last_color: None,
            theme_sample_at: None,
            port_probe_at: None,
            port_probe_ok: false,
            not_ready_logged: false,
            auth_slot: None,
            auth_check_pending: false,
            auth_check_at: None,
            auth_check_attempts: 0,
            auth_ok_logged: false,
            auth_recovery_offered: false,
            want_open: false,
        }
    }

    /// 安排一次认证自检（窗口新建或每次导航之后调用）。
    ///
    /// 时序：等 [`AUTH_CHECK_DELAY`] 让页面渲染出文档 → 在**页面上下文**里判断
    /// 是否为 401 认证页（见 [`dsh_ui::AuthState`]，裸 socket 探测区分不了
    /// "WebView2 已有 cookie"的情况）。
    fn schedule_auth_check(&mut self) {
        self.auth_slot = None;
        self.auth_check_pending = false;
        self.auth_check_attempts = 0;
        self.auth_ok_logged = false;
        self.auth_recovery_offered = false;
        self.auth_check_at = Some(std::time::Instant::now() + AUTH_CHECK_DELAY);
    }

    /// 每帧驱动认证自检（取结果 / 提交下一轮 / 处置）。
    ///
    /// 与 `maybe_sample_theme` 同一套「异步 + 共享槽位 + 不阻塞事件循环」的做法。
    fn maybe_check_auth(&mut self) {
        // 1) 取上一轮结果（取出并清空）
        let got = self
            .auth_slot
            .as_ref()
            .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
        if let Some(state) = got {
            self.auth_check_pending = false;
            match state {
                dsh_ui::AuthState::Ok => {
                    if !self.auth_ok_logged {
                        self.auth_ok_logged = true;
                        self.logger.info(
                            "内嵌界面认证正常（带 token 地址导航，或本机已有有效会话 cookie）。",
                        );
                    }
                    self.auth_check_at = None;
                    return;
                }
                dsh_ui::AuthState::Required => {
                    self.auth_check_at = None;
                    if !self.auth_recovery_offered {
                        self.recover_from_auth_required();
                    }
                    return;
                }
                dsh_ui::AuthState::Unknown => {
                    // 页面还没渲染完：继续等到下一轮（次数用尽就放弃）
                }
            }
        }

        // 2) 回调始终不来时的超时保护，避免 pending 卡死
        if self.auth_check_pending {
            if self
                .auth_check_at
                .is_some_and(|t| t.elapsed() > AUTH_CHECK_CALLBACK_TIMEOUT)
            {
                self.auth_check_pending = false;
            } else {
                return;
            }
        }
        if self.auth_recovery_offered || self.auth_check_attempts >= AUTH_CHECK_MAX_ATTEMPTS {
            return;
        }
        let Some(at) = self.auth_check_at else { return };
        if std::time::Instant::now() < at {
            return;
        }
        // 窗口不存在 / 被隐藏（最小化到托盘）时不检查：没有可见页面就没有可诊断的内容
        let Some(h) = &self.harness else {
            self.auth_check_at = None;
            return;
        };
        if !h.is_visible() {
            return;
        }

        let slot: AuthSlot = Arc::new(Mutex::new(None));
        let slot_for_cb = Arc::clone(&slot);
        self.auth_check_attempts += 1;
        self.auth_check_at = Some(std::time::Instant::now() + AUTH_CHECK_DELAY);
        let submitted = h.begin_auth_check(move |state| {
            if let Ok(mut g) = slot_for_cb.lock() {
                *g = Some(state);
            }
        });
        if submitted {
            self.auth_check_pending = true;
            self.auth_slot = Some(slot);
        }
    }

    /// 内嵌界面确认处于「需要认证」状态时的处置（自愈）。
    ///
    /// ## 为什么会出现这个状态
    ///
    /// `dsh web` 的浏览器认证只有两条路（源码 `dsh-client-connection` 的 `BrowserAuth`）：
    /// ① 启动时打印的**一次性 launch token**（`GET /?token=…` → 303 并种下 30 天会话 cookie）；
    /// ② 已种下的**签名 cookie**。
    ///
    /// launch token **只存在于 dsh 进程内存**（`processLaunchToken` 的 WeakMap），
    /// 所以"接管别处启动的服务"时启动器**读不到**它；此时唯一能用的就是 WebView2 profile
    /// 里的 cookie。cookie 不存在（全新安装 / profile 被清理 / 超过 30 天 / 端口变了导致
    /// cookie 名不同）时，界面必然显示 `dsh web authentication required`。
    ///
    /// ## 处置策略
    ///
    /// **让启动器自己拉起 dsh**（这样才能读到带 token 的地址并种下 cookie）：
    /// - 探测到**没有**活跃会话 → 直接重启（用户不会被打断，只写日志）；
    /// - 探测到**有**活跃会话 → 明确询问（默认「是」），说清"会中断正在进行的任务、
    ///   但历史会话与文件都在磁盘上"；
    /// - 用户拒绝 → 如实记录并告诉他手动恢复方式（托盘「停止服务」→「启动服务」）。
    fn recover_from_auth_required(&mut self) {
        self.auth_recovery_offered = true;
        self.logger.error(
            "内嵌界面要求认证（401）：当前 dsh 服务不是本启动器启动的，读不到它的一次性 token，\
             且本机 WebView2 配置里没有可用的登录 cookie。正在后台探测是否还有活跃会话…",
        );
        // 活跃会话探测会调用外部命令（`Get-CimInstance`，实测 ~1.3 秒、上限 5 秒），
        // **绝不能放在 UI 线程**：这里丢给 worker，结果经 `BackgroundTask::AuthActivity` 回来。
        let tx = self.task_tx.clone();
        let logger = self.logger.clone();
        let spawned = std::thread::Builder::new()
            .name("dsh-auth-probe".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let activity = dsh_core::probe_active_sessions();
                match &activity {
                    Some(a) => logger.info(&format!("认证自愈前的活跃会话探测：{}", a.summary())),
                    None => logger.warn("认证自愈前的活跃会话探测：无法判定（按有活跃会话处理）"),
                }
                let _ = tx.send(BackgroundTask::AuthActivity(activity));
            });
        if spawned.is_err() {
            // 起不了线程：按"无法判定"处理（改为询问），绝不猜"没有会话"就直接重启
            self.logger
                .warn("无法创建探测线程，按「有活跃会话」处理（将询问用户）。");
            self.apply_auth_recovery(None);
        }
    }

    /// 依据（后台探测到的）活跃会话情况执行认证自愈决策。
    fn apply_auth_recovery(&mut self, activity: Option<dsh_core::ActiveSessions>) {
        let busy_summary = activity
            .as_ref()
            .map(|a| a.summary())
            .unwrap_or_else(|| "无法判定（按有活跃会话处理）".to_string());
        let confirmed = match auth_recovery_action(activity.as_ref()) {
            AuthRecovery::AskUser => crate::dialog::confirm_restart_for_auth(&busy_summary),
            AuthRecovery::AutoRestart => {
                self.logger.warn(
                    "未检测到活跃会话：自动重启 dsh 服务，以便启动器取得带 token 的地址\
                     （历史会话与文件都在磁盘上，不丢失）。",
                );
                true
            }
        };
        if !confirmed {
            self.logger
                .warn("用户取消重启：界面仍不可用。手动恢复方式：托盘「停止服务」→「启动服务」。");
            crate::dialog::info(
                "界面暂时无法认证（当前服务不是本启动器启动的）。\n\n\
                 手动恢复：托盘菜单 →「停止服务」，再点「启动服务」——\n\
                 服务由启动器自己启动后，界面会自动打开。",
                &format!("{} · 界面认证", dsh_core::APP_NAME),
            );
            return;
        }

        self.logger
            .info("正在重启 dsh 服务以便启动器取得带 token 的地址（重启后内嵌界面会自动纠正）…");
        self.pending_url = None;
        self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
        self.want_open = true;
        // 重启走与「保存设置」完全相同的串行编排（stop → set_config → start），
        // 新进程的 launch token 会被 stdout 回调捕获，随后补发的 Ready 会带上它。
        self.spawn_restart(self.config.clone());
    }

    /// 端口是否已可连接（**阻塞 connect，按时间限流**）。
    ///
    /// 见 [`READY_PROBE_INTERVAL`]：间隔内直接复用上一次结果，因此每帧最多只会有
    /// 一次真正阻塞的探测。三处调用方（唤起待办、`open_harness`、启动参数）共用同一
    /// 份缓存，避免"每个路径各自探测"造成叠加阻塞。
    fn port_reachable(&mut self) -> bool {
        let now = std::time::Instant::now();
        if let Some(at) = self.port_probe_at {
            if now.duration_since(at) < READY_PROBE_INTERVAL {
                return self.port_probe_ok;
            }
        }
        self.port_probe_at = Some(now);
        self.port_probe_ok = dsh_core::is_port_listening(self.config.port);
        self.port_probe_ok
    }
    /// 判定「现在可以安全导航了吗」，并给出应当使用的地址。
    ///
    /// 返回 `Some(url)` = 可以导航；`None` = 还要再等（token 尚未到位）。
    ///
    /// 放行条件（任一）：
    /// - 已有**带 token** 的待导航地址；
    /// - 服务不是本进程启动的（接管外部实例，token 本来就拿不到，只能走 Cookie）；
    /// - 服务已捕获到 token；
    /// - 等待超过 [`AUTH_URL_WAIT_TICKS`]（放弃等待，退回普通地址并提示）。
    ///
    /// **与历史实现的区别（重要）**：旧版把「`pending_url.is_some()`」当作放行条件，
    /// 而 `Ready` 事件当时无条件填入**无 token** 的普通地址，于是这个条件恒真、
    /// 下面的等待逻辑**永远不可达**，用户看到的就是 HTTP 401 认证失败页（实测）。
    /// 现在只有带 token 的地址才算「已就绪」，等待逻辑真正可达。
    ///
    /// **调用方必须保证服务已经可达**（见 [`App::open_harness`] 里的端口检查）：
    /// 否则冷启动那 30 秒会白白吃掉 5 秒预算，最后还是开出一个 401 页面。
    fn auth_url_settled(&mut self) -> Option<String> {
        if let Some(u) = self.pending_url.clone() {
            if contains_token(&u) {
                self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                return Some(u);
            }
        }
        // 自有服务：等它的 token（stdout 回调捕获后会补发 Ready）
        if self.service.owns_service_process() {
            if let Some(u) = self.service.auth_url() {
                self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                self.pending_url = Some(u.clone());
                return Some(u);
            }
        } else {
            // 接管的外部实例拿不到 token，只能走 WebView2 里已持久化的登录 Cookie
            self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
            return Some(self.service.ready_url());
        }
        // 预算耗尽：退回普通地址，并说明后续如何恢复
        if self.auth_wait_ticks == 0 {
            self.logger.warn(
                "等待 dsh 的 token 地址超时（约 5 秒），按普通地址打开；\
                 若页面要求认证，请点托盘「刷新界面」或「停止服务」后重新启动。",
            );
            return Some(self.service.ready_url());
        }
        self.auth_wait_ticks -= 1;
        None
    }

    /// 打开（或激活）内嵌 Harness 窗口。
    ///
    /// 返回 `true` 表示「已经打开/不需要再打开」（调用方可清除待办），
    /// `false` 表示本帧条件不满足、**应当继续重试**。
    fn open_harness(&mut self, window_target: &tao::event_loop::EventLoopWindowTarget<()>) -> bool {
        // ---- 窗口已存在：**显示并聚焦**（用户关到托盘后唯一的恢复路径）----
        //
        // 🔴 P0 修复：旧实现的调用方（`pump_pending_open` 与唤起待办）在
        // `harness.is_some()` 时直接 return，于是本分支**永远不可达** ——
        // 窗口一旦被隐藏（默认 `tray_on_close = true`：点关闭按钮或按 Esc）就再也
        // 回不来：托盘「打开界面」与双击图标的唤起都毫无反应（日志却写着
        // 「已激活内嵌窗口」/「正在重新打开内嵌窗口」）。
        //
        // 地址尚未就绪时**同样要显示**：用户显式要求打开界面，哪怕页面暂时是
        // 等待状态，也比"点了没反应"可诊断得多；带 token 的地址到达后会就地导航。
        if self.harness.is_some() {
            let url = self.auth_url_settled();
            if let Some(h) = &self.harness {
                if let Some(url) = &url {
                    h.navigate(url);
                }
                h.show();
            }
            match url {
                Some(u) => self.logger.info(&format!("内嵌界面已显示并导航到：{u}")),
                None => self
                    .logger
                    .info("内嵌界面已显示（地址尚未就绪，将在服务就绪后自动导航）"),
            }
            // 重新做一次认证自检：这一次可能是带 token 的地址。
            // （token 会把 401 变成"已登录并种下 cookie"。）
            self.schedule_auth_check();
            return true;
        }

        // **先判服务是否可达，再消耗等待预算**：
        // 冷启动时 dsh 尚未监听端口（实测 30 秒以上），此刻消耗预算毫无意义，
        // 只会让 5 秒后被"放弃等待"的普通地址开出一个 401 页面。
        //
        // 探测本身是阻塞调用，因此走**按时间限流**的 [`App::port_reachable`]；
        // 「尚未就绪」也只在首次记录一次（此前每 100 ms 一行日志，120 秒会刷出
        // 上千行，且每行都要打开一次日志文件）。
        if !self.port_reachable() {
            if !self.not_ready_logged {
                self.not_ready_logged = true;
                self.logger.info(&format!(
                    "服务尚未就绪（端口 {} 未监听），界面稍后自动打开。",
                    self.config.port
                ));
            }
            return false;
        }
        self.not_ready_logged = false;

        // 地址优先级：带 token 的地址 > 普通地址（仅接管外部实例时才允许）。
        // dsh web 对无 token 访问返回 401，所以自有服务必须等到 token。
        let Some(url) = self.auth_url_settled() else {
            // token 还没到：本帧不开窗，由主循环每帧重试（见 `want_open`）
            return false;
        };

        match dsh_ui::HarnessWindow::new(window_target, &mut self.web_context, &url) {
            Ok(w) => {
                self.logger.info(&format!("已打开内嵌界面：{url}"));
                self.harness = Some(w);
                // 打开后立刻安排认证自检：接管外部实例时地址可能是不带 token 的普通地址，
                // 若 WebView2 里没有有效 cookie，页面会是 401 —— 必须发现并自愈，
                // 而不是把"认证失败页"当成正常界面留给用户。
                self.schedule_auth_check();
                true
            }
            Err(e) => {
                // WebView2 不可用：回退 Edge 精简窗口（可用性底线，不可省略）
                self.logger
                    .error(&format!("内嵌窗口创建失败（{e}），改用 Edge 精简窗口。"));
                crate::browser::open_edge_fallback(&url, &self.logger);
                // 已经用外部窗口兜底，不再重试内嵌窗口（否则会不断弹 Edge）
                true
            }
        }
    }

    /// 每帧驱动「待打开界面」的待办：让 [`AUTH_URL_WAIT_TICKS`] 成为真正的时间预算。
    ///
    /// 历史缺陷（本轮修正）：等待预算只被事件消耗，而事件之间无人驱动重试，
    /// 于是没有 token 时窗口**永远不会出现**。
    fn pump_pending_open(&mut self, window_target: &tao::event_loop::EventLoopWindowTarget<()>) {
        if !self.want_open {
            return;
        }
        // 注意：**不能**在 `harness.is_some()` 时短路返回。
        // 这里正是"隐藏到托盘的窗口能否被重新显示"的分水岭（见 open_harness 的说明）：
        // 窗口已存在时 open_harness 会显示并聚焦它，早退会把那条路径整条掐掉。
        if self.open_harness(window_target) {
            self.want_open = false;
        }
    }

    /// 把**已存在**的窗口就地导航到最新地址（**不改变可见性**）。
    ///
    /// 用于服务就绪/重启后的补导航：窗口可能被用户刻意隐藏到托盘，
    /// 此时只更新页面地址，绝不把它强行弹到前台（那是「打开界面」的语义）。
    fn navigate_existing_harness(&mut self) {
        if self.harness.is_none() {
            return;
        }
        let Some(url) = self.auth_url_settled() else {
            return;
        };
        if let Some(h) = &self.harness {
            h.navigate(&url);
        }
        self.logger.info(&format!("内嵌界面已就地导航到：{url}"));
        self.schedule_auth_check();
    }

    /// 刷新内嵌窗口。
    fn refresh_harness(&mut self) {
        match &self.harness {
            Some(h) => {
                h.reload();
                self.logger.info("已刷新内嵌窗口。");
            }
            None => self.logger.warn("内嵌窗口未打开，请先点“打开界面”。"),
        }
    }

    /// 打开（或激活）设置窗口。
    fn open_settings(&mut self, window_target: &tao::event_loop::EventLoopWindowTarget<()>) {
        if let Some(s) = &self.settings {
            s.show();
            return;
        }
        let init = self.settings_init();
        match dsh_ui::SettingsWindow::new(
            window_target,
            &mut self.web_context,
            &init,
            self.settings_tx.clone(),
        ) {
            Ok(w) => {
                self.logger.info("已打开设置窗口。");
                self.settings = Some(w);
            }
            Err(e) => self.logger.error(&format!("设置窗口创建失败：{e}")),
        }
    }

    /// 打开（或激活）新手指引窗口。
    ///
    /// 正常路径渲染 `ui/guide.html`；WebView2 不可用时退回 MessageBox 纯文本
    /// （由同一份 HTML 提取，因此文案仍只有一处来源）。
    fn open_guide(&mut self, window_target: &tao::event_loop::EventLoopWindowTarget<()>) {
        if let Some(g) = &self.guide {
            g.show();
            return;
        }
        match dsh_ui::GuideWindow::new(window_target, &mut self.web_context) {
            Ok(w) => {
                self.logger.info("已打开新手指引窗口。");
                self.guide = Some(w);
            }
            Err(e) => {
                self.logger
                    .error(&format!("指引窗口创建失败（{e}），改用对话框显示。"));
                crate::dialog::show_guide_fallback();
            }
        }
    }

    /// 执行退出收尾，返回 `false` 表示用户取消（应继续留在事件循环）。
    ///
    /// 托盘「退出」与 `--quit`（命名事件）共用本函数，保证两条路径的收尾语义完全一致。
    ///
    /// ## 是否询问用户，取决于 `service_lifecycle`（v5.0.0 定稿轮起）
    ///
    /// | 模式 | 退出行为 |
    /// |---|---|
    /// | `independent`（默认，设置页勾选「服务独立于启动器」） | **不询问**：直接退出，服务继续跑。 |
    /// | `tied`（取消勾选） | **每次询问**：此时退出会连带停掉服务，必须让用户确认。 |
    ///
    /// 这样设计的理由：**弹窗的价值在于提醒一次「意外的或不可逆的后果」**。
    /// 独立模式下退出启动器对正在进行的会话无害，没什么可提醒的——每次都问只会让
    /// 用户烦（而且会让人误以为"这个选项没生效"）；而 tied 模式下退出**真的会停服务**，
    /// 那次确认是必要的。
    fn begin_exit(&mut self) -> bool {
        let own = self.service.is_own_service_running();
        let adopted = self.service.is_adopted_service();
        let tied = self.service.stops_service_on_exit();

        // ---- independent：不询问，直接保留服务退出 ----
        if !tied {
            let kept = self.service.keep_children_on_exit();
            if own || adopted {
                if kept {
                    self.logger.info(
                        "服务独立于启动器：直接退出，dsh 继续在后台运行（下次打开会自动接管，会话不中断）。\
                         如需停止服务，请用托盘「停止服务」或设置页。",
                    );
                } else {
                    self.logger.warn("无法保留后台服务，退出将同时停止服务。");
                }
            } else {
                let _ = kept;
                self.logger.info("退出（当前没有正在运行的服务）。");
            }
            return true;
        }

        // ---- tied：退出会停服务，必须询问 ----
        if !(own || adopted) {
            // 没有正在运行的服务可问：按策略收尾即可
            let _ = self.service.stop();
            self.logger
                .info("退出（tied 模式，当前没有正在运行的服务）。");
            return true;
        }
        match crate::dialog::confirm_service_stop(own, adopted, true) {
            crate::dialog::ExitChoice::StopServiceAndExit => {
                if let Err(e) = self.service.stop() {
                    self.logger.error(&format!("停止服务失败：{e}"));
                } else {
                    self.logger.info("已按用户选择停止服务并退出。");
                }
            }
            crate::dialog::ExitChoice::KeepServiceAndExit => {
                if self.service.keep_children_on_exit() {
                    self.logger
                        .info("已保留后台服务（下次启动将自动接管，会话不中断）。");
                } else {
                    self.logger.warn("无法保留后台服务，退出将同时停止服务。");
                }
            }
            crate::dialog::ExitChoice::Cancel => {
                // 用户取消退出：保持在事件循环中
                return false;
            }
        }
        true
    }

    /// 后台启动服务，结果回传主循环。
    ///
    /// **为什么必须异步**：`ServiceHandle::start()` 里串着一批阻塞调用——
    /// `reconcile()`（进程存活 / 创建时间 / 身份判定，含多次 toolhelp 快照）、
    /// `is_port_listening()`（最长 300 ms 的 connect）、`clean_stale_dsh_locks()`
    /// （遍历 `~/.dsh` 下的目录）、以及 spawn 本身。整条路径实测可达数百毫秒到一秒以上，
    /// 放在 UI 线程上就表现为「点“启动服务”后界面卡一下」，与
    /// `ui-thread-never-blocks` 的修复目标直接冲突。
    ///
    /// 状态与就绪事件由服务层自己通过事件通道上报，本函数只负责把「启动」这个
    /// 阻塞动作搬离 UI 线程，并把失败结果回写设置页状态栏。
    fn spawn_start(&self) {
        let service = self.service.clone();
        let tx = self.task_tx.clone();
        let logger = self.logger.clone();
        let spawned = std::thread::Builder::new()
            .name("dsh-start".into())
            // 启动路径只做 Win32 调用与进程创建，不需要默认 8 MB 栈
            .stack_size(512 * 1024)
            .spawn(move || {
                let r = service.start().map_err(|e| e.to_string());
                if let Err(e) = &r {
                    logger.error(&format!("启动服务失败：{e}"));
                }
                let _ = tx.send(BackgroundTask::Start(r));
            });
        if let Err(e) = spawned {
            // 起不了线程时退化为同步启动（宁可卡一下，也不能不启动）
            self.logger
                .warn(&format!("无法创建启动线程（{e}），改为同步启动服务。"));
            match self.service.start() {
                Ok(()) => self.set_settings_status("正在启动服务…", false),
                Err(e) => self.set_settings_status(&format!("启动失败：{e}"), true),
            }
        }
    }

    /// 后台异步停止服务，结果回传主循环。
    ///
    /// **为什么必须在 worker 线程**：`ServiceHandle::stop()` 会递归终止整个进程树
    /// 并 `wait`（可能数百毫秒到数秒）。放在 UI 线程上会把事件循环冻住，
    /// 用户看到的是"点了停止服务之后界面卡住"。
    fn spawn_stop(&self) {
        let tx = self.task_tx.clone();
        let logger = self.logger.clone();
        self.service.stop_async(move |r| {
            if let Err(e) = &r {
                logger.error(&format!("停止服务失败：{e}"));
            }
            let _ = tx.send(BackgroundTask::Stop(r));
        });
    }

    /// 后台串行执行「停服 → 应用新配置 → 启动」，结果回传主循环。
    ///
    /// **为什么放到 worker 线程**：这三步都必须在 UI 线程之外完成——
    /// `stop()` 会递归终止进程树并 `wait`（可能数百毫秒到数秒），
    /// 在 UI 线程上执行会把界面冻住（历史上点「保存」会有明显卡顿）。
    /// 串行是必须的：`start()` 必须看到新的配置、且在旧进程真正退出之后。
    fn spawn_restart(&self, config: Settings) {
        let service = self.service.clone();
        let tx = self.task_tx.clone();
        let logger = self.logger.clone();
        let events = self.tx.clone();
        let fallback = config.clone();
        let spawned = std::thread::Builder::new()
            .name("dsh-restart".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let _ = service.stop();
                service.set_event_sender(events);
                service.set_config(config);
                let r = service.start().map_err(|e| e.to_string());
                if let Err(e) = &r {
                    logger.error(&format!("重启服务失败：{e}"));
                }
                let _ = tx.send(BackgroundTask::Restart(r));
            });
        if spawned.is_err() {
            // 起不了线程时退化为同步执行（宁可卡一下，也不能不重启）
            self.logger
                .warn("无法创建重启线程，改为同步重启服务（界面可能短暂卡顿）。");
            let _ = self.service.stop();
            self.service.set_event_sender(self.tx.clone());
            self.service.set_config(fallback);
            match self.service.start() {
                Ok(()) => self.set_settings_status("设置已保存，服务已按新配置重启。", false),
                Err(e) => self.set_settings_status(&format!("重启失败：{e}"), true),
            }
        }
    }

    /// 处理设置页发来的命令。
    fn handle_settings_command(&mut self, cmd: dsh_ui::SettingsCommand) {
        // 每条 IPC 命令都留痕，便于确认页面→Rust 通道是否真的打通
        self.logger.info(&format!("设置页命令: {cmd:?}"));
        match cmd {
            dsh_ui::SettingsCommand::Save {
                port,
                work_dir,
                node_path,
                tray_on_close,
                independent_service,
            } => {
                let parsed: u16 = match port.trim().parse() {
                    Ok(p) if p >= 1 => p,
                    _ => {
                        self.set_settings_status("端口必须是 1–65535 之间的数字", true);
                        return;
                    }
                };
                // 先在**副本**上改动：保存失败时内存配置必须保持原样，
                // 否则会出现「界面显示已改、磁盘仍是旧值、重启后又变回去」的错乱。
                let mut candidate = self.config.clone();
                candidate.port = parsed;
                candidate.work_dir = work_dir.trim().to_string();
                candidate.node_path = node_path.trim().to_string();
                // 缺字段 = 保留用户当前值（不是回退默认值）
                if let Some(v) = tray_on_close {
                    candidate.tray_on_close = v;
                }
                if let Some(v) = independent_service {
                    candidate.service_lifecycle = if v {
                        dsh_core::ServiceLifecycle::Independent
                    } else {
                        dsh_core::ServiceLifecycle::Tied
                    };
                }

                let port_changed = candidate.port != self.config.port;
                let work_changed = candidate.work_dir != self.config.work_dir;
                let node_changed = candidate.node_path != self.config.node_path;
                let lifecycle_changed =
                    candidate.service_lifecycle != self.config.service_lifecycle;
                let running = self.service.is_own_service_running();

                // 端口变更时先做一次**快速**占用预检，避免停了旧服务却起不来。
                //
                // 用 `is_port_free_to_bind`（一次 bind 系统调用，**不阻塞**）而不是
                // `is_port_listening`（阻塞 connect，最长 300 ms，且此刻在 UI 线程上）。
                // 语义差异如实标注：bind 失败也可能是 TIME_WAIT 残留导致的短暂拒绝，
                // 因此这里只是**建议性**预检——真正的权威判定在 `ServiceHandle::start()`
                // 的分支 3（那里会对占用者做身份校验并给出可操作的错误）。
                if port_changed && running && !dsh_core::is_port_free_to_bind(candidate.port) {
                    self.set_settings_status(
                        &format!(
                            "新端口 {} 当前不可用（被占用或处于 TIME_WAIT），无法切换；请换一个端口或稍后重试。",
                            candidate.port
                        ),
                        true,
                    );
                    return;
                }

                if let Err(e) = candidate.save() {
                    self.set_settings_status(&format!("保存失败：{e}（原有设置未改动）"), true);
                    return;
                }
                // 落盘成功后才提交到内存
                self.config = candidate;
                self.logger
                    .info(&format!("设置已保存（端口 {}）。", self.config.port));

                // ⚠️ 生命周期变更**不重启服务**。
                //
                // Job 归属是进程创建时决定的，无法事后改变；而"重启服务"会打断正在
                // 进行的会话。用户只是取消勾选「服务独立于启动器」，绝不该因此掉线。
                // 因此：策略只记录，等**下一次服务自然重启**时生效，并在状态栏说明。
                let only_lifecycle =
                    lifecycle_changed && !(port_changed || work_changed || node_changed);
                if only_lifecycle {
                    self.service.set_event_sender(self.tx.clone());
                    self.service.set_config(self.config.clone());
                    let note = if self.config.service_lifecycle == dsh_core::ServiceLifecycle::Tied
                    {
                        "设置已保存：服务生命周期策略已改为「服务随启动器退出而停止」，\
                         将在下次服务重启后生效（本次不打断会话）。"
                    } else {
                        "设置已保存：服务已改为「独立于启动器」，将在下次服务重启后生效\
                         （本次不打断会话）。"
                    };
                    self.set_settings_status(note, false);
                    self.logger.info(
                        "生命周期策略已保存，未重启服务（Job 归属无法事后改变；下次服务重启后生效）。",
                    );
                    return;
                }

                let need_restart = running && (port_changed || work_changed || node_changed);
                if need_restart {
                    self.logger
                        .info("配置已变更（端口/工作目录/node 路径），正在后台重启服务…");
                    // 重启后必须把界面重新导航到新地址，否则窗口仍停在旧服务的页面上
                    self.pending_url = None;
                    self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                    self.set_settings_status("设置已保存，服务正在以新配置重启…", false);
                    self.spawn_restart(self.config.clone());
                } else {
                    // 配置只影响下次启动的项（tray_on_close 等）也要同步给服务层
                    self.service.set_event_sender(self.tx.clone());
                    self.service.set_config(self.config.clone());
                    self.set_settings_status("设置已保存。", false);
                }
            }
            dsh_ui::SettingsCommand::ResetConfig => {
                if !crate::dialog::confirm_dangerous(
                    "即将恢复默认设置。\n\n\
                     · 配置文件 settings.toml 会被**删除**（端口、工作目录、Node 路径、托盘偏好全部回到默认值）；\n\
                     · 如果服务正在运行且新配置的端口/工作目录不同，服务会用默认配置重启；\n\
                     · 正在进行的会话可能因此中断。\n\n\
                     确定要继续吗？（默认「否」）",
                    &format!("{} · 恢复默认设置", dsh_core::APP_NAME),
                ) {
                    self.set_settings_status("已取消恢复默认设置。", false);
                    return;
                }
                if let Err(e) = Settings::reset_on_disk() {
                    self.set_settings_status(&format!("删除配置文件失败：{e}"), true);
                    return;
                }
                let old = self.config.clone();
                self.config = Settings::default();
                self.logger.warn(&format!(
                    "已恢复默认设置（删除 {}）。端口 {} → {}。",
                    Settings::file_path().display(),
                    old.port,
                    self.config.port
                ));
                let running = self.service.is_own_service_running();
                let changed = old.port != self.config.port
                    || old.work_dir != self.config.work_dir
                    || old.node_path != self.config.node_path;
                // 表单回填为默认值，让用户看到"当前生效的是什么"
                if let Some(s) = &self.settings {
                    s.apply_init(&self.settings_init());
                }
                self.pending_url = None;
                self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                if running && changed {
                    self.set_settings_status("已恢复默认设置，服务正在以默认配置重启…", false);
                    self.spawn_restart(self.config.clone());
                } else {
                    self.service.set_event_sender(self.tx.clone());
                    self.service.set_config(self.config.clone());
                    self.set_settings_status("已恢复默认设置。", false);
                }
            }
            dsh_ui::SettingsCommand::Start => {
                self.service.set_event_sender(self.tx.clone());
                self.service.set_config(self.config.clone());
                self.pending_url = None;
                self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                self.set_settings_status("正在启动服务…", false);
                // 阻塞的启动决策（对账 / 端口探测 / 锁清理 / spawn）放到 worker
                self.spawn_start();
            }
            dsh_ui::SettingsCommand::Stop => {
                self.pending_url = None;
                self.set_settings_status("正在停止服务…", false);
                self.spawn_stop();
            }
            dsh_ui::SettingsCommand::OpenLogDir => {
                crate::browser::open_directory(&dsh_core::log_dir());
            }
            dsh_ui::SettingsCommand::CleanArchived => {
                // 重入保护：探测与清理都是长阻塞操作，连点会并发起多个线程
                if self.cleaning {
                    self.set_settings_status("清理正在进行中，请稍候…", false);
                    return;
                }
                if self.cleanup_prechecking {
                    self.set_settings_status("正在检查是否有活跃会话，请稍候…", false);
                    return;
                }
                let running =
                    self.service.is_own_service_running() || self.service.is_adopted_service();

                // **活跃会话守卫**：清理必须先停 dsh，若此刻有会话在跑就会把它打断。
                // 因此先探测，并把探测结果交给确认对话框——同时提升确认强度
                // （默认焦点落在「否」，避免误按回车打断正在跑的任务）。
                //
                // 注意：**不能在此处硬性阻止**。正在执行这次点击的往往就是用户自己的
                // 会话，硬阻止会让「清理」永远不可用；因此选择"明确告知 + 提高确认门槛"。
                //
                // **探测放在 worker 线程**：它包含一次 `Get-CimInstance`（实测约 1.3 秒，
                // 上限 5 秒）与一次有界目录遍历。旧实现直接在 UI 线程上跑，
                // 表现为"点了清理按钮后界面卡住一秒多"——与 `ui-thread-never-blocks`
                // 的修复目标直接冲突。探测结果经 `BackgroundTask::CleanupPrecheck` 回到
                // 主循环，确认框仍在主线程弹（模态对话框本来就要求 UI 线程）。
                self.cleanup_prechecking = true;
                self.set_settings_status("正在检查是否有活跃会话…", false);
                let logger = self.logger.clone();
                let tx = self.task_tx.clone();
                let spawned = std::thread::Builder::new()
                    .name("dsh-cleanup-check".into())
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        let active = dsh_core::probe_active_sessions();
                        match &active {
                            Some(a) if a.any_activity() => logger.warn(&format!(
                                "清理归档会话前探测到活跃迹象：{}（{}）",
                                a.summary(),
                                if a.recent_session_hint.is_empty() {
                                    String::new()
                                } else {
                                    format!("涉及 {}", a.recent_session_hint.join(", "))
                                }
                            )),
                            Some(_) => logger.info("清理归档会话前探测：未发现活跃会话。"),
                            None => logger.warn("清理归档会话前无法探测活跃会话（按有活动处理）。"),
                        }
                        let count = dsh_core::archived_session_count();
                        let _ = tx.send(BackgroundTask::CleanupPrecheck {
                            running,
                            active,
                            count,
                        });
                    });
                if spawned.is_err() {
                    self.cleanup_prechecking = false;
                    self.set_settings_status("无法创建检查线程，清理未执行（可稍后重试）。", true);
                }
            }
        }
    }

    /// 「清理归档会话」的第二步：拿到探测结果后弹确认框并启动清理（**主线程**）。
    ///
    /// 拆出本函数是为了让 `handle_settings_command` 保持"纯命令分发"，
    /// 并让"探测在 worker、决策在主线程"这条分工一眼可见。
    fn begin_cleanup(
        &mut self,
        running: bool,
        active: Option<dsh_core::ActiveSessions>,
        count: usize,
    ) {
        // **二次确认**：把"停服务 / 会话中断 / 数据不可恢复"讲清，
        // 有活跃会话时文案升级并把默认焦点放到「否」。
        if !crate::dialog::confirm_dangerous(
            &Self::cleanup_confirm_text(running, active.as_ref(), count),
            &format!("{} · 清理归档会话", dsh_core::APP_NAME),
        ) {
            self.logger.info("用户取消归档清理。");
            self.set_settings_status("已取消清理。", false);
            return;
        }

        // 记住"清理前服务是否在跑"：清理完成后据此**自动把服务拉回来**，
        // 否则用户会停在"服务被停掉、界面打不开"的状态（实测反馈）。
        self.cleanup_restore_service = running;
        self.set_settings_status("正在清理归档会话…", false);

        // 清理可能对每个会话重试 4 次 × 300ms（被占用时），放主线程会卡死界面。
        // **停服务也一并放进 worker**（终止进程树同样是阻塞操作）。
        self.cleaning = true;
        let logger = self.logger.clone();
        let tx = self.task_tx.clone();
        let service = self.service.clone();
        let spawned = std::thread::Builder::new()
            .name("dsh-cleanup".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                if running {
                    if let Err(e) = service.stop() {
                        let msg = format!("清理前停止服务失败：{e}；请手动停止后重试。");
                        logger.error(&msg);
                        let _ = tx.send(BackgroundTask::Cleanup(Err(msg)));
                        return;
                    }
                    logger.info("已先停止 dsh 服务，开始清理归档会话（完成后会自动重新启动服务）…");
                }
                // 归档数由前置探测一并带回，这里不重复读一次 workspace.json
                if count == 0 {
                    let _ = tx.send(BackgroundTask::Cleanup(Ok((
                        0,
                        "当前没有归档会话".to_string(),
                    ))));
                    return;
                }
                logger.info(&format!("开始清理 {count} 个归档会话…"));
                let out = dsh_core::delete_archived_sessions().map_err(|e| e.to_string());
                match &out {
                    Ok((cleaned, detail)) => {
                        logger.info(&format!("归档清理完成：{detail}（清理 {cleaned} 个）"))
                    }
                    Err(e) => logger.error(&format!("归档清理失败：{e}")),
                }
                let _ = tx.send(BackgroundTask::Cleanup(out));
            });
        if spawned.is_err() {
            self.cleaning = false;
            self.cleanup_restore_service = false;
            self.set_settings_status("无法创建清理线程，清理未执行。", true);
        }
    }

    /// 设置页表单的回填数据（打开窗口与「恢复默认设置」后共用）。
    fn settings_init(&self) -> dsh_ui::SettingsInit {
        dsh_ui::SettingsInit {
            port: self.config.port,
            work_dir: self.config.work_dir.clone(),
            node_path: self.config.node_path.clone(),
            tray_on_close: self.config.tray_on_close,
            independent_service: self.config.service_lifecycle
                == dsh_core::ServiceLifecycle::Independent,
            log_path: dsh_core::log_dir()
                .join("launcher.log")
                .display()
                .to_string(),
        }
    }

    /// 把状态回写到设置页（窗口不存在时静默忽略）。
    fn set_settings_status(&self, text: &str, is_error: bool) {
        if let Some(s) = &self.settings {
            s.set_status(text, is_error);
        }
    }

    /// 「清理归档会话」的二次确认文案。
    ///
    /// 必须让用户在点「是」之前知道：**会停服务**、**会话会断**、**删除不可恢复**；
    /// 并说明服务之后会被自动拉回（v5.0.0 定稿轮起的行为）。
    ///
    /// `active` 为活跃会话探测结果：**检测到会话在跑时，文案升级为警告并明确点名**，
    /// 因为"正在跑的任务被打断"比"历史数据被删"更让人措手不及。
    ///
    /// `count` 由前置探测（worker 线程）一并算出：本函数在 UI 线程上执行，
    /// 不应再做任何文件 IO。
    fn cleanup_confirm_text(
        running: bool,
        active: Option<&dsh_core::ActiveSessions>,
        count: usize,
    ) -> String {
        let service_note = if running {
            "· 会先停止 dsh 服务——**正在进行的会话与任务会立即中断**；\n\
             · 清理完成后启动器会自动把服务重新启动。"
        } else {
            "· 当前服务未运行，因此不会中断任何会话。"
        };

        let activity_block = match active {
            // 有会话在跑：这是最需要预警的情形
            Some(a) if a.is_running() => {
                let hint = if a.recent_session_hint.is_empty() {
                    String::new()
                } else {
                    format!("\n  涉及：{}", a.recent_session_hint.join("、"))
                };
                format!(
                    "\n\n⚠ **{}** —— 现在继续会打断它。{hint}\n\
                     请先等会话结束，或确认你不在意中断。",
                    a.summary()
                )
            }
            // 只有"刚写过文件"的软信号
            Some(a) if a.any_activity() => {
                format!("\n\n注意：{}（可能是刚结束的会话）。", a.summary())
            }
            _ => String::new(),
        };

        format!(
            "即将清理全部归档会话（共 {count} 个）。\n\n\
             {service_note}\n\
             · 归档的历史数据将被**永久删除且不可恢复**。\
             {activity_block}\n\n\
             确定要继续吗？（默认「否」）"
        )
    }

    /// 清理结束后按需把服务拉回来（用户在清理前服务是运行中的）。
    ///
    /// 为什么需要：清理按设计必须停服务，但**旧实现没有把服务拉回**，用户会停在
    /// 「服务被停掉、内嵌界面打不开」的状态里，只能自己去点托盘「启动服务」
    /// （实测反馈）。这里在清理完成（或无事可做）后自动恢复。
    fn restore_service_after_cleanup(&mut self) {
        if !self.cleanup_restore_service {
            return;
        }
        self.cleanup_restore_service = false;
        if self.service.is_own_service_running() || self.service.is_adopted_service() {
            return; // 已经在跑，无需恢复
        }
        self.logger.info("归档清理结束，正在把服务重新启动…");
        self.service.set_event_sender(self.tx.clone());
        self.service.set_config(self.config.clone());
        self.pending_url = None;
        self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
        // 与托盘/设置页同一条路径：启动是阻塞动作，必须离开 UI 线程
        self.spawn_start();
    }

    /// 内嵌 Harness 窗口的快捷键处理（`ui/guide.html` 向用户承诺过这三个）。
    ///
    /// - `Ctrl` 按下状态来自 `WindowEvent::ModifiersChanged`（tao 0.35 提供），
    ///   **不再**靠自己数 KeyDown/KeyUp：键盘事件只送给**获得焦点**的窗口，
    ///   用户按住 Ctrl 切到别的程序再松开时，我们收不到 KeyUp，
    ///   于是「松开了但状态仍是按下」，之后随便敲一个 `R` 就会刷新页面、
    ///   把正在编辑的内容冲掉（实测可复现的输入事故）；
    /// - 失焦（`Focused(false)`）时同样清零，双保险；
    /// - `F5` / `Ctrl+R`：刷新内嵌窗口；
    /// - `Esc`：按 `tray_on_close` 偏好隐藏窗口（与标题栏关闭按钮同一语义）。
    ///
    /// 只对**内嵌 Harness 窗口**生效；设置页/指引窗口的按键不拦截，避免影响表单输入。
    fn handle_harness_key(&mut self, window_id: WindowId, key: &tao::event::KeyEvent) {
        if self.harness.as_ref().map(|h| h.window_id()) != Some(window_id) {
            return;
        }
        let pressed = key.state == ElementState::Pressed;
        let fresh = pressed && !key.repeat;
        match key.physical_key {
            // 兜底：仍然跟踪 Ctrl 的按下/抬起（ModifiersChanged 是主来源）
            KeyCode::ControlLeft | KeyCode::ControlRight => {
                self.ctrl_down = pressed;
            }
            KeyCode::F5 if fresh => {
                self.refresh_harness();
            }
            KeyCode::KeyR if fresh && self.ctrl_down => {
                self.refresh_harness();
            }
            KeyCode::Escape if fresh => {
                self.request_close_window(window_id);
            }
            _ => {}
        }
    }

    /// 周期采样页面主题并应用到 DWM 标题栏（**异步，不阻塞事件循环**）。
    ///
    /// 历史缺陷：主题采样没接线，`sample_and_apply_theme` 恒返回 `None`，
    /// 窗口标题栏永远停在 `set_dark_mode(hwnd, false)`。这里用异步回调把它接上：
    /// 每 [`THEME_SAMPLE_EVERY_TICKS`] 帧（约 3 秒）提交一次，结果写入共享槽位，
    /// **由主循环在后续帧非阻塞取出**——绝不在 UI 线程 `recv_timeout`。
    fn maybe_sample_theme(&mut self) {
        // 1) 取上一轮结果（**取出并清空**）
        let got = self
            .theme_slot
            .as_ref()
            .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
        if let Some(rgb) = got {
            self.theme_sample_pending = false;
            match rgb {
                Some(rgb) => {
                    // **只在颜色真的变化时打日志**：页面背景色是稳定值，
                    // 每次采样都打会瞬间刷爆日志（实测 400 行/秒），淹没真正的诊断。
                    if self.theme_last_color != Some(rgb) {
                        self.theme_last_color = Some(rgb);
                        self.logger.info(&format!(
                            "标题栏已跟随页面主题（rgb {},{},{}）",
                            rgb.0, rgb.1, rgb.2
                        ));
                    }
                }
                // 页面尚未给出可用背景色是常见且无害的中间状态（页面还在渲染）
                None if !self.theme_warned_no_color => {
                    self.theme_warned_no_color = true;
                    self.logger.info(
                        "首次主题采样未取到页面背景色（页面可能尚未渲染完），后续会自动重试。",
                    );
                }
                None => {}
            }
        }

        // 2) **按时间限流**（而不是按帧数）。
        //
        // ⚠️ 这里踩过两次坑，都值得记住：
        //   a) 只读不清槽位 → 同一结果被逐帧重复打印（4557 行 / 仅 35 种内容）；
        //   b) 改成取出并清空后，`pending` 几乎总是 false，于是**几乎每帧提交一次采样**，
        //      回调每帧回一个结果 → 每帧打一行（实测 400 行/秒，日志 50 秒涨 1.6 MB）。
        //   根因是限流挂在了"有没有结果"上，而应当挂在"距上次提交过了多久"上。
        if self.theme_sample_pending {
            // 回调始终不来时解除 pending，避免永久停采
            if self
                .theme_sample_at
                .is_some_and(|t| t.elapsed() > THEME_SAMPLE_CALLBACK_TIMEOUT)
            {
                self.theme_sample_pending = false;
                self.theme_sample_at = None;
            } else {
                return;
            }
        }
        // 距上次**提交**不足最小间隔就什么都不做（无论期间跑了多少帧）
        if let Some(t) = self.theme_sample_at {
            if t.elapsed() < THEME_SAMPLE_INTERVAL {
                return;
            }
        }
        let Some(h) = &self.harness else { return };
        if !h.is_visible() {
            return; // 窗口隐藏（最小化到托盘）时不采样，省一次 JS 往返
        }

        let slot: ThemeSlot = Arc::new(Mutex::new(None));
        let slot_for_cb = Arc::clone(&slot);
        let submitted = h.begin_theme_sample(move |rgb| {
            if let Ok(mut g) = slot_for_cb.lock() {
                *g = Some(rgb);
            }
        });
        // **无论提交成功与否都要记时**：限流的唯一依据就是 `theme_sample_at`，
        // 若只在成功时更新，一旦 `evaluate_script_with_callback` 失败（例如 WebView2
        // 正在销毁但句柄还在），`pending` 与 `sample_at` 都保持空，
        // 于是**每帧都重试一次**、永久静默空转（实测事件循环可跑数百 fps）。
        self.theme_sample_at = Some(std::time::Instant::now());
        if submitted {
            self.theme_sample_pending = true;
            self.theme_slot = Some(slot);
        }
    }

    /// 处理窗口的「关闭」请求（`CloseRequested` 或 `Esc`）。
    ///
    /// **行为由 `tray_on_close` 决定**（历史缺陷：该配置项完全没有消费方，
    /// 界面复选框、`settings.toml`、README 都承诺「关闭窗口时最小化到托盘」，
    /// 而实现只是把窗口对象丢掉）：
    ///
    /// - `tray_on_close = true`：**隐藏**窗口（保留 WebView 与页面状态，托盘可秒开）；
    /// - `tray_on_close = false`：真的关闭该窗口并释放资源。
    ///
    /// 设置/指引窗口不受该偏好影响（它们没有"常驻"语义），一律真正关闭。
    fn request_close_window(&mut self, id: WindowId) {
        let is_harness = self.harness.as_ref().map(|h| h.window_id()) == Some(id);
        if is_harness && self.config.tray_on_close {
            if let Some(h) = &self.harness {
                h.hide();
            }
            self.logger
                .info("界面已最小化到托盘（可在托盘菜单「打开界面」恢复）。");
            return;
        }
        self.on_window_closed(id);
    }

    /// 窗口关闭：释放对应窗口对象，允许再次打开。
    fn on_window_closed(&mut self, id: WindowId) {
        if self.harness.as_ref().map(|h| h.window_id()) == Some(id) {
            self.harness = None;
            // 新窗口会重新提交采样请求；**把整套主题状态清干净**，
            // 否则重开窗口时会把已销毁 WebView 留下的陈旧槽位值当作新结果，
            // 并因为 `theme_last_color` 相同而永远不再打日志（看起来像"没生效"）。
            self.theme_sample_pending = false;
            self.theme_slot = None;
            self.theme_sample_at = None;
            self.theme_last_color = None;
            self.theme_warned_no_color = false;
            // 认证自检状态同理必须清空：否则重开窗口时会读到已销毁 WebView 的陈旧结论，
            // 把新窗口误判成「需要认证」或「已认证」。
            self.auth_slot = None;
            self.auth_check_pending = false;
            self.auth_check_at = None;
            self.auth_check_attempts = 0;
            self.auth_ok_logged = false;
            self.auth_recovery_offered = false;
            self.logger.info("内嵌窗口已关闭。");
        }
        if self.settings.as_ref().map(|s| s.window_id()) == Some(id) {
            self.settings = None;
            self.logger.info("设置窗口已关闭。");
        }
        if self.guide.as_ref().map(|g| g.window_id()) == Some(id) {
            self.guide = None;
            self.logger.info("指引窗口已关闭。");
        }
    }

    pub fn run(mut self, opts: StartupOptions, boot: BootTimer) -> anyhow::Result<()> {
        // 设置事件发送通道
        self.service.set_event_sender(self.tx.clone());

        // EventLoop 必须是 run() 的局部变量：若存在 App 字段中，闭包捕获整体 self 时
        // 会与 self.event_loop.run(..) 的部分移动冲突。
        let event_loop = EventLoop::new();
        boot.mark(&self.logger, "事件循环创建完成");

        // 创建托盘图标（存活到事件循环结束）
        let _tray = match crate::icon::load_app_icon() {
            Some(icon) => match dsh_ui::build_tray_icon(icon) {
                Ok(tray) => {
                    self.logger.info("托盘图标已创建。");
                    Some(tray)
                }
                Err(e) => {
                    self.logger
                        .error(&format!("托盘图标创建失败：{e}（服务仍会在后台运行）"));
                    None
                }
            },
            None => {
                self.logger.error("无法加载应用图标，托盘不可用。");
                None
            }
        };

        // 与 v4 行为一致：启动即自动拉起服务。
        // **在 worker 线程发起**：启动决策里的对账 / 端口探测 / 锁清理都是阻塞调用，
        // 放在这里会把「首帧可响应」往后推（冷启动尤其明显）。托盘与窗口在首帧即可
        // 响应，服务在其后几百毫秒内就绪，用户感知更快。
        self.spawn_start();
        boot.mark(&self.logger, "服务启动请求已提交（后台线程）");

        self.logger.info("进入事件循环。");
        // 首帧前再记一次：这之后窗口/托盘已可响应，属于"用户可感知就绪"的分界
        let mut first_frame = true;

        // 启动参数只需处理一次（否则关闭窗口后会在下一帧立刻重开）
        let mut startup_flags_done = false;
        // 事件循环帧计数（100ms/帧），供 --ipc-probe 定时
        let mut tick: u64 = 0;

        // 运行事件循环
        event_loop.run(move |event, window_target, control_flow| {
            *control_flow = ControlFlow::WaitUntil(std::time::Instant::now() + IDLE_TICK);
            tick += 1;
            if first_frame {
                first_frame = false;
                boot.mark(&self.logger, "事件循环首帧（界面可响应）");
            }

            // 第二个实例双击图标 → 唤起本实例窗口（与 v4 的 ShowWindow 语义一致）
            if self.activation.poll() {
                self.activation_pending = true;
                self.activation_deadline = tick + ACTIVATION_MAX_WAIT_TICKS;
            }

            // `DSHLauncher.exe --quit` → 与托盘「退出」完全相同的收尾路径。
            // 这是发布脚本唯一可用的**优雅**退出入口（否则只能强杀，会切断会话）。
            if self.quit_event.poll() {
                self.logger.info(
                    "收到 --quit 请求，正在按退出流程收尾（服务去留按 service_lifecycle 决定）…",
                );
                if self.begin_exit() {
                    // **先置位回执再退出**：请求方（脚本）据此确认「启动器确实不在了」，
                    // 否则它只能在超时后靠猜——那会导致覆盖仍被占用的 exe。
                    self.quit_ack.signal();
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                // 用户取消：也必须置位回执，否则请求方会把「被拒绝」误判为
                // 「实例还没响应」，进而在实例仍在运行时去覆盖产物。
                self.quit_ack.signal();
                self.logger
                    .warn("--quit 被取消（用户选择了「取消」），继续运行（已回执告知请求方）。");
            }
            if self.activation_pending {
                if self.harness.is_some() {
                    // 窗口已存在（可能被隐藏到托盘、或被其它窗口盖住）→ 本帧稍后
                    // 由 `pump_pending_open` → `open_harness` 显示并聚焦它。
                    // （旧实现只置 `want_open`，而泵在窗口已存在时短路返回 ⇒ 双击图标
                    //   在窗口隐藏后完全无效。）
                    self.activation_pending = false;
                    self.logger.info("收到唤起请求，正在显示内嵌窗口。");
                    self.want_open = true;
                } else if !startup_flags_done {
                    // 本实例自己还在处理启动参数（如 --settings）：不要抢占焦点
                } else if tick >= self.activation_deadline {
                    // 等太久（服务始终没起来）：放弃唤起，避免永久挂着待办
                    self.activation_pending = false;
                    self.logger
                        .warn("唤起请求已过期：服务长时间未就绪，未能打开内嵌窗口。");
                } else if self.port_reachable() {
                    // 窗口曾被关闭、服务仍在 → 重新打开。
                    // **必须限流**：探测是阻塞 connect（超时 300ms），而事件循环每
                    // 100ms 醒一次；见 [`READY_PROBE_INTERVAL`]。
                    self.activation_pending = false;
                    self.logger.info("收到唤起请求，正在重新打开内嵌窗口。");
                    self.want_open = true;
                }
                // 服务尚未就绪：保留 pending，由主循环的 want_open 逐个 tick 重试。
                // 注意不能在这里清除：清早了会让「双击图标」在冷启动阶段丢失。
            }

            // 首帧：处理启动参数。
            // 必须放在事件循环内，此时才能拿到 window_target 创建窗口。
            if !startup_flags_done {
                startup_flags_done = true;
                if opts.open_settings {
                    self.open_settings(window_target);
                }
                if opts.open_guide {
                    self.open_guide(window_target);
                }
            }

            // IPC 探针：模拟一次「保存设置」点击（值不变，无副作用），
            // 用于验证 页面 → Rust 的 IPC 通道确实打通。
            if opts.ipc_probe {
                if tick == 15 {
                    if let Some(s) = &self.settings {
                        match s.evaluate("document.getElementById('saveBtn').click();") {
                            Ok(()) => self.logger.info("[探针] 已模拟点击「保存设置」"),
                            Err(e) => self.logger.error(&format!("[探针] 注入脚本失败：{e}")),
                        }
                    } else {
                        self.logger.error("[探针] 设置窗口不存在");
                    }
                }
                if tick == 45 {
                    self.logger
                        .info("[探针] 结束（若上方出现「设置页命令: Save」则 IPC 正常）");
                    *control_flow = ControlFlow::Exit;
                }
            }

            // 处理托盘菜单事件。
            // 必须循环排空：此前每帧只取一个事件，用户连点菜单时其余事件会延迟
            // 到后续帧才处理（最坏 100ms/个，且与“刷新”等有顺序依赖）。
            while let Some(tray_event) = handle_tray_events() {
                match tray_event {
                    dsh_ui::TrayEvent::Start => {
                        self.logger.info("托盘菜单：启动服务");
                        // 启动决策含对账 / 端口探测 / 锁清理等阻塞步骤 → 放 worker
                        self.spawn_start();
                    }
                    dsh_ui::TrayEvent::Stop => {
                        self.logger.info("托盘菜单：停止服务");
                        self.pending_url = None;
                        self.spawn_stop();
                    }
                    dsh_ui::TrayEvent::Open => {
                        self.logger.info("托盘菜单：打开界面");
                        // 这是把「关闭到托盘」的窗口重新拿回来的**唯一入口**：
                        // 窗口已存在时 `open_harness` 会显示并聚焦它（P0 修复点）。
                        // 同时置空等待预算：这是用户主动要开窗，5 秒内没拿到 token
                        // 就退普通地址（与事件驱动的历史行为一致）。
                        self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                        self.want_open = true;
                        self.pump_pending_open(window_target);
                    }
                    dsh_ui::TrayEvent::Refresh => {
                        self.logger.info("托盘菜单：刷新界面");
                        self.refresh_harness();
                    }
                    dsh_ui::TrayEvent::OpenInBrowser => {
                        self.logger.info("托盘菜单：在浏览器中打开");
                        // 必须用带 token 的地址：dsh web 对无 token 访问返回 401。
                        // 优先用已捕获/待导航的带 token 地址，其次才是服务层的
                        // `ready_url()`（它在没有 token 时会退回普通地址）。
                        let url = self
                            .pending_url
                            .clone()
                            .filter(|u| contains_token(u))
                            .or_else(|| self.service.auth_url())
                            .unwrap_or_else(|| self.service.ready_url());
                        if !contains_token(&url) {
                            self.logger.warn(
                                "尚未捕获到带 token 的地址，浏览器可能要求重新授权；\
                                 内嵌窗口会自动重试。",
                            );
                        }
                        crate::browser::open_default_browser(&url);
                    }
                    dsh_ui::TrayEvent::Settings => {
                        self.logger.info("托盘菜单：打开设置");
                        self.open_settings(window_target);
                    }
                    dsh_ui::TrayEvent::Guide => {
                        self.logger.info("托盘菜单：新手指引");
                        self.open_guide(window_target);
                    }
                    dsh_ui::TrayEvent::OpenLogDir => {
                        self.logger.info("托盘菜单：打开日志目录");
                        crate::browser::open_directory(&dsh_core::log_dir());
                    }
                    dsh_ui::TrayEvent::About => {
                        self.logger.info("托盘菜单：关于");
                        crate::dialog::about(
                            env!("CARGO_PKG_VERSION"),
                            self.service.stops_service_on_exit(),
                        );
                    }
                    dsh_ui::TrayEvent::Exit => {
                        self.logger.info("托盘菜单：退出");
                        if !self.begin_exit() {
                            return; // 用户取消退出：留在事件循环
                        }
                        *control_flow = ControlFlow::Exit;
                    }
                }
            }

            // 处理设置页发来的命令
            while let Ok(cmd) = self.settings_rx.try_recv() {
                self.handle_settings_command(cmd);
            }

            // 处理后台任务结果（归档清理 / 服务重启 / 停止服务）
            while let Ok(task) = self.task_rx.try_recv() {
                match task {
                    BackgroundTask::Cleanup(result) => {
                        self.cleaning = false;
                        match result {
                            Ok((cleaned, detail)) => self.set_settings_status(
                                &format!("已清理 {cleaned} 个归档会话。{detail}"),
                                false,
                            ),
                            Err(e) => self.set_settings_status(&format!("清理失败：{e}"), true),
                        }
                        // 清理按设计停过服务：按需自动拉回，避免用户停在"服务已停"的状态
                        self.restore_service_after_cleanup();
                    }
                    BackgroundTask::Restart(r) => match r {
                        Ok(()) => {
                            // 重启后必须把界面重新导航到新地址，否则窗口仍停在旧服务页面上
                            self.pending_url = None;
                            self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                            // 已有窗口（哪怕隐藏到托盘）就地导航；没有窗口才排入打开待办。
                            if self.harness.is_some() {
                                self.navigate_existing_harness();
                            } else {
                                self.want_open = true;
                            }
                            self.set_settings_status("设置已保存，服务已按新配置重启。", false);
                        }
                        Err(e) => self.set_settings_status(&format!("重启失败：{e}"), true),
                    },
                    BackgroundTask::Stop(r) => match r {
                        Ok(()) => self.set_settings_status("服务已停止。", false),
                        Err(e) => self.set_settings_status(&format!("停止失败：{e}"), true),
                    },
                    BackgroundTask::Start(r) => match r {
                        Ok(()) => {
                            // 「已受理」不等于「已就绪」：就绪与 token 由服务层事件驱动
                            self.set_settings_status(
                                "正在启动服务（就绪后界面会自动打开）…",
                                false,
                            );
                        }
                        Err(e) => self.set_settings_status(&format!("启动失败：{e}"), true),
                    },
                    // 认证自愈：后台探测结果到达后做决策（询问 / 自动重启）
                    BackgroundTask::AuthActivity(activity) => {
                        self.apply_auth_recovery(activity);
                    }
                    // 归档清理：前置探测结果到达 → 主线程弹确认框并启动清理
                    BackgroundTask::CleanupPrecheck {
                        running,
                        active,
                        count,
                    } => {
                        self.cleanup_prechecking = false;
                        self.begin_cleanup(running, active, count);
                    }
                }
            }

            // 处理服务事件（来自 worker 线程）
            while let Ok(evt) = self.rx.try_recv() {
                match evt {
                    ServiceEvent::Ready { url } => {
                        boot.mark(&self.logger, "服务就绪");
                        match &url {
                            Some(u) => self.logger.info(&format!("服务就绪: {u}")),
                            None => self.logger.info(
                                "服务已就绪，但尚未捕获到带 token 的地址（等待 dsh 输出…）。",
                            ),
                        }
                        // **关键**：只有带 token 的地址才写入 pending_url。
                        // 旧实现无条件写入，使「等 token 再导航」永远不可达 → 401 页面。
                        if let Some(u) = url {
                            if contains_token(&u) {
                                self.pending_url = Some(u);
                            }
                        }
                        // **就绪即重置等待预算**：预算应当从"服务已经可达"这一刻起算。
                        // 冷启动时 `auth_wait_ticks` 还是初值 0（`run()` 直接调 `start()`，
                        // 不像设置页路径那样预设预算），于是首个 `Ready{url:None}` 会
                        // 立刻因"预算耗尽"而用无 token 地址开窗 —— 又看到 401 页面。
                        self.auth_wait_ticks = AUTH_URL_WAIT_TICKS;
                        // 冷启动期间用户双击过图标 → 这次打开正好满足唤起请求
                        self.activation_pending = false;
                        // 窗口已存在（可能被用户隐藏到托盘）：只就地导航，
                        // **不得**把窗口强行弹到前台；尚无窗口才排入「打开待办」。
                        if self.harness.is_some() {
                            self.navigate_existing_harness();
                        } else {
                            self.want_open = true;
                        }
                        self.pump_pending_open(window_target);
                    }
                    ServiceEvent::StateChanged(state) => {
                        self.logger.info(&format!("服务状态: {:?}", state));
                    }
                }
            }

            // 「待打开界面」的每帧驱动：让 AUTH_URL_WAIT_TICKS 成为真正的时间预算
            self.pump_pending_open(window_target);

            // 主题跟随：周期采样页面背景色并着色 DWM 标题栏（异步，不阻塞本帧）
            self.maybe_sample_theme();

            // 认证自检：发现「需要认证」的 401 页面就自愈（异步，不阻塞本帧）
            self.maybe_check_auth();

            // 处理 tao 事件
            match event {
                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event: key, .. },
                    window_id,
                    ..
                } => {
                    self.handle_harness_key(window_id, &key);
                }
                Event::WindowEvent {
                    event: WindowEvent::ModifiersChanged(modifiers),
                    ..
                } => {
                    // 修饰键状态的权威来源（见 `handle_harness_key` 的说明）
                    self.ctrl_down = modifiers.control_key();
                }
                Event::WindowEvent {
                    event: WindowEvent::Focused(false),
                    window_id,
                    ..
                } => {
                    // 失焦后不可能再收到 KeyUp：必须清零，否则 Ctrl 会被记成一直按着，
                    // 用户之后随便敲一个 R 就会刷新页面、把正在编辑的内容冲掉。
                    if self.harness.as_ref().map(|h| h.window_id()) == Some(window_id) {
                        self.ctrl_down = false;
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    window_id,
                    ..
                } => {
                    // 关闭的是某个功能窗口 → 按“关闭到托盘”偏好处理；都不认识才退出
                    let known = self.harness.as_ref().map(|h| h.window_id()) == Some(window_id)
                        || self.settings.as_ref().map(|s| s.window_id()) == Some(window_id)
                        || self.guide.as_ref().map(|g| g.window_id()) == Some(window_id);
                    if known {
                        self.request_close_window(window_id);
                    } else {
                        *control_flow = ControlFlow::Exit;
                    }
                }
                Event::MainEventsCleared => {}
                _ => {}
            }
        });
    }
}

/// 必须**活到进程退出**的内核对象（单实例锁、唤起/退出/退出回执事件）。
///
/// 为什么打包成一个结构体：这些句柄的 Drop 就是"释放内核对象"，
/// 放在 `main` 的局部变量里会在 `run` 返回时提前释放（单实例保护随即失效）。
/// 同时这把 `run` 的参数个数压回 5 个（此前 8 个，会触发
/// `clippy::too_many_arguments`）。
pub struct RuntimeHandles {
    pub single: SingleInstance,
    pub activation: ActivationEvent,
    pub quit_event: QuitEvent,
    pub quit_ack: dsh_core::QuitAckEvent,
}

/// 运行主应用循环（入口函数）。
pub fn run(
    config: Settings,
    logger: RollingLogger,
    opts: StartupOptions,
    handles: RuntimeHandles,
    boot: BootTimer,
) -> anyhow::Result<()> {
    let app = App::new(
        config,
        logger,
        handles.single,
        handles.activation,
        handles.quit_event,
        handles.quit_ack,
    );
    app.run(opts, boot)
}

/// 运行自检模式（--selftest）。
pub fn run_selftest(config: &Settings, logger: &RollingLogger) -> anyhow::Result<()> {
    logger.info("==== 自检模式 ====");

    // 1. 解析 dsh 路径
    let node_path = if config.node_path.is_empty() {
        None
    } else {
        Some(config.node_path.as_str())
    };
    let dsh =
        dsh_core::Dsh::resolve(node_path).map_err(|e| anyhow::anyhow!("环境解析失败: {e}"))?;
    logger.info(&format!("node.exe: {}", dsh.node.display()));
    logger.info(&format!("bin.js: {}", dsh.bin_js.display()));

    // 2. 查找空闲端口
    let Some(port) = find_free_port() else {
        logger.error("FAIL: 找不到可用端口（45000–45100 均被占用）。");
        anyhow::bail!("自检失败：没有可用端口");
    };
    logger.info(&format!("测试端口: {port}"));

    // 3. 启动服务
    let mut process = dsh_core::ProcessManager::new();
    let work_dir = if config.work_dir.is_empty() {
        None
    } else {
        Some(std::path::Path::new(&config.work_dir))
    };
    let pid = process
        .start_dsh(port, work_dir, &dsh.node, &dsh.bin_js, None)
        .map_err(|e| anyhow::anyhow!("启动失败: {e}"))?;

    logger.info(&format!("已启动 PID {pid}，等待就绪（最长 120 秒）…"));

    // 4. 等待就绪
    let probe = dsh_core::ReadyProbe::new(port);
    let timeout = Duration::from_secs(120);
    let ready = probe.wait_for_ready(timeout).is_ok();

    if ready {
        logger.info(&format!("PASS: 服务已就绪（端口 {port}）。"));
    } else {
        logger.error("FAIL: 120 秒内未就绪。");
    }

    // 5. 停止服务
    let _ = process.stop();
    logger.info("已停止服务。");

    if ready {
        logger.info("==== 自检通过 ====");
        Ok(())
    } else {
        logger.error("==== 自检失败 ====");
        std::process::exit(1)
    }
}

/// 查找空闲端口。
///
/// 绑定失败时不再 `unwrap`（旧实现会直接 panic 掉整个进程）：
/// 退化到扫描一段高位端口，全部失败才返回 None，由调用方给出明确错误。
fn find_free_port() -> Option<u16> {
    if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", 0)) {
        if let Ok(addr) = listener.local_addr() {
            return Some(addr.port());
        }
    }
    (45_000..45_100).find(|p| dsh_core::is_port_free_to_bind(*p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsh_core::ActiveSessions;

    fn sessions(runners: usize, recent: usize) -> ActiveSessions {
        ActiveSessions {
            runner_processes: runners,
            recently_written: recent,
            recent_session_hint: Vec::new(),
        }
    }

    /// 认证自愈的决策必须是保守的：**只有确认"没有任何活动"才允许自动重启**。
    ///
    /// 这条断言防的是"静默打断用户正在跑的任务"：一旦判定逻辑被改成"默认自动重启"，
    /// 用户在界面坏掉的同时还会丢掉正在进行的工作。
    #[test]
    fn auth_recovery_only_auto_restarts_when_provably_idle() {
        assert_eq!(
            auth_recovery_action(Some(&sessions(0, 0))),
            AuthRecovery::AutoRestart,
            "明确没有活跃会话时才允许不询问"
        );
        assert_eq!(
            auth_recovery_action(Some(&sessions(1, 0))),
            AuthRecovery::AskUser,
            "有会话运行器进程 ⇒ 必须询问"
        );
        assert_eq!(
            auth_recovery_action(Some(&sessions(0, 3))),
            AuthRecovery::AskUser,
            "只有软信号（近期写入）也要询问"
        );
        assert_eq!(
            auth_recovery_action(None),
            AuthRecovery::AskUser,
            "无法判定 ⇒ 必须询问（不得猜成「没有会话」）"
        );
    }
}
