// DSHLauncherSetup.cs — DeepSeek Harness Launcher 一键安装包
// 功能: 环境检测(Node.js/dsh) → 缺失一键部署(winget→MSI→npm) → 快速安装
//       → 完成页(启动应用/创建桌面图标/新手指引)
// 命令行: --silent-install [目录]  静默安装(写 setup.log, 退出码 0=成功 1=失败 2=参数错误)
//         --detect-only            只检测环境(写 setup.detect.log)
// 构建: build-setup.ps1（内嵌 DSHLauncher.exe、app.ico、WebView2 三个运行库、
//       README.md 与维护手册 MAINTENANCE.zh.md / MAINTENANCE.en.md，单文件分发）
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
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Forms;
using Microsoft.Win32;

namespace DSHSetup
{
    internal static class Program
    {
        public const string AppName = "DeepSeek Harness Launcher";
        public const string AppVersion = "3.0.0";
        public const string InstallSubDir = "DSHLauncher";
        public const string ShortcutName = "DeepSeek Harness Launcher";

        [STAThread]
        private static int Main(string[] args)
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            if (args.Length > 0 && args[0] == "--silent-install")
            {
                string dir = (args.Length > 1 && args[1].Trim().Length > 0)
                    ? args[1].Trim() : DefaultInstallDir();
                try { dir = Path.GetFullPath(dir); }
                catch { Console.Error.WriteLine("无效的安装目录: " + dir); return 2; }
                // 拒绝安装到盘符根目录（卸载守卫同样拒绝删除，会留下垃圾）
                if (Regex.IsMatch(dir, @"^[A-Za-z]:\\?$"))
                {
                    Console.Error.WriteLine("拒绝安装到盘符根目录: " + dir);
                    return 2;
                }
                return SilentInstall.Run(dir);
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

        public static void Detect()
        {
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
                string dir = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
                    "Microsoft", "EdgeWebView", "Application");
                if (Directory.Exists(dir)
                    && Directory.GetFiles(dir, "msedgewebview2.exe", SearchOption.AllDirectories).Length > 0)
                {
                    return true;
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
                log("使用 winget 安装 Node.js LTS（可能弹出 UAC 授权，请允许）…");
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
            for (int attempt = 1; attempt <= 3; attempt++)
            {
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
                    return false;
                }
                log("正在静默安装（可能弹出 UAC 授权，请允许）…");
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
                    StringBuilder buf = new StringBuilder();
                    p.OutputDataReceived += delegate(object s, DataReceivedEventArgs e)
                    {
                        if (e.Data != null)
                        {
                            lock (buf) { buf.AppendLine(e.Data); }
                            if (log != null) log(e.Data);
                        }
                    };
                    p.ErrorDataReceived += delegate(object s, DataReceivedEventArgs e)
                    {
                        if (e.Data != null)
                        {
                            lock (buf) { buf.AppendLine(e.Data); }
                            if (log != null) log("[err] " + e.Data);
                        }
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
        public static int Run(string dir)
        {
            if (dir == null || dir.Trim().Length == 0) dir = Program.DefaultInstallDir();
            string logPath = Path.Combine(Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location), "setup.log");
            // 安装包位于只读目录时日志会静默丢失：先探测可写性，失败回退 %TEMP%
            try { File.AppendAllText(logPath, ""); }
            catch { logPath = Path.Combine(Path.GetTempPath(), "dshlauncher-setup.log"); }
            Action<string> log = delegate(string s)
            {
                string t = "[" + DateTime.Now.ToString("HH:mm:ss") + "] " + s;
                try { File.AppendAllText(logPath, t + "\r\n", new UTF8Encoding(true)); } catch { }
            };
            try
            {
                log("=== " + Program.AppName + " 静默安装 ===");
                log("目标目录: " + dir);
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
                Installer.Install(dir, log, true);
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
            string logPath = Path.Combine(Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location), "setup.detect.log");
            try
            {
                Env.Detect();
                StringBuilder sb = new StringBuilder();
                sb.AppendLine("node=" + (Env.NodeOk ? Env.NodePath : "MISSING"));
                sb.AppendLine("dsh=" + (Env.DshOk ? Env.BinJs : "MISSING"));
                sb.AppendLine("npm=" + (Env.NpmPath.Length > 0 ? Env.NpmPath : "MISSING"));
                sb.AppendLine("webview2=" + (Env.WebView2Ok ? "OK" : "MISSING"));
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
        public static void Install(string dir, Action<string> log, bool createShortcut)
        {
            StopLauncherInDir(dir, log); // 升级场景：先结束运行中的旧版，避免文件占用导致提取失败
            try
            {
                Directory.CreateDirectory(dir);
                ExtractResource("DSHLauncher.exe", Path.Combine(dir, "DSHLauncher.exe"));
                ExtractResource("app.ico", Path.Combine(dir, "app.ico"));
                // WebView2 内嵌模式运行库（内嵌进安装包，随安装释放）
                ExtractResource("Microsoft.Web.WebView2.Core.dll",
                    Path.Combine(dir, "Microsoft.Web.WebView2.Core.dll"));
                ExtractResource("Microsoft.Web.WebView2.WinForms.dll",
                    Path.Combine(dir, "Microsoft.Web.WebView2.WinForms.dll"));
                ExtractResource("WebView2Loader.dll", Path.Combine(dir, "WebView2Loader.dll"));
                // 随安装部署文档：README.md（简介）+ 升级与维护手册（中英双语）
                ExtractResource("README.md", Path.Combine(dir, "README.md"));
                ExtractResource("MAINTENANCE.zh.md", Path.Combine(dir, "MAINTENANCE.zh.md"));
                ExtractResource("MAINTENANCE.en.md", Path.Combine(dir, "MAINTENANCE.en.md"));
                WriteUninstallCmd(dir);
                RegisterUninstall(dir);
                if (createShortcut) CreateShortcut(dir);
                log("文件已复制到 " + dir);
            }
            catch (IOException ex)
            {
                throw new IOException("安装文件写入失败：" + ex.Message
                    + "。若提示“正由另一进程使用”，请先关闭正在运行的 " + Program.AppName + " 后重试。");
            }
            catch (Exception ex)
            {
                throw new Exception("安装失败：" + ex.Message);
            }
        }

        // 仅当目标目录里已有正在运行的旧版启动器时结束它（避免误杀其它位置运行的实例）。
        // 注意：只杀启动器本体，【不带 /T】——dsh web 服务与局域网网关是启动器的子进程，
        // 按进程树强杀会把正在服务的 Harness 会话一起杀掉（浏览器端立即断开），
        // 与 v2.0 起的“退出保留服务”语义冲突。
        private static void StopLauncherInDir(string dir, Action<string> log)
        {
            try
            {
                string target = Path.Combine(dir, "DSHLauncher.exe").ToLowerInvariant();
                foreach (Process p in Process.GetProcessesByName("DSHLauncher"))
                {
                    bool match = false;
                    try { match = p.MainModule.FileName.ToLowerInvariant() == target; }
                    catch { }
                    if (!match) { try { p.Dispose(); } catch { } continue; }
                    if (log != null)
                    {
                        log("检测到旧版 " + Program.AppName + " 正在运行（PID " + p.Id + "），已自动结束以便覆盖升级。");
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

        // 原子解压：先写 .tmp 再覆盖，中途失败（磁盘满/占用）不会留下半截损坏文件
        // 原子解压：先写 .tmp 再原子替换目标（File.Replace 同卷原子），
        // 中途失败（磁盘满/占用）不会留下半截文件；失败路径清理 .tmp
        private static void ExtractResource(string name, string outPath)
        {
            string tmp = outPath + ".tmp";
            try
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
                if (File.Exists(outPath))
                {
                    File.Replace(tmp, outPath, null); // 原子替换（目标存在时）
                }
                else
                {
                    File.Move(tmp, outPath);
                }
            }
            catch
            {
                try { if (File.Exists(tmp)) File.Delete(tmp); } catch { }
                throw;
            }
        }

        private static void WriteUninstallCmd(string dir)
        {
            // 无 BOM 写入（内容纯 ASCII）：cmd.exe 不识别 UTF-8 BOM，
            // 带 BOM 时首行会被解析为“ï»¿@echo off”报错且 @echo off 失效
            string p = dir.TrimEnd('\\');
            // 转义：单引号（PowerShell 字符串）、% （cmd 变量符，批处理中需写成 %%）
            string psEscape = p.Replace("'", "''").Replace("%", "%%");
            string bat =
                "@echo off\r\n" +
                "chcp 65001 >nul\r\n" +
                // 结束运行中的旧版启动器：仅限本安装目录，避免误杀其它位置运行的实例。
                // 卸载为彻底清理场景，用 /T 按进程树强杀（连同其启动的 dsh web / 网关），
                // 与升级安装（StopLauncherInDir 不带 /T、保留后台服务）语义不同，属有意设计
                "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \"" +
                "Get-Process DSHLauncher -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '" +
                psEscape + "\\*' } | ForEach-Object { & taskkill /PID $_.Id /T /F }\" >nul 2>&1\r\n" +
                "reg delete \"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\DSHLauncher\" /f >nul 2>&1\r\n" +
                "del \"%USERPROFILE%\\Desktop\\DSH Harness *.lnk\" >nul 2>&1\r\n" +
                "del \"%USERPROFILE%\\Desktop\\DeepSeek Harness Launcher*.lnk\" >nul 2>&1\r\n" +
                // 兼容 OneDrive 重定向的桌面（新旧两种快捷方式名都清掉）
                "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \"Get-ChildItem (Join-Path ([Environment]::GetFolderPath('Desktop')) '*.lnk') -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'DSH Harness *.lnk' -or $_.Name -like 'DeepSeek Harness Launcher*.lnk' } | Remove-Item -Force\" >nul 2>&1\r\n" +
                // 延迟删除安装目录（等本 cmd 退出避免占用）；带保护，拒绝删除盘符根目录/系统目录
                "start \"\" /min powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \"" +
                "Start-Sleep 2; $p='" + psEscape + "'; " +
                "if (-not ($p -match '^[A-Za-z]:\\\\?$') -and $p -ne $env:WINDIR -and $p -ne $env:USERPROFILE) " +
                "{ Remove-Item -LiteralPath $p -Recurse -Force }\"\r\n" +
                "exit\r\n";
            File.WriteAllText(Path.Combine(dir, "uninstall.cmd"), bat, new UTF8Encoding(false));
        }

        private static void RegisterUninstall(string dir)
        {
            if (dir == null || dir.Trim().Length == 0) return;
            try
            {
                // 用 RegistryKey API 直写（毫秒级、可感知失败），替代串行 8 次 reg.exe 子进程
                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(
                    @"Software\Microsoft\Windows\CurrentVersion\Uninstall\DSHLauncher"))
                {
                    if (key == null) return;
                    key.SetValue("DisplayName", Program.AppName);
                    key.SetValue("DisplayVersion", Program.AppVersion);
                    key.SetValue("Publisher", "KristoffersonLee");
                    key.SetValue("InstallLocation", dir);
                    key.SetValue("DisplayIcon", Path.Combine(dir, "DSHLauncher.exe"));
                    key.SetValue("UninstallString", "\"" + Path.Combine(dir, "uninstall.cmd") + "\"");
                    key.SetValue("NoModify", 1);
                    key.SetValue("NoRepair", 1);
                    // 估算大小（KB）：供“设置 → 应用”展示
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
            }
            catch { }
        }

        public static void CreateShortcut(string dir)
        {
            string desktop = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
            if (desktop.Length == 0) return;
            string lnk = Path.Combine(desktop, Program.ShortcutName + ".lnk");
            try
            {
                Type t = Type.GetTypeFromProgID("WScript.Shell");
                if (t == null) return;
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
            }
            catch { }
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
            lblHeader.Text = Program.AppName + " 一键安装包 v" + Program.AppVersion;
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
                "安装为当前用户，无需管理员权限。\n\n" +
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
                "· 安装为当前用户，无需管理员权限；\n" +
                "· 已安装过时选择同一目录即为升级，设置文件会自动保留；\n" +
                "· 安装内容：DSHLauncher.exe、WebView2 运行库、app.ico、README.md、维护手册（中英双语）、uninstall.cmd；\n" +
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
                "想卸载：运行安装目录里的 uninstall.cmd，或通过“设置→应用”卸载。\n" +
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
            btnBack.Enabled = !deploying && !(n == 3 && installDone);
            btnNext.Enabled = !deploying && !(n == 3 && !installDone && !installing);
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
                lblEnvHint.Text = "△ 缺少环境：点“一键部署缺失环境”自动安装，安装过程请看下方日志。";
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
            installing = true;
            btnBack.Enabled = false;
            btnNext.Enabled = false;
            btnCancel.Enabled = false;
            txtInstallLog.Clear();
            installDir = txtInstallDir.Text.Trim();
            if (installDir.Length == 0) installDir = Program.DefaultInstallDir();
            lblInstallStatus.Text = "正在安装到 " + installDir + " …";
            // 后台线程执行安装（资源解压 + 注册表写入可能耗时数秒）：
            // 原实现在 UI 线程同步执行导致窗口“未响应”、进度条冻结，用户易误判死机而强杀
            Thread t = new Thread(delegate()
            {
                bool ok = false;
                string err = "";
                try
                {
                    Installer.Install(installDir, delegate(string s)
                    {
                        if (InvokeRequired) BeginInvoke((Action)(delegate { InstallLog(s); }));
                        else InstallLog(s);
                    }, false);
                    ok = true;
                }
                catch (Exception ex) { err = ex.Message; }
                try
                {
                    BeginInvoke((Action)delegate
                    {
                        installing = false;
                        installDone = ok;
                        lblInstallStatus.Text = ok ? "安装完成 ✓" : "安装失败";
                        lblInstallStatus.ForeColor = ok ? Color.FromArgb(46, 125, 50) : Color.FromArgb(198, 40, 40);
                        if (!ok) InstallLog("错误：" + err);
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
            if (chkGuide.Checked)
            {
                try { Process.Start(exe, "--guide"); } catch { }
            }
            else if (chkLaunch.Checked)
            {
                try { Process.Start(exe); } catch { }
            }
            if (chkShortcut.Checked)
            {
                Installer.CreateShortcut(installDir);
            }
            Close();
        }
    }
}
