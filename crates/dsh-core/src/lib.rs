//! dsh-core —— 零 GUI 依赖的纯逻辑层。
//!
//! 这一层不引用任何窗口/WebView 相关 crate，因此全部可被 `cargo test` 覆盖。

pub mod config;
pub mod dsh;
pub mod icon;
pub mod log;
pub mod maintenance;
pub mod port;
pub mod probe;
pub mod process;
pub mod service_record;
pub mod single_instance;

pub use config::{ConfigError, ServiceLifecycle, Settings};
pub use dsh::{Dsh, DshError};
pub use log::{redact_secrets, RollingLogger};
pub use maintenance::{
    archived_session_count, clean_stale_dsh_locks, delete_archived_sessions, dsh_home,
    probe_active_sessions, ActiveSessions, MaintenanceError, ACTIVE_SESSION_WINDOW,
};
pub use port::{find_pid_on_port, is_port_free_to_bind, is_port_listening};
pub use probe::{ProbeError, ReadyProbe};
pub use process::{
    any_node_running, any_process_running, classify_dsh_identity, is_dsh_harness_process,
    is_process_alive, kill_process_tree, process_image_name, process_image_path,
    process_start_time, ChildLifecycle, DshIdentity, IdentitySteps, JobHandle, ProcessError,
    ProcessManager, ProcessTable,
};
pub use service_record::{reconcile, Reconciliation, ServiceRecord};
pub use single_instance::{
    request_existing_instance_quit, request_existing_instance_quit_within, request_quit_named,
    signal_existing_instance, ActivationEvent, QuitAckEvent, QuitEvent, QuitOutcome,
    SingleInstance, ACTIVATE_EVENT_NAME, MUTEX_NAME, QUIT_ACK_EVENT_NAME, QUIT_ACK_TIMEOUT,
    QUIT_EVENT_NAME,
};

/// 应用名称，用于路径、互斥体名、托盘提示、注册表键名。
pub const APP_NAME: &str = "DSHLauncher";

/// 用户可见**产品名**（安装向导标题、注册表 DisplayName、安装标记首行都用它）。
///
/// ## 为什么必须与 [`APP_NAME`] 并列存在
///
/// 两者是不等价的字符串：`APP_NAME` 是内部标识（目录名 / 互斥体名 / 注册表键名），
/// `PRODUCT_NAME` 是展示名。安装器的 `BuildMarkerText()` 写的是**产品名**，
/// 而卸载器此前用 `content.contains(APP_NAME)` 校验安装标记 —— 该断言在真实安装上
/// **永远为假**（`DeepSeek Harness Launcher` 不含子串 `DSHLauncher`），
/// 后果是**卸载器拒绝执行任何删除**（退出码 2）：注册表项、快捷方式、载荷与运行期数据全部残留。
/// 现在两侧共用本条常量，并由 `tools/check-consistency.ps1` 交叉校验安装器的 `AppName`。
pub const PRODUCT_NAME: &str = "DeepSeek Harness Launcher";

/// 平台目录取不到时退化为当前目录（**仅用于极端环境**：没有 HOME/APPDATA 的会话）。
///
/// 唯一实现：配置目录与运行期数据目录都必须用同一套退化语义，否则
/// 「读配置的路径」与「写日志的路径」会在异常环境下分叉。
fn dir_or_current(dir: Option<std::path::PathBuf>) -> std::path::PathBuf {
    dir.unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// `%APPDATA%\DSHLauncher`，**用户配置**目录（卸载默认保留）。
pub fn app_data_dir() -> std::path::PathBuf {
    dir_or_current(dirs::config_dir()).join(APP_NAME)
}

/// `%LOCALAPPDATA%\DSHLauncher`，**可再生的运行期数据**根目录。
///
/// 日志、WebView2 profile、Edge 回退 profile 与**服务簿记**都放这里；卸载器会整目录
/// 清理它，而 `%APPDATA%` 下的用户配置默认保留。这条分界线必须严格：
/// 放错的直接后果是「卸载后仍残留运行期状态」或「卸载顺手删掉用户配置」。
pub fn local_data_dir() -> std::path::PathBuf {
    dir_or_current(dirs::data_local_dir()).join(APP_NAME)
}

/// `%LOCALAPPDATA%\DSHLauncher\logs`，日志目录。
pub fn log_dir() -> std::path::PathBuf {
    local_data_dir().join("logs")
}

/// `%LOCALAPPDATA%\DSHLauncher\webview2-profile`，WebView2 用户数据目录。
///
/// **必须显式指定**：否则 WebView2 会在 exe 所在目录生成 `<exe>.WebView2\` 文件夹，
/// 这既破坏"单文件分发"，在 Program Files 等只读目录下更会直接初始化失败。
pub fn webview_data_dir() -> std::path::PathBuf {
    local_data_dir().join("webview2-profile")
}

/// `%LOCALAPPDATA%\DSHLauncher\edge-profile`，Edge 精简窗口回退用的用户目录。
pub fn edge_profile_dir() -> std::path::PathBuf {
    local_data_dir().join("edge-profile")
}
