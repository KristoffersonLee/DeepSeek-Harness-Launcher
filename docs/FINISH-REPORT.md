# DSHLauncher v5.0.0 LTS 收尾报告

- 版本 / Version：5.0.0（发布标签 / label：LTS）
- 执行时间：2026-09-12 01:28:11
- 工作目录：D:\DSHLauncher
- PowerShell：7.6.6

0. 进程创建: OK
0. 残留进程: DSHLauncher=0, msedgewebview2=6, node=2
   无本仓库路径下的启动器进程（不触碰其他位置的实例）
1. 清理后: DSHLauncher=0, 本项目 WebView2=0
2. PASS: DSHLauncher.exe 1,087,488 B, FileVersion=5.0.0, ProductVersion=5.0.0
   SHA256 = 4FB5D89985E0348C5DA146A02A6919F584864A63AD42B10194A98A23E6AF316D
3. PASS: DSHLauncherSetup.exe 1,641,472 B, FileVersion=5.0.0.0, ProductName=DeepSeek Harness Launcher
   SHA256 = 93CA189F30D07DD21BB6263F6D7EAEA254B0E598756493E58D29212C9DD9EA1C
4a. fmt: PASS
5. clippy: PASS
6. tests: 8 个套件通过 / 0 个失败（cargo 退出码 0，编译错误行 0）
6b. target 体积: 1,939.0 MB；构建目录 D:\DSHLauncher\target: 1,939.0 MB（回收用 tools\clean.ps1 -Cache）
7. consistency: PASS
8. version-chain: PASS
8b. doc-facts: PASS
9. 工作集       : 27,684 KB
   专用内存     : 4,700 KB
   峰值工作集   : 27,684 KB
   线程数       : 12
   CPU(空闲3s)  : 0 ms
   [2026-09-12 01:26:26] [INFO] [boot] 服务启动请求已提交（后台线程）: 15ms
   [2026-09-12 01:26:26] [INFO] [boot] 事件循环首帧（界面可响应）: 15ms
   [2026-09-12 01:26:26] [INFO] [boot] 服务就绪: 116ms
   [2026-09-12 01:30:01] [INFO] [boot] 单实例+配置+日志就绪: 3ms
   [2026-09-12 01:30:01] [INFO] [boot] 事件循环创建完成: 9ms
   [2026-09-12 01:30:01] [INFO] [boot] 服务启动请求已提交（后台线程）: 15ms
   [2026-09-12 01:30:01] [INFO] [boot] 事件循环首帧（界面可响应）: 15ms
   [2026-09-12 01:30:01] [INFO] [boot] 服务就绪: 119ms
   （启动器保持运行，便于直接观察）
   优雅退出：D:\DSHLauncher\DSHLauncher.exe --quit（等待最多 15s）
   回退强杀 PID 20900（D:\DSHLauncher\DSHLauncher.exe）—— 优雅退出后在 15s 内未退出
10. e2e: PASS
11a. cargo-audit: PASS（无漏洞）
11d. cargo-machete: PASS（无未使用依赖）

## 结果汇总

```
0. 进程创建: OK
0. 残留进程: DSHLauncher=0, msedgewebview2=6, node=2
1. 清理后: DSHLauncher=0, 本项目 WebView2=0
2. PASS: DSHLauncher.exe 1,087,488 B, FileVersion=5.0.0, ProductVersion=5.0.0
3. PASS: DSHLauncherSetup.exe 1,641,472 B, FileVersion=5.0.0.0, ProductName=DeepSeek Harness Launcher
4a. fmt: PASS
5. clippy: PASS
6. tests: 8 个套件通过 / 0 个失败（cargo 退出码 0，编译错误行 0）
6b. target 体积: 1,939.0 MB；构建目录 D:\DSHLauncher\target: 1,939.0 MB（回收用 tools\clean.ps1 -Cache）
7. consistency: PASS
8. version-chain: PASS
8b. doc-facts: PASS
9. 工作集       : 27,684 KB
10. e2e: PASS
11a. cargo-audit: PASS（无漏洞）
11d. cargo-machete: PASS（无未使用依赖）
```

## 失败清单

- 无

- 汇总：失败 0 项（cargo/脚本退出码、异常、编译错误均计入）
