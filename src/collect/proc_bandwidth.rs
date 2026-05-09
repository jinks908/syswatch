//! Per-process network bandwidth attribution.
//!
//! Wraps `netwatch_sdk::collectors::process_bandwidth::attribute`, which
//! splits the host's interface throughput proportionally to each
//! process's ESTABLISHED connection count. The kernel doesn't expose
//! true per-PID byte accounting cheaply on either macOS or Linux, so
//! this is an approximation — but it's the same shape netwatch's TUI
//! uses, and good enough to answer "which process is eating the wire."
//!
//! Costs: `lsof` (macOS) and `ss` (Linux) take 50–500 ms on a busy host.
//! We cache the per-PID map for `REFRESH` and re-sample no more often
//! than that, so the procs tab stays smooth even at sub-second tick
//! rates.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use netwatch_sdk::collectors::connections::collect_connections;
use netwatch_sdk::collectors::process_bandwidth::attribute;
use netwatch_sdk::types::InterfaceMetric;

use super::model::InterfaceTick;

const REFRESH: Duration = Duration::from_secs(2);
/// Top-N cap matches the "top X procs" intuition without unbounded
/// growth on hosts with thousands of connections.
const MAX_PROCS: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct ProcessBandwidthCollector {
    last_at: Option<Instant>,
    cached: HashMap<u32, (f64, f64)>, // pid -> (rx_rate, tx_rate)
}

impl ProcessBandwidthCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the per-PID rate map. Re-collects at most every REFRESH;
    /// otherwise returns the cached map. `current_net` is whatever the
    /// outer Collector just gathered for interfaces — we re-shape it
    /// into the SDK's `InterfaceMetric` so the attribution function can
    /// allocate by interface throughput.
    pub fn sample(&mut self, current_net: &[InterfaceTick]) -> HashMap<u32, (f64, f64)> {
        let stale = self.last_at.map(|t| t.elapsed() >= REFRESH).unwrap_or(true);
        if stale {
            self.cached = compute(current_net);
            self.last_at = Some(Instant::now());
        }
        self.cached.clone()
    }
}

fn compute(current_net: &[InterfaceTick]) -> HashMap<u32, (f64, f64)> {
    let conns = collect_connections();
    if conns.is_empty() {
        return HashMap::new();
    }
    let metrics: Vec<InterfaceMetric> = current_net
        .iter()
        .map(|i| InterfaceMetric {
            name: i.name.clone(),
            is_up: i.is_up,
            rx_bytes: i.rx_bytes,
            tx_bytes: i.tx_bytes,
            rx_bytes_delta: 0,
            tx_bytes_delta: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_drops: 0,
            tx_drops: 0,
            rx_rate: Some(i.rx_rate),
            tx_rate: Some(i.tx_rate),
            rx_history: None,
            tx_history: None,
        })
        .collect();
    let attributed = attribute(&conns, &metrics, MAX_PROCS);
    attributed
        .into_iter()
        .filter_map(|p| p.pid.map(|pid| (pid, (p.rx_rate, p.tx_rate))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_interfaces_yields_zero_rates() {
        // No interfaces means the SDK has no throughput to allocate;
        // any PIDs returned (real ESTABLISHED conns on the test host)
        // must therefore have zero rates. We don't assert empty
        // because the host running tests typically has active sockets.
        let map = compute(&[]);
        for (_pid, (rx, tx)) in &map {
            assert_eq!(*rx, 0.0);
            assert_eq!(*tx, 0.0);
        }
    }

    #[test]
    fn cache_short_circuits_within_refresh_window() {
        let mut c = ProcessBandwidthCollector::new();
        // First call populates cached + last_at; subsequent calls
        // within REFRESH should not re-invoke compute (we can't easily
        // verify the no-call directly, but we can verify the timestamp
        // doesn't move and the cache shape is stable).
        let _ = c.sample(&[]);
        let first_at = c.last_at;
        let _ = c.sample(&[]);
        assert_eq!(c.last_at, first_at);
    }
}
