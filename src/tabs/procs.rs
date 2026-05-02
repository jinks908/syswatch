use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, ProcSort, Snapshot};
use crate::collect::ProcTick;
use crate::ui::{
    palette as p,
    widgets::{human_bytes, human_rate, panel},
};

pub fn draw(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // sort strip
            Constraint::Min(0),    // process table
            Constraint::Length(7), // drill-in
        ])
        .split(area);

    draw_sort_strip(f, v[0], app, snap);
    let sorted = sort_procs(&snap.procs, app.proc_sort);
    draw_table(f, v[1], app, &sorted);
    draw_drill_in(f, v[2], &sorted, app.proc_sel);
}

fn draw_sort_strip(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" sort ", Style::default().fg(p::DIM)));
    for s in ProcSort::ALL.iter() {
        let active = *s == app.proc_sort;
        let label = format!(" {} ", s.label());
        if active {
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(p::CYAN)
                    .bg(p::SEL_BG)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled("\u{25BC} ", Style::default().fg(p::CYAN)));
        } else {
            spans.push(Span::styled(label, Style::default().fg(p::FG)));
            spans.push(Span::raw(" "));
        }
    }
    spans.push(Span::styled(
        format!(
            "    {} procs   press s to cycle sort, ↑/↓ select",
            snap.procs.len()
        ),
        Style::default().fg(p::DIM),
    ));
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(p::BG)),
        area,
    );
}

fn draw_table(f: &mut Frame, area: Rect, app: &App, procs: &[ProcTick]) {
    let block = panel("PROCESSES");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let header = Line::from(vec![
        Span::styled(format!("{:>7} ", "PID"), header_style()),
        Span::styled(format!("{:>7} ", "PPID"), header_style()),
        Span::styled(format!("{:<14} ", "USER"), header_style()),
        Span::styled(format!("{:>6} ", "%CPU"), header_style()),
        Span::styled(format!("{:>9} ", "RSS"), header_style()),
        Span::styled(format!("{:>9} ", "VIRT"), header_style()),
        Span::styled(format!("{:<5} ", "STATE"), header_style()),
        Span::styled(format!("{:>11} ", "IO/s"), header_style()),
        Span::styled("COMMAND", header_style()),
    ]);

    let take = inner.height.saturating_sub(1) as usize;
    let sel_clamped = app.proc_sel.min(procs.len().saturating_sub(1));
    // Scroll: keep selection visible.
    let start = sel_clamped.saturating_sub(take.saturating_sub(1));
    let end = (start + take).min(procs.len());

    let mut lines = vec![header];
    for (i, proc_) in procs[start..end].iter().enumerate() {
        let abs = start + i;
        let selected = abs == sel_clamped;
        let row_bg = if selected { p::SEL_BG } else { p::BG };
        let dot_color = if proc_.cpu_pct >= 30.0 {
            p::YELLOW
        } else if matches!(proc_.state, 'R') {
            p::GREEN
        } else if matches!(proc_.state, 'Z') {
            p::RED
        } else {
            p::FAINT
        };
        let cpu_color = if proc_.cpu_pct >= 30.0 {
            p::YELLOW
        } else {
            p::FG
        };
        let state_color = match proc_.state {
            'R' => p::GREEN,
            'S' | 'I' => p::FG,
            'Z' => p::RED,
            _ => p::DIM,
        };
        let spans = vec![
            Span::styled(
                format!("{:>7} ", proc_.pid),
                Style::default().fg(p::FG).bg(row_bg),
            ),
            Span::styled(
                format!("{:>7} ", proc_.ppid),
                Style::default().fg(p::DIM).bg(row_bg),
            ),
            Span::styled(
                format!("{:<14.14} ", proc_.user),
                Style::default().fg(p::DIM).bg(row_bg),
            ),
            Span::styled(
                format!("{:>5.1} ", proc_.cpu_pct),
                Style::default().fg(cpu_color).bg(row_bg),
            ),
            Span::styled(
                format!("{:>9} ", human_bytes(proc_.mem_rss)),
                Style::default().fg(p::FG).bg(row_bg),
            ),
            Span::styled(
                format!("{:>9} ", human_bytes(proc_.mem_virt)),
                Style::default().fg(p::DIM).bg(row_bg),
            ),
            Span::styled(
                format!(" {:<4} ", proc_.state),
                Style::default()
                    .fg(state_color)
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>11} ", human_rate(proc_.io_rate)),
                Style::default().fg(if proc_.io_rate > 0.0 { p::CYAN } else { p::DIM }).bg(row_bg),
            ),
            Span::styled(
                proc_.name.clone(),
                Style::default().fg(p::FG).bg(row_bg),
            ),
            // Trailing fill to extend the SEL_BG band across the row.
            Span::styled(
                fill(inner.width as usize, &proc_.name),
                Style::default().bg(row_bg),
            ),
            // Status dot at the very start? No — append a leading dot replaces alignment. Skip.
            Span::raw(""),
        ];
        let _ = dot_color;
        lines.push(Line::from(spans));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::BG)),
        inner,
    );
}

fn draw_drill_in(f: &mut Frame, area: Rect, procs: &[ProcTick], sel: usize) {
    let Some(p_) = procs.get(sel.min(procs.len().saturating_sub(1))) else {
        let block = panel("DRILL-IN");
        f.render_widget(block, area);
        return;
    };
    let block = panel(&format!("{}  pid {}  -  drill-in", p_.name, p_.pid));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cmd = if p_.cmd.is_empty() {
        p_.name.clone()
    } else {
        p_.cmd.clone()
    };
    let started = p_
        .start_time
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            chrono::DateTime::<chrono::Local>::from(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(d.as_secs()),
            )
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
        })
        .unwrap_or_else(|| "?".into());

    let lines = vec![
        kv("cmd", cmd, p::FG),
        kv("ppid", p_.ppid.to_string(), p::FG),
        kv("user", p_.user.clone(), p::FG),
        kv("rss / virt", format!("{} / {}", human_bytes(p_.mem_rss), human_bytes(p_.mem_virt)), p::FG),
        kv("cpu", format!("{:.1}%", p_.cpu_pct), p::FG),
        kv("io rate", human_rate(p_.io_rate), if p_.io_rate > 0.0 { p::CYAN } else { p::DIM }),
        kv("started", started, p::DIM),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::BG)),
        inner,
    );
}

fn kv(k: &str, v: String, val_color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<11} ", k), Style::default().fg(p::DIM)),
        Span::styled(v, Style::default().fg(val_color)),
    ])
}

fn sort_procs(procs: &[ProcTick], key: ProcSort) -> Vec<ProcTick> {
    let mut out = procs.to_vec();
    match key {
        ProcSort::Cpu => out.sort_by(|a, b| {
            b.cpu_pct
                .partial_cmp(&a.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        ProcSort::Rss => out.sort_by(|a, b| b.mem_rss.cmp(&a.mem_rss)),
        ProcSort::Io => out.sort_by(|a, b| {
            b.io_rate
                .partial_cmp(&a.io_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        ProcSort::Start => out.sort_by(|a, b| b.start_time.cmp(&a.start_time)),
        ProcSort::Name => out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
    }
    out
}

fn fill(width: usize, used: &str) -> String {
    // 7+1 + 7+1 + 14+1 + 5+1 + 9+1 + 9+1 + 5+1 + 11+1 = 73
    let used_w = 73 + used.chars().count();
    if width > used_w {
        std::iter::repeat(' ').take(width - used_w).collect()
    } else {
        String::new()
    }
}

fn header_style() -> Style {
    Style::default().fg(p::DIM).add_modifier(Modifier::BOLD)
}
