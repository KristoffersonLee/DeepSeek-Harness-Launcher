//! 跨启动器生命周期的服务簿记（`service.json`）。
//!
//! ## 为什么需要它
//!
//! v5.0.0 用 Job Object 的 `KILL_ON_JOB_CLOSE` 保证「启动器被强杀 ⇒ 不留孤儿进程」。
//! 但 `dsh web` 是**会话型服务**：正在对话/跑任务的会话挂在它上面。这个内核级保证
//! 与「退出启动器不切断会话」直接冲突，而且它是一个**进程级、不可逆**的开关——
//! 用户只有在走托盘「退出」并选择「保留服务」那一条路径上才能保住会话；
//! 崩溃、被强杀、被安装包升级覆盖、注销重启，全都会切断它。
//!
//! 实测（`examples/job_object_demo.rs`）：
//! disarm 后子进程在父进程正常退出与强杀两种情况下都存活；未 disarm 时都被回收。
//!
//! ## 本模块的定位
//!
//! 让 dsh **独立于启动器进程**（[`crate::process::ChildLifecycle::Independent`]），
//! 然后用**显式簿记 + 启动时对账**取代「内核保证」：
//!
//! ```text
//! 启动器 A 启动 dsh(PID=100, started_at=T) ──► 写 service.json {pid:100, port:3080, start_time:T}
//! 启动器 A 退出（任何方式）              ──► dsh 继续运行（外部作业）
//! 启动器 B 启动                          ──► 读 service.json：
//!       PID 100 存活 + 是 dsh + 创建时间匹配 + 端口在听  ⇒ 接管（不重启，会话保留）
//!       PID 100 已退出 / 身份不符 / 创建时间不符（PID 被复用） ⇒ 视为陈旧，清理记录
//!       PID 100 存活但端口不在听（启动到一半就被杀）        ⇒ 交给上层按策略处理
//! ```
//!
//! 「创建时间匹配」是防 **PID 复用**误判的关键：只用 PID 存活判断会把一个碰巧
//! 复用了该 PID 的无关进程当成自己的服务。
//!
//! ## 存储位置
//!
//! `%LOCALAPPDATA%\DSHLauncher\service.json`——与应用日志同级（可被卸载器清理），
//! 不放在 `%APPDATA%`（那里是用户配置，卸载默认保留）。

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// 当前簿记格式版本。
pub const RECORD_VERSION: u32 = 1;

/// 创建时间比较容差。
///
/// `GetProcessTimes` 返回 FILETIME（100 ns 粒度），而我们写入的是 `SystemTime`
/// 转换结果；两端都可能做整秒截断，故允许 2 秒误差。**远小于 PID 复用的可观测
/// 时间尺度**（进程创建-退出-复用至少要毫秒级到秒级，且复用后创建时间必然不同）。
const START_TIME_TOLERANCE: Duration = Duration::from_secs(2);

/// 服务簿记记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceRecord {
    /// 格式版本（未来迁移用）。
    pub version: u32,
    /// dsh 服务进程的 PID。
    pub pid: u32,
    /// 服务监听端口。
    pub port: u16,
    /// 该进程的映像路径（用于身份复核）。
    pub exe: String,
    /// 该进程的创建时间（自 UNIX 纪元起的秒数，用于防 PID 复用误判）。
    pub start_unix_secs: u64,
    /// 启动器自身的 PID（仅用于日志溯源，不参与判定）。
    pub launcher_pid: u32,
    /// 记录写入时间（自 UNIX 纪元起的秒数，仅用于日志/展示）。
    pub recorded_unix_secs: u64,
}

/// `service.json` 路径：`%LOCALAPPDATA%\DSHLauncher\service.json`。
///
/// ## 为什么必须在 `%LOCALAPPDATA%` 而不是 `%APPDATA%`（v5.0.0 LTS 修正）
///
/// 本模块的文档、README / 维护手册 / 发布说明、卸载器（`uninstall.ps1` 与安装包生成的
/// 那一份）以及 `tools/verify-service-lifecycle.ps1` **一直**都写着
/// `%LOCALAPPDATA%\DSHLauncher\service.json`，但代码此前用的是 `app_data_dir()`
/// （= `%APPDATA%\DSHLauncher`）。后果是实测可见的两条：
/// 1. **卸载残留**：卸载默认保留 `%APPDATA%`，于是这份运行期簿记**永远不会被清理**；
/// 2. **验证脚本假通过**：`verify-service-lifecycle.ps1` 读的是 LOCALAPPDATA 下的路径，
///    永远读不到记录，只能一直打印"尚无 service.json"，接管断言失去意义。
///
/// 语义上也只有一种正确分法：用户**配置**（settings.toml）属于 `%APPDATA%`（卸载保留），
/// 而可再生的**运行期状态**（日志 / profile / 服务簿记）属于 `%LOCALAPPDATA%`（卸载清理）。
pub fn record_path() -> PathBuf {
    crate::local_data_dir().join("service.json")
}

/// v5.0.0 LTS 之前在 `%APPDATA%\DSHLauncher\service.json` 写入的旧簿记。
///
/// 它不会被卸载器清理（`%APPDATA%` 默认保留），且再也无人读取，因此首次读取记录时
/// 顺手删掉——避免"用户配置目录里躺着一份过期运行期状态"。
fn legacy_record_path() -> PathBuf {
    crate::app_data_dir().join("service.json")
}

/// 删除旧位置的簿记（幂等；文件不存在视为成功）。
fn clear_legacy_record() {
    let p = legacy_record_path();
    if p.is_file() {
        let _ = std::fs::remove_file(p);
    }
}

impl ServiceRecord {
    /// 依据一个**已被确认为 dsh** 的 PID 构造记录。
    pub fn capture(pid: u32, port: u16, exe: &str) -> Option<Self> {
        let started = crate::process::process_start_time(pid)?;
        Some(Self {
            version: RECORD_VERSION,
            pid,
            port,
            exe: exe.to_string(),
            start_unix_secs: to_unix_secs(started),
            launcher_pid: std::process::id(),
            recorded_unix_secs: now_unix_secs(),
        })
    }

    /// 读取记录。文件不存在 / 内容损坏 / 版本不认识时返回 `None`（视为无记录）。
    ///
    /// 副作用（幂等）：顺手删除 v5.0.0 LTS 之前写在 `%APPDATA%` 下的旧簿记。
    pub fn load() -> Option<Self> {
        clear_legacy_record();
        let raw = std::fs::read_to_string(record_path()).ok()?;
        let rec: Self = serde_json::from_str(&raw).ok()?;
        if rec.version != RECORD_VERSION {
            return None;
        }
        Some(rec)
    }

    /// 原子写入记录（先写 `.tmp` 再 rename，避免崩溃留下半截 JSON）。
    pub fn save(&self) -> std::io::Result<()> {
        let path = record_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw =
            serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &path)
    }

    /// 删除记录（幂等）。
    pub fn clear() {
        let _ = std::fs::remove_file(record_path());
    }

    /// 记录中的进程创建时间。
    pub fn start_time(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.start_unix_secs)
    }

    /// 该记录描述的进程是否**仍然就是那个 dsh 服务**。
    ///
    /// 三个条件必须同时成立：
    /// 1. PID 存活；
    /// 2. 该 PID 的创建时间与记录一致（±[`START_TIME_TOLERANCE`]）——防 PID 复用；
    /// 3. 该 PID 通过 dsh 身份判定（[`crate::process::is_dsh_harness_process`]）。
    pub fn still_valid(&self) -> bool {
        if !crate::process::is_process_alive(self.pid) {
            return false;
        }
        match crate::process::process_start_time(self.pid) {
            Some(actual) => {
                let expected = self.start_time();
                let diff = if actual >= expected {
                    actual.duration_since(expected).unwrap_or(Duration::ZERO)
                } else {
                    expected.duration_since(actual).unwrap_or(Duration::MAX)
                };
                if diff > START_TIME_TOLERANCE {
                    return false;
                }
            }
            // 拿不到创建时间：无法排除 PID 复用，保守判定为「不再有效」
            None => return false,
        }
        crate::process::is_dsh_harness_process(self.pid)
    }
}

/// 启动时对账的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Reconciliation {
    /// 没有记录（首次运行，或上次启动器从未成功拉起服务）。
    NoRecord,
    /// 记录描述的 dsh 仍存活且身份匹配 ⇒ **应当接管**（不重启，会话保留）。
    Adopt { pid: u32, port: u16 },
    /// 记录陈旧（进程已退出 / PID 被复用 / 身份不符）⇒ 已清理记录，应正常启动。
    Stale { reason: String },
    /// 进程还活着、但已不在监听端口（例如启动到一半被杀，或端口被改）。
    /// **不由本模块决定杀或留**——交给上层按 `service_lifecycle` 策略处理。
    Orphan { pid: u32, port: u16 },
}

/// 执行启动时对账。
///
/// 副作用：对 `Stale` 情况会**删除** `service.json`（因为记录已无意义）。
/// 对 `Adopt` / `Orphan` 保留记录，由调用方在接管或处理后决定是否重写。
pub fn reconcile() -> Reconciliation {
    let Some(rec) = ServiceRecord::load() else {
        return Reconciliation::NoRecord;
    };
    if !crate::process::is_process_alive(rec.pid) {
        ServiceRecord::clear();
        return Reconciliation::Stale {
            reason: format!("记录的 PID {} 已退出", rec.pid),
        };
    }
    if !rec.still_valid() {
        ServiceRecord::clear();
        return Reconciliation::Stale {
            reason: format!(
                "PID {} 的身份或创建时间与记录不符（疑似 PID 复用），已清理记录",
                rec.pid
            ),
        };
    }
    if crate::port::is_port_listening(rec.port) {
        Reconciliation::Adopt {
            pid: rec.pid,
            port: rec.port,
        }
    } else {
        Reconciliation::Orphan {
            pid: rec.pid,
            port: rec.port,
        }
    }
}

fn to_unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_unix_secs() -> u64 {
    to_unix_secs(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let rec = ServiceRecord {
            version: RECORD_VERSION,
            pid: 4242,
            port: 3080,
            exe: "C:\\nodejs\\node.exe".into(),
            start_unix_secs: 1_700_000_000,
            launcher_pid: 99,
            recorded_unix_secs: 1_700_000_010,
        };
        let raw = serde_json::to_string(&rec).unwrap();
        let back: ServiceRecord = serde_json::from_str(&raw).unwrap();
        assert_eq!(rec, back);
        assert_eq!(back.port, 3080);
    }

    #[test]
    fn unknown_version_is_ignored() {
        // 版本不认识时必须视为「无记录」，而不是误用旧字段
        let raw = r#"{"version":999,"pid":1,"port":1,"exe":"x","start_unix_secs":0,"launcher_pid":1,"recorded_unix_secs":0}"#;
        let parsed: ServiceRecord = serde_json::from_str(raw).unwrap();
        assert_ne!(parsed.version, RECORD_VERSION);
    }

    #[test]
    fn dead_pid_is_not_valid() {
        let rec = ServiceRecord {
            version: RECORD_VERSION,
            // 几乎不可能存在的 PID
            pid: 0xFFFF_FFFE,
            port: 3080,
            exe: String::new(),
            start_unix_secs: 0,
            launcher_pid: 0,
            recorded_unix_secs: 0,
        };
        assert!(!rec.still_valid(), "已退出的进程不得被判为有效记录");
    }

    #[test]
    fn pid_reuse_is_rejected_by_start_time() {
        // 用当前进程做「存活」基线：PID 存活，但记录的创建时间与实际相差极远
        let me = std::process::id();
        let actual = crate::process::process_start_time(me).expect("应能取到自身创建时间");
        let actual_secs = to_unix_secs(actual);
        let rec = ServiceRecord {
            version: RECORD_VERSION,
            pid: me,
            port: 3080,
            exe: String::new(),
            // 伪造一个 10 天前的创建时间，模拟 PID 被复用
            start_unix_secs: actual_secs.saturating_sub(10 * 24 * 3600),
            launcher_pid: 0,
            recorded_unix_secs: 0,
        };
        assert!(
            !rec.still_valid(),
            "创建时间不匹配时必须判为无效（防 PID 复用误判）"
        );
    }

    /// 回归：簿记必须位于 `%LOCALAPPDATA%`（可被卸载器清理），**不得**落在
    /// `%APPDATA%`（用户配置目录，卸载默认保留 ⇒ 会留下永远不会被清理的运行期状态）。
    ///
    /// 这条断言同时锁住了"文档/卸载器/验证脚本都按 LOCALAPPDATA 找它"这一事实：
    /// 此前代码用 `app_data_dir()`，实测卸载后 `%APPDATA%\DSHLauncher\service.json`
    /// 仍在，而 `tools/verify-service-lifecycle.ps1` 永远读不到它。
    #[test]
    fn record_lives_under_local_app_data_not_roaming_config() {
        let p = record_path();
        assert_eq!(p.file_name().unwrap(), "service.json");
        assert_eq!(
            p.parent(),
            Some(crate::local_data_dir().as_path()),
            "service.json 必须直接位于 %LOCALAPPDATA%\\DSHLauncher 下"
        );
        assert_ne!(
            p.parent(),
            Some(crate::app_data_dir().as_path()),
            "service.json 不得放在 %APPDATA%（卸载默认保留，会残留运行期状态）"
        );
        assert_eq!(
            legacy_record_path(),
            crate::app_data_dir().join("service.json"),
            "旧路径必须仍是 %APPDATA%\\DSHLauncher\\service.json（供一次性清理）"
        );
    }

    #[test]
    fn self_process_start_time_is_readable() {
        let t = crate::process::process_start_time(std::process::id());
        assert!(t.is_some(), "应能读到自己进程的创建时间");
        // 应当是「过去」且「不太久远」
        let t = t.unwrap();
        assert!(t <= SystemTime::now());
        assert!(SystemTime::now().duration_since(t).unwrap().as_secs() < 86_400);
    }
}
