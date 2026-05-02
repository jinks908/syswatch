//! Cross-platform GPU discovery + (where free) live util/temp/power.
//!
//! macOS: `system_profiler SPDisplaysDataType -json` runs without sudo and
//! gives us name + vendor + shared VRAM. Live util/temp/power needs either
//! `powermetrics` (sudo) or IOReport (private FFI) — both deferred.
//!
//! Linux: scan `/sys/class/drm/card*/device/` for vendor/device PCI IDs and
//! read `gpu_busy_percent` per tick when the driver exposes it (AMDGPU,
//! recent i915). NVIDIA needs nvml-wrapper — feature-gated, future work.

use crate::collect::model::GpuTick;

const HINT_MACOS: &str =
    "live util/temp/power requires `sudo powermetrics --samplers gpu_power` (deferred)";
const HINT_LINUX_GENERIC: &str =
    "driver doesn't expose gpu_busy_percent; install nvml or amdgpu-tools";

pub struct GpuDiscovery {
    /// Cached at startup (subprocess on macOS is too slow to poll).
    pub devices: Vec<GpuTick>,
}

impl GpuDiscovery {
    pub fn new() -> Self {
        Self {
            devices: discover(),
        }
    }

    /// Refresh per-tick mutable fields (util/temp). On macOS this is a no-op;
    /// on Linux it re-reads gpu_busy_percent.
    #[allow(unused_mut, unused_variables)]
    pub fn refresh(&mut self) -> Vec<GpuTick> {
        let mut out = self.devices.clone();
        #[cfg(target_os = "linux")]
        for (i, dev) in out.iter_mut().enumerate() {
            if let Some(util) = read_linux_busy_percent(i) {
                dev.util_pct = Some(util);
                dev.live_data_hint = None;
            }
        }
        out
    }
}

#[cfg(target_os = "macos")]
fn discover() -> Vec<GpuTick> {
    use std::process::Command;
    let output = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output();
    let Ok(out) = output else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let Ok(parsed): Result<serde_json::Value, _> = serde_json::from_str(&text) else {
        return Vec::new();
    };
    let Some(arr) = parsed.get("SPDisplaysDataType").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .map(|d| {
            let name = d
                .get("sppci_model")
                .or_else(|| d.get("_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown GPU")
                .to_string();
            // SPDisplays returns localization keys like "sppci_vendor_Apple";
            // strip the prefix so the UI shows a real vendor name.
            let vendor = d
                .get("spdisplays_vendor")
                .and_then(|v| v.as_str())
                .map(|s| {
                    s.strip_prefix("sppci_vendor_")
                        .unwrap_or(s)
                        .trim_start_matches("0x")
                        .to_string()
                })
                .unwrap_or_else(|| "Apple".into());
            let vram = d
                .get("spdisplays_vram_shared")
                .or_else(|| d.get("spdisplays_vram"))
                .and_then(|v| v.as_str())
                .and_then(parse_vram_string);
            let driver = d
                .get("spdisplays_metalfamily")
                .or_else(|| d.get("spdisplays_mtlgpufamilysupport"))
                .and_then(|v| v.as_str())
                .map(String::from);
            GpuTick {
                name,
                vendor,
                driver,
                vram_total_bytes: vram,
                vram_used_bytes: None,
                util_pct: None,
                temp_c: None,
                power_w: None,
                live_data_hint: Some(HINT_MACOS.into()),
            }
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn discover() -> Vec<GpuTick> {
    use std::fs;
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Match cardN (no suffix).
        if !name_str.starts_with("card") || name_str.contains('-') {
            continue;
        }
        let card_path = entry.path();
        let device_path = card_path.join("device");
        let vendor_id = fs::read_to_string(device_path.join("vendor"))
            .ok()
            .map(|s| s.trim().to_string());
        let device_id = fs::read_to_string(device_path.join("device"))
            .ok()
            .map(|s| s.trim().to_string());

        let vendor = match vendor_id.as_deref() {
            Some("0x10de") => "NVIDIA",
            Some("0x1002") => "AMD",
            Some("0x8086") => "Intel",
            _ => "Unknown",
        }
        .to_string();
        let name = format!(
            "{} {}",
            vendor,
            device_id.unwrap_or_else(|| "Unknown".into())
        );

        out.push(GpuTick {
            name,
            vendor,
            driver: None,
            vram_total_bytes: None,
            vram_used_bytes: None,
            util_pct: None,
            temp_c: None,
            power_w: None,
            live_data_hint: Some(HINT_LINUX_GENERIC.into()),
        });
    }
    out
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn discover() -> Vec<GpuTick> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn read_linux_busy_percent(card_idx: usize) -> Option<f32> {
    let path = format!("/sys/class/drm/card{}/device/gpu_busy_percent", card_idx);
    let s = std::fs::read_to_string(path).ok()?;
    s.trim().parse::<f32>().ok()
}

#[cfg(target_os = "macos")]
fn parse_vram_string(s: &str) -> Option<u64> {
    // "16 GB" / "8192 MB" / "1024"
    let parts: Vec<&str> = s.split_whitespace().collect();
    let n: f64 = parts.first()?.parse().ok()?;
    let mult: u64 = match parts.get(1).map(|s| s.to_ascii_uppercase()).as_deref() {
        Some("GB") => 1024 * 1024 * 1024,
        Some("MB") => 1024 * 1024,
        Some("KB") => 1024,
        _ => 1,
    };
    Some((n * mult as f64) as u64)
}
