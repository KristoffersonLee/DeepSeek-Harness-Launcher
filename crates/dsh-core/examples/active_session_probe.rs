//! 活跃会话探测的**运行期**验证（只读，不修改任何数据）。
//!
//! ## 为什么需要它
//!
//! 「清理归档会话」必须先停止 dsh；若此刻有会话在跑就会把它打断。启动器因此在点击时
//! 探测活跃会话（[`dsh_core::probe_active_sessions`]），并把结果写进二次确认文案。
//!
//! 该探测由两个信号组成，其中"会话运行器进程存活"依赖一条**外部命令**
//! （`Get-CimInstance Win32_Process`；自己解析 PEB 需要 `ReadProcessMemory` +
//! 手工解读 `RTL_USER_PROCESS_PARAMETERS`，含 WOW64 结构差异，不值得为一次性探测承担）。
//! 这类"靠外部命令拿数据"的实现最容易**静默失效**——命令改名、输出格式变化、
//! 权限受限都会让它恒返回 0，而调用方只会看到"未检测到活跃会话"，从而**放过**一次
//! 本应警告的清理。因此必须有可执行的验证，而不是只靠代码审查。
//!
//! ## 断言
//!
//! - **A** 探测可调用且返回结构（不得 panic）
//! - **B** 若本机确实有 `dsh-subprocess-local` / `runner.js` 进程（由 PowerShell 独立复核），
//!   则探测必须报告 `runner_processes > 0` —— 这是**交叉验证**，两边都错才会漏报
//! - **C** 会话文件计数与 `summary()` 自洽
//! - **D** 探测耗时在可接受范围（它是点击触发的一次性调用，不应拖住界面）
//!
//! 用法：`cargo run --offline -p dsh-core --example active_session_probe`
//! 退出码：0 = 通过；1 = 失败

use std::time::Instant;

fn main() {
    println!("=== 活跃会话探测验证（只读）===");
    let mut failed = false;

    // ---- A: 可调用 ----
    let t0 = Instant::now();
    let probe = dsh_core::probe_active_sessions();
    let elapsed = t0.elapsed();
    match &probe {
        Some(a) => {
            println!("[PASS] A: 探测返回结构（耗时 {elapsed:?}）");
            println!("       运行器进程 = {}", a.runner_processes);
            println!("       近期写入文件 = {}", a.recently_written);
            println!("       涉及会话 = {:?}", a.recent_session_hint);
            println!("       摘要 = {}", a.summary());
            println!(
                "       is_running = {}  any_activity = {}",
                a.is_running(),
                a.any_activity()
            );
        }
        None => {
            println!("[NOTE] A: 探测返回 None（无法判定）—— 调用方应按有活动处理");
        }
    }

    // ---- B: 与 PowerShell 独立复核交叉验证 ----
    let expected = independent_runner_count();
    match (&probe, expected) {
        (Some(a), Some(exp)) => {
            println!("[INFO] B: 独立复核（PowerShell）运行器数 = {exp}");
            if exp > 0 && a.runner_processes == 0 {
                println!(
                    "[FAIL] B: 独立复核说有 {exp} 个会话运行器，但探测报告 0 —— \
                     探测链路失效（清理会漏掉「有会话在跑」的警告）"
                );
                failed = true;
            } else if exp > 0 {
                println!(
                    "[PASS] B: 交叉验证一致（探测 = {} ≥ 1）",
                    a.runner_processes
                );
            } else {
                println!("[NOTE] B: 本机当前没有会话运行器，无法交叉验证（不算失败）");
            }
        }
        (_, None) => println!("[NOTE] B: 独立复核不可用（PowerShell 查询失败），跳过"),
        (None, _) => println!("[NOTE] B: 探测返回 None，跳过交叉验证"),
    }

    // ---- C: 自洽性 ----
    if let Some(a) = &probe {
        let ok = a.any_activity() == (a.runner_processes > 0 || a.recently_written > 0);
        if ok {
            println!("[PASS] C: summary/标志 与计数自洽");
        } else {
            println!("[FAIL] C: 标志与计数不自洽");
            failed = true;
        }
        if a.recent_session_hint.len() > 3 {
            println!("[FAIL] C: 会话提示超过 3 条");
            failed = true;
        }
    }

    // ---- D: 耗时 ----
    // 生产实现内部对外部命令有 5 秒硬上限（超时即杀进程并如实返回 0），
    // 因此这里留出余量取 8 秒：它既能发现"限时失效"，也不会因为慢机器误报。
    if elapsed.as_secs_f64() < 8.0 {
        println!(
            "[PASS] D: 探测耗时 {:?} 在可接受范围（< 8s；内部外部命令上限 5s）",
            elapsed
        );
    } else {
        println!(
            "[FAIL] D: 探测耗时 {:?} 过长（说明外部命令超时保护失效，会明显拖住点击响应）",
            elapsed
        );
        failed = true;
    }

    println!("=== 结果：{} ===", if failed { "不通过" } else { "通过" });
    if failed {
        std::process::exit(1);
    }
}

/// 用与实现**不同**的路径独立数一遍会话运行器（交叉验证用）。
///
/// 同样必须限时：本程序是发布验证链的一环，不能被一条卡住的 PowerShell 挂死。
fn independent_runner_count() -> Option<usize> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(@(Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | \
               Where-Object { $_.CommandLine -like '*dsh-subprocess-local*' })).Count",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;

    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.take(64 * 1024).read_to_string(&mut buf);
        buf
    });

    let start = std::time::Instant::now();
    let ok = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.success(),
            Ok(None) if start.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let text = reader.join().unwrap_or_default();
    if !ok {
        return None;
    }
    text.trim().parse().ok()
}
