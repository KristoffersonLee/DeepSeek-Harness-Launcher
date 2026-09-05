// LanAccess.cs — DSHLauncher 局域网共享辅助层
// 职责: 局域网 IP 探测 / PIN 解析(.env 与环境变量，禁止硬编码) / Windows 防火墙规则
//       / 内嵌 lan-gateway.mjs 资源释放 / Ollama 检测
// 兼容 C# 5（系统自带 csc v4.0.30319），勿使用字符串插值 / ?. / out var 等新语法。
// 依赖: System.dll / System.Core.dll / System.Windows.Forms（Application.ExecutablePath）
//       / System.Net.NetworkInformation

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Reflection;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows.Forms;

namespace DSHLauncher
{
    internal static class LanAccess
    {
        // workspace.json 归档列表与归档 ID 的正则（多处共用，保持一致）
        private static readonly Regex ArchivedListRegex = new Regex(@"""archivedSessionIds""\s*:\s*\[([^\]]*)\]");
        private static readonly Regex ArchivedIdRegex = new Regex(@"""session-[a-f0-9-]+""");
        // 健康探测超时（毫秒）
        private const int HttpProbeTimeoutMs = 900;

        // ---------------------------------------------------------------------
        // 局域网 IP 探测：优先 WiFi / 以太网，排除虚拟网卡（VPN/TAP/VMware/蓝牙等）
        // 注意：仅识别 Ethernet / Wireless80211 两种枚举值，个别驱动上报
        //       FastEthernet / GigabitEthernet / FastEthernetFx 会被跳过（属罕见情况）。
        // ---------------------------------------------------------------------
        private static readonly string[] VirtualMarkers = new string[]
        {
            "virtual", "vmware", "virtualbox", "hyper-v", "vethernet", "vpn", "tap-",
            "tap ", "tun", "wsl", "loopback", "bluetooth", "蓝牙", "npcap", "nordvpn",
            "wireguard", "zerotier", "hamachi", "tailscale", "utun", "ppp", "teredo",
            "isatap", "kali", "sandbox", "docker", "vmnet"
        };

        private static bool IsVirtualAdapter(string name)
        {
            if (string.IsNullOrEmpty(name)) return true;
            string n = name.ToLowerInvariant();
            foreach (string m in VirtualMarkers)
            {
                if (n.IndexOf(m, StringComparison.Ordinal) >= 0) return true;
            }
            return false;
        }

        /// <summary>返回活动 WiFi/以太网的 IPv4 地址；找不到返回 ""。</summary>
        public static string DetectLanIp(out string adapterName, out bool wireless)
        {
            adapterName = "";
            wireless = false;
            string bestIp = null;
            string bestName = "";
            bool bestWireless = false;
            int bestScore = -1;
            try
            {
                foreach (NetworkInterface ni in NetworkInterface.GetAllNetworkInterfaces())
                {
                    try
                    {
                        if (ni.OperationalStatus != OperationalStatus.Up) continue;
                        NetworkInterfaceType t = ni.NetworkInterfaceType;
                        bool isWireless = (t == NetworkInterfaceType.Wireless80211);
                        bool isEth = (t == NetworkInterfaceType.Ethernet);
                        if (!isWireless && !isEth) continue; // 跳过环回/隧道/PPP 等
                        string name = (ni.Name ?? "") + " " + (ni.Description ?? "");
                        if (IsVirtualAdapter(name)) continue;
                        IPInterfaceProperties props = ni.GetIPProperties();
                        bool hasGateway = props.GatewayAddresses != null && props.GatewayAddresses.Count > 0;
                        foreach (UnicastIPAddressInformation a in props.UnicastAddresses)
                        {
                            if (a.Address == null || a.Address.AddressFamily != AddressFamily.InterNetwork) continue;
                            string ip = a.Address.ToString();
                            if (ip.StartsWith("169.254") || ip.StartsWith("127.")) continue; // APIPA / 环回
                            // 评分：有网关的连接优先；WiFi 略优先于以太网（手机一般连 WiFi）
                            int score = (hasGateway ? 8 : 0) + (isWireless ? 2 : 1);
                            if (score > bestScore)
                            {
                                bestScore = score;
                                bestIp = ip;
                                bestName = name;
                                bestWireless = isWireless;
                            }
                        }
                    }
                    catch { }
                }
            }
            catch { }
            adapterName = bestName;
            wireless = bestWireless;
            return bestIp ?? "";
        }

        /// <summary>探测某 IP:端口是否可绑定（被占用返回 true）。</summary>
        public static bool IsLanPortInUse(string ip, int port)
        {
            TcpListener l = null;
            try
            {
                l = new TcpListener(IPAddress.Parse(ip), port);
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

        /// <summary>探测局域网网关上是否已有实例在运行（接管旧会话）。</summary>
        public static bool IsGatewayRunning(string ip, int port)
        {
            try
            {
                HttpWebRequest req = (HttpWebRequest)WebRequest.Create("http://" + ip + ":" + port + "/__lan/health");
                req.Timeout = HttpProbeTimeoutMs;
                req.ReadWriteTimeout = HttpProbeTimeoutMs;
                // 局域网探测必须直连，避免走 IE/系统代理（Clash 等代理可能误判网关存在/不存在）
                req.Proxy = null;
                using (HttpWebResponse resp = (HttpWebResponse)req.GetResponse())
                {
                    return resp.StatusCode == HttpStatusCode.OK;
                }
            }
            catch (Exception)
            {
                return false;
            }
        }

        // ---------------------------------------------------------------------
        // PIN / Token 解析：环境变量 DSH_LAN_PIN → 启动器目录 .env → %APPDATA%.env
        // → 自动生成并持久化到 lan-pin.txt（禁止硬编码）
        // ---------------------------------------------------------------------
        // 注意：AppDataDir 使用 ApplicationData（漫游配置文件）。域环境下 PIN/Token/Secret
        // 文件会同步到其他机器，但 PIN 仅在本地网关有效，实际风险可控。
        // 如需更高安全性，可迁移到 LocalApplicationData（需处理现有安装的迁移）。
        public static string AppDataDir
        {
            get
            {
                return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "DSHLauncher");
            }
        }

        public static string LauncherDir
        {
            get { return Path.GetDirectoryName(Application.ExecutablePath); }
        }

        public static string PinFilePath
        {
            get { return Path.Combine(AppDataDir, "lan-pin.txt"); }
        }

        public static string TokenFilePath
        {
            get { return Path.Combine(AppDataDir, "lan-token.txt"); }
        }

        public static string SecretFilePath
        {
            get { return Path.Combine(AppDataDir, "lan-secret.txt"); }
        }

        /// <summary>删除会话签名密钥：PIN 变更后调用，使所有旧 Cookie 失效，手机端必须重新输入新 PIN。</summary>
        public static void DeleteSecret()
        {
            try { if (File.Exists(SecretFilePath)) File.Delete(SecretFilePath); } catch { }
        }

        /// <summary>读取 dsh 归档会话数量（%USERPROFILE%\.dsh\storages\workspace.json）。</summary>
        public static int ArchivedSessionCount()
        {
            try
            {
                string ws = WorkspaceJsonPath();
                if (!File.Exists(ws)) return 0;
                string raw = File.ReadAllText(ws, Encoding.UTF8);
                Match m = ArchivedListRegex.Match(raw);
                if (!m.Success) return 0;
                return ArchivedIdRegex.Matches(m.Groups[1].Value).Count;
            }
            catch { return 0; }
        }

        private static string WorkspaceJsonPath()
        {
            return Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                ".dsh", "storages", "workspace.json");
        }

        /// <summary>彻底删除全部归档会话：删除磁盘会话目录并从 workspace.json 归档列表移除。</summary>
        public static int DeleteArchivedSessions(out string detail)
        {
            detail = "";
            int deleted = 0;
            try
            {
                string ws = WorkspaceJsonPath();
                if (!File.Exists(ws)) { detail = "未找到 workspace.json（可能没有归档会话）"; return 0; }
                string raw = File.ReadAllText(ws, Encoding.UTF8);
                Match m = ArchivedListRegex.Match(raw);
                if (!m.Success) { detail = "workspace.json 无归档会话字段"; return 0; }
                List<string> ids = new List<string>();
                // 注意：正则本身不含捕获组，匹配值（含引号）取 im.Value 再去引号即为 sessionId
                foreach (Match im in ArchivedIdRegex.Matches(m.Groups[1].Value))
                {
                    ids.Add(im.Value.Trim('"'));
                }
                if (ids.Count == 0) { detail = "没有归档会话"; return 0; }
                // 删除磁盘上的会话目录（sessions/<工作区目录>/<sessionId>/）
                // 注意：Directory.Delete 递归删除对 reparse point（符号链接/junction）会按目录处理，
                //       会话目录位于用户可写区，理论上存在被替换为链接的风险（低）；
                //       如需更严格可先检查 ReparsePoint 属性再删。
                string sessionsDir = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".dsh", "sessions");
                if (Directory.Exists(sessionsDir))
                {
                    foreach (string wd in Directory.GetDirectories(sessionsDir))
                    {
                        foreach (string sd in Directory.GetDirectories(wd))
                        {
                            string name = Path.GetFileName(sd);
                            if (ids.Contains(name))
                            {
                                try { Directory.Delete(sd, true); deleted++; }
                                catch { }
                            }
                        }
                    }
                }
                // 清空归档列表（归档会话已被删除）
                string newRaw = raw.Replace(m.Value, "\"archivedSessionIds\": []");
                File.WriteAllText(ws, newRaw, new UTF8Encoding(false));
                detail = "已彻底删除 " + deleted + " / " + ids.Count + " 个归档会话（其余可能被占用，建议重启服务后重试）。";
            }
            catch (Exception ex)
            {
                detail = "清理失败：" + ex.Message;
            }
            return deleted;
        }

        public static string GatewayLogPath
        {
            get
            {
                return Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "DSHLauncher", "logs", "lan-gateway.log");
            }
        }

        public static Dictionary<string, string> ParseEnvFile(string path)
        {
            Dictionary<string, string> d = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            try
            {
                if (!File.Exists(path)) return d;
                foreach (string raw in File.ReadAllLines(path, Encoding.UTF8))
                {
                    string line = raw.Trim();
                    if (line.Length == 0 || line.StartsWith("#") || line.StartsWith(";")) continue;
                    int eq = line.IndexOf('=');
                    if (eq < 0) continue;
                    string key = line.Substring(0, eq).Trim();
                    // 去行内注释（#/;），去首尾空白与成对引号：
                    // 形如 DSH_LAN_PIN="123456" 或 DSH_LAN_PIN=123456 # 注释 都能正确解析
                    string val = StripInlineComment(line.Substring(eq + 1)).Trim().Trim('"').Trim('\'');
                    if (key.Length > 0) d[key] = val;
                }
            }
            catch { }
            return d;
        }

        /// <summary>剥离 .env 行内注释（引号内不剥离）。</summary>
        private static string StripInlineComment(string v)
        {
            bool inQuote = false;
            for (int i = 0; i < v.Length; i++)
            {
                char c = v[i];
                if (c == '"' || c == '\'') inQuote = !inQuote;
                else if (!inQuote && (c == '#' || c == ';')) return v.Substring(0, i);
            }
            return v;
        }

        /// <summary>解析生效 PIN 及其来源说明（用于面板展示）。</summary>
        public static string EffectivePin(out string source)
        {
            source = "";
            try
            {
                string envPin = Environment.GetEnvironmentVariable("DSH_LAN_PIN");
                if (!string.IsNullOrEmpty(envPin))
                {
                    source = "环境变量 DSH_LAN_PIN";
                    return envPin;
                }
                string launcherEnv = Path.Combine(LauncherDir, ".env");
                Dictionary<string, string> d1 = ParseEnvFile(launcherEnv);
                string v;
                if (d1.TryGetValue("DSH_LAN_PIN", out v) && !string.IsNullOrEmpty(v))
                {
                    source = "启动器目录 .env";
                    return v;
                }
                string appEnv = Path.Combine(AppDataDir, ".env");
                Dictionary<string, string> d2 = ParseEnvFile(appEnv);
                if (d2.TryGetValue("DSH_LAN_PIN", out v) && !string.IsNullOrEmpty(v))
                {
                    source = "%APPDATA%\\DSHLauncher\\.env";
                    return v;
                }
                if (File.Exists(PinFilePath))
                {
                    string s = File.ReadAllText(PinFilePath, Encoding.UTF8).Trim();
                    if (s.Length > 0) { source = "启动器自动生成"; return s; }
                }
            }
            catch { }
            return "";
        }

        /// <summary>生成并持久化一个新的 6 位数字 PIN（加密安全随机，避免 Random() 时间种子可预测）。</summary>
        public static string GeneratePin()
        {
            StringBuilder sb = new StringBuilder(6);
            using (System.Security.Cryptography.RandomNumberGenerator rng =
                System.Security.Cryptography.RandomNumberGenerator.Create())
            {
                byte[] buf = new byte[1];
                for (int i = 0; i < 6; i++)
                {
                    int v;
                    do
                    {
                        rng.GetBytes(buf);
                        v = buf[0] % 10;
                    } while (buf[0] >= 250); // 拒绝高位字节，消除模偏差（0-9 均等概率）
                    sb.Append((char)('0' + v));
                }
            }
            string pin = sb.ToString();
            try
            {
                Directory.CreateDirectory(AppDataDir);
                File.WriteAllText(PinFilePath, pin, new UTF8Encoding(false));
            }
            catch { }
            return pin;
        }

        /// <summary>把用户自定义 PIN 持久化到 lan-pin.txt。</summary>
        public static void SavePin(string pin)
        {
            try
            {
                Directory.CreateDirectory(AppDataDir);
                File.WriteAllText(PinFilePath, pin.Trim(), new UTF8Encoding(false));
            }
            catch { }
        }

        // ---------------------------------------------------------------------
        // 内嵌 lan-gateway.mjs 资源释放（构建时 /resource 嵌入）
        // ---------------------------------------------------------------------
        public static string WriteGateway()
        {
            try
            {
                Assembly asm = Assembly.GetExecutingAssembly();
                string resName = null;
                foreach (string n in asm.GetManifestResourceNames())
                {
                    if (n.EndsWith("lan-gateway.mjs", StringComparison.OrdinalIgnoreCase)) { resName = n; break; }
                }
                if (resName == null) return "";
                Directory.CreateDirectory(AppDataDir);
                string dest = Path.Combine(AppDataDir, "lan-gateway.mjs");
                using (Stream s = asm.GetManifestResourceStream(resName))
                {
                    if (s == null) return "";
                    using (Stream fs = File.Create(dest))
                    {
                        byte[] buf = new byte[65536];
                        int n;
                        while ((n = s.Read(buf, 0, buf.Length)) > 0) fs.Write(buf, 0, n);
                    }
                }
                return dest;
            }
            catch (Exception)
            {
                return "";
            }
        }

        // ---------------------------------------------------------------------
        // Windows 防火墙：remoteip=localsubnet，仅放行局域网
        // ---------------------------------------------------------------------
        public static string RuleName(int port)
        {
            return "DSHLauncher LAN " + port;
        }

        private static string NetshPath
        {
            get
            {
                string s = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "netsh.exe");
                return File.Exists(s) ? s : "netsh.exe";
            }
        }

        private static string RunCommand(string file, string args, out int exitCode, bool elevated)
        {
            exitCode = -1;
            StringBuilder outp = new StringBuilder();
            try
            {
                ProcessStartInfo psi = new ProcessStartInfo(file, args);
                psi.UseShellExecute = false;
                psi.CreateNoWindow = true;
                psi.RedirectStandardOutput = true;
                psi.RedirectStandardError = true;
                // netsh 管道输出为 UTF-8：不设置编码会按 ANSI/GBK 解码，中文（如“已启用”）变乱码导致匹配失败
                psi.StandardOutputEncoding = Encoding.Default;
                psi.StandardErrorEncoding = Encoding.Default;
                // elevated 分支已移除：所有调用方均传 elevated=false，
                // 提权操作由 TryAddRuleElevated 通过独立 PowerShell 脚本完成
                using (Process p = Process.Start(psi))
                {
                    outp.Append(p.StandardOutput.ReadToEnd());
                    outp.Append(p.StandardError.ReadToEnd());
                    p.WaitForExit(15000);
                    exitCode = p.ExitCode;
                }
            }
            catch (Exception ex)
            {
                // 用户取消 UAC 或启动失败
                outp.Append(ex.Message);
            }
            return outp.ToString();
        }

        public static bool HasRule(int port)
        {
            try
            {
                int code;
                string outp = RunCommand(NetshPath,
                    "advfirewall firewall show rule name=\"" + RuleName(port) + "\"", out code, false);
                if (code != 0) return false;
                // 解析“已启用: 是”/“Enabled: Yes”键值，避免把“已禁用”误判为存在规则
                return Regex.IsMatch(outp, @"(?im)^\s*(enabled|已启用)\s*[:：]\s*(yes|是)\s*$");
            }
            catch (Exception)
            {
                return false;
            }
        }

        /// <summary>尝试添加防火墙规则（非提权）。返回 0=成功 1=权限不足 2=其它失败。</summary>
        public static int TryAddRule(int port, out string message)
        {
            message = "";
            int code;
            string outp = RunCommand(NetshPath,
                "advfirewall firewall add rule name=\"" + RuleName(port) + "\" dir=in action=allow "
                + "protocol=TCP localport=" + port + " remoteip=localsubnet", out code, false);
            string lower = outp.ToLowerInvariant();
            if (code == 0) { message = "防火墙规则已添加（仅限本地子网访问）。"; return 0; }
            if (lower.IndexOf("提升", StringComparison.Ordinal) >= 0
                || lower.IndexOf("elevat", StringComparison.Ordinal) >= 0
                || lower.IndexOf("access denied", StringComparison.Ordinal) >= 0
                || lower.IndexOf("拒绝访问", StringComparison.Ordinal) >= 0)
            {
                message = "需要管理员权限（非提权添加被拒绝）。";
                return 1;
            }
            message = "netsh 返回: " + outp.Trim();
            return 2;
        }

        /// <summary>提权（UAC）添加防火墙规则。以独立 PowerShell 窗口执行脚本并保持窗口显示结果，避免命令一闪而过。</summary>
        public static bool TryAddRuleElevated(int port)
        {
            string script = WriteFirewallScript(port);
            if (script.Length == 0) return false;
            return RunFirewallElevated(script);
        }

        /// <summary>把 netsh 防火墙命令写入随机命名的临时 ps1 脚本（解决命令行引号嵌套问题，Read-Host 保持窗口）。
        /// 脚本执行完毕后自删，防止被同用户进程替换导致提权（TOCTOU）。</summary>
        private static string WriteFirewallScript(int port)
        {
            try
            {
                Directory.CreateDirectory(AppDataDir);
                // 随机文件名：同用户下的恶意进程无法预测路径，降低“预置脚本 → 等 UAC 提权执行”的风险
                string path = Path.Combine(AppDataDir,
                    "firewall-add-" + Guid.NewGuid().ToString("N").Substring(0, 8) + ".ps1");
                string rule = RuleName(port);
                StringBuilder sb = new StringBuilder();
                sb.AppendLine("$ErrorActionPreference = 'Continue'");
                sb.AppendLine("Write-Host 'DeepSeek Harness Launcher：正在添加局域网防火墙规则（仅限本地子网）...' -ForegroundColor Cyan");
                sb.AppendLine("Write-Host ''");
                // 先删除同名规则避免重复堆积，再添加一条干净的规则
                sb.AppendLine("netsh advfirewall firewall delete rule name=\"" + rule + "\"");
                sb.AppendLine("netsh advfirewall firewall add rule name=\"" + rule + "\" dir=in action=allow protocol=TCP localport=" + port + " remoteip=localsubnet");
                sb.AppendLine("Write-Host ''");
                sb.AppendLine("if ($LASTEXITCODE -eq 0) {");
                sb.AppendLine("    Write-Host '成功：防火墙规则已添加（仅限本地子网访问）。' -ForegroundColor Green");
                sb.AppendLine("} else {");
                sb.AppendLine("    Write-Host '失败：请确认以管理员身份运行，或检查上方 netsh 输出。' -ForegroundColor Red");
                sb.AppendLine("}");
                sb.AppendLine("Write-Host ''");
                sb.AppendLine("Read-Host '按 Enter 键关闭窗口'");
                sb.AppendLine("Remove-Item -LiteralPath $MyInvocation.MyCommand.Path -Force -ErrorAction SilentlyContinue");
                File.WriteAllText(path, sb.ToString(), new UTF8Encoding(true));
                return path;
            }
            catch (Exception)
            {
                return "";
            }
        }

        /// <summary>提权启动 PowerShell 执行防火墙脚本（不阻塞等待；用户在新窗口查看结果后按 Enter 关闭）。</summary>
        private static bool RunFirewallElevated(string script)
        {
            try
            {
                ProcessStartInfo psi = new ProcessStartInfo();
                psi.FileName = "powershell.exe";
                // 使用单引号包裹路径，防止用户名含 $() 等 PowerShell 特殊字符时被解释为脚本块
                psi.Arguments = "-NoProfile -ExecutionPolicy Bypass -File '" + script.Replace("'", "''") + "'";
                psi.UseShellExecute = true;
                psi.Verb = "runas"; // 触发 UAC，提升为新进程
                Process p = Process.Start(psi);
                if (p != null) { try { p.Dispose(); } catch { } }
                return true; // 已发起提权，用户在窗口中查看结果
            }
            catch (Exception)
            {
                return false; // 用户取消 UAC 或启动失败
            }
        }

        /// <summary>尝试删除防火墙规则。返回 0=成功 1=权限不足 2=其它失败。</summary>
        public static int TryRemoveRule(int port, out string message)
        {
            message = "";
            int code;
            string outp = RunCommand(NetshPath,
                "advfirewall firewall delete rule name=\"" + RuleName(port) + "\"", out code, false);
            string lower = outp.ToLowerInvariant();
            if (code == 0) { message = "防火墙规则已删除。"; return 0; }
            if (lower.IndexOf("提升", StringComparison.Ordinal) >= 0
                || lower.IndexOf("elevat", StringComparison.Ordinal) >= 0
                || lower.IndexOf("access denied", StringComparison.Ordinal) >= 0
                || lower.IndexOf("拒绝访问", StringComparison.Ordinal) >= 0)
            {
                message = "需要管理员权限（非提权删除被拒绝）。";
                return 1;
            }
            message = "netsh 返回: " + outp.Trim();
            return 2;
        }

        /// <summary>面板显示的手动命令（自动配置失败时）。</summary>
        public static string ManualAddCommand(int port)
        {
            return "netsh advfirewall firewall add rule name=\"" + RuleName(port)
                + "\" dir=in action=allow protocol=TCP localport=" + port
                + " remoteip=localsubnet";
        }

        public static string ManualRemoveCommand(int port)
        {
            return "netsh advfirewall firewall delete rule name=\"" + RuleName(port) + "\"";
        }

        // ---------------------------------------------------------------------
        // Ollama 检测（若使用 Ollama 作为本地推理后端）
        // ---------------------------------------------------------------------
        public static bool IsOllamaInstalled()
        {
            try
            {
                string pathVar = Environment.GetEnvironmentVariable("PATH") ?? "";
                foreach (string dir in pathVar.Split(';'))
                {
                    string d = dir.Trim().Trim('"');
                    if (d.Length == 0) continue;
                    if (File.Exists(Path.Combine(d, "ollama.exe"))) return true;
                }
                string local = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "Programs", "Ollama");
                if (Directory.Exists(local) && File.Exists(Path.Combine(local, "ollama.exe"))) return true;
            }
            catch { }
            return false;
        }
    }
}
