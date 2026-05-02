//! Battery / thermal / fan collection.
//!
//! macOS (no sudo): `ioreg -rn AppleSmartBattery` for charge / cycles / health
//! / temperature / system power draw (V × A from the battery). `pmset -g batt`
//! tells us "drawing from AC" vs "Battery". `pmset -g therm` reports a CPU
//! Speed Limit when thermal throttling kicks in. Fans + per-component power
//! need `powermetrics` (sudo) — surfaced as a hint.
//!
//! Linux: `/sys/class/power_supply/BAT*/*` + `/sys/class/thermal/thermal_zone*/temp`
//! + `/sys/class/hwmon/hwmon*/fan*_input`. All readable without sudo.

use std::time::{Duration, Instant};

use crate::collect::model::*;

const HINT_MACOS_FANS_POWER: &str =
    "fans + per-component power need `sudo powermetrics --samplers thermal,smc` (deferred)";

/// Battery / thermal data changes slowly — refreshing on every 1Hz tick would
/// spawn 3 subprocesses per second on macOS. We cache the last result and
/// re-sample at most every REFRESH interval; the UI can keep pace with the
/// fast loop without paying the subprocess tax.
const REFRESH: Duration = Duration::from_secs(5);

pub struct PowerCollector {
    last_sample_at: Option<Instant>,
    cached: PowerTick,
}

impl PowerCollector {
    pub fn new() -> Self {
        Self {
            last_sample_at: None,
            cached: PowerTick::default(),
        }
    }

    pub fn sample(&mut self) -> PowerTick {
        let stale = self
            .last_sample_at
            .map(|t| t.elapsed() >= REFRESH)
            .unwrap_or(true);
        if stale {
            self.cached = sample_inner();
            self.last_sample_at = Some(Instant::now());
        }
        self.cached.clone()
    }
}

#[cfg(target_os = "macos")]
fn sample_inner() -> PowerTick {
    use std::process::Command;

    let mut tick = PowerTick::default();

    // Battery + power-draw from ioreg AppleSmartBattery.
    if let Ok(out) = Command::new("ioreg").args(["-rn", "AppleSmartBattery"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        let battery = parse_macos_ioreg_battery(&text);
        tick.system_power_w = battery
            .as_ref()
            .and_then(|b| match (b.voltage_v, b.amperage_ma) {
                (Some(v), Some(a)) => Some(v * (a.unsigned_abs() as f32) / 1000.0),
                _ => None,
            });
        tick.battery = battery;
    }

    // Power source from pmset -g batt's first line: "Now drawing from 'X Power'".
    if let Ok(out) = Command::new("pmset").args(["-g", "batt"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        tick.source = parse_macos_pmset_source(&text);
    }

    // Thermal throttle from pmset -g therm. If the line "CPU_Speed_Limit = N"
    // is present we use N; if pmset only prints "no warning level recorded"
    // we know the system is fine → 100.
    if let Ok(out) = Command::new("pmset").args(["-g", "therm"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        tick.thermal_throttle_pct = Some(parse_macos_pmset_throttle(&text));
    }

    tick.live_data_hint = Some(HINT_MACOS_FANS_POWER.into());
    tick
}

#[cfg(target_os = "linux")]
fn sample_inner() -> PowerTick {
    use std::fs;
    use std::path::Path;

    let mut tick = PowerTick::default();

    // First /sys/class/power_supply with type=Battery wins.
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let path = entry.path();
            let supply_type = read_trim(&path.join("type"));
            match supply_type.as_deref() {
                Some("Battery") => {
                    let bat = parse_linux_battery(&path);
                    tick.system_power_w = derive_linux_power_w(&path);
                    tick.battery = Some(bat);
                }
                Some("Mains") | Some("UPS") => {
                    if read_trim(&path.join("online")).as_deref() == Some("1") {
                        tick.source = PowerSource::Ac;
                    }
                }
                _ => {}
            }
        }
    }
    if tick.source == PowerSource::Unknown && tick.battery.is_some() {
        tick.source = PowerSource::Battery;
    }

    // Thermal zones.
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with("thermal_zone") {
                continue;
            }
            let path = entry.path();
            let zone_type = read_trim(&path.join("type")).unwrap_or_else(|| name_str.to_string());
            let temp_milli = read_trim(&path.join("temp"))
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            tick.thermal_zones.push(ThermalZone {
                name: zone_type,
                temp_c: temp_milli as f32 / 1000.0,
            });
        }
    }

    // Fans via hwmon (Linux exposes them per-chip, names vary).
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let chip = entry.path();
            for i in 1..=8 {
                let input = chip.join(format!("fan{}_input", i));
                if !Path::new(&input).exists() {
                    break;
                }
                let rpm = read_trim(&input).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                if rpm == 0 {
                    continue;
                }
                let label = read_trim(&chip.join(format!("fan{}_label", i)))
                    .unwrap_or_else(|| format!("fan{}", i));
                let target = read_trim(&chip.join(format!("fan{}_target", i)))
                    .and_then(|s| s.parse::<u32>().ok());
                tick.fans.push(FanTick {
                    name: label,
                    rpm,
                    target_rpm: target,
                });
            }
        }
    }

    // Linux exposes throttling indirectly (cpufreq, throttle_count). Skipping
    // until we add a cpufreq collector — leave None so the UI shows "—".
    tick.thermal_throttle_pct = None;
    tick
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn sample_inner() -> PowerTick {
    PowerTick::default()
}

// ───────────────────────── parsers ─────────────────────────

#[cfg(target_os = "macos")]
fn parse_macos_ioreg_battery(text: &str) -> Option<BatteryTick> {
    let mut bat = BatteryTick::default();
    let mut saw_charge = false;
    for line in text.lines() {
        let line = line.trim();
        // Lines look like:  "FieldName" = value
        let Some(eq) = line.find(" = ") else { continue };
        let key = line[..eq].trim().trim_matches('"');
        let val = line[eq + 3..].trim();
        match key {
            "CurrentCapacity" => {
                bat.charge_pct = val.parse::<f32>().unwrap_or(0.0);
                saw_charge = true;
            }
            "MaxCapacity" => {
                // MaxCapacity in ioreg AppleSmartBattery is the *current* full
                // capacity expressed as a % of design — i.e. battery health.
                bat.health_pct = val.parse::<f32>().ok();
            }
            "CycleCount" => bat.cycle_count = val.parse().ok(),
            "Temperature" => {
                bat.temp_c = val.parse::<f32>().ok().map(|v| v / 100.0);
            }
            "Voltage" => bat.voltage_v = val.parse::<f32>().ok().map(|v| v / 1000.0),
            "Amperage" => {
                // ioreg prints this as an unsigned 64-bit int even though it's
                // semantically signed. Round-trip through u64 -> i64.
                bat.amperage_ma = val.parse::<u64>().ok().map(|v| v as i64 as i32);
            }
            "TimeRemaining" => {
                bat.time_remaining_min = val.parse::<u32>().ok().filter(|v| *v > 0 && *v < 60_000);
            }
            "IsCharging" => bat.is_charging = val.eq_ignore_ascii_case("Yes"),
            "FullyCharged" => bat.fully_charged = val.eq_ignore_ascii_case("Yes"),
            _ => {}
        }
    }
    if saw_charge {
        Some(bat)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn parse_macos_pmset_source(text: &str) -> PowerSource {
    for line in text.lines() {
        if let Some(start) = line.find("drawing from '") {
            let rest = &line[start + "drawing from '".len()..];
            if let Some(end) = rest.find('\'') {
                let label = &rest[..end];
                if label.starts_with("AC") {
                    return PowerSource::Ac;
                }
                if label.starts_with("Battery") {
                    return PowerSource::Battery;
                }
            }
        }
    }
    PowerSource::Unknown
}

#[cfg(target_os = "macos")]
fn parse_macos_pmset_throttle(text: &str) -> u32 {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("CPU_Speed_Limit") {
            // "CPU_Speed_Limit \t= 87"
            if let Some(eq) = rest.find('=') {
                if let Ok(n) = rest[eq + 1..].trim().parse::<u32>() {
                    return n;
                }
            }
        }
    }
    100
}

#[cfg(target_os = "linux")]
fn parse_linux_battery(path: &std::path::Path) -> BatteryTick {
    let mut bat = BatteryTick::default();
    bat.charge_pct = read_trim(&path.join("capacity"))
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let status = read_trim(&path.join("status")).unwrap_or_default();
    bat.is_charging = status.eq_ignore_ascii_case("Charging");
    bat.fully_charged = status.eq_ignore_ascii_case("Full");
    bat.cycle_count = read_trim(&path.join("cycle_count")).and_then(|s| s.parse().ok());
    bat.voltage_v = read_trim(&path.join("voltage_now"))
        .and_then(|s| s.parse::<f32>().ok())
        .map(|v| v / 1_000_000.0);
    bat.amperage_ma = read_trim(&path.join("current_now"))
        .and_then(|s| s.parse::<i64>().ok())
        .map(|v| (v / 1000) as i32);
    bat.temp_c = read_trim(&path.join("temp"))
        .and_then(|s| s.parse::<f32>().ok())
        .map(|v| v / 10.0);
    let energy_full_design = read_trim(&path.join("energy_full_design"))
        .and_then(|s| s.parse::<f32>().ok());
    let energy_full = read_trim(&path.join("energy_full"))
        .and_then(|s| s.parse::<f32>().ok());
    if let (Some(d), Some(f)) = (energy_full_design, energy_full) {
        if d > 0.0 {
            bat.health_pct = Some((f / d * 100.0).clamp(0.0, 100.0));
        }
    }
    bat
}

#[cfg(target_os = "linux")]
fn derive_linux_power_w(path: &std::path::Path) -> Option<f32> {
    // power_now is in microwatts; voltage*current is the fallback.
    if let Some(uw) = read_trim(&path.join("power_now")).and_then(|s| s.parse::<f32>().ok()) {
        return Some(uw / 1_000_000.0);
    }
    let v_uv = read_trim(&path.join("voltage_now")).and_then(|s| s.parse::<f32>().ok())?;
    let c_ua = read_trim(&path.join("current_now")).and_then(|s| s.parse::<f32>().ok())?;
    Some(v_uv * c_ua.abs() / 1e12)
}

#[cfg(target_os = "linux")]
fn read_trim(p: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_real_ioreg_sample() {
        // Captured from a real MacBook running this branch.
        let sample = r#"
      "CurrentCapacity" = 74
      "TimeRemaining" = 378
      "Amperage" = 18446744073709551133
      "FullyCharged" = No
      "MaxCapacity" = 100
      "Temperature" = 3064
      "DesignCapacity" = 6249
      "IsCharging" = No
      "Voltage" = 12135
      "CycleCount" = 91
        "#;
        let bat = parse_macos_ioreg_battery(sample).expect("battery parsed");
        assert_eq!(bat.charge_pct as i32, 74);
        assert_eq!(bat.cycle_count, Some(91));
        assert_eq!(bat.health_pct, Some(100.0));
        assert!(!bat.is_charging);
        assert_eq!(bat.time_remaining_min, Some(378));
        assert!((bat.voltage_v.unwrap() - 12.135).abs() < 0.001);
        assert!((bat.temp_c.unwrap() - 30.64).abs() < 0.01);
        // 18446744073709551133 == -483 as i64 → -483 mA.
        assert_eq!(bat.amperage_ma, Some(-483));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_pmset_source_ac_and_battery() {
        let bat_sample = "Now drawing from 'Battery Power'\n -InternalBattery-0\t75%; discharging; 4:23 remaining present: true";
        assert_eq!(parse_macos_pmset_source(bat_sample), PowerSource::Battery);
        let ac_sample = "Now drawing from 'AC Power'";
        assert_eq!(parse_macos_pmset_source(ac_sample), PowerSource::Ac);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pmset_no_throttle_returns_100() {
        let healthy = "Note: No thermal warning level has been recorded";
        assert_eq!(parse_macos_pmset_throttle(healthy), 100);
        let throttled = "CPU_Scheduler_Limit \t= 100\nCPU_Available_CPUs \t= 14\nCPU_Speed_Limit \t= 87";
        assert_eq!(parse_macos_pmset_throttle(throttled), 87);
    }
}
