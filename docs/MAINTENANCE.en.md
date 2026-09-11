# Upgrade & Maintenance Manual (English)

> This document is the maintenance manual for the **DSHLauncher** repository (standalone; ships with the project and is deployed into the install directory by the installer).
> It covers installing, upgrading, verifying, and troubleshooting **dsh** (`@deepseek-ai/dsh`, the DeepSeek Harness CLI/service) and **DSHLauncher**.
> The body is generic (machine-independent); the appendices keep the publisher machine's snapshot and upgrade history for reference.
> Official dsh is still under fast preview/rc/alpha iteration and may introduce **breaking changes** (port/protocol/interface, config format, working-directory layout, etc.). Read sections 3, 4 and 7 before upgrading.
> **As of launcher v4.1.0, a "Fix Modules" button and health check are built in** — use them first when encountering native module issues.
>
> ⚠️ **As of v5.0.0 LTS the launcher is a full Rust rewrite** (see [`RELEASE_NOTES_v5.0.0.md`](RELEASE_NOTES_v5.0.0.md) and [`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md)). What changed for this manual:
> - **LAN sharing is fully removed** (the related chapter and troubleshooting entries no longer apply);
> - runtime dependencies dropped from .NET Framework + side-by-side DLLs to **WebView2 Runtime only**; the artifact is now a **single-file exe**;
> - the build script moved from `csc.exe` to `cargo build --offline`;
> - **the "Fix Modules" button is gone in v5** (its `dsh-core` upgrade/health APIs were deleted as dead code too) — handle native-module issues with the manual `node-gyp rebuild` steps in §1.5;
> - new: **loss detection / auto-reconnect** and **orphan lock recovery** (see §1.6).
> - **The release label is 5.0.0 LTS**: check the installed build with `DSHLauncher.exe --version`
>   (prints `DSHLauncher 5.0.0 LTS (release)`) and list every switch plus the exit-code contract with
>   `--help`. The Cargo version remains the legal semver `5.0.0` — `LTS` is never part of the version string.

## Manual TOC

1. [Overview](#11-overview)
2. [Requirements & Path Conventions](#12-requirements--path-conventions)
3. [Installing & Upgrading dsh](#13-installing--upgrading-dsh)
4. [Post-Upgrade Verification Checklist](#14-post-upgrade-verification-checklist)
5. [Common Failures & Fixes](#15-common-failures--fixes)
6. [Launcher Behavior](#16-launcher-behavior)
7. [Maintenance Rules & Pitfalls](#17-maintenance-rules--pitfalls)
8. [Appendix A: Publisher Machine Snapshot (2026-09-01)](#18-appendix-a-publisher-machine-snapshot-2026-09-01)
9. [Appendix B: Version Tracking & Breaking Changes](#19-appendix-b-version-tracking--breaking-changes)
10. [Appendix C: Publisher Upgrade History](#110-appendix-c-publisher-upgrade-history)

### 1.1 Overview

- **dsh**: DeepSeek Harness's Node.js service and CLI. The web UI listens at `http://127.0.0.1:3080/` by default. Installed globally via npm (`@deepseek-ai/dsh`).
- **DSHLauncher**: a Windows desktop shell. Double-click to auto-start `dsh web` and display Harness in an embedded **WebView2** window (no browser needed), with tray, settings, logging, auto-adopt, and self-healing.

### 1.2 Requirements & Path Conventions

**Requirements**

| Component | Requirement |
|---|---|
| OS | Windows 10 / 11 (64-bit) |
| Node.js | Official LTS or newer (DSHLauncher starts dsh with the system Node) |
| npm | Official latest; **npm 12 blocks un-whitelisted install/postinstall scripts by default** (see pitfall in 1.7) |
| WebView2 Runtime | Usually ships with Edge; the launcher auto-deploys it or falls back to Edge |

**Path conventions (generic)**

| Purpose | Path (generic) | Note |
|---|---|---|
| npm global prefix | `npm config get prefix` (usually `%APPDATA%\npm` on Windows) | dsh installs at `<prefix>\node_modules\@deepseek-ai\dsh` |
| dsh user data | `%USERPROFILE%\.dsh\` | sessions (`sessions\`), config (`settings.yaml`), credentials (`.credentials.yaml`), plugins (`profiles\`) |
| Launcher log | `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` | first stop when troubleshooting |
| Launcher settings | `%APPDATA%\DSHLauncher\settings.ini` | port / working dir / tray behavior |
| Web UI | `http://127.0.0.1:3080/` | default port; **requires a one-time token since 0.1.2-alpha** (see 1.6) |

> Conventions: `<npm-prefix>` is the output of `npm config get prefix`; `<dsh-version>` is the target version.

### 1.3 Installing & Upgrading dsh

> May be executed by an AI agent following this manual, or manually with the commands below. Both are equivalent.

**Determine the target version (always first)**

```powershell
npm view @deepseek-ai/dsh dist-tags --json   # latest / next / alpha tags
npm view @deepseek-ai/dsh versions --json    # all published versions
```

- The GitHub releases page (`deepseek-ai/deepseek-harness`) may publish before npm; **npm availability is authoritative**.
- alpha/rc are prereleases: read the release notes of the target version and assess **breaking changes** (see Appendix B).

**Install / upgrade command**

```powershell
# 0) Confirm the npm prefix (must match the one the launcher uses)
npm config get prefix

# 1) Install a specific version (explicit version + anti-corrupt cache + allow native build scripts; do not omit)
npm install -g "@deepseek-ai/dsh@<dsh-version>" --no-audit --no-fund --prefer-online --registry=https://registry.npmjs.org/ `
  --allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
```

| Flag | Purpose |
|---|---|
| `@deepseek-ai/dsh@<version>` | **Always pin an explicit version** to avoid tag drift |
| `--prefer-online` | bypass a corrupted local npm cache tarball |
| `--registry=https://registry.npmjs.org/` | **Enforce official registry**, immune to local `.npmrc` mirror config |
| `--allow-scripts=...` | **required on npm 12**: allow native build scripts for fs-ext (file lock), koffi (FFI), node-pty (terminal), etc.; otherwise you get a half-installed package (Failure B/G in 1.5) |
| `--no-audit --no-fund` | faster, quieter |

If the prefix/permissions are wrong: use the system npm with an explicit prefix, e.g.
`"C:\Program Files\nodejs\npm.cmd" install -g "@deepseek-ai/dsh@<version>" --prefix "<npm-prefix>" --no-audit --no-fund --prefer-online --allow-scripts=...`.
If the default npm-cache reports EPERM, add `--cache <writable-directory>`.

**Pre-install preparation (mandatory)**

- **You MUST stop the dsh web service before installing**, otherwise the running process locks files (e.g. `koffi.node`), causing install failures (EBUSY) or half-built directories:
  ```powershell
  # Find running dsh processes
  Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' }
  # Stop (if any)
  Stop-Process -Id <PID> -Force
  ```
- Let the install **finish completely** (about 3–5 minutes); **do not kill processes mid-install** (interruptions once caused deadlocked workers and half-built directories).
- Upgrading only changes files on disk; a running dsh web is unaffected. **Restart the launcher afterwards** to load the new version.

**⚠️ Special case: upgrading remotely via an AI Agent inside DSH**

When a user chats with an AI Agent (like this one) through the DSH Web UI and asks the Agent to upgrade dsh, a paradox arises:
- The Agent runs inside the dsh process
- The manual requires stopping dsh before upgrading
- Stopping dsh = killing the Agent's own conversation channel

**Resolution:**
1. The Agent should **NOT execute** `Stop-Process` itself; instead, it should output the full upgrade commands for the user to run manually in an external terminal.
2. The user runs stop → install → verify → restart in PowerShell / cmd.
3. After the upgrade, the Agent can continue the conversation via the new dsh version.

Example output template:
```
Since I run inside DSH, I cannot stop my own process. Please run these commands manually in a terminal:

# 1. Stop dsh
Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force

# 2. Install new version
npm install -g "@deepseek-ai/dsh@<version>" --no-audit --no-fund --prefer-online --registry=https://registry.npmjs.org/ --allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs

# 3. Verify
dsh --version

# 4. Restart dsh
dsh web
```

**Post-install cleanup**

- Failed installs may leave `dsh-broken-*`, `dsh-partial-*`, `dsh-spill-*`, `dsh-subprocess-*` directories in `%TEMP%` (hundreds of MB each); clean up manually:
  ```powershell
  Get-ChildItem "$env:TEMP" -Directory | Where-Object { $_.Name -like 'dsh-*' } | Remove-Item -Recurse -Force
  ```
- After a failed install, corrupted tarballs may remain in the npm cache; add `--prefer-online` on the next install to bypass the cache.

### 1.4 Post-Upgrade Verification Checklist

Verify **every item** after install/reinstall:

> ⚠️ **Version note (0.1.5-alpha.1+)**: fs-ext was a native module introduced by 0.1.3-alpha.2 and **removed by upstream in 0.1.5-alpha.1** (session file locks now use `@deepseek-ai/node-addon-system`: POSIX `flock(2)` / Windows kernel semaphore; no local compilation needed). For targets ≥ 0.1.5-alpha.1, **skip item 4 and the fs-ext table row** — `Cannot find module 'fs-ext'` is expected, not Failure G. fs-ext verification applies only to 0.1.3-alpha.2–0.1.4.

```powershell
# 1) Version
dsh --version

# 2) Usage (exercises bin.js -> dsh-app-boot -> commander/js-yaml loading chain)
dsh --help

# 3) Plugin config tree (validates YAML parsing and full plugin-tree loading; normally 500+ lines, no error/mismatch)
dsh --profile web --dump-config

# 4) fs-ext native module loadable (critical! auto-checked by v4.1.0+ health probe)
node -e "const f=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/fs-ext'); console.log(typeof f.flock)"

# 5) koffi native version match (critical!)
node -e "const k=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"

# 6) node-pty loadable
node -e "const p=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/node-pty'); console.log(typeof p.spawn)"
```

| Check | Expected |
|---|---|
| `dsh --version` | matches the installed version |
| fs-ext | prints `function`; `Cannot find module 'fs_ext'` means missing compilation |
| koffi | prints a version consistent with the JS wrapper (e.g. `3.1.6`); `Mismatched native Koffi modules` means a broken install |
| node-pty | prints `function` |
| `dump-config` | no `error` / `mismatch` / `failed to` |
| Key files | under `<npm-prefix>\node_modules\@deepseek-ai\dsh\`: `package.json`, `lib\bin.js`, `node_modules\commander\index.js`, `node_modules\js-yaml\dist\js-yaml.mjs`, `node_modules\fs-ext\build\Release\fs_ext.node`, `node_modules\@koromix\koffi-win32-x64\win32_x64\koffi.node` |
| Leftovers | `<npm-prefix>\node_modules\@deepseek-ai\` should normally contain only `dsh` (see Failure E in 1.5) |
| Web UI | embedded window renders correctly after launcher restart (token since 0.1.2-alpha, see 1.6) |

### 1.5 Common Failures & Fixes

**Failure A: Missing modules (js-yaml / commander)**

- **Symptom**: `dsh` reports a missing module; the launcher cannot start.
- **Cause**: interrupted install or wrong prefix, leaving a half-built directory.
- **Fix**: delete the install directory and reinstall completely (1.3), let it finish; if the prefix was wrong, reinstall with an explicit `--prefix`.

**Failure B: koffi native mismatch (Mismatched native Koffi modules)**

- **Symptom**: the launcher throws `Mismatched native Koffi modules` when loading the `subprocess`/`sandbox` plugins, exit code 1 — the launcher "won't open".
- **Cause**: npm 12's allow-scripts policy blocked koffi's build scripts → JS and native binaries out of sync.
- **Fix**: reinstall completely with `--allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` (1.3), then verify koffi (1.4). **Never try to replace a single .node file.**
- **Built-in fix**: v4.1.0+ can use the "Fix Modules" button in the upgrade page for one-click rebuild.

**Failure C: Session encoding mismatch (uses .jsonl, but this backend is configured for compression "zstd")**

- **Symptom**: the launcher log reports `encodingMismatch`, exit code 1; `dsh --version` / `dump-config` work fine.
- **Cause**: since 0.1.1-rc.2 the session backend defaults to `compression: zstd`; a session directory that **mixes encodings** (plaintext `session.jsonl` alongside `session.jsonl.zstd`) crashes initialization. **Reinstalling does not help** (the default stays zstd).
- **Fix (unify the root to zstd)**:
  1. Locate conflicts: count plaintext vs zstd files under `%USERPROFILE%\.dsh\sessions`;
  2. For each conflicting directory: first decode the first zstd frame to confirm the session id matches and the full session is there (proving zstd is authoritative); **back up the plaintext `session.jsonl` OUTSIDE `.dsh`**, then delete the plaintext, keeping only `session.jsonl.zstd`;
  3. ⚠️ **Never back up inside `.dsh`** (dsh scans that tree as session roots, and a plaintext backup retriggers the same crash);
  4. Confirm zero plaintext files under `.dsh\sessions`, then restart the launcher.
- **Alternative**: if all sessions are plaintext and you want to keep them, append `- id: session-persistence-jsonl / config: { compression: none }` to `%USERPROFILE%\.dsh\profiles\web\cordis.patch.yml` (only when there are no zstd sessions).

**Failure D: One-time token auth on the Web UI since 0.1.2-alpha (401)**

- **Symptom**: `http://127.0.0.1:3080/` returns **401**; the embedded window shows `dsh web authentication required; reopen the URL printed by dsh web`; the log shows `dsh web: http://127.0.0.1:3080/?token=…` (different every launch).
- **Cause**: since 0.1.2-alpha, `dsh-client-connection` enforces **one-time token auth** on the Web UI (a new token per launch; after first visit it exchanges for a 30-day browser-session cookie). There is **no config switch to disable it**.
- **Fix**: use the token URL printed by `dsh web`; **DSHLauncher v2.0+ captures that URL automatically** — nothing manual needed. Upgrade old launchers.
- **Key points**: the token is one-time and changes every launch; the browser-session cookie lasts 30 days by default, so reopening the launcher auto-adopts (see 1.6) without re-authentication; if the cookie expires and you get 401, restart the service once.

**Failure E: Launcher won't open / unresponsive**

1. Stop the launcher and confirm the crashed process exited (`Get-NetTCPConnection -LocalPort 3080`); use `Get-CimInstance Win32_Process` to inspect command lines and **do not kill unrelated node processes** (e.g. other tools' MCP/agent).
2. Clean leftover temp directories:
   ```powershell
   Get-ChildItem "<npm-prefix>\node_modules\@deepseek-ai" -Force   # should normally contain only dsh
   Remove-Item "<leftover-path>" -Recurse -Force                    # e.g. .dsh-* (contains sharp DLLs)
   ```
   Leftovers locked by a running launcher can be deleted **after restarting the launcher**.
3. Delete the broken install (`Remove-Item "<npm-prefix>\node_modules\@deepseek-ai\dsh" -Recurse -Force`), reinstall completely (1.3), verify (1.4), then restart the launcher.

**Failure EBUSY: Install fails (file locked)**

- **Symptom**: `npm install -g @deepseek-ai/dsh@<version>` reports `EBUSY: resource busy or locked` or `EEXIST: file already exists`, install aborts.
- **Cause**: A running dsh web process has locked files such as `koffi.node`; npm cannot overwrite them.
- **Fix**:
  1. **Stop the dsh process first**:
     ```powershell
     Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force
     ```
  2. Clean residual directories (`%TEMP%\dsh-broken-*`, `dsh-partial-*`, `dsh-spill-*`, `dsh-subprocess-*`).
  3. Reinstall (add `--prefer-online` to bypass cache):
     ```powershell
     npm install -g "@deepseek-ai/dsh@<version>" --no-audit --no-fund --prefer-online --registry=https://registry.npmjs.org/ --allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
     ```
  4. Verify with the checklist in section 1.4.

**Failure G: fs-ext native module missing (Cannot find module 'fs_ext') — applies only to 0.1.3-alpha.2–0.1.4; upstream removed fs-ext in 0.1.5-alpha.1, so this error then means a wrong version or broken install**

- **Symptom**: starting dsh web reports `Cannot find module 'fs_ext'` or `Module did not self-register: '...fs_ext.node'`; launcher log shows `Error: Cannot find module 'fs_ext'`.
- **Cause (three layers)**:
  1. **Compilation never ran**: npm 11.19.0+ allow-scripts whitelist blocked fs-ext's build script → `build/Release/fs_ext.node` never existed.
  2. **ABI mismatch**: rebuild used a different Node version (e.g. Node 22 → ABI 127) than the one dsh actually runs on (e.g. Node 26 → ABI 147) → cannot load.
  3. **Node 26 thin-LTO pitfall**: Node 26's `common.gypi` enables clang-style thin-LTO (`-flto=thin`) for Windows by default; MSVC linker reports `LNK1117`.
- **Fix**:
  1. **Built-in fix** (v4.1.0+): click the "Fix Modules" button in the upgrade page; auto-locates npm global dir and runs `node-gyp rebuild`.
  2. **Manual fix**:
     ```powershell
     $fsExt = "$(npm root -g)\node_modules\@deepseek-ai\dsh\node_modules\fs-ext"
     cd $fsExt
     node-gyp rebuild
     ```
  3. **If LNK1117 persists**: modify Node 26 header cache (`%LOCALAPPDATA%\node-gyp\Cache\26.7.0\...\common.gypi`) to disable `enable_thin_lto=="true"`, then rebuild.
- **Prevention**: launcher v4.1.0+ includes `fs-ext` in the allow-scripts whitelist of its `npm install -g` command, preventing future installs from skipping compilation.

**Failure F: Common false alarms (not problems)**

| Phenomenon | Note |
|---|---|
| `UNMET OPTIONAL DEPENDENCY @img/sharp-*` (darwin/linux/freebsd) in `npm ls -g` | not installed on Windows; normal |
| `EPERM` writing `cordis.yml` | usually another instance holds the port / sandbox restrictions; not a corrupt config |
| `atomic-write: timed out waiting for the writer lock at …\.dsh\*.lock` | orphan lock left by a **force-killed** dsh (the lock file contains the owner PID). **The v5 launcher cleans it automatically before starting the service** (only when that PID is really gone; a live lock is never touched); for manual handling see §1.5 |
| `npm warn cleanup Failed to remove .dsh-*` | temp dir locked by a running process; deletable after restart |

### 1.6 Launcher Behavior

> The following describes **v5.0.0 (Rust)**; differences from v4 are noted.

- **Embedded window**: WebView2 renders Harness (no browser); falls back to a lightweight Edge window, then the default browser.
- **Tray menu**: open UI / refresh / open in browser / start service / stop service / guide / log folder / settings / about / quit.
- **Token-auth adaptation (v2.0+)**: the launcher captures the `/?token=…` URL from `dsh web` output and navigates to it;
  when adopting an **externally started** instance (or on older dsh without that output) it falls back to the plain URL
  (relying on the persisted login cookie).
- **Service lifecycle (v2.0+; semantics changed in the v5.0.0 finalisation round — important)**:
  - **By default the service is independent of the launcher**
    (`service_lifecycle = "independent"` in `settings.toml`): dsh is **not** attached to the
    launcher's Job Object, so quitting / crashing / being replaced by an installer / signing out
    **never interrupts a running session**. The next launch reconciles
    `%LOCALAPPDATA%\DSHLauncher\service.json` and **re-adopts** it automatically.
  - If you want the old "kill everything when the launcher dies" semantics, uncheck
    *service is independent of the launcher* in Settings (`service_lifecycle = "tied"`): dsh is then
    attached to the job and a force-killed launcher reaps it in the kernel.
  - To stop the service: tray "Stop service", or choose "Yes" in the exit prompt;
  - Closing a feature window (or pressing `Esc`) follows the *minimize to tray* preference:
    when checked the window is **hidden** (page state preserved, instant restore from the tray),
    otherwise it is really closed;
  - Adoption relies on the browser-session cookie (30 days by default); if it expires and a relaunch
    gets 401, restart the service once.
- **Orphan-process policy (changed in the v5.0.0 finalisation round)**: under the default `independent` mode, **quitting the
  launcher does not reap dsh** — that is precisely how "quitting never interrupts a session" is achieved.
  Ownership is recorded in `service.json` (PID + port + **process creation time**) and verified on the
  next launch; the creation-time comparison prevents a recycled PID from being mistaken for our service.
  For a kernel-level zero-residue guarantee, set `service_lifecycle = "tied"`.
  - On startup, a recorded dsh that is no longer listening (killed halfway through boot) is terminated
    when `stop_stale_orphan` is true (the default) — such a process holds no live session.
- **Loss detection & auto-reconnect (new in v5)**: the port is probed every 1.5 s; ~12 s without a response marks it lost and logs it.
  - An **own** service that exits is **auto-restarted** (up to 3 times);
  - An **adopted** instance that exits (e.g. its CMD window was closed — Windows sends `CTRL_CLOSE_EVENT` to every
    process attached to that console) cannot be restarted for you, but the launcher **keeps watching that port**:
    as soon as a service appears again it **re-adopts it and reconnects the UI**.
- **Orphan lock recovery (new in v5)**: dsh serialises writes through an `wx`-created `<file>.lock`; a force-killed
  process leaves it behind and the next start fails with `atomic-write: timed out waiting for the writer lock`.
  The launcher reads the **owner PID** from the lock and removes it only when that process is really gone;
  if the content is unparseable it requires the file to be at least 5 s old (avoiding the create-then-write race).
- **Configuration**: `%APPDATA%\DSHLauncher\settings.toml` (TOML, strongly validated). A v4 `settings.ini` is
  **migrated automatically** on first launch (port / working directory / Node path / tray option) and persisted.
  A corrupt config now raises an explicit dialog (with the file path) instead of silently falling back.
- **Log**: `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log`. v5 uses **append-only rolling** (2 MB × 3 files) rather
  than v4's full rewrite per line past the threshold; `?token=xxx` is redacted so credentials never hit disk.
- **Embedded WebView2 data folder**: `%LOCALAPPDATA%\DSHLauncher\webview2-profile` (v5 pins it explicitly, avoiding a
  `<exe>.WebView2\` folder next to the exe — which would break single-file distribution and **fail outright in a
  read-only install directory**).
- **CLI flags**: `--selftest` (hidden start → ready → stop check) · `--settings`/`-s` (open settings on
  launch) · `--guide`/`-g` (guide) · `--ipc-probe` (self-test: simulates a settings-page button click to
  verify IPC) · `--quit` (**new in the v5.0.0 finalisation round**: asks the running instance to exit gracefully — release and
  automation scripts should use this instead of force-killing, which cuts the running session).
- **Shortcuts (v5.0.0 finalisation round+, inside the embedded window)**: `F5` / `Ctrl+R` reload; `Esc` hides the window
  according to the *minimize to tray* preference.
- **Title bar follows the theme (v5.0.0 finalisation round+)**: the page background is sampled asynchronously roughly every
  3 seconds and applied to the DWM title bar (light/dark). Sampling happens on the callback thread and
  never blocks the UI.
- **Crash forensics (v5.0.0 finalisation round+)**: release builds use `panic = "abort"`, so `SetUnhandledExceptionFilter`
  is registered; a crash leaves one line in `launcher.log`:
  `[FATAL] unhandled exception code=0x… address=0x… pid=… tid=…` (no memory allocation).
- **Not in v5 (v4 had them)**: the settings-page "Fix Modules" button (as of v5.0.0 LTS the underlying `dsh-core` upgrade/health APIs are deleted too — a deliberate scope reduction) and settings-window position memory.
  (Raising the existing window on a second launch **was implemented in v5.0.0** via the named event
  `Local\DSHLauncher_Activate_v5`.)

### 1.7 Maintenance Rules & Pitfalls

1. **The global npm prefix must match the one the launcher uses.** Confirm with `npm config get prefix` first; under multi-Node environments (e.g. another tool's managed Node) `npm` may point at a different prefix — the safest way is the system npm with an explicit `--prefix`.
2. **You MUST stop the dsh service before upgrading** (root cause of Failure EBUSY): a running process locks files such as `koffi.node`, causing install failures or half-built directories. Run `Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force` to stop.
3. **npm 12's allow-scripts policy silently breaks native modules** (root cause of Failure B/G): always install dsh with `--allow-scripts=fs-ext,koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs`, then verify koffi and fs-ext immediately. Launcher v4.1.0–v4.2.4 ships a "Fix Modules" button for one-click rebuild; **v5.0.0 no longer offers it** — the unused `dsh-core` upgrade/health APIs (`version` / `dist_tags` / `upgrade` / `check_health`) have been deleted outright as dead code (v5.0.0 LTS finalisation round) — use the manual `node-gyp rebuild` steps in §1.5.
4. **Always verify integrity after upgrade/reinstall** (checklist in 1.4), especially the koffi native version and `dump-config`.
5. **Never install while killing processes**; let the install finish; if you must interrupt, check for leftover processes first.
6. **Assess breaking changes before upgrading**: read the target release notes (see Appendix B); protocol/config changes (e.g. token auth, APIProxy removal) may affect the launcher and model config.
7. **Pin explicit versions** to avoid tag drift.
8. **Credential safety**: API keys live in `%USERPROFILE%\.dsh\.credentials.yaml` — **never commit them to the repo or write them into documents**; upgrading dsh does not change credentials.
9. **Cleanup after failed installs**: check `%TEMP%` for leftover `dsh-*` directories and delete them (hundreds of MB each); add `--prefer-online` on the next install to bypass a corrupted npm cache.

### 1.8 Appendix A: Publisher Machine Snapshot (2026-09-09)

> The following records the publisher machine (Windows, user dir `C:\Users\20183`) — **reference only, not generic requirements**.

**Version baseline**

| Component | Version | Location |
|---|---|---|
| Node.js | v26.7.0 | `C:\Program Files\nodejs` |
| npm | 12.0.2 | prefix `C:\Users\20183\AppData\Roaming\npm` |
| pnpm | 11.22.0 | same |
| @deepseek-ai/dsh | **0.1.5-rc.1** (npm `latest`/`next` tag; `alpha` = 0.1.5-alpha.2) | `Roaming\npm\node_modules\@deepseek-ai\dsh` |
| Git | 2.55.0.4 | WinGet MinGit |
| Python | 3.13.15 | `C:\Users\20183\Local\Programs\Python\Python313` |
| DSHLauncher | **v5.0.0** (full Rust rewrite: wry + tao, single-file exe, LAN removed, loss detection, orphan-lock recovery) | `D:\DSHLauncher` |

**Agent & model API references**

| Item | Value |
|---|---|
| provider | `deepseek-official` |
| BASE URL (OpenAI format) | `https://api.deepseek.com` |
| BASE URL (Anthropic format) | `https://api.deepseek.com/anthropic` |
| API key env var | `DEEPSEEK_API_KEY` |
| Default model | `deepseek-flash` (provider `deepseek-official`, reasoningEffort: high; synced with the 0.1.5-rc.1 built-in default on 2026-09-10 and the gateway was verified open); `LongCat-2.0` (provider `longcat`, credential ref `LONGCAT_API_KEY`) is kept in the subagent allow-list |
| BASE URL override env var (optional) | `DEEPSEEK_BASE_URL` (falls back to `https://api.deepseek.com` when unset) |
| Default output cap | dsh default `256K` (official max 384K; a model's own cap and explicit request values win) |

Imported model catalog (re-verified on 0.1.5-rc.1: `dsh-llm-deepseek` (0.1.5-rc.1) now advertises **four** advisory entries — the new `deepseek-flash` (DeepSeek-V41-Flash, text + image, declares `systemPromptUpdate: in-history`) leads the catalog and is the **built-in default for new sessions**; the other three (`deepseek-v4-flash` / `deepseek-v4-pro` / `deepseek-v4-flash-vision-exp`) are unchanged from 0.1.3-alpha.2 / 0.1.2-rc.1, all four with a 1M (1e6) context window; pi-ai stays at 0.85.1; this machine's `settings.yaml` specifies `agent-default-model` explicitly, which per the upstream "explicit config wins" rule keeps taking precedence; upstream warns requests for `deepseek-flash` may fail with `INVALID_REQUEST` until the gateway enables that id — verified on this machine 2026-09-10: a direct `api.deepseek.com` call returned HTTP 200, so it is enabled; the model-discovery enhancements (custom-provider `models` objects, native Anthropic lists, name/context/max-output-token backfill) do not change the imported/default entries):

| Model id | Context | Output cap | Input modalities |
|---|---|---|---|
| `deepseek-flash` | 1M | 256K (dsh default) | text + image (DeepSeek-V41-Flash; new in 0.1.5-rc.1, catalog leader and built-in default; declares `systemPromptUpdate: in-history`; requests may return `INVALID_REQUEST` until the gateway enables the id) |
| `deepseek-v4-flash` | 1M | 384K | text |
| `deepseek-v4-pro` | 1M | 384K | text |
| `deepseek-v4-flash-vision-exp` | 1M | 384K | text + image (experimental; not listed by `/list-models` but callable directly) |
| `LongCat-2.0` | 1M | — | text (custom provider: `https://api.longcat.chat/openai/v1`, `LONGCAT_API_KEY`) |

Credential / API key references (secrets live in `C:\Users\20183\.dsh\.credentials.yaml`, never committed):

| Env var | Purpose | Note |
|---|---|---|
| `DEEPSEEK_API_KEY` | deepseek-official (chat + web search) | required |
| `LONGCAT_API_KEY` | longcat (LongCat-2.0) | required when using LongCat |
| `ZHIPU_API_KEY` / `AGNES_API_KEY` | (reserved) | no provider configured, unused |

> Credential document format: `.credentials.yaml` `version: 1` (unchanged in 0.1.5-rc.1; all four refs intact and the secrets themselves untouched); providers still resolve `apiKeyEnv` (`credential-ref`) per request — no migration needed.

### 1.9 Appendix B: Version Tracking & Breaking Changes

| Version | Tag | Key points |
|---|---|---|
| 0.1.1-rc.2 | latest/next | JSONL session backend default compression changed to `zstd` (watch mixed-encoding crashes, Failure C); built-in DeepSeek model catalog (flash/pro/vision-exp) |
| 0.1.2-alpha.1 | (GitHub only, not on npm) | **APIProxy removed → @Remote gateway**; pi-ai model support updates + vLLM thinking budget; unified `dsh` Profile startup; WebFetch enabled by default (SSRF protection) |
| 0.1.2-alpha.2 | alpha (npm) | all alpha.1 changes; **one-time token auth on the Web UI** (Failure D, launcher adapted); restored `SessionEvent.ignorable`; unified RemoteError; Node 24 startup fix |
| 0.1.2-alpha.3 | alpha (npm) | long-conversation right-nav & rendering memory improvements, image echo/delivery fixes, connection-misdetection fix, narrow-viewport schedule fix; removed the **optional** SQLite persistence backend (zstd JSONL unaffected); **no event-structure / API / token-auth contract changes** (mobile UI and LAN gateway need no adaptation) |
| 0.1.2-alpha.4 | alpha (npm) | parent/continuable child Agents exchange follow-up messages via `send_message`; custom model discovery reuses Profile headers, model catalog supports search/filter; long-conversation streaming render/nav-preview memory optimizations; `web_fetch` enabled by default for Python SDK/Headless/ACP; general-purpose `workflow` tool removed by default in Web PTC Mode; `Session.events` replaced by internal on-demand read APIs (`seq`/`eventAt()`/`snapshotEvents()`) (**external JSON-RPC API contract unchanged**: `session/list`/`session/page`/`session/prompt` response structures intact — mobile UI and LAN gateway need no adaptation) |
| 0.1.2-alpha.5 | alpha (npm) | fix startup failure or session title loss when upgrading from 0.1.1-rc.2 or 0.1.2-alpha.3 (**no API contract changes**) |
| 0.1.2-rc.1 | latest/next (npm) | **first release candidate for 0.1.2**, summarizing all changes since 0.1.1-rc.2; **breaking change**: Session persistence API now owned by lifecycle-scoped `SessionHandle`s, `agentLoop.create()` is asynchronous with a new Session lock; Session format upgraded to v2 (old v0/v1 logs migrated to current format); Remote gateway unifies remote-call API and error dispatch (legacy APIProxy removed); new Inspector tool, Web Preview, connection status display, subagent model selection, turn navigation, image echo/delivery fixes, etc. (**JSON-RPC API contract identical to alpha.4**: mobile UI and LAN gateway need no adaptation) |
| 0.1.3-alpha.1 | (GitHub only, not on npm; the npm alpha tag jumped straight to alpha.2) | **breaking change**: Session persistence API now owned by lifecycle-scoped `SessionHandle`s, `agentLoop.create()` is async + session lock (each Session held by at most one process); Session format upgraded to v2 (v0/v1 logs migrated via immutable adjacent generations); new features: generic file uploads of any type (mixed preview area, progress/cancel, stays visible across conversation switches), outbound requests honoring `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`, direct image rendering for top-level and PTC-nested `read_image` calls, enhanced model discovery (custom-provider `models` objects + native Anthropic model lists + name/context/max-output-token backfill); contains a known performance regression (fixed in alpha.2) (**external JSON-RPC contract change verified against alpha.2**: see next row) |
| 0.1.3-alpha.2 | alpha (npm) | pi-ai upgraded to 0.85.1 (supports more new models); Web header "Open in" for the workspace; continuable subagents support message queueing / editing / deletion / Steer (individual or all) / Stop; PTC mode can expand commands and their output; fixes: Web auto-reconnect after disconnection, chat auto-scroll, Windows Python SDK startup crash, leftover background processes; long-conversation open/resume/continue lag and memory improvements, model can read preview-omitted content on demand for long references; **default tool changes**: SDK, Headless and ACP now use read/write/edit for file editing (Web minimal and sdk-minimal unchanged); **custom persona config split into a prefix and a suffix** (existing configs and related constants need adapting); ordinary subprocess handles no longer include `pid` (terminal handles unaffected — a removed field that readers tolerate) (**external JSON-RPC contract unchanged**: release notes contain no entries touching the `session/list`/`session/page`/`session/prompt` endpoints → mobile UI and LAN gateway need no adaptation) |
| 0.1.5-alpha.1 | alpha (npm) | **Session format upgraded to V3** (restoring supported historical sessions writes new log files while keeping the originals; system prompts join the message history; legacy PTC events and `code` preset references migrate automatically; custom log readers must adapt; downgrade reads are unsupported); **Agent plugin API: `ctx.agent` removed** (callers must pass the Agent explicitly; continuable-subagent ownership corrected so they are excluded from root-session scheduling); Inbox is now a type-only interface (plugins use `agent.inbox`; `hasPending`/`claim` are no longer public API); experimental right Sidebar added (Detail panel removed), dynamic system-prompt support (requires explicit model opt-in); **native-dependency change: fs-ext removed** (session locks now use `@deepseek-ai/node-addon-system`: POSIX `flock(2)` / Windows kernel semaphore; no local compilation needed — §1.4 item 4 no longer applies); pi-ai still 0.85.1; optional Codex/Claude Code subagent runtimes bumped (explicit model settings unchanged); fixes for Send/Enter busy-behavior parity, user-only goal resume, POSIX-path local images, empty-message rejection, etc. (**external JSON-RPC contract unchanged**: no release entries touch `session/list`/`session/page`/`session/prompt` → mobile UI and LAN gateway need no adaptation) |
| 0.1.5-alpha.2 | alpha (npm) | right-Sidebar previews for common document types (Markdown, syntax-highlighted code, HTML, PDF, images); models can explicitly deliver files in conversations (Sidebar preview / open in default app / reveal in file manager); `/feedback` supports detailed feedback submissions; fixes: pi-ai model settings no longer disappear when a catalog change invalidates the configuration (diagnostics and repair controls preserved), custom-provider Base URLs validated/normalized before discovery or provider creation, Windows native folder picker opening behind other windows, stale Composer placeholder after whitespace input, filtered subagents still receiving guidance for unavailable tools, MCP duplicate pagination cursors hanging startup/sync, npm installs requiring a local `fs-ext` build, etc.; Settings localization and accessibility improvements; **Session data format stays V3** (no new migration); **Web plugin panel API changes** (plugins register global panels through `sidebar.panellist` and `main`; the former `conversation` slot moves to the `conversation` key under `main`); **minimal-profile default tools** (Web `minimal` and Python `sdk-minimal` are shell-only by default with `str_replace_editor` by explicit opt-in; persistent Bash reports completion/timeout status consistently); experimental Agent Teams packages installable from npm (explicit profile opt-in, not enabled by default); pi-ai still 0.85.1; fs-ext stays removed (**external JSON-RPC contract unchanged**: no release entries touch `session/list`/`session/page`/`session/prompt` → mobile UI and LAN gateway need no adaptation; API/credential/model-reference contract identical to 0.1.5-alpha.1) |
| 0.1.5-rc.1 | latest/next (npm; alpha = 0.1.5-alpha.2) | **first release candidate for 0.1.5**, summarizing all changes since 0.1.2-rc.1; **model-reference change**: adds `DeepSeek-V41-Flash` (`deepseek-flash`, text + image, supports in-history system-prompt updates), now the catalog leader and the **built-in default model for new sessions** (a configuration file that specifies a model explicitly wins; requests may return `INVALID_REQUEST` until the gateway enables the id); model discovery supports custom-provider `models` objects and native Anthropic lists (with name/context/max-output-token backfill); generic file uploads, right Sidebar (tabs/split/fullscreen + document previews), subagent message queueing/editing/Steer/Stop, `/feedback`, Web-header "Open in", outbound requests honoring `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`; **Session format V3 + SessionHandle lifecycle + asynchronous `agentLoop.create()` + session lock** (in place since 0.1.3-alpha.1); **default tool changes** (SDK/Headless/ACP use read/write/edit; Web `minimal` and `sdk-minimal` are shell-only with `str_replace_editor` opt-in); **plugin API changes** (`ctx.agent` removed, Inbox type-only, Web panels via `sidebar.panellist`/`main`); pi-ai 0.85.1; Codex 0.153.4 / Claude Code 2.1.263; fs-ext stays removed; **credential-reference contract unchanged** (`DEEPSEEK_API_KEY`/`LONGCAT_API_KEY`, `.credentials.yaml` version 1); **external JSON-RPC contract unchanged** → mobile UI and LAN gateway need no adaptation |
| Launcher v3.0.0 | — | LAN sharing & standalone mobile UI (non-dsh-native): collapsible grouped session list, chat (history / load-earlier / outline nav), read-only mode (UI hidden + gateway API blocked), session filtering (archived/subagent/blank), one-click archived-session purge, PIN rotation kicks all devices, randomized SW version auto-flushes caches |
| Launcher v4.1.0 | — | multi-channel version selection (freely switch between npm dist-tags: latest / alpha / next / rc / beta / dev, etc.); "Fix Modules" button in the upgrade page (one-click rebuild of fs-ext / koffi / node-pty); post-upgrade `CheckDshHealth` probe and status-bar native-module health; enforced official registry; allow-scripts whitelist completed (added fs-ext) |
| Launcher v4.2.0 (RC) | — | full governance & bottom-up refactor: version unified to 4.2.0 project-wide; "Fix Modules" now probes both the nested and top-level npm layouts and auto-detects installed native modules (root-cause fix for the npm 12 nesting issue where koffi/node-pty were misjudged as "not installed" and the button no-op'd); fs-ext dropped automatically with the dsh 0.1.5-alpha.1 removal (file locks now use `@deepseek-ai/node-addon-system`); health-check copy, tooltips and maintenance manuals synced; changelog navigator/anchors tidied; new consistency checker `tools/check-consistency.ps1` |
| Launcher v4.2.1 | — | fixes "Clean archived sessions" always deleting 0 (archived dirs are 44-char `session-<uuid>` while the old guard accepted only 36 chars); cleanup now stops the dsh service first, deletes, and removes only the successfully deleted ids from the archive list — no more orphaned data or lost lists |
| Launcher v4.2.2 | — | archived-session cleanup reliability: stops dsh by port (including non-adopted/external instances) and waits for the port to be released before deleting; clears leftover archive markers whose data is already gone and purges the `session_projcache` projection cache so a running dsh can no longer resurrect the list via `workspace.json` rewrites |
| Launcher v4.2.3 | — | archived-session cleanup hardening: process identity check before killing (aborts if a non-dsh program holds the port — no collateral kills); authoritative stop verdict ("process gone + port rebindable"); auto second-round cleanup (up to 3) when the list is resurrected after restart; one UAC escalation when a normal kill fails; busy protection on the cleanup button |
| Launcher v4.2.4 | — | settings is now a **standalone top-level window** (own taskbar entry, independently minimizable/closable, remembers position, closed with the launcher); under a limited token a new equivalent identity ("node process + owner of the configured port") restores auto-adoption and cleanup (fixes "PID 0"), with one UAC-elevated `Get-NetTCPConnection`-based port kill as fallback |
| **Launcher v5.0.0** | — | **full Rust rewrite** (wry + tao, no .NET, no side-by-side DLLs, single-file exe); **LAN sharing fully removed** (~6,400 lines + 902 KB); Job Object guarantees zero orphans; `GetExtendedTcpTable` identifies port owners without admin; append-only rolling log; **new: loss detection & auto-reconnect, orphan lock recovery, settings-page IPC, CLI flags**; fixes startup self-deadlock, no-window-on-adopt, WebView2 side-by-side data folder, stray console window from the GUI subsystem, missing title-bar icon. See [`RELEASE_NOTES_v5.0.0.md`](RELEASE_NOTES_v5.0.0.md) and [`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md) §13. **Finalisation round** — **service decoupled from the launcher** (default `service_lifecycle = "independent"`: quitting / crashing / upgrading never interrupts a session; `service.json` bookkeeping + startup reconciliation, PID-reuse safe); **token-capture race fixed** (readiness measured ~914 ms before the token arrived — the old code navigated to the token-less URL, i.e. HTTP 401); **fixed the lock-held-spawn deadlock that left a tray-only process with no window**; **non-dsh port owners are never adopted or killed**; stderr drained; `tray_on_close` wired; `F5`/`Ctrl+R`/`Esc`; theme following wired (async); **"Clean archived sessions" now asks for confirmation and restarts the service afterwards**; **exit behaviour follows the "service independent" setting: no prompt when independent, asked every time when tied**; `--quit` for graceful exit; `--build-info` artifact check; crash forensics; theme-log flooding fixed; `build.ps1` can publish via hot swap; documented numbers now come from a single source. See [`RELEASE_NOTES_v5.0.0.md`](RELEASE_NOTES_v5.0.0.md), [`TECHNICAL-ROADMAP.md`](TECHNICAL-ROADMAP.md) §13 and [`IMPLEMENTATION-v5.0.0.md`](IMPLEMENTATION-v5.0.0.md) |

> Check with `npm view @deepseek-ai/dsh dist-tags`; release notes at `https://github.com/deepseek-ai/deepseek-harness/releases`.

### 1.10 Appendix C: Publisher Upgrade History

| Date | Action | Result |
|---|---|---|
| 2026-08-19 | full env check; npm 11.19→12.0.2; pnpm 11.22.0; fixed dsh rc.7 integrity (js-yaml.mjs/commander missing) | ✅ |
| 2026-08-20 | dsh rc.7 → rc.8 (`next` tag) | ⚠️ koffi native mismatch crash → fixed via `--prefer-online` + allowed-scripts reinstall (koffi 3.1.6 / 503-line plugin tree) |
| 2026-08-22 | dsh rc.8 → 0.1.1-rc.2 (built-in V4-Flash-Vision-Exp registration) | ✅ 14-item verification PASS (koffi 3.1.6 / 514-line plugin tree) |
| 2026-08-30 | re-check: npm latest/next = 0.1.1-rc.2; GitHub released 0.1.2-alpha.1 (not on npm yet) → hold | ✅ stayed on rc.2 |
| 2026-08-30 (fix) | fixed session-encoding mismatch crash (zstd/plaintext, Failure C); backups moved outside `.dsh` | ✅ |
| 2026-08-30 (p2) | workflow change: removed one-click upgrade script; "one sentence triggers agent per manual"; cleaned root | ✅ |
| 2026-08-31 | dsh 0.1.1-rc.2 → **0.1.2-alpha.2** (npm alpha tag); token-auth breaking change → launcher adaptation (Failure D) | ✅ |
| 2026-08-31 (fix) | launcher behavior fix: exit keeps dsh web running (web stays connected); adoption relies on 30-day cookie | ✅ |
| 2026-08-31 (p2) | version bumped to v2.0.0; manual merged into README (single doc ships with release); installer ships README; uninstall script generalized | ✅ |
| 2026-09-01 | dsh 0.1.2-alpha.2 → **0.1.2-alpha.3** (npm alpha tag); verified: koffi 3.1.6 / node-pty loadable / 539-line plugin tree, no error; confirmed event structure, `dsh-auth-` cookie and token contracts unchanged → **mobile UI upgrade NOT needed**; API references re-verified identical to Appendix A | ✅ |
| 2026-09-01 (fix) | launcher full fix round & v3.0.0 release prep: audit fixes (archive purge / log redaction / gateway security), mobile UI fixes (subagent-injection filtering, paginated outline, chronological order, title wrapping), UTF-8 process output fix, dsh upgraded to 0.1.2-alpha.3; manual split into this standalone file | ✅ |
| 2026-09-01 (p2) | dsh 0.1.2-alpha.3 → **0.1.2-alpha.4** (npm alpha tag); verified: koffi 3.1.6 / node-pty loadable / 529-line plugin tree, no error; confirmed `Session.events` internal API change does not affect external JSON-RPC contract (`session/list`/`session/page`/`session/prompt` response structures unchanged) → **mobile UI upgrade NOT needed**; API references re-verified identical to Appendix A | ✅ |
| 2026-09-04 | dsh 0.1.2-alpha.4 → **0.1.2-alpha.5** (npm alpha tag); verified: koffi 3.2.1 / node-pty loadable / plugin tree, no error; confirmed no API contract changes → **mobile UI upgrade NOT needed** | ✅ |
| 2026-09-04 (fix) | attempted dsh 0.1.2-alpha.5 → **0.1.2-rc.1** upgrade, failed with EBUSY because running dsh process locked `koffi.node`; left `dsh-broken-*`, `dsh-partial-*` directories (~409 MB); fixed per Failure EBUSY procedure: stop process → clean residuals → reinstall alpha.4 | ⚠️ fixed |
| 2026-09-04 (p2) | cleaned install residuals: deleted 9 `dsh-*` residual directories from `%TEMP%` (~409 MB total); updated manual section 1.3 (added mandatory "stop dsh before upgrade" step, post-install cleanup) and Failure EBUSY | ✅ |
| 2026-09-04 (p3) | dsh 0.1.2-alpha.4 → **0.1.2-rc.1** (npm latest/next tag); verified: koffi 3.2.1 / node-pty loadable / plugin tree, no error; confirmed Session persistence API internal change (SessionHandle + session lock) does not affect external JSON-RPC contract → **mobile UI upgrade NOT needed**; API references re-verified identical to Appendix A | ✅ |
| 2026-09-08 | dsh 0.1.2-rc.1 → **0.1.3-alpha.2** (npm alpha tag; latest/next still 0.1.2-rc.1; upgraded via launcher v4.1.0 multi-channel); verified: installed version matches / fs-ext loadable (new native module) / koffi 3.2.1 / node-pty loadable; local check of `dsh-llm-deepseek` model catalog identical to rc.1 (flash/pro/vision-exp, 1M context); release notes contain no changes to the external JSON-RPC endpoints `session/list`/`session/page`/`session/prompt` → **mobile UI upgrade NOT needed**; API references re-verified identical to Appendix A | ✅ |
| 2026-09-08 (docs) | docs sync: Appendix A version baseline → 0.1.3-alpha.2, model catalog re-verify note updated; Appendix B adds 0.1.3-alpha.2 and Launcher v4.1.0 rows; Appendix C records this upgrade | ✅ |
| 2026-09-09 | dsh 0.1.3-alpha.2 → **0.1.5-alpha.1** (npm alpha tag; latest/next still 0.1.2-rc.1); verified: installed version matches / koffi 3.2.1 / node-pty loadable / **fs-ext removed by upstream (normal — do not treat as Failure G)**; pi-ai stays 0.85.1; local check of `dsh-llm-deepseek` model catalog unchanged (flash/pro/vision-exp, 1M context); release notes contain no external JSON-RPC endpoint changes (`session/list`/`session/page`/`session/prompt`) → **mobile UI upgrade NOT needed**; API references re-verified identical to Appendix A | ✅ |
| 2026-09-09 (docs) | docs sync: Appendix A version baseline → 0.1.5-alpha.1, model catalog re-verify note updated, fs-ext removal version notes; Appendix B adds the 0.1.5-alpha.1 row; Appendix C records this upgrade | ✅ |
| 2026-09-09 (p2) | **v4.2.0 (RC) released** (previous internal dev version never shipped; its items are folded into this release): full governance changes landed (version unified to 4.2.0; "Fix Modules" nested dual-path location fix + fs-ext removal adaptation; npm dist-tags JSON parser fix; health check / tooltips / manuals / CHANGELOG synced; consistency checker `tools/check-consistency.ps1` added); dsh stays on 0.1.5-alpha.1, API references re-verified identical to Appendix A | ✅ |
| 2026-09-09 (p3) | launcher **v4.2.1 released**: packaged the archived-session cleanup fix (44-char dir guard + stop-service-first deletion + remove only deleted ids); installer rebuilt and published; dsh stays on 0.1.5-alpha.1 | ✅ |
| 2026-09-09 (p4) | launcher **v4.2.2 released**: packaged the "stop dsh by port + purge ghost markers/projection cache" fix; installer rebuilt and published; dsh stays on 0.1.5-alpha.1 | ✅ |
| 2026-09-09 (p5) | launcher **v4.2.3 released**: packaged the cleanup hardening (identity check to avoid collateral kills + authoritative stop verdict + auto second-round resurrection healing + UAC escalation fallback + UI busy protection); installer rebuilt and published; dsh stays on 0.1.5-alpha.1 | ✅ |
| 2026-09-09 (p6) | dsh 0.1.5-alpha.1 → **0.1.5-alpha.2** (npm `alpha` tag; latest/next still 0.1.2-rc.1); verified: installed version matches / pi-ai still 0.85.1 / local check of `dsh-llm-deepseek` (0.1.5-alpha.2) model catalog unchanged (flash/pro/vision-exp, 1M context); release notes contain no external JSON-RPC endpoint changes (`session/list`/`session/page`/`session/prompt`) → **mobile UI upgrade NOT needed**; API/credential references re-verified identical to Appendix A | ✅ |
| 2026-09-10 (p7) | launcher **v4.2.4 released**: packaged the standalone settings window + limited-token identity/elevated port-stop fixes; installer rebuilt and published; dsh was on 0.1.5-alpha.2 at packaging time (upgraded to 0.1.5-rc.1 later the same day, see below), API references re-verified identical to Appendix A | ✅ |
| 2026-09-10 (p8) | dsh 0.1.5-alpha.2 → **0.1.5-rc.1** (npm `latest`/`next` tag, so the stable/preview channel is now the 0.1.5 line; `alpha` stays 0.1.5-alpha.2); verified: installed version matches (dsh web process started after the install) / pi-ai still 0.85.1 / `dsh-llm-deepseek` (0.1.5-rc.1) catalog gains `deepseek-flash` (V41 Flash, 1M, text+image, `systemPromptUpdate: in-history`) and the built-in default model becomes `deepseek-flash`; this machine's explicit `agent-default-model` = `longcat/LongCat-2.0` keeps taking precedence per the upstream "explicit config wins" rule (requests for `deepseek-flash` may return `INVALID_REQUEST` until the gateway enables the id); **credential-reference re-check**: all four refs (`DEEPSEEK_API_KEY`/`LONGCAT_API_KEY`, ...) and `.credentials.yaml` `version: 1` unchanged — no migration needed; **model-reference sync**: this machine's `agent-default-model` switched back to `deepseek-official/deepseek-flash` (reasoningEffort: high) and `deepseek-flash` was added to the subagent `allowedModels` (`LongCat-2.0` kept); release notes contain no external JSON-RPC endpoint changes (`session/list`/`session/page`/`session/prompt`) → **mobile UI upgrade NOT needed** | ✅ |
