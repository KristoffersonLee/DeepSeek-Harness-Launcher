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
using System.Reflection;
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
        public const string AppVersion = "3.0.0";
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
                // 强制创建窗口句柄：此后 BeginInvoke 必定可用，避免等待线程在句柄创建前收到唤起信号而异常退出
                GC.KeepAlive(form.Handle);
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
        // 行为固定：打开自动启动服务、就绪自动打开内嵌窗口（原 AutoStart/AutoOpen/LiteBrowser 死设置已移除）
        public bool TrayOnClose = true; // 点叉时最小化到托盘（后台运行）
        public bool LanEnabled = false; // 局域网共享开关（默认关闭，显式开启）
        public int LanPort = 3081;      // 局域网网关端口（默认 3080 的下一位）
        public string LanPin = "";      // 自定义访问 PIN（留空则自动生成，见 lan-pin.txt）
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
                        // 兼容旧版设置文件：autostart/autoopen/litebrowser 已无对应行为，忽略读取
                        case "nodepath":
                            NodePath = val;
                            break;
                        case "workdir":
                            if (val.Length > 0) WorkDir = val;
                            break;
                        case "trayonclose":
                            TrayOnClose = ParseBool(val, TrayOnClose);
                            break;
                        case "lanenabled":
                            LanEnabled = ParseBool(val, LanEnabled);
                            break;
                        case "lanport":
                            int lp;
                            if (int.TryParse(val, out lp) && lp >= 1 && lp <= 65535) LanPort = lp;
                            break;
                        case "lanpin":
                            LanPin = val;
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
                sb.AppendLine("nodePath=" + NodePath);
                sb.AppendLine("workDir=" + WorkDir);
                sb.AppendLine("trayOnClose=" + TrayOnClose.ToString().ToLowerInvariant());
                sb.AppendLine("lanEnabled=" + LanEnabled.ToString().ToLowerInvariant());
                sb.AppendLine("lanPort=" + LanPort);
                sb.AppendLine("lanPin=" + LanPin);
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
                DetectError = "未找到 node.exe。请安装 Node.js，或在设置文件 settings.ini 中手动指定 nodePath。";
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

        public static Process StartServer(int port, string workDir, bool lanEnabled = false)
        {
            ProcessStartInfo psi = new ProcessStartInfo();
            psi.FileName = NodePath;
            psi.Arguments = "\"" + BinJs + "\" web --host 127.0.0.1 --port " + port;
            psi.UseShellExecute = false;
            psi.CreateNoWindow = true;
            psi.RedirectStandardOutput = true;
            psi.RedirectStandardError = true;
            // dsh web 的 stdout/stderr 为 UTF-8：必须显式指定解码编码，
            // 否则中文系统（GBK 代码页）按 ANSI 解码导致运行日志乱码
            psi.StandardOutputEncoding = Encoding.UTF8;
            psi.StandardErrorEncoding = Encoding.UTF8;
            if (Directory.Exists(workDir))
                psi.WorkingDirectory = workDir;
            else
                psi.WorkingDirectory = Path.GetDirectoryName(BinJs);
            if (lanEnabled)
            {
                // 若底层使用 Ollama 本地推理：允许局域网内的 Harness 网关调用模型接口
                psi.EnvironmentVariables["OLLAMA_HOST"] = "0.0.0.0";
                psi.EnvironmentVariables["OLLAMA_ORIGINS"] = "*";
            }
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
                    // 先等待退出再读输出，避免 ReadToEnd 无限阻塞（netstat 异常挂起时 5 秒超时兜底）
                    if (!p.WaitForExit(5000))
                    {
                        try { p.Kill(); } catch { }
                        return 0;
                    }
                    string outp = p.StandardOutput.ReadToEnd();
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
            try
            {
                using (Process p2 = Process.GetProcessById(pid)) { if (p2 != null) p2.Kill(); }
            }
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

        // 日志脱敏：剥离 dsh 一次性认证 token 与局域网 PIN，避免凭据明文落盘 launcher.log
        public static string RedactSecrets(string s)
        {
            if (string.IsNullOrEmpty(s)) return "";
            // ?token=xxx 或 &token=xxx（URL 查询参数形式）
            s = Regex.Replace(s, "[?&]token=[^&\\s\"']+", "?token=***", RegexOptions.IgnoreCase);
            // “访问 PIN：123456” / “PIN: 123456”（中文/英文冒号均可）
            s = Regex.Replace(s, "(?i)(访问\\s*)?pin\\s*[：:]\\s*[^\\s，。；）)\"']+", "$1PIN: ***");
            return s;
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
        private volatile string AuthenticatedUrl = null; // dsh web 输出的一次性 token URL（0.1.2-alpha 起认证必需；旧版本为 null 回退普通地址；跨线程读写需 volatile）
        private SettingsForm settingsForm = null;    // 设置窗口（替代原启动器面板）
        private Process lanGateway = null;           // 局域网网关进程（node lan-gateway.mjs）
        private string lanIp = "";                 // 当前绑定的局域网 IP
        private string lanAdapter = "";            // 网卡名（面板显示）
        private bool lanWireless = false;            // 是否无线网卡
        private string lanPlainUrl = "";           // 不含 token 的局域网地址（二维码/复制用）
        private int lanExternalPid = 0;              // 已接管的外部网关 PID
        internal event Action<string> LogLine;       // 日志行事件（设置窗口实时显示）
        private readonly object logLock = new object();                    // 日志缓冲锁
        private System.Collections.Generic.List<string> logBuffer = new System.Collections.Generic.List<string>();
        private const int LogBufferMax = 800;                              // 内存日志缓冲上限（行，超出丢最旧）
        private bool exitConfirmed = false;          // 内嵌窗口关闭并退出（已确认）
        private bool exitKillService = false;        // 退出时是否同时停止服务（默认否：退出保留服务，网页端不中断）

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
                ReleaseServer();
                serverReady = false;
                starting = false;
                StartServer();
            }
            else
            {
                Log("设置已保存（端口 " + p + "）。");
            }
            // 端口变更且局域网已开启：先停旧网关（其 DSH_TARGET 仍指向旧端口），
            // 服务就绪后 PollTick → MaybeStartLanGateway 会用新端口/新 token 自动重启网关
            if (portChanged && settings.LanEnabled)
            {
                Log("局域网网关目标端口已变更，正在重启网关…");
                StopLanGateway();
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
                    // 按字节裁剪（保留尾部约 1MB）：全中文日志的字符数远小于字节数，
                    // 原“字符数 > 100 万”判断会导致中文日志永不裁剪、无限增长
                    byte[] allBytes = File.ReadAllBytes(p);
                    if (allBytes.Length > 1024 * 1024)
                    {
                        byte[] tail = new byte[1024 * 1024];
                        Array.Copy(allBytes, allBytes.Length - tail.Length, tail, 0, tail.Length);
                        string s = Encoding.UTF8.GetString(tail);
                        // 丢弃解码产生的替换字符（U+FFFD），避免首字符被截半
                        int bad = s.IndexOf('\uFFFD');
                        if (bad >= 0 && bad < 2) s = s.Substring(bad + 1);
                        File.WriteAllText(p, s, Encoding.UTF8);
                    }
                }
            }
            catch { }
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
            if (string.IsNullOrEmpty(e.Data)) return;
            // 日志脱敏：dsh 输出可能含一次性 token URL，不能明文落盘
            Log("  " + Engine.RedactSecrets(Engine.Sanitize(e.Data)));
            // 0.1.2-alpha 起 dsh web 打印带一次性 token 的认证 URL（形如 dsh web: http://127.0.0.1:3080/?token=...），
            // 内嵌窗口必须导航到该地址才能通过认证；旧版本无此输出时保持普通地址。
            try
            {
                string line = e.Data.Trim();
                int idx = line.IndexOf("dsh web: http", StringComparison.OrdinalIgnoreCase);
                if (idx >= 0)
                {
                    string url = line.Substring(idx + "dsh web: ".Length).Trim();
                    int lan = url.IndexOf(" (LAN: ", StringComparison.OrdinalIgnoreCase);
                    if (lan >= 0) url = url.Substring(0, lan);
                    if (url.StartsWith("http://", StringComparison.OrdinalIgnoreCase)
                        || url.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                    {
                        AuthenticatedUrl = url;
                        // 局域网网关需要此启动令牌（dsh 重启后自动重新兑换）
                        string tk = ExtractToken(url);
                        if (tk.Length > 0) SaveLanToken(tk);
                    }
                }
            }
            catch { } // 解析失败不影响日志
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



        // 释放服务进程对象并置空（进程已退出/已结束的场景；Dispose 只释放句柄，不杀进程）
        private void ReleaseServer()
        {
            if (server != null)
            {
                try { server.Dispose(); } catch { }
                server = null;
            }
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
                    ReleaseServer();
                    serverReady = false;
                    starting = false;
                    StopLanGateway(); // 服务停止时同步关闭局域网入口
                    SetState(RunState.Stopped);
                    RevealLauncherOnError();
                    // 宿主窗体常驻隐藏（Visible 恒为 false），以是否有可见的内嵌窗口/设置窗口判断是否弹气泡
                    bool anyWindowVisible = (embedded != null && !embedded.IsDisposed && embedded.Visible)
                        || (settingsForm != null && !settingsForm.IsDisposed && settingsForm.Visible);
                    if (!anyWindowVisible)
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
                            MaybeStartLanGateway();
                            OpenBrowser(); // 固定：就绪后自动打开界面
                        }
                        else if ((DateTime.Now - serverStartTime).TotalMilliseconds > StartupTimeoutMs)
                        {
                            // 进程存活但长时间未就绪：结束“无限启动中”的卡死状态
                            Log("启动超时：120 秒内服务未就绪，已停止并复位。请查看上方日志输出后重试。");
                            try { Engine.KillProcessTree(server); } catch { }
                            ReleaseServer();
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
                                ReleaseServer();
                                serverReady = false;
                                starting = false;
                                lostResponse = 0;
                                SetState(RunState.Stopped);
                                StartServer();
                            }
                            else
                            {
                                Log("服务连续挂起且已自动重启 3 次仍未恢复，请检查工作目录/端口设置后点击托盘菜单“启动服务”重试。");
                                try { Engine.KillProcessTree(server); } catch { }
                                ReleaseServer();
                                serverReady = false;
                                starting = false;
                                lostResponse = 0;
                                autoRestartCount = 0;
                                StopLanGateway(); // 服务已不可用，同步关闭局域网入口，避免手机端误以为仍可用
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
                            Log("已接管端口 " + port + " 上的 DSH Harness（PID " + pid + "），托盘“停止服务”可直接关闭。");
                        }
                        else
                        {
                            Log("端口 " + port + " 上的进程无法确认是 DSH Harness，停止前需要人工确认。");
                        }
                    }
                    MaybeStartLanGateway(); // 识别完成后才尝试启动局域网网关
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
                            Log("点击托盘菜单“启动服务”将自动结束该残留进程并重新启动服务。");
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
                    ReleaseServer();
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

            // Resolve 可能因目录无权限等抛异常（如 PATH 遍历、Directory.GetDirectories），必须保护，避免在 UI 线程崩溃
            try
            {
                Engine.Resolve(settings);
                if (Engine.NodePath == null || Engine.BinJs == null)
                {
                    Log("无法启动：" + Engine.DetectError.Replace("\n", " "));
                    SetState(RunState.Error);
                    RevealLauncherOnError();
                    return;
                }
            }
            catch (Exception ex)
            {
                Log("无法解析环境：" + ex.Message);
                SetState(RunState.Error);
                RevealLauncherOnError();
                return;
            }

            try
            {
                server = Engine.StartServer(port, settings.WorkDir, settings.LanEnabled);
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
                ReleaseServer();
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
                    ReleaseServer();
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
                ReleaseServer();
                serverReady = false;
                starting = false;
                StopLanGateway(); // 同步关闭局域网入口
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
                StopLanGateway(); // 同步关闭局域网入口
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

        // ------------------------- 局域网共享（LAN） -------------------------
        internal string UiLanPortText { get { return settings.LanPort.ToString(); } }
        internal bool UiLanEnabled { get { return settings.LanEnabled; } }
        internal string UiLanPin { get { return settings.LanPin; } }
        internal string UiLanIp { get { return lanIp; } }
        internal string UiLanAdapter { get { return lanAdapter; } }
        internal bool UiLanWireless { get { return lanWireless; } }
        internal string UiLanPlainUrl { get { return lanPlainUrl; } }
        internal bool UiOllamaDetected { get { return LanAccess.IsOllamaInstalled(); } }

        // 生效 PIN 的来源说明（环境变量/.env/lan-pin.txt 解析结果，供面板展示）
        internal string UiLanPinSource
        {
            get
            {
                string src;
                LanAccess.EffectivePin(out src);
                // lan-pin.txt 可能存的是自定义 PIN 或自动生成 PIN：以 settings.LanPin 区分显示，
                // 避免用户已自定义却显示“启动器自动生成”造成误导
                if (settings.LanPin.Length > 0 && src == "启动器自动生成")
                    return "自定义（lan-pin.txt）";
                return src;
            }
        }

        internal string UiLanStatus
        {
            get
            {
                if (!settings.LanEnabled) return "未开启";
                if (lanGateway != null && !lanGateway.HasExited) return "运行中 · " + lanPlainUrl;
                if (lanExternalPid > 0) return "运行中（已接管）· " + lanPlainUrl;
                if (lanIp.Length == 0) return "未运行（未检测到活动网卡）";
                return "未运行";
            }
        }

        internal string UiFirewallStatus
        {
            get
            {
                if (!settings.LanEnabled) return "未配置（局域网未开启）";
                try
                {
                    if (LanAccess.HasRule(settings.LanPort)) return "已配置（仅本地子网）";
                }
                catch { }
                return "未配置（需要管理员权限）";
            }
        }

        // 刷新局域网 IP 探测结果（面板/启动网关共用）
        private void RefreshLanIp()
        {
            lanIp = LanAccess.DetectLanIp(out lanAdapter, out lanWireless);
            if (lanIp.Length > 0)
            {
                lanPlainUrl = "http://" + lanIp + ":" + settings.LanPort + "/";
            }
            else
            {
                lanPlainUrl = "";
            }
        }

        // 启动时清理孤儿网关：局域网未开启时，若局域网端口残留 lan-gateway.mjs 进程则结束
        // （启动器被强杀/断电时不会走正常退出清理，残留网关会继续监听）
        private void CleanupOrphanGateway()
        {
            if (settings.LanEnabled) return;
            try
            {
                int pid = Engine.FindPidOnPort(settings.LanPort);
                if (pid > 0 && pid != Process.GetCurrentProcess().Id && IsLanGatewayPid(pid))
                {
                    Log("检测到残留的局域网网关（PID " + pid + "），但局域网共享已关闭，正在清理…");
                    Engine.KillProcessTree(pid);
                }
            }
            catch { }
        }

        // 服务就绪 / 外部实例被接管后，若局域网开关打开则启动（或接管）网关
        private DateTime lastLanStartAttempt = DateTime.MinValue; // 启动失败重试节流
        private void MaybeStartLanGateway()
        {
            if (!settings.LanEnabled) return;
            if (lanGateway != null)
            {
                try { if (!lanGateway.HasExited) return; } catch { }
            }
            if (lanExternalPid > 0)
            {
                // 已接管的网关进程若已死亡则清除引用，允许重新启动（自愈）
                try { Process.GetProcessById(lanExternalPid); return; }
                catch { lanExternalPid = 0; }
            }
            // 失败后最多每 5 秒重试一次，避免接管路径下反复刷日志
            if ((DateTime.Now - lastLanStartAttempt).TotalSeconds < 5) return;
            lastLanStartAttempt = DateTime.Now;
            StartLanGateway();
        }

        private void StartLanGateway()
        {
            if (lanIp.Length == 0) RefreshLanIp();
            if (lanIp.Length == 0)
            {
                Log("局域网共享：未检测到活动 WiFi/以太网 IP，无法开启。");
                return;
            }
            // 确保 node.exe 已解析（接管外部实例时 StartServer 未调用，Engine.NodePath 可能为 null）
            if (Engine.NodePath == null || Engine.NodePath.Length == 0)
            {
                try { Engine.Resolve(settings); }
                catch (Exception ex)
                {
                    Log("局域网共享：环境解析失败（" + ex.Message + "）。");
                    return;
                }
                if (Engine.NodePath == null || Engine.NodePath.Length == 0)
                {
                    Log("局域网共享：未找到 node.exe，无法启动网关（" + Engine.DetectError + "）。");
                    return;
                }
            }
            // 获取 dsh 启动令牌：自启路径由 OnServerOutput 捕获并写入 lan-token.txt；
            // 接管路径可能缺失（旧启动器实例启动的 dsh web 不会重打 token）——
            // 若已确认占用者是 DSH Harness 且 LAN 需要令牌，则重启一次以获取新令牌
            // （网页会话 Cookie 持久化，重启不影响已认证浏览器）。
            bool haveToken = ExtractToken(AuthenticatedUrl).Length > 0;
            if (!haveToken)
            {
                try
                {
                    string t = File.Exists(LanAccess.TokenFilePath)
                        ? File.ReadAllText(LanAccess.TokenFilePath, Encoding.UTF8).Trim() : "";
                    haveToken = t.Length > 0;
                }
                catch { }
            }
            // 仅在外部/接管实例（非自启）且确认占用者是 DSH Harness 时重启以获取令牌
            bool oursRunning = server != null;
            try { if (server != null && server.HasExited) oursRunning = false; } catch { oursRunning = false; }
            if (!haveToken && !oursRunning)
            {
                int ownerPid = externalPid > 0 ? externalPid : Engine.FindPidOnPort(Port);
                bool ownerIsHarness = ownerPid > 0
                    && ownerPid != Process.GetCurrentProcess().Id
                    && Engine.IsDshHarness(ownerPid);
                if (ownerIsHarness)
                {
                    Log("局域网共享需要 dsh 启动令牌，正在重启已接管的 Harness 服务以获取令牌…");
                    StopLanGateway(); // 先停掉已接管的网关
                    // 若局域网端口上还有遗留网关（旧启动器启动、可能配置过期），一并清理
                    int oldGw = Engine.FindPidOnPort(settings.LanPort);
                    if (oldGw > 0 && oldGw != Process.GetCurrentProcess().Id && IsLanGatewayPid(oldGw))
                    {
                        try { Engine.KillProcessTree(oldGw); } catch { }
                    }
                    try { Engine.KillProcessTree(ownerPid); } catch { }
                    externalPid = 0;
                    adoptTried = false;
                    serverReady = false;
                    starting = false;
                    SetState(RunState.Stopped);
                    StartServer();
                    return;
                }
            }

            // 端口被占用且不是已有网关 → 提示换端口
            if (LanAccess.IsLanPortInUse(lanIp, settings.LanPort) && !LanAccess.IsGatewayRunning(lanIp, settings.LanPort))
            {
                Log("局域网端口 " + settings.LanPort + " 已被占用，请更换端口。");
                return;
            }
            // 已有网关在运行（上次会话遗留）→ 接管
            if (LanAccess.IsGatewayRunning(lanIp, settings.LanPort))
            {
                int pid = Engine.FindPidOnPort(settings.LanPort);
                if (pid > 0 && IsLanGatewayPid(pid))
                {
                    lanExternalPid = pid;
                    Log("已接管运行中的局域网网关（PID " + pid + "）。");
                }
                else
                {
                    Log("局域网端口 " + settings.LanPort + " 上已有服务在运行，未接管。");
                }
                return;
            }
            string gw = LanAccess.WriteGateway();
            if (gw.Length == 0)
            {
                Log("局域网共享：无法释放 lan-gateway.mjs（资源缺失，请重新构建启动器）。");
                return;
            }
            // PIN 解析：环境变量/.env → 自动生成
            string pinSrc;
            string pin = LanAccess.EffectivePin(out pinSrc);
            if (pin.Length == 0)
            {
                pin = LanAccess.GeneratePin();
                // 不在日志中打印 PIN（凭据不入日志文件），PIN 只在设置面板展示
                Log("已自动生成访问 PIN（可在设置面板查看或修改）。");
            }
            string token = ExtractToken(AuthenticatedUrl);
            if (token.Length > 0) SaveLanToken(token);

            ProcessStartInfo psi = new ProcessStartInfo();
            psi.FileName = Engine.NodePath;
            psi.Arguments = "\"" + gw + "\"";
            psi.UseShellExecute = false;
            psi.CreateNoWindow = true;
            psi.RedirectStandardOutput = true;
            psi.RedirectStandardError = true;
            // lan-gateway.mjs 的 console.log 输出为 UTF-8：显式指定解码编码，避免运行日志中文乱码
            psi.StandardOutputEncoding = Encoding.UTF8;
            psi.StandardErrorEncoding = Encoding.UTF8;
            psi.WorkingDirectory = LanAccess.AppDataDir;
            psi.EnvironmentVariables["DSH_LAN_HOST"] = lanIp;
            psi.EnvironmentVariables["DSH_LAN_PORT"] = settings.LanPort.ToString();
            psi.EnvironmentVariables["DSH_TARGET"] = "http://127.0.0.1:" + Port;
            psi.EnvironmentVariables["DSH_LAN_PIN"] = pin;
            psi.EnvironmentVariables["DSH_LAN_PIN_FILE"] = LanAccess.PinFilePath;
            psi.EnvironmentVariables["DSH_LAN_TOKEN"] = token;
            psi.EnvironmentVariables["DSH_LAN_TOKEN_FILE"] = LanAccess.TokenFilePath;
            psi.EnvironmentVariables["DSH_LAN_SECRET_FILE"] = LanAccess.SecretFilePath;
            psi.EnvironmentVariables["DSH_LAN_LOG"] = LanAccess.GatewayLogPath;
            try { psi.EnvironmentVariables["DSH_LAN_ICON_B64"] = LoadWhaleIconB64(); } catch { }
            try
            {
                lanGateway = Process.Start(psi);
                lanGateway.OutputDataReceived += OnLanGatewayOutput;
                lanGateway.ErrorDataReceived += OnLanGatewayOutput;
                lanGateway.BeginOutputReadLine();
                lanGateway.BeginErrorReadLine();
                lanExternalPid = 0;
                lanPlainUrl = "http://" + lanIp + ":" + settings.LanPort + "/";
                // 不在日志中打印 PIN（凭据不入日志文件）
                Log("局域网共享已开启: " + lanPlainUrl + "（PIN 请在设置面板查看）");
                ApplyFirewallBestEffort();
            }
            catch (Exception ex)
            {
                lanGateway = null;
                Log("局域网网关启动失败：" + ex.Message);
            }
        }

        private void OnLanGatewayOutput(object sender, DataReceivedEventArgs e)
        {
            if (string.IsNullOrEmpty(e.Data)) return;
            Log("  [lan] " + Engine.Sanitize(e.Data));
        }

        private static bool IsLanGatewayPid(int pid)
        {
            string cmd = Engine.GetCommandLine(pid);
            if (cmd.Length == 0) return false;
            return cmd.IndexOf("lan-gateway.mjs", StringComparison.OrdinalIgnoreCase) >= 0;
        }

        private static string ExtractToken(string url)
        {
            if (string.IsNullOrEmpty(url)) return "";
            int i = url.IndexOf("token=", StringComparison.OrdinalIgnoreCase);
            if (i < 0) return "";
            string t = url.Substring(i + 6);
            int amp = t.IndexOf('&');
            if (amp >= 0) t = t.Substring(0, amp);
            return t;
        }

        private static void SaveLanToken(string token)
        {
            try
            {
                Directory.CreateDirectory(LanAccess.AppDataDir);
                File.WriteAllText(LanAccess.TokenFilePath, token, new UTF8Encoding(false));
            }
            catch { }
        }

        internal static string LoadWhaleIconB64()
        {
            try
            {
                Assembly asm = Assembly.GetExecutingAssembly();
                string resName = null;
                foreach (string n in asm.GetManifestResourceNames())
                {
                    if (n.EndsWith("whale-256.png", StringComparison.OrdinalIgnoreCase)) { resName = n; break; }
                }
                if (resName == null) return "";
                using (Stream s = asm.GetManifestResourceStream(resName))
                {
                    if (s == null) return "";
                    using (MemoryStream ms = new MemoryStream())
                    {
                        s.CopyTo(ms);
                        return Convert.ToBase64String(ms.ToArray());
                    }
                }
            }
            catch (Exception)
            {
                return "";
            }
        }

        internal void StopLanGateway()
        {
            if (lanGateway != null)
            {
                try
                {
                    if (!lanGateway.HasExited) Engine.KillProcessTree(lanGateway);
                }
                catch { }
                lanGateway = null;
            }
            if (lanExternalPid > 0)
            {
                try { Engine.KillProcessTree(lanExternalPid); } catch { }
                lanExternalPid = 0;
            }
        }

        private void ApplyFirewallBestEffort()
        {
            int port = settings.LanPort;
            try
            {
                if (LanAccess.HasRule(port))
                {
                    Log("防火墙规则已存在：" + LanAccess.RuleName(port));
                    return;
                }
            }
            catch { }
            string msg;
            int r = LanAccess.TryAddRule(port, out msg);
            if (r == 0)
            {
                Log(msg);
            }
            else
            {
                Log("防火墙自动配置未生效（" + msg + "）。可在设置面板复制手动命令，或以管理员身份重试。");
            }
        }

        private void RemoveFirewallBestEffort()
        {
            int port = settings.LanPort;
            try
            {
                if (!LanAccess.HasRule(port)) return;
            }
            catch { return; }
            string msg;
            int r = LanAccess.TryRemoveRule(port, out msg);
            if (r == 0) Log(msg);
            else Log("防火墙规则删除未生效（" + msg + "）。请以管理员身份执行手动命令。");
        }

        // 设置面板提交局域网配置（开关/端口/PIN）。返回是否已生效（弹窗取消时返回 false）。
        internal bool CommitLanSettings(bool enabled, string portText, string pinText)
        {
            int port;
            if (!int.TryParse(portText, out port) || port < 1 || port > 65535)
            {
                MessageBox.Show("局域网端口必须是 1–65535 之间的数字。", Program.AppName,
                    MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return false;
            }
            if (port == settings.Port)
            {
                MessageBox.Show("局域网端口不能与 Harness 端口（" + settings.Port + "）相同。", Program.AppName,
                    MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return false;
            }
            bool portChanged = port != settings.LanPort;
            bool enabling = enabled && !settings.LanEnabled;

            if (enabling)
            {
                DialogResult r = MessageBox.Show(
                    "此功能将允许同一 WiFi 下的其他设备访问您的 AI 服务，请确保处于可信网络（如家庭 WiFi），不要在公共网络开启。\n\n是否继续开启局域网访问？",
                    "开启局域网共享", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
                if (r != DialogResult.Yes) return false;
            }

            if (enabled)
            {
                string name;
                bool w;
                string ip = LanAccess.DetectLanIp(out name, out w);
                if (ip.Length == 0)
                {
                    MessageBox.Show("未检测到活动 WiFi/以太网 IP，无法开启局域网访问。", Program.AppName,
                        MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    return false;
                }
                if (LanAccess.IsLanPortInUse(ip, port) && !LanAccess.IsGatewayRunning(ip, port))
                {
                    MessageBox.Show("端口 " + port + " 已被占用，请更换一个未使用的端口。", Program.AppName,
                        MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    return false;
                }
                lanIp = ip;
                lanAdapter = name;
                lanWireless = w;
                lanPlainUrl = "http://" + ip + ":" + port + "/";
            }

            bool wasRunning = settings.LanEnabled
                && (lanGateway != null || lanExternalPid > 0);

            settings.LanPort = port;
            settings.LanEnabled = enabled;
            string oldPinSrc;
            string oldEffectivePin = LanAccess.EffectivePin(out oldPinSrc);
            string trimmedPin = pinText == null ? "" : pinText.Trim();
            if (trimmedPin.Length > 0)
            {
                // 自定义 PIN：写入 settings 与 lan-pin.txt（彻底解决“自定义不生效”）
                settings.LanPin = trimmedPin;
                LanAccess.SavePin(trimmedPin);
            }
            else
            {
                // 清空 = 回到自动生成：必须同步删除 lan-pin.txt，否则旧自定义 PIN 仍会生效
                settings.LanPin = "";
                try { if (File.Exists(LanAccess.PinFilePath)) File.Delete(LanAccess.PinFilePath); } catch { }
            }
            settings.Save();
            // PIN 发生变化 → 轮换签名密钥，旧 Cookie 全部失效（手机需重新输入 PIN）
            string newPinSrc;
            string newEffectivePin = LanAccess.EffectivePin(out newPinSrc);
            if (newEffectivePin.Length > 0 && newEffectivePin != oldEffectivePin)
            {
                LanAccess.DeleteSecret();
            }

            if (enabled)
            {
                StartLanGateway();
            }
            else
            {
                StopLanGateway();
                RemoveFirewallBestEffort();
                Log("局域网共享已关闭，手机将无法访问。");
            }
            if (enabled && portChanged && wasRunning)
            {
                Log("局域网端口已变更，正在重启网关…");
                StopLanGateway();
                StartLanGateway();
            }
            return true;
        }

        // 重新生成 PIN（写 lan-pin.txt 并重启网关使生效）
        internal string RegenerateLanPin()
        {
            settings.LanPin = "";
            string pin = LanAccess.GeneratePin();
            settings.Save();
            // 轮换会话签名密钥：所有旧 Cookie 立即失效，手机端必须输入新 PIN
            LanAccess.DeleteSecret();
            if (lanGateway != null || lanExternalPid > 0)
            {
                Log("PIN 已重新生成，正在重启局域网网关…");
                StopLanGateway();
                StartLanGateway();
            }
            return pin;
        }

        // 以管理员身份（UAC）配置防火墙
        internal void TryFirewallElevated()
        {
            bool ok = LanAccess.TryAddRuleElevated(settings.LanPort);
            if (ok) Log("已打开管理员窗口配置防火墙（请在弹出的 PowerShell 窗口中查看结果，按 Enter 关闭）。");
            else Log("未能打开管理员窗口（可能取消了 UAC 授权）。");
        }

        // 彻底删除全部归档会话（删除磁盘数据 + 清空归档列表 + 重启服务生效）
        internal void CleanArchivedSessions()
        {
            try
            {
                int count = LanAccess.ArchivedSessionCount();
                if (count == 0)
                {
                    MessageBox.Show("当前没有归档会话。", Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Information);
                    return;
                }
                DialogResult r = MessageBox.Show(
                    "将彻底删除 " + count + " 个已归档会话及其全部数据（聊天记录、文件引用），此操作不可恢复。\n\n确定继续吗？",
                    "清理归档会话", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
                if (r != DialogResult.Yes) return;
                string detail;
                int deleted = LanAccess.DeleteArchivedSessions(out detail);
                // 重启 dsh 服务让归档列表变更生效
                if (deleted > 0)
                {
                    Log("归档会话已清理，正在重启服务以生效…");
                    if (server != null && !server.HasExited) { try { Engine.KillProcessTree(server); } catch { } }
                    if (externalPid > 0) { try { Engine.KillProcessTree(externalPid); } catch { } }
                    ReleaseServer();
                    serverReady = false;
                    starting = false;
                    StartServer();
                }
                MessageBox.Show(detail, "清理归档会话", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            catch (Exception ex)
            {
                MessageBox.Show("清理失败：" + ex.Message, Program.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        // 在浏览器中打开局域网地址（本机验证用）
        internal void OpenLanInBrowser()
        {
            if (lanPlainUrl.Length == 0) return;
            try { Process.Start(lanPlainUrl); } catch { }
        }

        // QR 码页面（单文件 HTML + qrcode.js CDN，WebView2 渲染）
        internal static string LanQrHtml(string url)
        {
            // 二维码页面（v3 修复）：
            //  - WebView2 已强制 --force-device-scale-factor=1（CSS 像素=物理像素），容器固定尺寸一定放得下
            //  - #qr-wrapper 固定 220x220，flex-shrink:0，overflow:visible !important；min() 兜底防极端视口
            //  - #qr 固定 200x200 + overflow:hidden，canvas/img 绝对定位重叠（qrcode.js 会同时生成两者，避免堆叠撑高）
            //  - 生成后 setTimeout 强制 canvas/img 全尺寸 200px、display:block、transform:none、object-fit:contain
            //  - 覆盖全局 CSS 的 max-width:100%、transform:scale、zoom 干扰；qrcode.js 参数严格 200
            string html =
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\">" +
                "<style>" +
                "html,body{margin:0;padding:0;background:#ffffff;height:100%;overflow:hidden;}" +
                "body{font-family:'Segoe UI','Microsoft YaHei',sans-serif;display:flex;flex-direction:column;" +
                "align-items:center;justify-content:flex-start;}" +
                "#qr-wrapper{width:min(220px,calc(100vw - 8px));height:220px;" +
                "flex:none;flex-shrink:0;margin:4px auto 2px;overflow:visible !important;" +
                "display:flex;align-items:center;justify-content:center;background:#ffffff;}" +
                "#qr-wrapper *{max-width:none !important;max-height:none !important;transform:none !important;" +
                "zoom:1 !important;scale:none !important;}" +
                "#qr{width:200px;height:200px;position:relative;overflow:hidden;flex:none;background:#ffffff;}" +
                "#qr canvas,#qr img{position:absolute;top:0;left:0;display:block !important;" +
                "width:200px !important;height:200px !important;min-width:200px !important;min-height:200px !important;" +
                "max-width:200px !important;max-height:200px !important;object-fit:contain !important;}" +
                "#u{font-size:11px;line-height:14px;height:14px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis;color:#333;padding:0 8px;text-align:center;max-width:230px;}" +
                "</style></head><body>" +
                "<div id=\"qr-wrapper\"><div id=\"qr\"></div></div>" +
                "<div id=\"u\"></div>" +
                "<script src=\"https://cdn.jsdelivr.net/npm/qrcodejs@1.0.0/qrcode.min.js\"></script>" +
                "<script>" +
                "var url=" + JsonEscape(url) + ";" +
                "document.getElementById('u').textContent=url;" +
                "function forceSize(){" +
                "try{var els=document.querySelectorAll('#qr canvas,#qr img');" +
                "for(var i=0;i<els.length;i++){var el=els[i];" +
                "el.style.display='block';el.style.width='200px';el.style.height='200px';" +
                "el.style.minWidth='200px';el.style.minHeight='200px';" +
                "el.style.maxWidth='200px';el.style.maxHeight='200px';" +
                "el.style.transform='none';el.style.objectFit='contain';}}catch(e){}}" +
                "function draw(){try{" +
                "var q=new QRCode(document.getElementById('qr'),{text:url,width:200,height:200," +
                "colorDark:'#111827',colorLight:'#ffffff',correctLevel:QRCode.CorrectLevel.M});" +
                "setTimeout(forceSize,60);setTimeout(forceSize,300);" +
                "document.getElementById('u').textContent='扫码访问 '+url;" +
                "}catch(e){forceSize();document.getElementById('u').textContent=url;}}" +
                "if(typeof QRCode!=='undefined'){draw();}else{" +
                "var s1=document.createElement('script');" +
                "s1.src='https://cdn.jsdelivr.net/npm/qrcodejs@1.0.0/qrcode.min.js';" +
                "s1.onload=draw;s1.onerror=function(){var s2=document.createElement('script');" +
                "s2.src='https://cdnjs.cloudflare.com/ajax/libs/qrcodejs/1.0.0/qrcode.min.js';" +
                "s2.onload=draw;s2.onerror=function(){" +
                "document.getElementById('u').textContent='离线模式：请手动输入 '+url;};" +
                "document.head.appendChild(s2);};" +
                "document.head.appendChild(s1);}</script>" +
                "</body></html>";
            return html;
        }

        private static string JsonEscape(string s)
        {
            if (s == null) return "\"\"";
            StringBuilder sb = new StringBuilder();
            sb.Append('"');
            foreach (char ch in s)
            {
                if (ch == '"' || ch == '\\') sb.Append('\\').Append(ch);
                else if (ch == '\n') sb.Append("\\n");
                else if (ch == '\r') sb.Append("\\r");
                else if (ch < 32) sb.Append("\\u").Append(((int)ch).ToString("x4"));
                else sb.Append(ch);
            }
            sb.Append('"');
            return sb.ToString();
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
                Log("服务未运行，请先通过托盘菜单“启动服务”。");
                return;
            }
            string url = !string.IsNullOrEmpty(AuthenticatedUrl)
                ? AuthenticatedUrl
                : "http://127.0.0.1:" + port + "/";
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
                    return;
                }
                Log("未找到 Edge，改用默认浏览器打开。");
                Process.Start(url);
                Log("已在浏览器中打开 " + url);
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
            // 清理孤儿局域网网关（LAN 关闭时确保手机无法访问）
            CleanupOrphanGateway();
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
                    StopLanGateway();
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
                    StopLanGateway();
                    killedAny = true;
                    Log("已停止。");
                }
                else
                {
                    Log("服务保持后台运行，下次打开本程序会自动识别并接管。");
                }
            }
            else if (forceExit && (serverRunning || extRunning))
            {
                // 托盘「退出」：默认保留服务后台运行（网页端不中断），下次打开自动识别接管；
                // 如需退出时停止，先用托盘「停止服务」，或在设置中关闭窗口时选择“是”。
                if (exitKillService)
                {
                    if (serverRunning) { try { Engine.KillProcessTree(server); killedAny = true; } catch { } }
                    if (extRunning) { try { Engine.KillProcessTree(externalPid); killedAny = true; } catch { } }
                    StopLanGateway();
                }
                else
                {
                    Log("服务保持后台运行，下次打开本程序会自动识别并接管。");
                }
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
        private TextBox txtPort, txtWork, txtLanPort, txtLanPin;
        private Button btnBrowseWork, btnSave, btnGenPin, btnFwElevated, btnCopyCmd, btnCopyUrl, btnOpenLan;
        private CheckBox chkTray, chkLan;
        private Label lblLanStatus, lblPinSrc, lblFwStatus, lblOllama;
        private TextBox txtLanUrl, txtManual;
        private TextBox txtLog;
        private Button btnDoc, btnLogDir, btnAbout, btnClose, btnCleanArch;
        private WebView2 lanQr;                 // 二维码（qrcode.js 渲染到 canvas）
        private bool qrReady = false;
        private string qrShownUrl = "";
        private System.Windows.Forms.Timer refreshTimer;

        public SettingsForm(LauncherForm host)
        {
            this.host = host;
            this.ShowInTaskbar = false;
            this.Text = Program.AppName + " 设置";
            this.ClientSize = new Size(680, 780);
            this.MinimumSize = new Size(640, 710);
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
            txtWork.SetBounds(82, 56, 380, 26);
            txtWork.Text = host.UiWorkDir;

            btnBrowseWork = new Button();
            btnBrowseWork.SetBounds(468, 56, 92, 26);
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
            btnSave.SetBounds(564, 92, 96, 30);
            btnSave.Text = "保存设置";
            btnSave.Click += delegate
            {
                host.CommitSettings(txtPort.Text.Trim(), txtWork.Text.Trim(), chkTray.Checked);
                // 保存后同步回显示（端口校验失败时由宿主复位）
                txtPort.Text = host.UiPortText;
                txtWork.Text = host.UiWorkDir;
                RefreshLanPanel();
            };

            // 运行日志
            Label lLog = new Label();
            lLog.SetBounds(16, 134, 100, 20);
            lLog.Text = "运行日志:";

            txtLog = new TextBox();
            txtLog.SetBounds(16, 156, 648, 120);
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

            // ==================== 局域网共享面板 ====================
            // 用 Panel（客户区无标题偏移，坐标精确可控），避免 GroupBox 客户区下移导致控件超出被裁剪。
            // 布局预算：Panel 高 320，内容最大底 316；手动命令区独立置于 Panel 下方。
            Panel pLan = new Panel();
            pLan.SetBounds(12, 290, 656, 340);
            pLan.BorderStyle = BorderStyle.FixedSingle;
            pLan.BackColor = Color.FromArgb(252, 252, 252);

            chkLan = new CheckBox();
            chkLan.SetBounds(16, 8, 320, 20);
            chkLan.Text = "允许局域网访问（默认关闭）";
            chkLan.Checked = host.UiLanEnabled;

            Label lLanPort = new Label();
            lLanPort.SetBounds(16, 32, 44, 22);
            lLanPort.Text = "端口:";
            lLanPort.TextAlign = ContentAlignment.MiddleLeft;

            txtLanPort = new TextBox();
            txtLanPort.SetBounds(60, 32, 58, 26);
            txtLanPort.Text = host.UiLanPortText;

            lblLanStatus = new Label();
            lblLanStatus.SetBounds(128, 32, 500, 22);
            lblLanStatus.ForeColor = Color.FromArgb(70, 70, 70);

            txtLanUrl = new TextBox();
            txtLanUrl.SetBounds(16, 62, 472, 26);
            txtLanUrl.ReadOnly = true;
            txtLanUrl.BackColor = Color.White;

            btnCopyUrl = new Button();
            btnCopyUrl.SetBounds(496, 62, 68, 26);
            btnCopyUrl.Text = "复制地址";
            btnCopyUrl.Click += delegate { Clipboard.SetText(txtLanUrl.Text); };

            btnOpenLan = new Button();
            btnOpenLan.SetBounds(572, 62, 68, 26);
            btnOpenLan.Text = "打开";
            btnOpenLan.Click += delegate { host.OpenLanInBrowser(); };

            // 二维码（WebView2 + qrcode.js；控件需大于 QR 页面内容 220 容器 + 边距）
            lanQr = new WebView2();
            lanQr.SetBounds(16, 94, 232, 226);
            lanQr.DefaultBackgroundColor = Color.White;

            // PIN（右侧，x=262 起）
            Label lblPin = new Label();
            lblPin.SetBounds(262, 92, 160, 18);
            lblPin.Text = "访问密码 (PIN):";

            txtLanPin = new TextBox();
            txtLanPin.SetBounds(262, 112, 120, 24);
            txtLanPin.UseSystemPasswordChar = true;

            btnGenPin = new Button();
            btnGenPin.SetBounds(390, 112, 100, 24);
            btnGenPin.Text = "重新生成";
            btnGenPin.Click += delegate
            {
                string pin = host.RegenerateLanPin();
                txtLanPin.Text = "";
                RefreshLanPanel();
                MessageBox.Show("新的访问 PIN 已生成：" + pin + "\n\n手机扫码后输入该 PIN 即可访问。",
                    "PIN 已更新", MessageBoxButtons.OK, MessageBoxIcon.Information);
            };

            lblPinSrc = new Label();
            lblPinSrc.SetBounds(262, 140, 380, 16);
            lblPinSrc.ForeColor = Color.Gray;
            lblPinSrc.Font = new Font("Microsoft YaHei UI", 8.5f);

            Label lblPinHint = new Label();
            lblPinHint.SetBounds(262, 156, 380, 32);
            lblPinHint.Text = "留空 = 自动生成；也可在 .env 中配置 DSH_LAN_PIN（环境变量优先）。";
            lblPinHint.ForeColor = Color.Gray;
            lblPinHint.Font = new Font("Microsoft YaHei UI", 8.5f);

            // 防火墙
            Label lblFwTitle = new Label();
            lblFwTitle.SetBounds(262, 192, 56, 18);
            lblFwTitle.Text = "防火墙:";
            lblFwTitle.TextAlign = ContentAlignment.MiddleLeft;

            lblFwStatus = new Label();
            lblFwStatus.SetBounds(318, 192, 320, 18);
            lblFwStatus.ForeColor = Color.FromArgb(70, 70, 70);

            btnFwElevated = new Button();
            btnFwElevated.SetBounds(262, 212, 200, 24);
            btnFwElevated.Text = "以管理员身份配置防火墙";
            btnFwElevated.Click += delegate
            {
                host.TryFirewallElevated();
                RefreshLanPanel();
            };

            lblOllama = new Label();
            lblOllama.SetBounds(262, 240, 380, 30);
            lblOllama.ForeColor = Color.FromArgb(120, 100, 40);
            lblOllama.Font = new Font("Microsoft YaHei UI", 8.5f);
            // 文案由 RefreshLanPanel 按 LAN 开关状态动态设置（此处为初始占位）
            lblOllama.Text = "";

            // 事件
            chkLan.CheckedChanged += delegate
            {
                bool ok = host.CommitLanSettings(chkLan.Checked, txtLanPort.Text.Trim(), txtLanPin.Text);
                if (!ok) chkLan.Checked = host.UiLanEnabled;
                RefreshLanPanel();
            };
            txtLanPort.Leave += delegate
            {
                if (chkLan.Checked && txtLanPort.Text.Trim() != host.UiLanPortText)
                {
                    host.CommitLanSettings(true, txtLanPort.Text.Trim(), txtLanPin.Text);
                    RefreshLanPanel();
                }
            };
            // PIN 输入失焦即保存（无论 LAN 开关状态）：自定义 PIN 必须可靠生效
            txtLanPin.Leave += delegate
            {
                if (txtLanPin.Text.Trim() != host.UiLanPin)
                {
                    host.CommitLanSettings(chkLan.Checked, txtLanPort.Text.Trim(), txtLanPin.Text);
                    RefreshLanPanel();
                }
            };

            pLan.Controls.Add(chkLan);
            pLan.Controls.Add(lLanPort);
            pLan.Controls.Add(txtLanPort);
            pLan.Controls.Add(lblLanStatus);
            pLan.Controls.Add(txtLanUrl);
            pLan.Controls.Add(btnCopyUrl);
            pLan.Controls.Add(btnOpenLan);
            pLan.Controls.Add(lanQr);
            pLan.Controls.Add(lblPin);
            pLan.Controls.Add(txtLanPin);
            pLan.Controls.Add(btnGenPin);
            pLan.Controls.Add(lblPinSrc);
            pLan.Controls.Add(lblPinHint);
            pLan.Controls.Add(lblFwTitle);
            pLan.Controls.Add(lblFwStatus);
            pLan.Controls.Add(btnFwElevated);
            pLan.Controls.Add(lblOllama);

            // 手动命令（无管理员权限时的备用方案，独立置于面板下方）
            Label lblManualTitle = new Label();
            lblManualTitle.SetBounds(12, 646, 460, 18);
            lblManualTitle.Text = "防火墙手动命令（自动配置失败时，管理员执行）：";

            txtManual = new TextBox();
            txtManual.SetBounds(12, 666, 492, 62);
            txtManual.Multiline = true;
            txtManual.ReadOnly = true;
            txtManual.BackColor = Color.FromArgb(245, 245, 245);
            txtManual.ForeColor = Color.FromArgb(60, 60, 60);
            txtManual.Font = new Font("Consolas", 8.5f);
            txtManual.ScrollBars = ScrollBars.Vertical;
            txtManual.WordWrap = false;

            btnCopyCmd = new Button();
            btnCopyCmd.SetBounds(512, 682, 130, 28);
            btnCopyCmd.Text = "复制手动命令";
            btnCopyCmd.Click += delegate { Clipboard.SetText(txtManual.Text); };

            // 底部
            btnDoc = new Button();
            btnDoc.SetBounds(16, 746, 96, 30);
            btnDoc.Text = "使用文档";
            btnDoc.Click += delegate { GuideForm.ShowGuide(this); };

            btnLogDir = new Button();
            btnLogDir.SetBounds(120, 746, 116, 30);
            btnLogDir.Text = "打开日志目录";
            btnLogDir.Click += delegate { host.OpenLogDir(); };

            btnCleanArch = new Button();
            btnCleanArch.SetBounds(352, 746, 140, 30);
            btnCleanArch.Text = "清理归档会话";
            btnCleanArch.Click += delegate { host.CleanArchivedSessions(); };

            btnAbout = new Button();
            btnAbout.SetBounds(244, 746, 100, 30);
            btnAbout.Text = "关于此程序";
            btnAbout.Click += delegate { host.ShowAbout(); };

            btnClose = new Button();
            btnClose.SetBounds(564, 746, 96, 30);
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
                        this.Controls.Add(pLan);
            this.Controls.Add(lblManualTitle);
            this.Controls.Add(txtManual);
            this.Controls.Add(btnCopyCmd);
            this.Controls.Add(btnDoc);
            this.Controls.Add(btnLogDir);
            this.Controls.Add(btnCleanArch);
            this.Controls.Add(btnAbout);
            this.Controls.Add(btnClose);

            // 日志实时显示
            host.LogLine += OnLogLine;
            this.FormClosed += delegate
            {
                try { host.LogLine -= OnLogLine; } catch { }
                try { if (refreshTimer != null) refreshTimer.Stop(); } catch { }
            };

            // 局域网状态定时刷新（网关/防火墙状态变化时同步界面）
            refreshTimer = new System.Windows.Forms.Timer();
            refreshTimer.Interval = 1500;
            refreshTimer.Tick += delegate { RefreshLanPanel(); };
            refreshTimer.Start();

            RefreshLanPanel();
            InitQrWebView2();
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
            RefreshLanPanel();
        }

        // ---------------- 局域网面板刷新 + 二维码 ----------------
        private void RefreshLanPanel()
        {
            if (IsDisposed) return;
            try
            {
                if (InvokeRequired)
                {
                    BeginInvoke((Action)(delegate { RefreshLanPanel(); }));
                    return;
                }
                chkLan.Checked = host.UiLanEnabled;
                // 正在编辑中的输入框不刷新，避免用户输入被定时器覆盖丢失（自定义 PIN/端口必须可靠提交）
                if (!txtLanPort.Focused) txtLanPort.Text = host.UiLanPortText;
                if (!txtLanPin.Focused) txtLanPin.Text = host.UiLanPin;
                lblLanStatus.Text = host.UiLanStatus;
                txtLanUrl.Text = host.UiLanPlainUrl;
                lblPinSrc.Text = "生效来源: " + host.UiLanPinSource;
                lblFwStatus.Text = host.UiFirewallStatus;
                int lp = 3081;
                try { lp = int.Parse(host.UiLanPortText); } catch { }
                txtManual.Text = LanAccess.ManualAddCommand(lp) + "\r\n" + LanAccess.ManualRemoveCommand(lp);
                // Ollama 提示按 LAN 开关动态显示（OLLAMA_HOST 仅在开启局域网时才会被设置）
                if (host.UiOllamaDetected)
                {
                    lblOllama.Visible = true;
                    lblOllama.Text = host.UiLanEnabled
                        ? "检测到 Ollama：局域网已开启，dsh 进程已设置 OLLAMA_HOST=0.0.0.0、OLLAMA_ORIGINS=*。"
                        : "检测到 Ollama：开启局域网访问后，将为 dsh 进程自动设置 OLLAMA_HOST=0.0.0.0、OLLAMA_ORIGINS=*。";
                }
                else
                {
                    lblOllama.Visible = false;
                }
                ReloadQr();
            }
            catch { }
        }

        // 二维码 WebView2 环境缓存（同进程复用同一 user-data-folder，窗体重建不重复创建）
        private static CoreWebView2Environment cachedQrEnv = null;
        private static readonly object cachedQrEnvLock = new object();

        private void InitQrWebView2()
        {
            try
            {
                string profile = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "DSHLauncher", "webview2-profile-settings");
                CoreWebView2EnvironmentOptions opt = new CoreWebView2EnvironmentOptions();
                opt.AdditionalBrowserArguments = "--force-device-scale-factor=1";

                TaskScheduler ui = TaskScheduler.FromCurrentSynchronizationContext();
                Task<CoreWebView2Environment> envTask;
                lock (cachedQrEnvLock)
                {
                    envTask = cachedQrEnv != null ? Task.FromResult(cachedQrEnv) : null;
                }
                if (envTask == null)
                {
                    envTask = CoreWebView2Environment.CreateAsync(null, profile, opt);
                    envTask.ContinueWith(delegate(Task<CoreWebView2Environment> t)
                    {
                        if (t.Status == TaskStatus.RanToCompletion)
                        {
                            lock (cachedQrEnvLock) { if (cachedQrEnv == null) cachedQrEnv = t.Result; }
                        }
                    });
                }
                envTask.ContinueWith(
                    delegate(Task<CoreWebView2Environment> t)
                    {
                        if (t.IsFaulted || t.IsCanceled) return;
                        CoreWebView2Environment env = t.Result;
                        lanQr.EnsureCoreWebView2Async(env).ContinueWith(delegate(Task t2)
                        {
                            if (t2.IsFaulted || t2.IsCanceled) return;
                            qrReady = true;
                            try { lanQr.CoreWebView2.Settings.AreDefaultContextMenusEnabled = false; } catch { }
                            ReloadQr();
                        }, ui);
                    }, ui);
            }
            catch { }
        }

        private void ReloadQr()
        {
            string url = host.UiLanPlainUrl;
            if (url.Length == 0 || !qrReady || lanQr.CoreWebView2 == null) return;
            if (url == qrShownUrl) return;
            qrShownUrl = url;
            try { lanQr.CoreWebView2.NavigateToString(LauncherForm.LanQrHtml(url)); } catch { }
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
            this.MinimumSize = new Size(1000, 600); // 最小宽度 1000，避免触发 dsh 移动端响应式布局
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

        // WebView2 环境缓存：同一进程对同一 user-data-folder 重复 CreateAsync 会失败，
        // 窗体重建时复用已创建的环境（static 跨实例共享）
        private static CoreWebView2Environment cachedEnv = null;
        private static readonly object cachedEnvLock = new object();

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
                Task<CoreWebView2Environment> envTask;
                lock (cachedEnvLock)
                {
                    envTask = cachedEnv != null ? Task.FromResult(cachedEnv) : null;
                }
                if (envTask == null)
                {
                    envTask = CoreWebView2Environment.CreateAsync(null, profile, opt);
                    envTask.ContinueWith(delegate(Task<CoreWebView2Environment> t)
                    {
                        if (t.Status == TaskStatus.RanToCompletion)
                        {
                            lock (cachedEnvLock) { if (cachedEnv == null) cachedEnv = t.Result; }
                        }
                    });
                }
                envTask.ContinueWith(
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
                                InjectDesktopLayout();
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

        // 注入桌面端布局强制（force-desktop）：WebView2 环境或宽视口（>900px）下强制显示侧边栏，
        // 避免 DPI 缩放 / 窄窗口触发 dsh 前端的移动端响应式布局（侧边栏被隐藏）
        private void InjectDesktopLayout()
        {
            try
            {
                string js =
                    "(function(){" +
                    "var IS_WEBVIEW2=!!(window.chrome&&window.chrome.webview);" +
                    "var css='body.force-desktop aside,body.force-desktop [class*=\"sidebar\" i]," +
                    "body.force-desktop [class*=\"rail\" i],body.force-desktop [class*=\"drawer\" i]" +
                    "{display:flex!important;transform:none!important;visibility:visible!important;" +
                    "opacity:1!important;pointer-events:auto!important;max-width:none!important}';" +
                    "var st=document.createElement('style');st.textContent=css;" +
                    "(document.head||document.documentElement).appendChild(st);" +
                    "function apply(){var force=IS_WEBVIEW2||window.innerWidth>900;" +
                    "if(document.body){if(force)document.body.classList.add('force-desktop');" +
                    "else document.body.classList.remove('force-desktop');}}" +
                    "if(document.body)apply();" +
                    "document.addEventListener('DOMContentLoaded',apply);" +
                    "window.addEventListener('resize',apply);" +
                    "var iv=setInterval(function(){if(document.body)apply();},1500);" +
                    "setTimeout(function(){clearInterval(iv);},120000);" +
                    "})();";
                wv.CoreWebView2.AddScriptToExecuteOnDocumentCreatedAsync(js);
            }
            catch { }
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
            // 3 位十六进制（如 #fff），每位扩展为两位
            m = Regex.Match(css, "#([0-9a-fA-F]{3})(?![0-9a-fA-F])");
            if (m.Success)
            {
                string h = m.Groups[1].Value;
                int val = Convert.ToInt32(
                    h.Substring(0, 1) + h.Substring(0, 1) +
                    h.Substring(1, 1) + h.Substring(1, 1) +
                    h.Substring(2, 1) + h.Substring(2, 1), 16);
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
            "· 没装也没关系：一键安装包会帮你部署，或在设置文件 settings.ini 中手动指定 nodePath。\n" +
            "\n" +
            "──────────────────────────────\n" +
            "四、常见问题\n" +
            "──────────────────────────────\n" +
            "· 打不开 http://127.0.0.1:3080 → 先通过托盘菜单\"启动服务\"，等状态变\"运行中\"；\n" +
            "· 卡在\"启动中\"或提示残留进程无响应 → 会自动清理上次遗留的 Harness 进程并重新启动，\n" +
            "  或再点一次\"启动服务\"触发自动清理；服务挂起时也会自动重启；\n" +
            "· 显示\"运行中（已接管）\" → 说明已有 Harness 在跑，直接点停止/打开界面；\n" +
            "· 嫌浏览器占内存 → 默认用\"内嵌窗口\"打开界面（无需浏览器，像原生软件一样）；\n" +
            "  WebView2 不可用时自动回退 Edge 精简窗口，两者都比完整浏览器省内存；\n" +
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
        // 写自检报告：日志路径不可写时静默跳过（避免 catch 内再次抛出导致崩溃对话框）
        private static void WriteReport(string logPath, List<string> report)
        {
            try { File.WriteAllLines(logPath, report.ToArray(), new UTF8Encoding(true)); }
            catch { }
        }

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
                    WriteReport(logPath, report);
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
                // 局域网共享资源与探测检查
                string gwPath = LanAccess.WriteGateway();
                report.Add("lan-gateway.mjs: " + (gwPath.Length > 0 ? "已释放 ✓" : "FAIL: 资源缺失"));
                // PWA 图标资源检查（二维码/手机端图标依赖）
                string iconB64 = LauncherForm.LoadWhaleIconB64();
                report.Add("whale-256.png: " + (iconB64.Length > 0 ? "已内嵌 ✓" : "FAIL: 资源缺失"));
                string lanIpT; string lanNameT; bool lanWT;
                lanIpT = LanAccess.DetectLanIp(out lanNameT, out lanWT);
                report.Add("局域网 IP 探测: " + (lanIpT.Length > 0 ? lanIpT + "（" + lanNameT + "）" : "未检测到活动 WiFi/以太网"));
                report.Add("PIN 解析: " + (LanAccess.EffectivePin(out lanNameT).Length > 0 ? "可用" : "将自动生成"));
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
                WriteReport(logPath, report);
                return (ready && freed && identityOk) ? 0 : 1;
            }
            catch (Exception ex)
            {
                report.Add("异常: " + ex.ToString());
                WriteReport(logPath, report);
                return 1;
            }
            finally
            {
                // 清理自检临时工作目录，避免残留
                try { Directory.Delete(Path.Combine(Path.GetTempPath(), "dsh-launcher-selftest"), true); } catch { }
            }
        }

        private static void AppendTrim(StringBuilder sb, string line)
        {
            // 自检报告同样脱敏：dsh 输出含一次性 token URL，不能明文写入 selftest.log
            sb.AppendLine(Engine.RedactSecrets(Engine.Sanitize(line)));
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
