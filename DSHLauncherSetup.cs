// DSHLauncherSetup.cs — DeepSeek Harness Launcher 一键安装包
// 功能: 环境检测(Node.js/dsh) → 缺失一键部署(winget→MSI→npm) → 快速安装
//       → 完成页(启动应用/创建桌面图标/新手指引)
// 命令行: --silent-install [目录]  静默安装(写 setup.log, 退出码 0=成功 1=失败 2=参数错误)
//         --detect-only            只检测环境(写 setup.detect.log)
// 构建: build-setup.ps1（内嵌单文件 DSHLauncher.exe、app.ico、
//       README.md 与维护手册 MAINTENANCE.zh.md / MAINTENANCE.en.md，单文件分发）
//       注：v5 启动器为 Rust 单文件，WebView2 loader 静态链接，无需内嵌旁挂 DLL。
// 作者: KristoffersonLee
// 兼容 C# 5（系统自带 csc v4.0.30319；注意 StandardOutputEncoding 需 .NET 4.5+ 运行时，
//        Win10/11 自带 4.8，兼容 4.0 目标即可）。

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Net;
using System.Reflection;
using System.Security.Principal;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Forms;
using Microsoft.Win32;

namespace DSHSetup
{
    internal static class Program
    {
        // ---- 产品元数据：必须与 Rust 启动器（crates/dsh-app/build.rs）以及
        //      注册表 Uninstall 项完全一致。启动器侧由 Cargo.toml 的
        //      workspace version 经 CARGO_PKG_VERSION 注入 exe 的版本资源。
        public const string AppName = "DeepSeek Harness Launcher";
        /// <summary>
        /// 版本号**唯一来源**：本程序集的 AssemblyFileVersion，由
        /// tools/gen-setup-version.ps1 从 Cargo.toml 的 [workspace.package] version
        /// 生成（build-setup.ps1 会把它编进来）。
        ///
        /// 这里用反射读取，而不是第二个手写常量：v5.0.0 审计前此处写死 "5.0.0"，
        /// 与 Cargo.toml 构成**两处手写版本**，一旦漏改就会出现
        /// 「exe 是 5.0.0、注册表 DisplayVersion 却是旧版」的漂移。
        /// 反射读取后版本只需改 Cargo.toml 一处；tools/verify-version.ps1 仍会校验链路。
        /// </summary>
        public static readonly string AppVersion = ResolveAppVersion();

        /// <summary>兜底版本号：仅在 AssemblyFileVersion 缺失时使用（正常构建不会走到）。</summary>
        private const string AppVersionFallback = "0.0.0";

        private static string ResolveAppVersion()
        {
            try
            {
                Assembly asm = Assembly.GetExecutingAssembly();
                object[] attrs = asm.GetCustomAttributes(typeof(AssemblyFileVersionAttribute), false);
                if (attrs != null && attrs.Length > 0)
                {
                    AssemblyFileVersionAttribute a = (AssemblyFileVersionAttribute)attrs[0];
                    if (a != null && a.Version != null && a.Version.Trim().Length > 0
                        && a.Version.Trim() != "0.0.0.0")
                    {
                        return a.Version.Trim();
                    }
                }
                Version v = asm.GetName().Version;
                if (v != null && v.ToString() != "0.0.0.0") return v.ToString();
            }
            catch { }
            return AppVersionFallback;
        }

        public const string InstallSubDir = "DSHLauncher";
        public const string ShortcutName = "DeepSeek Harness Launcher";
        /// 发布者，与 exe 版本资源 CompanyName 一致。
        public const string Publisher = "KristoffersonLee";
        /// 卸载注册表项（HKCU）。
        public const string UninstallKey =
            @"Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher";
        /// 启动器进程名，用于升级前结束旧版 / 卸载前结束本目录实例。
        public const string LauncherExe = "DSHLauncher.exe";

        [STAThread]
        private static int Main(string[] args)
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            if (args.Length > 0 && args[0] == "--silent-install")
            {
                string dir = (args.Length > 1 && args[1].Trim().Length > 0)
                    ? args[1].Trim() : DefaultInstallDir();
                // P0-1：CLI 与 GUI 共用同一份安装目录校验。旧实现只拒绝「盘符根目录」，
                // 于是 --silent-install "%LOCALAPPDATA%" / "%APPDATA%" / "C:\Users" /
                // "%WINDIR%\System32" 全部放行——而生成的卸载器会**递归删除**该目录。
                string full, reason;
                if (!InstallDirGuard.TryResolve(dir, out full, out reason))
                {
                    Console.Error.WriteLine("无效的安装目录：" + reason);
                    return 2;
                }
                return SilentInstall.Run(full);
            }
            if (args.Length > 0 && args[0] == "--detect-only")
            {
                return SilentInstall.DetectOnly();
            }
            Application.Run(new SetupForm());
            return 0;
        }

        public static string DefaultInstallDir()
        {
            string local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            return Path.Combine(local, "Programs", InstallSubDir);
        }
    }

    // ---------------------------------------------------------------------
    // 安装目录守卫（P0-1）：CLI 与 GUI 共用同一份校验
    //
    // 为什么必须有它：生成的卸载器会 `Remove-Item -Recurse -Force` 安装目录，而安装
    // 目录来自用户输入。旧实现里 GUI 完全不校验（直接 txtInstallDir.Text），CLI 只
    // 拒绝「盘符根目录」，于是 `--silent-install "%LOCALAPPDATA%"`（以及 %APPDATA%、
    // C:\Users、%WINDIR%\System32、%TEMP%）都会被接受，卸载时被整棵删除。
    // 注意 [IO.Path]::GetPathRoot('C:\Users\x\AppData\Local') 只返回 'C:\'，
    // 所以「等于根目录」这一条根本保护不了这些路径——必须做「同路径/祖先/后代」判定。
    // ---------------------------------------------------------------------
    internal static class InstallDirGuard
    {
        /// <summary>
        /// 安装标记文件：生成到安装目录里。生成的卸载器**只有看到它**才会递归删除
        /// 安装目录（P0-1 / P2-7）；标记缺失时只删可单独识别的文件。
        /// </summary>
        public const string MarkerName = ".dsllauncher-install";

        /// <summary>把用户输入解析成规范化的绝对路径并做安全检查。</summary>
        /// <param name="input">用户输入（GUI 文本框 / CLI 参数）。</param>
        /// <param name="full">成功时为规范化后的绝对路径（已折叠 ..）。</param>
        /// <param name="reason">失败时为中文拒绝理由。</param>
        public static bool TryResolve(string input, out string full, out string reason)
        {
            full = "";
            reason = "";
            if (input == null || input.Trim().Length == 0)
            {
                reason = "安装目录不能为空。";
                return false;
            }
            string raw = input.Trim().Trim('"');
            // 0) 裸盘符 / 盘符根：必须在 GetFullPath **之前**判断，因为
            //    [IO.Path]::GetFullPath('D:') 解析的是「D: 盘上的当前目录」
            //    （例如 D:\DSHLauncher）——只看展开后的结果会让裸 'D:' 蒙混过关。
            //    TrimEnd('\\', ':') 把 'D:' / 'D:\' / 'D' 都收敛成长度 1 的 'D'。
            if (raw.TrimEnd('\\', ':').Length <= 1)
            {
                reason = "拒绝安装到盘符根目录（卸载时会递归删除整卷）：" + raw;
                return false;
            }
            string resolved;
            try
            {
                resolved = Path.GetFullPath(raw);
            }
            catch (Exception ex)
            {
                reason = "无法解析为有效路径（" + ex.Message + "）：" + input;
                return false;
            }
            resolved = Normalize(resolved);
            if (resolved.Length == 0)
            {
                reason = "无法解析为有效路径：" + input;
                return false;
            }

            // 1) 卷根（C:\、D:\、\\server\share）——递归删除等于清空整卷
            string root = Path.GetPathRoot(resolved);
            if (root != null && root.Length > 0 && Same(resolved, Normalize(root)))
            {
                reason = "拒绝安装到卷根目录（卸载时会递归删除整卷）：" + resolved;
                return false;
            }

            // 2) 受保护目录：%LOCALAPPDATA%\Programs 是文档约定的默认安装位置，
            //    只对它开一个**严格子目录**的例外（默认目录 %LOCALAPPDATA%\Programs\
            //    DSHLauncher 必须继续可用）。
            string allowed = "";
            string localAppData = SafeFolder(Environment.SpecialFolder.LocalApplicationData);
            if (localAppData.Length > 0)
            {
                try { allowed = Normalize(Path.Combine(localAppData, "Programs")); }
                catch { allowed = ""; }
            }
            bool insideAllowed = allowed.Length > 0 && IsUnder(resolved, allowed);

            foreach (string guard in ProtectedDirs())
            {
                if (Same(resolved, guard))
                {
                    reason = "拒绝安装到系统/用户保护目录：" + resolved + "（= " + guard + "）";
                    return false;
                }
                if (IsUnder(resolved, guard))
                {
                    if (insideAllowed) continue; // 文档约定的例外：%LOCALAPPDATA%\Programs\...
                    reason = "拒绝安装到系统/用户保护目录之内：" + resolved + "（位于 " + guard + " 内）";
                    return false;
                }
                if (IsUnder(guard, resolved))
                {
                    // 安装目录是保护目录的上级：递归删除会连带删掉保护目录（如 C:\Users）
                    reason = "拒绝安装到系统/用户保护目录的上级：" + resolved + "（包含 " + guard + "）";
                    return false;
                }
            }
            full = resolved;
            return true;
        }

        /// <summary>规范形式：绝对路径，去掉末尾分隔符（卷根保持原样）。</summary>
        public static string Normalize(string path)
        {
            return NormalizeCore(path);
        }

        /// <summary>
        /// 规范化的唯一实现。**绝不**调用 Same()：Normalize↔Same 互相调用会无限递归
        /// （实测直接 StackOverflow 崩溃进程），因此比较逻辑必须建立在这个不可重入的
        /// 内核函数之上。
        /// </summary>
        private static string NormalizeCore(string path)
        {
            string p = path;
            try { p = Path.GetFullPath(path); }
            catch { }
            if (p == null) return "";
            p = p.Trim();
            if (p.Length == 0) return "";
            string root = null;
            try { root = Path.GetPathRoot(p); }
            catch { }
            if (root != null && root.Length > 0
                && string.Equals(p, root, StringComparison.OrdinalIgnoreCase)) return p;
            return p.TrimEnd('\\', '/');
        }

        /// <summary>大小写无关的全路径相等（Windows 语义）。</summary>
        public static bool Same(string a, string b)
        {
            if (a == null || b == null) return false;
            return string.Equals(NormalizeCore(a), NormalizeCore(b), StringComparison.OrdinalIgnoreCase);
        }

        /// <summary>
        /// child 是否**严格位于** parent 之内。先规范化再比较，并且必须在分隔符边界上
        /// 比较——只做 StartsWith 会让 `C:\WindowsX` 命中 `C:\Windows`。
        /// </summary>
        public static bool IsUnder(string child, string parent)
        {
            string c = Normalize(child);
            string p = Normalize(parent);
            if (c.Length == 0 || p.Length == 0) return false;
            if (Same(c, p)) return false;
            if (c.Length <= p.Length) return false;
            if (!c.StartsWith(p, StringComparison.OrdinalIgnoreCase)) return false;
            if (p.EndsWith("\\") || p.EndsWith("/")) return true; // 卷根 / UNC 根自身以分隔符结尾
            char sep = c[p.Length];
            return sep == '\\' || sep == '/';
        }

        /// <summary>
        /// 受保护目录集合：等于它们、位于它们之内、或是它们的上级，都会被拒绝。
        /// 环境变量 + SpecialFolder 双取，避免 32 位进程 / 特殊配置下取到空值。
        /// </summary>
        private static List<string> ProtectedDirs()
        {
            List<string> list = new List<string>();
            Add(list, EnvVar("WINDIR"));
            Add(list, EnvVar("SystemRoot"));
            Add(list, EnvVar("ProgramFiles"));
            Add(list, EnvVar("ProgramFiles(x86)"));
            Add(list, EnvVar("ProgramData"));
            Add(list, EnvVar("USERPROFILE"));
            Add(list, EnvVar("APPDATA"));
            Add(list, EnvVar("LOCALAPPDATA"));
            Add(list, EnvVar("TEMP"));
            Add(list, EnvVar("TMP"));
            Add(list, SafeFolder(Environment.SpecialFolder.Windows));
            Add(list, SafeFolder(Environment.SpecialFolder.ProgramFiles));
            Add(list, SafeFolder(Environment.SpecialFolder.ProgramFilesX86));
            Add(list, SafeFolder(Environment.SpecialFolder.CommonApplicationData));
            Add(list, SafeFolder(Environment.SpecialFolder.UserProfile));
            Add(list, SafeFolder(Environment.SpecialFolder.ApplicationData));
            Add(list, SafeFolder(Environment.SpecialFolder.LocalApplicationData));
            return list;
        }

        private static void Add(List<string> list, string raw)
        {
            if (raw == null) return;
            string p = Normalize(raw.Trim().Trim('"'));
            if (p.Length == 0) return;
            foreach (string e in list) { if (Same(e, p)) return; }
            list.Add(p);
        }

        private static string EnvVar(string name)
        {
            try { return Environment.GetEnvironmentVariable(name); }
            catch { return null; }
        }

        private static string SafeFolder(Environment.SpecialFolder f)
        {
            try { return Environment.GetFolderPath(f); }
            catch { return null; }
        }
    }

    // ---------------------------------------------------------------------
    // 环境检测与一键部署
    // ---------------------------------------------------------------------
    internal static class Env
    {
        // 部署类子进程最长等待时间（winget / MSI / npm / WebView2 引导程序）
        public const int DeployTimeoutMs = 600000;

        public static string NodePath = "";
        public static string BinJs = "";
        public static string NpmPath = "";
        public static bool WebView2Ok = false; // WebView2 运行时（内嵌界面依赖）

        public static bool NodeOk { get { return NodePath.Length > 0; } }
        public static bool DshOk { get { return BinJs.Length > 0; } }

        // -----------------------------------------------------------------
        // TLS / 权限（P2-11）
        // -----------------------------------------------------------------

        /// <summary>
        /// 必须在**第一次 HTTP 请求之前**调用。.NET Framework 4.x 默认仍是
        /// Ssl3|Tls（TLS 1.0），对 Microsoft 的 go.microsoft.com / download 端点
        /// 已经协商失败——WebView2 引导程序下载就是在这里挂掉的。显式抬到 TLS 1.2。
        /// 用数字 3072（= SecurityProtocolType.Tls12）而非枚举名，是为了兼容
        /// 目标框架里没有该枚举成员的老编译环境。
        /// </summary>
        public static void EnableModernTls()
        {
            try
            {
                ServicePointManager.SecurityProtocol = (SecurityProtocolType)3072;
            }
            catch { } // 平台不支持时保持默认：下载失败会走原有的重试 + 报错路径
        }

        /// <summary>当前进程是否已经提权（管理员）。失败时保守返回 false。</summary>
        public static bool IsElevated()
        {
            try
            {
                WindowsIdentity id = WindowsIdentity.GetCurrent();
                if (id == null) return false;
                return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator);
            }
            catch { return false; }
        }

        /// <summary>
        /// 部署类步骤（winget / MSI / WebView2 引导程序）需要管理员权限，而本安装包
        /// **全程 UseShellExecute = false**，根本不会弹 UAC——旧日志写“可能弹出 UAC
        /// 授权，请允许”是假的，用户会一直等到超时。这里改成如实说明，并顺带报告
        /// 当前进程是否已提权（只报告，不提权：不引入 runas/UAC 自提升）。
        /// </summary>
        public static string ElevationNote()
        {
            return IsElevated()
                ? "当前进程已提权（管理员），部署步骤可以直接写系统位置。"
                : "当前进程未提权：本安装包不会弹 UAC（所有子进程都以当前权限运行），"
                  + "因此 Node.js 的机器级安装与 WebView2 运行时安装可能因权限不足而失败；"
                  + "失败时请右键安装包选择“以管理员身份运行”后重试。";
        }

        public static void Detect()
        {
            EnableModernTls();
            NodePath = ""; BinJs = ""; NpmPath = "";
            WebView2Ok = CheckWebView2();
            // 合并机器级 PATH：winget/MSI 安装 Node 写的是机器 PATH，当前进程环境不会自动刷新，
            // 不合并会导致“部署成功但检测失败”
            MergeMachinePath();

            // 1) node.exe
            foreach (string dir in SplitPath())
            {
                string cand = Path.Combine(dir, "node.exe");
                if (File.Exists(cand)) { NodePath = cand; break; }
            }
            if (NodePath.Length == 0)
            {
                string la = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
                string p = Path.Combine(la, "nodejs");
                if (Directory.Exists(p))
                {
                    // 多版本共存时选版本号最大者（NTFS 目录枚举顺序不保证版本序）；
                    // 目录名形如 node-v22.14.0 或 node-v22.14.0-win-x64，取版本前缀比较
                    Version best = null;
                    string bestDir = null;
                    foreach (string d in Directory.GetDirectories(p, "node-v*"))
                    {
                        Match vm = Regex.Match(Path.GetFileName(d), @"^node-v(\d+)\.(\d+)\.(\d+)");
                        if (!vm.Success) continue;
                        Version v = new Version(
                            int.Parse(vm.Groups[1].Value),
                            int.Parse(vm.Groups[2].Value),
                            int.Parse(vm.Groups[3].Value));
                        if (best == null || v > best) { best = v; bestDir = d; }
                    }
                    if (bestDir != null)
                    {
                        string cand = Path.Combine(bestDir, "node.exe");
                        if (File.Exists(cand)) NodePath = cand;
                    }
                    if (NodePath.Length == 0)
                    {
                        string cand = Path.Combine(p, "node.exe");
                        if (File.Exists(cand)) NodePath = cand;
                    }
                }
            }
            if (NodePath.Length == 0)
            {
                string pf = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
                string cand = Path.Combine(pf, "nodejs", "node.exe");
                if (File.Exists(cand)) NodePath = cand;
            }

            // 2) npm.cmd
            if (NodePath.Length > 0)
            {
                string cand = Path.Combine(Path.GetDirectoryName(NodePath), "npm.cmd");
                if (File.Exists(cand)) NpmPath = cand;
            }
            if (NpmPath.Length == 0)
            {
                foreach (string dir in SplitPath())
                {
                    string cand = Path.Combine(dir, "npm.cmd");
                    if (File.Exists(cand)) { NpmPath = cand; break; }
                }
            }

            // 3) dsh bin.js
            if (NodePath.Length > 0)
            {
                string cand = Path.Combine(Path.GetDirectoryName(NodePath),
                    "node_modules", "@deepseek-ai", "dsh", "lib", "bin.js");
                if (File.Exists(cand)) BinJs = cand;
            }
            if (BinJs.Length == 0)
            {
                string cand = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "npm",
                    "node_modules", "@deepseek-ai", "dsh", "lib", "bin.js");
                if (File.Exists(cand)) BinJs = cand;
            }
        }

        // 合并机器级 PATH（HKLM\...\Session Manager\Environment）到当前进程环境
        private static void MergeMachinePath()
        {
            try
            {
                object mp = Registry.GetValue(
                    @"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment", "Path", null);
                if (mp == null) return;
                string machine = mp.ToString();
                if (machine.Length == 0) return;
                string cur = Environment.GetEnvironmentVariable("PATH") ?? "";
                if (cur.IndexOf(machine, StringComparison.OrdinalIgnoreCase) < 0)
                {
                    Environment.SetEnvironmentVariable("PATH", cur + ";" + machine);
                }
            }
            catch { }
        }

        // WebView2 运行时检测：注册表版本号 + 磁盘 msedgewebview2.exe 双保险
        private static bool CheckWebView2()
        {
            try
            {
                object v = Registry.GetValue(
                    @"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
                    "pv", null);
                if (v != null && v.ToString().Length > 0) return true;
                // 优先检查 ProgramFilesX86（64 位系统标准位置），回退 ProgramFiles（32 位系统 / 特殊安装）
                string[] dirs = new string[]
                {
                    Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
                        "Microsoft", "EdgeWebView", "Application"),
                    Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                        "Microsoft", "EdgeWebView", "Application")
                };
                foreach (string dir in dirs)
                {
                    if (!Directory.Exists(dir)) continue;
                    // msedgewebview2.exe 位于各版本子目录根：只扫一层，
                    // 避免 AllDirectories 递归遍历整个运行时目录（数十万文件）拖慢检测
                    string exe = Path.Combine(dir, "msedgewebview2.exe");
                    bool found = File.Exists(exe);
                    if (!found)
                    {
                        foreach (string sub in Directory.GetDirectories(dir))
                        {
                            if (File.Exists(Path.Combine(sub, "msedgewebview2.exe"))) { found = true; break; }
                        }
                    }
                    if (found) return true;
                }
            }
            catch { }
            return false;
        }

        private static IEnumerable<string> SplitPath()
        {
            List<string> list = new List<string>();
            foreach (string dir in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(';'))
            {
                string d = dir.Trim().Trim('"');
                if (d.Length > 0) list.Add(d);
            }
            return list;
        }

        // 一键部署 Node.js：优先 winget，失败则下载官方 MSI 静默安装
        public static bool DeployNode(Action<string> log)
        {
            log("开始部署 Node.js…");
            string winget = FindOnPath("winget.exe");
            if (winget.Length > 0)
            {
                log("使用 winget 安装 Node.js LTS…" + ElevationNote());
                int code = RunProcess(winget,
                    "install --id OpenJS.NodeJS.LTS --exact --silent --accept-package-agreements --accept-source-agreements",
                    "", log, DeployTimeoutMs);
                if (code == 0)
                {
                    Detect();
                    if (NodeOk) { log("Node.js 安装成功 ✓"); return true; }
                }
                log("winget 未成功（退出码 " + code + "），尝试直接下载安装包…");
            }
            else
            {
                log("未找到 winget，尝试直接下载安装包…");
            }
            try
            {
                string msiUrl = GetNodeMsiUrl();
                if (msiUrl.Length == 0) { log("无法确定 Node.js 下载地址。"); return false; }
                // 随机临时文件名，避免并发/残留互相覆盖
                string msi = Path.Combine(Path.GetTempPath(),
                    "node-lts-setup-" + Process.GetCurrentProcess().Id + "-" + Guid.NewGuid().ToString("N").Substring(0, 6) + ".msi");
                log("正在下载 " + msiUrl + " …");
                if (!DownloadFileWithRetry(msiUrl, msi, log, MinNodeMsiBytes))
                {
                    log("Node.js 下载失败，请检查网络后重试。");
                    try { File.Delete(msi); } catch { }
                    return false;
                }
                log("正在静默安装（可能需要几分钟）…");
                int code = RunProcess("msiexec.exe", "/i \"" + msi + "\" /qn /norestart", "", log, DeployTimeoutMs);
                try { File.Delete(msi); } catch { }
                if (code == 0)
                {
                    Detect();
                    if (NodeOk) { log("Node.js 安装成功 ✓"); return true; }
                }
                log("MSI 安装未成功（退出码 " + code + "，可能需要管理员权限）。");
            }
            catch (Exception ex)
            {
                log("下载/安装失败：" + ex.Message);
            }
            return false;
        }

        // 当前 CPU 架构对应的 Node.js MSI 后缀（x64 / arm64）
        private static string NodeMsiArch()
        {
            // Environment.Is64BitOperatingSystem 反映 OS 位数（.NET 4.0+ 可用），
            // 避免 PROCESSOR_ARCHITECTURE 返回当前进程位数（32 位进程在 64 位系统上返回 x86）
            if (!Environment.Is64BitOperatingSystem) return "x86";
            string arch = Environment.GetEnvironmentVariable("PROCESSOR_ARCHITECTURE");
            if (arch != null && arch.ToLowerInvariant() == "arm64") return "arm64";
            return "x64";
        }

        // 动态解析最新 LTS 版本的 MSI 下载地址；失败则回退 v24 / v22 目录列表
        private static string GetNodeMsiUrl()
        {
            string arch = NodeMsiArch();
            try
            {
                string json = DownloadStringWithRetry("https://nodejs.org/dist/index.json");
                if (json != null)
                {
                    // index.json 按版本倒序，第一个带 lts 代号（字符串）的即最新 LTS
                    Match m = Regex.Match(json,
                        "\"version\":\"v(\\d+\\.\\d+\\.\\d+)\"[^}]*?\"lts\":\"([^\"]*)\"");
                    if (m.Success)
                    {
                        string ver = m.Groups[1].Value;
                        return "https://nodejs.org/dist/v" + ver + "/node-v" + ver + "-" + arch + ".msi";
                    }
                }
            }
            catch { }
            // 目录列表回退：nginx 目录按字典序排列，须收集全部匹配按版本号取最大，
            // 否则会选到通道内最旧的补丁版
            string[] dists = new string[] { "latest-v24.x", "latest-v22.x" };
            foreach (string dist in dists)
            {
                try
                {
                    string html = DownloadStringWithRetry("https://nodejs.org/dist/" + dist + "/");
                    if (html == null) continue;
                    Version best = null;
                    string bestName = null;
                    foreach (Match mm in Regex.Matches(html,
                        "node-v(\\d+)\\.(\\d+)\\.(\\d+)-" + arch + "\\.msi"))
                    {
                        Version v = new Version(
                            int.Parse(mm.Groups[1].Value),
                            int.Parse(mm.Groups[2].Value),
                            int.Parse(mm.Groups[3].Value));
                        if (best == null || v > best) { best = v; bestName = mm.Value; }
                    }
                    if (bestName != null)
                    {
                        return "https://nodejs.org/dist/" + dist + "/" + bestName;
                    }
                }
                catch { }
            }
            return "";
        }

        // 下载辅助：显式超时 + 最多 3 次退避重试 + 最小体积校验（弱网容错）。
        // 用 HttpWebRequest（.NET 4.0 即支持 Timeout），避免 WebClient.Timeout 的 4.5+ 依赖
        private const int DownloadTimeoutMs = 300000;
        private const long MinNodeMsiBytes = 5 * 1024 * 1024; // Node MSI 约 30MB+，5MB 为兜底下限

        private static string DownloadStringWithRetry(string url)
        {
            EnableModernTls(); // 第一次 HTTP 请求前抬到 TLS 1.2（P2-11）
            for (int attempt = 1; attempt <= 3; attempt++)
            {
                try
                {
                    HttpWebRequest req = (HttpWebRequest)WebRequest.Create(url);
                    req.UserAgent = "DSHLauncherSetup";
                    req.Timeout = DownloadTimeoutMs;
                    req.ReadWriteTimeout = DownloadTimeoutMs;
                    using (HttpWebResponse resp = (HttpWebResponse)req.GetResponse())
                    using (StreamReader sr = new StreamReader(resp.GetResponseStream(), Encoding.UTF8))
                    {
                        return sr.ReadToEnd();
                    }
                }
                catch { if (attempt == 3) throw; }
            }
            return null;
        }

        private static bool DownloadFileWithRetry(string url, string dest, Action<string> log, long minBytes)
        {
            EnableModernTls(); // 第一次 HTTP 请求前抬到 TLS 1.2（P2-11）
            for (int attempt = 1; attempt <= 3; attempt++)
            {
                if (attempt > 1) Thread.Sleep(1000 * (attempt - 1)); // 退避：1s / 2s，弱网下连发无意义
                try
                {
                    if (log != null && attempt > 1) log("下载失败，正在重试（第 " + attempt + " 次）…");
                    HttpWebRequest req = (HttpWebRequest)WebRequest.Create(url);
                    req.UserAgent = "DSHLauncherSetup";
                    req.Timeout = DownloadTimeoutMs;
                    req.ReadWriteTimeout = DownloadTimeoutMs;
                    using (HttpWebResponse resp = (HttpWebResponse)req.GetResponse())
                    using (Stream src = resp.GetResponseStream())
                    using (FileStream fs = File.Create(dest))
                    {
                        byte[] buf = new byte[65536];
                        int n;
                        while ((n = src.Read(buf, 0, buf.Length)) > 0) fs.Write(buf, 0, n);
                    }
                    FileInfo fi = new FileInfo(dest);
                    if (fi.Length >= minBytes) return true;
                    if (log != null) log("下载文件体积异常（" + fi.Length + " 字节），重新下载…");
                }
                catch (Exception ex)
                {
                    if (log != null) log("下载失败：" + ex.Message);
                }
            }
            return false;
        }

        // 一键部署 dsh：npm install -g（无权限时回退到当前用户目录安装）
        public static bool DeployDsh(Action<string> log)
        {
            if (NpmPath.Length == 0)
            {
                log("未找到 npm，请先安装 Node.js。");
                return false;
            }
            log("使用 npm 安装 @deepseek-ai/dsh（可能需要一两分钟）…");
            int code = RunProcess(NpmPath, "install -g @deepseek-ai/dsh",
                Path.GetDirectoryName(NpmPath), log, DeployTimeoutMs);
            Detect();
            if (DshOk) { log("dsh 安装成功 ✓"); return true; }
            // 机器级 Node 时全局目录可能无写权限（EACCES）：改用当前用户目录 --prefix 重试
            string prefix = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "npm");
            log("默认全局安装未成功（npm 退出码 " + code + "），尝试安装到当前用户目录…");
            code = RunProcess(NpmPath, "install -g @deepseek-ai/dsh --prefix \"" + prefix + "\"",
                Path.GetDirectoryName(NpmPath), log, DeployTimeoutMs);
            Detect();
            if (DshOk) { log("dsh 安装成功 ✓（当前用户目录）"); return true; }
            log("dsh 安装未成功（npm 退出码 " + code + "）。");
            return false;
        }

        // 一键部署 WebView2 运行时（内嵌界面依赖；官方 Evergreen 引导程序，随 Edge 更新）
        public static bool DeployWebView2(Action<string> log)
        {
            log("开始部署 WebView2 运行时…");
            try
            {
                string boot = Path.Combine(Path.GetTempPath(),
                    "webview2-bootstrapper-" + Process.GetCurrentProcess().Id + "-" + Guid.NewGuid().ToString("N").Substring(0, 6) + ".exe");
                log("正在下载 WebView2 运行时引导程序…");
                if (!DownloadFileWithRetry("https://go.microsoft.com/fwlink/p/?LinkId=2124703", boot, log, 512 * 1024))
                {
                    log("WebView2 引导程序下载失败，请检查网络后重试。");
                    try { File.Delete(boot); } catch { }
                    return false;
                }
                log("正在静默安装 WebView2 运行时…" + ElevationNote());
                int code = RunProcess(boot, "/silent /install", "", log, DeployTimeoutMs);
                try { File.Delete(boot); } catch { }
                Detect();
                if (WebView2Ok) { log("WebView2 运行时安装成功 ✓"); return true; }
                log("WebView2 安装未成功（退出码 " + code + "，可能需要管理员权限）。");
            }
            catch (Exception ex)
            {
                log("下载/安装失败：" + ex.Message);
            }
            return false;
        }

        private static string FindOnPath(string name)
        {
            foreach (string dir in SplitPath())
            {
                string cand = Path.Combine(dir, name);
                if (File.Exists(cand)) return cand;
            }
            return "";
        }

        public static int RunProcess(string file, string args, string workDir, Action<string> log, int timeoutMs)
        {
            try
            {
                ProcessStartInfo psi = new ProcessStartInfo();
                psi.FileName = file;
                psi.Arguments = args;
                psi.UseShellExecute = false;
                psi.CreateNoWindow = true;
                psi.RedirectStandardOutput = true;
                psi.RedirectStandardError = true;
                // node/npm 输出为 UTF-8，明确编码避免中文系统（GBK）下日志乱码
                psi.StandardOutputEncoding = Encoding.UTF8;
                psi.StandardErrorEncoding = Encoding.UTF8;
                if (workDir.Length > 0) psi.WorkingDirectory = workDir;
                using (Process p = Process.Start(psi))
                {
                    p.OutputDataReceived += delegate(object s, DataReceivedEventArgs e)
                    {
                        if (e.Data != null && log != null) log(e.Data);
                    };
                    p.ErrorDataReceived += delegate(object s, DataReceivedEventArgs e)
                    {
                        if (e.Data != null && log != null) log("[err] " + e.Data);
                    };
                    p.BeginOutputReadLine();
                    p.BeginErrorReadLine();
                    if (!p.WaitForExit(timeoutMs))
                    {
                        // 结束整个进程树，避免留下孤儿进程
                        try
                        {
                            string tk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "taskkill.exe");
                            ProcessStartInfo kpsi = new ProcessStartInfo(tk, "/PID " + p.Id + " /T /F");
                            kpsi.UseShellExecute = false;
                            kpsi.CreateNoWindow = true;
                            using (Process kp = Process.Start(kpsi)) { if (kp != null) kp.WaitForExit(3000); }
                        }
                        catch { }
                        try { p.Kill(); } catch { }
                        try { p.Dispose(); } catch { }
                        if (log != null) log("进程超时，已强制终止。");
                        return -1;
                    }
                    return p.ExitCode;
                }
            }
            catch (Exception ex)
            {
                if (log != null) log("执行失败：" + ex.Message);
                return -1;
            }
        }
    }

    // ---------------------------------------------------------------------
    // 静默安装（命令行模式，也用于自动化测试）
    // ---------------------------------------------------------------------
    internal static class SilentInstall
    {
        /// <summary>
        /// 日志路径的唯一解析入口（P2-11）：Run 与 DetectOnly 共用同一份逻辑。
        /// 旧实现里只有 Run 探测可写性，DetectOnly 直接往安装包所在目录写——安装包位于
        /// 只读目录（或 Program Files）时，`--detect-only` 会因为日志写不进去而报成
        /// 「环境异常」，掩盖真正的问题。
        /// </summary>
        public static string ResolveLogPath(string fileName)
        {
            string beside = null;
            try
            {
                beside = Path.Combine(Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location), fileName);
                File.AppendAllText(beside, ""); // 探测可写性
                return beside;
            }
            catch { }

            string temp = null;
            try
            {
                temp = Path.Combine(Path.GetTempPath(), "dshlauncher-" + fileName);
                File.AppendAllText(temp, ""); // %TEMP% 也可能不可写（受限环境）
                return temp;
            }
            catch { }

            // 两处都不可写：仍返回 %TEMP% 路径（调用方对写入失败已经是容忍的），
            // 但没有可写日志这一点必须在日志/控制台里说出来。
            return temp != null ? temp : (beside != null ? beside : fileName);
        }

        public static int Run(string dir)
        {
            if (dir == null || dir.Trim().Length == 0) dir = Program.DefaultInstallDir();
            string logPath = ResolveLogPath("setup.log");
            Action<string> log = delegate(string s)
            {
                string t = "[" + DateTime.Now.ToString("HH:mm:ss") + "] " + s;
                try { File.AppendAllText(logPath, t + "\r\n", new UTF8Encoding(true)); } catch { }
            };
            try
            {
                log("=== " + Program.AppName + " 静默安装 ===");
                log("目标目录: " + dir);
                log("日志文件: " + logPath);
                log("权限状态: " + Env.ElevationNote());
                Env.Detect();
                log("Node.js : " + (Env.NodeOk ? Env.NodePath : "未找到"));
                log("dsh     : " + (Env.DshOk ? Env.BinJs : "未找到"));
                log("WebView2: " + (Env.WebView2Ok ? "已安装" : "未找到"));
                if (!Env.NodeOk)
                {
                    log("缺少 Node.js，开始一键部署…");
                    if (!Env.DeployNode(log)) { log("FAIL: Node.js 部署失败"); return 1; }
                }
                if (!Env.DshOk)
                {
                    log("缺少 dsh，开始一键部署…");
                    if (!Env.DeployDsh(log)) { log("FAIL: dsh 部署失败"); return 1; }
                }
                if (!Env.WebView2Ok)
                {
                    log("缺少 WebView2 运行时，开始一键部署…");
                    // 非致命：部署失败时内嵌界面会自动回退 Edge
                    if (!Env.DeployWebView2(log)) { log("WARN: WebView2 部署失败（内嵌界面不可用时将回退 Edge）"); }
                }
                if (!Installer.Install(dir, log, true))
                {
                    // P1-5：注册表项 / 快捷方式这类步骤失败必须冒泡成非零退出码——
                    // 旧实现吞掉异常后返回 0，用户得到一个「显示安装成功但无法卸载」的安装。
                    log("FAIL: 安装未完全成功（详见上面的 WARN / FAIL 行）");
                    return 1;
                }
                log("安装完成 ✓ 目标: " + dir);
                return 0;
            }
            catch (Exception ex)
            {
                log("异常: " + ex.ToString());
                return 1;
            }
        }

        public static int DetectOnly()
        {
            // P2-11：与 Run 共用同一个可写性探测（失败回退 %TEMP%）。
            // 旧实现直接写安装包所在目录：只读目录下 --detect-only 会把「日志写不进去」
            // 误报成「环境异常」。
            string logPath = ResolveLogPath("setup.detect.log");
            try
            {
                Env.Detect();
                StringBuilder sb = new StringBuilder();
                sb.AppendLine("node=" + (Env.NodeOk ? Env.NodePath : "MISSING"));
                sb.AppendLine("dsh=" + (Env.DshOk ? Env.BinJs : "MISSING"));
                sb.AppendLine("npm=" + (Env.NpmPath.Length > 0 ? Env.NpmPath : "MISSING"));
                sb.AppendLine("webview2=" + (Env.WebView2Ok ? "OK" : "MISSING"));
                sb.AppendLine("elevated=" + (Env.IsElevated() ? "yes" : "no"));
                sb.AppendLine("log=" + logPath);
                File.WriteAllText(logPath, sb.ToString(), new UTF8Encoding(true));
                return (Env.NodeOk && Env.DshOk) ? 0 : 1;
            }
            catch
            {
                return 1;
            }
        }
    }

    // ---------------------------------------------------------------------
    // 安装动作：释放资源 / 快捷方式 / 卸载 / 注册表
    // ---------------------------------------------------------------------
    internal static class Installer
    {
        /// <summary>安装负载（内嵌资源 → 安装目录）。</summary>
        private static readonly string[] Payloads = new string[] {
            "DSHLauncher.exe", "dsh-uninstall.exe", "app.ico", "README.md", "MAINTENANCE.zh.md", "MAINTENANCE.en.md"
        };

        /// <summary>
        /// **上一版遗留、本版不再发布的载荷**：升级安装时必须清掉。
        ///
        /// 为什么需要：v5.0.0 LTS 把脚本卸载器（uninstall.cmd + uninstall.ps1）换成了原生
        /// `dsh-uninstall.exe`，但升级只覆盖"本版载荷"、不会碰旧文件 —— 实测升级后安装目录里
        /// **同时存在两套卸载器**，而旧脚本自己的清理清单里没有 `dsh-uninstall.exe`，
        /// 用户若点到旧脚本就会留下卸载器（且新的卸载器 crate 会把旧脚本误判成"第三方文件"
        /// 而保留目录 ⇒ 卸载不干净）。
        /// 与 `dsh-uninstall` 的 `LEGACY_PAYLOADS` 必须逐项一致（一致性门禁会比对）。
        /// </summary>
        private static readonly string[] ObsoletePayloads = new string[] {
            "uninstall.cmd", "uninstall.ps1"
        };

        /// <summary>事务化暂存后缀：所有负载先写 '&lt;name&gt;.tmp'，全部就绪后才落位（P1-6）。</summary>
        public const string TmpSuffix = ".tmp";

        /// <summary>安装标记文件名（与 InstallDirGuard 共用，卸载器据此决定是否递归删除）。</summary>
        public const string MarkerName = InstallDirGuard.MarkerName;

        /// <summary>优雅退出的请求进程最多等待时间（启动器 --quit 内部还要等回执）。</summary>
        private const int QuitWaitMs = 8000;
        /// <summary>优雅退出后的有界等待步数（100ms/步 → 15 秒）。</summary>
        private const int GracefulWaitSteps = 150;

        /// <summary>
        /// 安装。返回 true = 全部步骤成功；返回 false = 只有「可选步骤」失败（桌面快捷方式），
        /// 安装本身可用，调用方**不得**把它报告成完全成功（P1-5）。
        /// 致命失败（目录校验、解压、卸载器生成、注册表）抛异常，并已回滚本次创建的文件（P1-6）。
        /// </summary>
        public static bool Install(string dir, Action<string> log, bool createShortcut)
        {
            // P0-1：未通过校验的目录绝不进入安装——生成的卸载器会递归删除安装目录
            string full, reason;
            if (!InstallDirGuard.TryResolve(dir, out full, out reason))
            {
                throw new ArgumentException("安装目录被拒绝：" + reason);
            }
            dir = full;

            StopLauncherInDir(dir, log); // 升级场景：先结束运行中的旧版，避免文件占用导致提取失败

            bool dirExisted = Directory.Exists(dir);
            bool keyExisted = false;
            try { keyExisted = Registry.CurrentUser.OpenSubKey(Program.UninstallKey) != null; }
            catch { }

            List<string> created = new List<string>();
            List<StagedFile> plan = new List<StagedFile>();
            bool shortcutOk = true;
            try
            {
                Directory.CreateDirectory(dir);

                // ---- 阶段 1：把**所有**负载先写成 <name>.tmp（P1-6）----
                // 旧实现是「逐个解压 + 立即覆盖目标」：中途失败会留下一个既没有卸载器、
                // 也没有 Add/Remove 条目的半截安装，而且已经落位的文件无法回收。
                foreach (string name in Payloads)
                {
                    string payload = name;
                    plan.Add(NewStaged(dir, payload, delegate(string tmp) { ExtractResourceToTmp(payload, tmp); }));
                }
                // 卸载器不再是"生成的脚本"，而是随安装包发布的**原生 exe**
                // （`dsh-uninstall.exe` 已在 Payloads 里，因此同样走"先写 .tmp → 全部就绪才落位"
                //   的事务化流程，失败会回滚并冒泡）。
                // 为什么改（v5.0.0 LTS）：脚本卸载器依赖 PowerShell 与执行策略 —— 组策略
                // AllSigned/Restricted 会覆盖命令行 `-ExecutionPolicy Bypass`，AppLocker/WDAC
                // 也能直接封锁脚本执行，加固环境下用户**根本卸载不掉**；原生 exe 没有这层依赖，
                // 且路径为原生 UTF-16（非 ASCII / 超长 / 含引号的安装目录都安全）。
                plan.Add(NewStaged(dir, MarkerName, delegate(string tmp)
                {
                    // 安装标记（P0-1）：卸载器只在看到它时才递归删除安装目录
                    File.WriteAllText(tmp, BuildMarkerText(dir), new UTF8Encoding(true));
                }));
                foreach (StagedFile s in plan) s.WriteTmpFile();

                // ---- 阶段 2：全部暂存成功后才逐个落位（同卷原子替换）----
                foreach (StagedFile s in plan)
                {
                    CommitStaged(s);
                    if (!s.ExistedBefore) created.Add(s.Final);
                }

                // ---- 阶段 3：清理上一版遗留的载荷（升级路径）----
                // 放在落位**之后**：即使这里失败，新版安装也是完整可用的。
                foreach (string name in ObsoletePayloads)
                {
                    string stale = Path.Combine(dir, name);
                    foreach (string victim in new string[] { stale, stale + TmpSuffix })
                    {
                        if (!File.Exists(victim)) continue;
                        try
                        {
                            File.Delete(victim);
                            log("已清理上一版遗留文件：" + Path.GetFileName(victim));
                        }
                        catch (Exception ex)
                        {
                            if (log != null) log("WARN: 无法删除上一版遗留文件 " + victim + "：" + ex.Message);
                        }
                    }
                }

                // 卸载注册表项：失败必须冒泡（P1-5），否则装出来的东西无法卸载却报告成功
                if (!RegisterUninstall(dir, log))
                {
                    throw new Exception("卸载注册表项写入失败（该安装将无法从“设置 → 应用”卸载）");
                }

                if (createShortcut)
                {
                    shortcutOk = CreateShortcut(dir, log);
                    if (!shortcutOk && log != null)
                    {
                        log("WARN: 桌面快捷方式创建失败（安装文件本身已就绪，可从安装目录直接启动）。");
                    }
                }
                log("文件已复制到 " + dir);
                return shortcutOk;
            }
            catch (Exception ex)
            {
                RollbackInstall(plan, created, dir, dirExisted, keyExisted, log);
                if (ex is ArgumentException) throw; // 校验失败：原样抛出（调用方按参数错误处理）
                if (ex is IOException)
                {
                    throw new IOException("安装文件写入失败：" + ex.Message
                        + "。若提示“正由另一进程使用”，请先关闭正在运行的 " + Program.AppName + " 后重试。");
                }
                throw new Exception("安装失败：" + ex.Message);
            }
        }

        // -----------------------------------------------------------------
        // 事务化安装的暂存/落位/回滚（P1-6）
        // -----------------------------------------------------------------
        private sealed class StagedFile
        {
            public string Final;
            public string Tmp;
            public bool ExistedBefore;
            public Action<string> WriteTmp;

            public void WriteTmpFile()
            {
                if (WriteTmp != null) WriteTmp(Tmp);
            }
        }

        private static StagedFile NewStaged(string dir, string name, Action<string> writeTmp)
        {
            StagedFile s = new StagedFile();
            s.Final = Path.Combine(dir, name);
            s.Tmp = s.Final + TmpSuffix;
            s.ExistedBefore = File.Exists(s.Final);
            s.WriteTmp = writeTmp;
            return s;
        }

        private static void CommitStaged(StagedFile s)
        {
            if (File.Exists(s.Final)) File.Replace(s.Tmp, s.Final, null); // 同卷原子替换（目标存在）
            else File.Move(s.Tmp, s.Final);
        }

        private static string BuildMarkerText(string dir)
        {
            // 首行是**产品名**（人类可读）；同时显式写入内部标识 `product=`，
            // 使卸载器的标记校验既能接受历史安装（只有产品名），也不再依赖两处字符串巧合。
            // 背景：卸载器曾用 content.contains("DSHLauncher") 校验，而产品名不含该子串，
            // 导致**真实安装无法卸载**（退出码 2，拒绝执行任何删除）。
            return Program.AppName + "\r\n"
                + "product=" + Program.InstallSubDir + "\r\n"
                + "version=" + Program.AppVersion + " LTS" + "\r\n"
                + "installed=" + DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + "\r\n"
                + "install_dir=" + dir + "\r\n";
        }

        /// <summary>
        /// 安装失败时的回滚（P1-6）：删掉本次运行创建的 .tmp 与新文件；本次新建的目录
        /// 若已空则一并删除；本次新建的注册表项也删掉。升级场景下**原有文件与原有注册表
        /// 项保持不变**（只删本次 created 列表里的东西）。
        /// </summary>
        private static void RollbackInstall(List<StagedFile> plan, List<string> created, string dir,
            bool dirExisted, bool keyExisted, Action<string> log)
        {
            int rolled = 0;
            foreach (StagedFile s in plan)
            {
                try { if (File.Exists(s.Tmp)) { File.Delete(s.Tmp); rolled++; } }
                catch { }
            }
            foreach (string f in created)
            {
                try { if (File.Exists(f)) { File.Delete(f); rolled++; } }
                catch { }
            }
            if (!keyExisted)
            {
                try
                {
                    if (Registry.CurrentUser.OpenSubKey(Program.UninstallKey) != null)
                    {
                        Registry.CurrentUser.DeleteSubKeyTree(Program.UninstallKey);
                    }
                }
                catch { }
            }
            if (!dirExisted)
            {
                try
                {
                    if (Directory.Exists(dir) && Directory.GetFileSystemEntries(dir).Length == 0)
                    {
                        Directory.Delete(dir);
                    }
                }
                catch { }
            }
            if (log != null)
            {
                log("安装失败：已回滚本次创建的 " + rolled + " 个文件"
                    + (dirExisted ? "（原有文件保持不变）" : "（安装目录为本次新建，已清空）"));
            }
        }

        // 仅当目标目录里已有正在运行的旧版启动器时结束它（避免误杀其它位置运行的实例）。
        //
        // 先请求优雅退出，再兜底强杀（P2-8）：
        //   `<dir>\DSHLauncher.exe --quit` 走的是与托盘「退出」完全相同的收尾路径。
        //   退出码契约（crates/dsh-app/src/main.rs + dsh-core/single_instance.rs）：
        //     0 = 已确认优雅退出，或本来就没有实例；
        //     3 = 请求已送达但超时无回执（例如 tied 模式下用户还没在确认框上选择）。
        //   注意「已回执」并不等于「进程已消失」——用户点“取消”也会回执。因此不能只看
        //   退出码，必须再有界等待目标进程真的退出，然后才 fallback 到 taskkill /PID /F。
        //
        // 关于「退出保留服务」的真相（service_lifecycle 默认 independent）：
        //   * independent（默认）：dsh 子进程用 CREATE_BREAKAWAY_FROM_JOB 创建，
        //     **根本不在**启动器的 Job Object 里（见 crates/dsh-core/src/process.rs）。
        //     启动器无论被强杀还是正常退出都不会带走它，这里不带 /T 的 taskkill /F 同样
        //     只结束启动器本体。
        //   * tied：dsh 子进程挂在启动器的 Job（JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE）上。
        //     优雅退出时启动器会解除该限制从而保留服务（keep_children_on_exit）；但**强杀**
        //     走不到那段代码——Job 句柄随进程被内核关闭，整棵进程树一并回收，
        //     与 taskkill 是否带 /T 无关。
        //   （旧注释声称「dsh 由本启动器的 Job Object 托管、优雅退出时解除
        //     KILL_ON_JOB_CLOSE、不带 /T 就是为了保留服务」——在 independent 默认值下
        //     整段都不成立：那时子进程压根不在我们的 Job 里。）
        private static void StopLauncherInDir(string dir, Action<string> log)
        {
            try
            {
                string target = Path.Combine(dir, Program.LauncherExe).ToLowerInvariant();
                string exe = Path.Combine(dir, Program.LauncherExe);
                foreach (Process p in Process.GetProcessesByName("DSHLauncher"))
                {
                    bool match = false;
                    try { match = p.MainModule.FileName.ToLowerInvariant() == target; }
                    catch { }
                    if (!match) { try { p.Dispose(); } catch { } continue; }

                    if (log != null)
                    {
                        log("检测到旧版 " + Program.AppName + " 正在运行（PID " + p.Id + "），先请求它优雅退出…");
                    }

                    // 1) 优先让启动器自己收尾（服务去留由它按 service_lifecycle 决定）
                    int quitCode = -1;
                    if (File.Exists(exe))
                    {
                        try
                        {
                            ProcessStartInfo gpsi = new ProcessStartInfo(exe, "--quit");
                            gpsi.UseShellExecute = false;
                            gpsi.CreateNoWindow = true;
                            using (Process gp = Process.Start(gpsi))
                            {
                                if (gp != null && gp.WaitForExit(QuitWaitMs)) quitCode = gp.ExitCode;
                            }
                        }
                        catch { }
                    }
                    else if (log != null)
                    {
                        log("目标目录里没有 " + Program.LauncherExe + "，无法请求优雅退出，直接强制结束。");
                    }

                    // 2) 有界等待目标进程真的退出（最多 15 秒）
                    bool exited = false;
                    for (int i = 0; i < GracefulWaitSteps; i++)
                    {
                        try { if (p.HasExited) { exited = true; break; } }
                        catch { exited = true; break; }
                        if (i == 0 && log != null) log("等待旧版优雅退出（最多 " + (GracefulWaitSteps / 10) + " 秒）…");
                        Thread.Sleep(100);
                    }
                    if (exited)
                    {
                        if (log != null) log("旧版已优雅退出（--quit 退出码 " + quitCode + "）。");
                        try { p.Dispose(); } catch { }
                        continue;
                    }

                    // 3) 兜底：强制结束启动器本体（不带 /T，避免连带结束独立的 dsh 服务）
                    if (log != null)
                    {
                        log("优雅退出未生效（--quit 退出码 " + quitCode + "），改用 taskkill /PID " + p.Id + " /F。");
                    }
                    try
                    {
                        string tk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "taskkill.exe");
                        ProcessStartInfo psi = new ProcessStartInfo(tk, "/PID " + p.Id + " /F");
                        psi.UseShellExecute = false;
                        psi.CreateNoWindow = true;
                        using (Process kp = Process.Start(psi)) { if (kp != null) kp.WaitForExit(5000); }
                    }
                    catch { }
                    // 等待进程真正退出，避免立即解压时文件仍被占用（taskkill /F 是异步生效的）
                    for (int i = 0; i < 30; i++)
                    {
                        try { if (p.HasExited) break; } catch { break; }
                        Thread.Sleep(100);
                    }
                    try { p.Dispose(); } catch { }
                }
            }
            catch { }
        }

        // 只负责把内嵌资源写到**暂存路径**（<name>.tmp）。落位与回滚由 Install 统一处理：
        // 全部暂存成功后才 File.Move/File.Replace，中途失败不会留下半截安装（P1-6）。
        private static void ExtractResourceToTmp(string name, string tmp)
        {
            using (Stream s = Assembly.GetExecutingAssembly().GetManifestResourceStream(name))
            {
                if (s == null) throw new Exception("内嵌资源缺失: " + name);
                using (FileStream fs = File.Create(tmp))
                {
                    byte[] buf = new byte[65536];
                    int n;
                    while ((n = s.Read(buf, 0, buf.Length)) > 0) fs.Write(buf, 0, n);
                }
            }
        }

        /// <summary>
        /// 把两个**生成**的卸载器文件登记进事务化 plan（名字保留自旧实现）。
        /// 旧写法直接往安装目录写字、且没有任何失败上报点；现在写入发生在 Install 的
        /// 事务块内：任何一步失败都会抛异常 → 回滚 + 明确报错（P1-5 / P1-6）。
        /// </summary>
        /// <summary>
        /// 写 HKCU 卸载注册表项。返回 false = 写入失败或后置校验失败（P1-5）。
        /// 旧实现以裸 `catch { }` 收尾：注册表写不进去时静默安装照样返回 0、GUI 照样显示
        /// “安装完成”，用户得到一个**无法卸载**的安装。
        /// </summary>
        private static bool RegisterUninstall(string dir, Action<string> log)
        {
            if (dir == null || dir.Trim().Length == 0)
            {
                if (log != null) log("FAIL: 安装目录为空，无法写入卸载注册表项。");
                return false;
            }
            try
            {
                // 用 RegistryKey API 直写（毫秒级、可感知失败），替代串行 8 次 reg.exe 子进程
                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(Program.UninstallKey))
                {
                    if (key == null)
                    {
                        if (log != null) log("FAIL: 无法创建卸载注册表项 " + Program.UninstallKey);
                        return false;
                    }
                    key.SetValue("DisplayName", Program.AppName);
                    key.SetValue("DisplayVersion", Program.AppVersion);
                    key.SetValue("Version", Program.AppVersion); // 部分第三方卸载工具读这个
                    key.SetValue("Publisher", Program.Publisher);
                    key.SetValue("InstallLocation", dir);
                    key.SetValue("DisplayIcon", Path.Combine(dir, Program.LauncherExe));
                    string uninstallExe = Path.Combine(dir, "dsh-uninstall.exe");
                    key.SetValue("UninstallString", "\"" + uninstallExe + "\"");
                    // 静默卸载入口（MDM/脚本用）；用户配置默认保留，--purge 才连配置一起删
                    key.SetValue("QuietUninstallString", "\"" + uninstallExe + "\" --silent");
                    key.SetValue("NoModify", 1);
                    key.SetValue("NoRepair", 1);
                    // 估算大小（KB）：供“设置 → 应用”展示。只统计安装目录根下的文件
                    // （exe + 图标 + 三份文档），避免为了一个展示字段做全树遍历拖慢安装。
                    // 这个 catch 是真正无害的：EstimatedSize 只是展示字段，失败不影响卸载。
                    try
                    {
                        long est = 0;
                        foreach (string f in Directory.GetFiles(dir))
                        {
                            try { est += new FileInfo(f).Length; } catch { }
                        }
                        key.SetValue("EstimatedSize", (int)(est / 1024));
                    }
                    catch { }
                }

                // 后置校验：键与关键值必须真的读得回来（P1-5）
                using (RegistryKey chk = Registry.CurrentUser.OpenSubKey(Program.UninstallKey))
                {
                    if (chk == null)
                    {
                        if (log != null) log("FAIL: 卸载注册表项写入后读不回来：" + Program.UninstallKey);
                        return false;
                    }
                    object uninstall = chk.GetValue("UninstallString");
                    object quiet = chk.GetValue("QuietUninstallString");
                    if (uninstall == null || uninstall.ToString().Trim().Length == 0)
                    {
                        if (log != null) log("FAIL: 卸载注册表项缺少 UninstallString。");
                        return false;
                    }
                    if (quiet == null || quiet.ToString().Trim().Length == 0)
                    {
                        if (log != null) log("FAIL: 卸载注册表项缺少 QuietUninstallString（静默卸载入口）。");
                        return false;
                    }
                }
                return true;
            }
            catch (Exception ex)
            {
                if (log != null) log("FAIL: 写入卸载注册表项失败：" + ex.ToString());
                return false;
            }
        }

        /// <summary>
        /// 创建桌面快捷方式。返回 false = 失败（P1-5）：快捷方式失败可以是非致命的，
        /// 但必须记录日志，并且调用方**不得**把整个安装报告成完全成功。
        /// </summary>
        public static bool CreateShortcut(string dir, Action<string> log)
        {
            string desktop = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
            if (desktop.Length == 0)
            {
                if (log != null) log("WARN: 取不到桌面目录，跳过快捷方式创建。");
                return false;
            }
            string lnk = Path.Combine(desktop, Program.ShortcutName + ".lnk");
            try
            {
                Type t = Type.GetTypeFromProgID("WScript.Shell");
                if (t == null)
                {
                    if (log != null) log("WARN: 系统未注册 WScript.Shell，无法创建快捷方式。");
                    return false;
                }
                object shell = Activator.CreateInstance(t);
                object sc = t.InvokeMember("CreateShortcut", BindingFlags.InvokeMethod, null, shell,
                    new object[] { lnk });
                Type scType = sc.GetType();
                scType.InvokeMember("TargetPath", BindingFlags.SetProperty, null, sc,
                    new object[] { Path.Combine(dir, "DSHLauncher.exe") });
                scType.InvokeMember("WorkingDirectory", BindingFlags.SetProperty, null, sc, new object[] { dir });
                scType.InvokeMember("IconLocation", BindingFlags.SetProperty, null, sc,
                    new object[] { Path.Combine(dir, "app.ico") + ",0" });
                scType.InvokeMember("Description", BindingFlags.SetProperty, null, sc,
                    new object[] { "一键启动 DeepSeek Harness" });
                scType.InvokeMember("Save", BindingFlags.InvokeMethod, null, sc, null);
                return File.Exists(lnk);
            }
            catch (Exception ex)
            {
                if (log != null) log("WARN: 创建桌面快捷方式失败：" + ex.Message);
                return false;
            }
        }
    }

    // ---------------------------------------------------------------------
    // 向导窗体
    // ---------------------------------------------------------------------
    internal class SetupForm : Form
    {
        private Panel[] pages;
        private int page = 0;
        private Button btnBack, btnNext, btnCancel;
        private Label lblHeader;

        // 环境页
        private Label lblNode, lblDsh, lblNpm, lblWebView2;
        private Button btnDeployAll, btnRedetect;
        private CheckBox chkSkip;
        private Label lblEnvHint;
        private TextBox txtDeployLog;

        // 安装选项页
        private TextBox txtInstallDir;
        private Button btnBrowse;

        // 安装中页
        private Label lblInstallStatus;
        private TextBox txtInstallLog;
        private ProgressBar progBar;

        // 完成页
        private CheckBox chkLaunch, chkShortcut, chkGuide;
        private Label lblDone;
        private Label lblInstalledDir;

        private string installDir = Program.DefaultInstallDir();
        private bool envOk = false;
        private bool installDone = false;
        private bool deploying = false;

        public SetupForm()
        {
            this.Text = Program.AppName + " 安装向导";
            this.ClientSize = new Size(660, 480);
            this.MinimumSize = new Size(620, 440);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.Font = new Font("Microsoft YaHei UI", 9f);
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); }
            catch { }

            lblHeader = new Label();
            lblHeader.SetBounds(12, 10, 636, 44);
            lblHeader.Font = new Font("Microsoft YaHei UI", 15f, FontStyle.Bold);
            lblHeader.Text = Program.AppName + " 一键安装包 v" + Program.AppVersion + " LTS";
            lblHeader.ForeColor = Color.FromArgb(37, 99, 235);

            btnBack = new Button();
            btnBack.SetBounds(400, 440, 88, 30);
            btnBack.Text = "上一步";
            btnBack.Click += delegate { Go(-1); };

            btnNext = new Button();
            btnNext.SetBounds(494, 440, 88, 30);
            btnNext.Text = "下一步";
            btnNext.Click += delegate { Go(1); };

            btnCancel = new Button();
            btnCancel.SetBounds(588, 440, 60, 30);
            btnCancel.Text = "取消";
            btnCancel.Click += delegate
            {
                if (page >= 3 && installDone)
                {
                    DialogResult r = MessageBox.Show("安装已完成，确定要退出吗？", Program.AppName,
                        MessageBoxButtons.OKCancel, MessageBoxIcon.Question);
                    if (r == DialogResult.OK) Close();
                }
                else
                {
                    Close();
                }
            };

            this.Controls.Add(lblHeader);
            this.Controls.Add(btnBack);
            this.Controls.Add(btnNext);
            this.Controls.Add(btnCancel);

            BuildPages();
            ShowPage(0);
        }

        private void BuildPages()
        {
            pages = new Panel[5];
            for (int i = 0; i < 5; i++)
            {
                Panel p = new Panel();
                p.SetBounds(12, 56, 636, 372);
                p.Visible = false;
                this.Controls.Add(p);
                pages[i] = p;
            }

            // ---------- 页 0: 欢迎 ----------
            Label welcome = new Label();
            welcome.SetBounds(20, 40, 596, 220);
            welcome.Font = new Font("Microsoft YaHei UI", 11f);
            welcome.Text =
                "本安装包会完成三件事：\n\n" +
                "1. 检测电脑是否已具备运行环境（Node.js、dsh 与 WebView2 运行时）；\n" +
                "2. 如果缺环境，可一键自动部署（winget / 官方安装包 / npm / 微软官方）；\n" +
                "3. 安装 " + Program.AppName + "，一个双击即可启动 DeepSeek Harness 的小工具。\n\n" +
                "· 安装 " + Program.AppName + " 本身为当前用户，不需要管理员权限；\n" +
                "· 但第 2 步的“一键部署环境”需要管理员权限：本安装包**不会**弹出 UAC，\n" +
                "  若部署失败，请右键安装包选择“以管理员身份运行”后重试。\n\n" +
                "点击“下一步”开始。";
            Label welcome2 = new Label();
            welcome2.SetBounds(20, 280, 596, 60);
            welcome2.ForeColor = Color.FromArgb(110, 110, 110);
            welcome2.Text = "提示：新手指引会告诉你端口是什么、工作目录怎么选，\n安装完成后可以一键打开。";
            pages[0].Controls.Add(welcome);
            pages[0].Controls.Add(welcome2);

            // ---------- 页 1: 环境检测 ----------
            lblNode = new Label();
            lblNode.SetBounds(20, 30, 596, 24);
            lblDsh = new Label();
            lblDsh.SetBounds(20, 58, 596, 24);
            lblNpm = new Label();
            lblNpm.SetBounds(20, 86, 290, 24);
            lblWebView2 = new Label();
            lblWebView2.SetBounds(310, 86, 290, 24);

            btnDeployAll = new Button();
            btnDeployAll.SetBounds(20, 120, 200, 32);
            btnDeployAll.Text = "一键部署缺失环境";
            btnDeployAll.Click += BtnDeployAllClick;

            btnRedetect = new Button();
            btnRedetect.SetBounds(228, 120, 110, 32);
            btnRedetect.Text = "重新检测";
            btnRedetect.Click += delegate { RefreshEnv(); };

            chkSkip = new CheckBox();
            chkSkip.SetBounds(348, 125, 280, 24);
            chkSkip.Text = "环境稍后再装，仍要继续安装";
            chkSkip.CheckedChanged += delegate { UpdateNextState(); };

            lblEnvHint = new Label();
            lblEnvHint.SetBounds(20, 158, 596, 26);

            txtDeployLog = new TextBox();
            txtDeployLog.SetBounds(20, 190, 596, 160);
            txtDeployLog.Multiline = true;
            txtDeployLog.ReadOnly = true;
            txtDeployLog.ScrollBars = ScrollBars.Both;
            txtDeployLog.WordWrap = false;
            txtDeployLog.BackColor = Color.White;
            txtDeployLog.Font = new Font("Consolas", 9f);

            pages[1].Controls.Add(lblNode);
            pages[1].Controls.Add(lblDsh);
            pages[1].Controls.Add(lblNpm);
            pages[1].Controls.Add(lblWebView2);
            pages[1].Controls.Add(btnDeployAll);
            pages[1].Controls.Add(btnRedetect);
            pages[1].Controls.Add(chkSkip);
            pages[1].Controls.Add(lblEnvHint);
            pages[1].Controls.Add(txtDeployLog);

            // ---------- 页 2: 安装选项 ----------
            Label lblDir = new Label();
            lblDir.SetBounds(20, 60, 596, 24);
            lblDir.Text = "选择安装目录：";

            txtInstallDir = new TextBox();
            txtInstallDir.SetBounds(20, 90, 500, 26);
            txtInstallDir.Text = installDir;

            btnBrowse = new Button();
            btnBrowse.SetBounds(528, 90, 88, 26);
            btnBrowse.Text = "浏览…";
            btnBrowse.Click += delegate
            {
                FolderBrowserDialog dlg = new FolderBrowserDialog();
                dlg.Description = "选择安装目录";
                if (Directory.Exists(txtInstallDir.Text.Trim())) dlg.SelectedPath = txtInstallDir.Text.Trim();
                if (dlg.ShowDialog(this) == DialogResult.OK) txtInstallDir.Text = dlg.SelectedPath;
            };

            Label lblNote = new Label();
            lblNote.SetBounds(20, 130, 596, 120);
            lblNote.ForeColor = Color.FromArgb(110, 110, 110);
            lblNote.Text =
                "· 安装 " + Program.AppName + " 为当前用户，不需要管理员权限；\n" +
                "· 环境部署（Node.js 机器级安装 / WebView2 运行时）需要管理员权限，本安装包不会弹 UAC；\n" +
                "· 已安装过时选择同一目录即为升级，设置文件会自动保留；\n" +
                "· 安装内容：DSHLauncher.exe（单文件）、dsh-uninstall.exe（卸载器）、app.ico、README.md、维护手册（中英双语）；\n" +
                "· 桌面图标与启动选项在最后一步选择。";

            pages[2].Controls.Add(lblDir);
            pages[2].Controls.Add(txtInstallDir);
            pages[2].Controls.Add(btnBrowse);
            pages[2].Controls.Add(lblNote);

            // ---------- 页 3: 安装中 ----------
            lblInstallStatus = new Label();
            lblInstallStatus.SetBounds(20, 110, 596, 28);
            lblInstallStatus.Font = new Font("Microsoft YaHei UI", 11f, FontStyle.Bold);

            txtInstallLog = new TextBox();
            txtInstallLog.SetBounds(20, 145, 596, 150);
            txtInstallLog.Multiline = true;
            txtInstallLog.ReadOnly = true;
            txtInstallLog.ScrollBars = ScrollBars.Both;
            txtInstallLog.WordWrap = false;
            txtInstallLog.BackColor = Color.White;
            txtInstallLog.Font = new Font("Consolas", 9f);

            progBar = new ProgressBar();
            progBar.SetBounds(20, 305, 596, 20);
            progBar.Style = ProgressBarStyle.Marquee;
            progBar.MarqueeAnimationSpeed = 30;

            pages[3].Controls.Add(lblInstallStatus);
            pages[3].Controls.Add(txtInstallLog);
            pages[3].Controls.Add(progBar);

            // ---------- 页 4: 完成 ----------
            lblDone = new Label();
            lblDone.SetBounds(20, 40, 596, 36);
            lblDone.Font = new Font("Microsoft YaHei UI", 14f, FontStyle.Bold);
            lblDone.Text = "✔ 安装完成！";
            lblDone.ForeColor = Color.FromArgb(46, 125, 50);

            chkLaunch = new CheckBox();
            chkLaunch.SetBounds(20, 100, 400, 26);
            chkLaunch.Text = "立即启动 " + Program.AppName;
            chkLaunch.Checked = true;

            chkShortcut = new CheckBox();
            chkShortcut.SetBounds(20, 135, 400, 26);
            chkShortcut.Text = "创建桌面图标";
            chkShortcut.Checked = true;

            chkGuide = new CheckBox();
            chkGuide.SetBounds(20, 170, 400, 26);
            chkGuide.Text = "打开新手指引（端口 / 工作目录怎么设置）";
            chkGuide.Checked = true;

            lblInstalledDir = new Label();
            lblInstalledDir.SetBounds(20, 215, 596, 24);
            lblInstalledDir.ForeColor = Color.FromArgb(110, 110, 110);

            Label tip = new Label();
            tip.SetBounds(20, 250, 596, 90);
            tip.ForeColor = Color.FromArgb(110, 110, 110);
            tip.Text =
                "之后想再次打开：双击桌面图标即可。\n" +
                "想卸载：通过“设置→应用”卸载，或运行安装目录里的 dsh-uninstall.exe；\n" +
                "升级与维护：参见安装目录里的 README.md 与维护手册（MAINTENANCE.zh.md / MAINTENANCE.en.md）。";

            pages[4].Controls.Add(lblDone);
            pages[4].Controls.Add(chkLaunch);
            pages[4].Controls.Add(chkShortcut);
            pages[4].Controls.Add(chkGuide);
            pages[4].Controls.Add(lblInstalledDir);
            pages[4].Controls.Add(tip);
        }

        private void ShowPage(int n)
        {
            for (int i = 0; i < pages.Length; i++) pages[i].Visible = (i == n);
            btnBack.Visible = n > 0;
            btnBack.Enabled = !deploying && !installing && !(n == 3 && installDone);
            btnNext.Enabled = !deploying && !installing;
            btnCancel.Enabled = !deploying && !installing;
            btnNext.Text = (n == pages.Length - 1) ? "完成" : "下一步";
            if (n == 1) RefreshEnv();
            if (n == 2) { installDir = txtInstallDir.Text.Trim(); }
            if (n == 3 && !installDone && !installing) DoInstall();
            if (n == 4)
            {
                lblInstalledDir.Text = "已安装到：" + installDir;
                if (installDone)
                {
                    lblDone.Text = "✔ 安装完成！";
                    lblDone.ForeColor = Color.FromArgb(46, 125, 50);
                }
                else
                {
                    lblDone.Text = "✘ 安装失败";
                    lblDone.ForeColor = Color.FromArgb(198, 40, 40);
                }
                btnNext.Enabled = true;
            }
            UpdateNextState();
        }

        private bool installing = false;

        private void Go(int delta)
        {
            int next = page + delta;
            if (delta > 0 && page == 1 && !envOk && !chkSkip.Checked)
            {
                MessageBox.Show("缺少运行环境。请先点击“一键部署缺失环境”，\n或勾选“环境稍后再装，仍要继续安装”。",
                    Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            if (delta > 0 && page == 4)
            {
                Finish();
                return;
            }
            page = next;
            ShowPage(page);
        }

        private void RefreshEnv()
        {
            Env.Detect();
            SetEnvLabel(lblNode, "Node.js", Env.NodeOk, Env.NodePath, "未找到（需要时点“一键部署”）");
            SetEnvLabel(lblDsh, "dsh", Env.DshOk, Env.BinJs, "未找到（需要时点“一键部署”）");
            SetEnvLabel(lblNpm, "npm", Env.NpmPath.Length > 0, Env.NpmPath, "未找到");
            SetEnvLabel(lblWebView2, "WebView2", Env.WebView2Ok, "已安装", "未安装（内嵌界面需要）");
            envOk = Env.NodeOk && Env.DshOk && Env.WebView2Ok;
            chkSkip.Visible = !envOk;
            if (envOk)
            {
                lblEnvHint.Text = "✓ 环境就绪，可以直接安装。";
                lblEnvHint.ForeColor = Color.FromArgb(46, 125, 50);
            }
            else
            {
                // P2-11：环境部署需要管理员权限，而本安装包不会弹 UAC——如实告知，
                // 并顺带报告当前进程是否已提权（只报告，不尝试自提升）。
                lblEnvHint.Text = "△ 缺少环境：点“一键部署缺失环境”自动安装，安装过程请看下方日志。"
                    + (Env.IsElevated()
                        ? "（当前进程已提权，可直接部署）"
                        : "（当前进程未提权且不会弹 UAC：Node.js / WebView2 安装可能因权限不足失败）");
                lblEnvHint.ForeColor = Color.FromArgb(230, 126, 34);
            }
            UpdateNextState();
        }

        private void SetEnvLabel(Label lbl, string name, bool ok, string detail, string missText)
        {
            lbl.Text = (ok ? "✓ " : "✗ ") + name + "：" + (ok ? detail : missText);
            lbl.ForeColor = ok ? Color.FromArgb(46, 125, 50) : Color.FromArgb(198, 40, 40);
        }

        private void UpdateNextState()
        {
            if (page == 1 && !deploying)
            {
                btnNext.Enabled = envOk || chkSkip.Checked;
            }
        }

        private void BtnDeployAllClick(object sender, EventArgs e)
        {
            if (deploying) return;
            deploying = true;
            btnDeployAll.Enabled = false;
            btnRedetect.Enabled = false;
            btnBack.Enabled = false;
            btnNext.Enabled = false;
            btnCancel.Enabled = false;
            txtDeployLog.Clear();
            UiLog("开始环境检测与部署…");
            Thread t = new Thread(delegate()
            {
                try
                {
                    Env.Detect();
                    if (!Env.NodeOk) Env.DeployNode(UiLog);
                    if (!Env.DshOk) Env.DeployDsh(UiLog);
                    if (!Env.WebView2Ok) Env.DeployWebView2(UiLog);
                }
                catch (Exception ex)
                {
                    UiLog("部署异常：" + ex.Message);
                }
                try
                {
                    BeginInvoke((Action)delegate
                    {
                        deploying = false;
                        UiLog("部署流程结束。");
                        RefreshEnv();
                        btnDeployAll.Enabled = true;
                        btnRedetect.Enabled = true;
                        btnBack.Enabled = true;
                        btnCancel.Enabled = true;
                    });
                }
                catch { } // 窗体已关闭时静默丢弃
            });
            t.IsBackground = true;
            t.Start();
        }

        private void UiLog(string s)
        {
            if (InvokeRequired)
            {
                try { BeginInvoke((Action)(delegate { UiLog(s); })); }
                catch { } // 窗体已关闭时静默丢弃
                return;
            }
            try
            {
                txtDeployLog.AppendText("[" + DateTime.Now.ToString("HH:mm:ss") + "] " + s + "\r\n");
                if (txtDeployLog.TextLength > 200000)
                {
                    int cut = txtDeployLog.Text.IndexOf('\n', 100000);
                    if (cut > 0) { txtDeployLog.Select(0, cut + 1); txtDeployLog.SelectedText = ""; }
                }
                txtDeployLog.SelectionStart = txtDeployLog.TextLength;
                txtDeployLog.ScrollToCaret();
            }
            catch { }
        }

        private void DoInstall()
        {
            txtInstallLog.Clear();

            // P0-1：GUI 侧同样必须校验。旧实现直接把 txtInstallDir.Text.Trim() 当安装目录，
            // 一个字符都不校验——用户填 %LOCALAPPDATA% / C:\Users / %WINDIR%\System32 都会被
            // 原样写进卸载器，卸载时被递归删除。校验不过就停在原地并弹框说明原因。
            string candidate = txtInstallDir.Text.Trim();
            if (candidate.Length == 0) candidate = Program.DefaultInstallDir();
            string full, reason;
            if (!InstallDirGuard.TryResolve(candidate, out full, out reason))
            {
                InstallLog("拒绝安装：" + reason);
                MessageBox.Show("安装目录不可用：\n\n" + reason
                    + "\n\n请选择其它目录（默认：" + Program.DefaultInstallDir() + "）。",
                    Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return; // 不进入安装流程：按钮状态保持可操作，用户可以改路径重试
            }
            installDir = full;

            installing = true;
            btnBack.Enabled = false;
            btnNext.Enabled = false;
            btnCancel.Enabled = false;
            lblInstallStatus.Text = "正在安装到 " + installDir + " …";
            // 后台线程执行安装（资源解压 + 注册表写入可能耗时数秒）：
            // 原实现在 UI 线程同步执行导致窗口“未响应”、进度条冻结，用户易误判死机而强杀
            Thread t = new Thread(delegate()
            {
                bool ok = false;
                bool warned = false;
                string err = "";
                try
                {
                    // P1-5：返回 false = 只有“可选步骤”（桌面快捷方式）失败——安装本身可用，
                    // 但不能报告成完全成功；抛异常 = 致命失败（已回滚）。
                    ok = Installer.Install(installDir, delegate(string s)
                    {
                        if (InvokeRequired) BeginInvoke((Action)(delegate { InstallLog(s); }));
                        else InstallLog(s);
                    }, false);
                    warned = !ok;
                    ok = true;
                }
                catch (Exception ex) { err = ex.Message; ok = false; }
                try
                {
                    BeginInvoke((Action)delegate
                    {
                        installing = false;
                        installDone = ok;
                        if (!ok)
                        {
                            lblInstallStatus.Text = "安装失败";
                            lblInstallStatus.ForeColor = Color.FromArgb(198, 40, 40);
                            InstallLog("错误：" + err);
                        }
                        else if (warned)
                        {
                            lblInstallStatus.Text = "安装完成（有警告）";
                            lblInstallStatus.ForeColor = Color.FromArgb(230, 126, 34);
                        }
                        else
                        {
                            lblInstallStatus.Text = "安装完成 ✓";
                            lblInstallStatus.ForeColor = Color.FromArgb(46, 125, 50);
                        }
                        btnBack.Enabled = false; // 安装完成后不允许回退
                        btnNext.Enabled = true;
                        btnCancel.Enabled = true;
                        progBar.Style = ProgressBarStyle.Continuous;
                        progBar.Value = ok ? 100 : 0;
                    });
                }
                catch { } // 窗体已关闭时静默丢弃
            });
            t.IsBackground = true;
            t.Start();
        }

        private void InstallLog(string s)
        {
            txtInstallLog.AppendText("[" + DateTime.Now.ToString("HH:mm:ss") + "] " + s + "\r\n");
            // 防无限增长：超过 20 万字符截掉最旧的一半
            if (txtInstallLog.TextLength > 200000)
            {
                int cut = txtInstallLog.Text.IndexOf('\n', 100000);
                if (cut > 0) { txtInstallLog.Select(0, cut + 1); txtInstallLog.SelectedText = ""; }
            }
            txtInstallLog.SelectionStart = txtInstallLog.TextLength;
            txtInstallLog.ScrollToCaret();
        }

        private void Finish()
        {
            if (!installDone) { Close(); return; } // 安装失败时不启动、不创建快捷方式
            string exe = Path.Combine(installDir, "DSHLauncher.exe");
            // 同时勾选"打开新手指引"和"立即启动"时：先以 --guide 启动（指引窗口含启动器功能），
            // 再额外启动一个普通实例，确保用户关闭指引后仍有运行中的启动器
            if (chkGuide.Checked)
            {
                try { Process.Start(exe, "--guide"); } catch { }
            }
            if (chkLaunch.Checked)
            {
                try { Process.Start(exe); } catch { }
            }
            if (chkShortcut.Checked)
            {
                // P1-5：快捷方式失败不再被吞掉——用户明确勾选了这一项，失败就要看得见
                if (!Installer.CreateShortcut(installDir, InstallLog))
                {
                    MessageBox.Show("桌面快捷方式创建失败（安装文件本身已就绪，可从安装目录直接启动）。",
                        Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning);
                }
            }
            Close();
        }
    }
}
