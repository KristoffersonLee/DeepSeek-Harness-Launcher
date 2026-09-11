//! dsh-uninstall —— DSHLauncher 的原生卸载器（**取代** v5.0.0 之前的 `uninstall.cmd` + `.ps1`）。
//!
//! # 为什么从脚本改为原生 exe
//!
//! 旧实现是 `uninstall.cmd` → `powershell.exe -ExecutionPolicy Bypass -File uninstall.ps1`，
//! 它在真实环境里有四个无法用"多写点防护"解决的缺口：
//!
//! 1. **执行策略**：命令行 `-ExecutionPolicy Bypass` 只覆盖本机设置，**覆盖不了组策略**
//!    （`MachinePolicy`/`UserPolicy` = AllSigned/Restricted 时脚本仍被拒），也拦不住
//!    AppLocker/WDAC 封锁脚本执行 ⇒ 加固镜像上**卸载不了**，用户被卡住并留下残骸；
//! 2. **二次依赖 PowerShell**：延迟删除目录靠再起一个隐藏 `powershell.exe`（模板里还要往
//!    `%TEMP%` 写临时脚本）⇒ 同类封锁下目录删不掉，失败还会留下孤儿脚本；
//! 3. **AV/EDR 形态**：`cmd → powershell -Bypass → Remove-Item -Recurse` 是最敏感的行为模式之一；
//! 4. **企业/MDM 惯例**：按 `UninstallString` 调用时期望 `/quiet` 之类的约定开关与稳定退出码；
//!    脚本可读性虽好，但无法做 Authenticode 签名获得同等信任。
//!
//! 原生 exe 一次解决全部四条：无脚本引擎依赖、原生 UTF-16 路径（非 ASCII/超长/含引号都安全）、
//! 自删除改为"复制自身到 `%TEMP%` 重入"（不再需要隐藏 PowerShell 子进程）、可签名、可带版本资源。
//!
//! # 安全护栏（与旧脚本逐条对齐，且有单测）
//!
//! * **安装标记 `.dsllauncher-install` 必须存在**才执行任何删除 —— 这是区分"安装副本"与
//!   "源码树/便携目录"的唯一可靠依据（源码树曾因只判断"有 DSHLauncher.exe"被误删过）；
//! * **受保护路径**（盘符根、`%WINDIR%`、`%ProgramFiles%`、用户目录等）拒绝递归删除；
//! * 目录里若存在**不属于本安装包的文件**，只删我们自己的文件、**保留目录本身**；
//! * `%APPDATA%\DSHLauncher`（用户配置）**默认保留**，只有 `--purge` 才删；
//! * `--dry-run` 只报告影响范围，**不修改任何状态**；
//! * 结束启动器时**只结束从本目录启动的实例**（按映像路径比较），绝不误杀别处的安装。
//!
//! # 用法
//!
//! ```text
//! dsh-uninstall.exe                 # 卸载（保留用户配置）
//! dsh-uninstall.exe --purge         # 连用户配置一起删
//! dsh-uninstall.exe --silent        # 只打印问题（QuietUninstallString 用它；亦接受 /quiet）
//! dsh-uninstall.exe --dry-run       # 只报告影响范围
//! dsh-uninstall.exe --clean-residue # 安装目录已被删除时，只清注册表项 + 桌面快捷方式
//! dsh-uninstall.exe --help          # 用法与退出码
//! ```
//!
//! 退出码：`0` 成功（含"本来就干净"）；`1` 至少一步失败（未通过后置条件校验）；
//! `2` 被防护规则拒绝（缺标记 / 受保护路径 / 源码树 / 残留清理准入不通过）。
//!
//! # 残留清理模式（`--clean-residue`）
//!
//! 正规卸载的安装目录来自**本程序所在目录**（与旧脚本的 `$PSScriptRoot` 同一语义）。
//! 安装目录一旦被删除，注册表卸载项与桌面快捷方式就会指向不存在的文件，而三条正规入口
//! 全部被护栏拒绝（安装目录里的卸载器随目录消失 / 源码树副本被源码树护栏拒绝 /
//! `--deferred-pass` 被"缺少安装标记"拒绝）⇒ 用户**没有任何受支持的手段**清掉那两处残留。
//!
//! 该模式只清"磁盘外"的两处残留，**不删除任何文件或目录**，准入比正规卸载更严
//! （注册表三个值互相印证 + 安装标记必须已不存在），详见 [`steps::residue_pass`]。

mod cli;
mod guards;
mod plan;
mod platform;
mod steps;

use cli::Args;
use plan::Context;

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    if args.help {
        cli::print_help();
        std::process::exit(0);
    }

    // 残留清理模式必须**在 `Context::new` 之前**分支：后者的准入校验要求安装标记存在，
    // 而本模式处理的恰恰是"标记已随安装目录消失"的情形（见文件头说明）。
    if args.clean_residue {
        std::process::exit(steps::residue_pass(&args));
    }

    // 安装目录从**本可执行文件所在目录**推导（与旧脚本的 $PSScriptRoot 同一语义）：
    // 不硬编码、不接受外部传入的任意路径（第二阶段重入除外，且那里会二次校验）。
    let install_dir = match platform::own_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("无法确定本程序所在目录：{e}");
            std::process::exit(1);
        }
    };

    let ctx = match Context::new(install_dir, &args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let code = if args.deferred_pass {
        steps::deferred_pass(&ctx)
    } else {
        steps::main_pass(&ctx)
    };
    std::process::exit(code);
}
