# DSHLauncher v5.0.0 LTS 收尾报告

- 版本 / Version：5.0.0（发布标签 / label：LTS）
- 执行时间：2026-09-12 01:57:47
- 工作目录：D:\DSHLauncher
- PowerShell：7.6.6

0. 进程创建: OK
0. 残留进程: DSHLauncher=0, msedgewebview2=12, node=2
   无本仓库路径下的启动器进程（不触碰其他位置的实例）
1. 清理后: DSHLauncher=0, 本项目 WebView2=0
2. PASS: DSHLauncher.exe 1,087,488 B, FileVersion=5.0.0, ProductVersion=5.0.0
   SHA256 = 9EC7FD963307D84C56A43C6AC4CDDCA5E8B8D0640F12F501C8F11B6C0EDE01A8
3. PASS: DSHLauncherSetup.exe 1,641,472 B, FileVersion=5.0.0.0, ProductName=DeepSeek Harness Launcher
   SHA256 = D457AB108C997BF55ACBC99526C80DD6654E8B5A3975FBF17E1FD3DB1C6674B0
3b. PASS: 安装包内嵌 6 项资源与当前产物逐字节一致
4a. fmt: PASS
5. clippy: PASS
6. tests: 8 个套件通过 / 0 个失败（cargo 退出码 0，编译错误行 0）
6b. target 体积: 1,939.9 MB；构建目录 D:\DSHLauncher\target: 1,939.9 MB（回收用 tools\clean.ps1 -Cache）
7. consistency: PASS
8. version-chain: PASS
8b. doc-facts: PASS
9. 工作集       : 22,568 KB
   专用内存     : 3,656 KB
   峰值工作集   : 22,568 KB
   线程数       : 12
   CPU(空闲3s)  : 0 ms
   [2026-09-12 01:57:04] [INFO] [boot] 服务启动请求已提交（后台线程）: 12ms
   [2026-09-12 01:57:04] [INFO] [boot] 事件循环首帧（界面可响应）: 12ms
   [2026-09-12 01:57:04] [INFO] [boot] 服务就绪: 122ms
   [2026-09-12 01:59:12] [INFO] [boot] 单实例+配置+日志就绪: 3ms
   [2026-09-12 01:59:12] [INFO] [boot] 事件循环创建完成: 6ms
   [2026-09-12 01:59:12] [INFO] [boot] 服务启动请求已提交（后台线程）: 11ms
   [2026-09-12 01:59:12] [INFO] [boot] 事件循环首帧（界面可响应）: 11ms
   [2026-09-12 01:59:12] [INFO] [boot] 服务就绪: 123ms
   （启动器保持运行，便于直接观察）
   优雅退出：D:\DSHLauncher\DSHLauncher.exe --quit（等待最多 15s）
   优雅退出成功（未使用强杀）
10. e2e: PASS
11a. cargo-audit: PASS（无漏洞）
11d. cargo-machete: PASS（无未使用依赖）

## 结果汇总

```
0. 进程创建: OK
0. 残留进程: DSHLauncher=0, msedgewebview2=12, node=2
1. 清理后: DSHLauncher=0, 本项目 WebView2=0
2. PASS: DSHLauncher.exe 1,087,488 B, FileVersion=5.0.0, ProductVersion=5.0.0
3. PASS: DSHLauncherSetup.exe 1,641,472 B, FileVersion=5.0.0.0, ProductName=DeepSeek Harness Launcher
3b. PASS: 安装包内嵌 6 项资源与当前产物逐字节一致
4a. fmt: PASS
5. clippy: PASS
6. tests: 8 个套件通过 / 0 个失败（cargo 退出码 0，编译错误行 0）
6b. target 体积: 1,939.9 MB；构建目录 D:\DSHLauncher\target: 1,939.9 MB（回收用 tools\clean.ps1 -Cache）
7. consistency: PASS
8. version-chain: PASS
8b. doc-facts: PASS
9. 工作集       : 22,568 KB
10. e2e: PASS
11a. cargo-audit: PASS（无漏洞）
11d. cargo-machete: PASS（无未使用依赖）
```

## 失败清单

- 无

- 汇总：失败 0 项（cargo/脚本退出码、异常、编译错误均计入）
