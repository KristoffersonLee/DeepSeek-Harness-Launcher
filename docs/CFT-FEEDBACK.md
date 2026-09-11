> **中文说明**：本文件是 `docs/` 里唯一一份英文文档 —— 一份**可直接粘贴到上游 Cargo 的反馈草稿**：
> 对应 [*Call for Testing: Build Dir Layout v2*](https://blog.rust-lang.org/2026/03/13/call-for-testing-build-dir-layout-v2/)
> 与跟踪 issue [#15010 · Re-organize build-dir by package + hash, rather than artifact type](https://github.com/rust-lang/cargo/issues/15010)
> （另有稳定化 PR [#16807](https://github.com/rust-lang/cargo/pull/16807)）。
> 自 `Title:` 起为英文正文，可按原样整段粘贴；路径、字节数与版本号全部取自本仓库实测。

**Title:** build-dir v2: document the final-artifact (uplift) guarantee per target kind, and expose the effective build-dir

**Body:**

We maintain a Windows desktop launcher — a small Cargo workspace whose build, test, release and cleanup steps are
PowerShell scripts rather than CI. We ran the CFT (`cargo -Zbuild-dir-new-layout`, and separately
`CARGO_BUILD_BUILD_DIR`) and audited every place where our tooling touches the build tree; everything quoted below
is read-only observation of the trees the CFT run left on this machine.

### 1. The per-unit `out/` directory versus the final artifacts in `target/<profile>/`

**Bin / lib units (new layout, observed).** Per-package buckets were produced, and the unit's artifact lives in
`out/`, including the build script's own binary:

```text
D:\DSHLauncher\target\release\build\dsh-app\b993bcd4178be58e\out\dsh_app.exe                        1,087,488 bytes
D:\DSHLauncher\target\release\build\dsh-app\1b6fa888e1d3504f\out\build_script_build.exe              331,264 bytes
D:\DSHLauncher\target\release\build\dsh-uninstall\b0b8fb4084d71f58\out\dsh_uninstall.exe             310,272 bytes
D:\DSHLauncher\target\release\build\dsh-core\861435f5b8feb886\out\libdsh_core-861435f5b8feb886.rlib  1,158,060 bytes
```

The final-artifact paths exist as well, but were **not the same file** as the `out/` artifact:

```text
PS> fsutil hardlink list D:\DSHLauncher\target\release\dsh-app.exe
\DSHLauncher\target\release\dsh-app.exe            (1,078,784 bytes)
\DSHLauncher\target\release\deps\dsh_app.exe       <- its only other link
PS> fsutil hardlink list ...\build\dsh-app\b993bcd4178be58e\out\dsh_app.exe
...\build\dsh-app\b993bcd4178be58e\out\dsh_app.exe   <- only itself
```

The two dep-info files name *different* artifacts for the same logical target (source lists abbreviated):

```text
# target\release\build\dsh-app\b993bcd4178be58e\out\dsh_app.d   (note the mixed path separators)
D:\DSHLauncher\target\release\build\dsh-app/b993bcd4178be58e\out\dsh_app.exe: crates\dsh-app\src\main.rs ...
# target\release\dsh-app.d
D:\DSHLauncher\target\release\dsh-app.exe: D:\DSHLauncher\app.ico ...
```

This is plausibly residue of two builds with different toolchains, so we are not reporting a bug — but the layout
alone gives tooling no way to tell which path holds the artifact Cargo considers current.

**Example (the case we still cannot answer).** Our self-test's example artifact exists only at the final-artifact
path, and its dep-info names exactly that path:

```text
D:\DSHLauncher\target\debug\examples\job_object_demo.exe     323,072 bytes, single link (no reparse point)
D:\DSHLauncher\target\debug\examples\job_object_demo.d      -> names ...\target\debug\examples\job_object_demo.exe
D:\DSHLauncher\target\debug\build\dsh-core\                 does not exist in this tree
```

That example was built without the new layout, so we cannot tell from our tree whether an example built *with* the
new layout also appears as `target/<profile>/build/dsh-core/<hash>/out/job_object_demo.exe`, whether the two paths
are the same file or two copies, or which is authoritative — not hypothetical for us: `selftest.ps1` used to
hardcode that path and now takes it from cargo's `--message-format=json` `executable` field for exactly this reason.

**What we would like documented** (per target kind: bin, example, lib/dylib/cdylib, test, bench):
1. Is `target/<profile>/<name>[.exe]` / `target/<profile>/examples/<name>[.exe]` guaranteed to exist after a successful `cargo build` (respectively `--example`)?
2. Is it guaranteed to be the artifact of the most recent build — the same file, a hardlink or a copy of `<build-dir>/<profile>/build/<pkg>/<hash>/out/<name>` — or may it be a leftover, as observed above?
3. Is "hardlink where possible, copy otherwise" (as for `deps/` → `target/` before) still the contract, and do `CARGO_BUILD_BUILD_DIR` / `--target-dir` change any of this?

We are grateful for the CFT post's "What is not changing: the layout of final artifacts within target dir" and the
final/intermediate split in the [Build Cache chapter](https://doc.rust-lang.org/cargo/reference/build-cache.html) — we are asking for the details (existence, identity, per-target-kind coverage), not for a new guarantee.

### 2. There is no way to print the *effective* build-dir

`cargo build --help` (cargo 1.98.1) documents `--target-dir` but prints nothing about build-dir, and `cargo -Z help`
(nightly 2026-09-04) only offers the toggle `-Z build-dir-new-layout`. The closest machine-readable source we found
is `cargo metadata`'s `build_directory` field, annotated `(unstable)` in
[cargo-metadata(1)](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html) — so tooling must re-derive the
effective build-dir from `CARGO_BUILD_BUILD_DIR`, `.cargo/config.toml`'s `[build] build-dir`, the default, and the
base for relative paths. Our cleanup script (`tools/clean.ps1`) had to do exactly that; without it, `-Cache`
silently leaves the largest cache on disk behind — with `CARGO_BUILD_BUILD_DIR` set, this repository measured
538 MB of intermediates moved out of `target\`, all of which `-Cache` would have missed:

```powershell
function Resolve-BuildDir {
    $value = $env:CARGO_BUILD_BUILD_DIR
    if ([string]::IsNullOrWhiteSpace($value)) {
        $cfg = Join-Path $root '.cargo\config.toml'
        if (-not (Test-Path -LiteralPath $cfg)) { $cfg = Join-Path $root '.cargo\config' }
        if (Test-Path -LiteralPath $cfg) {
            $m = [regex]::Match((Get-Content -LiteralPath $cfg -Raw), '(?ms)^\[build\].*?^\s*build-dir\s*=\s*"([^"]+)"')
            if ($m.Success) { $value = $m.Groups[1].Value }
        }
    }
    if ([string]::IsNullOrWhiteSpace($value)) { return $target }   # default: <workspace>/target
    if ([System.IO.Path]::IsPathRooted($value)) { return $value }
    return (Join-Path $root $value)                                # relative -> workspace root
}
```

Resolution order implemented: `CARGO_BUILD_BUILD_DIR` → `.cargo/config.toml` (or legacy `.cargo/config`) `[build] build-dir` at the workspace root → default `target`; absolute values as-is, relative values joined to the workspace root.
It is a best-effort subset: it ignores `%CARGO_HOME%\config.toml`, config files in ancestor directories, `--config` overrides, and how `-C`/`--manifest-path` change which config is discovered; and it parses TOML with a regex. We would rather ask Cargo than guess:

1. a stable way to print the effective build-dir — e.g. `cargo metadata`'s `build_directory` leaving `(unstable)`, a `cargo config get build.build-dir` recipe in the book, or a line in `cargo build --help`; and
2. if the field must stay unstable, a note (in the build-dir / Build Cache docs) telling tools to ask Cargo instead of re-deriving it, with the precedence order (`CARGO_BUILD_BUILD_DIR` vs `build.build-dir` vs `--config` vs default) spelled out.

### Environment / how to see the same layout

```text
Windows x64, workspace D:\DSHLauncher; no .cargo/config.toml; CARGO_BUILD_BUILD_DIR unset in the tree measured
cargo 1.98.1 (797e8a9bc 2026-08-05) / rustc 1.98.1 (48a229cea 2026-09-01)   # stable (= latest stable), OLD layout
cargo 1.100.0-nightly (3c0b53475 2026-09-04)                                # new layout, default on
  Update: we have since **pinned** a dated nightly (`rust-toolchain.toml` -> `nightly-2026-09-10`) as our
  main toolchain, so layout v2 is now what we build with every day. The stable/old-layout lines above are
  kept because they describe what the current *stable* still produces — which is exactly why we had to pin.
  (docs\RELEASE-VERIFICATION-v5.0.0-LTS.md §9.2 and §11 record the nightly runs; final-artifact paths unchanged.)

cargo +nightly build --release --locked
Get-ChildItem -Recurse target\release\build\<pkg>\*\out    # per-package buckets: the artifacts quoted above
Get-ChildItem target\release\*.exe                         # final artifacts at the documented paths
fsutil hardlink list target\release\dsh-app.exe            # shows which file the final path really is
```

Thanks for the CFT and for the tracking issue — happy to re-test once the wording above is settled.
