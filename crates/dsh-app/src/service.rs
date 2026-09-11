//! 服务生命周期管理：启动 / 接管 / 停止 / 就绪探测 / 失联自愈。
//!
//! 运行时模型：
//! - 主线程：tao 事件循环（窗口、托盘、IPC 回调）
//! - worker 线程：就绪等待（可取消）、失联监测、归档清理
//! - **服务进程独立性**：由 `settings.toml` 的 `service_lifecycle` 决定
//!   （默认 `independent`：dsh 不挂在启动器的 Job 上，启动器退出/崩溃/被升级
//!   覆盖都不会切断正在进行的会话）。跨启动器生命周期的归属由
//!   `%LOCALAPPDATA%\DSHLauncher\service.json` 簿记 + 启动时对账保证。
//!
//! ## 三个「自有 / 外部」概念必须分清
//!
//! | 概念 | 含义 | 决定什么 |
//! |---|---|---|
//! | [`ServiceInner::owns_process`] | 这个 dsh 是**本进程** spawn 的（持有 `Child`） | 能否读到它 stdout 里的 token URL |
//! | [`ServiceState::External`] | 我们只是**旁观**一个别处启动的实例 | 停止服务时是否敢杀它 |
//! | `service.json` | **上一次启动器**留下的簿记 | 本次是接管还是重启 |
//!
//! 历史缺陷正源于把这几个概念混在一个 `is_own_process()` 里。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use dsh_core::{
    ProcessError, ProcessManager, ReadyProbe, Reconciliation, RollingLogger, ServiceRecord,
    Settings,
};

/// `start()` 的 spawn 阶段结果。
///
/// 单列这个枚举是为了表达「**本次调用没有 spawn**，但服务确实已经在运行」——
/// 它与 `Err` 完全不同（不是失败），与 `Spawned` 也不同（不能把 `owns_process`
/// 记成自己）。
enum SpawnStage {
    /// 本线程确实 spawn 了新进程。
    Spawned(u32),
    /// 另一条路径（失联自愈线程 / 用户连点「启动服务」）已在此之前拉起进程。
    AlreadyRunning,
}

/// 失联监测的探测间隔。
const MONITOR_INTERVAL: Duration = Duration::from_millis(1500);
/// 连续多少次无响应判定为「失联」。8 × 1.5s ≈ 12 秒，
/// 足以跨过 dsh 重启/GC 造成的短暂无响应，又不会让用户等太久。
const LOST_LIMIT: u32 = 8;
/// 自有服务失联后自动重启的最大次数（v4 为 3 次）。
const MAX_AUTO_RESTART: u32 = 3;
/// 服务连续存活多久后，把自动重启计数**清零**。
///
/// 历史缺陷：计数只在「失联后重新接管」这一条路径上重置，于是长期运行中偶发的
/// 3 次崩溃会永久耗尽自愈额度（用户只能重启启动器）。这里改为「稳定运行一段时间
/// 即视为健康，重新获得额度」。
const RESTART_QUOTA_REFILL_AFTER: Duration = Duration::from_secs(300);
/// 等待就绪的总预算（dsh 冷启动实测可达 30 秒以上，留足余量）。
const READY_BUDGET: Duration = Duration::from_secs(120);

/// 服务状态。
///
/// ## 显式状态机（v5.0.0 定稿轮：把隐含态补全）
///
/// 此前只有 5 个值，而真实运行态有 6 种——「停止中」（正在递归终止进程树、
/// 可能要数百毫秒）**没有建模**，于是那段时间里 UI 仍显示 Running，
/// 用户点「打开界面」会得到一个正在消失的服务。
///
/// ```text
/// Stopped ──start()──► Starting ──probe 成功──► Running
///    ▲                    │                        │
///    │                    │ 子进程消失/超时          │ 失联且重启额度用尽
///    │                    ▼                        ▼
///    │                  Error ◄────────────────────┘
///    │                    │
///    │  端口重新出现 dsh    ▼
///    └──────────────── External ──失联──► Error
///
/// Stopping：stop() 的阻塞段（终止进程树）所在的状态；期间不被失联监测计入。
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    /// 我们只是**旁观**一个别处启动的实例（拿不到它的 token）。
    External,
    /// 终止请求已发出，正在回收进程树（阻塞段，可能数百毫秒）。
    Stopping,
    /// 启动失败 / 失联且无法自愈。**注意**：UI 只能看到「有问题」，
    /// 具体原因在日志里（`Error` 单一枚举值承载了「启动超时」与「失联放弃」
    /// 两种语义，这是已知的建模缺陷，见 `docs/AUDIT-REPORT-v5.0.0.md` 的
    /// B.1 状态机小节；不改动枚举是为了不破坏设置页/日志的既有判定）。
    Error,
}

impl ServiceState {
    /// 是否处于「界面应当可用」的状态（失联监测只在这些状态下工作）。
    ///
    /// `Stopping` **不算**活跃：终止过程中端口必然会掉，若计入失联统计，
    /// 会把「用户主动停止」误报成「服务失联」并触发自动重启。
    fn is_active(&self) -> bool {
        matches!(
            self,
            ServiceState::Starting | ServiceState::Running | ServiceState::External
        )
    }
}

/// 发送给主线程的事件。
///
/// 注意：日志不在此列——日志统一由 `RollingLogger` 落地，避免重复记录。
#[derive(Debug)]
pub enum ServiceEvent {
    StateChanged(ServiceState),
    /// 服务就绪。`url` 为 `Some` 表示拿到了**带 token** 的可用地址，
    /// `None` 表示「就绪了但暂无 token」（调用方应等待或走 cookie）。
    ///
    /// **为什么要用 `Option`**：历史上这里无条件填普通地址，而主循环拿到普通
    /// 地址后立刻 `pending_url = Some(plain)`，使得「等 token 再导航」的逻辑
    /// 永远不可达——用户看到的就是 HTTP 401 认证失败页（实测）。
    Ready {
        url: Option<String>,
    },
}

/// 服务管理器：封装 ProcessManager + ReadyProbe + 状态。
///
/// `Clone` 是廉价的（内部仅 `Arc` 计数），用于把句柄交给监测线程。
#[derive(Clone)]
pub struct ServiceHandle {
    inner: Arc<Mutex<ServiceInner>>,
}

struct ServiceInner {
    /// 子进程管理器。
    ///
    /// **为什么单独一把锁**（v5.0.0 定稿轮的关键修正）：`ProcessManager::start_dsh`
    /// 会在内部 spawn 子进程并启动 stdout 读取线程，而读取线程上的就绪回调
    /// （`on_ready`）**要取 `ServiceInner` 锁**。若 spawn 与 `ServiceInner` 锁
    /// 共处同一临界区，就构成「持锁做会回调取同一把锁的事」——
    /// 这正是本仓库历史上那次「只剩托盘图标、界面永不出现」的确定性死锁。
    /// 上一轮只把 spawn 前的守卫 drop 掉、spawn 时又立刻重新取锁，属于**半个修复**；
    /// 现在把进程管理器拆到独立互斥量，spawn 全程不持有 `ServiceInner` 锁，
    /// 死锁在结构上不再可能。
    ///
    /// 锁序约定：**只允许 `ServiceInner` → `ProcessManager`**，
    /// 任何「持 `ProcessManager` 锁再去取 `ServiceInner` 锁」的写法都会形成环路。
    process: Arc<Mutex<ProcessManager>>,
    probe: ReadyProbe,
    state: ServiceState,
    config: Settings,
    logger: RollingLogger,
    event_tx: Option<mpsc::Sender<ServiceEvent>>,
    dsh_paths: Option<(std::path::PathBuf, std::path::PathBuf)>,
    /// 是否启用失联监测。用户显式「停止服务」后置 false。
    monitor_enabled: bool,
    /// 监测线程是否已在运行，防止重复 spawn。
    monitor_running: bool,
    /// 是否持有子进程的 stdout（决定 token 是否**有可能**被捕获到）。
    owns_process: bool,
    /// dsh 启动输出里捕获的**带 token 访问地址**。
    auth_url: Option<String>,
    /// 就绪 worker 的代际号（`stop()` / 新 `start()` 时递增，使在途 worker 立即退出）。
    ready_gen: Arc<AtomicU64>,
    /// 「停止服务」请求标志：让在途 worker 知道本次未就绪是**预期**的（不报错）。
    stop_requested: Arc<AtomicBool>,
    /// 本轮自愈窗口内已重启次数。
    restarts: u32,
    /// 服务最近一次被观察到健康的时刻（用于重启额度回填）。
    healthy_since: Option<Instant>,
}

impl ServiceInner {
    /// 在 `ProcessManager` 锁内执行一段操作。
    ///
    /// 锁序：调用方**可以**持有 `ServiceInner` 锁（`ServiceInner` → `ProcessManager`
    /// 是允许的方向），但闭包内**绝对不能**再去取 `ServiceInner` 锁。
    ///
    /// 锁中毒时用 `into_inner()` 继续（与 `RollingLogger` 一致）：
    /// 本进程是 `panic = "abort"`，锁中毒只可能来自 `debug_assert` 之外的
    /// 显式 catch 场景；宁可继续跑，也不要让一次中毒把服务管理永久瘫痪。
    fn with_process<R>(&self, f: impl FnOnce(&mut ProcessManager) -> R) -> R {
        let mut g = self.process.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

impl ServiceHandle {
    pub fn new(config: Settings, logger: RollingLogger) -> Self {
        let lifecycle = config.service_lifecycle.to_child_lifecycle();
        let mut process = ProcessManager::with_lifecycle(lifecycle);
        process.set_logger(logger.clone());
        let probe = ReadyProbe::new(config.port);
        Self {
            inner: Arc::new(Mutex::new(ServiceInner {
                process: Arc::new(Mutex::new(process)),
                probe,
                state: ServiceState::Stopped,
                config,
                logger,
                event_tx: None,
                dsh_paths: None,
                monitor_enabled: false,
                monitor_running: false,
                owns_process: false,
                auth_url: None,
                ready_gen: Arc::new(AtomicU64::new(0)),
                stop_requested: Arc::new(AtomicBool::new(false)),
                restarts: 0,
                healthy_since: None,
            })),
        }
    }

    /// 确保失联监测线程在运行（幂等）。
    fn ensure_monitor(&self) {
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.monitor_running {
                return;
            }
            g.monitor_running = true;
        }
        let handle = self.clone();
        thread::spawn(move || {
            Self::monitor_worker(handle.clone());
            // 线程结束后允许再次启动
            if let Ok(mut g) = handle.inner.lock() {
                g.monitor_running = false;
            }
        });
    }

    /// 设置事件发送通道。
    pub fn set_event_sender(&self, tx: mpsc::Sender<ServiceEvent>) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .event_tx = Some(tx);
    }

    /// 更新配置（设置页保存后调用）。
    ///
    /// 同步探测端口；并在**无子进程**时切换生命周期策略（切换需要重启服务才生效）。
    pub fn set_config(&self, config: Settings) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let node_changed = inner.config.node_path != config.node_path;
        inner.probe.set_port(config.port);
        inner.config = config;
        // node_path 变了，缓存的 dsh 路径必须失效，否则会继续用旧 node 启动
        if node_changed {
            inner.dsh_paths = None;
        }
    }

    /// 发送事件（**不获取锁**，调用方必须已持有 `inner` 锁）。
    ///
    /// 注意：`std::sync::Mutex` 不可重入，因此所有在持锁状态下触发的通知都必须走
    /// 这套以 `&ServiceInner` 为参数的辅助函数，否则会自死锁。
    fn emit_with(inner: &ServiceInner, event: ServiceEvent) {
        if let Some(tx) = &inner.event_tx {
            let _ = tx.send(event);
        }
    }

    /// 记录一行日志（**不获取锁**，调用方必须已持有 `inner` 锁）。
    fn emit_log_with(inner: &ServiceInner, level: &str, msg: &str) {
        match level {
            "ERROR" => inner.logger.error(msg),
            "WARN" => inner.logger.warn(msg),
            _ => inner.logger.info(msg),
        }
    }

    /// 普通访问地址（不含一次性 token）。**只应作为最后手段使用**：
    /// dsh web 对无 token 访问返回 HTTP 401（实测）。
    fn plain_url(port: u16) -> String {
        format!("http://127.0.0.1:{port}/")
    }

    /// 打开界面时应当使用的地址：优先带 token。
    ///
    /// 只有在「接管外部实例、拿不到 token」时才退回普通地址——那条路径依赖
    /// WebView2 里已持久化的登录 Cookie（与 v4 行为一致），首次使用可能仍需手动授权。
    pub fn ready_url(&self) -> String {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .auth_url
            .clone()
            .unwrap_or_else(|| Self::plain_url(inner.config.port))
    }

    /// 已捕获的带 token 地址（`None` = 尚未拿到）。
    pub fn auth_url(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .auth_url
            .clone()
    }

    /// 本进程是否 spawn 了这个服务（只有这种情况才**有可能**捕获到 token）。
    pub fn owns_service_process(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.owns_process && inner.with_process(|p| p.is_running())
    }

    /// 当前服务是否由本进程托管（自有且存活）。
    pub fn is_own_service_running(&self) -> bool {
        self.owns_service_process()
    }

    /// 是否为已接管的外部实例。
    pub fn is_adopted_service(&self) -> bool {
        matches!(self.state(), ServiceState::External)
    }

    /// 启动或接管服务。
    ///
    /// 决策顺序（**这就是「服务独立于启动器」得以成立的编排**）：
    ///
    /// 1. **本进程自有服务还活着** ⇒ 直接返回（并补发 `Ready`）；
    /// 2. **上一次启动器留下的簿记仍然有效**（PID 存活 + 创建时间匹配 + 身份是
    ///    dsh + 端口在听）⇒ **接管，不重启**，会话与浏览器 Cookie 全部保留；
    /// 3. 端口上有服务但没有簿记（用户手工起的）⇒ 校验身份后接管；**身份无法
    ///    确认则拒绝接管**（决不误杀无关程序）；
    /// 4. 都没有 ⇒ 自己拉起一个 dsh。
    pub fn start(&self) -> Result<(), ServiceError> {
        // 启动阶段计时（与 `app.rs` 的 `[boot]` 同一风格）。
        //
        // 为什么需要：启动决策里有多个**阻塞**步骤（进程对账、端口探测、PATH 解析、
        // 孤儿锁清理、spawn 本身）。此前日志只有「提交了启动请求」与「dsh web 已启动」
        // 两条，中间是黑盒——真出现卡住时无法区分是卡在"解析 node 路径"还是"清理锁文件"
        // （实测：一次非默认端口的启动在 5 分钟内没有任何后续日志）。
        let t_start = Instant::now();
        self.ensure_monitor();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.monitor_enabled = true;
        inner.stop_requested.store(false, Ordering::SeqCst);
        let port = inner.config.port;

        // ---- 分支 1：本进程托管的服务已在运行 ----
        if inner.with_process(|p| p.is_running()) {
            Self::emit_log_with(&inner, "INFO", "服务已在运行，直接打开界面。");
            // 归属必须如实建模：只有本进程 spawn 的才算 `Running`，
            // 接管来的外部实例仍是 `External`（否则失联监测会试图"重启"它）。
            inner.state = if inner.owns_process {
                ServiceState::Running
            } else {
                ServiceState::External
            };
            let state = inner.state.clone();
            Self::emit_with(&inner, ServiceEvent::StateChanged(state));
            let url = inner.auth_url.clone();
            Self::emit_with(&inner, ServiceEvent::Ready { url });
            return Ok(());
        }

        // ---- 分支 2：上一次启动器留下的簿记仍然有效 → 接管（不重启）----
        match dsh_core::reconcile() {
            Reconciliation::Adopt {
                pid,
                port: rec_port,
            } if rec_port == port => {
                let msg = format!(
                    "发现上次启动留下的 dsh 服务仍然可用（PID {pid}，端口 {rec_port}），已直接接管（会话未中断）。"
                );
                Self::emit_log_with(&inner, "INFO", &msg);
                inner.state = ServiceState::External;
                inner.with_process(|p| p.adopt(pid));
                inner.owns_process = false;
                // 接管来的实例拿不到一次性 token（走 WebView2 里已持久化的 Cookie）
                inner.auth_url = None;
                Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::External));
                let url = inner.auth_url.clone();
                Self::emit_with(&inner, ServiceEvent::Ready { url });
                return Ok(());
            }
            Reconciliation::Adopt {
                pid,
                port: rec_port,
            } => {
                // **簿记里的端口 ≠ 当前配置的端口 ⇒ 不能接管**（v5.0.0 定稿轮实测踩到）。
                //
                // 旧实现无条件接管：它把 `adopted_pid` 指向一个**监听在别的端口**的进程，
                // 而就绪探测、失联监测、`ready_url()` 全部用 `config.port`——
                // 于是「已接管」是个假象（界面指向配置端口，被接管的进程在另一个端口），
                // 失联监测也在盯错端口。自检 E 段的「未见接管日志」就是这个原因：
                // 测试用 3099，而 `service.json` 里还留着上一次真实会话的 3080。
                //
                // 正确行为：**不接管**，继续按配置端口走分支 3/4（端口空闲就自己拉起，
                // 被别的 dsh 占着就接管那一个）；旧记录会被新的簿记覆盖。
                Self::emit_log_with(
                    &inner,
                    "WARN",
                    &format!(
                        "服务簿记记录的是端口 {rec_port}（PID {pid}），与当前配置的端口 {port} 不一致；\
                         为避免界面/监测指向错误的端口，本次**不接管**该实例，将按配置端口 {port} 重新处理。"
                    ),
                );
            }
            Reconciliation::Orphan {
                pid,
                port: rec_port,
            } => {
                // 进程活着但端口没在听：交互式启动到一半被杀留下的残留。
                // 它没有任何活跃会话（端口都没在听），按配置决定是否清理。
                let msg = format!(
                    "发现上次遗留的 dsh 进程（PID {pid}，端口 {rec_port}）已不在监听端口。"
                );
                Self::emit_log_with(&inner, "WARN", &msg);
                if inner.config.stop_stale_orphan {
                    // 终止残留进程树是**阻塞**操作（递归 + 快照），而这里仍持着
                    // ServiceInner 锁（同时也就在 UI 线程上）——因此丢到独立线程执行。
                    // 它已经不监听端口、没有任何活跃会话，晚几百毫秒回收没有副作用。
                    ServiceRecord::clear();
                    Self::emit_log_with(
                        &inner,
                        "INFO",
                        &format!("已结束该残留进程（PID {pid}）并清理簿记。"),
                    );
                    thread::spawn(move || {
                        dsh_core::kill_process_tree(pid);
                    });
                }
                // 落到分支 3/4 继续
            }
            Reconciliation::Stale { reason } => {
                Self::emit_log_with(
                    &inner,
                    "INFO",
                    &format!("上次的服务簿记已失效（{reason}），将按需重新启动。"),
                );
            }
            Reconciliation::NoRecord => {}
        }
        Self::emit_log_with(
            &inner,
            "INFO",
            &format!("[start] 服务对账: {}ms", t_start.elapsed().as_millis()),
        );

        // ---- 分支 3：端口上有服务，但不由本进程托管 ----
        let t_probe = Instant::now();
        let port_has_service = dsh_core::is_port_listening(port);
        Self::emit_log_with(
            &inner,
            "INFO",
            &format!(
                "[start] 端口 {port} 探测（is_port_listening={port_has_service}）: {}ms",
                t_probe.elapsed().as_millis()
            ),
        );
        if port_has_service {
            let Some(pid) = dsh_core::find_pid_on_port(port) else {
                let msg = format!("端口 {port} 被占用但无法确定占用者 PID，已放弃接管。");
                Self::emit_log_with(&inner, "WARN", &msg);
                inner.state = ServiceState::Error;
                Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Error));
                return Err(ServiceError::Start(msg));
            };
            if pid == std::process::id() {
                // 端口被自己占用（异常状态），不做任何接管
                inner.state = ServiceState::Error;
                Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Error));
                return Ok(());
            }
            // **身份校验**：不是 dsh 就绝不接管、绝不杀。
            //
            // 这里必须区分三态（v5.0.0 定稿轮修正 —— 实测踩到）：早期实现把"读不到进程信息"
            // 也说成「被非 dsh 程序占用」，于是日志会出现自相矛盾的结论
            // （前一秒还在正常接管那个 PID，后一秒说它不是 dsh），误导排查方向。
            let identity = dsh_core::classify_dsh_identity(pid);
            if !identity.is_dsh() {
                let reason = if identity.is_unknown() {
                    "无法读取该进程信息（可能正在退出或权限受限）"
                } else {
                    "经校验不是 dsh 程序"
                };
                let img = {
                    let p = identity.steps().image_path.clone();
                    if p.is_empty() {
                        "<无法读取>".to_string()
                    } else {
                        p
                    }
                };
                let msg = format!(
                    "端口 {port} 被 PID {pid} 占用，但{reason}（映像 {img}；{}）。\
                     为避免误杀无关程序，启动器不会接管也不会结束它；请稍后重试或更换端口。",
                    identity.summary()
                );
                inner.logger.error(&msg);
                inner.state = ServiceState::Error;
                Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Error));
                return Err(ServiceError::Start(msg));
            }
            let msg = format!("端口 {port} 上已有 dsh 服务在运行（PID {pid}），直接接管。");
            Self::emit_log_with(&inner, "INFO", &msg);
            inner.state = ServiceState::External;
            inner.with_process(|p| p.adopt(pid));
            inner.owns_process = false;
            // 记录簿记，供下次启动器启动时对账
            record_service(pid, port, &inner.logger);
            // 外部实例的一次性 token 不可得
            Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::External));
            let url = inner.auth_url.clone();
            Self::emit_with(&inner, ServiceEvent::Ready { url });
            return Ok(());
        }

        // ---- 分支 4：自己拉起服务 ----
        let t_resolve = Instant::now();
        let (node, bin_js) = match &inner.dsh_paths {
            Some(p) => p.clone(),
            None => {
                let node_path = if inner.config.node_path.is_empty() {
                    None
                } else {
                    Some(inner.config.node_path.as_str())
                };
                let dsh = dsh_core::Dsh::resolve(node_path)
                    .map_err(|e| ServiceError::Resolve(e.to_string()))?;
                let paths = (dsh.node.clone(), dsh.bin_js.clone());
                inner.dsh_paths = Some(paths.clone());
                paths
            }
        };
        Self::emit_log_with(
            &inner,
            "INFO",
            &format!(
                "[start] 解析 dsh 路径（node={}）: {}ms",
                node.display(),
                t_resolve.elapsed().as_millis()
            ),
        );

        inner.probe.bump_generation();

        // 启动前清理 dsh 遗留的原子写锁（按锁文件内的持有者 PID 判定，活跃锁不碰）
        let t_locks = Instant::now();
        let cleaned = dsh_core::clean_stale_dsh_locks();
        Self::emit_log_with(
            &inner,
            "INFO",
            &format!(
                "[start] 清理遗留锁（命中 {} 个）: {}ms",
                cleaned.len(),
                t_locks.elapsed().as_millis()
            ),
        );
        if !cleaned.is_empty() {
            let names: Vec<String> = cleaned
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string())
                })
                .collect();
            Self::emit_log_with(
                &inner,
                "WARN",
                &format!(
                    "清理了 {} 个 dsh 遗留锁文件（上次异常退出所致）：{}",
                    names.len(),
                    names.join(", ")
                ),
            );
        }

        // 每次启动都是新的 dsh 进程 ⇒ token 也是新的；清掉旧地址
        inner.auth_url = None;
        // 生命周期策略可能被设置页改过。
        //
        // ⚠️ **切换策略不能杀掉正在服务的 dsh**（实测踩到）。
        // Job 归属是**进程创建时**决定的、无法事后改变，所以"切换策略"只能作用于
        // **以后新拉起**的进程。早期实现这里会先 `process.stop()` 再 set_lifecycle，
        // 后果：用户只是取消勾选「服务独立于启动器」，正在进行的会话立刻被打断，
        // 而且重启还失败了（端口上残留着正在退出的旧进程）→ 服务进入 Error，
        // 退出时连"要不要停服务"的询问都没有对象。
        //
        // 现在的语义：**能改就改（当前没有子进程时）；有子进程就只记录，
        // 等下一次自然重启时生效**，并在日志里说清楚。
        let lifecycle = inner.config.service_lifecycle.to_child_lifecycle();
        if inner.with_process(|p| p.lifecycle()) != lifecycle {
            if inner.with_process(|p| p.set_lifecycle(lifecycle)) {
                Self::emit_log_with(
                    &inner,
                    "INFO",
                    &format!(
                        "生命周期策略已切换为 {}",
                        inner.config.service_lifecycle.label()
                    ),
                );
            } else {
                Self::emit_log_with(
                    &inner,
                    "WARN",
                    &format!(
                        "生命周期策略将改为 {}，但当前有在运行的服务进程（Job 归属无法事后改变）；\
                         为避免打断正在进行的会话，本次不重启，下次服务重启后生效。",
                        inner.config.service_lifecycle.label()
                    ),
                );
            }
        }

        // 工作目录：留空表示沿用 dsh 默认（这里复用 config 上的唯一判定，避免两处各写一遍）
        let work_dir = inner.config.effective_work_dir().map(|p| p.to_path_buf());
        // **不存在的目录必须说出来**：`ProcessManager::node_command` 只在目录存在时才
        // `current_dir`，因此配错的路径会被**静默忽略**，dsh 将沿用启动器的工作目录——
        // 表现为"工作目录设置没生效"，而日志里没有任何线索（Harness 的 workspace
        // 也随之改变，用户很难把两件事联系起来）。
        if let Some(dir) = &work_dir {
            if !dir.is_dir() {
                Self::emit_log_with(
                    &inner,
                    "WARN",
                    &format!(
                        "配置的工作目录不存在或不可访问（{}），本次将沿用 dsh 的默认工作目录；\
                         请在设置页更正该路径。",
                        dir.display()
                    ),
                );
            }
        }

        // 读取 dsh stdout 的回调：捕获带 token 的就绪地址。
        //
        // ⚠️ **本回调会取 `ServiceInner` 锁**，因此 `start_dsh` 必须在**完全不持有
        // 该锁**的状态下调用。这正是把 `ProcessManager` 拆到独立互斥量的原因：
        // spawn 只持有 `ProcessManager` 锁，而进程管理器内部**从不**回头取
        // `ServiceInner` 锁，因此不存在环路（曾经这里是确定性死锁：
        // 守卫从函数开头活到 spawn 之后，读取线程的回调与主线程互相等待）。
        let inner_for_hook = Arc::clone(&self.inner);
        let on_ready: dsh_core::process::ReadyHook = Box::new(move |url: String, raw: String| {
            let Ok(mut g) = inner_for_hook.lock() else {
                return;
            };
            g.logger.info(&format!(
                "已捕获 dsh 就绪地址（token 已脱敏）：{}",
                dsh_core::redact_secrets(&raw)
            ));
            g.auth_url = Some(url);
            // 若就绪事件已经先发过（url=None），这里补发一次带 token 的 Ready，
            // 让主循环把导航地址升级为带 token 的版本。
            let st = g.state.clone();
            if matches!(st, ServiceState::Starting | ServiceState::Running) {
                let url = g.auth_url.clone();
                Self::emit_with(&g, ServiceEvent::Ready { url });
            }
        });

        // ★ 从句柄里取出进程管理器，然后**释放 ServiceInner 锁**再 spawn。
        //
        // 🔴 **P0 回归修复（v5.0.0 LTS 定稿轮）**：这里此前写的是
        //     `let pm = { let g = self.inner.lock()…; Arc::clone(&g.process) };`
        // 而 `inner`（本函数开头取的守卫）**此刻仍然存活** —— `std::sync::Mutex` 不可重入，
        // 于是同一线程第二次 `lock()` **永久阻塞**：启动服务在「清理遗留锁」之后立刻自死锁，
        // 且因为死锁发生在持有 `ServiceInner` 锁的状态下，**后续任何服务操作（状态查询、
        // 设置页保存、停止服务）都会一起卡死**。
        //
        // 为什么长期没被发现：正常使用中端口上总有上一次启动器留下的 dsh，`start()` 走
        // 「接管」分支（在到达这里之前就 return 了）；只有**端口上没有服务**（全新机器、
        // 手动停掉服务后重启、换端口）时才会走到本分支 —— 而那条路径恰好没有单元测试覆盖。
        // 实测证据：`[start] 清理遗留锁（命中 0 个）: 8ms` 之后 5 分钟无任何后续日志。
        let pm = Arc::clone(&inner.process);
        drop(inner);

        let t_spawn = Instant::now();
        let started: Result<SpawnStage, ProcessError> = {
            let mut g = pm.lock().unwrap_or_else(|e| e.into_inner());
            // **spawn 前必须在同一临界区内复查存活**（v5.0.0 定稿轮修正）。
            //
            // 上面的分支 1 检查与这里的 spawn 之间释放过 `ServiceInner` 锁，于是
            // 另一条路径（失联自愈线程的 `handle.start()`、或用户连点「启动服务」）
            // 可能已经抢先拉起进程。若不复查，两个线程会各自 spawn 一个 dsh：
            // 它们抢同一个端口，一个必然绑定失败，而**簿记/状态会指向失败的那个**，
            // 表现为「服务已启动但界面打不开」，且日志里两个 PID 互相矛盾。
            // 此刻持有 `ProcessManager` 锁 ⇒ 与另一次 spawn 互斥，复查是可靠的。
            if g.is_running() {
                Ok(SpawnStage::AlreadyRunning)
            } else {
                g.start_dsh(port, work_dir.as_deref(), &node, &bin_js, Some(on_ready))
                    .map(SpawnStage::Spawned)
            }
        };

        // 重新取锁写回状态
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Self::emit_log_with(
            &inner,
            "INFO",
            &format!("[start] spawn 子进程: {}ms", t_spawn.elapsed().as_millis()),
        );
        let pid = match started {
            Ok(SpawnStage::AlreadyRunning) => {
                Self::emit_log_with(
                    &inner,
                    "INFO",
                    "服务已在运行（另一条路径刚完成启动），本次不重复拉起。",
                );
                return Ok(());
            }
            Ok(SpawnStage::Spawned(pid)) => pid,
            Err(e) => {
                inner.state = ServiceState::Error;
                inner.owns_process = false;
                let msg = format!("启动失败：{e}");
                inner.logger.error(&msg);
                Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Error));
                return Err(ServiceError::Start(msg));
            }
        };

        // **停止优先**（v5.0.0 定稿轮修正）：spawn 期间用户可能点了「停止服务」——
        // 那时 `stop()` 找不到子进程句柄，什么也没杀就回到 `Stopped`；如果这里直接
        // 覆盖状态，就会留下一个**无人监视**的 dsh（UI 停在 Starting，直到 120 秒
        // 就绪超时才被回收），且 `owns_process` 与真实归属不符。
        if inner.stop_requested.load(Ordering::SeqCst) {
            let pm = Arc::clone(&inner.process);
            inner.state = ServiceState::Stopped;
            inner.owns_process = false;
            inner.auth_url = None;
            inner.healthy_since = None;
            ServiceRecord::clear();
            Self::emit_log_with(
                &inner,
                "WARN",
                &format!("启动期间收到停止请求：已立即回收刚启动的进程（PID {pid}）。"),
            );
            Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Stopped));
            drop(inner);
            // 阻塞终止在 ServiceInner 锁之外执行（锁序：ServiceInner → ProcessManager）
            pm.lock()
                .unwrap_or_else(|e| e.into_inner())
                .stop()
                .map_err(|e| ServiceError::Stop(e.to_string()))?;
            return Ok(());
        }

        inner.state = ServiceState::Starting;
        inner.owns_process = true;
        inner.healthy_since = None;
        record_service(pid, port, &inner.logger);
        let msg = format!(
            "dsh web 已启动（PID {pid}，生命周期 {}）",
            inner.config.service_lifecycle.label()
        );
        Self::emit_log_with(&inner, "INFO", &msg);
        Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Starting));

        // 就绪 worker：带代际号，stop()/新 start() 会让它立即退出
        let gen = inner.ready_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let inner_clone = Arc::clone(&self.inner);
        let ready_gen = Arc::clone(&inner.ready_gen);
        let stop_flag = Arc::clone(&inner.stop_requested);
        drop(inner);
        thread::spawn(move || {
            Self::wait_for_ready_worker(inner_clone, ready_gen, stop_flag, gen);
        });

        Ok(())
    }

    /// 停止服务（**阻塞**：会递归终止进程树并等待，可能数百毫秒）。
    ///
    /// 只对**本进程托管**或**已确认身份**的服务生效。外部实例若身份无法确认，
    /// 由 `start()` 分支 3 拒绝接管，这里也不会去杀它。
    ///
    /// ## 线程约束
    ///
    /// **不要从 UI 线程直接调用**（会冻结事件循环）。UI 路径由
    /// `dsh-app/src/app.rs` 的 `App::spawn_stop()` 放到 worker 线程执行，
    /// 并把结果通过后台任务通道回传；本阻塞版本供 worker 线程与进程退出收尾使用。
    ///
    /// ## 实现要点
    ///
    /// 终止过程**不持有 `ServiceInner` 锁**：只持有 `ProcessManager` 锁。
    /// 此前把 `inner.process.stop()` 放在 `inner` 锁内，导致「持锁做数百毫秒的
    /// 进程终止」——UI 线程上的任何 `state()` / `ready_url()` 都会跟着卡住。
    pub fn stop(&self) -> Result<(), ServiceError> {
        // 快照 + 状态迁移：让 UI 立刻进入「停止中」，并让在途 worker 失效
        let outcome = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.monitor_enabled = false;
            inner.stop_requested.store(true, Ordering::SeqCst);
            inner.ready_gen.fetch_add(1, Ordering::SeqCst);
            inner.state = ServiceState::Stopping;
            inner.owns_process = false;
            let pm = Arc::clone(&inner.process);
            Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Stopping));
            // 释放 ServiceInner 锁后再做阻塞终止（锁序：ServiceInner → ProcessManager）
            drop(inner);
            let mut pm_guard = pm.lock().unwrap_or_else(|e| e.into_inner());
            pm_guard
                .stop()
                .map_err(|e| ServiceError::Stop(e.to_string()))
        };
        let (killed_pids, kill_error) = match outcome {
            Ok(v) => (v, None),
            Err(e) => (Vec::new(), Some(e)),
        };

        ServiceRecord::clear();

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.state = ServiceState::Stopped;
        inner.auth_url = None;
        inner.healthy_since = None;
        Self::emit_log_with(
            &inner,
            "INFO",
            &format!(
                "服务已停止{}。",
                if killed_pids.is_empty() {
                    String::new()
                } else {
                    format!(
                        "（已结束 PID {}）",
                        killed_pids
                            .iter()
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ),
        );
        Self::emit_with(&inner, ServiceEvent::StateChanged(ServiceState::Stopped));
        match kill_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// 停止服务（**不阻塞调用方**）：把阻塞终止放到 worker 线程。
    ///
    /// `on_done` 在终止结束后（在 worker 线程上）被调用，用于把结果回传给调用方
    /// （`app.rs` 用它把结果投递到后台任务通道，再回主循环更新界面状态）。
    ///
    /// 重复调用是安全的（`ProcessManager::stop()` 幂等）。
    pub fn stop_async(&self, on_done: impl FnOnce(Result<(), String>) + Send + 'static) {
        let handle = self.clone();
        // 回调放进槽位：这样「线程创建失败」的唯一分支也能把它取回来同步调用
        // （`FnOnce` 只能被消费一次，不能同时被两个分支捕获）。
        let slot = Arc::new(Mutex::new(Some(on_done)));
        let slot_in_thread = Arc::clone(&slot);
        let spawned = thread::Builder::new()
            .name("dsh-stop".into())
            // 终止路径只做 Win32 调用，不需要默认 8 MB 栈
            .stack_size(256 * 1024)
            .spawn(move || {
                let r = handle.stop().map_err(|e| e.to_string());
                if let Err(e) = &r {
                    let g = handle.inner.lock().unwrap_or_else(|e| e.into_inner());
                    Self::emit_log_with(&g, "ERROR", &format!("停止服务时出错：{e}"));
                }
                let cb = slot_in_thread
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                if let Some(cb) = cb {
                    cb(r);
                }
            });
        if let Err(e) = spawned {
            // 起不了线程时退化为同步终止（宁可卡一下，也不能不停止）
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            Self::emit_log_with(
                &g,
                "ERROR",
                &format!("无法创建停止线程（{e}），改为同步停止。"),
            );
            drop(g);
            let r = self.stop().map_err(|e| e.to_string());
            let cb = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(cb) = cb {
                cb(r);
            }
        }
    }

    /// 失联监测线程：守护服务存活，并在恢复后让界面自动重连。
    fn monitor_worker(handle: ServiceHandle) {
        let mut lost: u32 = 0;
        loop {
            thread::sleep(MONITOR_INTERVAL);

            // ---- 读取快照（短锁）----
            let (state, port, enabled, restarts, healthy_since) = {
                let g = handle.inner.lock().unwrap_or_else(|e| e.into_inner());
                (
                    g.state.clone(),
                    g.config.port,
                    g.monitor_enabled,
                    g.restarts,
                    g.healthy_since,
                )
            };
            if !enabled {
                // **不要在锁外直接 return**：ensure_monitor() 是在锁内检查
                // monitor_running 的，若本线程观察到 monitor_enabled=false 就退出，
                // 而调用方紧接着又 start()（快速「停止→启动」），ensure_monitor()
                // 会看到 monitor_running==true 而不再新建线程 —— 于是
                // 「监测线程已消失但 monitor_enabled==true」：自愈、失联重连、
                // 外部实例重新接管**全部静默失效**。这里在锁内再确认一次。
                let mut g = handle.inner.lock().unwrap_or_else(|e| e.into_inner());
                if !g.monitor_enabled {
                    g.monitor_running = false;
                    return;
                }
                drop(g);
                lost = 0;
                continue;
            }

            // ---- 探测（不持锁，避免阻塞主线程）----
            let alive = dsh_core::is_port_listening(port);

            let mut g = handle.inner.lock().unwrap_or_else(|e| e.into_inner());

            // 重启额度回填：服务稳定运行足够久即视为健康
            let mut restarts = restarts;
            let mut healthy_since = healthy_since;
            if alive {
                match healthy_since {
                    None => {
                        healthy_since = Some(Instant::now());
                        g.healthy_since = healthy_since;
                    }
                    Some(t) if t.elapsed() >= RESTART_QUOTA_REFILL_AFTER && restarts > 0 => {
                        g.logger.info(&format!(
                            "服务已稳定运行 {} 秒，自动重启额度已恢复（原已用 {restarts} 次）。",
                            t.elapsed().as_secs()
                        ));
                        restarts = 0;
                        g.restarts = 0;
                    }
                    _ => {}
                }
            } else {
                // 服务不可达：清零「健康起点」，下次可达时重新计时
                g.healthy_since = None;
            }

            if state.is_active() {
                if alive {
                    if lost > 0 {
                        // 从失联中恢复：让界面重新导航以重建连接
                        g.logger
                            .info(&format!("服务 {port} 已恢复响应，正在重新连接界面…"));
                        lost = 0;
                        Self::emit_with(&g, ServiceEvent::StateChanged(g.state.clone()));
                        let url = g.auth_url.clone();
                        Self::emit_with(&g, ServiceEvent::Ready { url });
                    }
                    continue;
                }

                let starting = matches!(state, ServiceState::Starting);
                let child_alive = g.with_process(|p| p.is_running());

                // Starting 期间端口尚未监听是**预期**行为（dsh 冷启动实测 30 秒以上，
                // 而 8 × 1.5 s = 12 s 就会走到这里）。旧实现把这段时间也算作
                // 「无响应」，于是冷启动第 12 秒：状态被错误置为 Error、刚写下的
                // service.json 被清掉、日志报「自有进程已退出」——而子进程活得好好的。
                // 这段时间的权威是就绪 worker；监测线程只管「子进程是否还在」。
                if starting && child_alive {
                    lost = 0;
                    continue;
                }

                lost += 1;
                if lost == 1 && !starting {
                    Self::emit_log_with(&g, "WARN", &format!("服务 {port} 无响应，正在连续检测…"));
                }
                // Starting 且子进程已消失 ⇒ 不必等满 12 秒，直接按失联处理
                let give_up = lost >= LOST_LIMIT || (starting && !child_alive);
                if !give_up {
                    continue;
                }
                lost = 0;

                let was_own = g.owns_process;

                if was_own && !child_alive && restarts < MAX_AUTO_RESTART {
                    restarts += 1;
                    g.restarts = restarts;
                    Self::emit_log_with(
                        &g,
                        "WARN",
                        &format!("自有服务已退出，正在自动重启（第 {restarts} 次）…"),
                    );
                    g.state = ServiceState::Stopped;
                    g.owns_process = false;
                    Self::emit_with(&g, ServiceEvent::StateChanged(ServiceState::Stopped));
                    drop(g);
                    // start() 需要重新取锁，必须先释放
                    if let Err(e) = handle.start() {
                        let g2 = handle.inner.lock().unwrap_or_else(|e| e.into_inner());
                        Self::emit_log_with(&g2, "ERROR", &format!("自动重启失败：{e}"));
                    }
                    continue;
                }

                // 外部实例（或重启次数用尽）：无法代为重启，仅标记失联并继续看守
                g.with_process(|p| p.clear_adopted());
                g.state = ServiceState::Error;
                g.owns_process = false;
                ServiceRecord::clear();
                Self::emit_log_with(
                    &g,
                    "ERROR",
                    &format!(
                        "服务 {port} 已失联（{}）。已停止重试；若该端口重新出现服务，将自动重新接管。",
                        if was_own {
                            "自有进程已退出且自动重启用尽"
                        } else {
                            "外部实例已退出"
                        }
                    ),
                );
                Self::emit_with(&g, ServiceEvent::StateChanged(ServiceState::Error));
                continue;
            }

            // ---- 非活跃状态：看守端口，出现服务就自动接管 ----
            //
            // 这里只可能是 `Error`：`Stopped` 不需要自动接管，而 `Stopping` **不可能**
            // 出现在本分支 —— `stop()` 在同一个临界区里同时置 `monitor_enabled = false`
            // 与 `state = Stopping`，因此快照里不会出现「enabled 为真且正在停止」。
            // 把条件写成显式的 `Error` 而不是「非 Stopped」，是为了让这条不变量在
            // 未来有人改动 `stop()` 的加锁范围时**立刻**暴露（而不是悄悄接管一个正在被
            // 终止的进程）。
            if alive && matches!(state, ServiceState::Error) {
                // 身份校验：端口上出现的必须是 dsh，否则不接管（避免误杀无关程序）
                let Some(pid) = dsh_core::find_pid_on_port(port) else {
                    continue;
                };
                if pid == std::process::id() {
                    continue;
                }
                if !dsh_core::is_dsh_harness_process(pid) {
                    g.logger.warn(&format!(
                        "端口 {port} 重新出现服务（PID {pid}），但它不是 dsh，已忽略（不接管、不结束）。"
                    ));
                    continue;
                }
                g.state = ServiceState::External;
                g.with_process(|p| p.clear_adopted());
                g.with_process(|p| p.adopt(pid));
                g.owns_process = false;
                record_service(pid, port, &g.logger);
                Self::emit_log_with(
                    &g,
                    "INFO",
                    &format!("检测到端口 {port} 重新有 dsh 服务在运行（PID {pid}），已重新接管。"),
                );
                Self::emit_with(&g, ServiceEvent::StateChanged(ServiceState::External));
                let url = g.auth_url.clone();
                Self::emit_with(&g, ServiceEvent::Ready { url });
                lost = 0;
                g.restarts = 0;
            }
        }
    }

    /// 当前状态（设置页展示用）。
    pub fn state(&self) -> ServiceState {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .clone()
    }

    /// 优雅退出时保留后台服务。
    ///
    /// `independent`（默认）下**天然成立**——dsh 从未挂在启动器的 Job 上。
    /// `tied` 下解除 Job 的 `KILL_ON_JOB_CLOSE`。
    pub fn keep_children_on_exit(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.with_process(|p| p.keep_children_on_exit())
    }

    /// 当前服务进程 PID（本进程托管的或接管的）。
    ///
    /// 目前无生产调用方 —— 设置页改端口时曾用它判断"是否需要重启"，但**生命周期
    /// 变更已改为不重启**，于是它失去了唯一调用点。保留它是因为诊断脚本会用到，
    /// 且它只是 `ProcessManager::pid` 的透明转发。
    #[allow(dead_code)]
    pub fn pid(&self) -> Option<u32> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.with_process(|p| p.pid())
    }

    /// 生命周期策略是否要求「服务随启动器退出而停止」。
    pub fn stops_service_on_exit(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .config
            .service_lifecycle
            == dsh_core::ServiceLifecycle::Tied
    }

    /// worker 线程：等待服务就绪。
    ///
    /// **可取消**：`stop()` 或新的 `start()` 会递增代际号，本线程在下一轮检查时
    /// 立即返回，不再空转满 120 秒。
    fn wait_for_ready_worker(
        inner: Arc<Mutex<ServiceInner>>,
        ready_gen: Arc<AtomicU64>,
        stop_flag: Arc<AtomicBool>,
        my_gen: u64,
    ) {
        let probe = {
            let g = inner.lock().unwrap_or_else(|e| e.into_inner());
            g.probe.clone()
        };

        // 分片等待，便于及时响应取消（每次最多 250 ms）
        let deadline = Instant::now() + READY_BUDGET;
        let outcome = loop {
            if ready_gen.load(Ordering::SeqCst) != my_gen {
                return; // 已被取消（stop 或新一轮 start）
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break Err(dsh_core::ProbeError::Timeout);
            }
            let slice = remaining.min(Duration::from_millis(250));
            match probe.wait_for_ready(slice) {
                Ok(()) => break Ok(()),
                Err(dsh_core::ProbeError::Timeout) => continue,
                Err(e) => break Err(e),
            }
        };

        // 取消检查：若期间发生了 stop/restart，本次结果作废
        if ready_gen.load(Ordering::SeqCst) != my_gen {
            return;
        }

        match outcome {
            Ok(()) => {
                let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
                // **关键**：在锁内二次确认 token，并用它构造事件。
                // 历史缺陷：这里只读一次 auth_url，与 stdout 捕获线程存在竞态，
                // 结果是「就绪事件先到、token 后到」，界面被导航到无 token 地址
                // ⇒ 用户看到 HTTP 401 认证失败页（实测取证）。
                let url = g.auth_url.clone();
                g.state = ServiceState::Running;
                g.owns_process = g.with_process(|p| p.is_running());
                g.healthy_since = Some(Instant::now());
                let shown = url
                    .clone()
                    .unwrap_or_else(|| Self::plain_url(g.config.port));
                g.logger.info(&format!(
                    "服务就绪 ✓（地址 token 已脱敏：{}）",
                    dsh_core::redact_secrets(&shown)
                ));
                Self::emit_with(&g, ServiceEvent::StateChanged(ServiceState::Running));
                // url 为 None 时**不填普通地址**：交给主循环等 token（或走 cookie），
                // 避免把 401 页面开给用户。stdout 回调捕获到 token 后会补发 Ready。
                Self::emit_with(&g, ServiceEvent::Ready { url });
            }
            Err(dsh_core::ProbeError::Timeout) => {
                // 先把进程管理器句柄取出来，**释放 ServiceInner 锁**再做阻塞终止：
                // 持锁做进程终止会让 UI 线程的 state()/ready_url() 一起卡住。
                let pm = {
                    let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
                    // 用户主动停止导致的未就绪是预期行为，不报错
                    if stop_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    g.state = ServiceState::Error;
                    g.owns_process = false;
                    g.logger
                        .error("启动超时：120 秒内服务未就绪，已停止并复位。");
                    ServiceRecord::clear();
                    Self::emit_with(&g, ServiceEvent::StateChanged(ServiceState::Error));
                    Arc::clone(&g.process)
                };
                let mut pm_guard = pm.lock().unwrap_or_else(|e| e.into_inner());
                let _ = pm_guard.stop();
            }
            Err(dsh_core::ProbeError::GenerationChanged) => {}
        }
    }
}

/// 写入服务簿记（失败只记日志，不阻断流程）。
fn record_service(pid: u32, port: u16, logger: &RollingLogger) {
    let exe = dsh_core::process_image_path(pid)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    match ServiceRecord::capture(pid, port, &exe) {
        Some(rec) => {
            if let Err(e) = rec.save() {
                logger.warn(&format!("写入服务簿记失败（不影响运行）：{e}"));
            }
        }
        None => logger.warn(&format!(
            "无法读取 PID {pid} 的创建时间，本次不写服务簿记（下次启动将按端口重新识别）。"
        )),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("环境解析失败：{0}")]
    Resolve(String),
    #[error("启动失败：{0}")]
    Start(String),
    #[error("停止失败：{0}")]
    Stop(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（P0）：**端口上没有服务时**，`start()` 必须在有界时间内返回。
    ///
    /// ## 这条测试防的是什么
    ///
    /// 该路径曾在「清理遗留锁」之后因**同一线程重复 `lock()` 同一个
    /// `std::sync::Mutex`**（不可重入）而永久阻塞：实测 `[start] 清理遗留锁: 8ms`
    /// 之后 5 分钟没有任何后续日志，且因为死锁时仍持有 `ServiceInner` 锁，
    /// 后续任何服务操作都会一起卡死。
    ///
    /// 为什么长期没暴露：正常使用中端口上总有上一次启动器留下的 dsh，`start()` 直接走
    /// 「接管」分支就 return 了；**只有端口上没有服务**（全新环境 / 手动停服后重启 /
    /// 换端口）才会走到 spawn 分支。所以这里显式覆盖那条路径。
    ///
    /// 本测试会真的拉起一个 dsh，因此**默认跳过**，需要 `DSH_TEST_SPAWN=1` 显式开启
    /// （普通 `cargo test` 不应在开发机上启动真实服务、更不该改动簿记）；
    /// 开启时它会用随机空闲端口、结束时停掉服务并清空簿记，不留残留。
    #[test]
    fn start_without_existing_service_returns_within_budget() {
        if std::env::var_os("DSH_TEST_SPAWN").is_none() {
            return; // 默认跳过（见文档注释）
        }
        // 取一个当前空闲的端口，避免碰到真实服务（3080）与用户会话
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("应能绑定临时端口");
            let p = l.local_addr().expect("应能读取临时端口").port();
            drop(l);
            p
        };
        let dsh_available = dsh_core::Dsh::resolve(None).is_ok();
        let cfg = Settings {
            port,
            ..Default::default()
        };
        let logger = RollingLogger::new(std::env::temp_dir().join("dsh-start-deadlock-test"));
        let handle = ServiceHandle::new(cfg, logger);

        let t = Instant::now();
        let r = handle.start();
        let elapsed = t.elapsed();
        assert!(
            elapsed < Duration::from_secs(60),
            "start() 必须在有界时间内返回（自死锁会永久阻塞），实际 {elapsed:?}"
        );
        if dsh_available {
            assert!(r.is_ok(), "本机已安装 dsh 时应能启动服务：{r:?}");
            assert!(
                handle.is_own_service_running(),
                "spawn 之后必须如实建模为「自有服务在运行」"
            );
        }
        // 收尾：停掉本次启动的服务（幂等），并清掉簿记
        let _ = handle.stop();
    }

    /// 状态机的核心不变量：**「终止中」不算活跃**。
    ///
    /// `Stopping` 期间端口必然会掉；若把它计入失联统计，失联监测会把
    /// 「用户主动停止」误报成「服务失联」并触发自动重启。
    #[test]
    fn stopping_is_not_an_active_state() {
        assert!(ServiceState::Starting.is_active());
        assert!(ServiceState::Running.is_active());
        assert!(ServiceState::External.is_active());
        assert!(!ServiceState::Stopping.is_active());
        assert!(!ServiceState::Stopped.is_active());
        assert!(!ServiceState::Error.is_active());
    }

    /// 生命周期策略决定 `stops_service_on_exit` —— 退出确认与 Job 处理都依赖它。
    #[test]
    fn lifecycle_flag_follows_config() {
        let logger = RollingLogger::new(std::env::temp_dir().join("dsh-lifecycle-test"));
        let tied = ServiceHandle::new(
            Settings {
                service_lifecycle: dsh_core::ServiceLifecycle::Tied,
                ..Default::default()
            },
            logger.clone(),
        );
        assert!(tied.stops_service_on_exit());
        assert_eq!(tied.state(), ServiceState::Stopped);
        assert!(!tied.is_adopted_service());
        assert!(!tied.is_own_service_running());

        let indep = ServiceHandle::new(Settings::default(), logger);
        assert!(!indep.stops_service_on_exit());
    }
}
