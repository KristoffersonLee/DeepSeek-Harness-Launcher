//! DSHLauncher v5 —— 唯一入口。
//!
//! 组装 dsh-core + dsh-ui，负责：
//! 1. 单实例检查（CreateMutexW）
//! 2. 解析命令行参数（--selftest / --guide / --settings）
//! 3. 加载配置（损坏则提示并可重置）
//! 4. 初始化 RollingLogger
//! 5. 启动 worker 线程 + tao 事件循环
//! 6. 托盘事件循环
//! 7. 退出清理（Job Object 关闭 → 子进程自动回收）

// 关键：release 构建使用 Windows GUI 子系统。
// 若不声明，Rust 默认链接为 CONSOLE 子系统，双击启动器会额外弹出一个终端窗口
// （用户会看到"两个进程"）。debug 构建保留控制台，便于开发期查看输出。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod browser;
mod cli;
mod crash;
mod dialog;
mod icon;
mod service;
mod tray_handler;

/// 启动阶段计时：用于「启动时间」基线与对比（写入日志，可复现测量）。
///
/// 计时器由 `app::run` 复用同一实例，0 点为 `main` 入口
/// （进程创建到 `main` 之间的耗时无法自测）。
type BootTimer = app::BootTimer;

/// 安全地向 stderr 写一行（GUI 子系统下 stderr 可能无效，`eprintln!` 会 panic）。
fn warn(msg: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{msg}");
}

/// 安全地向 stdout 写文本（**必须忽略写入错误**）。
///
/// **为什么不能直接用 `print!`**：release 是 Windows **GUI 子系统**，从资源管理器
/// 或没有控制台的脚本启动时 `GetStdHandle(STD_OUTPUT_HANDLE)` 是空句柄，
/// 写入失败会让 `print!` **panic**；而 release 开了 `panic = "abort"`，
/// 于是 `--build-info` / `--probe-identity` 会在写下标记文件**之前**整进程崩溃——
/// 部署脚本因此把一个好产物判成坏的，且拿不到任何诊断。
/// 这里显式忽略写入错误：文件的写入才是脚本的可靠依据。
fn emit_stdout(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// 处理 `--build-info`：把构建/修复标记写到**文件**后退出。
///
/// 为什么不只 `println!`：release 是 Windows **GUI 子系统**，进程默认没有控制台，
/// `println!` 的输出无处可去（部署脚本读不到）。因此先 `AttachConsole` 借用父控制台
/// 试着打印，同时**总是**写一份标记文件 —— 文件才是脚本可靠的判定依据。
fn emit_build_info_and_exit() -> ! {
    let text = cli::build_info_text();
    emit_console_text(&text);
    let path = dsh_core::app_data_dir().join("build-info.txt");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, &text) {
        warn(&format!("[DSHLauncher] 写入 {} 失败: {e}", path.display()));
    }
    std::process::exit(0);
}

/// 打印文本到控制台并退出（`--version` / `--help`）。
///
/// GUI 子系统下必须先把父进程的控制台接过来，否则终端里什么都看不到
/// （`emit_stdout` 只是"写失败也不 panic"的兜底，不代表输出可达）。
fn emit_console_text_and_exit(text: &str) -> ! {
    emit_console_text(text);
    std::process::exit(0);
}

/// 借用父进程控制台后写 stdout（无控制台时静默失败）。
///
/// ## 为什么必须先探测 stdout 句柄（实测缺陷）
///
/// `AttachConsole(ATTACH_PARENT_PROCESS)` 会把进程的标准句柄指向**控制台**，
/// 于是「stdout 已被重定向到文件/管道」的场景会被破坏：实测
/// `DSHLauncher.exe --version > ver.txt` 得到 **0 字节**，`cmd /c "... --version"`
/// 在终端上也看不到任何输出 —— 也就是说 `--version` / `--help` 这两个
/// 「安装包与 CI 用来核对版本」的入口**完全静默**（而 `--build-info` 还能靠标记文件兜底，
/// 它同样丢掉了 stdout）。反证：不调用本函数的 `--probe-identity` 输出正常。
///
/// 现在的策略：**先看进程是否已经有可写的 stdout 句柄**；
/// - 有（终端直接运行、管道、文件重定向）⇒ 直接写，尊重重定向；
/// - 没有（资源管理器双击启动的 GUI 进程）⇒ 才 `AttachConsole` 借用父控制台。
fn emit_console_text(text: &str) {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    if !has_stdout_handle() {
        // 无 stdout：试着借用父进程的控制台（交互式运行时用户能看到）
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    emit_stdout(text);
}

/// 进程当前是否有一个可用的 stdout 句柄（重定向到文件/管道/终端时为真）。
///
/// 只判定「存在」，不判定「可写」：调用方关心的是「要不要去借用控制台」。
fn has_stdout_handle() -> bool {
    use windows::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
    unsafe {
        GetStdHandle(STD_OUTPUT_HANDLE)
            .map(|h| !h.is_invalid())
            .unwrap_or(false)
    }
}

fn main() -> anyhow::Result<()> {
    let boot = BootTimer::start();

    // 0. 崩溃取证：`panic = "abort"` 下没有栈回溯，至少要在日志里留一行 FATAL
    crash::install(dsh_core::log_dir().join("launcher.log"));

    // 1. 解析命令行参数（需先解析：测试模式在互斥体冲突时必须返回非零，
    //    否则 CI/脚本会把"因已有实例而直接退出"误判为通过）
    let args = cli::parse_args();

    // 1a0. `--version` / `--help`：**必须最先处理**，且绝不触碰互斥体/窗口。
    //
    // 这是安装包与 CI 验证「装出来的是不是这一版」的最小接口。早先版本没有这两个
    // 入口：`DSHLauncher.exe --version` 会被当作普通启动（抢互斥体、可能弹窗或
    // 唤起已有实例），于是"安装后核对版本"根本无法自动化。
    if args.version {
        emit_console_text_and_exit(&cli::version_text());
    }
    if args.help {
        emit_console_text_and_exit(&cli::help_text());
    }

    // 1a. `--build-info`：打印/记录构建与修复标记后退出（部署脚本据此判定产物真伪）
    if args.build_info {
        emit_build_info_and_exit();
    }

    // 1a2. `--probe-identity <PID>`：诊断身份判定过程。
    //      存在的意义：`is_dsh_harness_process` 是"防误杀"闸门，一旦它把真正的 dsh
    //      判成"非 dsh"，后果是接管失败 + 服务停掉后起不来（实测踩到）。这类失败在
    //      日志里只有一行结论，看不出卡在哪一步，因此把每一步都打印出来。
    if let Some(pid) = args.probe_identity {
        let id = dsh_core::classify_dsh_identity(pid);
        let s = id.steps();
        let verdict = match &id {
            dsh_core::DshIdentity::IsDsh { .. } => "IsDsh",
            dsh_core::DshIdentity::NotDsh { .. } => "NotDsh",
            dsh_core::DshIdentity::Unknown { .. } => "Unknown",
        };
        let out = format!(
            "pid={pid}\nverdict={verdict}\nimage_name={}\nimage_path={}\ndsh_node={}\nmatched={}\nnote={}\nsummary={}\n",
            s.image_name,
            s.image_path,
            s.dsh_node,
            s.matched,
            s.note,
            id.summary()
        );
        emit_stdout(&out);
        // GUI 子系统下没有控制台，所以同时写一份文件（供脚本读取）
        let path = dsh_core::app_data_dir().join("identity-probe.txt");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, &out);
        std::process::exit(0);
    }

    // 1b. `--quit`：请求已在运行的实例优雅退出（本进程不启动）。
    //     必须在单实例检查**之前**处理——请求方自己不该去持有互斥体。
    //
    //     退出码契约（脚本据此判断「能不能覆盖产物」）：
    //       0 = 已确认退出，或本来就没有实例（幂等）；
    //       3 = 请求已送达但**未收到回执**（实例可能仍在运行，例如 tied 模式下
    //           用户还没在确认框上做出选择）——脚本必须自行确认后再覆盖 exe。
    if args.quit {
        let outcome = dsh_core::request_existing_instance_quit();
        let msg = format!("[DSHLauncher] --quit：{}", outcome.describe());
        // GUI 子系统下 stderr 可能无效；同时写标记文件供脚本读取（与 --build-info 同款）
        warn(&msg);
        emit_stdout(&format!("{msg}\n"));
        let path = dsh_core::app_data_dir().join("quit-result.txt");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(
            &path,
            format!(
                "version={}\noutcome={:?}\nexit_code={}\n",
                env!("CARGO_PKG_VERSION"),
                outcome,
                outcome.exit_code()
            ),
        );
        let code = outcome.exit_code();
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    // 2. 单实例检查（互斥体实现位于 dsh-core，避免重复代码）
    let single = match dsh_core::SingleInstance::acquire()? {
        Some(instance) => instance,
        None => {
            if args.selftest || args.ipc_probe {
                // 测试模式下这是硬失败：本次根本没有执行任何验证
                warn(&format!(
                    "[DSHLauncher] 已有实例在运行（持有 {}），无法执行 {}；请先退出启动器。",
                    dsh_core::MUTEX_NAME,
                    if args.selftest {
                        "--selftest"
                    } else {
                        "--ipc-probe"
                    }
                ));
                std::process::exit(2);
            }
            // 正常启动：已有实例 → 唤起它的窗口（与 v4 行为一致），本进程退出
            if dsh_core::signal_existing_instance() {
                return Ok(());
            }
            // 事件尚不存在（拥有者刚拿到互斥体、还没建好事件）：
            // 短暂重试后仍失败则视为「对方正在启动中」，直接退出即可。
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if dsh_core::signal_existing_instance() {
                    return Ok(());
                }
            }
            // 极端情形（对方已持有互斥体但事件迟迟不可用，例如正在崩溃退出中）：
            // 静默退出会让用户觉得"双击了但什么都没发生"，因此明确提示一次。
            warn("[DSHLauncher] 已有实例在运行，但无法唤起其窗口；请查看系统托盘。");
            dialog::warn(
                "已经有一个 DSHLauncher 在运行，但本次没能唤起它的窗口。\n\n\
                 请查看系统托盘（右下角隐藏图标）里的 DSHLauncher；\n\
                 若托盘里也没有，请等待几秒后重试。",
                &format!("{} · 已有实例", dsh_core::APP_NAME),
            );
            return Ok(());
        }
    };

    // 2b. 唤起事件、退出请求事件与**退出回执**事件：都必须在进入事件循环前创建，
    //     否则信号会丢失。回执事件让 `--quit` 的请求方不必靠猜（见 single_instance.rs）。
    let activation = dsh_core::ActivationEvent::create()?;
    let quit_event = dsh_core::QuitEvent::create()?;
    let quit_ack = dsh_core::QuitAckEvent::create()?;

    // 3. 加载配置（损坏时保留错误，稍后在 GUI 模式下明确提示，不再静默降级）
    let mut config_error: Option<String> = None;
    let config = match dsh_core::Settings::load() {
        Ok(c) => c,
        Err(e) => {
            config_error = Some(e.to_string());
            dsh_core::Settings::default()
        }
    };

    // 4. 初始化日志
    let logger = dsh_core::RollingLogger::new(dsh_core::log_dir());
    logger.info(&format!(
        "{} v{} 启动",
        dsh_core::APP_NAME,
        env!("CARGO_PKG_VERSION")
    ));
    logger.info(&format!("服务地址: http://127.0.0.1:{}/", config.port));
    if let Some(err) = &config_error {
        logger.error(&format!("配置加载失败，已回退默认值：{err}"));
    }
    // 「配置由更新版的启动器写入」是一个**必须让用户知道**的状态：
    // 字段语义可能已经变了，静默继续可能误读设置（`Settings::is_from_newer_schema`
    // 的文档就是这么要求的，但此前没有任何调用方——承诺落空）。
    if config.is_from_newer_schema() {
        logger.warn(&format!(
            "配置文件由更新版本的启动器写入（schema_version={} > {}）；\
             本次按向前兼容运行，若设置表现异常请用设置页「恢复默认设置」。",
            config.schema_version,
            dsh_core::config::CURRENT_SCHEMA_VERSION
        ));
    }
    boot.mark(&logger, "单实例+配置+日志就绪");

    // 5. 处理特殊模式
    if args.selftest {
        return app::run_selftest(&config, &logger);
    }

    // 6. 配置损坏时明确提示（GUI 模式；selftest 已在上面提前返回）
    if let Some(err) = config_error {
        dialog::warn(
            &format!(
                "配置文件解析失败，本次已使用默认设置运行。\n\n错误：{err}\n\n\
                 配置文件位置：{}\n\
                 你可以在设置页点「恢复默认设置」，或手动删除该文件后重启。",
                dsh_core::Settings::file_path().display()
            ),
            &format!("{} · 配置异常", dsh_core::APP_NAME),
        );
    }

    // 7. 启动应用主循环
    app::run(
        config,
        logger,
        app::StartupOptions {
            open_settings: args.settings,
            open_guide: args.guide,
            ipc_probe: args.ipc_probe,
        },
        app::RuntimeHandles {
            single,
            activation,
            quit_event,
            quit_ack,
        },
        boot,
    )
}
