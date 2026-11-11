use ratatui::layout::{Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::Frame;

use crate::model::{AggMode, MemKind};
use crate::ui::app::App;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(" pspp ");
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

    let summary = format!(
        "processes: {}  total: {}  mem: {}  agg: {}  tick: {}ms",
        app.snapshot.procs.len(),
        format_gib(app.snapshot.total_mem),
        mem_kind_label(app.mem_kind),
        agg_mode_label(app.agg_mode),
        app.update_ms
    );

    frame.render_widget(Paragraph::new(Line::from(summary)), rows[0]);
    frame.render_widget(Paragraph::new(Line::from("q quit — more coming")), rows[2]);
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
