//! 强类型配置：`%APPDATA%\DSHLauncher\settings.toml`
//!
//! 与 v4 的关键差异：解析失败**返回 Err 而不是静默用默认值**，由 UI 明确提示并可一键重置。
//! 首次运行时若存在 v4 的 `settings.ini`，会自动迁移（README 承诺重装保留设置）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::app_data_dir;

/// 当前配置 schema 版本。
///
/// 用途：未来做**语义变更**（而非仅追加字段）时有迁移锚点。目前加载到更高版本时
/// 按「向前兼容」处理并在日志里留痕（见 [`Settings::load`]）。
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// 子进程生命周期策略（映射到 [`crate::process::ChildLifecycle`]）。
///
/// 为什么做成配置项：`dsh web` 是**会话型服务**。把它的生死绑死在启动器上会导致
/// 「退出 / 崩溃 / 被安装包升级覆盖启动器 ⇒ 正在进行的会话被切断」。默认值因此改为
/// [`ServiceLifecycle::Independent`]，同时保留 [`ServiceLifecycle::Tied`]
/// 供需要「启动器一死就回收一切」的用户选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceLifecycle {
    /// 默认：dsh 独立于启动器（不挂 Job），退出/崩溃/升级都不中断会话；
    /// 下次启动由 `service.json` 对账后自动接管。
    #[default]
    Independent,
    /// 兼容 v5.0.0 语义：dsh 挂在启动器的 Job 上，启动器被强杀时一并回收
    /// （「零孤儿」的内核级保证），代价是退出启动器即中断会话。
    Tied,
}

impl ServiceLifecycle {
    /// 转为 `process` 层的生命周期类型。
    pub fn to_child_lifecycle(self) -> crate::process::ChildLifecycle {
        match self {
            ServiceLifecycle::Independent => crate::process::ChildLifecycle::Independent,
            ServiceLifecycle::Tied => crate::process::ChildLifecycle::Tied,
        }
    }

    /// 人类可读名（日志用）。
    pub fn label(self) -> &'static str {
        match self {
            ServiceLifecycle::Independent => "independent（服务独立于启动器）",
            ServiceLifecycle::Tied => "tied（服务随启动器退出而停止）",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// 配置 schema 版本（保存时补全，便于将来迁移）。
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_tray_on_close")]
    pub tray_on_close: bool,
    #[serde(default)]
    pub node_path: String,
    /// dsh 进程工作目录，决定 Harness 的 workspace。留空表示沿用 dsh 默认。
    #[serde(default)]
    pub work_dir: String,
    /// 子进程生命周期策略（见 [`ServiceLifecycle`]）。
    #[serde(default)]
    pub service_lifecycle: ServiceLifecycle,
    /// 启动时若发现「上次遗留、且已不在监听端口」的 dsh 进程，是否结束它。
    ///
    /// 默认 `true`：这类进程通常是「启动到一半就被杀」的残留，占资源但不可用；
    /// 结束它是安全的——它没有任何活跃会话（端口都没在听）。
    #[serde(default = "default_true")]
    pub stop_stale_orphan: bool,
}

fn default_port() -> u16 {
    3080
}
fn default_tray_on_close() -> bool {
    true
}
fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}
fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            port: default_port(),
            tray_on_close: default_tray_on_close(),
            node_path: String::new(),
            work_dir: String::new(),
            service_lifecycle: ServiceLifecycle::default(),
            stop_stale_orphan: true,
        }
    }
}

impl Settings {
    pub fn file_path() -> PathBuf {
        app_data_dir().join("settings.toml")
    }

    /// 旧版 ini 路径（v4 及更早）。
    fn legacy_ini_path() -> PathBuf {
        app_data_dir().join("settings.ini")
    }

    /// 读取配置。文件不存在时尝试从旧版 ini 迁移，仍无则返回默认值。
    ///
    /// 迁移成功后**立即落盘**为 `settings.toml`，并**删除旧 ini**：否则用户日后
    /// 一旦删除 `settings.toml`，陈旧的 ini 会被静默重新导入，配置「复活」成旧值。
    ///
    /// 若落盘失败则不删 ini（保留唯一副本，避免配置丢失）。
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::file_path();
        if !path.exists() {
            return match Self::migrate_from_ini() {
                Some(migrated) => {
                    // **迁移结果同样要过校验**：旧 ini 里完全可能有 `port=0` 这类非法值，
                    // 直接采信会让「配置非法 ⇒ 明确报错」的承诺在迁移路径上落空
                    // （实测后果：服务用 --port 0 启动，界面永远连不上）。
                    migrated.validate().map_err(|e| ConfigError::Corrupted {
                        path: Self::legacy_ini_path(),
                        reason: format!("旧版 settings.ini 迁移后不合法：{e}"),
                    })?;
                    // 落盘失败不阻断启动：内存中的配置仍然可用，且不删 ini
                    if migrated.save().is_ok() {
                        let _ = std::fs::remove_file(Self::legacy_ini_path());
                    }
                    Ok(migrated)
                }
                None => Ok(Self::default()),
            };
        }
        let raw = std::fs::read_to_string(&path)?;
        let s: Self = toml::from_str(&raw).map_err(|e| ConfigError::Corrupted {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        s.validate()?;
        Ok(s)
    }

    /// 配置中记录的 schema 版本是否高于本程序认识的版本。
    ///
    /// 高于当前版本意味着「配置文件由更新版的启动器写入」——字段语义可能已变。
    /// 调用方应提示用户（而不是静默继续），因为降级使用可能误读字段。
    pub fn is_from_newer_schema(&self) -> bool {
        self.schema_version > CURRENT_SCHEMA_VERSION
    }

    /// 保存配置（原子写：先写 .tmp 再 rename，避免崩溃留下半截文件）。
    pub fn save(&self) -> Result<(), ConfigError> {
        self.validate()?;
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// 删除 `settings.toml`（幂等；文件不存在视为成功）。
    ///
    /// v5.0.0 定稿轮新增：此前这里是一个 `pub fn reset() -> Self`，它只是
    /// `Settings::default()` 的别名，**全项目零调用方**，且会让调用方误以为
    /// "重置"还做了别的收尾。真正的重置语义是「让下次启动回到默认值」，
    /// 也就是把配置文件移除——那正是本函数做的事。
    ///
    /// **同时清理旧版 `settings.ini`**：否则一份迁移失败的陈旧 ini 会在下次启动时
    /// 被重新导入 —— 用户点了「恢复默认设置」却依然起不来（无法自解释的故障）。
    pub fn reset_on_disk() -> std::io::Result<()> {
        remove_if_exists(&Self::file_path())?;
        remove_if_exists(&Self::legacy_ini_path())?;
        Ok(())
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::Invalid("port 不能为 0".into()));
        }
        Ok(())
    }

    /// 从 v4 的 settings.ini 迁移（`key=value` 行，忽略注释与节名）。
    fn migrate_from_ini() -> Option<Self> {
        Self::parse_ini(&std::fs::read_to_string(Self::legacy_ini_path()).ok()?)
    }

    /// 解析旧版 ini **文本**（未知键与非法值一律忽略）。
    ///
    /// 抽成纯函数的唯一目的：让**真正跑的迁移逻辑**可被单测覆盖。此前这里没有独立
    /// 函数，测试里把解析循环**重写了一遍** —— 于是生产代码那条路径从未被任何测试
    /// 执行过（测试通过只证明那份副本能跑）。
    fn parse_ini(raw: &str) -> Option<Self> {
        let mut s = Self::default();
        let mut saw_any = false;
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with(';')
                || line.starts_with('[')
            {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "port" => {
                    if let Ok(p) = v.parse() {
                        s.port = p;
                        saw_any = true;
                    }
                }
                "trayonclose" => {
                    s.tray_on_close =
                        matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
                    saw_any = true;
                }
                "nodepath" => {
                    s.node_path = v.to_string();
                    saw_any = true;
                }
                "workdir" => {
                    s.work_dir = v.to_string();
                    saw_any = true;
                }
                _ => {}
            }
        }
        saw_any.then_some(s)
    }

    /// 工作目录：留空时返回 None（由调用方沿用 dsh 默认）。
    pub fn effective_work_dir(&self) -> Option<&Path> {
        if self.work_dir.trim().is_empty() {
            None
        } else {
            Some(Path::new(&self.work_dir))
        }
    }
}

/// 删除文件；**不存在视为成功**（幂等），其余错误如实上抛。
///
/// 为什么不写成「let _ = remove_file(..)」：删除失败（文件被占用、权限不足）必须让
/// 调用方知道 —— 「恢复默认设置」静默失败会让用户以为已经重置，而下次启动又读到
/// 同一份坏配置。
fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("配置文件损坏：{path}（{reason}）—— 可在设置页一键重置")]
    Corrupted { path: PathBuf, reason: String },
    #[error("配置项非法：{0}")]
    Invalid(String),
    #[error("序列化失败：{0}")]
    Serialize(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert_eq!(s.port, 3080);
        assert!(s.tray_on_close);
        assert!(s.effective_work_dir().is_none());
        // 默认必须是「服务独立于启动器」——否则退出启动器会切断正在进行的会话
        assert_eq!(s.service_lifecycle, ServiceLifecycle::Independent);
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(s.stop_stale_orphan);
    }

    #[test]
    fn roundtrip_toml() {
        let s = Settings {
            port: 3099,
            tray_on_close: false,
            node_path: "C:\\node\\node.exe".into(),
            work_dir: "D:\\ws".into(),
            service_lifecycle: ServiceLifecycle::Tied,
            stop_stale_orphan: false,
            ..Default::default()
        };
        let raw = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(s, back);
        // TOML 里必须是可读的字符串形式（便于用户手改）
        assert!(raw.contains("service_lifecycle = \"tied\""), "{raw}");
    }

    #[test]
    fn lifecycle_parses_both_spellings() {
        let ind: Settings = toml::from_str("service_lifecycle = \"independent\"").unwrap();
        assert_eq!(ind.service_lifecycle, ServiceLifecycle::Independent);
        let tied: Settings = toml::from_str("service_lifecycle = \"tied\"").unwrap();
        assert_eq!(tied.service_lifecycle, ServiceLifecycle::Tied);
        // 缺失字段时必须回退到默认（老配置文件的兼容路径）
        let old: Settings = toml::from_str("port = 3080\n").unwrap();
        assert_eq!(old.service_lifecycle, ServiceLifecycle::Independent);
        assert_eq!(old.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn lifecycle_maps_to_child_lifecycle() {
        assert_eq!(
            ServiceLifecycle::Independent.to_child_lifecycle(),
            crate::process::ChildLifecycle::Independent
        );
        assert_eq!(
            ServiceLifecycle::Tied.to_child_lifecycle(),
            crate::process::ChildLifecycle::Tied
        );
    }

    #[test]
    fn newer_schema_is_flagged() {
        let mut s = Settings::default();
        assert!(!s.is_from_newer_schema());
        s.schema_version = CURRENT_SCHEMA_VERSION + 1;
        assert!(s.is_from_newer_schema());
    }

    #[test]
    fn rejects_zero_port() {
        let s = Settings {
            port: 0,
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }

    /// 旧版 ini 迁移：这里测的是**生产代码里真正跑的那份解析器**。
    ///
    /// 旧测试把解析循环又抄了一遍，于是迁移代码本身从未被执行过 ——
    /// 测试通过只证明那份副本能跑（典型的假覆盖）。
    #[test]
    fn parses_legacy_ini_shape_with_the_real_parser() {
        let raw = "# c\n[section]\nport=3099\ntrayonclose=false\nworkdir=D:\\ws\n\n";
        let s = Settings::parse_ini(raw).expect("有已知键时应返回迁移结果");
        assert_eq!(s.port, 3099);
        assert!(!s.tray_on_close);
        assert_eq!(s.work_dir, "D:\\ws");
        // 大小写 / 空白 / CRLF：真实用户手改过的 ini 长这样
        let s2 = Settings::parse_ini("  PORT = 3100  \r\n").expect("应识别大写键名");
        assert_eq!(s2.port, 3100);
        // 无已知键 / 非法值 / 空内容：绝不能产生「迁移成功」的假象
        assert!(Settings::parse_ini("mystery=1").is_none());
        assert!(Settings::parse_ini("port=not-a-number").is_none());
        assert!(
            Settings::parse_ini("port=99999").is_none(),
            "超出 u16 的端口必须被忽略"
        );
        assert!(Settings::parse_ini("").is_none());
    }

    /// 迁移结果必须与 TOML 路径走**同一套校验**。
    ///
    /// 旧实现直接从迁移函数返回、跳过校验：一份 port=0 的旧 ini 会让启动器拿 0
    /// 端口去拉起服务（界面永远连不上，且日志里没有线索）。
    #[test]
    fn migrated_settings_are_validated() {
        let bad = Settings::parse_ini("port=0").expect("port=0 本身是可解析的");
        assert!(
            bad.validate().is_err(),
            "port=0 必须判为非法（不得穿过迁移路径）"
        );
        let good = Settings::parse_ini("port=3080").expect("应能解析");
        assert!(good.validate().is_ok());
    }

    /// 删除助手必须幂等，且**不得**把「删掉了一个目录」当成成功。
    #[test]
    fn remove_if_exists_is_idempotent_and_strict() {
        let p = std::env::temp_dir().join(format!("dsh_cfg_rm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        remove_if_exists(&p).expect("删除不存在的文件必须成功（幂等）");
        std::fs::write(&p, b"x").expect("应能写入临时文件");
        remove_if_exists(&p).expect("应能删除已存在的文件");
        assert!(!p.exists());
        std::fs::create_dir_all(&p).expect("应能建临时目录");
        assert!(
            remove_if_exists(&p).is_err(),
            "删除目录必须报错，不能谎报成功"
        );
        let _ = std::fs::remove_dir_all(&p);
    }
}
