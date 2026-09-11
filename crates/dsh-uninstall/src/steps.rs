//! 卸载的两个阶段与全部状态变更。
//!
//! ## 为什么分两阶段
//!
//! Windows 不允许删除**正在运行**的镜像文件。旧脚本用"再起一个隐藏 `powershell.exe`
//! + 往 `%TEMP%` 写临时脚本"绕开；本程序用同一条思路但不再依赖脚本引擎：
//!
//! ```text
//! 阶段一（安装目录里的 dsh-uninstall.exe）
//!   结束本目录的启动器 → 删注册表项 → 删快捷方式 → 删 %LOCALAPPDATA% 运行期数据
//!   →（--purge 时）删 %APPDATA% 用户配置 → 删本安装包的文件（**跳过自身**）
//!   → 复制自身到 %TEMP%\dsh-uninstall-<pid>.exe → 用它重入（--deferred-pass <安装目录>）
//!   → 打印结论并退出
//! 阶段二（%TEMP% 里的副本）
//!   短暂等待 → 再次校验标记与护栏 → 递归删除安装目录 → 调度"重启时删除本副本"
//! ```
//!
//! 两个阶段都**重新校验**标记与受保护路径（纵深防御：不信任上一阶段传来的东西）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::guards;
use crate::plan::{Context, MARKER_NAME};
use crate::platform;

/// 阶段一：返回值即进程退出码（0 / 1 / 2）。
pub fn main_pass(ctx: &Context) -> i32 {
    // ---- 只报告：dry-run 不修改任何状态，也不校验后置条件（没有发生变更）----
    if ctx.dry_run {
        print_plan(ctx);
        return 0;
    }

    let mut failures: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    info(
        ctx,
        &format!("DSHLauncher 卸载器 —— 安装目录：{}", ctx.display()),
    );

    // 顺手清掉上次卸载遗留在 %TEMP% 的副本（它们本来要等到重启才消失）
    let stale = platform::sweep_stale_temp_copies(&ctx.self_exe);
    if stale > 0 {
        info(ctx, &format!("已清理 {stale} 个历史遗留的临时副本"));
    }

    // 1) 结束**本目录**的启动器（只认映像路径，绝不误杀别处安装）
    match stop_launcher(ctx) {
        Ok(killed) if killed.is_empty() => info(ctx, "没有从本目录运行的启动器"),
        Ok(killed) => info(
            ctx,
            &format!(
                "已结束本目录的启动器（PID {}）",
                killed
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        Err(e) => failures.push(e),
    }

    // 2) 注册表卸载项
    if platform::uninstall_key_exists() {
        if platform::remove_uninstall_key() && !platform::uninstall_key_exists() {
            info(ctx, "已移除注册表卸载项");
        } else {
            failures.push(format!(
                "删除注册表项失败或未生效：HKCU\\{}",
                platform::UNINSTALL_KEY
            ));
        }
    } else {
        info(ctx, "注册表卸载项不存在（无需移除）");
    }

    // 3) 桌面快捷方式（含 OneDrive 重定向后的真实桌面 + 公共桌面）
    let shortcuts = platform::remove_desktop_shortcuts();
    if shortcuts.is_empty() {
        info(ctx, "没有需要移除的桌面快捷方式");
    } else {
        for p in &shortcuts {
            info(ctx, &format!("已移除快捷方式 {}", p.display()));
        }
    }

    // 4) 运行期数据（日志 / WebView2 profile / service.json）——总是清理
    let local = platform::local_dir();
    if local.exists() {
        match remove_dir_all_verified(&local) {
            Ok(()) => info(ctx, &format!("已删除运行期数据 {}", local.display())),
            Err(e) => failures.push(format!("删除 {} 失败：{e}", local.display())),
        }
    }

    // 5) 用户配置——默认保留，只有 --purge 才删
    let roaming = platform::roaming_dir();
    if ctx.purge {
        if roaming.exists() {
            match remove_dir_all_verified(&roaming) {
                Ok(()) => info(
                    ctx,
                    &format!("已删除用户配置 {}（--purge）", roaming.display()),
                ),
                Err(e) => failures.push(format!("删除 {} 失败：{e}", roaming.display())),
            }
        } else {
            info(ctx, "用户配置不存在（无需删除）");
        }
    } else if roaming.exists() {
        notes.push(format!(
            "已保留用户配置：{}（如需一并删除，请用 --purge）",
            roaming.join("settings.toml").display()
        ));
    }

    // 6) 本安装包的文件（跳过正在运行的自身；标记文件交给阶段二）
    //
    // 清单**只算一次**：旧实现在删除与复核两处各调用一次 `files_to_remove()`
    // （两次分配、两次字符串格式化），而这是一份纯函数结果，本该复用。
    let payloads = ctx.files_to_remove();
    for path in &payloads {
        if !path.exists() {
            continue;
        }
        if let Err(e) = std::fs::remove_file(path) {
            failures.push(format!("删除 {} 失败：{e}", path.display()));
        }
    }
    // 复核：文件必须真的没了（旧脚本同样做后置条件校验）
    for path in &payloads {
        if path.exists() {
            failures.push(format!("文件仍然存在：{}", path.display()));
        }
    }

    // 7) 目录本身：交给阶段二（运行中的镜像删不掉），前提是护栏通过
    let mut deferred = false;
    if !ctx.may_delete_dir_itself() {
        failures.push(format!(
            "拒绝递归删除安装目录（受保护路径或缺少标记 {}）：{}",
            MARKER_NAME,
            ctx.display()
        ));
    } else {
        let foreign = guards::foreign_entries(
            &ctx.install_dir,
            crate::plan::PAYLOADS,
            crate::plan::LEGACY_PAYLOADS,
            MARKER_NAME,
        );
        if foreign.is_empty() {
            match spawn_deferred_pass(ctx) {
                Ok(()) => {
                    deferred = true;
                    notes.push(format!(
                        "安装目录的删除已交给后台副本（约 1 秒后移除）：{}",
                        ctx.display()
                    ));
                }
                Err(e) => failures.push(format!("无法启动延迟清理：{e}")),
            }
        } else {
            notes.push(format!(
                "目录里还有 {} 项不属于本安装包的内容，已保留目录本身：{}",
                foreign.len(),
                foreign.join(", ")
            ));
        }
    }

    summarize(ctx, &failures, &notes, deferred)
}

/// 残留清理模式（`--clean-residue`）：只清"磁盘外"的两处残留。
///
/// 适用情形：安装目录已被删除（手工删除 / 磁盘清理 / 安全软件隔离），于是注册表卸载项与
/// 桌面快捷方式都指向不存在的文件，而正规卸载入口全部被护栏拒绝 —— 卸载器从**自身所在
/// 目录**推导安装目录，目录没了就没有正规入口，源码树里的副本会被源码树护栏拒绝，
/// `--deferred-pass <dir>` 会被"缺少安装标记"拒绝。实测后果：设置 → 应用里的卸载按钮
/// 只会报找不到可执行文件，用户没有任何受支持的手段清掉那两处残留。
///
/// **本函数不删除任何文件或目录**（这是它与 [`main_pass`] 的本质区别，也是它敢在
/// "没有安装目录归属证据"时运行的前提）：
/// * `%LOCALAPPDATA%` 运行期数据不碰 —— 可能正被另一个实例（例如源码树里跑着的启动器）使用；
/// * `%APPDATA%` 用户配置不碰 —— `--purge` 在本模式下被忽略，需重装后走正规卸载。
///
/// 准入校验见 [`guards::residue_target_of`]（注册表三个值互相印证）与
/// [`guards::residue_dir_allowed`]（受保护路径 / 源码树 / 安装标记仍在 ⇒ 拒绝）。
pub fn residue_pass(args: &crate::cli::Args) -> i32 {
    let silent = args.silent;
    let say = |msg: &str| {
        if !silent {
            println!("{msg}");
        }
    };

    if !platform::uninstall_key_exists() {
        say(&format!(
            "没有注册表残留（HKCU\\{} 不存在），无需清理。",
            platform::UNINSTALL_KEY
        ));
        return 0;
    }

    let install_location = platform::read_uninstall_value("InstallLocation").unwrap_or_default();
    let uninstall_string = platform::read_uninstall_value("UninstallString").unwrap_or_default();
    let display_icon = platform::read_uninstall_value("DisplayIcon").unwrap_or_default();

    let dir = match guards::residue_target_of(&install_location, &uninstall_string, &display_icon) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    if let Err(e) = guards::residue_dir_allowed(&dir) {
        eprintln!("{e}");
        return 2;
    }

    let dir_state = if dir.exists() {
        "目录仍在，但没有安装标记"
    } else {
        "目录已不存在"
    };
    let shortcuts = platform::desktop_shortcuts();

    // ---- 只报告 ----
    if args.dry_run {
        say("DRY-RUN：残留清理范围（不会修改任何内容）");
        say(&format!(
            "  安装目录            : {}（{dir_state}）",
            dir.display()
        ));
        say(&format!(
            "  注册表卸载项        : HKCU\\{} → 将被删除",
            platform::UNINSTALL_KEY
        ));
        say(&format!(
            "  桌面快捷方式        : {} 个将被删除",
            shortcuts.len()
        ));
        for p in &shortcuts {
            say(&format!("      - {}", p.display()));
        }
        say("  用户配置 / 运行期数据: **不处理**（本模式不做任何文件系统删除）");
        say("DRY-RUN：以上为完整影响范围；未改动任何内容。");
        return 0;
    }

    let mut failures: Vec<String> = Vec::new();
    say(&format!(
        "DSHLauncher 残留清理 —— 安装目录：{}（{dir_state}）",
        dir.display()
    ));
    if args.purge {
        say("NOTE: 残留清理模式不处理用户配置（--purge 被忽略）；如需删除配置请重装后走正规卸载。");
    }

    if platform::remove_uninstall_key() && !platform::uninstall_key_exists() {
        say("已移除注册表卸载项");
    } else {
        failures.push(format!(
            "删除注册表项失败或未生效：HKCU\\{}",
            platform::UNINSTALL_KEY
        ));
    }

    let removed = platform::remove_desktop_shortcuts();
    if removed.is_empty() {
        say("没有需要移除的桌面快捷方式");
    } else {
        for p in &removed {
            say(&format!("已移除快捷方式 {}", p.display()));
        }
    }

    if dir.exists() {
        say(&format!(
            "NOTE: 安装目录仍然存在，本模式未删除任何文件：{}",
            dir.display()
        ));
    }

    if failures.is_empty() {
        say("");
        say("  残留清理完成（未删除任何文件或目录）");
        0
    } else {
        println!();
        println!("  残留清理未完全成功：{} 项失败", failures.len());
        for f in &failures {
            println!("    - {f}");
        }
        1
    }
}

/// 阶段二：从 `%TEMP%` 的副本里删除安装目录本身。
pub fn deferred_pass(ctx: &Context) -> i32 {
    // 给阶段一/启动器一点时间释放文件句柄
    std::thread::sleep(Duration::from_millis(900));

    // 重新校验（纵深防御）：标记 + 受保护路径 + 目录内容
    if !ctx.marker.is_file() {
        report_silent(&format!(
            "延迟清理：安装标记 {} 不存在，已跳过递归删除（{}）",
            MARKER_NAME,
            ctx.display()
        ));
        cleanup_self_copy(ctx);
        return 1;
    }
    if guards::is_protected_root(&ctx.install_dir) {
        report_silent(&format!(
            "延迟清理：受保护路径，拒绝删除（{}）",
            ctx.display()
        ));
        cleanup_self_copy(ctx);
        return 1;
    }

    // 先清掉标记与残余文件，再删目录（避免"目录删了一半、标记还在"的半截状态）
    let _ = std::fs::remove_file(&ctx.marker);
    for path in ctx.files_to_remove() {
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
    let ok = match std::fs::remove_dir_all(&ctx.install_dir) {
        Ok(()) => !ctx.install_dir.exists(),
        Err(_) => !ctx.install_dir.exists(),
    };
    if !ok {
        report_silent(&format!("延迟清理失败：目录仍存在 {}", ctx.display()));
    }
    cleanup_self_copy(ctx);
    if ok {
        0
    } else {
        1
    }
}

/// 让 `%TEMP%` 里的副本在下次重启时被系统删除（运行中的镜像无法自删）。
fn cleanup_self_copy(ctx: &Context) {
    // Windows **不允许运行中的镜像删除自己**（实测：对自身路径取 DELETE 权限被拒），
    // 因此唯一可行的是"标记为下次重启时删除"（系统会在重启早期、镜像尚未映射时删掉它）。
    // 代价是 %TEMP% 里留一个约 300 KB 的副本直到重启 —— 平台限制，已在帮助与文档里写明。
    platform::schedule_delete_on_reboot(&ctx.self_exe);
}

/// 结束本目录里的启动器，并在有界时间内等待其退出。
fn stop_launcher(ctx: &Context) -> Result<Vec<u32>, String> {
    let killed = platform::stop_launcher_in(&ctx.install_dir);
    if killed.is_empty() {
        return Ok(killed);
    }
    // 有界等待：文件被占用时删除会失败，等一会儿比立刻失败更可靠
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if !killed.iter().any(|p| dsh_core::is_process_alive(*p)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let alive: Vec<u32> = killed
        .iter()
        .copied()
        .filter(|p| dsh_core::is_process_alive(*p))
        .collect();
    if alive.is_empty() {
        Ok(killed)
    } else {
        Err(format!(
            "启动器进程在 10 秒内未退出（PID {}），部分文件可能仍被占用",
            alive
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// 把自身复制到 `%TEMP%` 并以 `--deferred-pass` 重入。
fn spawn_deferred_pass(ctx: &Context) -> Result<(), String> {
    let temp = std::env::temp_dir().join(format!("dsh-uninstall-{}.exe", std::process::id()));
    platform::copy_file(&ctx.self_exe, &temp)
        .map_err(|e| format!("复制自身到临时目录失败：{e}"))?;

    let mut args: Vec<String> = vec![
        "--deferred-pass".to_string(),
        ctx.install_dir.display().to_string(),
    ];
    if ctx.purge {
        args.push("--purge".to_string());
    }
    if ctx.silent {
        args.push("--silent".to_string());
    }
    std::process::Command::new(&temp)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动临时副本失败：{e}"))
}

/// 删除目录并复核（返回 Err 时目录仍存在）。
fn remove_dir_all_verified(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {
            if dir.exists() {
                Err("删除后目录仍存在".into())
            } else {
                Ok(())
            }
        }
        Err(e) => {
            if dir.exists() {
                Err(e.to_string())
            } else {
                Ok(())
            }
        }
    }
}

/// dry-run：把完整影响范围打印出来，改动为零。
fn print_plan(ctx: &Context) {
    info(ctx, "DRY-RUN：卸载影响范围（不会修改任何内容）");
    info(ctx, &format!("  安装目录            : {}", ctx.display()));
    info(
        ctx,
        &format!("  安装标记            : {}（已具备）", ctx.marker.display()),
    );

    let planned = ctx.files_to_remove();
    let present: Vec<PathBuf> = planned.iter().filter(|p| p.exists()).cloned().collect();
    info(
        ctx,
        &format!(
            "  将删除的安装文件    : {} 个（其中存在 {} 个）",
            planned.len(),
            present.len()
        ),
    );
    if !ctx.is_deferred_pass() {
        info(
            ctx,
            "      （正在运行的 dsh-uninstall.exe 由后台副本在第二阶段删除）",
        );
    }
    for p in &present {
        info(ctx, &format!("      - {}", p.display()));
    }

    info(
        ctx,
        &format!(
            "  注册表卸载项        : HKCU\\{} → {}",
            platform::UNINSTALL_KEY,
            if platform::uninstall_key_exists() {
                "存在，将被删除"
            } else {
                "不存在"
            }
        ),
    );

    let matched = platform::desktop_shortcuts();
    info(
        ctx,
        &format!("  桌面快捷方式        : {} 个将被删除", matched.len()),
    );
    for p in &matched {
        info(ctx, &format!("      - {}", p.display()));
    }

    let local = platform::local_dir();
    info(
        ctx,
        &format!(
            "  运行期数据          : {} → {}",
            local.display(),
            if local.exists() {
                "存在，将被删除（日志 / WebView2 profile / service.json）"
            } else {
                "不存在"
            }
        ),
    );
    let roaming = platform::roaming_dir();
    info(
        ctx,
        &format!(
            "  用户配置            : {} → {}",
            roaming.display(),
            if ctx.purge {
                "将被删除（--purge）"
            } else {
                "**保留**（默认）"
            }
        ),
    );

    let foreign = guards::foreign_entries(
        &ctx.install_dir,
        crate::plan::PAYLOADS,
        crate::plan::LEGACY_PAYLOADS,
        MARKER_NAME,
    );
    let dir_action = if guards::is_protected_root(&ctx.install_dir) {
        "护栏拒绝递归删除（受保护路径）"
    } else if foreign.is_empty() {
        "递归删除（由后台副本执行，约 1 秒后）"
    } else {
        "保留目录（含有不属于本安装包的内容）"
    };
    info(ctx, &format!("  安装目录本身        : {dir_action}"));
    for name in &foreign {
        info(ctx, &format!("      - 外来项目：{name}"));
    }
    info(
        ctx,
        "DRY-RUN：以上为完整影响范围；未改动任何内容，也未校验卸载后置条件。",
    );
}

/// 打印结论并给出退出码。
fn summarize(ctx: &Context, failures: &[String], notes: &[String], deferred: bool) -> i32 {
    for n in notes {
        println!("NOTE: {n}");
    }
    if failures.is_empty() {
        if !ctx.silent {
            println!();
            println!(
                "  卸载完成{}",
                if deferred {
                    "（安装目录正在后台移除）"
                } else {
                    ""
                }
            );
            println!("  已保留用户配置（如需一并删除请用 --purge）");
        }
        return 0;
    }
    println!();
    println!("  卸载未完全成功：{} 项失败", failures.len());
    for f in failures {
        println!("    - {f}");
    }
    1
}

/// 常规信息（`--silent` 时静默）。
fn info(ctx: &Context, msg: &str) {
    if !ctx.silent {
        println!("{msg}");
    }
}

/// 阶段二的报告：即使是 silent 也要把**问题**说清楚（走 stderr + 退出码）。
fn report_silent(msg: &str) {
    eprintln!("{msg}");
}
