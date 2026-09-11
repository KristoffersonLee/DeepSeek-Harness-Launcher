//! 命令行参数解析。

/// 解析后的命令行参数。
#[derive(Debug, Default, Clone, Copy)]
pub struct Args {
    /// `--selftest`：运行隐藏自检模式（启动 → 就绪 → 停止）后退出
    pub selftest: bool,
    /// `--guide`：启动时打开新手指引
    pub guide: bool,
    /// `--settings`：启动时打开设置窗口
    pub settings: bool,
    /// `--ipc-probe`：自检用——打开设置页后自动模拟一次按钮点击，验证 IPC 往返
    pub ipc_probe: bool,
    /// `--quit`：请求**已在运行**的实例优雅退出（本进程随即退出）。
    ///
    /// 为什么需要：v5.0.0 之前没有任何脚本可用的退出入口，发布脚本只能强杀，
    /// 而强杀会切断正在进行的 dsh 会话。有了它，`finish-release.ps1` 之类可以
    /// **先优雅退出再覆盖发布产物**。
    pub quit: bool,
    /// `--build-info`：打印构建/修复标记后退出（**发布脚本用它判定产物真伪**）。
    ///
    /// 为什么需要：Windows 不允许覆盖正在运行的 exe，而"先改名让位、再放新产物"
    /// 的部署脚本必须能**确认落地的是新版而不是上次留下的旧文件**。
    /// 靠二进制里搜字符串不可靠（`debug_assert!` 与注释都不会进 release 产物），
    /// 因此提供一个可执行的判定入口。
    pub build_info: bool,
    /// `--probe-identity <PID>`：诊断用 —— 打印启动器对某个 PID 的**身份判定过程**。
    ///
    /// 为什么需要：`is_dsh_harness_process()` 是"防误杀"的关键闸门。一旦它把真正的
    /// dsh 判成"非 dsh"，后果是**接管失败 + 服务被停掉后起不来**（实测踩到）。
    /// 而这类失败在日志里只表现为一行"被非 dsh 程序占用"，看不出究竟卡在哪一步。
    /// 本入口把每一步（镜像名 / 映像路径 / 是否系统目录 / 是否含 dsh 特征 / 祖先链）
    /// 逐项打印，使误判可定位、可回归验证。
    pub probe_identity: Option<u32>,
    /// `-V` / `--version`：打印版本（含发布通道标签）后退出。
    ///
    /// 为什么需要：这是**任何安装包/CI 都必需的最小可用接口**。此前启动器完全没有
    /// 这个入口——`DSHLauncher.exe --version` 会被当成"普通启动"：抢互斥体、
    /// 弹出窗口、甚至唤起已有实例。于是"安装后验证版本"这一步无法自动化。
    pub version: bool,
    /// `-h` / `--help`：打印用法后退出（同样必须在单实例检查之前处理）。
    pub help: bool,
}

/// 发布通道标签（**不是版本号的一部分**）。
///
/// Cargo 版本必须是合法的 semver（`5.0.0`）。`-LTS` 若写进版本字符串就变成
/// **预发布版本**（`5.0.0-LTS` < `5.0.0`，且 npm/dist-tag 语义完全不同），
/// 因此 LTS 只作为*发布标签*出现在用户可见输出、文档与制品名中。
pub const RELEASE_CHANNEL: &str = "LTS";

/// `--version` 的输出文本。
///
/// 版本号**只**来自编译期注入的 `CARGO_PKG_VERSION`（唯一来源 = workspace
/// `Cargo.toml`），本函数不出现任何手写版本字面量。
pub fn version_text() -> String {
    format!(
        "{} {} {} ({})\n",
        dsh_core::APP_NAME,
        env!("CARGO_PKG_VERSION"),
        RELEASE_CHANNEL,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    )
}

/// `--help` 的输出文本（选项、退出码都必须与实现一致）。
pub fn help_text() -> String {
    format!(
        "{name} {version} {channel} —— dsh web 的 Windows 桌面启动器\n\
         \n\
         用法: DSHLauncher.exe [选项]\n\
         \n\
         选项:\n\
         \x20 无参数                 启动启动器（托盘常驻，自动启动 / 接管 dsh web 服务）\n\
         \x20 -s, --settings         启动时直接打开设置窗口\n\
         \x20 -g, --guide            启动时打开新手指引\n\
         \x20 -V, --version          打印版本后退出\n\
         \x20 -h, --help             打印本帮助后退出\n\
         \x20 --quit                 请求已在运行的实例优雅退出（服务去留按 service_lifecycle 决定）\n\
         \x20 --build-info           打印构建与修复标记后退出（发布脚本据此判定产物真伪）\n\
         \x20 --selftest             运行自检（启动 → 就绪 → 停止）后退出\n\
         \x20 --probe-identity <PID> 打印对某个 PID 的 dsh 身份判定过程（防误杀诊断）\n\
         \x20 --ipc-probe            自检用：打开设置页并模拟一次「保存设置」点击\n\
         \n\
         退出码: 0 = 成功 / 1 = 自检失败 / 2 = 已有实例导致自检无法执行 / 3 = --quit 未收到回执\n",
        name = dsh_core::APP_NAME,
        version = env!("CARGO_PKG_VERSION"),
        channel = RELEASE_CHANNEL,
    )
}

/// 本构建包含的修复标记。
///
/// 每当改动影响"产物是否可安全替换"的行为时，就往这个列表里加一条 ——
/// 部署脚本据此判定"根产物是不是我要的那一版"。
///
/// 标记命名不带版本号：此前的写法是在注释里加版本前缀（`// <版本>：…`），
/// 而全仓库只保留 `5.0.0` 一个版本号，标记里的版本前缀只会制造第二处"版本事实"。
pub const FIX_MARKERS: &[&str] = &[
    // 修复 ServiceHandle::start() 持锁 spawn 导致的**确定性死锁**
    //（症状：只有托盘图标、无窗口、菜单点不动、日志停在"托盘图标已创建。"）。
    // 定稿轮进一步把 `ProcessManager` 拆到独立互斥量，spawn 全程不持有 ServiceInner 锁。
    "deadlock-fix-service-start-lock-scope",
    // 默认服务独立于启动器（退出/崩溃/升级不再切断会话）
    "service-independent-default",
    // token 捕获竞态 + 只有带 token 才导航；定稿轮补上「每帧驱动等待预算」，
    // 否则没有 token 时窗口永远不会出现。
    "token-capture-race-fix",
    "token-wait-tick-driven",
    // 主题采样结果改为「取出并清空」。旧实现只读不清，同一结果被逐帧重复
    // 处理与打印（实测刷出 4557 行日志、仅 35 种内容），且每帧多跑一次 JS 采样。
    "theme-sample-read-and-clear",
    // 「清理归档会话」加二次确认 + 清理后自动重启服务。
    // 旧实现会静默停掉正在服务的 dsh（用户以为启动器自己断了他的会话），
    // 且清理后不把服务拉回来（界面打不开，只能手动去点「启动服务」）。
    "cleanup-confirm-and-restore",
    // 清理前探测**活跃会话**（会话运行器进程 + 会话文件近期写入），
    // 命中则把确认升级为警告并点名正在跑的会话 —— 避免清理打断正在进行的对话/任务。
    "active-session-guard",
    // 退出询问随「服务独立」设置变化（独立不弹窗、tied 每次询问）；
    // 且**取消勾选该设置不再重启/打断服务**（Job 归属无法事后改变，改为下次重启生效）；
    // 身份判定区分「明确不是 dsh」与「读不到信息」，后者不再被报成"非 dsh 程序占用"。
    "exit-policy-and-lifecycle-safety",
    // 定稿轮：所有阻塞操作（终止进程树 / 配置变更重启 / 归档清理）移出 UI 线程，
    // 事件循环不再被冻住。
    "ui-thread-never-blocks",
    // 定稿轮：失联监测不再把「Starting 期间端口还没监听」误判为失联
    //（冷启动 30 秒 > 12 秒阈值，旧实现会把状态打成 Error 并清掉 service.json）。
    "monitor-startup-grace",
    // 定稿轮：设置页新增「恢复默认设置」（此前文档承诺的重置入口并不存在）。
    "settings-reset-wired",
    // 发布工程轮：
    //  - 提供 `--version` / `--help`（安装包与 CI 校验版本的最小接口；此前
    //    `--version` 会被当成普通启动——抢互斥体、弹窗）；
    //  - 启动服务（对账 / 端口探测 / 锁清理 / spawn）整体移出 UI 线程；
    //  - 「打开界面」泵与唤起待办共用**按时间限流**的端口探测（此前逐帧阻塞 connect）。
    "cli-version-help",
    "service-start-offloaded",
    "port-probe-throttled",
    // 进程管理加固：存活判定改用 WaitForSingleObject（消除退出码 259 歧义）、
    // 进程树终止改为迭代 + 去重（不再递归，父子成环/深树都不会栈溢出）。
    "process-liveness-and-tree-hardened",
    // 内嵌界面认证自愈：`dsh web` 的 launch token 只存在于 dsh 进程内存里，
    // 接管"别处启动"的服务时读不到它；此时若 WebView2 里也没有有效的签名 cookie，
    // 界面就是 `dsh web authentication required`（实测用户可见的故障）。
    // 现在页面加载后会在**页面上下文**里自检（裸 socket 探测区分不了"已有 cookie"），
    // 命中则让启动器自己重启服务以取得带 token 的地址（无活跃会话时自动、否则询问）。
    "auth-required-self-heal",
];

/// 渲染 `--build-info` 输出（键值对，便于脚本解析）。
pub fn build_info_text() -> String {
    format!(
        "version={}\nprofile={}\nfixes={}\n",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        FIX_MARKERS.join(",")
    )
}

/// 解析命令行参数（进程参数入口）。
pub fn parse_args() -> Args {
    parse_from(std::env::args().skip(1))
}

/// 从任意参数序列解析（`parse_args` 只是把进程参数喂进来）。
///
/// 拆出本函数是为了**可单测**：`std::env::args()` 在测试进程里无法替换，
/// 于是"未知参数被忽略"、"`--probe-identity=` 等号形式"、"`-V` 短选项"这些
/// 契约此前完全没有测试覆盖。
pub fn parse_from<I: IntoIterator<Item = String>>(argv: I) -> Args {
    let argv: Vec<String> = argv.into_iter().collect();
    let mut args = Args::default();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--selftest" | "selftest" => args.selftest = true,
            "--guide" | "-g" => args.guide = true,
            "--settings" | "-s" => args.settings = true,
            "--ipc-probe" => {
                args.ipc_probe = true;
                args.settings = true; // 探针必须先有设置窗口
            }
            "--quit" => args.quit = true,
            "--build-info" => args.build_info = true,
            "--version" | "-V" => args.version = true,
            "--help" | "-h" => args.help = true,
            // 需要参数的开关：下一个 token 是 PID（也接受 --probe-identity=1234 形式）
            "--probe-identity" => {
                if let Some(v) = argv.get(i + 1).and_then(|s| s.parse::<u32>().ok()) {
                    args.probe_identity = Some(v);
                    i += 1;
                }
            }
            a if a.starts_with("--probe-identity=") => {
                args.probe_identity = a["--probe-identity=".len()..].parse::<u32>().ok();
            }
            _ => {}
        }
        i += 1;
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(list: &[&str]) -> Args {
        parse_from(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_version_and_help_switches() {
        // 发布验证的最小接口：短选项与长选项都必须被识别
        assert!(args_of(&["--version"]).version);
        assert!(args_of(&["-V"]).version);
        assert!(args_of(&["--help"]).help);
        assert!(args_of(&["-h"]).help);
        // 二者都不应顺带打开窗口
        let a = args_of(&["--version"]);
        assert!(!a.settings && !a.guide && !a.selftest && !a.ipc_probe);
    }

    #[test]
    fn ipc_probe_implies_settings_window() {
        // 探针必须先有设置窗口才能注入点击，这一隐含契约不能丢
        assert!(args_of(&["--ipc-probe"]).settings);
    }

    #[test]
    fn parses_probe_identity_both_spellings() {
        assert_eq!(
            args_of(&["--probe-identity", "4321"]).probe_identity,
            Some(4321)
        );
        assert_eq!(
            args_of(&["--probe-identity=4321"]).probe_identity,
            Some(4321)
        );
        // 非法值不得 panic，也不得吞掉后续合法参数
        let a = args_of(&["--probe-identity", "abc", "--quit"]);
        assert_eq!(a.probe_identity, None);
        assert!(a.quit, "非法 PID 之后的参数仍必须被解析");
    }

    #[test]
    fn unknown_arguments_are_ignored() {
        let a = args_of(&["--nope", "junk", "-x"]);
        assert!(!a.selftest && !a.version && !a.help && !a.settings && !a.quit);
    }

    #[test]
    fn version_and_help_text_are_version_and_channel_aware() {
        let v = version_text();
        assert!(v.contains(env!("CARGO_PKG_VERSION")), "{v}");
        assert!(v.contains(RELEASE_CHANNEL), "{v}");
        assert!(v.starts_with(dsh_core::APP_NAME), "{v}");

        let h = help_text();
        for needle in [
            "--version",
            "--help",
            "--quit",
            "--build-info",
            "--selftest",
            "--probe-identity",
            "--ipc-probe",
            "--settings",
            "--guide",
            "退出码",
        ] {
            assert!(h.contains(needle), "帮助文本缺少 {needle}");
        }
    }

    #[test]
    fn build_info_text_reports_version_profile_and_unique_markers() {
        let t = build_info_text();
        assert!(t.contains(&format!("version={}", env!("CARGO_PKG_VERSION"))));
        assert!(t.contains("fixes="));
        // 标记必须非空且**不重复**：重复意味着同一修复被记录两次，会让
        // 「产物真伪判定」失去意义
        assert!(!FIX_MARKERS.is_empty());
        let mut sorted: Vec<&str> = FIX_MARKERS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "FIX_MARKERS 存在重复项");
        assert!(FIX_MARKERS.iter().all(|m| !m.trim().is_empty()));
    }
}
