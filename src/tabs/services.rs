use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, ServiceSort, Snapshot};
use crate::collect::{ServiceStatus, ServiceTick};
use crate::ui::{palette as p, widgets::panel};

pub fn draw(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // sort strip
            Constraint::Min(0),    // table
            Constraint::Length(7), // detail
        ])
        .split(area);

    draw_sort_strip(f, v[0], app, snap);
    let sorted = sort_services(&snap.services, app.service_sort);
    draw_table(f, v[1], app, &sorted);
    draw_detail(f, v[2], &sorted, app.service_sel);
}

fn draw_sort_strip(f: &mut Frame, area: Rect, app: &App, snap: &Snapshot) {
    let (running, idle, failed, unknown) = counts(&snap.services);
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" sort ", Style::default().fg(p::DIM)));
    for s in ServiceSort::ALL.iter() {
        let active = *s == app.service_sort;
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
    spans.push(Span::raw("    "));
    spans.push(Span::styled(
        format!("{} total  ", snap.services.len()),
        Style::default().fg(p::DIM),
    ));
    spans.push(Span::styled(
        format!("{} running  ", running),
        Style::default().fg(p::GREEN).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        format!("{} idle  ", idle),
        Style::default().fg(p::DIM),
    ));
    spans.push(Span::styled(
        format!("{} failed  ", failed),
        Style::default()
            .fg(if failed > 0 { p::RED } else { p::DIM })
            .add_modifier(Modifier::BOLD),
    ));
    if unknown > 0 {
        spans.push(Span::styled(
            format!("{} unknown", unknown),
            Style::default().fg(p::DIM),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(p::BG)),
        area,
    );
}

fn draw_table(f: &mut Frame, area: Rect, app: &App, services: &[ServiceTick]) {
    let block = panel("SERVICES");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if services.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "No services reported (collector not yet sampled or platform unsupported).",
                Style::default().fg(p::DIM),
            )]))
            .style(Style::default().bg(p::BG)),
            inner,
        );
        return;
    }

    let header = Line::from(vec![
        Span::styled(format!("{:<8} ", "STATUS"), header_style()),
        Span::styled(format!("{:>7} ", "PID"), header_style()),
        Span::styled(format!("{:>5} ", "EXIT"), header_style()),
        Span::styled("NAME", header_style()),
    ]);

    let take = inner.height.saturating_sub(1) as usize;
    let sel_clamped = app.service_sel.min(services.len().saturating_sub(1));
    let start = sel_clamped.saturating_sub(take.saturating_sub(1));
    let end = (start + take).min(services.len());

    let mut lines = vec![header];
    for (i, svc) in services[start..end].iter().enumerate() {
        let abs = start + i;
        let selected = abs == sel_clamped;
        let row_bg = if selected { p::SEL_BG } else { p::BG };
        let (status_color, status_label) = status_style(svc.status);
        let pid_text = svc.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into());
        let exit_text = svc
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".into());
        let exit_color = match svc.exit_code {
            Some(c) if c < 0 => p::YELLOW, // killed by signal — common on macOS
            Some(c) if c > 0 => p::RED,
            _ => p::DIM,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:<7} ", status_label),
                Style::default()
                    .fg(status_color)
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>7} ", pid_text),
                Style::default().fg(p::FG).bg(row_bg),
            ),
            Span::styled(
                format!("{:>5} ", exit_text),
                Style::default().fg(exit_color).bg(row_bg),
            ),
            Span::styled(svc.name.clone(), Style::default().fg(p::FG).bg(row_bg)),
            Span::styled(
                fill_remainder(inner.width as usize, &svc.name),
                Style::default().bg(row_bg),
            ),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::BG)),
        inner,
    );
}

fn draw_detail(f: &mut Frame, area: Rect, services: &[ServiceTick], sel: usize) {
    let Some(svc) = services.get(sel.min(services.len().saturating_sub(1))) else {
        let block = panel("DETAIL");
        f.render_widget(block, area);
        return;
    };
    let block = panel(format!("{}  -  detail", svc.name));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (status_color, status_label) = status_style(svc.status);
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{:<10} ", "status"), Style::default().fg(p::DIM)),
            Span::styled(
                status_label,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        kv(
            "pid",
            svc.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into()),
            p::FG,
        ),
        kv(
            "exit code",
            svc.exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "—".into()),
            p::FG,
        ),
        kv("notes", svc.detail.clone(), p::DIM),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(p::BG)),
        inner,
    );
}

fn status_style(s: ServiceStatus) -> (ratatui::style::Color, &'static str) {
    match s {
        ServiceStatus::Running => (p::GREEN, "RUN"),
        ServiceStatus::Idle => (p::DIM, "IDLE"),
        ServiceStatus::Failed => (p::RED, "FAIL"),
        ServiceStatus::Unknown => (p::FAINT, "?"),
    }
}

fn counts(services: &[ServiceTick]) -> (usize, usize, usize, usize) {
    let mut r = 0;
    let mut i = 0;
    let mut f = 0;
    let mut u = 0;
    for s in services {
        match s.status {
            ServiceStatus::Running => r += 1,
            ServiceStatus::Idle => i += 1,
            ServiceStatus::Failed => f += 1,
            ServiceStatus::Unknown => u += 1,
        }
    }
    (r, i, f, u)
}

fn sort_services(services: &[ServiceTick], key: ServiceSort) -> Vec<ServiceTick> {
    let mut out = services.to_vec();
    match key {
        ServiceSort::Name => out.sort_by(|a, b| a.name.cmp(&b.name)),
        ServiceSort::Status => out.sort_by(|a, b| {
            // Failed first, then Running, then Idle, then Unknown.
            let rank = |s: &ServiceTick| match s.status {
                ServiceStatus::Failed => 0,
                ServiceStatus::Running => 1,
                ServiceStatus::Idle => 2,
                ServiceStatus::Unknown => 3,
            };
            rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
        }),
        ServiceSort::Pid => out.sort_by(|a, b| match (a.pid, b.pid) {
            (Some(pa), Some(pb)) => pa.cmp(&pb),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        }),
    }
    out
}

fn kv(k: &str, v: String, val_color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<10} ", k), Style::default().fg(p::DIM)),
        Span::styled(v, Style::default().fg(val_color)),
    ])
}

fn fill_remainder(width: usize, used: &str) -> String {
    // 1 + 7 + 1 + 7 + 1 + 5 + 1 = 23 chars before the name column
    let used_w = 23 + used.chars().count();
    if width > used_w {
        " ".repeat(width - used_w)
    } else {
        String::new()
    }
}

fn header_style() -> Style {
    Style::default().fg(p::DIM).add_modifier(Modifier::BOLD)
}
