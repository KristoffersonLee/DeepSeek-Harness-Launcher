# Upgrade & Maintenance Manual (English)

> This document is the maintenance manual for the **DSHLauncher** repository (standalone; ships with the project and is deployed into the install directory by the installer).
> It covers installing, upgrading, verifying, and troubleshooting **dsh** (`@deepseek-ai/dsh`, the DeepSeek Harness CLI/service) and **DSHLauncher**.
> The body is generic (machine-independent); the appendices keep the publisher machine's snapshot and upgrade history for reference.
> Official dsh is still under fast preview/rc/alpha iteration and may introduce **breaking changes** (port/protocol/interface, config format, working-directory layout, etc.). Read sections 3, 4 and 7 before upgrading.

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
npm install -g "@deepseek-ai/dsh@<dsh-version>" --no-audit --no-fund --prefer-online `
  --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
```

| Flag | Purpose |
|---|---|
| `@deepseek-ai/dsh@<version>` | **Always pin an explicit version** to avoid tag drift |
| `--prefer-online` | bypass a corrupted local npm cache tarball |
| `--allow-scripts=...` | **required on npm 12**: allow native build scripts for koffi (FFI), node-pty (terminal), etc.; otherwise you get a half-installed package (Failure B in 1.5) |
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
npm install -g "@deepseek-ai/dsh@<version>" --no-audit --no-fund --prefer-online --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs

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

```powershell
# 1) Version
dsh --version

# 2) Usage (exercises bin.js -> dsh-app-boot -> commander/js-yaml loading chain)
dsh --help

# 3) Plugin config tree (validates YAML parsing and full plugin-tree loading; normally 500+ lines, no error/mismatch)
dsh --profile web --dump-config

# 4) koffi native version match (critical!)
node -e "const k=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/koffi'); console.log(k.version)"

# 5) node-pty loadable
node -e "const p=require('<npm-prefix>/node_modules/@deepseek-ai/dsh/node_modules/node-pty'); console.log(typeof p.spawn)"
```

| Check | Expected |
|---|---|
| `dsh --version` | matches the installed version |
| koffi | prints a version consistent with the JS wrapper (e.g. `3.1.6`); `Mismatched native Koffi modules` means a broken install |
| node-pty | prints `function` |
| `dump-config` | no `error` / `mismatch` / `failed to` |
| Key files | under `<npm-prefix>\node_modules\@deepseek-ai\dsh\`: `package.json`, `lib\bin.js`, `node_modules\commander\index.js`, `node_modules\js-yaml\dist\js-yaml.mjs`, `node_modules\@koromix\koffi-win32-x64\win32_x64\koffi.node` |
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
- **Fix**: reinstall completely with `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs` (1.3), then verify koffi (1.4). **Never try to replace a single .node file.**

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
     npm install -g "@deepseek-ai/dsh@<version>" --no-audit --no-fund --prefer-online --allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs
     ```
  4. Verify with the checklist in section 1.4.

**Failure F: Common false alarms (not problems)**

| Phenomenon | Note |
|---|---|
| `UNMET OPTIONAL DEPENDENCY @img/sharp-*` (darwin/linux/freebsd) in `npm ls -g` | not installed on Windows; normal |
| `EPERM` writing `cordis.yml` | usually another instance holds the port / sandbox restrictions; not a corrupt config |
| LAN sharing won't open / mobile 401 | see the LAN sharing chapter: check the switch, PIN, firewall rules; troubleshoot via `%LOCALAPPDATA%\DSHLauncher\logs\lan-gateway.log` |
| `npm warn cleanup Failed to remove .dsh-*` | temp dir locked by a running process; deletable after restart |

### 1.6 Launcher Behavior

- **Embedded window**: WebView2 renders Harness (no browser); falls back to a lightweight Edge window when unavailable.
- **Tray menu**: open UI / refresh / open in browser / start service / stop service / guide / log folder / settings / about / quit.
- **Token-auth adaptation (v2.0+)**: the launcher captures the `/?token=…` URL from `dsh web` output and navigates to it; older dsh versions without that output fall back to the plain URL (compatible).
- **Service lifecycle (v2.0+)**:
  - Quitting the launcher **keeps dsh web running in the background by default** — the web page stays connected; the next launch auto-detects and adopts it;
  - To stop the service: tray "Stop service", or choose "Yes" in the close-window prompt;
  - Clicking ✕ (when "minimize to tray on close" is checked) hides to the tray; the app and service keep running;
  - Adoption relies on the browser-session cookie (30 days by default); if it expires and a relaunch gets 401, restart the service once.
- **Log**: `%LOCALAPPDATA%\DSHLauncher\logs\launcher.log` (auto-trimmed beyond 2 MB).

### 1.7 Maintenance Rules & Pitfalls

1. **The global npm prefix must match the one the launcher uses.** Confirm with `npm config get prefix` first; under multi-Node environments (e.g. another tool's managed Node) `npm` may point at a different prefix — the safest way is the system npm with an explicit `--prefix`.
2. **You MUST stop the dsh service before upgrading** (root cause of Failure EBUSY): a running process locks files such as `koffi.node`, causing install failures or half-built directories. Run `Get-Process -Name 'node' | Where-Object { $_.CommandLine -like '*dsh*' } | Stop-Process -Force` to stop.
3. **npm 12's allow-scripts policy silently breaks native modules** (root cause of Failure B): always install dsh with `--allow-scripts=koffi,node-pty,@deepseek-ai/dsh-subprocess-local,@google/genai,protobufjs`, then verify koffi immediately.
4. **Always verify integrity after upgrade/reinstall** (checklist in 1.4), especially the koffi native version and `dump-config`.
5. **Never install while killing processes**; let the install finish; if you must interrupt, check for leftover processes first.
6. **Assess breaking changes before upgrading**: read the target release notes (see Appendix B); protocol/config changes (e.g. token auth, APIProxy removal) may affect the launcher and model config.
7. **Pin explicit versions** to avoid tag drift.
8. **Credential safety**: API keys live in `%USERPROFILE%\.dsh\.credentials.yaml` — **never commit them to the repo or write them into documents**; upgrading dsh does not change credentials.
9. **Cleanup after failed installs**: check `%TEMP%` for leftover `dsh-*` directories and delete them (hundreds of MB each); add `--prefer-online` on the next install to bypass a corrupted npm cache.

### 1.8 Appendix A: Publisher Machine Snapshot (2026-09-01)

> The following records the publisher machine (Windows, user dir `C:\Users\20183`) — **reference only, not generic requirements**.

**Version baseline**

| Component | Version | Location |
|---|---|---|
| Node.js | v26.7.0 | `C:\Program Files\nodejs` |
| npm | 12.0.2 | prefix `C:\Users\20183\AppData\Roaming\npm` |
| pnpm | 11.22.0 | same |
| @deepseek-ai/dsh | **0.1.2-rc.1** (npm `latest`/`next` tag; alpha = 0.1.2-alpha.5) | `Roaming\npm\node_modules\@deepseek-ai\dsh` |
| Git | 2.55.0.4 | WinGet MinGit |
| Python | 3.13.15 | `C:\Users\20183\Local\Programs\Python\Python313` |
| DSHLauncher | v3.0.0 (LAN sharing + standalone mobile UI + read-only mode + archive purge) | `D:\DSHLauncher` |

**Agent & model API references**

| Item | Value |
|---|---|
| provider | `deepseek-official` |
| BASE URL (OpenAI format) | `https://api.deepseek.com` |
| BASE URL (Anthropic format) | `https://api.deepseek.com/anthropic` |
| API key env var | `DEEPSEEK_API_KEY` |
| Default model | `deepseek-v4-flash` (reasoningEffort: high) |
| Default output cap | dsh default `256K` (official max 384K) |

Imported model catalog (re-verified on 0.1.2-rc.1, identical to 0.1.2-alpha.5; new model catalog search/filter and subagent model selection are UI features that do not affect the catalog itself):

| Model id | Context | Output cap | Input modalities |
|---|---|---|---|
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
| 0.1.3-alpha.1 | (GitHub only, not on npm) | **breaking change**: Session persistence API → `SessionHandle`, `agentLoop.create()` async + session lock; Session format v2; new features: generic file upload, HTTP proxy support, `read_image` direct rendering, enhanced model discovery (**JSON-RPC API contract NOT yet verified**: pending npm release) |
| Launcher v3.0.0 | — | LAN sharing & standalone mobile UI (non-dsh-native): collapsible grouped session list, chat (history / load-earlier / outline nav), read-only mode (UI hidden + gateway API blocked), session filtering (archived/subagent/blank), one-click archived-session purge, PIN rotation kicks all devices, randomized SW version auto-flushes caches |

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
