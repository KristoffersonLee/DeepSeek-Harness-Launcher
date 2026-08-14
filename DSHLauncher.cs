// DSHLauncher.cs — 一键启动 DeepSeek Harness 的 Windows 桌面启动器
// 编译要求: .NET Framework 4.x（Windows 10/11 自带），使用系统 csc.exe，无第三方依赖。
// 构建: 运行同目录 build.ps1
// 说明: 通过 node.exe 启动 dsh 的 web profile（bin.js web），轮询端口就绪后自动打开浏览器。
// 兼容 C# 5（系统自带 csc v4.0.30319 的语法级别），勿使用字符串插值 / ?. / out var 等新语法。

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Management;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;
using Microsoft.Win32;
using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.WinForms;

namespace DSHLauncher
{
    internal enum RunState
    {
        Stopped,
        Starting,
        Running,
        External, // 端口上已有外部实例在运行
        Error
    }

    internal static class Program
    {
        public const string AppName = "DeepSeek Harness Launcher";
        public const string AppVersion = "1.0.0";
        public static bool OpenGuideOnStart = false;

        [STAThread]
        private static int Main(string[] args)
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            // 隐藏自检模式: DSHLauncher.exe --selftest
            if (args.Length > 0 && (args[0] == "--selftest" || args[0] == "selftest"))
            {
                return SelfTest.Run();
            }
            foreach (string a in args)
            {
                if (a == "--guide" || a == "-g") OpenGuideOnStart = true;
            }

            bool createdNew;
            using (EventWaitHandle showEvt = new EventWaitHandle(false, EventResetMode.AutoReset,
                "DSHLauncher.SingleInstance", out createdNew))
            {
                if (!createdNew)
                {
                    // 已有实例在运行（可能藏在托盘），唤起它的窗口后退出
                    try { showEvt.Set(); }
                    catch { }
                    return 0;
                }
                LauncherForm form = new LauncherForm();
                Thread waiter = new Thread(delegate()
                {
                    try
                    {
                        while (showEvt.WaitOne())
                        {
                            form.BeginInvoke((Action)(delegate { form.ShowWindow(); }));
                        }
                    }
                    catch (Exception)
                    {
                        // 程序退出时事件句柄被释放，静默结束等待线程
                    }
                });
                waiter.IsBackground = true;
                waiter.Start();
                Application.Run(form);
            }
            return 0;
        }
    }

    // ---------------------------------------------------------------------
    // 设置（%APPDATA%\DSHLauncher\settings.ini）
    // ---------------------------------------------------------------------
    internal class Settings
    {
        public int Port = 3080;
        public bool AutoStart = true; // 打开本程序时自动启动服务
        public bool AutoOpen = true;  // 服务就绪后自动打开浏览器
        public bool TrayOnClose = true; // 点叉时最小化到托盘（后台运行）
        public bool LiteBrowser = true; // 用内嵌窗口（WebView2）打开界面，无需浏览器
        public string NodePath = "";  // 可选: 手动指定 node.exe
        public string WorkDir = "";   // dsh 进程工作目录（决定 Harness 的 workspace）

        private static string Dir
        {
            get { return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "DSHLauncher"); }
        }

        public static string FilePath
        {
            get { return Path.Combine(Dir, "settings.ini"); }
        }

        public Settings()
        {
            string d = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
            if (string.IsNullOrEmpty(d) || !Directory.Exists(d))
                d = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            WorkDir = d;
        }

        public void Load()
        {
            try
            {
                if (!File.Exists(FilePath)) return;
                foreach (string raw in File.ReadAllLines(FilePath, Encoding.UTF8))
                {
                    string line = raw.Trim();
                    if (line.Length == 0 || line.StartsWith("#") || line.StartsWith(";")) continue;
                    int eq = line.IndexOf('=');
                    if (eq < 0) continue;
                    string key = line.Substring(0, eq).Trim().ToLowerInvariant();
                    string val = line.Substring(eq + 1).Trim();
                    switch (key)
                    {
                        case "port":
                            int p;
                            if (int.TryParse(val, out p) && p >= 1 && p <= 65535) Port = p;
                            break;
                        case "autostart":
                            AutoStart = ParseBool(val, AutoStart);
                            break;
                        case "autoopen":
                            AutoOpen = ParseBool(val, AutoOpen);
                            break;
                        case "nodepath":
                            NodePath = val;
                            break;
                        case "workdir":
                            if (val.Length > 0) WorkDir = val;
                            break;
                        case "trayonclose":
                            TrayOnClose = ParseBool(val, TrayOnClose);
                            break;
                        case "litebrowser":
                            LiteBrowser = ParseBool(val, LiteBrowser);
                            break;
                    }
                }
            }
            catch (Exception ex)
            {
                Debug.WriteLine("settings load: " + ex.Message);
            }
        }

        private static bool ParseBool(string v, bool dflt)
        {
            bool r;
            if (bool.TryParse(v, out r)) return r;
            return dflt;
        }

        public void Save()
        {
            try
            {
                Directory.CreateDirectory(Dir);
                StringBuilder sb = new StringBuilder();
                sb.AppendLine("# " + Program.AppName + " settings");
                sb.AppendLine("port=" + Port);
                sb.AppendLine("autoStart=" + AutoStart.ToString().ToLowerInvariant());
                sb.AppendLine("autoOpen=" + AutoOpen.ToString().ToLowerInvariant());
                sb.AppendLine("nodePath=" + NodePath);
                sb.AppendLine("workDir=" + WorkDir);
                sb.AppendLine("trayOnClose=" + TrayOnClose.ToString().ToLowerInvariant());
                sb.AppendLine("liteBrowser=" + LiteBrowser.ToString().ToLowerInvariant());
                File.WriteAllText(FilePath, sb.ToString(), new UTF8Encoding(false));
            }
            catch (Exception ex)
            {
                Debug.WriteLine("settings save: " + ex.Message);
            }
        }
    }

    // ---------------------------------------------------------------------
    // 核心引擎：路径解析 / 进程启动 / 端口检测
    // ---------------------------------------------------------------------
    internal static class Engine
    {
        public static string NodePath;
        public static string BinJs;
        public static string DetectError = "";

        public static void Resolve(Settings s)
        {
            NodePath = null;
            BinJs = null;
            DetectError = "";

            // 1) 用户手动指定的 node.exe
            if (s.NodePath.Length > 0 && File.Exists(s.NodePath))
            {
                NodePath = s.NodePath;
            }

            // 2) PATH 上找 node.exe
            if (NodePath == null)
            {
                string pathVar = Environment.GetEnvironmentVariable("PATH") ?? "";
                foreach (string dir in pathVar.Split(';'))
                {
                    string d = dir.Trim().Trim('"');
                    if (d.Length == 0) continue;
                    string cand = Path.Combine(d, "node.exe");
                    if (File.Exists(cand)) { NodePath = cand; break; }
                }
            }

            // 3) 常见安装目录
            if (NodePath == null)
            {
                string la = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
                string p = Path.Combine(la, "nodejs");
                if (Directory.Exists(p))
                {
                    foreach (string d in Directory.GetDirectories(p, "node-v*"))
                    {
                        string cand = Path.Combine(d, "node.exe");
                        if (File.Exists(cand)) { NodePath = cand; break; }
                    }
                    if (NodePath == null)
                    {
                        string cand = Path.Combine(p, "node.exe");
                        if (File.Exists(cand)) NodePath = cand;
                    }
                }
            }
            if (NodePath == null)
            {
                string pf = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
                string cand = Path.Combine(pf, "nodejs", "node.exe");
                if (File.Exists(cand)) NodePath = cand;
            }

            if (NodePath == null)
            {
                DetectError = "未找到 node.exe。请在“Node…”中手动指定 node.exe 的完整路径。";
                return;
            }

            // 定位 dsh 的 bin.js
            string[] candidates = new string[]
            {
                Path.Combine(Path.GetDirectoryName(NodePath), "node_modules", "@deepseek-ai", "dsh", "lib", "bin.js"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "npm",
                    "node_modules", "@deepseek-ai", "dsh", "lib", "bin.js")
            };
            foreach (string c in candidates)
            {
                if (File.Exists(c)) { BinJs = c; break; }
            }
            if (BinJs == null)
            {
                DetectError = "在 " + Path.GetDirectoryName(NodePath) + " 下未找到 @deepseek-ai/dsh 的 bin.js。\n"
                    + "请检查 dsh 是否已安装（npm install -g @deepseek-ai/dsh）。";
            }
        }

        public static Process StartServer(int port, string workDir)
        {
            ProcessStartInfo psi = new ProcessStartInfo();
            psi.FileName = NodePath;
            psi.Arguments = "\"" + BinJs + "\" web --host 127.0.0.1 --port " + port;
            psi.UseShellExecute = false;
            psi.CreateNoWindow = true;
            psi.RedirectStandardOutput = true;
            psi.RedirectStandardError = true;
            if (Directory.Exists(workDir))
                psi.WorkingDirectory = workDir;
            else
                psi.WorkingDirectory = Path.GetDirectoryName(BinJs);
            return Process.Start(psi);
        }

        // 端口是否已被占用（Loopback 绑定探测）
        public static bool IsPortListening(int port)
        {
            TcpListener l = null;
            try
            {
                l = new TcpListener(IPAddress.Loopback, port);
                l.Start();
                l.Stop();
                return false;
            }
            catch (SocketException)
            {
                if (l != null) { try { l.Stop(); } catch { } }
                return true;
            }
            catch (Exception)
            {
                if (l != null) { try { l.Stop(); } catch { } }
                return false;
            }
        }

        // HTTP 探测：服务器是否已响应
        public static bool IsServerReady(int port)
        {
            try
            {
                HttpWebRequest req = (HttpWebRequest)WebRequest.Create("http://127.0.0.1:" + port + "/");
                req.Timeout = 800;
                req.ReadWriteTimeout = 800;
                req.AllowAutoRedirect = true;
                HttpWebResponse resp = (HttpWebResponse)req.GetResponse();
                resp.Close();
                return true;
            }
            catch (WebException wex)
            {
                if (wex.Response != null)
                {
                    wex.Response.Close();
                    return true; // 有响应（哪怕是错误页），说明服务已起来
                }
                return false;
            }
            catch (Exception)
            {
                return false;
            }
        }

        // 通过 netstat 找到监听指定端口的 PID
        public static int FindPidOnPort(int port)
        {
            try
            {
                string netstat = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "netstat.exe");
                if (!File.Exists(netstat)) return 0;
                ProcessStartInfo psi = new ProcessStartInfo(netstat, "-ano -p TCP");
                psi.UseShellExecute = false;
                psi.CreateNoWindow = true;
                psi.RedirectStandardOutput = true;
                psi.RedirectStandardError = true;
                using (Process p = Process.Start(psi))
                {
                    string outp = p.StandardOutput.ReadToEnd();
                    p.WaitForExit(5000);
                    string marker = ":" + port + " ";
                    foreach (string line in outp.Split('\n'))
                    {
                        string t = line.Trim();
                        if (t.IndexOf("LISTENING", StringComparison.OrdinalIgnoreCase) < 0) continue;
                        if (t.IndexOf(marker, StringComparison.Ordinal) < 0) continue;
                        string[] parts = t.Split(new char[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
                        if (parts.Length >= 5)
                        {
                            int pid;
                            if (int.TryParse(parts[parts.Length - 1], out pid)) return pid;
                        }
                    }
                }
            }
            catch (Exception)
            {
            }
            return 0;
        }

        // 通过 WMI 读取进程命令行（用于识别端口占用者身份）
        public static string GetCommandLine(int pid)
        {
            try
            {
                using (ManagementObjectSearcher searcher = new ManagementObjectSearcher(
                    "SELECT CommandLine FROM Win32_Process WHERE ProcessId=" + pid))
                {
                    foreach (ManagementObject obj in searcher.Get())
                    {
                        object cmd = obj["CommandLine"];
                        if (cmd != null) return cmd.ToString();
                    }
                }
            }
            catch (Exception)
            {
            }
            return "";
        }

        // 判断进程是否 DSH Harness（命令行含 @deepseek-ai\dsh，或 bin.js + web）
        public static bool IsDshHarness(int pid)
        {
            string cmd = GetCommandLine(pid);
            if (cmd.Length == 0) return false;
            string c = cmd.ToLowerInvariant();
            if (c.Contains("@deepseek-ai") && c.Contains("dsh")) return true;
            if (c.Contains("bin.js") && c.Contains(" web")) return true;
            return false;
        }

        // 结束进程树（依赖系统自带 taskkill /T /F；.NET 4.0 无 Kill(bool) 重载）
        public static void KillProcessTree(int pid)
        {
            try
            {
                string tk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "taskkill.exe");
                if (File.Exists(tk))
                {
                    ProcessStartInfo psi = new ProcessStartInfo(tk, "/PID " + pid + " /T /F");
                    psi.UseShellExecute = false;
                    psi.CreateNoWindow = true;
                    Process tp = Process.Start(psi);
                    if (tp != null)
                    {
                        tp.WaitForExit(10000);
                        tp.Dispose();
                    }
                    return;
                }
            }
            catch (Exception)
            {
            }
            try { Process.GetProcessById(pid).Kill(); }
            catch { }
        }

        public static void KillProcessTree(Process p)
        {
            if (p == null) return;
            try
            {
                if (!p.HasExited) KillProcessTree(p.Id);
            }
            catch (Exception)
            {
                try { p.Kill(); } catch { }
            }
            finally
            {
                try { p.Dispose(); } catch { }
            }
        }

        public static string Sanitize(string s)
        {
            if (string.IsNullOrEmpty(s)) return "";
            return Regex.Replace(s, "\u001b\\[[0-9;]*m", "");
        }
    }

    // ---------------------------------------------------------------------
    // 主窗体
    // ---------------------------------------------------------------------
    internal class LauncherForm : Form
    {
        internal Settings settings = new Settings();
        private Process server = null;
        private bool serverReady = false;
        private bool starting = false;
        private bool forceExit = false;
        private int externalPid = 0;   // 已接管的端口占用进程（已验证是 DSH Harness）
        private bool adoptTried = false; // 本次外部实例是否已尝试过识别

        private RunState state = RunState.Stopped;
        private DateTime serverStartTime;         // 服务进程启动时间（启动超时检测用）
        private DateTime lastProbe = DateTime.MinValue; // 上次就绪探测触发时间（限频）
        private int lostResponse = 0;             // 就绪后连续无响应探测次数
        private int autoRestartCount = 0;         // 挂起自动重启次数（超过上限则停止自动重启）
        private bool stuckHarness = false;        // 端口被无响应残留进程占用且已确认为 DSH Harness
        private volatile bool probeInFlight = false; // 异步就绪探测进行中
        private volatile bool probeReady = false;    // 异步就绪探测最近一次结果
        internal bool AppExiting = false;            // 程序真正退出中（用于内嵌窗口放行关闭）
        private HarnessWindow embedded = null;       // 内嵌 Harness 窗口（WebView2）
        private SettingsForm settingsForm = null;    // 设置窗口（替代原启动器面板）
        internal event Action<string> LogLine;       // 日志行事件（设置窗口实时显示）
        private readonly object logLock = new object();                    // 日志缓冲锁
        private System.Collections.Generic.List<string> logBuffer = new System.Collections.Generic.List<string>();
        private const int LogBufferMax = 800;                              // 内存日志缓冲上限（行，超出丢最旧）
        private bool exitConfirmed = false;          // 内嵌窗口关闭并退出（已确认）
        private bool exitKillService = true;         // 退出时是否同时停止服务

        private const int StartupTimeoutMs = 120000; // 启动超时（毫秒）：120 秒未就绪则停止并复位
        private const int ProbeIntervalMs = 1500;    // 就绪探测最小间隔（毫秒），防止 UI 线程频繁阻塞
        private const int LostResponseLimit = 20;    // 就绪后连续无响应判定挂起的次数（约 30 秒）
        private System.Windows.Forms.Timer pollTimer;
        private NotifyIcon tray;
        private ContextMenuStrip trayMenu;



        private int Port
        {
            get { return settings.Port; }
        }

        // ---------- 供设置窗口（SettingsForm）使用的内部接口 ----------
        internal string UiPortText { get { return settings.Port.ToString(); } }
        internal string UiWorkDir { get { return settings.WorkDir; } }
        internal bool UiTray { get { return settings.TrayOnClose; } }
        internal string UiLogText { get { lock (logLock) { return string.Join("\r\n", logBuffer); } } }

        // 内嵌窗口“关闭并退出”流程（TrayOnClose 未勾选时）：询问是否停止服务，然后真正退出
        internal void RequestAppExit()
        {
            if (exitConfirmed) return;
            bool serverRunning = server != null && !server.HasExited;
            bool extRunning = externalPid > 0;
            if (serverRunning || extRunning)
            {
                string names = (serverRunning ? "服务" : "")
                    + (serverRunning && extRunning ? "、外部实例" : (extRunning ? "外部实例" : ""));
                DialogResult r = MessageBox.Show(
                    "DSH Harness 正在运行（" + names + "）。\n\n是否同时停止？\n选择“否”则继续在后台运行。",
                    Program.AppName, MessageBoxButtons.YesNoCancel, MessageBoxIcon.Question);
                if (r == DialogResult.Cancel) return; // 取消：不退出
                exitKillService = (r == DialogResult.Yes);
            }
            exitConfirmed = true;
            Close(); // 触发 OnFormClosing 完成收尾
        }

        // 打开设置窗口（原启动器面板已隐藏，设置窗口是唯一的设置入口）
        internal void OpenSettings()
        {
            if (settingsForm == null || settingsForm.IsDisposed)
            {
                settingsForm = new SettingsForm(this);
            }
            settingsForm.ShowWindow();
        }

        // 设置窗口提交：校验并持久化，必要时重启服务
        internal void CommitSettings(string portText, string workDir, bool tray)
        {
            int p;
            if (!int.TryParse(portText, out p) || p < 1 || p > 65535)
            {
                MessageBox.Show("端口必须是 1–65535 之间的数字。", Program.AppName,
                    MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            bool portChanged = p != settings.Port;
            bool workChanged = workDir != settings.WorkDir;
            bool oursRunning = server != null && !server.HasExited;

            // 若端口变更但新端口已被占用，拒绝切换（避免服务被停掉又起不来）
            if (portChanged && oursRunning && Engine.IsPortListening(p))
            {
                MessageBox.Show("新端口 " + p + " 已被占用，无法切换。\n请先停止服务或换一个端口。",
                    Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }

            settings.Port = p;
            settings.TrayOnClose = tray;
            settings.WorkDir = workDir;
            settings.Save();

            if (oursRunning && (portChanged || workChanged))
            {
                Log("配置已变更（端口/工作目录），正在重启服务…");
                try { Engine.KillProcessTree(server); }
                catch { }
                server = null;
                serverReady = false;
                starting = false;
                StartServer();
            }
            else
            {
                Log("设置已保存（端口 " + p + "）。");
            }
            if (externalPid > 0 && portChanged)
            {
                Log("提示：外部实例仍在旧端口运行，如需停止请把端口切回后再操作。");
            }
        }

        public LauncherForm()
        {
            this.ShowInTaskbar = false; // 宿主永不出现在任务栏（内嵌窗口才是主界面）
            this.Text = Program.AppName;
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }
            settings.Load();

            pollTimer = new System.Windows.Forms.Timer();
            pollTimer.Interval = 700;
            pollTimer.Tick += PollTick;
            pollTimer.Start();

            BuildTray();
            Log(Program.AppName + " 就绪。");
            Log("服务地址: http://127.0.0.1:" + Port + "/");
        }

        // ------------------------- UI 构建 -------------------------


        private void BuildTray()
        {
            tray = new NotifyIcon();
            tray.Icon = this.Icon;
            tray.Text = Program.AppName;
            trayMenu = new ContextMenuStrip();
            trayMenu.Items.Add("打开界面", null, delegate { OpenBrowser(); });
            trayMenu.Items.Add("刷新界面", null, delegate { RefreshEmbedded(); });
            trayMenu.Items.Add("在浏览器中打开界面", null, delegate { OpenInDefaultBrowser(); });
            trayMenu.Items.Add("启动服务", null, delegate { StartServer(true); });
            trayMenu.Items.Add("停止服务", null, OnStopClick);
            trayMenu.Items.Add("新手指引", null, delegate { GuideForm.ShowGuide(null); });
            trayMenu.Items.Add("打开日志目录", null, delegate { OpenLogDir(); });
            trayMenu.Items.Add(new ToolStripSeparator());
            trayMenu.Items.Add("设置…", null, delegate { OpenSettings(); });
            trayMenu.Items.Add(new ToolStripSeparator());
            trayMenu.Items.Add("关于", null, delegate { ShowAbout(); });
            trayMenu.Items.Add("退出", null, delegate { forceExit = true; Close(); });
            tray.ContextMenuStrip = trayMenu;
            tray.DoubleClick += delegate { ShowWindow(); };
            tray.Visible = true;
        }

        internal void ShowAbout()
        {
            MessageBox.Show(
                Program.AppName + " v" + Program.AppVersion + "\n\n"
                + "作者: KristoffersonLee\n\n"
                + "启动 / 停止 / 接管外部实例 / 后台运行 / 内嵌窗口（无需浏览器）\n\n"
                + "构建: .NET Framework 4.x（系统自带 csc）\n"
                + "内嵌引擎: Microsoft WebView2\n"
                + "图标: 官方 DeepSeek 鲸鱼 LOGO",
                "关于", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        // 托盘气泡提示
        internal void TrayBalloon(string msg)
        {
            try { tray.ShowBalloonTip(1500, Program.AppName, msg, ToolTipIcon.Info); } catch { }
        }

        // 打开日志目录（%LOCALAPPDATA%\DSHLauncher\logs）
        internal void OpenLogDir()
        {
            try
            {
                string dir = Path.GetDirectoryName(LogFilePath);
                Directory.CreateDirectory(dir);
                Process.Start("explorer.exe", "\"" + dir + "\"");
            }
            catch (Exception ex)
            {
                Log("打开日志目录失败：" + ex.Message);
            }
        }

        private static string LogFilePath
        {
            get
            {
                return Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "DSHLauncher", "logs", "launcher.log");
            }
        }

        // 日志同时写入文件（供“打开日志目录”查看），超过 2MB 自动裁剪
        private void AppendLogFile(string line)
        {
            try
            {
                string p = LogFilePath;
                Directory.CreateDirectory(Path.GetDirectoryName(p));
                File.AppendAllText(p, line + "\r\n", Encoding.UTF8);
                FileInfo fi = new FileInfo(p);
                if (fi.Length > 2 * 1024 * 1024)
                {
                    string all = File.ReadAllText(p, Encoding.UTF8);
                    if (all.Length > 1000000)
                    {
                        File.WriteAllText(p, all.Substring(all.Length - 1000000), Encoding.UTF8);
                    }
                }
            }
            catch { }
        }

        // 启动器面板永不显示，此方法保留为空实现（调用方无需改动）
        private void HideLauncherOnOpen()
        {
        }

        public void ShowWindow()
        {
            // 唤起主界面（内嵌窗口）；未打开时退而打开设置
            if (embedded != null && !embedded.IsDisposed)
            {
                embedded.ShowWindow();
            }
            else
            {
                OpenSettings();
            }
        }

        // ------------------------- 日志与状态 -------------------------
        internal void Log(string line)
        {
            if (InvokeRequired)
            {
                try { BeginInvoke((Action)(delegate { Log(line); })); }
                catch { } // 窗体已关闭时静默丢弃
                return;
            }
            try
            {
                string tsLine = "[" + DateTime.Now.ToString("HH:mm:ss") + "] " + line;
                AppendLogFile(tsLine);
                // 有界环形缓冲（设置窗口用），上限 800 行，超出丢最旧
                lock (logLock)
                {
                    logBuffer.Add(tsLine);
                    if (logBuffer.Count > LogBufferMax) logBuffer.RemoveAt(0);
                }
                // 设置窗口实时显示日志
                if (LogLine != null)
                {
                    try { LogLine(tsLine); } catch { }
                }
            }
            catch { } // 控件已释放时静默丢弃
        }

        private void OnServerOutput(object sender, DataReceivedEventArgs e)
        {
            if (!string.IsNullOrEmpty(e.Data)) Log("  " + Engine.Sanitize(e.Data));
        }

        // 状态文本（供托盘悬浮文字使用）
        internal string StatusText
        {
            get
            {
                switch (state)
                {
                    case RunState.Running: return "运行中";
                    case RunState.Starting: return "启动中…";
                    case RunState.External: return externalPid > 0 ? "运行中（已接管）" : "运行中（外部实例）";
                    case RunState.Error: return "出错";
                    default: return "未运行";
                }
            }
        }

        private void SetState(RunState st)
        {
            state = st;
            try { tray.Text = Program.AppName + " · " + StatusText; } catch { }
        }



        // ------------------------- 轮询 -------------------------
        // HTTP 就绪探测限频：避免服务无响应时 UI 线程被同步超时反复阻塞（表现为界面卡死）
        private bool ProbeDue()
        {
            return (DateTime.Now - lastProbe).TotalMilliseconds >= ProbeIntervalMs;
        }

        // 后台线程执行 HTTP 探测，结果写入 probeReady；UI 线程不再被阻塞
        private void ProbeAsync(int port)
        {
            if (probeInFlight) return;
            probeInFlight = true;
            try
            {
                ThreadPool.QueueUserWorkItem(delegate
                {
                    bool ok = false;
                    try { ok = Engine.IsServerReady(port); }
                    catch { }
                    probeReady = ok;
                    probeInFlight = false;
                });
            }
            catch
            {
                probeInFlight = false;
            }
        }

        private void PollTick(object sender, EventArgs e)
        {
            int port = Port;
            if (server != null)
            {
                bool exited = false;
                try { exited = server.HasExited; }
                catch { exited = true; }
                if (exited)
                {
                    int code = -1;
                    try { code = server.ExitCode; } catch { }
                    Log("服务进程已退出（退出码 " + code + "）。");
                    server = null;
                    serverReady = false;
                    starting = false;
                    SetState(RunState.Stopped);
                    RevealLauncherOnError();
                    if (!Visible)
                    {
                        try
                        {
                            tray.ShowBalloonTip(2000, Program.AppName,
                                "服务已退出（退出码 " + code + "）。双击托盘图标查看。", ToolTipIcon.Warning);
                        }
                        catch { }
                    }
                    return;
                }
                // ---- 启动中：就绪探测（后台线程执行，UI 不卡顿） ----
                if (!serverReady)
                {
                    if (ProbeDue())
                    {
                        lastProbe = DateTime.Now;
                        ProbeAsync(port);
                        if (probeReady)
                        {
                            starting = false; // 关键修复：就绪后必须复位 starting，否则“打开界面”永远提示“服务正在启动”
                            serverReady = true;
                            lostResponse = 0;
                            autoRestartCount = 0;
                            Log("服务就绪 ✓ http://127.0.0.1:" + port + "/");
                            SetState(RunState.Running);
                            OpenBrowser(); // 固定：就绪后自动打开界面
                        }
                        else if ((DateTime.Now - serverStartTime).TotalMilliseconds > StartupTimeoutMs)
                        {
                            // 进程存活但长时间未就绪：结束“无限启动中”的卡死状态
                            Log("启动超时：120 秒内服务未就绪，已停止并复位。请查看上方日志输出后重试。");
                            try { Engine.KillProcessTree(server); } catch { }
                            server = null;
                            serverReady = false;
                            starting = false;
                            lostResponse = 0;
                            SetState(RunState.Stopped);
                            RevealLauncherOnError();
                        }
                    }
                    return;
                }

                // ---- 运行中：挂起检测（连续无响应则自动重启） ----
                if (ProbeDue())
                {
                    lastProbe = DateTime.Now;
                    ProbeAsync(port);
                    if (probeReady)
                    {
                        lostResponse = 0;
                    }
                    else
                    {
                        lostResponse++;
                        if (lostResponse == 1)
                        {
                            Log("警告：服务 HTTP 无响应，将连续检测约 30 秒，仍无响应则自动重启。");
                        }
                        if (lostResponse >= LostResponseLimit)
                        {
                            if (autoRestartCount < 3)
                            {
                                autoRestartCount++;
                                Log("服务连续无响应约 30 秒，判定已挂起，正在自动重启（第 " + autoRestartCount + " 次）…");
                                try { Engine.KillProcessTree(server); } catch { }
                                server = null;
                                serverReady = false;
                                starting = false;
                                lostResponse = 0;
                                SetState(RunState.Stopped);
                                StartServer();
                            }
                            else
                            {
                                Log("服务连续挂起且已自动重启 3 次仍未恢复，请检查工作目录/端口设置后点击“一键启动”重试。");
                                try { Engine.KillProcessTree(server); } catch { }
                                server = null;
                                serverReady = false;
                                starting = false;
                                lostResponse = 0;
                                autoRestartCount = 0;
                                SetState(RunState.Error);
                            }
                        }
                    }
                }
                return;
            }
            if (!starting)
            {
                bool listening = Engine.IsPortListening(port);
                if (!listening)
                {
                    externalPid = 0;
                    adoptTried = false;
                    stuckHarness = false;
                    SetState(RunState.Stopped);
                    return;
                }
                // 端口被占用：后台探测其是否就绪（限频，UI 不卡顿）
                if (!ProbeDue()) return;
                lastProbe = DateTime.Now;
                ProbeAsync(port);
                if (probeReady)
                {
                    SetState(RunState.External);
                    if (externalPid == 0 && !adoptTried)
                    {
                        // 只尝试识别一次：确认占用者是 DSH Harness 后接管，停止时可直接关闭
                        adoptTried = true;
                        int pid = Engine.FindPidOnPort(port);
                        bool isHarness = pid > 0 && pid != Process.GetCurrentProcess().Id && Engine.IsDshHarness(pid);
                        if (isHarness)
                        {
                            externalPid = pid;
                            Log("已接管端口 " + port + " 上的 DSH Harness（PID " + pid + "），点击“停止”可直接关闭。");
                        }
                        else
                        {
                            Log("端口 " + port + " 上的进程无法确认是 DSH Harness，停止前需要人工确认。");
                        }
                    }
                }
                else
                {
                    // 端口被占用但 HTTP 无响应：残留进程卡死场景，识别一次并给出可恢复提示
                    externalPid = 0;
                    if (!adoptTried)
                    {
                        adoptTried = true;
                        int pid = Engine.FindPidOnPort(port);
                        bool isHarness = pid > 0 && pid != Process.GetCurrentProcess().Id && Engine.IsDshHarness(pid);
                        stuckHarness = isHarness;
                        if (isHarness)
                        {
                            Log("检测到端口 " + port + " 上有一个无响应的残留 DSH Harness（PID " + pid + "）。");
                            Log("点击“一键启动”将自动结束该残留进程并重新启动服务。");
                        }
                        else
                        {
                            Log("端口 " + port + " 被其他程序占用且无响应。请关闭占用程序或更换端口。");
                        }
                    }
                    SetState(stuckHarness ? RunState.Error : RunState.Stopped);
                }
            }
        }

        // ------------------------- 操作 -------------------------
        private void OnStartClick(object sender, EventArgs e)
        {
            StartServer(true);
        }

        private void StartServer()
        {
            StartServer(false);
        }

        private void StartServer(bool userInitiated)
        {
            int port = Port;
            // 清理“进程已退出但轮询尚未发现”的陈旧状态，避免误判仍在运行而卡住
            if (server != null)
            {
                bool exited = false;
                try { exited = server.HasExited; }
                catch { exited = true; }
                if (exited)
                {
                    server = null;
                    serverReady = false;
                    starting = false;
                    lostResponse = 0;
                }
            }
            if (server != null)
            {
                if (userInitiated) Log("服务已在运行，直接打开界面。");
                OpenBrowser(); // 固定：直接打开界面
                return;
            }
            if (starting) return;

            if (Engine.IsPortListening(port))
            {
                if (Engine.IsServerReady(port))
                {
                    // 端口上有可用的服务：核验身份后接管（避免误报“DSH Harness”）
                    SetState(RunState.External);
                    int pid = Engine.FindPidOnPort(port);
                    bool isHarness = pid > 0 && pid != Process.GetCurrentProcess().Id && Engine.IsDshHarness(pid);
                    externalPid = isHarness ? pid : 0;
                    adoptTried = true;
                    Log("端口 " + port + " 上已有服务在运行（"
                        + (isHarness ? "DSH Harness，已接管" : "非 DSH Harness，未接管") + "），直接打开界面。");
                    OpenBrowser();
                    return;
                }

                // 端口被占用但无响应：若是残留卡死的 DSH Harness，自动清理后继续向下启动新服务
                int stuckPid = Engine.FindPidOnPort(port);
                bool stuckIsHarness = stuckPid > 0
                    && stuckPid != Process.GetCurrentProcess().Id
                    && Engine.IsDshHarness(stuckPid);
                if (stuckIsHarness)
                {
                    Log("端口 " + port + " 上有无响应的残留 DSH Harness（PID " + stuckPid + "），正在自动清理并重新启动…");
                    try { Engine.KillProcessTree(stuckPid); }
                    catch (Exception ex) { Log("清理残留进程失败：" + ex.Message); }
                    DateTime deadline = DateTime.Now.AddSeconds(3);
                    while (Engine.IsPortListening(port) && DateTime.Now < deadline) Thread.Sleep(150);
                    if (Engine.IsPortListening(port))
                    {
                        SetState(RunState.Error);
                        Log("端口 " + port + " 仍被占用，无法启动。请手动结束占用进程（PID " + stuckPid + "）后重试。");
                        return;
                    }
                    // 端口已释放，继续往下启动新服务
                }
                else
                {
                    SetState(RunState.Error);
                    Log("端口 " + port + " 已被其他程序占用但无响应，无法启动。请更换端口或关闭占用程序。");
                    return;
                }
            }

            Engine.Resolve(settings);
            if (Engine.NodePath == null || Engine.BinJs == null)
            {
                Log("无法启动：" + Engine.DetectError.Replace("\n", " "));
                SetState(RunState.Error);
                RevealLauncherOnError();
                return;
            }

            try
            {
                server = Engine.StartServer(port, settings.WorkDir);
                starting = true;
                serverReady = false;
                probeReady = false; // 复位就绪探测结果，避免陈旧值误判新服务已就绪
                externalPid = 0;
                adoptTried = false;
                stuckHarness = false;
                lostResponse = 0;
                if (userInitiated) autoRestartCount = 0; // 手动启动时重置自动重启计数
                serverStartTime = DateTime.Now;
                lastProbe = DateTime.MinValue; // 让下一次轮询立即触发第一次就绪探测
                server.OutputDataReceived += OnServerOutput;
                server.ErrorDataReceived += OnServerError;
                server.BeginOutputReadLine();
                server.BeginErrorReadLine();
                Log("正在启动 dsh web（PID " + server.Id + "）…");
                Log("  地址: http://127.0.0.1:" + port + "/");
                Log("  工作目录: " + settings.WorkDir);
                SetState(RunState.Starting);
            }
            catch (Exception ex)
            {
                server = null;
                starting = false;
                Log("启动失败：" + ex.Message);
                SetState(RunState.Error);
                RevealLauncherOnError();
            }
        }

        private void OnServerError(object sender, DataReceivedEventArgs e)
        {
            if (!string.IsNullOrEmpty(e.Data)) Log("  [err] " + Engine.Sanitize(e.Data));
        }

        private void OnStopClick(object sender, EventArgs e)
        {
            // 清理“进程已退出但轮询尚未发现”的陈旧状态
            if (server != null)
            {
                bool exited = false;
                try { exited = server.HasExited; }
                catch { exited = true; }
                if (exited)
                {
                    server = null;
                    serverReady = false;
                    starting = false;
                }
            }
            if (server != null)
            {
                Log("正在停止服务（PID " + server.Id + "）…");
                try
                {
                    Engine.KillProcessTree(server);
                }
                catch (Exception ex)
                {
                    Log("停止时出错：" + ex.Message);
                }
                Log("服务已停止。");
                server = null;
                serverReady = false;
                starting = false;
                SetState(RunState.Stopped);
                return;
            }

            int port = Port;
            if (externalPid > 0)
            {
                // 已接管的 DSH Harness（身份已验证），直接关闭，无需确认
                Log("正在停止已接管的 DSH Harness（PID " + externalPid + "）…");
                try
                {
                    Engine.KillProcessTree(externalPid);
                    Log("已停止（PID " + externalPid + "）。");
                }
                catch (Exception ex)
                {
                    Log("停止时出错：" + ex.Message);
                }
                externalPid = 0;
                adoptTried = false;
                SetState(RunState.Stopped);
                return;
            }
            int pid = Engine.FindPidOnPort(port);
            if (pid > 0 && pid != Process.GetCurrentProcess().Id)
            {
                DialogResult r = MessageBox.Show(
                    "检测到外部进程 PID " + pid + " 占用了端口 " + port + "。\n\n是否结束该进程？\n（仅当确认它是 DSH Harness 时选择“是”）",
                    "停止外部实例", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
                if (r == DialogResult.Yes)
                {
                    try
                    {
                        Engine.KillProcessTree(pid);
                        Log("已结束外部进程 PID " + pid + "。");
                        SetState(RunState.Stopped);
                    }
                    catch (Exception ex)
                    {
                        Log("无法结束外部进程：" + ex.Message);
                    }
                }
                return;
            }
            Log("没有正在运行的服务。");
        }

        private void OpenBrowser()
        {
            int port = Port;
            if (starting && !Engine.IsServerReady(port))
            {
                Log("服务正在启动，请稍候再打开界面…");
                return;
            }
            bool running = (server != null && !server.HasExited) || Engine.IsServerReady(port);
            if (!running)
            {
                Log("服务未运行，请先点击“一键启动”。");
                return;
            }
            string url = "http://127.0.0.1:" + port + "/";
            try
            {
                // 固定内嵌模式：WebView2 显示 Harness，无需浏览器（不可用时自动回退 Edge）
                if (embedded == null || embedded.IsDisposed)
                {
                    try
                    {
                        embedded = new HarnessWindow(url, this);
                    }
                    catch (Exception ex)
                    {
                        Log("内嵌窗口初始化失败（" + ex.Message + "），改用 Edge 精简窗口打开。");
                        OpenEdgeFallback(url);
                        return;
                    }
                }
                embedded.NavigateTo(url);
                embedded.ShowWindow();
                HideLauncherOnOpen();
            }
            catch (Exception ex)
            {
                Log("打开界面失败：" + ex.Message);
            }
        }

        // Edge 精简窗口回退（app 模式 + 独立用户目录 + 全套省内存参数）
        internal void OpenEdgeFallback(string url)
        {
            try
            {
                string edge = FindEdgePath();
                if (edge.Length > 0)
                {
                    string profile = Path.Combine(
                        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                        "DSHLauncher", "edge-profile");
                    StringBuilder args = new StringBuilder();
                    args.Append("--app=\"" + url + "\"");
                    args.Append(" --no-first-run --no-default-browser-check");
                    args.Append(" --disable-extensions --disable-background-networking");
                    args.Append(" --disable-component-update --disable-sync --disable-breakpad");
                    args.Append(" --disable-background-mode --disable-gpu");
                    args.Append(" --user-data-dir=\"" + profile + "\"");
                    args.Append(" --disable-features=msEdgeSidebarV2,msEdgeShoppingAssistant,msEdgeTranslate");
                    Process.Start(edge, args.ToString());
                    Log("已在精简窗口（Edge app 模式）中打开 " + url);
                    LogEdgeMemory(profile);
                    HideLauncherOnOpen();
                    return;
                }
                Log("未找到 Edge，改用默认浏览器打开。");
                Process.Start(url);
                Log("已在浏览器中打开 " + url);
                HideLauncherOnOpen();
            }
            catch (Exception ex)
            {
                Log("打开浏览器失败：" + ex.Message);
            }
        }

        // 统计精简窗口进程树的内存（按独立用户目录过滤，一次 WMI 查询取命令行）
        private void LogEdgeMemory(string profile)
        {
            try
            {
                Dictionary<int, string> cmds = new Dictionary<int, string>();
                using (ManagementObjectSearcher searcher = new ManagementObjectSearcher(
                    "SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name='msedge.exe'"))
                {
                    foreach (ManagementObject obj in searcher.Get())
                    {
                        object id = obj["ProcessId"];
                        object cmd = obj["CommandLine"];
                        if (id != null && cmd != null)
                        {
                            cmds[Convert.ToInt32(id)] = cmd.ToString();
                        }
                    }
                }
                long ws = 0, priv = 0;
                int n = 0;
                foreach (Process p in Process.GetProcessesByName("msedge"))
                {
                    string c;
                    if (cmds.TryGetValue(p.Id, out c)
                        && c.IndexOf(profile, StringComparison.OrdinalIgnoreCase) >= 0)
                    {
                        n++;
                        try { ws += p.WorkingSet64; } catch { }
                        try { priv += p.PrivateMemorySize64; } catch { }
                    }
                    try { p.Dispose(); } catch { }
                }
                if (n > 0)
                {
                    Log("精简窗口内存：" + n + " 个进程 / 工作集 " + (ws / (1024 * 1024))
                        + " MB / 专用 " + (priv / (1024 * 1024))
                        + " MB（任务管理器默认列显示的是工作集）。");
                }
            }
            catch { }
        }

        // 定位 msedge.exe（标准安装目录 + 注册表 App Paths）
        // 刷新内嵌窗口（托盘菜单）
        private void RefreshEmbedded()
        {
            if (embedded != null && !embedded.IsDisposed)
            {
                embedded.ReloadPage();
                Log("已刷新内嵌窗口。");
            }
            else
            {
                Log("内嵌窗口未打开，请先点“打开界面”。");
            }
        }

        // 在系统默认浏览器中打开界面（托盘菜单）
        private void OpenInDefaultBrowser()
        {
            try
            {
                Process.Start("http://127.0.0.1:" + Port + "/");
                Log("已在默认浏览器中打开界面。");
            }
            catch (Exception ex)
            {
                Log("打开浏览器失败：" + ex.Message);
            }
        }

        private static string FindEdgePath()
        {
            string[] candidates = new string[]
            {
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
                    "Microsoft", "Edge", "Application", "msedge.exe"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                    "Microsoft", "Edge", "Application", "msedge.exe")
            };
            foreach (string c in candidates)
            {
                if (File.Exists(c)) return c;
            }
            try
            {
                object v = Registry.GetValue(
                    @"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
                    "", null);
                if (v != null)
                {
                    string s = v.ToString();
                    if (s.Length > 0 && File.Exists(s)) return s;
                }
            }
            catch { }
            return "";
        }



        // ------------------------- 生命周期 -------------------------
        protected override void OnShown(EventArgs e)
        {
            base.OnShown(e);
            // 启动器面板永不显示：立即隐藏，完全退居托盘/内嵌界面
            Hide();
            if (Program.OpenGuideOnStart)
            {
                GuideForm.ShowGuide(this);
            }
            // 固定：打开程序即自动启动服务
            System.Windows.Forms.Timer t = new System.Windows.Forms.Timer();
            t.Interval = 600;
            t.Tick += delegate
            {
                t.Stop();
                StartServer();
            };
            t.Start();
        }

        // 启动失败时打开设置窗口（内含日志），让用户看到错误原因
        private void RevealLauncherOnError()
        {
            try
            {
                OpenSettings();
                tray.ShowBalloonTip(2000, Program.AppName,
                    "服务启动出现问题，请查看设置窗口日志。", ToolTipIcon.Warning);
            }
            catch { }
        }

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            pollTimer.Stop();
            bool userClose = e.CloseReason == CloseReason.UserClosing
                || e.CloseReason == CloseReason.TaskManagerClosing;

            // 内嵌窗口“关闭并退出”（TrayOnClose 未勾选）：询问已在 RequestAppExit 完成
            if (exitConfirmed)
            {
                bool srvRun = server != null && !server.HasExited;
                bool extRun = externalPid > 0;
                if (exitKillService)
                {
                    if (srvRun) { try { Engine.KillProcessTree(server); } catch { } }
                    if (extRun) { try { Engine.KillProcessTree(externalPid); } catch { } }
                    Log("已停止。");
                }
                else
                {
                    Log("服务保持后台运行，下次打开本程序会自动识别并接管。");
                }
                tray.Visible = false;
                AppExiting = true;
                try { if (embedded != null && !embedded.IsDisposed) embedded.Close(); } catch { }
                base.OnFormClosing(e);
                return;
            }

            if (!forceExit && userClose && settings.TrayOnClose)
            {
                // 点叉 → 隐藏到托盘，程序与服务继续在后台运行
                e.Cancel = true;
                pollTimer.Start();
                Hide();
                try
                {
                    tray.ShowBalloonTip(1500, Program.AppName,
                        "已最小化到托盘，服务继续在后台运行。", ToolTipIcon.Info);
                }
                catch { }
                return;
            }
            bool serverRunning = server != null && !server.HasExited;
            bool extRunning = externalPid > 0;
            bool killedAny = false;
            if (!forceExit && userClose && (serverRunning || extRunning))
            {
                string names = (serverRunning ? "服务" : "")
                    + (serverRunning && extRunning ? "、外部实例" : (extRunning ? "外部实例" : ""));
                DialogResult r = MessageBox.Show(
                    "DSH Harness 正在运行（" + names + "）。\n\n是否同时停止？\n选择“否”则继续在后台运行。",
                    Program.AppName, MessageBoxButtons.YesNoCancel, MessageBoxIcon.Question);
                if (r == DialogResult.Cancel) { e.Cancel = true; pollTimer.Start(); return; }
                if (r == DialogResult.Yes)
                {
                    if (serverRunning) { try { Engine.KillProcessTree(server); killedAny = true; } catch { } }
                    if (extRunning) { try { Engine.KillProcessTree(externalPid); killedAny = true; } catch { } }
                    Log("已停止。");
                }
                else
                {
                    Log("服务保持后台运行，下次打开本程序会自动识别并接管。");
                }
            }
            else if (forceExit && (serverRunning || extRunning))
            {
                if (serverRunning) { try { Engine.KillProcessTree(server); killedAny = true; } catch { } }
                if (extRunning) { try { Engine.KillProcessTree(externalPid); killedAny = true; } catch { } }
            }
            // 退出前短暂等待端口释放，避免立刻重新启动时提示“端口被占用”
            if (killedAny)
            {
                DateTime dl = DateTime.Now.AddSeconds(2.5);
                int p = Port;
                while (Engine.IsPortListening(p) && DateTime.Now < dl) Thread.Sleep(120);
            }
            tray.Visible = false;
            AppExiting = true;
            try { if (embedded != null && !embedded.IsDisposed) embedded.Close(); } catch { }
            base.OnFormClosing(e);
        }

        protected override void OnFormClosed(FormClosedEventArgs e)
        {
            try { tray.Dispose(); } catch { }
            KillWebView2Processes();
            base.OnFormClosed(e);
        }

        // 结束本程序启动的 WebView2 进程（按独立用户目录过滤，不影响其它应用）
        private static void KillWebView2Processes()
        {
            try
            {
                Dictionary<int, string> cmds = new Dictionary<int, string>();
                using (ManagementObjectSearcher searcher = new ManagementObjectSearcher(
                    "SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name='msedgewebview2.exe'"))
                {
                    foreach (ManagementObject obj in searcher.Get())
                    {
                        object id = obj["ProcessId"];
                        object cmd = obj["CommandLine"];
                        if (id != null && cmd != null) cmds[Convert.ToInt32(id)] = cmd.ToString();
                    }
                }
                foreach (Process p in Process.GetProcessesByName("msedgewebview2"))
                {
                    string c;
                    if (cmds.TryGetValue(p.Id, out c)
                        && c.IndexOf("webview2-profile", StringComparison.OrdinalIgnoreCase) >= 0)
                    {
                        try { Engine.KillProcessTree(p.Id); } catch { }
                    }
                    try { p.Dispose(); } catch { }
                }
            }
            catch { }
        }
    }

    // ---------------------------------------------------------------------
    // 设置窗口（替代原启动器面板；从内嵌窗口菜单/托盘打开）
    // ---------------------------------------------------------------------
    internal class SettingsForm : Form
    {
        private LauncherForm host;
        private TextBox txtPort, txtWork;
        private Button btnBrowseWork, btnSave;
        private CheckBox chkTray;
        private TextBox txtLog;
        private Button btnDoc, btnLogDir, btnAbout, btnClose;

        public SettingsForm(LauncherForm host)
        {
            this.host = host;
            this.ShowInTaskbar = false;
            this.Text = Program.AppName + " 设置";
            this.ClientSize = new Size(560, 500);
            this.MinimumSize = new Size(520, 440);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.Font = new Font("Microsoft YaHei UI", 9f);
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }

            // 端口
            Label lPort = new Label();
            lPort.SetBounds(16, 20, 48, 24);
            lPort.Text = "端口:";
            lPort.TextAlign = ContentAlignment.MiddleLeft;

            txtPort = new TextBox();
            txtPort.SetBounds(64, 20, 70, 26);
            txtPort.Text = host.UiPortText;

            // 工作目录
            Label lWork = new Label();
            lWork.SetBounds(16, 56, 66, 24);
            lWork.Text = "工作目录:";
            lWork.TextAlign = ContentAlignment.MiddleLeft;

            txtWork = new TextBox();
            txtWork.SetBounds(82, 56, 360, 26);
            txtWork.Text = host.UiWorkDir;

            btnBrowseWork = new Button();
            btnBrowseWork.SetBounds(448, 56, 92, 26);
            btnBrowseWork.Text = "浏览…";
            btnBrowseWork.Click += delegate
            {
                FolderBrowserDialog dlg = new FolderBrowserDialog();
                dlg.Description = "选择 dsh 的工作目录（Harness 会话所在目录）";
                if (Directory.Exists(txtWork.Text.Trim())) dlg.SelectedPath = txtWork.Text.Trim();
                if (dlg.ShowDialog(this) == DialogResult.OK) txtWork.Text = dlg.SelectedPath;
            };

            // 唯一的行为选项：关闭时最小化到托盘（其余行为均为默认固定）
            chkTray = new CheckBox();
            chkTray.SetBounds(16, 96, 190, 22);
            chkTray.Text = "关闭时最小化到托盘";
            chkTray.Checked = host.UiTray;

            btnSave = new Button();
            btnSave.SetBounds(424, 92, 116, 30);
            btnSave.Text = "保存设置";
            btnSave.Click += delegate
            {
                host.CommitSettings(txtPort.Text.Trim(), txtWork.Text.Trim(), chkTray.Checked);
                // 保存后同步回显示（端口校验失败时由宿主复位）
                txtPort.Text = host.UiPortText;
                txtWork.Text = host.UiWorkDir;
            };

            // 运行日志
            Label lLog = new Label();
            lLog.SetBounds(16, 132, 100, 20);
            lLog.Text = "运行日志:";

            txtLog = new TextBox();
            txtLog.SetBounds(16, 154, 528, 268);
            txtLog.Multiline = true;
            txtLog.ReadOnly = true;
            txtLog.ScrollBars = ScrollBars.Both;
            txtLog.WordWrap = false;
            txtLog.BackColor = Color.White;
            txtLog.ForeColor = Color.FromArgb(30, 30, 30);
            txtLog.Font = new Font("Consolas", 9.5f);
            txtLog.HideSelection = false;
            txtLog.Text = host.UiLogText;
            ContextMenuStrip logMenu = new ContextMenuStrip();
            logMenu.Items.Add("清空日志", null, delegate { txtLog.Clear(); });
            logMenu.Items.Add("复制全部", null, delegate { if (txtLog.TextLength > 0) Clipboard.SetText(txtLog.Text); });
            txtLog.ContextMenuStrip = logMenu;

            // 底部
            btnDoc = new Button();
            btnDoc.SetBounds(16, 440, 96, 30);
            btnDoc.Text = "使用文档";
            btnDoc.Click += delegate { GuideForm.ShowGuide(this); };

            btnLogDir = new Button();
            btnLogDir.SetBounds(120, 440, 116, 30);
            btnLogDir.Text = "打开日志目录";
            btnLogDir.Click += delegate { host.OpenLogDir(); };

            btnAbout = new Button();
            btnAbout.SetBounds(244, 440, 100, 30);
            btnAbout.Text = "关于此程序";
            btnAbout.Click += delegate { host.ShowAbout(); };

            btnClose = new Button();
            btnClose.SetBounds(448, 440, 96, 30);
            btnClose.Text = "关闭";
            btnClose.Click += delegate { Close(); };

            this.Controls.Add(lPort);
            this.Controls.Add(txtPort);
            this.Controls.Add(lWork);
            this.Controls.Add(txtWork);
            this.Controls.Add(btnBrowseWork);
            this.Controls.Add(chkTray);
            this.Controls.Add(btnSave);
            this.Controls.Add(lLog);
            this.Controls.Add(txtLog);
            this.Controls.Add(btnDoc);
            this.Controls.Add(btnLogDir);
            this.Controls.Add(btnAbout);
            this.Controls.Add(btnClose);

            // 日志实时显示
            host.LogLine += OnLogLine;
            this.FormClosed += delegate
            {
                try { host.LogLine -= OnLogLine; } catch { }
            };
        }

        private void OnLogLine(string line)
        {
            if (IsDisposed) return;
            try
            {
                if (InvokeRequired)
                {
                    BeginInvoke((Action)(delegate { OnLogLine(line); }));
                    return;
                }
                txtLog.AppendText(line + "\r\n");
                txtLog.SelectionStart = txtLog.TextLength;
                txtLog.ScrollToCaret();
            }
            catch { }
        }

        public void ShowWindow()
        {
            Show();
            WindowState = FormWindowState.Normal;
            Activate();
        }
    }
    // ---------------------------------------------------------------------
    // 内嵌 Harness 窗口（WebView2，无需浏览器）
    // 标准窗口形态：系统标题栏；标题栏/边框颜色跟随 Harness 主题（浅色主题→浅色栏，深色→深色）
    // ---------------------------------------------------------------------
    internal class HarnessWindow : Form
    {
        private WebView2 wv;
        private LauncherForm host;
        private string url;
        private bool wvReady = false;
        private Label lblInit;
        private MenuStrip menuStrip;   // 标准菜单栏（标题栏下方）

        [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
        private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int attrValue, int attrSize);
        private const int DWMWA_USE_IMMERSIVE_DARK_MODE = 20; // Win10 1809+ 深/浅色标题栏
        private const int DWMWA_BORDER_COLOR = 34;            // Win11 22H2+ 边框色
        private const int DWMWA_CAPTION_COLOR = 35;           // Win11 22H2+ 标题栏色
        private const int DWMWA_TEXT_COLOR = 36;              // Win11 22H2+ 标题文字色

        public HarnessWindow(string url, LauncherForm host)
        {
            this.host = host;
            this.url = url;
            this.Text = Program.AppName;
            this.ClientSize = new Size(1100, 720);
            this.MinimumSize = new Size(640, 480);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.Font = new Font("Microsoft YaHei UI", 9f);
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }

            // 纯净界面：标准窗口、无工具条；刷新用 F5/Ctrl+R（WebView2 内自带），其它操作在托盘菜单
            this.KeyPreview = true;
            this.KeyDown += delegate(object s, KeyEventArgs e)
            {
                if (e.KeyCode == Keys.F5 || (e.Control && e.KeyCode == Keys.R))
                {
                    if (wvReady) { try { wv.Reload(); } catch { } }
                    e.Handled = true;
                }
            };

            lblInit = new Label();
            lblInit.Text = "正在初始化内嵌浏览器…";
            lblInit.TextAlign = ContentAlignment.MiddleCenter;
            lblInit.Dock = DockStyle.Fill;
            lblInit.ForeColor = Color.Gray;

            wv = new WebView2();
            wv.Dock = DockStyle.Fill;
            wv.Visible = false;
            wv.NavigationCompleted += delegate(object s, CoreWebView2NavigationCompletedEventArgs e)
            {
                wv.Visible = true;
                lblInit.Visible = false;
                SampleTheme(); // 加载完成后，标题栏/边框颜色跟随 Harness 外观
            };

            // 菜单栏必须最后添加：WinForms 按反向 z-order 布局 Dock，
            // 最后添加的控件最先占位 → 菜单栏先占顶部，页面自动下移、绝不遮挡
            BuildMenuBar();
            this.Controls.Add(lblInit);
            this.Controls.Add(wv);
            this.Controls.Add(menuStrip);

            this.FormClosing += delegate(object s, FormClosingEventArgs e)
            {
                // 程序未退出时：按“关闭时最小化到托盘”设置决定行为
                if (!host.AppExiting)
                {
                    if (host.settings.TrayOnClose)
                    {
                        // 勾选：点 ✕ = 收进托盘，窗口从任务栏消失，服务继续后台运行
                        e.Cancel = true;
                        Hide();
                        try { host.TrayBalloon("已收进托盘，服务继续运行。双击托盘图标恢复。"); } catch { }
                    }
                    else
                    {
                        // 未勾选：点 ✕ = 真正的关闭（询问是否停止服务后退出）
                        host.RequestAppExit();
                        if (!host.AppExiting) e.Cancel = true; // 用户取消则不关闭
                    }
                }
            };

            InitWebView2();
        }

        // 标准菜单栏（标题栏下方，页面在其下自动下移）：设置 / 帮助（使用文档、打开日志目录）/ 关于
        private void BuildMenuBar()
        {
            menuStrip = new MenuStrip();
            menuStrip.Dock = DockStyle.Top;
            menuStrip.Padding = new Padding(6, 2, 0, 2);
            menuStrip.GripMargin = new Padding(0);

            ToolStripMenuItem mSettings = new ToolStripMenuItem("设置(S)");
            mSettings.Click += delegate { host.OpenSettings(); };
            ToolStripMenuItem mHelp = new ToolStripMenuItem("帮助(H)");
            ToolStripMenuItem mDoc = new ToolStripMenuItem("使用文档");
            mDoc.Click += delegate { GuideForm.ShowGuide(this); };
            ToolStripMenuItem mLog = new ToolStripMenuItem("打开日志目录");
            mLog.Click += delegate { host.OpenLogDir(); };
            mHelp.DropDownItems.Add(mDoc);
            mHelp.DropDownItems.Add(mLog);
            ToolStripMenuItem mAbout = new ToolStripMenuItem("关于此程序(A)");
            mAbout.Click += delegate { host.ShowAbout(); };
            menuStrip.Items.Add(mSettings);
            menuStrip.Items.Add(mHelp);
            menuStrip.Items.Add(mAbout);
        }

        // 菜单栏主题渲染器（背景/文字/悬停跟随 Harness 深/浅色）
        private class ThemeRenderer : ToolStripProfessionalRenderer
        {
            private Color bg, fg, hover;
            public ThemeRenderer(Color bg, Color fg, Color hover)
            {
                this.bg = bg; this.fg = fg; this.hover = hover;
            }
            protected override void OnRenderMenuItemBackground(ToolStripItemRenderEventArgs e)
            {
                using (SolidBrush b = new SolidBrush(e.Item.Selected ? hover : bg))
                {
                    e.Graphics.FillRectangle(b, new Rectangle(Point.Empty, e.Item.Size));
                }
            }
            protected override void OnRenderToolStripBackground(ToolStripRenderEventArgs e)
            {
                using (SolidBrush b = new SolidBrush(bg))
                {
                    e.Graphics.FillRectangle(b, e.AffectedBounds);
                }
            }
            protected override void OnRenderItemText(ToolStripItemTextRenderEventArgs e)
            {
                e.TextColor = fg;
                base.OnRenderItemText(e);
            }
            protected override void OnRenderToolStripBorder(ToolStripRenderEventArgs e)
            {
            }
        }

        public void ReloadPage()
        {
            if (wvReady && wv.CoreWebView2 != null)
            {
                try { wv.Reload(); } catch { }
            }
        }

        private void InitWebView2()
        {
            try
            {
                string profile = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "DSHLauncher", "webview2-profile");
                CoreWebView2EnvironmentOptions opt = new CoreWebView2EnvironmentOptions();
                opt.AdditionalBrowserArguments =
                    "--disable-extensions --disable-background-networking --disable-component-update "
                    + "--disable-sync --disable-breakpad --disable-background-mode --disable-gpu "
                    + "--disable-features=msEdgeSidebarV2,msEdgeShoppingAssistant,msEdgeTranslate";
                TaskScheduler ui = TaskScheduler.FromCurrentSynchronizationContext();
                CoreWebView2Environment.CreateAsync(null, profile, opt).ContinueWith(
                    delegate(Task<CoreWebView2Environment> t)
                    {
                        if (t.IsFaulted || t.IsCanceled) { OnInitFailed(); return; }
                        CoreWebView2Environment env = t.Result;
                        wv.EnsureCoreWebView2Async(env).ContinueWith(delegate(Task t2)
                        {
                            if (t2.IsFaulted || t2.IsCanceled) { OnInitFailed(); return; }
                            wvReady = true;
                            try
                            {
                                // 禁用默认右键菜单，保持界面纯净
                                wv.CoreWebView2.Settings.AreDefaultContextMenusEnabled = false;
                                wv.CoreWebView2.Navigate(url);
                            }
                            catch { }
                        }, ui);
                    }, ui);
            }
            catch (Exception ex)
            {
                host.Log("内嵌窗口初始化失败：" + ex.Message);
                OnInitFailed();
            }
        }

        private void OnInitFailed()
        {
            if (IsDisposed) return;
            try
            {
                if (InvokeRequired)
                {
                    BeginInvoke((Action)(delegate { OnInitFailed(); }));
                    return;
                }
            }
            catch { return; }
            host.Log("WebView2 不可用，改用 Edge 精简窗口打开。");
            try { host.OpenEdgeFallback(url); } catch { }
            try { Close(); } catch { }
        }

        public void NavigateTo(string u)
        {
            url = u;
            if (wvReady && wv.CoreWebView2 != null)
            {
                try { wv.CoreWebView2.Navigate(u); } catch { }
            }
        }

        public void ShowWindow()
        {
            Show();
            Activate();
        }

        // ---------- 标题栏/边框颜色跟随 Harness 主题 ----------
        // 用 JS 采样页面背景色；Win11 直接给标题栏/边框/文字着色，Win10 用深/浅色标题栏
        private void SampleTheme()
        {
            if (wv == null || wv.CoreWebView2 == null) return;
            try
            {
                string js =
                    "(function(){try{var b=getComputedStyle(document.body).backgroundColor;" +
                    "if(!b||b==='rgba(0, 0, 0, 0)'||b==='transparent')b=getComputedStyle(document.documentElement).backgroundColor;" +
                    "if(!b||b==='rgba(0, 0, 0, 0)'||b==='transparent')return '';return b;}catch(e){return '';}})()";
                wv.CoreWebView2.ExecuteScriptAsync(js).ContinueWith(delegate(Task<string> t)
                {
                    if (t.IsFaulted || t.IsCanceled || t.Result == null) return;
                    string s = t.Result.Trim().Trim('"');
                    if (s.Length == 0) return;
                    try { ApplyTheme(ParseCssColor(s)); } catch { }
                }, TaskScheduler.FromCurrentSynchronizationContext());
            }
            catch { }
        }

        private void ApplyTheme(Color c)
        {
            try
            {
                bool dark = (0.299 * c.R + 0.587 * c.G + 0.114 * c.B) < 128;
                int colorRef = (c.B << 16) | (c.G << 8) | c.R; // COLORREF 0x00BBGGRR
                int text = dark ? 0x00FFFFFF : 0x000000;
                int darkMode = dark ? 1 : 0;
                DwmSetWindowAttribute(this.Handle, DWMWA_CAPTION_COLOR, ref colorRef, 4);
                DwmSetWindowAttribute(this.Handle, DWMWA_BORDER_COLOR, ref colorRef, 4);
                DwmSetWindowAttribute(this.Handle, DWMWA_TEXT_COLOR, ref text, 4);
                DwmSetWindowAttribute(this.Handle, DWMWA_USE_IMMERSIVE_DARK_MODE, ref darkMode, 4);
                this.BackColor = c; // 客户区边缘同色，避免色差
                try { wv.DefaultBackgroundColor = c; } catch { }
                // 菜单栏配色跟随主题（深色界面→深色菜单栏，浅色→浅色）
                if (menuStrip != null)
                {
                    Color barBg = dark ? Color.FromArgb(33, 33, 33) : Color.FromArgb(244, 244, 244);
                    Color barFg = dark ? Color.FromArgb(220, 220, 220) : Color.FromArgb(45, 45, 45);
                    Color barHover = dark ? Color.FromArgb(72, 72, 72) : Color.FromArgb(222, 222, 222);
                    menuStrip.Renderer = new ThemeRenderer(barBg, barFg, barHover);
                }
            }
            catch { }
        }

        private static Color ParseCssColor(string css)
        {
            Match m = Regex.Match(css, "rgba?\\((\\d+),\\s*(\\d+),\\s*(\\d+)");
            if (m.Success)
            {
                return Color.FromArgb(255,
                    int.Parse(m.Groups[1].Value),
                    int.Parse(m.Groups[2].Value),
                    int.Parse(m.Groups[3].Value));
            }
            m = Regex.Match(css, "#([0-9a-fA-F]{6})");
            if (m.Success)
            {
                int val = Convert.ToInt32(m.Groups[1].Value, 16);
                return Color.FromArgb(255, (val >> 16) & 0xFF, (val >> 8) & 0xFF, val & 0xFF);
            }
            throw new FormatException("未知颜色: " + css);
        }
    }

    // ---------------------------------------------------------------------
    // 新手指引窗口
    // ---------------------------------------------------------------------
    internal class GuideForm : Form
    {
        public const string GuideText =
            "【" + Program.AppName + " 新手引导】\n" +
            "\n" +
            "──────────────────────────────\n" +
            "一、端口是什么？要不要改？\n" +
            "──────────────────────────────\n" +
            "端口就是电脑给程序开的\"门\"。DSH Harness 默认使用 3080 端口，\n" +
            "浏览器通过 http://127.0.0.1:3080 访问它。\n" +
            "· 默认不用改，直接用就行；\n" +
            "· 只有一种情况需要改：提示\"端口被占用\"且占用的不是 Harness 时，\n" +
            "  换一个数字（比如 3090），点\"保存\"即可，服务会自动用新端口重启；\n" +
            "· 端口范围 1~65535，随便选个没被占用的就行。\n" +
            "\n" +
            "──────────────────────────────\n" +
            "二、工作目录是什么？怎么选？\n" +
            "──────────────────────────────\n" +
            "工作目录是 Harness\"干活\"的地方：它在这里保存会话记录，\n" +
            "也是它读写文件的范围。\n" +
            "· 想让它帮忙处理日常文件 → 选桌面（默认就是）；\n" +
            "· 想让它专注某个项目 → 选那个项目文件夹；\n" +
            "· 换了目录等于换了一个新工作区，之前的会话不会丢，也不会混在一起；\n" +
            "· 点\"浏览…\"选择，改完点\"保存\"，服务会自动重启生效。\n" +
            "\n" +
            "──────────────────────────────\n" +
            "三、环境要求（一般不用管）\n" +
            "──────────────────────────────\n" +
            "启动器会自动检测 Node.js 和 dsh。需要它们时：\n" +
            "· 安装 Node.js LTS 版本；\n" +
            "· 打开命令行执行：npm install -g @deepseek-ai/dsh\n" +
            "· 没装也没关系：一键安装包会帮你部署，或点\"Node…\"手动指定。\n" +
            "\n" +
            "──────────────────────────────\n" +
            "四、常见问题\n" +
            "──────────────────────────────\n" +
            "· 打不开 http://127.0.0.1:3080 → 先点\"一键启动\"，等状态变\"运行中\"；\n" +
            "· 卡在\"启动中\"或提示残留进程无响应 → 会自动清理上次遗留的 Harness 进程并重新启动，\n" +
            "  或点\"一键启动\"触发自动清理；服务挂起时也会自动重启；\n" +
            "· 显示\"运行中（已接管）\" → 说明已有 Harness 在跑，直接点停止/打开界面；\n" +
            "· 嫌浏览器占内存 → 默认用\"内嵌窗口\"打开界面（无需浏览器，像原生软件一样），\n" +
            "  取消勾选则改用 Edge 精简窗口；两者都比完整浏览器省内存；\n" +
            "· 想开机自启 → 把启动器快捷方式放进 shell:startup 文件夹；\n" +
            "· 点 ✕ 想彻底退出 → 取消勾选\"关闭时最小化到托盘\"，或用托盘菜单\"退出\"。\n";

        public GuideForm()
        {
            this.Text = "新手指引";
            this.ClientSize = new Size(660, 520);
            this.MinimumSize = new Size(560, 420);
            this.StartPosition = FormStartPosition.CenterParent;
            this.Font = new Font("Microsoft YaHei UI", 9.5f);
            try { this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); }
            catch { }

            RichTextBox box = new RichTextBox();
            box.SetBounds(12, 12, 636, 460);
            box.ReadOnly = true;
            box.BackColor = Color.White;
            box.BorderStyle = BorderStyle.None;
            box.Font = new Font("Microsoft YaHei UI", 10f);
            box.Text = GuideText;
            box.ScrollBars = RichTextBoxScrollBars.Vertical;

            Button btnClose = new Button();
            btnClose.SetBounds(560, 484, 88, 28);
            btnClose.Text = "关闭";
            btnClose.Click += delegate { Close(); };

            this.Controls.Add(box);
            this.Controls.Add(btnClose);
        }

        public static void ShowGuide(IWin32Window owner)
        {
            GuideForm f = new GuideForm();
            f.Show(owner);
        }
    }

    // ---------------------------------------------------------------------
    // 隐藏自检：DSHLauncher.exe --selftest（验证 启动→就绪→停止 全链路）
    // ---------------------------------------------------------------------
    internal static class SelfTest
    {
        public static int Run()
        {
            string exeDir = Path.GetDirectoryName(Application.ExecutablePath);
            string logPath = Path.Combine(exeDir, "selftest.log");
            List<string> report = new List<string>();
            try
            {
                Settings s = new Settings();
                s.WorkDir = Path.Combine(Path.GetTempPath(), "dsh-launcher-selftest");
                Directory.CreateDirectory(s.WorkDir);

                report.Add("==== " + Program.AppName + " 自检 ====");
                report.Add("时间: " + DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss"));
                report.Add("");

                Engine.Resolve(s);
                report.Add("node.exe: " + (Engine.NodePath ?? "(未找到)"));
                report.Add("bin.js  : " + (Engine.BinJs ?? "(未找到)"));
                report.Add("工作目录: " + s.WorkDir);
                if (Engine.NodePath == null || Engine.BinJs == null)
                {
                    report.Add("");
                    report.Add("FAIL: 未解析到 dsh 启动入口。");
                    File.WriteAllLines(logPath, report.ToArray(), new UTF8Encoding(true));
                    return 1;
                }

                int port = FindFreePort();
                report.Add("测试端口: " + port);
                report.Add("");

                StringBuilder sb = new StringBuilder();
                Process p = Engine.StartServer(port, s.WorkDir);
                report.Add("已启动 PID " + p.Id + "，等待就绪（最长 120 秒）…");
                string cmdline = Engine.GetCommandLine(p.Id);
                bool identityOk = cmdline.Length > 0 && Engine.IsDshHarness(p.Id);
                report.Add("WMI 命令行读取: " + (cmdline.Length > 0 ? "成功" : "失败"));
                report.Add("身份识别 (IsDshHarness): " + (identityOk ? "通过 ✓" : "FAIL"));
                p.OutputDataReceived += delegate(object o, DataReceivedEventArgs e)
                {
                    if (e.Data != null) { lock (sb) { AppendTrim(sb, e.Data); } }
                };
                p.ErrorDataReceived += delegate(object o, DataReceivedEventArgs e)
                {
                    if (e.Data != null) { lock (sb) { AppendTrim(sb, e.Data); } }
                };
                p.BeginOutputReadLine();
                p.BeginErrorReadLine();

                DateTime deadline = DateTime.Now.AddSeconds(120);
                bool ready = false;
                while (DateTime.Now < deadline)
                {
                    bool exited = false;
                    try { exited = p.HasExited; } catch { exited = true; }
                    if (exited) break;
                    if (Engine.IsServerReady(port)) { ready = true; break; }
                    Thread.Sleep(500);
                }

                if (ready)
                {
                    int ms = (int)(DateTime.Now.Subtract(deadline.AddSeconds(-120))).TotalMilliseconds;
                    report.Add("PASS: 服务已就绪（约 " + ms + " ms）。");
                    report.Add("监听检查: " + (Engine.IsPortListening(port) ? "通过" : "失败"));
                }
                else
                {
                    bool exited = false;
                    int code = -1;
                    try { exited = p.HasExited; code = p.ExitCode; } catch { }
                    report.Add("FAIL: 120 秒内未就绪。" + (exited ? " 进程已退出，退出码 " + code : ""));
                }

                report.Add("");
                report.Add("----- 服务输出（末尾）-----");
                lock (sb) { report.Add(sb.ToString()); }

                Engine.KillProcessTree(p);
                DateTime stopDeadline = DateTime.Now.AddSeconds(10);
                bool freed = true;
                while (Engine.IsPortListening(port) && DateTime.Now < stopDeadline) Thread.Sleep(300);
                if (Engine.IsPortListening(port))
                {
                    freed = false;
                    report.Add("FAIL: 停止后端口仍被占用。");
                }
                else
                {
                    report.Add("PASS: 停止后端口已释放。");
                }

                report.Add("");
                report.Add((ready && freed) ? "==== 自检通过 ====" : "==== 自检失败 ====");
                File.WriteAllLines(logPath, report.ToArray(), new UTF8Encoding(true));
                return (ready && freed && identityOk) ? 0 : 1;
            }
            catch (Exception ex)
            {
                report.Add("异常: " + ex.ToString());
                File.WriteAllLines(logPath, report.ToArray(), new UTF8Encoding(true));
                return 1;
            }
        }

        private static void AppendTrim(StringBuilder sb, string line)
        {
            sb.AppendLine(Engine.Sanitize(line));
            if (sb.Length > 16000) sb.Remove(0, sb.Length - 16000);
        }

        private static int FindFreePort()
        {
            TcpListener l = new TcpListener(IPAddress.Loopback, 0);
            l.Start();
            int p = ((IPEndPoint)l.LocalEndpoint).Port;
            l.Stop();
            return p;
        }
    }
}
