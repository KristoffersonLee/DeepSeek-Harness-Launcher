//! Job Object 验证程序（隔离测试，不触碰真实 dsh 服务）。
//!
//! 验证两种互补语义：
//!
//! **模式 1（默认）：强杀回收** —— 路线图回归清单 #1
//!   1. 创建 Job Object（KILL_ON_JOB_CLOSE）
//!   2. 启动长命子进程（ping）并挂进 Job
//!   3. 打印 PID 后挂起
//!   4. 外部 `taskkill /F` 强杀本程序 → 子进程应被内核一并回收
//!
//! **模式 2（`disarm`）：优雅退出保留服务** —— 延续 v2.0“退出保留后台服务”语义
//!   1. 同上创建 Job 并挂入子进程
//!   2. 调用 `keep_children_on_exit()` 解除 KILL_ON_JOB_CLOSE
//!   3. 本程序**正常退出** → 子进程应继续存活
//!
//! 两者由 `selftest.ps1` 的 A1 / A2 小节分别驱动。

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use dsh_core::JobHandle;
use windows::Win32::Foundation::HANDLE;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "disarm" {
        run_disarm_mode();
    } else {
        run_kill_on_close_mode();
    }
}

/// 模式 1：保持 KILL_ON_JOB_CLOSE，等待被外部强杀。
fn run_kill_on_close_mode() {
    let job = match JobHandle::new() {
        Ok(j) => j,
        Err(e) => fail(&format!("JobHandle::new 失败: {e}")),
    };

    let child = match spawn_long_lived() {
        Ok(c) => c,
        Err(e) => fail(&format!("启动子进程失败: {e}")),
    };
    let child_pid = child.id();

    if let Err(e) = job.assign_raw(HANDLE(child.as_raw_handle())) {
        fail(&format!("AssignProcessToJobObject 失败: {e}"));
    }

    println!("MODE=kill_on_close");
    println!("SELF_PID={}", std::process::id());
    println!("CHILD_PID={child_pid}");
    println!("READY=1");

    // 句柄必须存活，才能让 Job 在父进程被杀时关闭并触发回收
    let _keep_child = child;
    let _keep_job = job;

    std::thread::sleep(std::time::Duration::from_secs(600));
}

/// 模式 2：解除 KILL_ON_JOB_CLOSE 后正常退出，子进程应存活。
fn run_disarm_mode() {
    let child = match spawn_long_lived() {
        Ok(c) => c,
        Err(e) => fail(&format!("启动子进程失败: {e}")),
    };
    let child_pid = child.id();

    let job = match JobHandle::new() {
        Ok(j) => j,
        Err(e) => fail(&format!("JobHandle::new 失败: {e}")),
    };
    if let Err(e) = job.assign_raw(HANDLE(child.as_raw_handle())) {
        fail(&format!("AssignProcessToJobObject 失败: {e}"));
    }

    // 解除限制：模拟“用户选择保留后台服务”
    if job.disarm_kill_on_close().is_err() {
        fail("disarm_kill_on_close 失败");
    }

    println!("MODE=disarm");
    println!("SELF_PID={}", std::process::id());
    println!("CHILD_PID={child_pid}");
    println!("READY=1");

    // 丢弃句柄并正常退出：子进程应存活（否则说明 disarm 未生效）
    drop(job);
    drop(child);
    // main 正常返回 → 进程退出
}

fn spawn_long_lived() -> std::io::Result<std::process::Child> {
    Command::new("ping.exe")
        .args(["-n", "600", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
}

fn fail(msg: &str) -> ! {
    println!("ERROR={msg}");
    std::process::exit(2);
}
