//! Win32 版本资源（`VS_VERSION_INFO` + 图标）的构建期注入。
//!
//! ## 为什么单独成 crate
//!
//! 本项目有**两个**需要版本资源的发布产物：`DSHLauncher.exe` 与 `dsh-uninstall.exe`。
//! 而"生成 `.rc` → 定位 `rc.exe` → 链接资源"这段逻辑相当长：
//! - `.rc` 中的相对路径按 rc.exe 的工作目录解析，因此必须在 `OUT_DIR` 生成 `.rc`
//!   并写入图标的**绝对路径**；
//! - `rc.exe` 查找要覆盖三类真实布局：`WindowsSdkDir` 环境变量、Windows Kits 注册表
//!   （`KitsRoot10`）、`%ProgramFiles(x86)%` 默认根，最后才是 `PATH`；
//! - 必须在 SDK 的多架构目录里按**宿主架构**筛选（`bin\<ver>\x64|arm64`），
//!   选错会以 "os error 216" 失败。
//!
//! 复制一份就等于制造第二处事实来源，因此这里保留**唯一实现**，由两个 `build.rs` 调用。
//! 附带收益：本文件的单测会被 `cargo test` **真正执行**（放在 `build.rs` 里的 `#[test]`
//! 永远不会被编译或运行）。
//!
//! 版本号唯一来源仍是 workspace `Cargo.toml`（调用方用 `CARGO_PKG_VERSION` 传入）。

use std::path::{Path, PathBuf};

/// 一个发布产物的资源块内容。
pub struct Spec<'a> {
    /// `CARGO_MANIFEST_DIR`（调用方 crate 的根）。
    pub manifest_dir: PathBuf,
    /// `OUT_DIR`（`.rc` / `.res` 的生成位置）。
    pub out_dir: PathBuf,
    /// 图标文件（`.ico`）的绝对路径。
    pub icon_path: PathBuf,
    /// 完整版本字符串（`5.0.0`；后缀 `-alpha.1` / `+build` 不进入数字字段）。
    pub version: &'a str,
    /// 资源块里的 `ProductName`。
    pub product_name: &'a str,
    /// `FileDescription`。
    pub file_description: &'a str,
    /// `CompanyName`。
    pub company_name: &'a str,
    /// `LegalCopyright`。
    pub copyright: &'a str,
    /// `OriginalFilename` / `InternalName`。
    pub original_filename: &'a str,
    /// `.rc` / `.res` 的文件名前缀（避免两个 crate 同名互相覆盖）。
    pub base_name: &'a str,
    /// 生成的文件名（用于错误信息）。
    pub generated_label: &'a str,
    /// 是否允许"缺资源也放行"（对应 `DSH_ALLOW_MISSING_VERSION_RESOURCE` 逃生开关）。
    pub permit_missing: bool,
}

/// 资源 ID：托盘/窗口图标可按序号加载（`Icon::from_resource(1, ..)`）。
const ICON_RESOURCE_ID: u32 = 1;

/// 执行完整的资源注入流程（由 `build.rs` 调用）。
///
/// 失败时**让构建失败**（除非显式设置逃生开关）：v5.0.0 之前的构建脚本只打印 warning，
/// 结果发布出去的 exe 既没有图标资源也没有版本信息，安装包 `DisplayIcon` 与 Explorer
/// 属性页全是空白 —— 这类静默降级必须消除。
pub fn build(spec: &Spec<'_>) {
    println!("cargo:rerun-if-changed=build.rs");
    // **必须无条件声明**：此前只在图标存在时才声明 `rerun-if-changed`，于是
    // 「图标缺失时构建过一次」⇒ cargo 不知道图标后来出现了 ⇒ build.rs 不再重跑 ⇒
    // `.rc` 里**没有 ICON 行** ⇒ 产出的 exe 静默丢掉图标资源（体积少约 21 KB）。
    println!("cargo:rerun-if-changed={}", spec.icon_path.display());

    if !spec.icon_path.is_file() {
        panic!(
            "缺少图标文件 {}。请运行 `pwsh -NoProfile -File make-icon.ps1` 重新生成\
             （该脚本是确定性的：同样的输入必然产出同样的字节）。",
            spec.icon_path.display()
        );
    }

    let Some(numeric) = numeric_version(spec.version) else {
        panic!(
            "CARGO_PKG_VERSION=\"{}\" 无法转换为 Win32 四段版本号（需要 x.y.z 形式）",
            spec.version
        );
    };

    let rc_src = build_rc(spec, &numeric);
    let rc_file = spec.out_dir.join(format!("{}.rc", spec.base_name));
    let res_file = spec.out_dir.join(format!("{}.res", spec.base_name));
    if let Err(e) = std::fs::write(&rc_file, rc_src) {
        fail_or_warn(spec.permit_missing, &format!("写入 .rc 失败：{e}"));
        return;
    }

    let Some(rc_exe) = find_rc_exe() else {
        fail_or_warn(
            spec.permit_missing,
            "未找到 rc.exe（Windows SDK）。安装 Windows SDK 或 Visual Studio 后重试；\
             仅在临时排查时可用 DSH_ALLOW_MISSING_VERSION_RESOURCE=1 跳过。",
        );
        return;
    };
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=DSH_ALLOW_MISSING_VERSION_RESOURCE");

    let output = std::process::Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .current_dir(&spec.out_dir)
        .output();

    match output {
        Ok(o) if o.status.success() && res_file.is_file() => {
            // 交给链接器：把编译好的资源对象链进 exe
            println!("cargo:rustc-link-arg={}", res_file.display());
        }
        Ok(o) => fail_or_warn(
            spec.permit_missing,
            &format!(
                "rc.exe 编译失败（{}）：stdout={} stderr={}",
                o.status,
                String::from_utf8_lossy(&o.stdout).trim(),
                String::from_utf8_lossy(&o.stderr).trim()
            ),
        ),
        Err(e) => fail_or_warn(spec.permit_missing, &format!("rc.exe 调用失败：{e}")),
    }
}

/// 需要人工确认时让构建失败；显式设置逃生开关时退化为警告。
fn fail_or_warn(permit_missing: bool, message: &str) {
    if permit_missing {
        println!("cargo:warning={message}");
    } else {
        panic!("{message}");
    }
}

/// 把 `5.0.0` / `5.0.0-alpha.1` 解析为 `(5, 0, 0, 0)`。
///
/// 预发布后缀（`-alpha.1`）与构建元数据（`+build`）不进入数字字段，
/// 但完整字符串仍会写入 `StringFileInfo`，因此不会丢信息。
pub fn numeric_version(version: &str) -> Option<(u32, u32, u32, u32)> {
    let core = version.split(['-', '+']).next().unwrap_or(version).trim();
    let mut it = core.split('.');
    let major: u32 = it.next()?.trim().parse().ok()?;
    // 缺省的次版本/修订号按 0 处理（`5` → 5.0.0.0）
    let minor: u32 = it.next().unwrap_or("0").trim().parse().ok()?;
    let patch: u32 = it.next().unwrap_or("0").trim().parse().ok()?;
    // 第四段：Rust 版本号没有第四段，固定 0（Windows 惯例）
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch, 0))
}

/// 生成 `.rc` 源文本：图标 + 版本信息。
fn build_rc(spec: &Spec<'_>, numeric: &(u32, u32, u32, u32)) -> String {
    // 注意：.rc 字符串中的反斜杠是转义字符，Windows 路径必须双写。
    let mut out = String::new();
    if spec.icon_path.is_file() {
        let icon_escaped = spec.icon_path.display().to_string().replace('\\', "\\\\");
        out.push_str(&format!(
            "{ICON_RESOURCE_ID} ICON DISCARDABLE \"{icon_escaped}\"\n"
        ));
    }

    let (a, b, c, d) = *numeric;
    let generated = spec.generated_label;
    let version = spec.version;
    let product_name = spec.product_name;
    let file_description = spec.file_description;
    let company_name = spec.company_name;
    let copyright = spec.copyright;
    let original_filename = spec.original_filename;
    out.push_str(&format!(
        r#"
1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
FILEFLAGSMASK 0x3fL
#ifdef _DEBUG
FILEFLAGS 0x1L
#else
FILEFLAGS 0x0L
#endif
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "080404b0"
        BEGIN
            VALUE "CompanyName", "{company_name}"
            VALUE "FileDescription", "{file_description} ({generated})"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "{original_filename}"
            VALUE "LegalCopyright", "{copyright}"
            VALUE "OriginalFilename", "{original_filename}"
            VALUE "ProductName", "{product_name}"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x804, 1200
    END
END
"#
    ));
    out
}

/// 查找 rc.exe（Windows SDK / MSVC 工具链）。
///
/// 按可靠性排序：环境变量 → 注册表 KitsRoot10 → 默认安装根 → PATH。
pub fn find_rc_exe() -> Option<PathBuf> {
    // 1) 环境变量优先（cargo/VS 开发者环境）
    for key in ["WindowsSdkDir", "WindowsSdkVerBinPath"] {
        if let Some(dir) = std::env::var_os(key) {
            let base = PathBuf::from(dir);
            // WindowsSdkVerBinPath 直接指向 <ver>\<arch>，其本身就是目标目录
            if let Some(found) = pick_rc_in(&base) {
                return Some(found);
            }
            if let Some(found) = newest_rc_under(&base.join("bin")) {
                return Some(found);
            }
        }
    }

    // 2) 注册表：Windows Kits 的安装根（不依赖环境变量，最稳）
    for base in kits_roots_from_registry() {
        if let Some(found) = newest_rc_under(&base.join("bin")) {
            return Some(found);
        }
    }

    // 3) 常见 SDK 安装根
    for base in [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ] {
        if let Some(found) = newest_rc_under(Path::new(base)) {
            return Some(found);
        }
    }

    // 4) PATH
    if let Ok(o) = std::process::Command::new("where.exe")
        .arg("rc.exe")
        .output()
    {
        if o.status.success() {
            if let Some(line) = String::from_utf8_lossy(&o.stdout).lines().next() {
                let p = PathBuf::from(line.trim());
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    None
}

/// 从注册表读取 Windows Kits 根目录（`KitsRoot10` / `KitsRoot`）。
fn kits_roots_from_registry() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    // 优先用 PowerShell 读注册表：无需新增依赖，且 Windows 自带
    let script = r#"
$keys = @(
  'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots'
)
foreach ($k in $keys) {
  if (Test-Path $k) {
    $p = Get-ItemProperty -Path $k -ErrorAction SilentlyContinue
    foreach ($n in 'KitsRoot10','KitsRoot') {
      if ($p.$n) { Write-Output $p.$n }
    }
  }
}
"#;
    if let Ok(o) = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
    {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let line = line.trim();
                if !line.is_empty() {
                    roots.push(PathBuf::from(line));
                }
            }
        }
    }
    roots
}

/// 宿主进程的架构名（用于在 SDK 的多架构目录里选对 rc.exe）。
///
/// **必须按架构筛选**：SDK 的 `bin\<ver>\` 下同时存在 `x64` / `x86` / `arm64`
/// 三份 rc.exe，选错架构会直接以 “os error 216：该版本的 %1 与你运行的 Windows
/// 版本不兼容”失败（在 x64 宿主机上如果先枚举到 `arm64` 就会踩到）。
pub const fn host_arch() -> &'static str {
    // Rust 只允许为 x86_64 / aarch64 目标构建本项目
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// 某个目录本身就是 `<arch>`（含 rc.exe）时直接返回。
fn pick_rc_in(dir: &Path) -> Option<PathBuf> {
    // 优先精确匹配宿主架构目录；也接受调用方直接给出含 rc.exe 的目录
    let preferred = dir.join(host_arch()).join("rc.exe");
    if preferred.is_file() {
        return Some(preferred);
    }
    let cand = dir.join("rc.exe");
    cand.is_file().then_some(cand)
}

/// 在 `<base>/<version>/<arch>/rc.exe` 布局下挑选版本号最高的 rc.exe。
///
/// 版本目录名必须是纯数字点分形式（`10.0.26100.0`）；`x64`/`arm64`/`chpe`
/// 等架构目录会被 [`parse_version`] 拒绝，因此不会误选为“版本”。
/// 架构目录则按 [`host_arch`] 筛选，避免选到不可执行的 rc.exe。
fn newest_rc_under(base: &Path) -> Option<PathBuf> {
    let arch = host_arch();
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(base).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(ver) = parse_version(&name) else {
            continue;
        };
        let cand = entry.path().join(arch).join("rc.exe");
        if !cand.is_file() {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| ver > *b) {
            best = Some((ver.clone(), cand));
        }
    }
    best.map(|(_, p)| p)
}

/// 解析 `10.0.26100.0` 形式的版本号；非纯数字目录（`x64`）返回 None。
pub fn parse_version(s: &str) -> Option<Vec<u32>> {
    if s.is_empty() {
        return None;
    }
    let parts: Vec<u32> = s
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

/// 资源块里的通用元数据（两个产物共用，避免各写一份）。
pub mod meta {
    /// 产品名（安装包 `AppName`、`ProductName` 必须与之一致）。
    pub const PRODUCT_NAME: &str = "DeepSeek Harness Launcher";
    /// 公司名。
    pub const COMPANY_NAME: &str = "KristoffersonLee";
    /// 版权。
    pub const COPYRIGHT: &str = "Copyright (C) 2026 KristoffersonLee";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_version() {
        assert_eq!(numeric_version("5.0.0"), Some((5, 0, 0, 0)));
        assert_eq!(numeric_version("5.0.0-alpha.1"), Some((5, 0, 0, 0)));
        assert_eq!(numeric_version("5.0.0+build.7"), Some((5, 0, 0, 0)));
        assert_eq!(numeric_version("5"), Some((5, 0, 0, 0)));
        assert_eq!(numeric_version("5.1"), Some((5, 1, 0, 0)));
        assert_eq!(numeric_version("bogus"), None);
        assert_eq!(numeric_version("5.0.0.1"), None);
    }

    #[test]
    fn sdk_dir_names_are_not_versions() {
        assert_eq!(parse_version("10.0.26100.0"), Some(vec![10, 0, 26100, 0]));
        assert_eq!(parse_version("x64"), None);
        assert_eq!(parse_version("arm64"), None);
        assert_eq!(parse_version("chpe"), None);
        assert_eq!(parse_version("WppConfig"), None);
    }

    #[test]
    fn host_arch_is_a_known_sdk_dir() {
        // 必须是 SDK 实际使用的目录名，否则 rc.exe 永远找不到
        assert!(matches!(host_arch(), "x64" | "arm64"));
    }

    fn spec<'a>(icon: &'a Path, label: &'a str) -> Spec<'a> {
        Spec {
            manifest_dir: PathBuf::from(r"D:\repo"),
            out_dir: PathBuf::from(r"D:\repo\target\out"),
            icon_path: icon.to_path_buf(),
            version: "5.0.0",
            product_name: meta::PRODUCT_NAME,
            file_description: "DeepSeek Harness Launcher",
            company_name: meta::COMPANY_NAME,
            copyright: meta::COPYRIGHT,
            original_filename: "DSHLauncher.exe",
            base_name: "dsh_app",
            generated_label: label,
            permit_missing: false,
        }
    }

    /// 造一个**真实存在**的临时 `.ico`：ICON 行只在图标存在时才写入，
    /// 用不存在的路径去断言 ICON 行等于在断言一个永远不会发生的事。
    ///
    /// 说明：这条断言此前写在 `dsh-app/build.rs` 里，而 **build.rs 的 `#[test]` 永远不会被
    /// `cargo test` 编译或执行** —— 于是它一直"假绿"。把逻辑搬进本 crate（普通库）之后
    /// 它立刻跑了起来并失败，暴露出原来断言的是"不存在的图标也会写 ICON 行"。
    fn temp_icon(tag: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("dsh-buildinfo-{tag}-{}.ico", std::process::id()));
        std::fs::write(&p, b"\x00\x00\x01\x00").expect("应能写临时图标");
        p
    }

    #[test]
    fn rc_contains_version_icon_and_label() {
        let icon = temp_icon("launcher");
        let rc = build_rc(&spec(&icon, "launcher"), &(5, 0, 0, 0));
        assert!(rc.contains("FILEVERSION 5,0,0,0"));
        assert!(rc.contains("PRODUCTVERSION 5,0,0,0"));
        assert!(rc.contains(r#"VALUE "ProductVersion", "5.0.0""#));
        assert!(rc.contains(r#"VALUE "OriginalFilename", "DSHLauncher.exe""#));
        assert!(rc.contains(r#"VALUE "CompanyName", "KristoffersonLee""#));
        // 生成标记用于区分同一版本下的不同产物（启动器 / 卸载器）
        assert!(rc.contains("(launcher)"), "{rc}");
        // ICON 行必须写入，且路径中的反斜杠必须双写（.rc 里反斜杠是转义字符）
        let escaped = icon.display().to_string().replace('\\', "\\\\");
        assert!(
            rc.contains(&format!("1 ICON DISCARDABLE \"{escaped}\"")),
            "缺少 ICON 行：{rc}"
        );
        assert!(rc.contains("BLOCK \"080404b0\""));
        let _ = std::fs::remove_file(&icon);
    }

    #[test]
    fn missing_icon_omits_the_icon_line_but_keeps_version_info() {
        // 图标缺失时 build() 会先 panic（图标是必需资产），但 build_rc 本身不应写出无效路径
        let rc = build_rc(
            &spec(Path::new(r"D:\nope\missing.ico"), "launcher"),
            &(5, 0, 0, 0),
        );
        assert!(!rc.contains("ICON DISCARDABLE"), "{rc}");
        assert!(rc.contains("FILEVERSION 5,0,0,0"));
    }

    #[test]
    fn uninstaller_resource_uses_its_own_filename_and_label() {
        let icon = temp_icon("uninstaller");
        let mut s = spec(&icon, "uninstaller");
        s.original_filename = "dsh-uninstall.exe";
        s.base_name = "dsh_uninstall";
        let rc = build_rc(&s, &(5, 0, 0, 0));
        assert!(rc.contains(r#"VALUE "OriginalFilename", "dsh-uninstall.exe""#));
        assert!(rc.contains("(uninstaller)"), "{rc}");
        let _ = std::fs::remove_file(&icon);
    }
}
