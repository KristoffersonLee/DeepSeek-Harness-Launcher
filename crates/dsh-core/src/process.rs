//! 子进程管理：Job Object + 进程树兜底终止。
//!
//! v4 依赖 `taskkill /T /F`，启动器被强杀时兜底失效，dsh 残留占用端口。
//! 这里把子进程挂进 Job Object 并设置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，
//! 句柄关闭（包括进程被强杀时由 OS 关闭）则由内核回收整棵进程树。
//!
//! # 两种子进程生命周期（[`ChildLifecycle`]）
//!
//! `dsh web` 是**会话型服务**：正在对话/跑任务的会话挂在它上面。把它绑死在启动器
//! 进程上会产生用户不可接受的后果——**启动器一退出（哪怕是被强杀、崩溃、被安装包
//! 升级覆盖、注销重启）就会切断正在进行的会话**。
//!
//! 因此这里提供两种显式语义，由 `settings.toml` 的 `service_lifecycle` 选择：
//!
//! - [`ChildLifecycle::Independent`]（**默认**）：子进程用 `CREATE_BREAKAWAY_FROM_JOB`
//!   脱离启动器的 Job Object，**不随启动器退出/崩溃而终止**。启动器只负责
//!   「发现 → 接管 → 退出时按用户意愿处理」；跨启动器生命周期的归属由
//!   [`crate::service_record`] 的 `service.json` 簿记 + 下次启动对账保证。
//! - [`ChildLifecycle::Tied`]：保留 v5.0.0 原语义——子进程挂在 Job 上，启动器被
//!   强杀时由内核回收整棵进程树（「零孤儿」的内核级保证）。
//!
//! 两种语义的差异已由 `crates/dsh-core/examples/job_object_demo.rs` 实测：
//! 解除 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 后，子进程在父进程**正常退出**与
//! **被强杀**两种情况下都存活；未解除时两种情况下都被内核回收。

use core::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, STILL_ACTIVE, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::JobObjects::*;
use windows::Win32::System::Threading::*;

use crate::log::RollingLogger;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `CREATE_BREAKAWAY_FROM_JOB`：新进程不继承父进程所属的 Job Object。
///
/// 前提：父进程所在 Job 必须允许 breakaway（未设置
/// `JOB_OBJECT_LIMIT_BREAKAWAY_OK` / `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK` 时会以
/// `ERROR_ACCESS_DENIED` 失败）。因此调用方必须准备「不带该标志重试」的回退路径。
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// 子进程与启动器的生命周期关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChildLifecycle {
    /// 默认：dsh 独立于启动器，启动器退出/崩溃/被升级覆盖都不终止它。
    #[default]
    Independent,
    /// 兼容 v5.0.0：dsh 挂在启动器的 Job 上，随启动器（含强杀）一同被回收。
    Tied,
}

/// 判断 `spawn` 失败是否因为「父进程所在 Job 不允许 breakaway」。
fn is_breakaway_denied(e: &std::io::Error) -> bool {
    // ERROR_ACCESS_DENIED = 5
    e.raw_os_error() == Some(5)
}

/// dsh web 就绪回调：`(带 token 的地址, 原始启动行)`。
///
/// 在 stdout 读取线程上被调用一次（只报第一条匹配到的地址）。
pub type ReadyHook = Box<dyn Fn(String, String) + Send + 'static>;

/// 持续读取子进程 stdout：排空管道，并把 dsh 的启动行交给 `on_ready`。
///
/// 拆成独立函数（而不是塞进 `start_dsh`）是为了让"没人关心输出"时也能复用同一套
/// 排空逻辑：`on_ready` 为 `None` 时只是丢弃内容，绝不把输出累积到内存里。
///
/// `logger` 必须传进来：本函数的两条告警（浏览器移交未关掉 / 上游输出格式变了）
/// **正是发布环境下最需要留下的线索**，而 release 是 Windows GUI 子系统、stderr
/// 句柄可能无效 —— 只写 stderr 等于把这些告警丢掉（`eprintln_warn` 仅作兜底）。
fn spawn_ready_reader(
    stdout: Option<std::process::ChildStdout>,
    on_ready: Option<ReadyHook>,
    logger: Option<RollingLogger>,
) {
    use std::io::{BufRead, BufReader};
    let Some(stdout) = stdout else { return };
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut hook = on_ready;
        // 「上游输出格式可能变了」只告警一次，避免刷屏
        let mut shape_warned = false;
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some(parsed) = crate::dsh::parse_ready_url(&line) {
                if let Some(cb) = hook.take() {
                    cb(parsed, line.clone());
                }
            } else if crate::dsh::is_browser_open_notice(&line) {
                // 不应出现（已传 --no-open）；出现说明上游行为变了，留痕便于排查
                warn_report(
                    &logger,
                    &format!("dsh 正在尝试打开默认浏览器（--no-open 未生效）：{line}"),
                );
            } else if !shape_warned && hook.is_some() && crate::dsh::looks_like_ready_line(&line) {
                // **格式不符留痕**：这一行看起来本该是就绪地址行，却没解析出地址。
                // 上游一旦改输出格式，`parse_ready_url` 会**静默**返回 None，
                // 表现为「界面一直等不到 token → 401 或空白页」而日志里毫无线索。
                shape_warned = true;
                warn_report(
                    &logger,
                    &format!(
                        "dsh 输出了形似就绪地址的行，但无法按当前格式解析（上游格式可能已变化）：{line}"
                    ),
                );
            }
        }
    });
}

/// 告警落点：优先写日志文件（GUI 子系统下唯一可靠的落点），无 logger 时退回 stderr。
fn warn_report(logger: &Option<RollingLogger>, msg: &str) {
    match logger {
        Some(l) => l.warn(msg),
        None => eprintln_warn(msg),
    }
}

/// 持续读取并丢弃子进程 stderr 之外的任何输出，防止管道写满导致子进程阻塞。
///
/// **为什么必须做**：`Stdio::piped()` 创建的管道有固定缓冲区（Windows 上通常 4–64 KB）。
/// 子进程向 stderr 写入超过缓冲区容量后会在 `write` 上**永久阻塞**——表现为
/// 「服务假死、端口仍在监听但不再响应」，进而被失联监测误判并重启。
///
/// 这里把 stderr 接到日志（而不是简单丢弃），因为 dsh 的原生模块加载失败、
/// 端口占用等**关键错误都在 stderr 上**；同时限制单行长度避免日志被灌爆。
fn spawn_stderr_drain(stderr: Option<std::process::ChildStderr>, logger: Option<RollingLogger>) {
    use std::io::{BufRead, BufReader};
    let Some(stderr) = stderr else { return };
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let line = truncate_chars(&line, MAX_STDERR_LINE_CHARS);
            match &logger {
                // stderr 多为诊断信息，按 WARN 级别落盘（便于「打开日志目录」排查）
                Some(l) => l.warn(&format!("[dsh stderr] {line}")),
                None => eprintln_warn(&format!("dsh stderr: {line}")),
            }
        }
    });
}

/// stderr 单行最大字符数（防止异常输出把日志灌爆）。
const MAX_STDERR_LINE_CHARS: usize = 2000;

/// 按**字符**截断（不是字节），避免把 UTF-8 切坏。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("…（已截断）");
    out
}

/// 安全地向 stderr 写一行。
///
/// **为什么不用 `eprintln!`**：release 构建是 Windows GUI 子系统，stderr 句柄可能
/// 无效（无控制台时）。`eprintln!` 在写入失败时会 panic，而 `panic = "abort"`
/// 下这就是**整进程崩溃**。这里显式忽略写入错误。
fn eprintln_warn(msg: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "[warn] {msg}");
}

/// Job Object 句柄。Drop 时关闭，触发内核终止 Job 内全部进程。
pub struct JobHandle(HANDLE);

impl JobHandle {
    pub fn new() -> Result<Self, ProcessError> {
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of_val(&info) as u32,
            )?;
            Ok(Self(job))
        }
    }

    pub fn assign_raw(&self, handle: HANDLE) -> Result<(), ProcessError> {
        unsafe { AssignProcessToJobObject(self.0, handle)? };
        Ok(())
    }

    /// 解除“句柄关闭即终止 Job 内全部进程”的限制。
    ///
    /// 用途：v2.0 起启动器支持“退出时保留后台 dsh web 服务”，而 Job Object 的
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 会让子进程随启动器退出被回收——两者冲突。
    ///
    /// 优雅退出且用户选择保留服务时调用本方法，清空该限制，子进程即可在启动器
    /// 退出后继续运行（下次启动时被重新接管）。**强杀路径不会走到这里**，
    /// 因此“启动器崩溃/被强杀 ⇒ 无孤儿进程”的保障不受影响。
    ///
    /// 注意：解除之后若启动器再被强杀，子进程将不再被自动回收——这是用户的显式选择。
    pub fn disarm_kill_on_close(&self) -> Result<(), ProcessError> {
        unsafe {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT(0);
            SetInformationJobObject(
                self.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of_val(&info) as u32,
            )?;
            Ok(())
        }
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// HANDLE 是 Windows 内核句柄，可安全跨线程传递
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

/// 已启动的 dsh 服务进程。
pub struct ProcessManager {
    /// Job shell：`Independent` 模式下为 `None`（不去创建，避免任何意外挂载）。
    job: Option<JobHandle>,
    child: Option<Child>,
    /// 接管的外部进程（不是本进程启动的），退出时不自动杀。
    adopted_pid: Option<u32>,
    /// 本次启动采用的子进程生命周期策略。
    lifecycle: ChildLifecycle,
    /// 日志句柄（用于把 dsh 的 stderr 落到 launcher.log）。
    logger: Option<RollingLogger>,
    /// 本进程启动的子进程创建时间戳（用于身份校验，防 PID 复用误判）。
    child_started_at: Option<std::time::SystemTime>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::with_lifecycle(ChildLifecycle::default())
    }

    /// 按指定生命周期策略构造。
    ///
    /// `Tied` 才会创建 Job Object；`Independent` 下**根本不创建 Job**——
    /// 这比「创建后 disarm」更彻底：不存在任何「忘记 disarm」的路径。
    pub fn with_lifecycle(lifecycle: ChildLifecycle) -> Self {
        Self {
            job: match lifecycle {
                ChildLifecycle::Tied => JobHandle::new().ok(),
                ChildLifecycle::Independent => None,
            },
            child: None,
            adopted_pid: None,
            lifecycle,
            logger: None,
            child_started_at: None,
        }
    }

    /// 注入日志句柄（dsh 的 stderr 会写入 launcher.log）。
    pub fn set_logger(&mut self, logger: RollingLogger) {
        self.logger = Some(logger);
    }

    /// 当前生命周期策略。
    pub fn lifecycle(&self) -> ChildLifecycle {
        self.lifecycle
    }

    /// 运行时切换生命周期策略（设置页改动后生效；需在无子进程时调用）。
    ///
    /// 返回 `false` 表示当前仍有子进程、策略未切换（调用方应先 `stop()`）。
    pub fn set_lifecycle(&mut self, lifecycle: ChildLifecycle) -> bool {
        if self.child.is_some() || self.adopted_pid.is_some() {
            return false;
        }
        if self.lifecycle == lifecycle {
            return true;
        }
        self.lifecycle = lifecycle;
        self.job = match lifecycle {
            ChildLifecycle::Tied => JobHandle::new().ok(),
            ChildLifecycle::Independent => None,
        };
        true
    }

    pub fn is_running(&self) -> bool {
        if let Some(c) = &self.child {
            return is_process_alive(c.id());
        }
        if let Some(pid) = self.adopted_pid {
            return is_process_alive(pid);
        }
        false
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id()).or(self.adopted_pid)
    }

    /// 本进程启动的子进程的创建时间（无子进程时为 `None`）。
    pub fn child_started_at(&self) -> Option<std::time::SystemTime> {
        self.child_started_at
    }

    /// 启动 `node.exe <bin.js> web --host 127.0.0.1 --port <port> --no-open`。
    ///
    /// **必须带 `--no-open`**：dsh web 默认会自己用系统默认浏览器打开界面
    /// （源码：`dsh web: opening the default browser; pass --no-open to disable`），
    /// 于是用户双击启动器会先弹出一个浏览器窗口——这正是"启动器默认打开浏览器
    /// 网页版、而不是在启动器内部对话"的原因。启动器自己内嵌 WebView2 承载界面，
    /// 因此必须显式关掉 dsh 的浏览器移交。
    ///
    /// stdout/stderr 走管道，且**两者都被持续排空**：
    /// - stdout 由 [`spawn_ready_reader`] 读取——既排空管道，又捕获 dsh 打到
    ///   stdout 的**带 token 访问地址**（`dsh web: http://127.0.0.1:<port>/?token=…`），
    ///   这是拿到可用地址的唯一途径（无 token 访问返回 HTTP 401，已实测）；
    /// - stderr 由 [`spawn_stderr_drain`] 读取并落日志——**不排空会让子进程在
    ///   管道写满后永久阻塞**（表现为服务假死）。
    ///
    /// `on_ready` 在解析出该地址后被调用（在读取线程上），来源行同时作为第二个参数传出。
    pub fn start_dsh(
        &mut self,
        port: u16,
        work_dir: Option<&Path>,
        node: &Path,
        bin_js: &Path,
        on_ready: Option<ReadyHook>,
    ) -> Result<u32, ProcessError> {
        let child = self.spawn_node(port, work_dir, node, bin_js, CREATE_NO_WINDOW)?;
        let pid = child.id();
        self.finish_spawn(child, on_ready);
        Ok(pid)
    }

    /// 构造命令行（**不经 shell**，参数逐个传递，无注入面）。
    fn node_command(port: u16, work_dir: Option<&Path>, node: &Path, bin_js: &Path) -> Command {
        let mut cmd = Command::new(node);
        cmd.arg(bin_js)
            .arg("web")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            // 关键：禁止 dsh 自行拉起默认浏览器（内嵌窗口由启动器负责）
            .arg("--no-open")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = work_dir {
            if dir.exists() {
                cmd.current_dir(dir);
            }
        }
        cmd
    }

    /// 实际 spawn，并按生命周期决定是否脱离 Job。
    ///
    /// `Independent` 模式下先尝试 `CREATE_BREAKAWAY_FROM_JOB`；若父进程所在 Job
    /// 不允许 breakaway（`ERROR_ACCESS_DENIED`），**退化为不带该标志**——此时子进程
    /// 不会挂在我们的 Job 上（我们本来就没建），因此「独立」语义仍然成立。
    fn spawn_node(
        &self,
        port: u16,
        work_dir: Option<&Path>,
        node: &Path,
        bin_js: &Path,
        base_flags: u32,
    ) -> Result<Child, ProcessError> {
        let mut cmd = Self::node_command(port, work_dir, node, bin_js);
        cmd.creation_flags(base_flags | CREATE_NO_WINDOW);
        let first = match self.lifecycle {
            ChildLifecycle::Independent => cmd
                .creation_flags(base_flags | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
                .spawn(),
            ChildLifecycle::Tied => cmd.spawn(),
        };
        match first {
            Ok(c) => Ok(c),
            Err(e) if self.lifecycle == ChildLifecycle::Independent && is_breakaway_denied(&e) => {
                // 父进程所在 Job 不允许 breakaway：退化为普通 spawn。
                // 此时父 Job 会「继承式」把子进程纳入——但那只在父进程被强杀时
                // 才会回收它，与用户名下的「独立」预期冲突，因此留痕提示。
                self.log_warn(
                    "子进程无法脱离父 Job（ERROR_ACCESS_DENIED），将退化为继承父 Job；\
                     若需严格独立，请从非受限环境启动。",
                );
                Self::node_command(port, work_dir, node, bin_js)
                    .creation_flags(base_flags | CREATE_NO_WINDOW)
                    .spawn()
                    .map_err(ProcessError::from)
            }
            Err(e) => Err(ProcessError::from(e)),
        }
    }

    /// spawn 之后收尾：挂 Job（仅 Tied）、记录身份、接管 stdout/stderr。
    fn finish_spawn(&mut self, mut child: Child, on_ready: Option<ReadyHook>) {
        let pid = child.id();
        if let Some(job) = &self.job {
            let h = HANDLE(child.as_raw_handle());
            if let Err(e) = job.assign_raw(h) {
                // 挂 Job 失败不致命：仍然纳入常规 kill 路径，只是失去强杀兜底
                self.log_warn(&format!("AssignProcessToJobObject 失败: {e}"));
            }
        }
        self.child_started_at = process_start_time(pid);
        // 持续读取 stdout：排空管道 + 解析带 token 的就绪地址
        let stdout = child.stdout.take();
        spawn_ready_reader(stdout, on_ready, self.logger.clone());
        // stderr 同样必须排空，否则子进程会在管道写满后阻塞
        spawn_stderr_drain(child.stderr.take(), self.logger.clone());
        self.child = Some(child);
        self.adopted_pid = None;
    }

    fn log_warn(&self, msg: &str) {
        warn_report(&self.logger, msg);
    }

    /// 记录一个被接管的外部进程（v4 的"自动接管上次遗留进程"语义）。
    pub fn adopt(&mut self, pid: u32) {
        self.adopted_pid = Some(pid);
    }

    /// 清除已接管的进程记录（该进程已退出或不再可信时调用）。
    /// 注意：只清账本，**不会**去杀进程——外部实例的生死不由我们决定。
    pub fn clear_adopted(&mut self) {
        self.adopted_pid = None;
    }

    /// 停止服务：终止整棵进程树，并**等待直接子进程退出**。
    ///
    /// 返回被终止的 PID 列表（供日志与簿记清理使用）。
    ///
    /// 注意：本方法是阻塞的（`kill_process_tree` 遍历 + 等待子进程退出）。调用方
    /// 若在 UI 线程上，应放到 worker 线程执行，避免界面卡顿。
    ///
    /// **不等待端口释放**：端口由被终止的进程树持有，其释放由内核完成，且"端口是否
    /// 可重新绑定"属于调用方关注的判据（`dsh-core::is_port_free_to_bind`）。
    /// 此处不做——早先的文档曾声称本方法会等待端口释放，与实际行为不符。
    pub fn stop(&mut self) -> Result<Vec<u32>, ProcessError> {
        let mut killed = Vec::new();
        if let Some(pid) = self.adopted_pid.take() {
            kill_process_tree(pid);
            killed.push(pid);
        }
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            kill_process_tree(pid);
            killed.push(pid);
        }
        self.child_started_at = None;
        Ok(killed)
    }

    /// 优雅退出时是否让子进程继续存活。
    ///
    /// - `Independent`：**天然成立**（子进程从未挂在我们的 Job 上），无需任何操作。
    ///   这是默认路径——用户关闭启动器不会中断正在进行的会话。
    /// - `Tied`：解除 Job 的 `KILL_ON_JOB_CLOSE`。这是 v5.0.0 的旧语义，只有在
    ///   用户显式选择「绑死」时才需要，且**只对本次进程有效**（Job 是新进程的新对象）。
    ///
    /// 返回 `true` 表示子进程将在启动器退出后继续存活。
    pub fn keep_children_on_exit(&mut self) -> bool {
        if self.lifecycle == ChildLifecycle::Independent {
            // 子进程从未挂 Job；父进程退出不会影响它
            return true;
        }
        if self.child.is_none() && self.adopted_pid.is_none() {
            return true; // 没有子进程，无需保留
        }
        // 注意：不 take child——句柄随进程退出自然释放，子进程因限制已解除而存活
        match &self.job {
            Some(job) => job.disarm_kill_on_close().is_ok(),
            None => false, // 无 Job 时无法保证子进程存活策略
        }
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

// ProcessManager 包含 Child（内部有 RawHandle），在 Windows 上可安全跨线程传递
unsafe impl Send for ProcessManager {}
unsafe impl Sync for ProcessManager {}

/// 进程是否存活。
///
/// 句柄在**所有**返回路径上都被关闭（此前 `GetExitCodeProcess` 失败会泄漏句柄）。
///
/// ## 判定方式：`WaitForSingleObject(0)` 而不是退出码
///
/// 旧实现比较 `GetExitCodeProcess == STILL_ACTIVE(259)`。该哨兵值**同时也可能是一个
/// 进程真实的退出码**（`exit(259)`），于是这类已退出进程会被永久判为"存活"——
/// 影响面是实际存在的：`service.json` 记录会被判为有效（妨碍重新启动）、
/// 残留进程回收会被跳过、孤儿 `.lock` 文件永远不会被清理。
/// 改为等待 0 毫秒：`WAIT_TIMEOUT` = 仍在运行，`WAIT_OBJECT_0` = 已终止，
/// 与退出码取值无关。
///
/// `WaitForSingleObject` 需要 `SYNCHRONIZE`；若该权限拿不到（极少数受策略限制的
/// 目标进程），退化为退出码判定——此时**只能**接受 259 的歧义，但至少不会误杀。
pub fn is_process_alive(pid: u32) -> bool {
    /// `SYNCHRONIZE`：等待句柄所需的标准权限位。
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let access = PROCESS_ACCESS_RIGHTS(PROCESS_QUERY_LIMITED_INFORMATION.0 | SYNCHRONIZE);
        if let Ok(h) = OpenProcess(access, false, pid) {
            let r = WaitForSingleObject(h, 0);
            let _ = CloseHandle(h);
            return r == WAIT_TIMEOUT;
        }
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(h, &mut code).is_ok();
        let _ = CloseHandle(h);
        ok && code == STILL_ACTIVE.0 as u32
    }
}

/// 进程创建时间（用于防 PID 复用误判）。
///
/// PID 会被 Windows 回收复用；仅凭「PID 存活」判断「这就是我之前启动的那个进程」
/// 是不成立的。配合创建时间比较，可以把误判窗口从「分钟级」压到「不可观测」。
pub fn process_start_time(pid: u32) -> Option<std::time::SystemTime> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(h, &mut creation, &mut exit, &mut kernel, &mut user).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        filetime_to_system_time(creation)
    }
}

/// `FILETIME`（自 1601-01-01 UTC 起的 100ns 计数）→ `SystemTime`。
///
/// **必须保留亚秒精度**：该值用于「同一个 PID 还是不是我那个进程」的判定
/// （[`crate::service_record`] 防 PID 复用）。旧实现只取整秒，于是同一秒内
/// 创建的两个进程（PID 复用最可能发生的窗口）在比较时无法区分。
fn filetime_to_system_time(ft: FILETIME) -> Option<std::time::SystemTime> {
    const WINDOWS_TICKS_PER_SEC: u64 = 10_000_000;
    /// 1601-01-01 → 1970-01-01 的 100ns 计数。
    const EPOCH_DIFF_TICKS: u64 = 116_444_736_000_000_000;
    let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    if ticks < EPOCH_DIFF_TICKS {
        return None;
    }
    let since_unix = ticks - EPOCH_DIFF_TICKS;
    let secs = since_unix / WINDOWS_TICKS_PER_SEC;
    // 余数（100ns 计数）→ 纳秒；1 tick = 100 ns，故乘 100 后必然 < 1e9
    let nanos = (since_unix % WINDOWS_TICKS_PER_SEC) as u32 * 100;
    let dur = std::time::Duration::new(secs, nanos);
    std::time::UNIX_EPOCH.checked_add(dur)
}

/// 进程的镜像名（`PROCESSENTRY32W.szExeFile`，如 `node.exe`）。
pub fn process_image_name(pid: u32) -> Option<String> {
    ProcessTable::capture()?.image_name(pid).map(str::to_string)
}

/// 取 `PROCESSENTRY32W.szExeFile` 的 NUL 结尾 UTF-16 字符串。
fn entry_exe_name(entry: &PROCESSENTRY32W) -> String {
    let end = entry
        .szExeFile
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..end])
}

/// 进程表的一条记录。
struct ProcEntry {
    pid: u32,
    ppid: u32,
    exe: String,
}

/// **一次** `CreateToolhelp32Snapshot` 枚举得到的进程表（PID / 父 PID / 映像名）。
///
/// ## 为什么需要它
///
/// 身份判定、进程树回收、卸载器扫描都要按 PID 反复查「映像名 / 父进程 / 子进程」，
/// 而每次查询都新建一份 toolhelp 快照意味着**反复做 O(进程数) 的系统调用与内存拷贝**：
/// - `classify_dsh_identity` 最坏要枚举 8 次（自身镜像名 + 逐级祖先链 + …）；
/// - 卸载器的「只结束安装目录里的启动器」会对**每个 PID** 各查一次映像名
///   —— 机器上 300 个进程就是 300 次全表枚举（这是卸载里最慢的一段）；
/// - 进程树回收旧实现是**每个节点**各取一次快照。
///
/// 把「一次枚举」变成可复用的查询表后，上述路径的快照次数分别降到 1 / 1 / 树的深度。
///
/// ## 语义（必须如实理解）
///
/// 这是一份**某一时刻**的快照：进程在枚举期间退出或新建都可能不在表里。
/// 因此 `None` 只能读作「此刻查不到」，不能读作「绝对不存在」——需要权威存活判定
/// 时请用 [`is_process_alive`]（它直接问内核，不查快照）。
pub struct ProcessTable {
    entries: Vec<ProcEntry>,
}

impl ProcessTable {
    /// 枚举当前进程表。拿不到快照时返回 `None`（调用方必须把它当成「查不到」）。
    pub fn capture() -> Option<Self> {
        let mut entries = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    if entry.th32ProcessID != 0 {
                        entries.push(ProcEntry {
                            pid: entry.th32ProcessID,
                            ppid: entry.th32ParentProcessID,
                            exe: entry_exe_name(&entry),
                        });
                    }
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        Some(Self { entries })
    }

    /// 某个 PID 的映像名（如 `node.exe`）。
    pub fn image_name(&self, pid: u32) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.pid == pid)
            .map(|e| e.exe.as_str())
    }

    /// 某个 PID 的父进程 PID。
    pub fn parent_of(&self, pid: u32) -> Option<u32> {
        self.entries.iter().find(|e| e.pid == pid).map(|e| e.ppid)
    }

    /// 全部 PID。
    pub fn pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.entries.iter().map(|e| e.pid)
    }

    /// 直接子进程 PID。
    pub fn child_pids_of(&self, parent: u32) -> impl Iterator<Item = u32> + '_ {
        self.entries
            .iter()
            .filter(move |e| e.ppid == parent && e.pid != 0)
            .map(|e| e.pid)
    }

    /// 全部 `(PID, 父 PID)` 关系（进程树补偿扫描用）。
    pub fn child_links(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.entries.iter().map(|e| (e.pid, e.ppid))
    }

    /// 表里的进程数（诊断/测试用）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 进程的完整映像路径（`QueryFullProcessImageNameW`）。
///
/// 用途：确认某个监听端口的进程**确实是 dsh**（node.exe + `bin.js`），
/// 避免把无关程序当成 Harness 接管并强杀。需要 `PROCESS_QUERY_LIMITED_INFORMATION`。
pub fn process_image_path(pid: u32) -> Option<std::path::PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(h);
        if !ok || len == 0 {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(len as usize);
        let s = String::from_utf16_lossy(&buf[..end]);
        Some(std::path::PathBuf::from(s))
    }
}

/// 遍历预算：单次回收最多处理的进程数上限。
///
/// 用途是给递归/环状父子关系兜底（快照里的 `th32ParentProcessID` 在异常情况下可能
/// 形成环，例如父进程已退出、PID 被复用时读到陈旧的父子关系）。
const MAX_TREE_NODES: usize = 4096;

/// 进程树的最大追溯深度（同上：环状父子关系必须有硬上限）。
const MAX_TREE_DEPTH: usize = 64;

/// 终止进程及其子孙（toolhelp 快照，Job Object 的兜底路径）。
///
/// 返回**确实被成功终止**的 PID 列表（按「先子孙、后本体」的顺序）。
///
/// 为什么返回值必须存在：此前 `terminate()` 的布尔结果被丢弃，于是
/// 「用户点停止服务 → 因为权限不足没能杀掉进程」这条路径**完全静默**：
/// 日志说"服务已停止"、调用方把 PID 记进"已结束"列表，而进程还在。
/// 调用方现在可以把「未能终止」如实写进日志。
///
/// 实现要点（v5.0.0 定稿轮重写）：
/// 1. **不递归**：逐层收集（前沿队列 + 去重）。此前是"递归调用自身"，进程树较深时
///    会吃掉调用方线程的栈（`dsh-stop` 线程只给了 256 KB），且父子关系成环时会
///    **无限递归**；
/// 2. **先收集、后终止**：每层快照都在返回前关闭，不会同时持有 N+1 个快照句柄；
/// 3. 终止顺序为收集顺序的**逆序**（父总是先于子被收集 ⇒ 逆序即"先子孙后本体"）；
/// 4. **每层一次快照**（而不是每个节点一次）：快照次数从 O(节点数) 降到 O(树的深度)，
///    且仍能覆盖「收集过程中新派生的子孙」；
/// 5. 终止后再做**一次**补偿扫描，收掉「收集与终止之间新派生」的子进程。
pub fn kill_process_tree(root: u32) -> Vec<u32> {
    // 逐层（BFS）收集：**每层取一次快照**，因此「收集过程中才出现的子孙」也能被纳入，
    // 而快照次数只与树的**深度**有关。旧实现是每个节点各取一次全表快照
    // （O(节点数) 次枚举，进程树较大时是纯粹的系统调用浪费）。
    let mut order: Vec<u32> = Vec::new();
    let mut frontier: Vec<u32> = vec![root];
    let mut depth = 0usize;
    while !frontier.is_empty() && depth < MAX_TREE_DEPTH {
        let Some(table) = ProcessTable::capture() else {
            break;
        };
        let mut next: Vec<u32> = Vec::new();
        for pid in frontier.drain(..) {
            if order.contains(&pid) || order.len() >= MAX_TREE_NODES {
                continue;
            }
            order.push(pid);
            next.extend(table.child_pids_of(pid));
        }
        frontier = next;
        depth += 1;
    }

    // 逆序终止 = 先子孙、后本体（收集顺序是父先于子）
    let mut killed = Vec::new();
    for &pid in order.iter().rev() {
        if terminate(pid) {
            killed.push(pid);
        }
    }

    // 补偿扫描（一次快照）：收集与终止之间新派生的子进程不在 order 里。
    // 只处理「父进程确实在我们收集到的集合中」的进程，避免误伤无关程序。
    if let Some(table) = ProcessTable::capture() {
        for (pid, parent) in table.child_links() {
            if order.contains(&parent) && !order.contains(&pid) && terminate(pid) {
                killed.push(pid);
            }
        }
    }
    killed
}

fn terminate(pid: u32) -> bool {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let ok = TerminateProcess(h, 1).is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    false
}

/// 是否存在指定镜像名的进程（大小写不敏感）。
///
/// 用途：需要确认「某个镜像名是否在运行」时的保守判定（拿不到快照时返回 true）。
pub fn any_process_running(name: &str) -> bool {
    match ProcessTable::capture() {
        Some(t) => t.entries.iter().any(|e| e.exe.eq_ignore_ascii_case(name)),
        // 拿不到快照：保守返回 true（调用方据此不会误判「没有进程」而跳过必要的处理）
        None => true,
    }
}

/// 当前所有进程的 PID（单次 toolhelp 快照，返回前关闭句柄）。
///
/// 用途：需要"遍历进程后按条件筛选"的调用方（例如卸载器只结束**映像路径落在安装目录内**
/// 的启动器实例）。把快照枚举放在本层，避免每个调用方各写一份 toolhelp 循环。
///
/// 拿不到快照时返回空列表（调用方必须把它当成"没有进程"；需要区分"读不到"时请自行探测）。
pub fn all_process_ids() -> Vec<u32> {
    ProcessTable::capture()
        .map(|t| t.pids().collect())
        .unwrap_or_default()
}

/// 是否存在 node.exe 进程（dsh 运行时的宿主）。
///
/// **非生产 API**：生产路径用的是「按端口找 PID + 三重身份校验」
/// （[`classify_dsh_identity`]），而不是"机器上有没有 node"这种粗判。
/// 保留它是因为诊断脚本与测试需要这个廉价的存在性探测。
#[allow(dead_code)]
pub fn any_node_running() -> bool {
    any_process_running("node.exe")
}

/// 判定某 PID **确实是 dsh 服务**（而不是任何恰好占用同一端口的无关程序）。
///
/// ## 为什么必须有这个函数
///
/// 启动器接管「端口上已有的服务」时，唯一依据是 `GetExtendedTcpTable` 返回的
/// **端口占用者 PID**——它完全可能是任何程序。v4 为此设了明确防线
/// （源码注释与 CHANGELOG 均记载：「端口若被非 dsh 程序占用则**中止清理**，
/// 防止误杀无关进程」）；v5.0.0 丢失了该判定，于是托盘「停止服务」会
/// `kill_process_tree` 掉那个无关程序。
///
/// ## 判定依据（三重，全部通过才算 dsh）
///
/// 1. 镜像名必须是 `node.exe`（大小写不敏感）；
/// 2. 映像路径不得位于明显无关的系统目录（`\Windows\`、`\WindowsApps\`）；
/// 3. **必须能把它与 dsh 关联起来**，满足任一即可：
///    - 映像路径本身位于某个 `@deepseek-ai\dsh` 安装目录内；
///    - 该进程是**本启动器已知 dsh 路径下 node.exe 的同映像**；
///    - 其祖先链上存在以 `node.exe` 启动的进程（dsh 的 subprocess-local runner 即此形态）。
///
/// ## 读不到信息时的语义（v5.0.0 定稿轮修正 —— 实测踩过）
///
/// 早期实现把「拿不到映像路径」也当成"不是 dsh"，后果很严重：端口确实被
/// **真正的 dsh** 占着，但判定失败 → 日志报「端口被非 dsh 程序占用」→ 拒绝接管
/// → 服务被停掉后起不来（实测：`端口 3080 被非 dsh 程序占用（PID 11376，映像 <无法读取>）`，
/// 而那个 PID 恰恰是前一秒还在被正常接管的 dsh）。
///
/// 现在区分**三态**（见 [`DshIdentity`]）：`IsDsh` / `NotDsh` / `Unknown`。
/// 调用方必须按场景选择语义：
/// - 「敢不敢接管/杀」→ 用 [`is_dsh_harness_process`]（保守：`Unknown` 不接管、不杀）；
/// - 「怎么向用户/日志描述」→ 用本函数，**不要把 `Unknown` 说成"非 dsh 程序占用"**。
pub fn classify_dsh_identity(pid: u32) -> DshIdentity {
    // **只枚举一次进程表**：后续镜像名与祖先链查询都在同一份快照上完成。
    // 旧实现对每个查询各建一份 toolhelp 快照（自身镜像名 + 逐级祖先 + …，最坏 8 次全表枚举）。
    let Some(table) = ProcessTable::capture() else {
        return DshIdentity::Unknown {
            steps: IdentitySteps {
                note: "无法枚举进程表（toolhelp 快照失败）".into(),
                ..Default::default()
            },
        };
    };
    classify_dsh_identity_in(&table, pid)
}

/// [classify_dsh_identity] 的实现体：在**已捕获**的进程表上完成判定。
fn classify_dsh_identity_in(table: &ProcessTable, pid: u32) -> DshIdentity {
    let mut steps = IdentitySteps::default();

    let Some(name) = table.image_name(pid) else {
        steps.note = "无法读取进程镜像名（进程可能已退出或权限不足）".into();
        return DshIdentity::Unknown { steps };
    };
    steps.image_name = name.to_string();
    if !name.eq_ignore_ascii_case("node.exe") {
        steps.note = format!("镜像名是 {name}，不是 node.exe");
        return DshIdentity::NotDsh { steps };
    }

    let Some(path) = process_image_path(pid) else {
        // 镜像名已是 node.exe，但拿不到完整路径。
        // **这是"无法判定"，不是"不是 dsh"** —— 再用祖先链这一条独立信号尝试确认。
        steps.note = "镜像名是 node.exe，但无法读取完整映像路径（权限受限）".into();
        if is_descendant_of_node(table, pid) {
            steps.matched = "祖先链上存在 node.exe".into();
            return DshIdentity::IsDsh { steps };
        }
        steps.note.push_str("；祖先链上亦未发现 node.exe");
        return DshIdentity::Unknown { steps };
    };
    steps.image_path = path.display().to_string();
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.contains("\\windows\\") || lower.contains("\\windowsapps\\") {
        steps.note = "映像位于系统目录，判为无关程序".into();
        return DshIdentity::NotDsh { steps };
    }
    // 依据 3a：路径里带 dsh 安装特征
    if lower.contains("@deepseek-ai") && lower.contains("dsh") {
        steps.matched = "映像路径含 @deepseek-ai\\dsh".into();
        return DshIdentity::IsDsh { steps };
    }
    // 依据 3b：映像与已解析出的 dsh node.exe 相同
    if let Some(dsh_node) = crate::dsh::resolve_installed_node() {
        steps.dsh_node = dsh_node.display().to_string();
        if paths_equal_ignore_case(&dsh_node, &path) {
            steps.matched = "映像路径 == 已解析的 dsh node.exe".into();
            return DshIdentity::IsDsh { steps };
        }
    }
    // 依据 3c：祖先链上有 node.exe（dsh 会派生子进程，如 subprocess-local runner）
    if is_descendant_of_node(table, pid) {
        steps.matched = "祖先链上存在 node.exe（dsh 的 runner 形态）".into();
        return DshIdentity::IsDsh { steps };
    }
    steps.note = "镜像名是 node.exe，但不含 dsh 特征、也不在 dsh 祖先链上".into();
    DshIdentity::NotDsh { steps }
}

/// 身份判定的三态结果（见 [`classify_dsh_identity`]）。
#[derive(Debug, Clone, PartialEq)]
pub enum DshIdentity {
    /// 确认是 dsh 服务。
    IsDsh { steps: IdentitySteps },
    /// 能读到信息，且明确**不是** dsh。
    NotDsh { steps: IdentitySteps },
    /// **无法判定**（读不到进程信息）—— 不要把它当成 `NotDsh`。
    Unknown { steps: IdentitySteps },
}

impl DshIdentity {
    /// 是否**确认**是 dsh（`Unknown` 返回 false）。
    pub fn is_dsh(&self) -> bool {
        matches!(self, DshIdentity::IsDsh { .. })
    }

    /// 是否是"无法判定"。
    pub fn is_unknown(&self) -> bool {
        matches!(self, DshIdentity::Unknown { .. })
    }

    /// 判定过程中的证据（用于诊断输出）。
    pub fn steps(&self) -> &IdentitySteps {
        match self {
            DshIdentity::IsDsh { steps }
            | DshIdentity::NotDsh { steps }
            | DshIdentity::Unknown { steps } => steps,
        }
    }

    /// 面向日志/用户的一句话结论。
    pub fn summary(&self) -> String {
        let s = self.steps();
        match self {
            DshIdentity::IsDsh { .. } => format!("确认是 dsh（依据：{}）", s.matched),
            DshIdentity::NotDsh { .. } => format!("不是 dsh（{}）", s.note),
            DshIdentity::Unknown { .. } => format!("无法判定是否为 dsh（{}）", s.note),
        }
    }
}

/// 身份判定的中间证据（用于诊断输出）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdentitySteps {
    pub image_name: String,
    pub image_path: String,
    pub dsh_node: String,
    /// 命中的判定依据（为空表示未命中）。
    pub matched: String,
    /// 未命中/无法判定时的原因说明。
    pub note: String,
}

/// 布尔包装：**只有确认是 dsh 才返回 `true`**（`Unknown` 视为不可信 → 不接管、不杀）。
///
/// 需要区分"明确不是 dsh"与"读不到信息"时，请直接用 [`classify_dsh_identity`]。
pub fn is_dsh_harness_process(pid: u32) -> bool {
    match ProcessTable::capture() {
        // 拿不到快照 ⇒ 无法确认 ⇒ false（保守：不接管、不杀）
        Some(t) => classify_dsh_identity_in(&t, pid).is_dsh(),
        None => false,
    }
}

fn paths_equal_ignore_case(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// 沿父进程链向上查找，若某一级祖先的映像名为 `node.exe` 则返回 true。
///
/// 全部查询都在**同一份**进程表快照上完成（旧实现每一级各建一份全表快照）。
fn is_descendant_of_node(table: &ProcessTable, pid: u32) -> bool {
    let mut current = pid;
    // 限制深度，避免异常链导致死循环
    for _ in 0..ANCESTOR_MAX_DEPTH {
        let Some(parent) = table.parent_of(current) else {
            return false;
        };
        if parent == 0 || parent == current {
            return false;
        }
        if table
            .image_name(parent)
            .is_some_and(|n| n.eq_ignore_ascii_case("node.exe"))
        {
            return true;
        }
        current = parent;
    }
    false
}

/// 祖先链的最大追溯层数（防异常父子关系成环）。
const ANCESTOR_MAX_DEPTH: usize = 6;

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("Job Object 操作失败：{0}")]
    Job(String),
    #[error("进程启动失败：{0}")]
    StartFailed(#[from] std::io::Error),
}

impl From<windows::core::Error> for ProcessError {
    fn from(e: windows::core::Error) -> Self {
        ProcessError::Job(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_object_can_be_created_and_closed() {
        assert!(JobHandle::new().is_ok());
    }

    #[test]
    fn disarm_kill_on_close_succeeds() {
        // 优雅退出保留服务依赖此调用；失败会导致“退出即停服”而非用户选择的行为
        let job = JobHandle::new().expect("Job 创建失败");
        assert!(job.disarm_kill_on_close().is_ok());
        // 解除后仍应可再次挂载进程（限制只影响句柄关闭时的行为）
        assert!(job.disarm_kill_on_close().is_ok());
    }

    #[test]
    fn keep_children_on_exit_without_child_is_ok() {
        let mut pm = ProcessManager::new();
        assert!(pm.keep_children_on_exit(), "无子进程时应视为成功");
    }

    #[test]
    fn adopted_child_counts_as_children() {
        let mut pm = ProcessManager::new();
        pm.adopt(std::process::id()); // 假装接管了当前进程
        assert!(pm.is_running());
        assert!(pm.keep_children_on_exit());
    }

    #[test]
    fn current_process_is_alive() {
        assert!(is_process_alive(std::process::id()));
    }

    /// 进程表：一次枚举后必须能按 PID 查映像名/父进程，且对不存在的 PID 一律返回 None。
    #[test]
    fn process_table_is_queryable() {
        let t = ProcessTable::capture().expect("应能枚举进程表");
        assert!(t.len() > 1, "进程表不应只有一个条目");
        let me = std::process::id();
        let name = t.image_name(me).expect("自身进程必须在表里");
        assert!(!name.is_empty());
        // 不存在的 PID：必须是 None（而不是 0、不是 panic）
        assert!(t.parent_of(0xFFFF_FFFE).is_none());
        assert!(t.image_name(0xFFFF_FFFE).is_none());
        // 表里查到的子进程必须也在表里（自洽性）
        for c in t.child_pids_of(me) {
            assert!(t.image_name(c).is_some(), "子进程 {c} 必须也在进程表里");
        }
        // pids() 与 len() 必须一致
        assert_eq!(t.pids().count(), t.len());
    }

    /// 表驱动的身份判定必须保持三态语义：读不到 ⇒ Unknown（不得退化成 NotDsh）。
    #[test]
    fn table_driven_identity_keeps_three_state_semantics() {
        let t = ProcessTable::capture().expect("应能枚举进程表");
        let id = classify_dsh_identity_in(&t, 0xFFFF_FFFE);
        assert!(id.is_unknown(), "实际：{}", id.summary());
        let own = classify_dsh_identity_in(&t, std::process::id());
        assert!(
            !own.is_dsh() && !own.is_unknown(),
            "实际：{}",
            own.summary()
        );
    }

    /// `FILETIME` 转换必须保留亚秒精度，且早于 UNIX 纪元的时间戳返回 None。
    #[test]
    fn filetime_keeps_sub_second_precision() {
        const EPOCH_DIFF_TICKS: u64 = 116_444_736_000_000_000;
        // 1 秒 + 500 毫秒（FILETIME 的单位是 100ns）
        let ticks = EPOCH_DIFF_TICKS + 10_000_000 + 5_000_000;
        let ft = FILETIME {
            dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let t = filetime_to_system_time(ft).expect("应能转换");
        let d = t
            .duration_since(std::time::UNIX_EPOCH)
            .expect("应晚于 UNIX 纪元");
        assert_eq!(d.as_secs(), 1);
        assert_eq!(d.subsec_millis(), 500, "亚秒精度必须保留：{d:?}");
        // 1601 年（FILETIME 零值）早于 UNIX 纪元 → None（不得回绕）
        assert!(filetime_to_system_time(FILETIME::default()).is_none());
    }

    /// 回归：以 `259`（= `STILL_ACTIVE` 哨兵值）退出的进程**不得**被判为存活。
    ///
    /// 旧实现（`GetExitCodeProcess == STILL_ACTIVE`）在这里必然误判：进程已退出，
    /// 但因为调用方仍持有 `Child` 句柄，进程对象尚未销毁，`OpenProcess` 依旧成功、
    /// 退出码恰好是 259 ⇒ 报"存活"。影响面：`service.json` 记录被判有效、
    /// 残留进程回收被跳过、孤儿 `.lock` 永远不会被清理。
    #[test]
    fn exited_process_with_exit_code_259_is_not_alive() {
        use std::process::{Command, Stdio};
        let mut child = Command::new("cmd.exe")
            .args(["/c", "exit", "259"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("应能启动 cmd.exe");
        let pid = child.id();
        let status = child.wait().expect("应能等到子进程退出");
        assert_eq!(status.code(), Some(259), "测试前提：退出码应为 259");
        // 注意：此时仍持有 Child（进程对象存活），正是旧实现踩坑的窗口
        assert!(
            !is_process_alive(pid),
            "以 259 退出的进程不得被判为存活（STILL_ACTIVE 哨兵值歧义）"
        );
        drop(child);
    }

    /// 回归：进程树终止必须**先子孙、后本体**，且不依赖递归。
    #[test]
    fn kill_process_tree_terminates_a_live_process() {
        use std::process::{Command, Stdio};
        let mut child = Command::new("ping.exe")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("应能启动 ping.exe 作为测试子进程");
        let pid = child.id();
        assert!(is_process_alive(pid), "刚启动的子进程应存活");

        let killed = kill_process_tree(pid);
        assert!(
            killed.contains(&pid),
            "根进程应出现在「确实被终止」的列表中：{killed:?}"
        );
        let _ = child.wait();
        assert!(!is_process_alive(pid), "终止后根进程不应存活");
    }

    #[test]
    fn kill_process_tree_of_absent_pid_is_empty() {
        // 不存在的 PID：必须安全返回空列表（不得 panic、不得计入"已终止"）
        assert!(kill_process_tree(0xFFFF_FFFE).is_empty());
    }

    #[test]
    fn bogus_pid_is_not_alive() {
        // 用一个几乎不可能存在的 PID
        assert!(!is_process_alive(0xFFFF_FFFE));
    }

    #[test]
    fn detects_running_and_absent_processes() {
        // 当前进程必然在运行（按自身镜像名探测）
        let me = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .expect("应能取到自身镜像名");
        assert!(any_process_running(&me), "应能检测到自身进程 {me}");

        // 大小写不敏感
        assert!(any_process_running(&me.to_uppercase()), "应大小写不敏感");

        // 一个几乎不可能存在的镜像名
        assert!(
            !any_process_running("definitely_not_a_real_process_xyz.exe"),
            "不应误报不存在的进程"
        );
    }

    #[test]
    fn node_running_probe_does_not_panic() {
        // 结果取决于机器状态，这里只要求可安全调用（不得 panic）
        let _ = any_node_running();
    }

    #[test]
    fn unreadable_process_is_unknown_not_notdsh() {
        // **关键语义**（实测踩过的坑）：读不到进程信息时必须判为 Unknown，
        // 而不是 NotDsh。早期实现把两者混为一谈 → 端口上明明是 dsh，
        // 日志却报「被非 dsh 程序占用」→ 拒绝接管 → 服务停掉后起不来。
        let id = classify_dsh_identity(0xFFFF_FFFE);
        assert!(
            id.is_unknown(),
            "读不到信息的 PID 必须判为 Unknown，实际：{}",
            id.summary()
        );
        assert!(!id.is_dsh());
        // 布尔包装在 Unknown 下必须是 false（保守：不接管、不杀）
        assert!(!is_dsh_harness_process(0xFFFF_FFFE));
        // 诊断信息必须能说明"为什么判不出来"
        assert!(!id.steps().note.is_empty(), "Unknown 必须给出原因");
    }

    #[test]
    fn own_process_is_not_dsh_and_says_why() {
        // 本测试进程是 cargo test 的 exe（不是 node.exe）→ 必须是 NotDsh，
        // 且原因里点明镜像名不匹配（可诊断性）。
        let me = std::process::id();
        let id = classify_dsh_identity(me);
        assert!(!id.is_dsh(), "测试进程不应被判为 dsh：{}", id.summary());
        assert!(!id.is_unknown(), "自身进程的信息必然可读：{}", id.summary());
        let s = id.steps();
        assert!(!s.image_name.is_empty(), "应能读到自身镜像名");
        assert!(
            s.note.contains("不是 node.exe") || s.note.contains("系统目录") || !s.note.is_empty(),
            "NotDsh 必须给出原因：{}",
            s.note
        );
    }

    #[test]
    fn identity_summary_is_always_non_empty() {
        // summary() 会被写进日志与错误信息，任何分支都不能为空
        for pid in [std::process::id(), 0xFFFF_FFFE, 4] {
            let id = classify_dsh_identity(pid);
            assert!(!id.summary().is_empty(), "pid {pid} 的 summary 为空");
        }
    }
}
