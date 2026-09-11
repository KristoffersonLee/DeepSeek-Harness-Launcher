//! 崩溃取证：在 `panic = "abort"` 下仍能留下**一行 FATAL 记录**。
//!
//! ## 为什么需要它
//!
//! release profile 启用了 `panic = "abort"`（体积优先），这意味着：
//! - 没有栈展开，也没有默认的 panic 输出（GUI 子系统连 stderr 都没有）；
//! - 用户报「启动器忽然不见了」时，`launcher.log` 里**什么都没有**，
//!   无法区分「是崩溃」还是「被杀掉」。
//!
//! 这里注册 `SetUnhandledExceptionFilter`：进程因未处理的结构化异常
//! （访问冲突 `0xC0000005`、栈溢出 `0xC00000FD`、DLL 初始化失败 `0xC0000142` 等）
//! 即将终止时，用**不分配内存**的方式把一行诊断写进日志。
//!
//! ## 约束（决定了实现风格）
//!
//! 崩溃上下文里**不能**做任何可能再次失败或分配的事：不能 `format!`、
//! 不能取锁、不能调用需要堆的 API。因此本模块：
//! - 只用栈上缓冲 + `WriteFile`；
//! - 日志路径在**注册时**就转成 UTF-16 存好，处理器里只读不查；
//! - 处理器内部**忽略一切返回码**（它本来就是为了记录失败而存在的）。

use std::sync::OnceLock;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_ALWAYS,
};
use windows::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_POINTERS};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};

/// 日志文件的 UTF-16 绝对路径（注册时确定，处理器里只读）。
static LOG_PATH_W: OnceLock<Vec<u16>> = OnceLock::new();

/// 注册崩溃处理器。**只能调用一次**（重复调用只更新路径）。
///
/// `log_path` 应指向 `launcher.log`（与 `RollingLogger` 同一文件，
/// 这样用户点「打开日志目录」就能看到崩溃记录）。
pub fn install(log_path: std::path::PathBuf) {
    let wide: Vec<u16> = log_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let _ = LOG_PATH_W.set(wide);
    unsafe {
        // 依赖 Win32_System_Diagnostics_Debug feature
        SetUnhandledExceptionFilter(Some(unhandled_filter));
    }
}

/// 未处理异常过滤器：写一行 FATAL 后交回系统（返回 `EXCEPTION_CONTINUE_SEARCH`）。
unsafe extern "system" fn unhandled_filter(info: *const EXCEPTION_POINTERS) -> i32 {
    // 384 字节：中文 FATAL 行实测约 200 字节（UTF-8 下每个汉字 3 字节），
    // 原 256 字节余量太小，一旦超限 `Writer::bytes` 会**静默截断**，恰好丢掉尾部的
    // pid/tid（崩溃取证里最需要的信息）。
    let mut line = [0u8; 384];
    let code = if info.is_null() {
        0u32
    } else {
        (*(*info).ExceptionRecord).ExceptionCode.0 as u32
    };
    let addr = if info.is_null() {
        0usize
    } else {
        (*(*info).ExceptionRecord).ExceptionAddress as usize
    };
    let n = write_fatal(&mut line, code, addr);
    if n > 0 {
        append_raw(&line[..n]);
    }
    // 1 = EXCEPTION_EXECUTE_HANDLER 不在过滤器里使用；这里交回系统按默认流程终止
    0 // EXCEPTION_CONTINUE_SEARCH
}

/// 把诊断行写进栈缓冲，返回字节数（不分配）。
fn write_fatal(buf: &mut [u8], code: u32, addr: usize) -> usize {
    let mut w = Writer::new(buf);
    w.str("[FATAL] 未处理异常：进程即将终止 code=0x");
    w.hex_u32(code);
    w.str(" address=0x");
    w.hex_usize(addr);
    w.str(" pid=");
    w.u32(unsafe { GetCurrentProcessId() });
    w.str(" tid=");
    w.u32(unsafe { GetCurrentThreadId() });
    w.str("（这是崩溃取证记录；panic=abort 下不会产生栈回溯）\n");
    w.len()
}

/// 极简、无分配的字节写入器。
struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn bytes(&mut self, b: &[u8]) {
        for &c in b {
            if self.pos >= self.buf.len() {
                return;
            }
            self.buf[self.pos] = c;
            self.pos += 1;
        }
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn u32(&mut self, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut i = tmp.len();
        if v == 0 {
            self.bytes(b"0");
            return;
        }
        while v > 0 && i > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        self.bytes(&tmp[i..]);
    }
    fn hex_u32(&mut self, v: u32) {
        self.hex_bytes(&v.to_be_bytes());
    }
    fn hex_usize(&mut self, v: usize) {
        self.hex_bytes(&v.to_be_bytes());
    }
    fn hex_bytes(&mut self, b: &[u8]) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for &byte in b {
            self.bytes(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xF) as usize]]);
        }
    }
    fn len(&self) -> usize {
        self.pos
    }
}

/// 以追加方式把字节写入日志文件（不分配、忽略错误）。
fn append_raw(data: &[u8]) {
    let Some(path) = LOG_PATH_W.get() else {
        return;
    };
    unsafe {
        let Ok(h) = CreateFileW(
            windows::core::PCWSTR(path.as_ptr()),
            FILE_APPEND_DATA.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) else {
            return;
        };
        if h == INVALID_HANDLE_VALUE {
            return;
        }
        let mut written = 0u32;
        let _ = WriteFile(h, Some(data), Some(&mut written), None);
        let _ = CloseHandle(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 崩溃诊断行必须**完整**落在栈缓冲里。
    ///
    /// `Writer::bytes` 越界时是**静默截断**（崩溃上下文里不能 panic），而被截掉的
    /// 恰好是尾部的 pid/tid —— 崩溃取证最需要的那两个数字。
    #[test]
    fn fatal_line_fits_without_truncation() {
        let mut buf = [0u8; 384];
        let n = write_fatal(&mut buf, 0xC000_0005, 0x7FF6_1234_5678);
        let s = String::from_utf8_lossy(&buf[..n]);
        assert!(
            n > 0 && n < buf.len(),
            "缓冲不应被填满（否则无法排除截断）：{n}"
        );
        assert!(s.contains("[FATAL]"), "{s}");
        assert!(s.contains("code=0xC0000005"), "{s}");
        assert!(s.contains("address=0x00007FF612345678"), "{s}");
        assert!(s.contains("pid="), "{s}");
        assert!(s.contains("tid="), "{s}");
        assert!(
            s.ends_with('\n'),
            "必须以换行结尾，否则会与下一条日志粘连：{s:?}"
        );
    }

    /// 极端值（退出码 0、64 位地址全 1）同样必须完整写出。
    #[test]
    fn fatal_line_handles_extreme_values() {
        let mut buf = [0u8; 384];
        let n = write_fatal(&mut buf, 0, usize::MAX);
        let s = String::from_utf8_lossy(&buf[..n]);
        assert!(n > 0 && n < buf.len(), "极端值也不得填满缓冲：{n}");
        assert!(s.contains("code=0x00000000"), "{s}");
        assert!(s.contains("FFFFFFFFFFFFFFFF"), "{s}");
    }
}
