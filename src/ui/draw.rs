use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::Frame;

use crate::collect::Snapshot;
use crate::model::{AggMode, MemKind};
use crate::ui::app::App;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let theme = &app.theme;
    let area = frame.area();
    let fg = |rgb| Style::default().fg(theme.color(rgb));

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(theme.div_line))
        .title(Span::styled(" pspp ", fg(theme.title)));
    if let Some(bg) = theme.main_bg {
        block = block.style(Style::default().bg(theme.color(bg)));
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let _ = app.last_tick_at;
    let _ = app.last_mouse_position;
    let _ = app.tree.root_children().len();

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);

    let label = fg(theme.main_fg);
    let total_pct = usage_percent(
        total_used(&app.snapshot, app.mem_kind),
        app.snapshot.total_mem,
    );
    let summary = Line::from(vec![
        Span::styled(format!("processes: {}  ", app.snapshot.procs.len()), label),
        Span::styled("total: ", label),
        Span::styled(
            format_gib(app.snapshot.total_mem),
            Style::default().fg(theme.usage_color(total_pct)),
        ),
        Span::styled(
            format!(
                "  mem: {}  agg: {}  tick: {}ms",
                mem_kind_label(app.mem_kind),
                agg_mode_label(app.agg_mode),
                app.update_ms
            ),
            label,
        ),
    ]);

    frame.render_widget(Paragraph::new(summary), rows[0]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("q", fg(theme.hi_fg)),
            Span::styled(" quit — more coming", fg(theme.inactive_fg)),
        ])),
        rows[2],
    );
}

fn total_used(snapshot: &Snapshot, kind: MemKind) -> u64 {
    snapshot
        .procs
        .iter()
        .filter_map(|proc| kind.get(&proc.mem))
        .fold(0, u64::saturating_add)
}

fn usage_percent(used: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    (u128::from(used) * 100 / u128::from(total)).min(100) as u8
}

fn mem_kind_label(kind: MemKind) -> &'static str {
    match kind {
        MemKind::Rss => "rss",
        MemKind::Vsz => "vsz",
        MemKind::Footprint => "footprint",
        MemKind::Pss => "pss",
        MemKind::Uss => "uss",
        MemKind::Swap => "swap",
    }
}

fn agg_mode_label(mode: AggMode) -> &'static str {
    match mode {
        AggMode::Rollup => "rollup",
        AggMode::Flat => "flat",
    }
}

fn format_gib(bytes: u64) -> String {
    let gib = bytes as f64 / 1024_f64.powi(3);
    format!("{gib:.1} GiB")
}

#[cfg(test)]
mod tests {
    use super::usage_percent;

    #[test]
    fn usage_percent_clamps_and_handles_zero_total() {
        assert_eq!(usage_percent(5, 0), 0);
        assert_eq!(usage_percent(50, 200), 25);
        assert_eq!(usage_percent(500, 200), 100);
    }
}
