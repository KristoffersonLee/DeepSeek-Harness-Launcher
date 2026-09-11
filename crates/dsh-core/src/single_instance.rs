//! 单实例：命名互斥体 + 命名事件（唤起已有实例）。
//!
//! v4 用 `EventWaitHandle` 做单实例，并保留同一个事件做“唤起窗口”：
//! 第二个进程 `Set()` 事件，第一个进程的等待线程收到后把窗口显示到前台。
//! v5 早期版本只做了互斥体，第二个实例**静默退出**——用户双击图标没有任何反应
//! （窗口可能在托盘或最小化状态），这是相对 v4 的功能倒退，本模块把唤起语义补回来。
//!
//! 实现要点：
//! - 互斥体用 `CreateMutexW` + `ERROR_ALREADY_EXISTS` 判定首实例；
//! - 命名空间用 `Local\`：`Global\` 需要 `SeCreateGlobalPrivilege`，
//!   受限账户下创建会直接失败（单实例保护形同虚设）；
//! - 句柄不主动释放，进程退出时由 OS 回收——这正是我们想要的：崩溃时不会误判为“已退出”。

use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject,
    SYNCHRONIZATION_ACCESS_RIGHTS,
};

/// 应用单实例互斥体名。
pub const MUTEX_NAME: &str = r"Local\DSHLauncher_SingleInstance_v5";

/// 唤起已有实例的命名事件名。
pub const ACTIVATE_EVENT_NAME: &str = r"Local\DSHLauncher_Activate_v5";

/// 请求已有实例**优雅退出**的命名事件名。
///
/// ## 为什么需要它
///
/// v5.0.0 没有任何脚本可用的退出入口——`--selftest` / `--settings` / `--guide` /
/// `--ipc-probe` 都不是「退出」。后果很实际：
/// - 发布脚本（`build.ps1` / `finish-release.ps1`）无法优雅退出启动器，
///   只能 `Stop-Process -Force`；
/// - 而**强杀会切断正在进行的 dsh 会话**（旧 Job 语义下尤为严重）。
///
/// 本事件让「退出」成为可编排的动作：`DSHLauncher.exe --quit` → 已有实例走与托盘
/// 「退出」完全相同的收尾路径（按 `service_lifecycle` 决定服务去留）。
pub const QUIT_EVENT_NAME: &str = r"Local\DSHLauncher_Quit_v5";

/// 退出请求的**回执**事件名（首个实例写完收尾结果后置位）。
///
/// ## 为什么需要回执（v5.0.0 定稿新增）
///
/// 只有「请求已送达」是不够的：`tied` 模式下退出会弹确认框，用户可以点「取消」——
/// 此时启动器**继续运行**。若 `--quit` 不管结果都返回 0，编排脚本（发布、
/// 自动化）会把「用户拒绝了退出」误判为「已退出」，随后去覆盖仍被占用的 exe。
///
/// 因此引入回执：请求方在发出 `--quit` 后**短暂等待**该事件，据此区分
/// 「已退出」「被取消」「无人响应」三种结果，并给出不同退出码（见 `main.rs`）。
pub const QUIT_ACK_EVENT_NAME: &str = r"Local\DSHLauncher_QuitAck_v5";

/// `--quit` 等待回执的最长时间。
///
/// 必须覆盖「tied 模式下的确认对话框」：用户看到弹窗后需要时间做决定。
/// 20 秒足够，且脚本仍可在超时后自行决定如何处理（超时 = 无法判定）。
pub const QUIT_ACK_TIMEOUT: Duration = Duration::from_secs(20);

/// 事件等待上限（毫秒）：仅用于短等待，避免阻塞事件循环。
const ACTIVATE_WAIT_MS: u32 = 0;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// 使用应用默认互斥体名获取单实例锁。
    ///
    /// 返回 `Ok(Some(..))` 表示本进程是首个实例；`Ok(None)` 表示已有实例在运行。
    pub fn acquire() -> windows::core::Result<Option<Self>> {
        Self::acquire_named(MUTEX_NAME)
    }

    /// 使用指定名称获取单实例锁（测试用：避免多个测试/外部实例相互干扰）。
    pub fn acquire_named(name: &str) -> windows::core::Result<Option<Self>> {
        let wide = to_wide(name);
        unsafe {
            let h = CreateMutexW(None, true, PCWSTR(wide.as_ptr()))?;
            // CreateMutexW 成功时也需检查 GetLastError：已存在时返回既有句柄
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(h);
                return Ok(None);
            }
            Ok(Some(Self(h)))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 请求已有实例优雅退出（`--quit`）的**结果**。
///
/// 区分这三种结果是必要的：编排脚本必须能分辨「已退出」与「用户取消了退出」，
/// 否则会在 exe 仍被占用时去覆盖它（并把失败误判成发布成功）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitOutcome {
    /// 请求已送达，且已有实例完成收尾并置位回执 ⇒ **确认已退出**。
    Acknowledged,
    /// 请求已送达，但等待 [`QUIT_ACK_TIMEOUT`] 内没有回执 ⇒ 无法判定
    /// （实例可能仍在运行，例如 `tied` 模式下用户把确认框一直开着）。
    Sent { waited: Duration },
    /// 没有正在运行的实例（互斥体/事件不存在）⇒ 无需退出。
    NoInstance,
}

impl QuitOutcome {
    /// 面向脚本的退出码（`0` 表示「可以认为启动器已不在运行」）。
    ///
    /// - `Acknowledged` → `0`：已确认退出；
    /// - `NoInstance` → `0`：本来就没有实例（幂等，脚本可安全继续）；
    /// - `Sent`（超时无回执）→ `3`：**无法判定**，脚本必须自行确认后再覆盖产物。
    pub fn exit_code(self) -> i32 {
        match self {
            QuitOutcome::Acknowledged | QuitOutcome::NoInstance => 0,
            QuitOutcome::Sent { .. } => 3,
        }
    }

    /// 面向用户的说明（`--build-info` 同款：GUI 子系统里 stderr 可能无效，
    /// 因此调用方除了打印还会写标记文件）。
    pub fn describe(self) -> String {
        match self {
            QuitOutcome::Acknowledged => {
                "已确认正在运行的实例完成优雅退出（服务去留按 service_lifecycle 决定）。".into()
            }
            QuitOutcome::NoInstance => "没有正在运行的实例（无需退出）。".into(),
            QuitOutcome::Sent { waited } => format!(
                "已发出退出请求，但 {} 秒内未收到回执：实例可能仍在运行\
                 （例如 tied 模式下确认框等待用户选择）。请确认后再覆盖产物。",
                waited.as_secs()
            ),
        }
    }
}

/// 首实例持有的**退出回执**事件句柄。
///
/// 首实例在 [退出收尾完成 / 确认取消退出] 两个分支上都会 `SetEvent`，
/// 使 `--quit` 的请求方不必靠猜。
pub struct QuitAckEvent(HANDLE);

impl QuitAckEvent {
    pub fn create() -> windows::core::Result<Self> {
        Self::create_named(QUIT_ACK_EVENT_NAME)
    }

    pub fn create_named(name: &str) -> windows::core::Result<Self> {
        let wide = to_wide(name);
        unsafe {
            // 手动重置（false = auto-reset）：一次回执只消费一次
            let h = CreateEventW(None, false, false, PCWSTR(wide.as_ptr()))?;
            Ok(Self(h))
        }
    }

    /// 置位回执（收尾完成后调用）。
    pub fn signal(&self) {
        unsafe {
            let _ = SetEvent(self.0);
        }
    }
}

impl Drop for QuitAckEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// 内核句柄（`HANDLE`）本身可安全跨线程传递：它不指向线程局部状态，且
// `SetEvent` / `WaitForSingleObject` 对同一句柄的并发使用是内核支持的。
// `windows` crate 把 `HANDLE` 定义成裸指针，因此这里必须显式声明。
// 生产路径上 `QuitAckEvent` 由 `main` 创建后交给事件循环闭包（跨帧持有），
// 单测里还需要跨线程置位回执——两种用法都依赖这个 `Send`。
unsafe impl Send for QuitAckEvent {}
unsafe impl Send for QuitEvent {}
unsafe impl Send for ActivationEvent {}

/// 首实例持有的“唤起事件”句柄。
///
/// 用自动重置（auto-reset）事件：每次 `SetEvent` 只唤醒一次等待，
/// 不会因为多次双击而堆积信号。
pub struct ActivationEvent(HANDLE);

impl ActivationEvent {
    /// 创建（或打开）唤起事件。首实例必须在进入事件循环前调用。
    pub fn create() -> windows::core::Result<Self> {
        Self::create_named(ACTIVATE_EVENT_NAME)
    }

    pub fn create_named(name: &str) -> windows::core::Result<Self> {
        let wide = to_wide(name);
        unsafe {
            // 手动重置 = false（自动重置），初始未触发 = false
            let h = CreateEventW(None, false, false, PCWSTR(wide.as_ptr()))?;
            Ok(Self(h))
        }
    }

    /// 非阻塞检查是否收到唤起请求。
    ///
    /// 必须在 UI 线程的事件循环里轮询（每次循环调用一次），
    /// 因此用 0 超时，绝不阻塞界面。
    pub fn poll(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, ACTIVATE_WAIT_MS) == WAIT_OBJECT_0 }
    }
}

impl Drop for ActivationEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 第二个实例调用：通知已有实例把窗口显示到前台。
///
/// 返回 `true` 表示信号已送达（已有实例确实在运行且建好了事件）；
/// 返回 `false` 表示事件不存在——说明持有互斥体的进程尚未创建事件，
/// 此时应当继续正常启动流程（互斥体句柄会在拥有者退出时释放）。
pub fn signal_existing_instance() -> bool {
    signal_named(ACTIVATE_EVENT_NAME)
}

/// 请求已有实例优雅退出（`--quit`），并**等待回执**以区分三种结果。
///
/// 为什么不能只返回 `bool`：`bool` 只能表达「信号有没有送出去」，而编排脚本真正
/// 需要知道的是「启动器**是否已经不在了**」。`tied` 模式下退出会弹确认框，
/// 用户点「取消」时启动器继续运行——此时若返回成功，脚本会去覆盖仍被占用的 exe。
pub fn request_existing_instance_quit() -> QuitOutcome {
    request_existing_instance_quit_within(QUIT_ACK_TIMEOUT)
}

/// 带自定义回执超时的版本（便于测试与需要更快失败的脚本）。
pub fn request_existing_instance_quit_within(timeout: Duration) -> QuitOutcome {
    request_quit_named(QUIT_EVENT_NAME, QUIT_ACK_EVENT_NAME, timeout)
}

/// 按名称请求退出并等待回执（测试用：避免与真实实例相互干扰）。
///
/// 顺序很重要：**先打开回执事件，再发退出信号**。反过来的话，快速退出的实例
/// 可能在两者之间就置位并销毁了回执，请求方只会看到「未触发」而误判为超时。
pub fn request_quit_named(quit_name: &str, ack_name: &str, timeout: Duration) -> QuitOutcome {
    let ack = open_event_for_wait(ack_name);
    if !signal_named(quit_name) {
        return QuitOutcome::NoInstance;
    }
    let Some(ack) = ack else {
        // 有实例（信号送达）但拿不到回执句柄：无法判定，按超时处理
        return QuitOutcome::Sent {
            waited: Duration::ZERO,
        };
    };
    let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
    let start = std::time::Instant::now();
    let r = unsafe { WaitForSingleObject(ack, ms) };
    let waited = start.elapsed();
    unsafe {
        let _ = CloseHandle(ack);
    }
    if r == WAIT_OBJECT_0 {
        QuitOutcome::Acknowledged
    } else {
        // WAIT_TIMEOUT / WAIT_FAILED / WAIT_ABANDONED：一律按「无法判定」处理（保守）
        QuitOutcome::Sent { waited }
    }
}

/// 打开一个事件用于等待（`SYNCHRONIZE` 是 `WaitForSingleObject` 所需的最小权限）。
fn open_event_for_wait(name: &str) -> Option<HANDLE> {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let wide = to_wide(name);
    unsafe {
        OpenEventW(
            SYNCHRONIZATION_ACCESS_RIGHTS(SYNCHRONIZE),
            false,
            PCWSTR(wide.as_ptr()),
        )
        .ok()
    }
}

/// `--quit` 用的退出请求事件（首实例持有）。
pub struct QuitEvent(HANDLE);

impl QuitEvent {
    pub fn create() -> windows::core::Result<Self> {
        Self::create_named(QUIT_EVENT_NAME)
    }

    pub fn create_named(name: &str) -> windows::core::Result<Self> {
        let wide = to_wide(name);
        unsafe {
            // 自动重置事件：每收到一次请求只唤醒一次
            let h = CreateEventW(None, false, false, PCWSTR(wide.as_ptr()))?;
            Ok(Self(h))
        }
    }

    /// 非阻塞检查是否收到退出请求（在事件循环里每帧调用）。
    pub fn poll(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, ACTIVATE_WAIT_MS) == WAIT_OBJECT_0 }
    }
}

impl Drop for QuitEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 按名称发送唤起信号（测试用）。
pub fn signal_named(name: &str) -> bool {
    /// `EVENT_MODIFY_STATE`：`SetEvent` 所需的最小权限。
    const EVENT_MODIFY_STATE: u32 = 0x0002;
    let wide = to_wide(name);
    unsafe {
        let Ok(h) = OpenEventW(
            SYNCHRONIZATION_ACCESS_RIGHTS(EVENT_MODIFY_STATE),
            false,
            PCWSTR(wide.as_ptr()),
        ) else {
            return false;
        };
        let ok = SetEvent(h).is_ok();
        let _ = CloseHandle(h);
        ok
    }
}

/// UTF-16 + NUL 结尾。
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 每个测试用独立名称，避免与真实运行中的启动器（持有默认互斥体）冲突。
    fn unique_name(tag: &str) -> String {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        format!(r"Local\DSHLauncher_Test_{tag}_{}_{n}", std::process::id())
    }

    #[test]
    fn first_acquire_succeeds() {
        let name = unique_name("first");
        assert!(SingleInstance::acquire_named(&name).unwrap().is_some());
    }

    #[test]
    fn second_acquire_is_declined() {
        let name = unique_name("second");
        let _first = SingleInstance::acquire_named(&name)
            .unwrap()
            .expect("首个实例应获取成功");
        assert!(SingleInstance::acquire_named(&name).unwrap().is_none());
    }

    #[test]
    fn different_names_do_not_conflict() {
        let a = unique_name("a");
        let b = unique_name("b");
        let _ha = SingleInstance::acquire_named(&a).unwrap();
        // 不同名称互不影响
        assert!(SingleInstance::acquire_named(&b).unwrap().is_some());
    }

    #[test]
    fn activation_event_roundtrip() {
        let name = unique_name("activate");
        let evt = ActivationEvent::create_named(&name).expect("事件创建失败");
        // 尚未发信号 → 不触发
        assert!(!evt.poll(), "未发信号时不应触发");
        assert!(signal_named(&name), "应能打开并触发事件");
        assert!(evt.poll(), "发信号后应触发");
        // 自动重置：一次信号只消费一次
        assert!(!evt.poll(), "自动重置事件消费后不应再次触发");
    }

    #[test]
    fn signalling_absent_event_is_false() {
        // 不存在的名称必须安全返回 false（不能 panic、不能误判为已送达）
        assert!(!signal_named(&unique_name("absent")));
    }

    /// 复现 `main.rs` 的完整单实例流程（用唯一名称，不干扰真实启动器）：
    /// 首实例 = 拿到互斥体 + 创建唤起事件；第二实例 = 拿不到互斥体 → 发信号；
    /// 首实例在事件循环里轮询到该信号。
    ///
    /// 这条链路正是 v5 早期版本缺失的“双击图标唤起已有窗口”，
    /// 也是本轮修复的重点，因此必须端到端可验证（而不是只测两个零件）。
    #[test]
    fn second_instance_activates_first_instance() {
        let mutex = unique_name("e2e_mutex");
        let event = unique_name("e2e_event");

        // ---- 首实例 ----
        let first = SingleInstance::acquire_named(&mutex)
            .expect("CreateMutexW 失败")
            .expect("首个实例应拿到互斥体");
        let activation = ActivationEvent::create_named(&event).expect("事件创建失败");
        assert!(!activation.poll(), "此时不应有唤起请求");

        // ---- 第二实例：拿不到互斥体 → 转而发信号 ----
        assert!(
            SingleInstance::acquire_named(&mutex).unwrap().is_none(),
            "第二实例不应拿到互斥体"
        );
        assert!(signal_named(&event), "第二实例应能把信号送达");
        assert!(activation.poll(), "首实例应轮询到唤起请求");

        // 信号被消费后不应重复触发（自动重置语义）
        assert!(!activation.poll());
        drop(first);
    }

    /// `--quit` 的三态语义：**只要拿不到回执就不能声称「已退出」**。
    ///
    /// 这条断言防的是编排脚本的假通过——曾出现过「`--quit` 无脑返回 0」，
    /// 于是 `tied` 模式下用户点「取消」后脚本仍去覆盖被占用的 exe。
    #[test]
    fn quit_without_instance_reports_no_instance() {
        let quit_name = unique_name("quit_absent");
        let ack_name = unique_name("quit_ack_absent");
        let outcome = request_quit_named(&quit_name, &ack_name, Duration::from_millis(100));
        assert_eq!(outcome, QuitOutcome::NoInstance);
        // 没有实例 ⇒ 退出码 0（幂等，脚本可安全继续）
        assert_eq!(outcome.exit_code(), 0);
        assert!(!outcome.describe().is_empty());
    }

    /// 有实例、请求已送达、但无人置位回执 ⇒ 必须是 `Sent`（**不得**判为已退出）。
    #[test]
    fn quit_without_ack_is_unresolved_and_nonzero() {
        let quit_name = unique_name("quit_evt");
        let ack_name = unique_name("quit_ack");
        let _quit_holder = QuitEvent::create_named(&quit_name).expect("退出事件创建失败");
        let outcome = request_quit_named(&quit_name, &ack_name, Duration::from_millis(150));
        assert!(
            matches!(outcome, QuitOutcome::Sent { .. }),
            "无回执时必须判为无法判定，实际 {outcome:?}"
        );
        assert_ne!(outcome.exit_code(), 0, "无法判定时退出码必须非 0");
    }

    /// 首实例置位回执 ⇒ 才能判为 `Acknowledged`。
    #[test]
    fn quit_with_ack_is_acknowledged() {
        let quit_name = unique_name("quit_evt_ok");
        let ack_name = unique_name("quit_ack_ok");
        let _ack_keepalive = QuitAckEvent::create_named(&ack_name).expect("回执事件创建失败");

        // 模拟首实例：另起线程等请求 → 置位回执
        let ack_for_thread = QuitAckEvent::create_named(&ack_name).expect("回执事件创建失败");
        let quit_for_thread = QuitEvent::create_named(&quit_name).expect("退出事件创建失败");
        let t = std::thread::spawn(move || {
            for _ in 0..200 {
                if quit_for_thread.poll() {
                    ack_for_thread.signal();
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        });

        let outcome = request_quit_named(&quit_name, &ack_name, Duration::from_secs(5));
        let saw = t.join().expect("等待线程不应 panic");
        assert!(saw, "首实例应收到退出请求");
        assert_eq!(
            outcome,
            QuitOutcome::Acknowledged,
            "收到回执后必须判为已退出"
        );
        assert_eq!(outcome.exit_code(), 0);
    }
}
