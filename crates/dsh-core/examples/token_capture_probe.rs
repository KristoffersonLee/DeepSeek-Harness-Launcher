//! token 捕获链路的**运行期**验证程序（隔离端口，不碰真实服务）。
//!
//! ## 为什么需要它
//!
//! v5.0.0 审计的 P0 之一：`dsh web` 打到 stdout 的一次性 token 地址**从未被捕获**，
//! 结果是界面一律导航到无 token 地址 —— 实测该地址返回 **HTTP 401**，用户看到
//! 认证失败页。修复点有两个：
//!   1. 就绪 worker 在锁内**只读一次** `auth_url`，与 stdout 捕获线程竞态；
//!   2. `Ready` 事件无条件用无 token 地址，使「等 token」逻辑永远不可达。
//!
//! 其中「stdout 捕获回调是否真的会被调用」是**最容易悄悄失效**的一环
//! （管道接线、解析前缀、回调时机任一变化都会让它静默失效，且日志里只表现为
//! 「没有那一行」）。因此这里用**完全相同的生产代码路径**
//! （[`ProcessManager::start_dsh`] + [`ReadyHook`] + [`ReadyProbe`]）跑一遍真实验证：
//!
//! ```text
//! start_dsh(port, …, on_ready)  ← 生产路径：spawn + 挂 Job 策略 + stdout 排空 + stderr 排空
//!        │
//!        ├─ 读取线程：spawn_ready_reader → parse_ready_url → cb(url, raw)
//!        └─ 主线程   ：ReadyProbe::wait_for_ready（与生产同款分片等待）
//! ```
//!
//! 断言：
//! - **A** 回调被调用（若这里失败，说明捕获链路已断 —— 界面必然 401）
//! - **B** 回调给出的 URL 含 `?token=`
//! - **C** `parse_ready_url` 从原始行提取的 URL 与回调一致
//! - **D** 就绪探测成功
//! - **E** 「就绪事件先于 token 到达」这一竞态在这次运行里是否发生（**记录事实，不算失败**）
//! - **F** 收尾：进程被真正停止、端口释放（确保测试不留残留）
//!
//! 用法：`cargo run --offline -p dsh-core --example token_capture_probe -- <port>`
//! 退出码：0 = 通过；1 = 失败（并打印原因）

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dsh_core::{Dsh, ProcessManager, ReadyProbe, RollingLogger};

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(45680);

    println!("=== token 捕获链路验证（端口 {port}）===");

    // ---- 0. 环境解析（生产同一路径）----
    let dsh = match Dsh::resolve(None) {
        Ok(d) => d,
        Err(e) => fail(&format!("环境解析失败（需要本机安装了 dsh）: {e}")),
    };
    println!("node  : {}", dsh.node.display());
    println!("bin.js: {}", dsh.bin_js.display());

    let logger = RollingLogger::new(std::env::temp_dir().join("dsh-token-probe-logs"));
    let mut pm = ProcessManager::new();
    pm.set_logger(logger.clone());

    // ---- 1. 捕获回调（与 service.rs 的 on_ready 同构）----
    let captured: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
    let fired = Arc::new(AtomicBool::new(false));
    let captured_for_hook = Arc::clone(&captured);
    let fired_for_hook = Arc::clone(&fired);
    let on_ready: dsh_core::process::ReadyHook = Box::new(move |url: String, raw: String| {
        fired_for_hook.store(true, Ordering::SeqCst);
        if let Ok(mut g) = captured_for_hook.lock() {
            *g = Some((url, raw));
        }
    });

    // ---- 2. 启动（生产路径）----
    let t0 = Instant::now();
    let pid = match pm.start_dsh(port, None, &dsh.node, &dsh.bin_js, Some(on_ready)) {
        Ok(p) => p,
        Err(e) => fail(&format!("start_dsh 失败: {e}")),
    };
    println!("已启动 PID {pid}（生命周期策略 = {:?}）", pm.lifecycle());

    // ---- 3. 就绪探测，同时观察「就绪 vs token」的先后（复现 A2 竞态场景）----
    let probe = ReadyProbe::new(port);
    let mut ready_at: Option<Duration> = None;
    let mut token_at: Option<Duration> = None;
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if ready_at.is_none() && probe.wait_for_ready(Duration::from_millis(250)).is_ok() {
            ready_at = Some(t0.elapsed());
        }
        if token_at.is_none() && fired.load(Ordering::SeqCst) {
            token_at = Some(t0.elapsed());
        }
        if ready_at.is_some() && token_at.is_some() {
            break;
        }
        if ready_at.is_some() && token_at.is_none() && t0.elapsed() > Duration::from_secs(30) {
            // 就绪后 30 秒仍未捕获 token：捕获链路有问题，早退便于定位
            break;
        }
    }

    // ---- 4. 判定 ----
    let mut failed = false;
    match (&ready_at, &token_at) {
        (Some(r), Some(t)) => {
            println!(
                "就绪于 {r:?}；捕获 token 于 {t:?}（顺序：{}）",
                if t >= r {
                    "就绪先到 → token 后到"
                } else {
                    "token 先到"
                }
            );
        }
        (Some(r), None) => {
            println!("就绪于 {r:?}，但 30 秒内**未捕获到 token**");
        }
        (None, Some(t)) => println!("捕获 token 于 {t:?}，但未探测到就绪"),
        (None, None) => println!("既未就绪也未捕获 token"),
    }

    if ready_at.is_none() {
        println!("[FAIL] D: 就绪探测未成功");
        failed = true;
    } else {
        println!("[PASS] D: 就绪探测成功");
    }

    if !fired.load(Ordering::SeqCst) {
        println!("[FAIL] A: token 捕获回调**从未被调用** —— 界面将导航到无 token 地址（HTTP 401）");
        failed = true;
    } else {
        println!("[PASS] A: token 捕获回调已被调用");
    }

    if let Ok(g) = captured.lock() {
        if let Some((url, raw)) = g.as_ref() {
            if url.contains("token=") {
                println!("[PASS] B: 捕获地址含 token（已脱敏打印）：{}", redact(url));
            } else {
                println!("[FAIL] B: 捕获到的地址不含 token: {}", redact(url));
                failed = true;
            }
            // C：与 parse_ready_url 独立复算一致
            match dsh_core::dsh::parse_ready_url(raw) {
                Some(again) if &again == url => {
                    println!(
                        "[PASS] C: 解析结果可复算一致（原始行已脱敏）：{}",
                        redact(raw)
                    )
                }
                other => {
                    println!("[FAIL] C: 解析结果不可复算: {other:?}");
                    failed = true;
                }
            }
            // E：把竞态事实写进输出（供人工确认修复前提）
            if let (Some(r), Some(t)) = (ready_at, token_at) {
                if r < t {
                    println!(
                        "[NOTE] E: 本次「就绪先于 token {:?}」—— 正是旧实现会用无 token 地址导航的场景；\
                         修复后由 Ready{{url:None}} + 补发 Ready 覆盖",
                        t - r
                    );
                }
            }
        }
    }

    // ---- 5. 收尾：必须真正停掉，避免留残留 ----
    match pm.stop() {
        Ok(killed) => println!("已停止（killed={killed:?}）"),
        Err(e) => {
            println!("[FAIL] 停止失败: {e}");
            failed = true;
        }
    }
    std::thread::sleep(Duration::from_millis(800));
    let still = dsh_core::is_port_listening(port);
    if still {
        println!("[FAIL] F: 停止后端口 {port} 仍在监听（可能留残留）");
        failed = true;
    } else {
        println!("[PASS] F: 停止后端口已释放（无残留）");
    }

    println!("=== 结果：{} ===", if failed { "不通过" } else { "通过" });
    if failed {
        std::process::exit(1);
    }
}

/// 脱敏：token 值替换为 ***（复用生产实现，顺带验证它可用）。
fn redact(s: &str) -> String {
    dsh_core::redact_secrets(s).into_owned()
}

fn fail(msg: &str) -> ! {
    println!("[FAIL] {msg}");
    std::process::exit(1);
}
