//! Shared macOS IOReport + SMC sampler.
//!
//! macpow's `IOReportSampler` and `SmcConnection` are stateful: IOReport
//! needs two consecutive samples to derive power (energy/dt), and SMC
//! caches per-key info to avoid re-querying the controller. Both pieces
//! are expensive to spin up and can occasionally stall, so they live on a
//! worker thread. The UI thread only polls the latest completed tick.
//!
//! macpow types are deliberately not re-exported. Each tick returns a
//! [`MacosTick`] typed in syswatch's own data shapes — that way swapping
//! the sampler implementation later (direct IOReport FFI, or a different
//! crate) doesn't ripple into `gpu.rs` and `power.rs`.

#![cfg(target_os = "macos")]

use crate::collect::model::FanTick;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// One tick of macOS-only platform data, post-translated to syswatch
/// types so callers don't see macpow.
#[derive(Debug, Clone, Default)]
pub struct MacosTick {
    /// Per-rail GPU power (W). None on tick 0 (no previous sample yet)
    /// or whenever the IOReport delta couldn't be computed.
    pub gpu_power_w: Option<f32>,
    /// Hottest GPU thermistor (Tg* SMC keys, °C). None when no sensor
    /// reports a fresh, in-range value this tick.
    pub gpu_temp_c: Option<f32>,
    /// Total SoC power across every rail IOReport reports (W).
    pub system_power_w: Option<f32>,
    /// Aggregate CPU power, P-cluster + E-cluster + caches (W).
    pub cpu_power_w: Option<f32>,
    /// Apple Neural Engine power (W). Useful as a "is ML running?" hint.
    pub ane_power_w: Option<f32>,
    /// Fan readings, mapped from SMC into our FanTick shape.
    pub fans: Vec<FanTick>,
}

pub struct MacosSampler {
    rx: Receiver<MacosTick>,
    latest: Option<MacosTick>,
}

struct MacosSamplerWorker {
    sampler: macpow::ioreport::IOReportSampler,
    smc: macpow::smc::SmcConnection,
    prev_sample: Option<macpow::ioreport::Sample>,
}

/// Floor on the macOS sampler cadence. IOReport sampling has measurable
/// overhead (the kernel walks every subscribed channel) and SMC reads
/// hit the controller; sampling faster than this is wasted work and
/// can starve the rest of the system. Users who configure `tick_ms`
/// below this floor still get UI updates at their requested rate —
/// the macOS-specific fields just refresh at the floor.
const MACOS_SAMPLE_FLOOR: Duration = Duration::from_millis(250);

impl MacosSampler {
    /// Start the macOS sampler worker. Any failure to spawn the worker
    /// returns None; initialization failures inside the worker simply leave
    /// callers without a completed tick, which preserves UI responsiveness.
    ///
    /// `tick_ms` is the configured UI sample rate (from `SyswatchConfig`)
    /// — the worker matches that cadence, clamped to `MACOS_SAMPLE_FLOOR`.
    /// Changing `tick_ms` at runtime via the Settings popup currently
    /// requires a restart for the sampler to pick up the new value;
    /// the UI tick rate flips immediately regardless.
    pub fn try_init(tick_ms: u64) -> Option<Self> {
        let (tx, rx) = mpsc::channel();
        let interval = Duration::from_millis(tick_ms).max(MACOS_SAMPLE_FLOOR);
        std::thread::Builder::new()
            .name("syswatch-macos-sampler".into())
            .spawn(move || run_sampler_loop(tx, interval))
            .ok()?;
        Some(Self { rx, latest: None })
    }

    /// Return the most recent completed IOReport + SMC sample without
    /// blocking the UI thread. None means the worker has not produced a
    /// tick yet, or initialization failed inside the worker.
    pub fn tick(&mut self) -> Option<MacosTick> {
        while let Ok(tick) = self.rx.try_recv() {
            self.latest = Some(tick);
        }
        self.latest.clone()
    }
}

/// Initial backoff after a worker panic. Doubles on each subsequent
/// panic up to `BACKOFF_MAX`, then plateaus. Resets to this value
/// after a successful tick.
const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Long-running sampler loop with panic recovery.
///
/// `macpow`'s IOReport / SMC paths drop into OS frameworks that have
/// historically broken on macOS upgrades — without this wrapper, a
/// single panic in `sample_tick` would kill the worker thread and
/// the GPU / power / fan fields would silently show `None` forever.
/// We catch panics, log to stderr with the thread name so users have
/// something to grep, and re-init with exponential backoff so a
/// transient framework wobble doesn't blank platform metrics for
/// the rest of the session.
fn run_sampler_loop(tx: mpsc::Sender<MacosTick>, interval: Duration) {
    let mut backoff = BACKOFF_INITIAL;
    loop {
        let tx = tx.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let Some(mut worker) = MacosSamplerWorker::try_init() else {
                return Err("MacosSamplerWorker::try_init returned None".to_string());
            };
            loop {
                if tx.send(worker.sample_tick()).is_err() {
                    return Ok(());
                }
                std::thread::sleep(interval);
            }
        }));
        match result {
            // Channel disconnected — caller dropped, exit cleanly.
            Ok(Ok(())) => return,
            // Init failure — backoff and retry; usually means SMC /
            // IOReport handles aren't available right now.
            Ok(Err(_msg)) => {}
            // Panic caught — log + backoff + retry.
            Err(payload) => {
                let msg = panic_message(&payload);
                eprintln!(
                    "syswatch-macos-sampler: worker panicked ({msg}); restarting in {:?}",
                    backoff
                );
            }
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

impl MacosSamplerWorker {
    fn try_init() -> Option<Self> {
        let sampler = macpow::ioreport::IOReportSampler::new().ok()?;
        let mut smc = macpow::smc::SmcConnection::open().ok()?;
        // SMC needs a one-time async key-discovery phase. Drive it
        // inside the worker so any slow controller query cannot blank
        // or freeze the terminal UI.
        let handle = smc.start_temp_discovery();
        smc.finish_temp_discovery(handle);
        Some(Self {
            sampler,
            smc,
            prev_sample: None,
        })
    }

    /// Take one IOReport + SMC sample and project it into a `MacosTick`.
    /// Each sub-step is independently fallible; any single failure leaves
    /// that field as None and the others still populate.
    fn sample_tick(&mut self) -> MacosTick {
        let mut out = MacosTick::default();

        if let Ok(cur) = self.sampler.sample() {
            if let Some(prev) = self.prev_sample.as_ref() {
                if let Ok(power) = self.sampler.parse_power(prev, &cur) {
                    out.gpu_power_w = Some(power.gpu_w);
                    out.cpu_power_w = Some(power.cpu_w);
                    out.ane_power_w = Some(power.ane_w);
                    out.system_power_w = Some(power.total_w);
                }
            }
            self.prev_sample = Some(cur);
        }

        // Hottest fresh GPU thermistor. macOS reports several Tg* sensors
        // (die / package / proximity); the hottest is the headline.
        let temps = self.smc.read_temperatures();
        out.gpu_temp_c = temps
            .iter()
            .filter(|t| t.category == "GPU" && !t.stale)
            .map(|t| t.value_celsius)
            .fold(None, |acc, v| Some(acc.map_or(v, |a: f32| a.max(v))));

        // Fans: macpow returns actual + min/max — syswatch's FanTick
        // surfaces actual RPM and the platform-reported max as the
        // "target" (closest analogue when no real target is published).
        out.fans = self
            .smc
            .read_fans()
            .into_iter()
            .map(|f| FanTick {
                name: if f.name.is_empty() {
                    format!("fan{}", f.id)
                } else {
                    f.name
                },
                rpm: f.actual_rpm.max(0.0) as u32,
                target_rpm: if f.max_rpm > 0.0 {
                    Some(f.max_rpm as u32)
                } else {
                    None
                },
            })
            .collect();

        out
    }
}
