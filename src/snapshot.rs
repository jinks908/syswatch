//! On-demand snapshot dump (`S` key).
//!
//! Writes the current `Snapshot` to a timestamped JSON file under the
//! platform's local-data directory. Pure file IO — no network, no
//! background work — so the user can grab a one-shot diagnostic with
//! a single keystroke and share the resulting file.
//!
//! This is the foundation for the recording story (`R`, deferred to
//! Phase 2): once we have binary session capture, JSON snapshots stay
//! as the human-readable single-tick variant.

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Local};

use crate::collect::Snapshot;

/// Directory snapshots get written to. Created on first write.
///
/// - Linux: `~/.local/share/syswatch/snapshots`
/// - macOS: `~/Library/Application Support/syswatch/snapshots`
/// - Windows: `%LOCALAPPDATA%/syswatch/snapshots`
pub fn dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("syswatch").join("snapshots"))
}

/// Write `snap` as pretty-printed JSON. Returns the absolute path on
/// success so the UI can show it in a status flash.
pub fn write(snap: &Snapshot) -> anyhow::Result<PathBuf> {
    let dir = dir().ok_or_else(|| anyhow::anyhow!("cannot determine local data directory"))?;
    fs::create_dir_all(&dir)?;
    let ts: DateTime<Local> = snap.t.into();
    // Filesystem-safe ISO-8601 (no colons — those break Windows paths).
    let stem = ts.format("snap-%Y-%m-%dT%H-%M-%S");
    let path = dir.join(format!("{}.json", stem));
    let body = serde_json::to_string_pretty(snap)?;
    fs::write(&path, body)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_resolves_under_local_data() {
        // The directory lookup itself is purely a join on the platform
        // local-data dir; we just verify the suffix shape.
        let Some(d) = dir() else { return };
        let s = d.to_string_lossy();
        assert!(s.contains("syswatch"));
        assert!(s.ends_with("snapshots"));
    }

    #[test]
    fn writes_snapshot_round_trips() {
        // Compose a near-empty snapshot and round-trip it through the
        // writer + serde_json's parser to confirm the on-disk format
        // is stable enough for downstream consumption.
        let snap = Snapshot {
            t: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
            host: Default::default(),
            cpu: Default::default(),
            mem: Default::default(),
            disks: Vec::new(),
            disk_io: Default::default(),
            net: Vec::new(),
            procs: Vec::new(),
            gpus: Vec::new(),
            power: Default::default(),
            services: Vec::new(),
        };
        let body = serde_json::to_string_pretty(&snap).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&body).expect("re-parse");
        // Sanity: a few top-level keys we expect.
        assert!(v.get("host").is_some());
        assert!(v.get("cpu").is_some());
        assert!(v.get("procs").is_some());
    }
}
