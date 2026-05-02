use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Terminal;

use crate::collect::{Collector, Ring};
pub use crate::collect::Snapshot;
use crate::tabs;
use crate::ui::chrome;

pub struct Options {
    pub tick_ms: u64,
    pub start_tab: Option<String>,
}

pub struct History {
    /// Aggregate CPU usage % (0..100), one sample per tick.
    pub cpu: Ring<f32>,
    /// Memory used / total ratio (0..1).
    pub mem: Ring<f32>,
    /// Net rx+tx bytes/sec aggregated.
    pub net_rate: Ring<f64>,
    /// Disk rd+wr bytes/sec aggregated.
    pub io_rate: Ring<f64>,
}

impl History {
    fn new(cap: usize) -> Self {
        Self {
            cpu: Ring::new(cap),
            mem: Ring::new(cap),
            net_rate: Ring::new(cap),
            io_rate: Ring::new(cap),
        }
    }

    fn push(&mut self, snap: &Snapshot) {
        self.cpu.push(snap.cpu.usage_pct);
        let m = if snap.mem.total_bytes > 0 {
            (snap.mem.used_bytes as f32) / (snap.mem.total_bytes as f32)
        } else {
            0.0
        };
        self.mem.push(m);
        let net = snap.net.iter().map(|i| i.rx_rate + i.tx_rate).sum::<f64>();
        self.net_rate.push(net);
        self.io_rate
            .push(snap.disk_io.read_rate + snap.disk_io.write_rate);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabId {
    Overview,
    Cpu,
    Memory,
    Disks,
    Fs,
    Procs,
    Gpu,
    Power,
    Services,
    Net,
    Timeline,
    Insights,
}

pub const ALL_TABS: &[TabId] = &[
    TabId::Overview,
    TabId::Cpu,
    TabId::Memory,
    TabId::Disks,
    TabId::Fs,
    TabId::Procs,
    TabId::Gpu,
    TabId::Power,
    TabId::Services,
    TabId::Net,
    TabId::Timeline,
    TabId::Insights,
];

impl TabId {
    pub fn glyph(&self) -> &'static str {
        match self {
            TabId::Overview => "1",
            TabId::Cpu => "2",
            TabId::Memory => "3",
            TabId::Disks => "4",
            TabId::Fs => "5",
            TabId::Procs => "6",
            TabId::Gpu => "7",
            TabId::Power => "8",
            TabId::Services => "9",
            TabId::Net => "0",
            TabId::Timeline => "-",
            TabId::Insights => "+",
        }
    }
    pub fn title(&self) -> &'static str {
        match self {
            TabId::Overview => "Overview",
            TabId::Cpu => "CPU",
            TabId::Memory => "Memory",
            TabId::Disks => "Disks",
            TabId::Fs => "FS",
            TabId::Procs => "Procs",
            TabId::Gpu => "GPU",
            TabId::Power => "Power",
            TabId::Services => "Services",
            TabId::Net => "Net",
            TabId::Timeline => "Timeline",
            TabId::Insights => "Insights",
        }
    }
    fn from_str_loose(s: &str) -> Option<TabId> {
        match s.to_ascii_lowercase().as_str() {
            "overview" | "1" => Some(TabId::Overview),
            "cpu" | "2" => Some(TabId::Cpu),
            "memory" | "mem" | "3" => Some(TabId::Memory),
            "disks" | "disk" | "4" => Some(TabId::Disks),
            "fs" | "filesystems" | "5" => Some(TabId::Fs),
            "procs" | "processes" | "6" => Some(TabId::Procs),
            "gpu" | "7" => Some(TabId::Gpu),
            "power" | "8" => Some(TabId::Power),
            "services" | "9" => Some(TabId::Services),
            "net" | "network" | "0" => Some(TabId::Net),
            "timeline" | "-" => Some(TabId::Timeline),
            "insights" | "+" => Some(TabId::Insights),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcSort {
    Cpu,
    Rss,
    Io,
    Start,
    Name,
}

impl ProcSort {
    pub fn label(&self) -> &'static str {
        match self {
            ProcSort::Cpu => "cpu",
            ProcSort::Rss => "rss",
            ProcSort::Io => "io",
            ProcSort::Start => "start",
            ProcSort::Name => "name",
        }
    }
    pub const ALL: [ProcSort; 5] = [ProcSort::Cpu, ProcSort::Rss, ProcSort::Io, ProcSort::Start, ProcSort::Name];
    fn next(self) -> ProcSort {
        let i = ProcSort::ALL.iter().position(|s| *s == self).unwrap_or(0);
        ProcSort::ALL[(i + 1) % ProcSort::ALL.len()]
    }
}

pub struct App {
    pub active: TabId,
    pub paused: bool,
    pub history: History,
    pub snap: Option<Snapshot>,
    pub proc_sort: ProcSort,
    pub proc_sel: usize,
}

impl App {
    fn new(start: TabId) -> Self {
        Self {
            active: start,
            paused: false,
            history: History::new(120),
            snap: None,
            proc_sort: ProcSort::Cpu,
            proc_sel: 0,
        }
    }

    fn handle_key(&mut self, k: KeyEvent) -> bool {
        if k.kind != KeyEventKind::Press {
            return false;
        }
        match (k.code, k.modifiers) {
            (KeyCode::Char('q'), _) => return true,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
            (KeyCode::Char('p'), _) => self.paused = !self.paused,
            (KeyCode::Char('1'), _) => self.active = TabId::Overview,
            (KeyCode::Char('2'), _) => self.active = TabId::Cpu,
            (KeyCode::Char('3'), _) => self.active = TabId::Memory,
            (KeyCode::Char('4'), _) => self.active = TabId::Disks,
            (KeyCode::Char('5'), _) => self.active = TabId::Fs,
            (KeyCode::Char('6'), _) => self.active = TabId::Procs,
            (KeyCode::Char('7'), _) => self.active = TabId::Gpu,
            (KeyCode::Char('8'), _) => self.active = TabId::Power,
            (KeyCode::Char('9'), _) => self.active = TabId::Services,
            (KeyCode::Char('0'), _) => self.active = TabId::Net,
            (KeyCode::Char('-'), _) => self.active = TabId::Timeline,
            (KeyCode::Char('+') | KeyCode::Char('='), _) => self.active = TabId::Insights,
            (KeyCode::Tab, _) => self.active = next_tab(self.active),
            (KeyCode::BackTab, _) => self.active = prev_tab(self.active),
            (KeyCode::Up, _) if self.active == TabId::Procs => {
                self.proc_sel = self.proc_sel.saturating_sub(1);
            }
            (KeyCode::Down, _) if self.active == TabId::Procs => {
                let max = self
                    .snap
                    .as_ref()
                    .map(|s| s.procs.len().saturating_sub(1))
                    .unwrap_or(0);
                self.proc_sel = (self.proc_sel + 1).min(max);
            }
            (KeyCode::Char('s'), _) if self.active == TabId::Procs => {
                self.proc_sort = self.proc_sort.next();
                self.proc_sel = 0;
            }
            _ => {}
        }
        false
    }
}

fn next_tab(t: TabId) -> TabId {
    let i = ALL_TABS.iter().position(|x| *x == t).unwrap_or(0);
    ALL_TABS[(i + 1) % ALL_TABS.len()]
}

fn prev_tab(t: TabId) -> TabId {
    let i = ALL_TABS.iter().position(|x| *x == t).unwrap_or(0);
    ALL_TABS[(i + ALL_TABS.len() - 1) % ALL_TABS.len()]
}

pub fn run(opts: Options) -> Result<()> {
    let start = opts
        .start_tab
        .as_deref()
        .and_then(TabId::from_str_loose)
        .unwrap_or(TabId::Overview);
    let mut app = App::new(start);
    let mut collector = Collector::new();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let tick = Duration::from_millis(opts.tick_ms.max(100));
    let mut last_tick = Instant::now() - tick; // force immediate sample
    let res = loop {
        if last_tick.elapsed() >= tick {
            if !app.paused {
                let s = collector.sample();
                app.history.push(&s);
                let mut s = s;
                s.live = !app.paused;
                app.snap = Some(s);
            } else if let Some(snap) = app.snap.as_mut() {
                snap.live = false;
            }
            last_tick = Instant::now();
        }

        if let Some(snap) = &app.snap {
            term.draw(|f| draw(f, &app, snap))?;
        }

        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout.max(Duration::from_millis(33)))? {
            match event::read()? {
                Event::Key(k) => {
                    if app.handle_key(k) {
                        break Ok::<(), anyhow::Error>(());
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    };

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    res?;
    Ok(())
}

fn draw(f: &mut ratatui::Frame, app: &App, snap: &Snapshot) {
    let area = f.area();
    if area.width < 20 || area.height < 6 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header
            Constraint::Length(2),  // tab bar (label + underline)
            Constraint::Min(0),     // body
            Constraint::Length(2),  // footer (separator + hotkeys)
        ])
        .split(area);

    chrome::draw_header(f, chunks[0], snap);
    chrome::draw_tab_bar(f, chunks[1], app.active);
    let body = Rect {
        x: chunks[2].x,
        y: chunks[2].y,
        width: chunks[2].width,
        height: chunks[2].height,
    };
    tabs::draw(f, body, app, snap);
    chrome::draw_footer(f, chunks[3]);
}
