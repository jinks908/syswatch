use anyhow::Result;
use clap::Parser;

mod app;
mod collect;
mod config;
mod insights;
mod snapshot;
mod tabs;
mod ui;

use config::SyswatchConfig;

#[derive(Parser, Debug)]
#[command(
    name = "syswatch",
    version,
    about = "Single-host system diagnostics TUI"
)]
struct Cli {
    /// Fast-loop tick in milliseconds. Overrides the saved config when supplied.
    #[arg(long)]
    tick: Option<u64>,

    /// Start on a specific tab (overview, cpu, memory, disks, fs, procs, gpu, power, services, net, timeline, insights).
    /// Overrides the saved config.default_tab when supplied.
    #[arg(long)]
    tab: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = SyswatchConfig::load();

    // CLI --tick overrides config for this run only — it's mutated on the
    // in-memory copy so the running app reads it consistently. Saved
    // config on disk stays untouched until the user presses S in settings.
    if let Some(t) = cli.tick {
        cfg.tick_ms = t;
        cfg.validate();
    }

    // Apply theme before any rendering so the first frame uses it.
    ui::theme::set_by_name(&cfg.theme);

    // CLI --tab wins; otherwise fall back to the persisted default.
    let start_tab = cli.tab.or_else(|| Some(cfg.default_tab.clone()));

    app::run(app::Options {
        start_tab,
        config: cfg,
    })
}
