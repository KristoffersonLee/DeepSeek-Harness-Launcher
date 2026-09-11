//! 端口探测：不需要管理员权限。
//!
//! v4 用 WMI + `netstat` 双路径，受限权限下 WMI 读不到进程命令行、`netstat` 偶发失败。
//! 这里直接用 `GetExtendedTcpTable`（iphlpapi），一次调用拿到监听端口与 PID。

use core::ffi::c_void;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::Networking::WinSock::AF_INET;

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
/// 本地回环地址，避免每次探测都做字符串解析。
const LOOPBACK: [u8; 4] = [127, 0, 0, 1];

/// 返回在 `port` 上处于 LISTEN 状态的进程 PID。
pub fn find_pid_on_port(port: u16) -> Option<u32> {
    unsafe {
        let mut size = 0u32;
        let r = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if r != 0 && r != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        // 未占用任何端口时 size 可能为 0，此时无需分配
        if size == 0 {
            return None;
        }

        // **缓冲区必须是 u32 对齐的**：`MIB_TCPTABLE_OWNER_PID` 的对齐要求是 4，
        // 而 `Vec<u8>` 只保证 1 字节对齐（分配器实际给 8/16 字节，但那是实现细节，
        // 不是语言保证）。用 `Vec<u32>` 承载即可从类型上满足对齐要求，
        // 避免「把未对齐缓冲当成结构体解引用」这一类 UB。
        let mut buf: Vec<u32> = vec![0; (size as usize).div_ceil(4)];
        let buf_bytes = buf.len() * 4;
        let mut size2 = buf_bytes as u32;
        let r = GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size2,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if r != 0 {
            return None;
        }

        // 复核返回的缓冲区：表头 + dwNumEntries 个表项都必须落在缓冲区内，
        // 否则切片会读出越界内存（UB）而不是返回 None。
        //
        // 表项起点是**表头字段的偏移**（`dwNumEntries` 之后），不是
        // `size_of::<MIB_TCPTABLE_OWNER_PID>()`（后者含一个内联元素，会多算 24 字节）。
        const COUNT_SIZE: usize = std::mem::size_of::<u32>();
        const ENTRY_SIZE: usize = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        if buf_bytes < COUNT_SIZE + ENTRY_SIZE {
            return None;
        }
        let base = buf.as_ptr() as *const u8;
        // 读头部字段用 read_unaligned：不依赖缓冲对齐（虽然上面已经保证了 4 字节）
        let count = std::ptr::read_unaligned(base as *const u32) as usize;
        if buf_bytes < COUNT_SIZE + count.saturating_mul(ENTRY_SIZE) {
            return None;
        }

        let entries =
            std::slice::from_raw_parts(base.add(COUNT_SIZE) as *const MIB_TCPROW_OWNER_PID, count);
        // dwLocalPort 为 u32，低 16 位是网络字节序端口号
        let target = u32::from(port.to_be());
        entries
            .iter()
            .find(|e| e.dwState == MIB_TCP_STATE_LISTEN.0 as u32 && e.dwLocalPort == target)
            .map(|e| e.dwOwningPid)
    }
}

/// 端口是否可连接（TCP 三次握手成功即认为在监听）。
pub fn is_port_listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((LOOPBACK, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// 端口是否可被重新绑定（v4.2.4 起的权威判定：进程消失 + 端口可重绑）。
pub fn is_port_free_to_bind(port: u16) -> bool {
    std::net::TcpListener::bind(SocketAddr::from((LOOPBACK, port))).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_high_port_has_no_listener() {
        // 选一个极不可能被占用的端口
        assert!(find_pid_on_port(45671).is_none());
        assert!(!is_port_listening(45671));
    }

    #[test]
    fn bound_port_is_discoverable() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let pid = find_pid_on_port(port);
        assert!(pid.is_some(), "应能找到监听 {port} 的进程");
        assert_eq!(pid.unwrap(), std::process::id());
        assert!(is_port_listening(port));
    }

    #[test]
    fn occupied_port_cannot_rebind() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_port_free_to_bind(port));
    }
}
