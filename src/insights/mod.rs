//! Heuristic anomaly detection over the rolling session.
//!
//! Pure functions: each `insight_*` reads `(History, &Snapshot)` and returns
//! `Option<Insight>`. `compute()` runs them all and sorts by severity.
//!
//! Read-only by design — Insights surface what to look at, never mutate.

use crate::app::{History, TabId};
use crate::collect::Snapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warn,
    Crit,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "INFO",
            Severity::Warn => "WARN",
            Severity::Crit => "CRIT",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Insight {
    pub severity: Severity,
    pub title: String,
    pub body: Vec<String>,
    pub suggested_tab: TabId,
}

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;

pub fn compute(h: &History, snap: &Snapshot) -> Vec<Insight> {
    let mut out: Vec<Insight> = Vec::new();
    if let Some(i) = insight_swap_thrash(h, snap) {
        out.push(i);
    }
    if let Some(i) = insight_runaway_proc(h, snap) {
        out.push(i);
    }
    if let Some(i) = insight_disk_full(snap) {
        out.push(i);
    }
    if let Some(i) = insight_memory_pressure(h, snap) {
        out.push(i);
    }
    if let Some(i) = insight_high_load(snap) {
        out.push(i);
    }
    if let Some(i) = insight_zombie_party(snap) {
        out.push(i);
    }

    // Most severe first; cap to the spec's ~6 cards budget.
    out.sort_by(|a, b| b.severity.cmp(&a.severity));
    out.truncate(6);
    out
}

/// Swap usage growing meaningfully over the rolling window.
fn insight_swap_thrash(h: &History, snap: &Snapshot) -> Option<Insight> {
    let history = h.swap.to_vec();
    if history.len() < 8 {
        return None;
    }
    let now = *history.last().unwrap();
    if now < 64 * MIB {
        return None;
    }
    // Compare current to value ~30 ticks ago (or the oldest we have).
    let baseline_idx = history.len().saturating_sub(30);
    let baseline = history[baseline_idx];
    let growth = now.saturating_sub(baseline);

    let (severity, title) = if growth >= 512 * MIB {
        (
            Severity::Crit,
            format!(
                "swap thrash — {:.1} GB swapped, +{:.0} MB in last {}s",
                now as f64 / GIB as f64,
                growth as f64 / MIB as f64,
                history.len() - baseline_idx
            ),
        )
    } else if growth >= 100 * MIB {
        (
            Severity::Warn,
            format!(
                "memory pressure — swap rising +{:.0} MB over last {}s",
                growth as f64 / MIB as f64,
                history.len() - baseline_idx
            ),
        )
    } else {
        return None;
    };

    let top_rss = snap
        .procs
        .iter()
        .max_by_key(|p| p.mem_rss)
        .map(|p| {
            format!(
                "{} holds the largest resident set ({}).",
                p.name,
                fmt_bytes(p.mem_rss)
            )
        })
        .unwrap_or_default();

    Some(Insight {
        severity,
        title,
        body: vec![
            format!(
                "swap is {} of {} configured.",
                fmt_bytes(snap.mem.swap_used_bytes),
                fmt_bytes(snap.mem.swap_total_bytes.max(1))
            ),
            top_rss,
        ],
        suggested_tab: TabId::Memory,
    })
}

/// A single process whose CPU has been sustained high (EWMA over recent ticks).
fn insight_runaway_proc(h: &History, snap: &Snapshot) -> Option<Insight> {
    let mut top: Option<(u32, f32)> = None;
    for (pid, ewma) in &h.proc_cpu_ewma {
        if *ewma >= 50.0 {
            if top.map_or(true, |(_, v)| *ewma > v) {
                top = Some((*pid, *ewma));
            }
        }
    }
    let (pid, ewma) = top?;
    let proc_ = snap.procs.iter().find(|p| p.pid == pid)?;
    let severity = if ewma >= 90.0 {
        Severity::Crit
    } else {
        Severity::Warn
    };
    Some(Insight {
        severity,
        title: format!(
            "runaway process — {} (pid {}) sustained {:.0}% CPU",
            proc_.name, pid, ewma
        ),
        body: vec![
            format!(
                "instantaneous {:.1}% / RSS {} / state {}",
                proc_.cpu_pct,
                fmt_bytes(proc_.mem_rss),
                proc_.state
            ),
            format!("user {} / ppid {}", proc_.user, proc_.ppid),
        ],
        suggested_tab: TabId::Procs,
    })
}

/// Most-full mount above the warn/crit threshold.
fn insight_disk_full(snap: &Snapshot) -> Option<Insight> {
    let worst = snap
        .disks
        .iter()
        .filter(|d| d.total_bytes > 0)
        .max_by(|a, b| {
            a.usage_pct
                .partial_cmp(&b.usage_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    let severity = if worst.usage_pct >= 95.0 {
        Severity::Crit
    } else if worst.usage_pct >= 85.0 {
        Severity::Warn
    } else {
        return None;
    };
    let warn_count = snap.disks.iter().filter(|d| d.usage_pct >= 85.0).count();
    Some(Insight {
        severity,
        title: format!(
            "{} is {:.1}% full ({} of {})",
            worst.mount_point,
            worst.usage_pct,
            fmt_bytes(worst.used_bytes),
            fmt_bytes(worst.total_bytes)
        ),
        body: vec![
            format!(
                "{} free / fs {}",
                fmt_bytes(worst.available_bytes),
                worst.fs_type
            ),
            if warn_count > 1 {
                format!("{} mounts are above 85% utilization.", warn_count)
            } else {
                "no other mounts above 85% utilization.".into()
            },
        ],
        suggested_tab: TabId::Fs,
    })
}

/// RAM utilization sustained high over the window.
fn insight_memory_pressure(h: &History, snap: &Snapshot) -> Option<Insight> {
    if snap.mem.total_bytes == 0 {
        return None;
    }
    let recent: Vec<f32> = h.mem.to_vec();
    let last_n = 6usize.min(recent.len());
    if last_n < 3 {
        return None;
    }
    let avg = recent[recent.len() - last_n..].iter().sum::<f32>() / last_n as f32;
    let severity = if avg >= 0.95 {
        Severity::Crit
    } else if avg >= 0.85 {
        Severity::Warn
    } else {
        return None;
    };
    Some(Insight {
        severity,
        title: format!(
            "memory pressure — RAM {:.0}% used over last {}s",
            avg * 100.0,
            last_n
        ),
        body: vec![
            format!(
                "{} of {} ({} available)",
                fmt_bytes(snap.mem.used_bytes),
                fmt_bytes(snap.mem.total_bytes),
                fmt_bytes(snap.mem.available_bytes)
            ),
            "Sustained high pressure typically precedes swap activity or OOM kills.".into(),
        ],
        suggested_tab: TabId::Memory,
    })
}

/// Load average meaningfully above core count.
fn insight_high_load(snap: &Snapshot) -> Option<Insight> {
    let cores = snap.cpu.per_core.len().max(1) as f32;
    let load = snap.cpu.load_1;
    if load < cores * 1.5 {
        return None;
    }
    let severity = if load >= cores * 4.0 {
        Severity::Crit
    } else if load >= cores * 2.0 {
        Severity::Warn
    } else {
        Severity::Info
    };
    if severity == Severity::Info {
        return None;
    }
    Some(Insight {
        severity,
        title: format!(
            "load {:.2} on {} cores ({:.1}× saturation)",
            load,
            cores as u32,
            load / cores
        ),
        body: vec![
            format!(
                "load 1m / 5m / 15m  =  {:.2} / {:.2} / {:.2}",
                snap.cpu.load_1, snap.cpu.load_5, snap.cpu.load_15
            ),
            "Sustained load above 2× cores indicates a queue forming on the run queue.".into(),
        ],
        suggested_tab: TabId::Cpu,
    })
}

/// More than a handful of zombie processes — a parent isn't reaping.
fn insight_zombie_party(snap: &Snapshot) -> Option<Insight> {
    let zombies: Vec<&crate::collect::ProcTick> =
        snap.procs.iter().filter(|p| p.state == 'Z').collect();
    if zombies.len() < 5 {
        return None;
    }
    let severity = if zombies.len() >= 25 {
        Severity::Crit
    } else {
        Severity::Warn
    };
    let parents: Vec<u32> = {
        let mut v: Vec<u32> = zombies.iter().map(|z| z.ppid).collect();
        v.sort_unstable();
        v.dedup();
        v.truncate(5);
        v
    };
    Some(Insight {
        severity,
        title: format!("{} zombie processes — parent isn't reaping", zombies.len()),
        body: vec![
            format!(
                "Common parent pids: {}",
                parents
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "Restart the parent or send it SIGCHLD; zombies hold no resources but indicate a bug.".into(),
        ],
        suggested_tab: TabId::Procs,
    })
}

fn fmt_bytes(b: u64) -> String {
    crate::ui::widgets::human_bytes(b)
}
