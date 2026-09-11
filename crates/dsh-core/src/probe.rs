//! 就绪探测与挂起自愈。
//!
//! v4 用 WinForms Timer + 后台线程 + HttpWebRequest，靠 `probeGeneration` 代际号
//! 防止陈旧探测结果误判新服务就绪。这里保留代际号语义，但改为可单测的纯逻辑。

use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 默认启动超时：120 秒（与 v4 的 StartupTimeoutMs 一致）。
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
/// 连续失败多少次判定为挂起（v4 为 20 次 × 1.5s ≈ 30 秒）。
pub const DEFAULT_STUCK_THRESHOLD: u32 = 20;

pub struct ReadyProbe {
    port: u16,
    /// 探测代际号。
    ///
    /// **必须是 `Arc`**：`Clone` 此前会新建一个独立的 `AtomicU64`（只复制当前值），
    /// 于是 `bump_generation()` 对已经克隆出去的副本**完全无效**——
    /// 「代际号变化 ⇒ 在途探测立即作废」这条语义在克隆路径上是失效的
    /// （`wait_for_ready` 里的 `GenerationChanged` 分支因此永不可达）。
    /// 共享同一个计数器后，任何一处 bump 都能让所有副本立刻看到。
    generation: Arc<AtomicU64>,
}

impl Clone for ReadyProbe {
    fn clone(&self) -> Self {
        Self {
            port: self.port,
            generation: Arc::clone(&self.generation),
        }
    }
}

impl ReadyProbe {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_port(&mut self, port: u16) {
        self.port = port;
    }

    /// 每次启动/重启服务前递增，使在途的旧探测立即失效。
    pub fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// 阻塞等待就绪：退避 200ms → 500ms → 1s，直到超时。
    ///
    /// 代际号变化立即返回 `GenerationChanged`，避免把旧服务的结果算到新服务头上。
    ///
    /// **总耗时严格受 `timeout` 约束**：每次 TCP 探测的 connect 超时、以及两次
    /// 探测之间的退避睡眠，都被剩余预算截断。旧实现只检查“探测前后是否超时”，
    /// 单次探测自带固定超时，实测 300ms 预算会跑成 803ms。
    pub fn wait_for_ready(&self, timeout: Duration) -> Result<(), ProbeError> {
        const PROBE_CEILING: Duration = Duration::from_millis(800);
        let gen = self.generation();
        let mut interval = Duration::from_millis(200);
        let start = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(ProbeError::Timeout);
            }
            if probe_port_within(self.port, remaining.min(PROBE_CEILING)) {
                return Ok(());
            }
            if self.generation() != gen {
                return Err(ProbeError::GenerationChanged);
            }
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(ProbeError::Timeout);
            }
            std::thread::sleep(interval.min(remaining));
            interval = (interval * 2).min(Duration::from_secs(1));
        }
    }
}

/// 带自定义上限的单次探测。
///
/// 用 `SocketAddr::from` 直接构造地址，不做字符串解析——
/// 旧实现的 `format!("127.0.0.1:{port}").parse().unwrap()` 是可 panic 路径。
pub fn probe_port_within(port: u16, timeout: Duration) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), timeout).is_ok()
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("等待服务就绪超时")]
    Timeout,
    #[error("服务已重启（代际号变化），本次探测作废")]
    GenerationChanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_bumps() {
        let p = ReadyProbe::new(3080);
        assert_eq!(p.generation(), 0);
        assert_eq!(p.bump_generation(), 1);
        assert_eq!(p.generation(), 1);
    }

    #[test]
    fn clone_shares_generation_counter() {
        // 关键不变式：克隆必须**共享**代际号计数器。
        // 此前 clone 只复制当前值 → 对副本 bump 无效，
        // `wait_for_ready` 的 GenerationChanged 取消路径在克隆上永不可达。
        let p = ReadyProbe::new(3080);
        let clone = p.clone();
        p.bump_generation();
        assert_eq!(clone.generation(), 1, "克隆必须看到原始实例的 bump");
        clone.bump_generation();
        assert_eq!(p.generation(), 2, "原始实例必须看到克隆的 bump");
    }

    #[test]
    fn in_flight_probe_is_cancelled_by_bump() {
        // 端到端语义：一个正在等待的探测必须在别处 bump 后**立即**返回
        // GenerationChanged，而不是继续把旧服务的结果算到新服务头上。
        // 先绑一个端口再释放，确保这个端口此刻确实没人监听
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let p = l.local_addr().unwrap().port();
            drop(l);
            p
        };
        let p = ReadyProbe::new(port);
        let canceller = p.clone();
        let handle = std::thread::spawn(move || p.wait_for_ready(Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(120));
        canceller.bump_generation();
        let r = handle.join().unwrap();
        assert!(
            matches!(r, Err(ProbeError::GenerationChanged)),
            "应被代际号变化取消，实际 {r:?}"
        );
    }

    #[test]
    fn times_out_when_nothing_listens() {
        let p = ReadyProbe::new(45672);
        let r = p.wait_for_ready(Duration::from_millis(250));
        assert!(matches!(r, Err(ProbeError::Timeout)));
    }

    #[test]
    fn wait_never_exceeds_timeout() {
        // 旧实现：单次探测自带 800ms 超时且不受剩余预算约束，
        // 300ms 预算实测会跑成 ~803ms。现在必须严格收敛在预算内。
        let p = ReadyProbe::new(45673);
        let budget = Duration::from_millis(300);
        let start = std::time::Instant::now();
        assert!(p.wait_for_ready(budget).is_err());
        let spent = start.elapsed();
        assert!(
            spent < budget + Duration::from_millis(120),
            "等待耗时 {spent:?} 明显超出预算 {budget:?}"
        );
    }

    #[test]
    fn probe_within_respects_its_own_budget() {
        // 极短超时也必须按时返回，不能挂满默认 800ms
        let start = std::time::Instant::now();
        assert!(!probe_port_within(45674, Duration::from_millis(50)));
        let spent = start.elapsed();
        assert!(spent < Duration::from_millis(300), "耗时 {spent:?}");
    }

    #[test]
    fn returns_ok_when_port_opens() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let p = ReadyProbe::new(port);
        assert!(p.wait_for_ready(Duration::from_secs(2)).is_ok());
    }
}
