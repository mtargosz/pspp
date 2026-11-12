use std::ops::Range;

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::model::SortBy;
use crate::query::{format_pct, format_size};
use crate::ui::app::{App, AppMode, DisplayRow};
use crate::ui::graph;
use crate::ui::help;

const PCT_COL_WIDTH: u16 = 6;
const COMM_MIN_WIDTH: u16 = 12;
const OWN_COL_WIDTH: u16 = 11;
const CHILDREN_COL_WIDTH: u16 = 8;
const METER_MIN_WIDTH: u16 = 12;
const METER_MAX_WIDTH: u16 = 30;
const MEM_BOX_HEIGHT: u16 = 6;
const MEM_BOX_MIN_INNER_HEIGHT: u16 = 12;

pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    draw_main(frame, app);
    draw_overlay(frame, app);
}

fn draw_main(frame: &mut Frame<'_>, app: &mut App) {
    let theme = app.theme.clone();
    let area = frame.area();
    let fg = |rgb| Style::default().fg(theme.color(rgb));

    let mut block = Block::bordered()
        .border_type(app.border_type())
        .border_style(fg(theme.div_line))
        .title(Span::styled(" pspp ", fg(theme.title)));
    if let Some(clock) = app.clock_text() {
        block =
            block.title(Line::from(Span::styled(format!(" {clock} "), fg(theme.title))).centered());
    }
    if let Some(bg) = theme.main_bg {
        block = block.style(Style::default().bg(theme.color(bg)));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mem_height = if inner.height >= MEM_BOX_MIN_INNER_HEIGHT && inner.width >= 4 {
        MEM_BOX_HEIGHT
    } else {
        0
    };
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(mem_height),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);

    let status_area = layout[0];
    let mem_area = layout[1];
    let header_area = layout[2];
    let body_area = layout[3];
    let hint_area = layout[4];

    if mem_height > 0 {
        render_mem_box(frame, mem_area, app);
    }

    let status = status_line(app, usize::from(status_area.width));
    frame.render_widget(Paragraph::new(status), status_area);

    if body_area.width == 0 {
        app.clear_table_hit_test();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("q", fg(theme.hi_fg)),
                Span::styled(" quit", fg(theme.inactive_fg)),
            ])),
            hint_area,
        );
        return;
    }

    let agg_header = {
        let approx = app.rows().iter().any(|row| row.aggregated_approximate);
        let base = if approx {
            format!("{} Σ~", app.mem_kind.label().to_ascii_uppercase())
        } else {
            format!("{} Σ", app.mem_kind.label().to_ascii_uppercase())
        };
        if app.sort_by() == SortBy::Aggregated {
            format!("{base}{}", sort_dir_label(app.sort_desc()))
        } else {
            base
        }
    };

    let col_layout = build_column_layout(
        header_area,
        &agg_header,
        pid_column_width(app.tree.max_pid()),
    );
    let data_col_rects = data_column_rects(&col_layout);

    render_header(frame, &data_col_rects, &agg_header, app);

    let body_height = usize::from(body_area.height);
    app.set_viewport_height(body_height);

    if body_height == 0 {
        app.clear_table_hit_test();
        render_hint(frame, hint_area, app);
        return;
    }

    let (render_start, toggle_zones) = render_body(frame, body_area, &data_col_rects, app);
    app.set_table_hit_test(body_area, data_col_rects[0].x, render_start, toggle_zones);
    app.set_header_hit_test(vec![
        (data_col_rects[0], SortBy::Comm),
        (data_col_rects[1], SortBy::Pid),
        (data_col_rects[2], SortBy::Own),
        (data_col_rects[3], SortBy::Aggregated),
        (data_col_rects[4], SortBy::Aggregated),
    ]);
    render_hint(frame, hint_area, app);
}

fn draw_overlay(frame: &mut Frame<'_>, app: &mut App) {
    app.clear_overlay_hit_tests();
    match app.mode {
        AppMode::Normal | AppMode::FilterInput => {}
        AppMode::Menu => render_menu_overlay(frame, app),
        AppMode::Options => render_options_overlay(frame, app),
        AppMode::Help => render_help_overlay(frame, app),
    }
}

fn render_mem_box(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::bordered()
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme.color(theme.div_line)))
        .title(Span::styled(
            " mem ",
            Style::default().fg(theme.color(theme.title)),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let width = usize::from(inner.width);
    let used = app.mem_used();
    let max = app.mem_graph_max();
    let graph_height = inner.height.saturating_sub(1);

    if graph_height > 0 {
        let graph_area = Rect {
            height: graph_height,
            ..inner
        };
        let lines = graph::render(
            &app.mem_history,
            width,
            usize::from(graph_height),
            max,
            app.resolved_graph_symbols(),
            theme,
        );
        frame.render_widget(Paragraph::new(lines), graph_area);
    }

    // `used 12.1 GiB / 32.0 GiB ■■■■ 38%  Σ RSS 18.4 GiB`
    let pct = percentage(used, max);
    let label = "used ".to_owned();
    let used_text = format_size(used, app.base10);
    let max_text = format!(" / {}", format_size(max, app.base10));
    let pct_text = format!(" {pct}%");
    let sum_text = format!(
        "  Σ {} {}",
        app.mem_kind.label().to_ascii_uppercase(),
        format_size(app.mem_kind_sum(), app.base10)
    );
    let base_fixed = UnicodeWidthStr::width(label.as_str())
        + UnicodeWidthStr::width(used_text.as_str())
        + UnicodeWidthStr::width(max_text.as_str())
        + UnicodeWidthStr::width(pct_text.as_str())
        + UnicodeWidthStr::width(sum_text.as_str());
    let meter_width = width.saturating_sub(base_fixed + 1);

    let mut spans = vec![
        Span::styled(label, Style::default().fg(theme.color(theme.inactive_fg))),
        Span::styled(used_text, Style::default().fg(theme.usage_color(pct))),
        Span::styled(max_text, Style::default().fg(theme.color(theme.main_fg))),
    ];
    if meter_width > 0 {
        spans.push(Span::styled(
            " ",
            Style::default().fg(theme.color(theme.main_fg)),
        ));
        spans.extend(graph::meter_spans_styled(
            theme,
            meter_width,
            used,
            max,
            false,
            app.ascii_meters(),
        ));
    }
    spans.push(Span::styled(
        pct_text,
        Style::default().fg(theme.usage_color(pct)),
    ));
    spans.push(Span::styled(
        sum_text,
        Style::default().fg(theme.color(theme.inactive_fg)),
    ));

    let summary_area = Rect {
        y: inner.y + inner.height - 1,
        height: 1,
        ..inner
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), summary_area);
}

fn render_header(frame: &mut Frame<'_>, columns: &[Rect; 7], agg_header: &str, app: &App) {
    let theme = &app.theme;
    let base_style = Style::default().fg(theme.color(theme.title));
    let active_style = Style::default().fg(theme.color(theme.hi_fg));
    let arrow = sort_dir_label(app.sort_desc());

    let sortable = |label: &str, key: SortBy| -> (String, Style) {
        if app.sort_by() == key {
            (format!("{label}{arrow}"), active_style)
        } else {
            (label.to_owned(), base_style)
        }
    };

    let (comm_label, comm_style) = sortable("COMM", SortBy::Comm);
    let (pid_label, pid_style) = sortable("PID", SortBy::Pid);
    let (own_label, own_style) = sortable("OWN", SortBy::Own);
    let agg_style = if app.sort_by() == SortBy::Aggregated {
        active_style
    } else {
        base_style
    };

    frame.render_widget(Paragraph::new(comm_label).style(comm_style), columns[0]);
    frame.render_widget(
        Paragraph::new(pid_label)
            .style(pid_style)
            .alignment(Alignment::Right),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(own_label)
            .style(own_style)
            .alignment(Alignment::Right),
        columns[2],
    );
    frame.render_widget(
        Paragraph::new(agg_header)
            .style(agg_style)
            .alignment(Alignment::Right),
        columns[3],
    );
    frame.render_widget(
        Paragraph::new("%")
            .style(base_style)
            .alignment(Alignment::Right),
        columns[4],
    );
    frame.render_widget(
        Paragraph::new("CHILDREN")
            .style(base_style)
            .alignment(Alignment::Right),
        columns[5],
    );
    frame.render_widget(Paragraph::new("METER").style(base_style), columns[6]);
}

fn render_body(
    frame: &mut Frame<'_>,
    body_area: Rect,
    columns: &[Rect; 7],
    app: &App,
) -> (usize, Vec<Option<Range<u16>>>) {
    let rows = app.rows();
    if rows.is_empty() {
        if body_area.height > 0 && body_area.width > 0 {
            let msg = if app.is_filter_active() {
                if app.mode == AppMode::FilterInput {
                    "no matching processes (Esc to cancel, Del/Ctrl-U to clear)"
                } else {
                    "no matching processes (press c to clear)"
                }
            } else {
                "no processes"
            };
            let notice = truncate_display_width(msg, usize::from(body_area.width));
            let notice_rect = Rect {
                x: body_area.x,
                y: body_area.y + body_area.height / 2,
                width: body_area.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(notice)
                    .style(Style::default().fg(app.theme.color(app.theme.inactive_fg)))
                    .alignment(Alignment::Center),
                notice_rect,
            );
        }
        return (0, vec![None; usize::from(body_area.height)]);
    }

    let start = app.scroll();
    let end = (start + usize::from(body_area.height)).min(rows.len());
    let max_visible_agg = rows.iter().map(|row| row.aggregated).max().unwrap_or(0);

    let mut toggle_zones = Vec::with_capacity(usize::from(body_area.height));

    for (line_idx, row) in rows
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .enumerate()
    {
        let row_idx = start + line_idx;
        let row_rect = Rect {
            x: body_area.x,
            y: body_area.y + u16::try_from(line_idx).unwrap_or(u16::MAX),
            width: body_area.width,
            height: 1,
        };

        let selected = row_idx == app.cursor();
        if selected {
            let mut row_style = Style::default()
                .bg(app.theme.color(app.theme.selected_bg))
                .fg(app.theme.color(app.theme.selected_fg));
            if app.no_color {
                row_style = row_style.add_modifier(Modifier::REVERSED);
            }
            frame.render_widget(Block::default().style(row_style), row_rect);
        }

        let row_columns = columns.map(|column| Rect {
            y: row_rect.y,
            ..column
        });
        render_row_cells(frame, row, &row_columns, app, max_visible_agg, selected);

        let comm_width = columns[0].width;
        let zone = row.toggle_marker.as_ref().and_then(|marker| {
            if comm_width == 0 {
                return None;
            }
            let start_col = columns[0]
                .x
                .saturating_add(u16::try_from(marker.start).unwrap_or(u16::MAX));
            let end_col = columns[0]
                .x
                .saturating_add(u16::try_from(marker.end).unwrap_or(u16::MAX));
            let comm_end = columns[0].x.saturating_add(comm_width);
            if start_col >= comm_end {
                None
            } else {
                Some(start_col..end_col.min(comm_end))
            }
        });
        toggle_zones.push(zone);
    }

    while toggle_zones.len() < usize::from(body_area.height) {
        toggle_zones.push(None);
    }

    (start, toggle_zones)
}

fn render_row_cells(
    frame: &mut Frame<'_>,
    row: &DisplayRow,
    columns: &[Rect; 7],
    app: &App,
    max_visible_agg: u64,
    selected: bool,
) {
    let theme = &app.theme;
    let mut selected_style = Style::default()
        .fg(theme.color(theme.selected_fg))
        .bg(theme.color(theme.selected_bg));
    if app.no_color {
        selected_style = selected_style.add_modifier(Modifier::REVERSED);
    }
    let pid_text = row.pid.to_string();
    let own_text = with_approx_suffix(format_size(row.own, app.base10), row.own_approximate);
    let agg_text = with_approx_suffix(
        format_size(row.aggregated, app.base10),
        row.aggregated_approximate,
    );
    let pct_text = format_pct(row.aggregated, app.tree.total_mem());
    let children_text = row.child_count.to_string();
    let meter_width = usize::from(columns[6].width);

    let own_pct = percentage(row.own, app.tree.total_mem());
    let agg_pct = percentage(row.aggregated, app.tree.total_mem());

    let default_style = Style::default().fg(theme.color(theme.main_fg));
    let (prefix_style, comm_style) = if selected {
        (selected_style, selected_style)
    } else if app.is_direct_filter_match(row.pid) {
        (
            Style::default().fg(theme.color(theme.main_fg)),
            Style::default().fg(theme.color(theme.hi_fg)),
        )
    } else if app.is_context_filter_match(row.pid) {
        (
            Style::default().fg(theme.color(theme.inactive_fg)),
            Style::default().fg(theme.color(theme.inactive_fg)),
        )
    } else {
        (default_style, default_style)
    };
    let comm_spans = truncate_comm_spans(
        &row.tree_prefix,
        prefix_style,
        &row.comm,
        comm_style,
        usize::from(columns[0].width),
    );
    frame.render_widget(
        Paragraph::new(Line::from(comm_spans)).alignment(Alignment::Left),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(pid_text)
            .style(if selected {
                selected_style
            } else {
                default_style
            })
            .alignment(Alignment::Right),
        columns[1],
    );

    frame.render_widget(
        Paragraph::new(own_text)
            .style(if selected {
                selected_style
            } else {
                Style::default().fg(theme.usage_color(own_pct))
            })
            .alignment(Alignment::Right),
        columns[2],
    );

    frame.render_widget(
        Paragraph::new(agg_text)
            .style(if selected {
                selected_style
            } else {
                Style::default().fg(theme.usage_color(agg_pct))
            })
            .alignment(Alignment::Right),
        columns[3],
    );

    frame.render_widget(
        Paragraph::new(pct_text)
            .style(if selected {
                selected_style
            } else {
                Style::default().fg(theme.usage_color(agg_pct))
            })
            .alignment(Alignment::Right),
        columns[4],
    );

    frame.render_widget(
        Paragraph::new(children_text)
            .style(if selected {
                selected_style
            } else {
                default_style
            })
            .alignment(Alignment::Right),
        columns[5],
    );

    let meter_line = meter_line(meter_width, row.aggregated, max_visible_agg, app, selected);
    frame.render_widget(Paragraph::new(meter_line), columns[6]);
}

fn meter_line(width: usize, value: u64, scale: u64, app: &App, selected: bool) -> Line<'static> {
    let line = Line::from(graph::meter_spans_styled(
        &app.theme,
        width,
        value,
        scale,
        selected,
        app.ascii_meters(),
    ));
    if selected && app.no_color {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

struct StatusSegment {
    order: usize,
    width: usize,
    spans: Vec<Span<'static>>,
}

/// Priority-ordered status bar that drops lower-priority segments cleanly on narrow terminals
/// while keeping `filter: <input>█` visible (left-cropping `<input>` with `…` only when the
/// filter segment alone exceeds `width`).
fn status_line(app: &App, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let theme = &app.theme;
    let fg = |rgb| Style::default().fg(theme.color(rgb));
    let used_pct = percentage(app.mem_used(), app.tree.total_mem());

    let mut candidates = Vec::with_capacity(7);

    // Priority 1 (highest): active or in-progress filter indicator (`order = 6`, rightmost).
    if let Some(raw_filter) = app.filter_indicator() {
        let cropped = format_filter_segment(&raw_filter, app.mode == AppMode::FilterInput, width);
        let seg_w = UnicodeWidthStr::width(cropped.as_str());
        candidates.push(StatusSegment {
            order: 6,
            width: seg_w,
            spans: vec![Span::styled(cropped, fg(theme.hi_fg))],
        });
    }

    // Priority 2: process count (`order = 0`).
    let procs_text = format!("{} procs", app.tree.proc_count());
    candidates.push(StatusSegment {
        order: 0,
        width: UnicodeWidthStr::width(procs_text.as_str()),
        spans: vec![Span::styled(procs_text, fg(theme.main_fg))],
    });

    // Priority 3: system memory used / total (`order = 5`).
    let used_text = format!(
        "used {} / {}",
        format_size(app.mem_used(), app.base10),
        format_size(app.tree.total_mem(), app.base10)
    );
    candidates.push(StatusSegment {
        order: 5,
        width: UnicodeWidthStr::width(used_text.as_str()),
        spans: vec![Span::styled(
            used_text,
            Style::default().fg(theme.usage_color(used_pct)),
        )],
    });

    // Priority 4: memory metric (`order = 1`).
    let mem_label = app.mem_kind.label().to_ascii_uppercase();
    candidates.push(StatusSegment {
        order: 1,
        width: 4 + UnicodeWidthStr::width(mem_label.as_str()),
        spans: vec![
            Span::styled("mem ", fg(theme.inactive_fg)),
            Span::styled(mem_label, fg(theme.main_fg)),
        ],
    });

    // Priority 5: aggregation mode (`order = 2`).
    let agg_text = format!("agg {}", app.agg_mode);
    candidates.push(StatusSegment {
        order: 2,
        width: UnicodeWidthStr::width(agg_text.as_str()),
        spans: vec![Span::styled(agg_text, fg(theme.main_fg))],
    });

    // Priority 6: sort key & direction (`order = 3`).
    let sort_text = format!("sort {}{}", app.sort_by(), sort_dir_label(app.sort_desc()));
    candidates.push(StatusSegment {
        order: 3,
        width: UnicodeWidthStr::width(sort_text.as_str()),
        spans: vec![Span::styled(sort_text, fg(theme.main_fg))],
    });

    // Priority 7 (lowest, dropped first): refresh interval (`order = 4`).
    let update_text = format!("update {}ms", app.update_ms);
    candidates.push(StatusSegment {
        order: 4,
        width: UnicodeWidthStr::width(update_text.as_str()),
        spans: vec![Span::styled(update_text, fg(theme.main_fg))],
    });

    let mut kept: Vec<StatusSegment> = Vec::new();
    let mut used_width = 0_usize;
    for seg in candidates {
        if seg.width == 0 {
            continue;
        }
        let sep = if kept.is_empty() { 0 } else { 2 };
        if used_width.saturating_add(sep).saturating_add(seg.width) <= width {
            used_width = used_width.saturating_add(sep).saturating_add(seg.width);
            kept.push(seg);
        }
    }
    kept.sort_by_key(|seg| seg.order);

    let mut spans = Vec::new();
    for (idx, seg) in kept.into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled("  ", fg(theme.main_fg)));
        }
        spans.extend(seg.spans);
    }
    Line::from(spans)
}

fn format_filter_segment(text: &str, editing: bool, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if editing {
        let input = text
            .strip_prefix("filter: ")
            .and_then(|s| s.strip_suffix('█'))
            .unwrap_or(text);
        if max_width == 1 {
            return "█".to_owned();
        }
        if max_width < 10 {
            let tail = take_display_suffix(input, max_width - 2);
            return format!("…{tail}█");
        }
        let tail = take_display_suffix(input, max_width - 10);
        return format!("filter: …{tail}█");
    }
    truncate_display_width(text, max_width)
}

fn take_display_prefix(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut used = 0_usize;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used.saturating_add(w) > max_width {
            break;
        }
        used = used.saturating_add(w);
        out.push(ch);
    }
    out
}

fn take_display_suffix(text: &str, max_width: usize) -> String {
    let mut rev_chars = Vec::new();
    let mut used = 0_usize;
    for ch in text.chars().rev() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used.saturating_add(w) > max_width {
            break;
        }
        used = used.saturating_add(w);
        rev_chars.push(ch);
    }
    while rev_chars
        .last()
        .is_some_and(|ch| UnicodeWidthChar::width(*ch).unwrap_or(0) == 0)
    {
        rev_chars.pop();
    }
    rev_chars.into_iter().rev().collect()
}

/// Truncates `text` to at most `max_width` terminal display columns, appending `…` when cut and
/// never splitting a wide (2-column) character.
pub(crate) fn truncate_display_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width == 1 {
        return "…".to_owned();
    }
    let prefix = take_display_prefix(text, max_width - 1);
    format!("{prefix}…")
}

fn truncate_comm_spans(
    prefix: &str,
    prefix_style: Style,
    comm: &str,
    comm_style: Style,
    max_width: usize,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }
    let prefix_w = UnicodeWidthStr::width(prefix);
    let comm_w = UnicodeWidthStr::width(comm);
    if prefix_w.saturating_add(comm_w) <= max_width {
        return vec![
            Span::styled(prefix.to_owned(), prefix_style),
            Span::styled(comm.to_owned(), comm_style),
        ];
    }
    if prefix_w < max_width {
        let avail = max_width - prefix_w;
        return vec![
            Span::styled(prefix.to_owned(), prefix_style),
            Span::styled(truncate_display_width(comm, avail), comm_style),
        ];
    }
    let cut = take_display_prefix(prefix, max_width.saturating_sub(1));
    vec![Span::styled(format!("{cut}…"), prefix_style)]
}

/// Key hints for the current mode as `(keys, meaning)` segments, most important first.
fn hint_segments(mode: AppMode) -> &'static [(&'static str, &'static str)] {
    match mode {
        AppMode::Normal => &[
            ("↑/↓ j/k PgUp/PgDn", ""),
            ("←/→", ""),
            ("Space/Enter", ""),
            ("z/Z", "fold"),
            ("e", "agg"),
            ("m", "mem"),
            ("s", "sort"),
            ("S/r", "rev"),
            ("f//", "filter"),
            ("h/F1", "help"),
            ("o/F2", "options"),
            ("Esc", "menu"),
            ("q", "quit"),
        ],
        AppMode::FilterInput => &[
            ("type", "filter text"),
            ("Enter", "apply"),
            ("Esc", "cancel"),
            ("Backspace", "delete"),
        ],
        AppMode::Menu => &[("↑/↓ Tab", "select"), ("Enter", "open"), ("Esc/q", "close")],
        AppMode::Options => &[
            ("↑/↓", "select"),
            ("←/→", "change"),
            ("Enter", "edit"),
            ("click", "select/edit"),
            ("Esc/q", "back"),
        ],
        AppMode::Help => &[
            ("↑/↓ PgUp/PgDn", "scroll"),
            ("wheel", "scroll"),
            ("Esc/q", "back"),
        ],
    }
}

/// Renders as many whole hint segments as fit in `width` columns.
fn hint_line(mode: AppMode, width: usize, app: &App) -> Line<'static> {
    let theme = &app.theme;
    let key_style = Style::default().fg(theme.color(theme.main_fg));
    let text_style = Style::default().fg(theme.color(theme.inactive_fg));
    let quit_style = Style::default().fg(theme.color(theme.hi_fg));

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0_usize;
    for (keys, meaning) in hint_segments(mode) {
        let gap = if spans.is_empty() { 0 } else { 2 };
        let seg_len = UnicodeWidthStr::width(*keys)
            + if meaning.is_empty() {
                0
            } else {
                1 + UnicodeWidthStr::width(*meaning)
            };
        if used + gap + seg_len > width {
            break;
        }
        if gap > 0 {
            spans.push(Span::raw("  "));
        }
        let is_quit = mode == AppMode::Normal && *keys == "q";
        spans.push(Span::styled(
            (*keys).to_owned(),
            if is_quit { quit_style } else { key_style },
        ));
        if !meaning.is_empty() {
            spans.push(Span::styled(format!(" {meaning}"), text_style));
        }
        used += gap + seg_len;
    }
    Line::from(spans)
}

fn render_hint(frame: &mut Frame<'_>, hint_area: Rect, app: &App) {
    let line = hint_line(app.mode, usize::from(hint_area.width), app);
    frame.render_widget(Paragraph::new(line), hint_area);
}

fn menu_overlay_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).clamp(24, 44);
    let height = area.height.saturating_sub(4).clamp(13, 17);
    centered_rect(area, width, height)
}

fn options_overlay_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).clamp(40, 78);
    let height = area.height.saturating_sub(4).clamp(12, 22);
    centered_rect(area, width, height)
}

fn help_overlay_rect(area: Rect) -> Rect {
    let max_width = area.width.saturating_sub(2).max(24);
    let max_height = area.height.saturating_sub(2).max(8);
    let width = area.width.saturating_sub(6).clamp(24, max_width);
    let height = area.height.saturating_sub(4).clamp(8, max_height);
    centered_rect(area, width, height)
}

fn render_overlay_frame(
    frame: &mut Frame<'_>,
    overlay: Rect,
    title: &'static str,
    app: &App,
) -> Option<Rect> {
    if overlay.is_empty() {
        return None;
    }

    frame.render_widget(Clear, overlay);

    let theme = &app.theme;
    let mut block = Block::bordered()
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme.color(theme.div_line)))
        .title(Span::styled(
            title,
            Style::default().fg(theme.color(theme.title)),
        ));
    if let Some(bg) = theme.main_bg {
        block = block.style(Style::default().bg(theme.color(bg)));
    }
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    Some(inner)
}

fn render_menu_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let overlay = menu_overlay_rect(frame.area());
    let Some(inner) = render_overlay_frame(frame, overlay, " menu ", app) else {
        return;
    };

    let theme = &app.theme;
    let button_height = if inner.height >= 11 { 3 } else { 1 };
    let spacing = if button_height == 3 { 1 } else { 0 };

    let mut button_rects = [
        Rect::new(inner.x, inner.y, inner.width, button_height),
        Rect::new(inner.x, inner.y, inner.width, button_height),
        Rect::new(inner.x, inner.y, inner.width, button_height),
    ];

    let total_buttons_height = button_height * 3 + spacing * 2;
    let mut y = inner
        .y
        .saturating_add(inner.height.saturating_sub(total_buttons_height) / 2);

    for (idx, button) in App::menu_buttons().iter().enumerate() {
        let rect = Rect {
            x: inner.x.saturating_add(1),
            y,
            width: inner.width.saturating_sub(2),
            height: button_height,
        }
        .intersection(inner);
        button_rects[idx] = rect;
        y = y.saturating_add(button_height + spacing);
        if rect.is_empty() {
            continue;
        }

        let selected = *button == app.menu_selected();
        if rect.height >= 3 && rect.width >= 3 {
            let border_color = if selected { theme.hi_fg } else { theme.main_fg };
            let border_type = if selected {
                BorderType::Double
            } else {
                BorderType::Plain
            };

            let button_block = Block::default()
                .borders(Borders::ALL)
                .border_type(border_type)
                .border_style(Style::default().fg(theme.color(border_color)));
            let button_inner = button_block.inner(rect);
            frame.render_widget(button_block, rect);

            let mut label_style = Style::default().fg(theme.color(border_color));
            if selected && app.no_color {
                label_style = label_style.add_modifier(Modifier::REVERSED);
            }
            let label = Paragraph::new(button.label())
                .alignment(Alignment::Center)
                .style(label_style);
            frame.render_widget(label, button_inner);
        } else {
            let mut style = if selected {
                Style::default()
                    .fg(theme.color(theme.selected_fg))
                    .bg(theme.color(theme.selected_bg))
            } else {
                Style::default().fg(theme.color(theme.main_fg))
            };
            if selected && app.no_color {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let line_rect = Rect {
                x: rect.x,
                y: rect.y + rect.height / 2,
                width: rect.width,
                height: 1,
            };
            let label = Paragraph::new(button.label())
                .alignment(Alignment::Center)
                .style(style);
            frame.render_widget(label, line_rect);
        }
    }

    app.set_menu_hit_test(overlay, button_rects);
}

fn render_options_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let overlay = options_overlay_rect(frame.area());
    let Some(inner) = render_overlay_frame(frame, overlay, " options ", app) else {
        return;
    };

    if inner.width == 0 || inner.height == 0 {
        app.set_options_hit_test(overlay, Vec::new());
        return;
    }

    let theme = app.theme.clone();
    let sections = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
    let list_area = sections[0];
    let bottom = sections[1];

    let label_width = 20;
    let mut row_rects = Vec::new();
    let visible_rows = usize::from(list_area.height);
    let has_scrollbar = App::option_keys().len() > visible_rows && list_area.width > 0;
    let row_width = if has_scrollbar && list_area.width > 1 {
        list_area.width - 1
    } else {
        list_area.width
    };
    let first = app.option_scroll_for(visible_rows);
    for (line_idx, idx) in (first..App::option_keys().len())
        .take(visible_rows)
        .enumerate()
    {
        let key = App::option_keys()[idx];
        let row_rect = Rect {
            x: list_area.x,
            y: list_area.y + u16::try_from(line_idx).unwrap_or(u16::MAX),
            width: row_width,
            height: 1,
        };

        let selected = idx == app.option_selected_index();
        let line = format!(
            "{:<label_width$} {}",
            key.label(),
            app.option_value(key),
            label_width = label_width
        );

        let style = if selected {
            let mut selected_style = Style::default()
                .fg(theme.color(theme.selected_fg))
                .bg(theme.color(theme.selected_bg));
            if app.no_color {
                selected_style = selected_style.add_modifier(Modifier::REVERSED);
            }
            selected_style
        } else {
            Style::default().fg(theme.color(theme.main_fg))
        };

        frame.render_widget(Paragraph::new(line).style(style), row_rect);
        row_rects.push((row_rect, idx));
    }
    if has_scrollbar {
        let mut bar_state = ScrollbarState::new(
            App::option_keys()
                .len()
                .saturating_sub(visible_rows)
                .saturating_add(1),
        )
        .position(first);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .style(Style::default().fg(theme.color(theme.div_line)))
                .thumb_style(Style::default().fg(theme.color(theme.hi_fg))),
            list_area,
            &mut bar_state,
        );
    }
    app.set_options_hit_test(overlay, row_rects);

    if bottom.width > 0 && bottom.height > 0 {
        let desc_style = Style::default()
            .fg(theme.color(theme.main_fg))
            .bg(theme.color(theme.selected_bg));
        frame.render_widget(Block::default().style(desc_style), bottom);
        let desc_lines: Vec<Line<'static>> =
            help::wrap_text(app.selected_option_description(), usize::from(bottom.width))
                .into_iter()
                .take(usize::from(bottom.height))
                .map(Line::from)
                .collect();
        frame.render_widget(Paragraph::new(desc_lines).style(desc_style), bottom);
    }
}

fn render_help_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let overlay = help_overlay_rect(frame.area());
    let Some(inner) = render_overlay_frame(frame, overlay, " help ", app) else {
        app.set_help_scroll_metrics(0, 1);
        return;
    };

    app.set_help_hit_test(overlay);
    if inner.width == 0 || inner.height == 0 {
        app.set_help_scroll_metrics(0, 1);
        return;
    }

    let theme = app.theme.clone();
    // The last column is reserved for the scrollbar so text never runs underneath it.
    let columns = Layout::horizontal([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let body = columns[0];
    let bar_area = columns[1];
    let lines = help::build_help_lines(&theme, usize::from(body.width));
    app.set_help_scroll_metrics(lines.len(), usize::from(body.height));

    let scroll = u16::try_from(app.help_scroll()).unwrap_or(u16::MAX);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);

    if app.help_overflow() > 0 {
        let mut bar_state =
            ScrollbarState::new(app.help_overflow().saturating_add(1)).position(app.help_scroll());
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .style(Style::default().fg(theme.color(theme.div_line)))
                .thumb_style(Style::default().fg(theme.color(theme.hi_fg))),
            bar_area,
            &mut bar_state,
        );
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width) / 2);
    let y = area.y + (area.height.saturating_sub(height) / 2);
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// PID column width: digits of the largest pid present, at least 5.
fn pid_column_width(max_pid: i32) -> u16 {
    let digits = max_pid.max(0).to_string().len();
    u16::try_from(digits).unwrap_or(10).max(5)
}

/// Meter column width: grows with the terminal (`width / 6`, 12..=30 cells) before COMM takes
/// the remaining slack, but never squeezes COMM below its minimum.
fn meter_width(total_width: u16, fixed_width: u16) -> u16 {
    let desired = (total_width / 6).clamp(METER_MIN_WIDTH, METER_MAX_WIDTH);
    let room = total_width
        .saturating_sub(fixed_width)
        .saturating_sub(COMM_MIN_WIDTH);
    desired.min(room)
}

fn build_column_layout(area: Rect, agg_header: &str, pid_width: u16) -> Vec<Rect> {
    let agg_len = u16::try_from(UnicodeWidthStr::width(agg_header))
        .unwrap_or(11)
        .max(11);
    // Everything except COMM and the meter, including the six one-cell gaps between columns.
    let fixed = pid_width + OWN_COL_WIDTH + agg_len + PCT_COL_WIDTH + CHILDREN_COL_WIDTH + 6;
    Layout::horizontal([
        Constraint::Min(COMM_MIN_WIDTH),
        Constraint::Length(1),
        Constraint::Length(pid_width),
        Constraint::Length(1),
        Constraint::Length(OWN_COL_WIDTH),
        Constraint::Length(1),
        Constraint::Length(agg_len),
        Constraint::Length(1),
        Constraint::Length(PCT_COL_WIDTH),
        Constraint::Length(1),
        Constraint::Length(CHILDREN_COL_WIDTH),
        Constraint::Length(1),
        Constraint::Length(meter_width(area.width, fixed)),
    ])
    .split(area)
    .to_vec()
}

fn data_column_rects(columns: &[Rect]) -> [Rect; 7] {
    [
        columns[0],
        columns[2],
        columns[4],
        columns[6],
        columns[8],
        columns[10],
        columns[12],
    ]
}

fn sort_dir_label(desc: bool) -> &'static str {
    if desc {
        "↓"
    } else {
        "↑"
    }
}

fn with_approx_suffix(mut value: String, approximate: bool) -> String {
    if approximate {
        value.push('~');
    }
    value
}

fn percentage(value: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    (u128::from(value) * 100 / u128::from(total)).min(100) as u8
}

pub fn meter_fill_blocks(value: u64, scale: u64, width: usize) -> usize {
    if width == 0 || scale == 0 {
        return 0;
    }

    ((u128::from(value) * width as u128 + (u128::from(scale) / 2)) / u128::from(scale))
        .min(width as u128) as usize
}

#[cfg(test)]
mod tests {
    use super::{draw, hint_line, meter_fill_blocks};
    use crate::ui::app::{App, AppMode};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_string(width: u16, height: u16) -> String {
        let mut app = App::new(2_000);
        app.mem_history.push(app.mem_graph_max() / 2);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn mem_box_shows_when_tall_enough_and_hides_when_short() {
        let tall = render_to_string(100, 30);
        assert!(tall.contains("╭ mem "));
        assert!(tall.contains("used "), "{tall}");
        assert!(tall.contains(" / "));
        assert!(tall.contains("Σ RSS"));
        assert!(tall.contains('%'));
        assert!(!tall.contains(" total "));
        assert!(tall.contains("COMM"));
        assert!(tall.lines().filter(|line| line.contains("MiB")).count() > 3);

        let short = render_to_string(100, 10);
        assert!(!short.contains("╭ mem "));
        assert!(short.contains("COMM"));
    }

    #[test]
    fn clock_renders_in_the_top_border_only_when_enabled() {
        let mut app = tiny_app();
        app.clock_format = "%Y".to_owned();
        let year = chrono::Local::now().format("%Y").to_string();
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        let top: String = (0..60)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol().to_owned())
            .collect();
        assert!(top.contains(&year), "{top}");

        app.clock_format.clear();
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        let top: String = (0..60)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol().to_owned())
            .collect();
        assert!(!top.contains(&year), "{top}");
    }

    #[test]
    fn square_corners_replace_rounded_ones() {
        let mut app = tiny_app();
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "╭");
        app.rounded_corners = false;
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "┌");
    }

    #[test]
    fn options_overlay_scrolls_to_show_late_options() {
        let mut app = tiny_app();
        app.mode = AppMode::Options;
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        let first = screen_text(&terminal, 80, 16);
        assert!(
            first.contains("update_ms") && !first.contains("force_tty"),
            "{first}"
        );

        for _ in 0..16 {
            app.on_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        let last = screen_text(&terminal, 80, 16);
        assert!(
            last.contains("force_tty") && !last.contains("update_ms  "),
            "{last}"
        );
    }

    fn screen_text(terminal: &Terminal<TestBackend>, width: u16, height: u16) -> String {
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn meter_width_scales_with_the_terminal_and_spares_comm() {
        let fixed = 5 + 10 + 14 + 6 + 8 + 6;
        // Grows 12 -> 30 as the terminal widens, then stays at 30 while COMM takes the slack.
        assert_eq!(super::meter_width(80, fixed), 13);
        assert_eq!(super::meter_width(110, fixed), 18);
        assert_eq!(super::meter_width(150, fixed), 25);
        assert_eq!(super::meter_width(180, fixed), 30);
        assert_eq!(super::meter_width(400, fixed), 30);
        // Narrow terminals shrink the meter before COMM drops below 12 cells.
        assert_eq!(super::meter_width(66, fixed), 66 - fixed - 12);
        assert_eq!(super::meter_width(60, fixed), 0);
        assert_eq!(super::meter_width(0, fixed), 0);
    }

    #[test]
    fn pid_column_width_follows_the_largest_pid() {
        assert_eq!(super::pid_column_width(0), 5);
        assert_eq!(super::pid_column_width(1), 5);
        assert_eq!(super::pid_column_width(99_999), 5);
        assert_eq!(super::pid_column_width(100_000), 6);
        assert_eq!(super::pid_column_width(4_194_304), 7, "Linux pid_max");
        assert_eq!(super::pid_column_width(-3), 5);
    }

    #[test]
    fn large_pids_render_without_truncation() {
        use crate::collect::{MemSet, ProcRec, Snapshot};
        use crate::ui::app::UiSettings;
        let snapshot = Snapshot {
            procs: vec![
                ProcRec {
                    pid: 1,
                    ppid: 0,
                    comm: "init".into(),
                    mem: MemSet {
                        rss: Some(0),
                        vsz: Some(0),
                        ..MemSet::default()
                    },
                },
                ProcRec {
                    pid: 4_194_303,
                    ppid: 1,
                    comm: "worker".into(),
                    mem: MemSet {
                        rss: Some(0),
                        vsz: Some(0),
                        ..MemSet::default()
                    },
                },
            ],
            total_mem: 1000,
            used_mem: 10,
        };
        let mut app = App::with_snapshot(snapshot, UiSettings::default());
        let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        let screen = screen_text(&terminal, 90, 12);
        assert!(screen.contains("4194303"), "{screen}");
    }

    #[test]
    fn meter_fill_handles_zero_and_rounding() {
        assert_eq!(meter_fill_blocks(0, 100, 10), 0);
        assert_eq!(meter_fill_blocks(50, 100, 10), 5);
        assert_eq!(meter_fill_blocks(51, 100, 10), 5);
        assert_eq!(meter_fill_blocks(55, 100, 10), 6);
        assert_eq!(meter_fill_blocks(100, 100, 10), 10);
        assert_eq!(meter_fill_blocks(200, 100, 10), 10);
        assert_eq!(meter_fill_blocks(1, 100, 0), 0);
    }

    #[test]
    fn every_mode_renders_at_tiny_sizes_without_panicking() {
        let mut app = App::new(2_000);
        for mode in [
            AppMode::Normal,
            AppMode::FilterInput,
            AppMode::Menu,
            AppMode::Options,
            AppMode::Help,
        ] {
            app.mode = mode;
            for (width, height) in [(1, 1), (2, 2), (10, 3), (20, 5), (30, 12), (80, 24)] {
                let mut terminal =
                    Terminal::new(TestBackend::new(width, height)).expect("terminal");
                terminal
                    .draw(|frame| draw(frame, &mut app))
                    .unwrap_or_else(|err| panic!("{mode:?} at {width}x{height}: {err}"));
            }
        }
    }

    fn tiny_app() -> App {
        use crate::collect::{MemSet, ProcRec, Snapshot};
        use crate::ui::app::UiSettings;

        let procs = (1..=4)
            .map(|pid| ProcRec {
                pid,
                ppid: pid - 1,
                comm: format!("p{pid}"),
                mem: MemSet {
                    rss: Some(1000 * u64::try_from(pid).unwrap_or(1)),
                    vsz: Some(1000 * u64::try_from(pid).unwrap_or(1)),
                    ..MemSet::default()
                },
            })
            .collect();
        let snapshot = Snapshot {
            procs,
            total_mem: 100_000,
            used_mem: 50_000,
        };
        App::with_snapshot(snapshot, UiSettings::default())
    }

    #[test]
    fn mouse_events_at_every_position_of_a_tiny_frame_do_not_panic() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        for mode in [
            AppMode::Normal,
            AppMode::FilterInput,
            AppMode::Menu,
            AppMode::Options,
            AppMode::Help,
        ] {
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::ScrollUp,
                MouseEventKind::ScrollDown,
            ] {
                for y in 0..3 {
                    for x in 0..10 {
                        let mut app = tiny_app();
                        app.mode = mode;
                        let mut terminal =
                            Terminal::new(TestBackend::new(10, 3)).expect("terminal");
                        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
                        app.on_mouse(MouseEvent {
                            kind,
                            column: x,
                            row: y,
                            modifiers: KeyModifiers::NONE,
                        });
                        terminal
                            .draw(|frame| draw(frame, &mut app))
                            .unwrap_or_else(|err| panic!("{mode:?} {kind:?} at ({x},{y}): {err}"));
                    }
                }
            }
        }
    }

    #[test]
    fn hint_bar_depends_on_mode_and_is_truncated_to_width() {
        let app = App::new(2_000);
        let text = |mode, width| -> String {
            hint_line(mode, width, &app)
                .spans
                .iter()
                .map(|span| span.content.to_string())
                .collect()
        };

        assert!(text(AppMode::Normal, 200).contains("z/Z fold"));
        assert!(text(AppMode::Normal, 200).contains("q quit"));
        assert!(text(AppMode::FilterInput, 200).contains("Enter apply"));
        assert!(text(AppMode::Menu, 200).contains("Enter open"));
        assert!(text(AppMode::Options, 200).contains("←/→ change"));
        assert!(text(AppMode::Help, 200).contains("scroll"));
        for mode in [
            AppMode::Normal,
            AppMode::FilterInput,
            AppMode::Menu,
            AppMode::Options,
            AppMode::Help,
        ] {
            for width in 0..60 {
                assert!(
                    text(mode, width).chars().count() <= width,
                    "{mode:?} {width}"
                );
            }
        }
        assert_eq!(text(AppMode::Normal, 0), "");
    }
}

/// Whole-screen `TestBackend` tests over a deterministic 4-level synthetic tree.
#[cfg(test)]
mod screens {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use ratatui::Terminal;

    use super::draw;
    use crate::collect::{MemSet, ProcRec, Snapshot};
    use crate::model::{AggMode, MemKind};
    use crate::ui::app::{App, GraphSymbolSetting, UiSettings};

    const MIB: u64 = 1024 * 1024;
    const WIDTH: u16 = 110;
    const HEIGHT: u16 = 30;

    // launchd(100) ─┬─ shell(200) ─┬─ editor(300) ── plugin(400, footprint unknown)
    //               │              └─ build(50)
    //               └─ daemon(500)
    // stray(60)
    fn synthetic_app(agg_mode: AggMode, symbols: GraphSymbolSetting, filter: Option<&str>) -> App {
        let rec = |pid, ppid, comm: &str, mib: u64, known: bool| ProcRec {
            pid,
            ppid,
            comm: comm.into(),
            mem: MemSet {
                rss: Some(mib * MIB),
                vsz: Some(mib * MIB),
                footprint: known.then_some(mib * MIB),
                ..MemSet::default()
            },
        };
        let snapshot = Snapshot {
            procs: vec![
                rec(1, 0, "launchd", 100, true),
                rec(2, 1, "shell", 200, true),
                rec(3, 2, "editor", 300, true),
                rec(4, 3, "plugin", 400, false),
                rec(5, 2, "build", 50, true),
                rec(6, 1, "daemon", 500, true),
                rec(7, 0, "stray", 60, true),
            ],
            total_mem: 4096 * MIB,
            used_mem: 2048 * MIB,
        };
        let settings = UiSettings {
            mem_kind: MemKind::Footprint,
            agg_mode,
            graph_symbol: symbols,
            truecolor: crate::ui::theme::TruecolorMode::On,
            ..UiSettings::default()
        };
        let mut app = App::with_snapshot(snapshot, settings);
        for pct in [10, 30, 50, 70, 90, 100, 60, 40, 20, 80] {
            app.mem_history.push(4096 * MIB / 100 * pct);
        }
        if let Some(text) = filter {
            app.set_filter(text);
        }
        app
    }

    fn render(app: &mut App) -> (Vec<String>, Buffer) {
        render_sized(app, WIDTH, HEIGHT)
    }

    fn render_sized(app: &mut App, width: u16, height: u16) -> (Vec<String>, Buffer) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let lines = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect();
        (lines, buffer)
    }

    #[test]
    fn readme_screen_capture_matches_the_renderer() {
        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        let (lines, _) = render_sized(&mut app, 100, 24);
        let capture = lines
            .iter()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        let body = format!(
            "A plain-text capture of the TUI, rendered by the test suite from a synthetic process tree \
             (100×24; the green→red usage gradient and the selected-row highlight are colours and do \
             not show up in text). `launchd` is the selected row.\n\n```text\n{capture}\n```\n\n\
             To capture your own: run `pspp` in a terminal about 120 columns wide and use the terminal's \
             screenshot tool (macOS: `Cmd-Shift-4`, then `Space` and click the window)."
        );
        crate::docsync::sync_block(
            "README.md",
            crate::docsync::markdown_markers("screen"),
            &body,
        );
    }

    /// The table row for `comm`: `(y, text without the frame's left border)`.
    fn row_of<'a>(lines: &'a [String], comm: &str) -> (usize, &'a str) {
        lines
            .iter()
            .enumerate()
            .find(|(_, line)| {
                line.contains(&format!(" {comm} "))
                    || line.contains(&format!("─ {comm} "))
                    || line.starts_with(&format!("│{comm} "))
                    || line.contains(&format!("] {comm} "))
            })
            .map(|(y, line)| (y, line.strip_prefix('│').unwrap_or(line)))
            .unwrap_or_else(|| panic!("no row for {comm}:\n{}", lines.join("\n")))
    }

    fn has_row(lines: &[String], comm: &str) -> bool {
        lines.iter().any(|line| {
            line.strip_prefix('│')
                .is_some_and(|rest| rest.split_whitespace().any(|word| word == comm))
        })
    }

    /// Number of METER cells on screen row `y` drawn in a "filled" (non-background) colour.
    fn filled_meter_cells(app: &App, buffer: &Buffer, lines: &[String], y: usize) -> usize {
        let header = lines
            .iter()
            .find(|line| line.contains("METER"))
            .expect("header line");
        let meter_x = header
            .find("METER")
            .map(|byte| unicode_width::UnicodeWidthStr::width(&header[..byte]))
            .unwrap_or(0);
        let raw_empty = app.theme.color(app.theme.meter_bg);
        let selected_bg = app.theme.color(app.theme.selected_bg);
        let selected_empty = if raw_empty == selected_bg && raw_empty != Color::Reset {
            Color::Gray
        } else {
            raw_empty
        };
        (meter_x..meter_x + 18)
            .filter(|x| {
                let cell = &buffer[(
                    u16::try_from(*x).unwrap_or(0),
                    u16::try_from(y).unwrap_or(0),
                )];
                let empty = if cell.bg == selected_bg && selected_bg != Color::Reset {
                    selected_empty
                } else {
                    raw_empty
                };
                matches!(cell.symbol(), "■" | "#") && cell.fg != empty
            })
            .count()
    }

    #[test]
    fn rollup_screen_shows_tree_glyphs_sums_and_approximate_markers() {
        for symbols in [
            GraphSymbolSetting::Braille,
            GraphSymbolSetting::Block,
            GraphSymbolSetting::Tty,
        ] {
            let mut app = synthetic_app(AggMode::Rollup, symbols, None);
            let (lines, _) = render(&mut app);
            let body = |comm| row_of(&lines, comm).1.to_owned();

            assert!(lines[1].contains("agg rollup") && lines[1].contains("used 2.0 GiB / 4.0 GiB"));
            assert!(
                lines.iter().any(|l| l.contains("FOOTPRINT Σ~↓")),
                "approx header"
            );

            // Tree glyphs, depth by depth.
            assert!(
                body("launchd").starts_with("[-] launchd"),
                "{}",
                body("launchd")
            );
            assert!(body("shell").starts_with("├─ [-] shell"));
            assert!(body("editor").starts_with("│  ├─ [-] editor"));
            assert!(body("plugin").starts_with("│  │  └─ plugin"));
            assert!(body("build").starts_with("│  └─ build"));
            assert!(body("daemon").starts_with("└─ daemon"));
            assert!(body("stray").starts_with("stray"));

            // Σ values (rollup) with `~` wherever the unknown footprint feeds in.
            assert!(body("launchd").contains("1.1 GiB~"), "{}", body("launchd"));
            assert!(body("shell").contains("550.0 MiB~"));
            assert!(body("editor").contains("300.0 MiB~"));
            assert!(body("plugin").contains("0 B~"));
            assert!(body("build").contains("50.0 MiB") && !body("build").contains('~'));
            assert!(body("daemon").contains("500.0 MiB") && !body("daemon").contains('~'));
            assert!(body("launchd").contains("28.1%"), "% of 4 GiB RAM");

            if symbols == GraphSymbolSetting::Tty {
                assert!(
                    !lines.iter().any(|l| l.contains('■')),
                    "Tty mode must use '#'/'-' instead of '■'"
                );
                assert!(lines
                    .iter()
                    .any(|l| l.contains("used 2.0 GiB / 4.0 GiB #") && l.contains("- 50%")));
                assert!(body("launchd").contains("##################"));
                assert!(body("shell").contains("#########---------"));
                assert!(body("plugin").contains("------------------"));
            } else {
                assert!(lines.iter().any(|l| l.contains('■')));
            }
        }
    }

    #[test]
    fn flat_screen_sums_direct_children_only() {
        let mut app = synthetic_app(AggMode::Flat, GraphSymbolSetting::Braille, None);
        let (lines, _) = render(&mut app);
        let body = |comm| row_of(&lines, comm).1.to_owned();

        assert!(lines[1].contains("agg flat"));
        // launchd's own 100 MiB is excluded: shell(200) + daemon(500).
        assert!(body("launchd").contains("100.0 MiB") && body("launchd").contains("700.0 MiB"));
        assert!(body("shell").contains("350.0 MiB"), "{}", body("shell"));
        assert!(
            body("editor").contains("0 B~"),
            "its only child has unknown footprint"
        );
        assert!(body("build").contains("0 B"));
        assert_eq!(body("daemon").matches("500.0 MiB").count(), 1, "own only");
        assert!(body("daemon").contains(" 0 B "), "flat Σ of a leaf is 0");
        assert!(body("launchd").starts_with("[-] launchd"));
        assert!(body("build").starts_with("│  ├─ build"));
        assert!(body("editor").starts_with("│  └─ [-] editor"));
        assert!(body("plugin").starts_with("│     └─ plugin"));
    }

    #[test]
    fn filtered_screen_keeps_matches_and_ancestors_only() {
        for mode in [AggMode::Rollup, AggMode::Flat] {
            let mut app = synthetic_app(mode, GraphSymbolSetting::Block, Some("plug"));
            let (lines, buffer) = render(&mut app);

            for shown in ["launchd", "shell", "editor", "plugin"] {
                assert!(has_row(&lines, shown), "{mode:?}: {shown} must stay");
            }
            for hidden in ["build", "daemon", "stray"] {
                assert!(
                    !has_row(&lines, hidden),
                    "{mode:?}: {hidden} must be filtered out"
                );
            }
            assert!(lines[1].contains("filter: plug"));

            // Hidden later siblings leave no dangling `├─`: the chain is all `└─`.
            assert!(row_of(&lines, "shell").1.starts_with("└─ [-] shell"));
            assert!(row_of(&lines, "editor").1.starts_with("   └─ [-] editor"));
            assert!(row_of(&lines, "plugin").1.starts_with("      └─ plugin"));

            // Direct match highlighted, context ancestors dimmed.
            let (y, line) = row_of(&lines, "plugin");
            let x = line
                .find("plugin")
                .map(|byte| line[..byte].chars().count() + 1)
                .unwrap_or(0);
            let direct = &buffer[(u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0))];
            assert_eq!(direct.fg, app.theme.color(app.theme.hi_fg));
            let (y, line) = row_of(&lines, "shell");
            let x = line
                .find("shell")
                .map(|byte| line[..byte].chars().count() + 1)
                .unwrap_or(0);
            let context = &buffer[(u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0))];
            assert_eq!(context.fg, app.theme.color(app.theme.inactive_fg));
        }
    }

    #[test]
    fn collapsed_node_stays_collapsed_while_filtering() {
        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, Some("plug"));
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let press = |app: &mut App, code| app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
        press(&mut app, KeyCode::Down); // select shell
        press(&mut app, KeyCode::Left); // collapse it
        let (lines, _) = render(&mut app);
        assert!(has_row(&lines, "shell"));
        assert!(!has_row(&lines, "editor") && !has_row(&lines, "plugin"));
        assert!(row_of(&lines, "shell").1.starts_with("└─ [+] shell"));
    }

    #[test]
    fn meter_fill_tracks_each_rows_share_of_the_largest_sum() {
        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        let (lines, buffer) = render(&mut app);
        // At 110 columns the meter is 18 cells wide; scale = launchd's 1150 MiB (launchd itself is
        // the selected row, drawn solid).
        let expected = [
            ("shell", 9),
            ("editor", 5),
            ("daemon", 8),
            ("build", 1),
            ("stray", 1),
            ("plugin", 0),
        ];
        for (comm, filled) in expected {
            let (y, _) = row_of(&lines, comm);
            assert_eq!(
                filled_meter_cells(&app, &buffer, &lines, y),
                filled,
                "{comm}"
            );
        }
        // Rows are never recoloured to a Reset/default colour.
        let (y, _) = row_of(&lines, "shell");
        assert_ne!(buffer[(5, u16::try_from(y).unwrap_or(0))].fg, Color::Reset);
    }

    #[test]
    fn memory_graph_uses_the_selected_glyph_family() {
        let braille = |c: char| ('\u{2801}'..='\u{28ff}').contains(&c);
        let block_only = |c: char| "▗▐▖▄▟▌▙".contains(c);
        let tty_only = |c: char| "░▒".contains(c);

        for (symbols, expect) in [
            (GraphSymbolSetting::Braille, 0),
            (GraphSymbolSetting::Block, 1),
            (GraphSymbolSetting::Tty, 2),
        ] {
            let mut app = synthetic_app(AggMode::Rollup, symbols, None);
            let (lines, _) = render(&mut app);
            // Graph rows live inside the `mem` box (screen rows 3..=5).
            let graph: String = lines[3..=5].join("");
            let counts = (
                graph.chars().filter(|c| braille(*c)).count(),
                graph.chars().filter(|c| block_only(*c)).count(),
                graph.chars().filter(|c| tty_only(*c)).count(),
            );
            match expect {
                0 => assert!(
                    counts.0 > 0 && counts.1 == 0 && counts.2 == 0,
                    "braille {counts:?}"
                ),
                1 => assert!(
                    counts.1 > 0 && counts.0 == 0 && counts.2 == 0,
                    "block {counts:?}"
                ),
                _ => assert!(
                    counts.2 > 0 && counts.0 == 0 && counts.1 == 0,
                    "tty {counts:?}"
                ),
            }
        }
    }

    #[test]
    fn mem_box_summary_names_used_total_and_sigma() {
        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        let (lines, _) = render(&mut app);
        let summary = lines
            .iter()
            .find(|l| l.contains("used 2.0 GiB / 4.0 GiB ■"))
            .expect("summary line");
        assert!(summary.contains("50%"), "{summary}");
        assert!(summary.contains("Σ FOOTPRINT 1.2 GiB"), "{summary}");
    }

    fn rect_lines(buffer: &Buffer, rect: ratatui::layout::Rect) -> Vec<String> {
        (rect.y..rect.y + rect.height)
            .map(|y| {
                (rect.x..rect.x + rect.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn overlays_clear_underlying_table_and_apply_main_bg() {
        use crate::ui::app::AppMode;
        use crate::ui::theme::{Rgb, Theme, ThemeSpec};
        use ratatui::layout::Rect;
        use ratatui::widgets::Block;

        let width = 110;
        let height = 28;
        let area = Rect::new(0, 0, width, height);
        let nord_rgb = Rgb(0x2e, 0x34, 0x40);
        let nord_spec = ThemeSpec {
            main_bg: Some(nord_rgb),
            ..ThemeSpec::default()
        };

        // Build both `synthetic_app` and a worker-populated variant whose long COMM names and
        // extra rows span directly beneath all three centered modal rectangles.
        let worker_app = || {
            let mut procs = vec![
                ProcRec {
                    pid: 1,
                    ppid: 0,
                    comm: "launchd".into(),
                    mem: MemSet {
                        rss: Some(100 * MIB),
                        vsz: Some(100 * MIB),
                        footprint: Some(100 * MIB),
                        ..MemSet::default()
                    },
                },
                ProcRec {
                    pid: 2,
                    ppid: 1,
                    comm: "shell".into(),
                    mem: MemSet {
                        rss: Some(200 * MIB),
                        vsz: Some(200 * MIB),
                        footprint: Some(200 * MIB),
                        ..MemSet::default()
                    },
                },
                ProcRec {
                    pid: 3,
                    ppid: 1,
                    comm: "daemon".into(),
                    mem: MemSet {
                        rss: Some(500 * MIB),
                        vsz: Some(500 * MIB),
                        footprint: Some(500 * MIB),
                        ..MemSet::default()
                    },
                },
            ];
            for idx in 0..20 {
                procs.push(ProcRec {
                    pid: 100 + idx,
                    ppid: 2,
                    comm: format!("worker-{idx:02}-background-service-process-name"),
                    mem: MemSet {
                        rss: Some(80 * MIB),
                        vsz: Some(80 * MIB),
                        footprint: Some(80 * MIB),
                        ..MemSet::default()
                    },
                });
            }
            let mut app = App::with_snapshot(
                Snapshot {
                    procs,
                    total_mem: 4096 * MIB,
                    used_mem: 2048 * MIB,
                },
                UiSettings {
                    mem_kind: MemKind::Footprint,
                    truecolor: crate::ui::theme::TruecolorMode::On,
                    ..UiSettings::default()
                },
            );
            app.mem_history.push(2048 * MIB);
            app
        };

        for make_base_app in [
            || synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None),
            worker_app,
        ] {
            for with_nord_bg in [false, true] {
                for (mode, overlay) in [
                    (AppMode::Menu, super::menu_overlay_rect(area)),
                    (AppMode::Options, super::options_overlay_rect(area)),
                    (AppMode::Help, super::help_overlay_rect(area)),
                ] {
                    let mut app = make_base_app();
                    if with_nord_bg {
                        app.theme = Theme::build(true, true, Some(&nord_spec));
                    } else {
                        app.theme = Theme::build(true, false, Some(&nord_spec));
                    }

                    // Sanity-check that Normal mode actually paints `■` meters inside `overlay`.
                    let (_, normal_buf) = render_sized(&mut app, width, height);
                    assert!(
                        rect_lines(&normal_buf, overlay)
                            .iter()
                            .any(|line| line.contains('■')),
                        "expected Normal mode to draw meters inside {overlay:?}"
                    );

                    app.mode = mode;
                    let (_, buffer) = render_sized(&mut app, width, height);
                    let overlay_text = rect_lines(&buffer, overlay).join("\n");

                    for leaked in [
                        "launchd", "shell", "daemon", "worker", "plugin", "stray", "■",
                    ] {
                        assert!(
                            !overlay_text.contains(leaked),
                            "{mode:?} (nord={with_nord_bg}) leaked {leaked:?}:\n{overlay_text}"
                        );
                    }

                    let inner = Block::bordered().inner(overlay);
                    let expected_bg = if with_nord_bg {
                        Color::Rgb(nord_rgb.0, nord_rgb.1, nord_rgb.2)
                    } else {
                        Color::Reset
                    };
                    let selected_bg = app.theme.color(app.theme.selected_bg);

                    for y in overlay.y..overlay.y + overlay.height {
                        for x in overlay.x..overlay.x + overlay.width {
                            let bg = buffer[(x, y)].bg;
                            let in_inner = x >= inner.x
                                && x < inner.x + inner.width
                                && y >= inner.y
                                && y < inner.y + inner.height;
                            let uses_selected_bg = mode == AppMode::Options
                                && in_inner
                                && (y == inner.y || y >= inner.y + inner.height - 2);
                            if uses_selected_bg {
                                assert_eq!(
                                    bg, selected_bg,
                                    "{mode:?} at ({x},{y}) expected selected_bg"
                                );
                            } else {
                                assert_eq!(
                                    bg, expected_bg,
                                    "{mode:?} (nord={with_nord_bg}) at ({x},{y}) had unexpected bg"
                                );
                            }
                        }
                    }

                    // At 110x28, Options has 18 list rows for 17 options; row 18 must be blank.
                    if mode == AppMode::Options {
                        let empty_row_y = inner.y + 17;
                        let empty_row: String = (inner.x..inner.x + inner.width)
                            .map(|x| buffer[(x, empty_row_y)].symbol().to_owned())
                            .collect();
                        assert!(
                            empty_row.chars().all(|ch| ch == ' '),
                            "expected unused Options list row to be empty, got {empty_row:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn menu_buttons_center_labels_inside_interiors_and_degrade_on_short_terminals() {
        use crate::ui::app::AppMode;
        use ratatui::layout::Rect;
        use ratatui::widgets::Block;

        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        app.mode = AppMode::Menu;
        let (_, buffer) = render_sized(&mut app, 110, 28);

        let overlay = super::menu_overlay_rect(Rect::new(0, 0, 110, 28));
        let inner = Block::bordered().inner(overlay);
        let button_height = 3_u16;
        let spacing = 1_u16;
        let total_buttons_height = button_height * 3 + spacing * 2;
        let first_y = inner.y + (inner.height - total_buttons_height) / 2;

        for (idx, (label, selected)) in [("Options", true), ("Help", false), ("Quit", false)]
            .into_iter()
            .enumerate()
        {
            let button_rect = Rect {
                x: inner.x + 1,
                y: first_y + u16::try_from(idx).unwrap() * (button_height + spacing),
                width: inner.width - 2,
                height: button_height,
            };
            let rows = rect_lines(&buffer, button_rect);
            if selected {
                assert!(
                    rows[0].starts_with('╔')
                        && rows[0].ends_with('╗')
                        && rows[0].chars().all(|c| matches!(c, '╔' | '═' | '╗')),
                    "selected top border overwritten: {:?}",
                    rows[0]
                );
                assert!(
                    rows[1].starts_with('║') && rows[1].ends_with('║') && rows[1].contains(label),
                    "selected interior row missing {label}: {:?}",
                    rows[1]
                );
                assert!(
                    rows[2].starts_with('╚')
                        && rows[2].ends_with('╝')
                        && rows[2].chars().all(|c| matches!(c, '╚' | '═' | '╝')),
                    "selected bottom border malformed: {:?}",
                    rows[2]
                );
            } else {
                assert!(
                    rows[0].starts_with('┌')
                        && rows[0].ends_with('┐')
                        && rows[0].chars().all(|c| matches!(c, '┌' | '─' | '┐')),
                    "unselected top border overwritten: {:?}",
                    rows[0]
                );
                assert!(
                    rows[1].starts_with('│') && rows[1].ends_with('│') && rows[1].contains(label),
                    "unselected interior row missing {label}: {:?}",
                    rows[1]
                );
            }
        }

        // Short terminal (20x8): single-line unbordered buttons render cleanly without panicking.
        let (short_lines, short_buf) = render_sized(&mut app, 20, 8);
        let short_joined = short_lines.join("\n");
        for label in ["Options", "Help", "Quit"] {
            assert!(
                short_joined.contains(label),
                "20x8 menu missing {label}:\n{short_joined}"
            );
        }
        let options_y = short_lines
            .iter()
            .position(|line| line.contains("Options"))
            .expect("Options row on 20x8") as u16;
        let options_x = short_lines[usize::from(options_y)]
            .find("Options")
            .expect("Options col") as u16;
        assert_eq!(
            short_buf[(options_x, options_y)].bg,
            app.theme.color(app.theme.selected_bg)
        );
        assert_eq!(
            short_buf[(options_x, options_y)].fg,
            app.theme.color(app.theme.selected_fg)
        );
    }

    #[test]
    fn options_wraps_update_ms_description_in_two_rows_without_duplicate_hint_bars() {
        use crate::ui::app::{AppMode, OptionKey};
        use ratatui::layout::Rect;
        use ratatui::widgets::Block;

        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        app.mode = AppMode::Options;
        assert_eq!(app.selected_option_key(), OptionKey::UpdateMs);

        let (lines, buffer) = render_sized(&mut app, 80, 24);
        let overlay = super::options_overlay_rect(Rect::new(0, 0, 80, 24));
        let inner = Block::bordered().inner(overlay);
        let desc_rect = Rect {
            x: inner.x,
            y: inner.y + inner.height - 2,
            width: inner.width,
            height: 2,
        };

        let desc_rows = rect_lines(&buffer, desc_rect);
        let desc_joined = desc_rows
            .iter()
            .map(|row| row.trim())
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            desc_joined.contains("Refresh interval") && desc_joined.contains("adjusts by 100."),
            "UpdateMs description clipped: {desc_rows:?}"
        );
        assert_eq!(desc_joined, OptionKey::UpdateMs.description());

        for &key in App::option_keys() {
            let wrapped = crate::ui::help::wrap_text(key.description(), usize::from(inner.width));
            assert!(
                wrapped.len() <= 2,
                "{key:?} description exceeds 2 rows on 80x24: {wrapped:?}"
            );
        }

        let selected_bg = app.theme.color(app.theme.selected_bg);
        for y in desc_rect.y..desc_rect.y + desc_rect.height {
            for x in desc_rect.x..desc_rect.x + desc_rect.width {
                assert_eq!(
                    buffer[(x, y)].bg,
                    selected_bg,
                    "description cell ({x},{y}) missing selected_bg"
                );
            }
        }

        // Hint bar appears only once (outer frame bottom hint row, y = 22), never inside Options.
        let hint_rows: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(y, line)| line.contains("Esc/q back").then_some(y))
            .collect();
        assert_eq!(
            hint_rows,
            vec![22],
            "unexpected hint rows:\n{}",
            lines.join("\n")
        );

        // Help at 80x24 likewise has only the outer bottom hint row, never an internal duplicate.
        app.mode = AppMode::Help;
        let (help_lines, _) = render_sized(&mut app, 80, 24);
        let help_hint_rows: Vec<usize> = help_lines
            .iter()
            .enumerate()
            .filter_map(|(y, line)| line.contains("Esc/q back").then_some(y))
            .collect();
        assert_eq!(
            help_hint_rows,
            vec![22],
            "unexpected Help hint rows:\n{}",
            help_lines.join("\n")
        );
    }

    #[test]
    fn overflowing_options_and_help_scrollbars_use_single_line_track() {
        use crate::ui::app::AppMode;
        use ratatui::layout::Rect;
        use ratatui::widgets::Block;

        // Overflowing Options at 80x16 (8 visible rows for 17 options).
        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        app.mode = AppMode::Options;
        let (opt_lines, opt_buf) = render_sized(&mut app, 80, 16);
        assert!(
            !opt_lines.iter().any(|line| line.contains('║')),
            "Options rendered double-line scrollbar track:\n{}",
            opt_lines.join("\n")
        );
        let opt_inner =
            Block::bordered().inner(super::options_overlay_rect(Rect::new(0, 0, 80, 16)));
        let opt_bar_x = opt_inner.x + opt_inner.width - 1;
        let opt_list_height = opt_inner.height - 2;
        let opt_bar: Vec<&str> = (opt_inner.y..opt_inner.y + opt_list_height)
            .map(|y| opt_buf[(opt_bar_x, y)].symbol())
            .collect();
        assert!(
            opt_bar.iter().all(|sym| matches!(*sym, "│" | "█"))
                && opt_bar.contains(&"│")
                && opt_bar.contains(&"█"),
            "unexpected Options scrollbar glyphs: {opt_bar:?}"
        );

        // Overflowing Help at 80x20.
        app.mode = AppMode::Help;
        let (help_lines, help_buf) = render_sized(&mut app, 80, 20);
        assert!(
            !help_lines.iter().any(|line| line.contains('║')),
            "Help rendered double-line scrollbar track:\n{}",
            help_lines.join("\n")
        );
        let help_inner = Block::bordered().inner(super::help_overlay_rect(Rect::new(0, 0, 80, 20)));
        let help_bar_x = help_inner.x + help_inner.width - 1;
        let help_bar: Vec<&str> = (help_inner.y..help_inner.y + help_inner.height)
            .map(|y| help_buf[(help_bar_x, y)].symbol())
            .collect();
        assert!(
            help_bar.iter().all(|sym| matches!(*sym, "│" | "█"))
                && help_bar.contains(&"│")
                && help_bar.contains(&"█"),
            "unexpected Help scrollbar glyphs: {help_bar:?}"
        );
    }

    #[test]
    fn single_root_and_multi_root_tree_connectors_render_accurately() {
        let mk_proc = |pid, ppid, comm: &str, mib: u64| ProcRec {
            pid,
            ppid,
            comm: comm.into(),
            mem: MemSet {
                rss: Some(mib * MIB),
                vsz: Some(mib * MIB),
                ..MemSet::default()
            },
        };

        // 1) Sole root launchd -> edge + code -> helper + gpu -> leaf.
        let mut single_root = App::with_snapshot(
            Snapshot {
                procs: vec![
                    mk_proc(1, 0, "launchd", 10),
                    mk_proc(2, 1, "edge", 400),
                    mk_proc(3, 2, "helper", 300),
                    mk_proc(4, 3, "leaf", 200),
                    mk_proc(5, 2, "gpu", 100),
                    mk_proc(6, 1, "code", 50),
                ],
                total_mem: 4096 * MIB,
                used_mem: 1024 * MIB,
            },
            UiSettings::default(),
        );
        let (lines, _) = render_sized(&mut single_root, 100, 20);
        assert!(row_of(&lines, "launchd").1.starts_with("[-] launchd"));
        assert!(row_of(&lines, "edge").1.starts_with("├─ [-] edge"));
        assert!(row_of(&lines, "helper").1.starts_with("│  ├─ [-] helper"));
        assert!(row_of(&lines, "leaf").1.starts_with("│  │  └─ leaf"));
        assert!(row_of(&lines, "gpu").1.starts_with("│  └─ gpu"));
        assert!(row_of(&lines, "code").1.starts_with("└─ code"));

        // 2) Multi-root: non-last root_a has last child `└─ [-] child` with grandchild.
        let mut multi_root = App::with_snapshot(
            Snapshot {
                procs: vec![
                    mk_proc(1, 0, "root_a", 0),
                    mk_proc(2, 1, "first_child", 100),
                    mk_proc(3, 2, "first_gc", 100),
                    mk_proc(4, 1, "child", 50),
                    mk_proc(5, 4, "grandchild", 50),
                    mk_proc(6, 0, "root_b", 100),
                ],
                total_mem: 4096 * MIB,
                used_mem: 512 * MIB,
            },
            UiSettings::default(),
        );
        let (lines, _) = render_sized(&mut multi_root, 100, 20);
        assert!(row_of(&lines, "root_a").1.starts_with("[-] root_a"));
        assert!(row_of(&lines, "first_child")
            .1
            .starts_with("├─ [-] first_child"));
        assert!(row_of(&lines, "first_gc").1.starts_with("│  └─ first_gc"));
        assert!(row_of(&lines, "child").1.starts_with("└─ [-] child"));
        assert!(
            row_of(&lines, "grandchild")
                .1
                .starts_with("   └─ grandchild"),
            "unexpected continuation under └─ child: {}",
            row_of(&lines, "grandchild").1
        );
        assert!(row_of(&lines, "root_b").1.starts_with("root_b"));
    }

    #[test]
    fn selected_zero_meter_gradient_cells_and_no_color_ascii_meters() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        let (lines, buffer) = render(&mut app);
        let header = lines.iter().find(|l| l.contains("METER")).expect("header");
        let meter_x =
            unicode_width::UnicodeWidthStr::width(&header[..header.find("METER").unwrap()]) as u16;

        // Non-selected `shell` (9 filled cells out of 18) uses per-cell positional gradient.
        let (shell_y, _) = row_of(&lines, "shell");
        let shell_y = shell_y as u16;
        let first_fg = buffer[(meter_x, shell_y)].fg;
        let ninth_fg = buffer[(meter_x + 8, shell_y)].fg;
        assert_ne!(
            first_fg, ninth_fg,
            "expected positional gradient across filled meter cells"
        );
        assert_eq!(
            buffer[(meter_x + 9, shell_y)].fg,
            app.theme.color(app.theme.meter_bg)
        );

        // Move selection to `plugin` (row 3: launchd, shell, editor, plugin), which has 0 B~ (0%).
        for _ in 0..3 {
            app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let (lines, buffer) = render(&mut app);
        let (plugin_y, _) = row_of(&lines, "plugin");
        assert_eq!(
            filled_meter_cells(&app, &buffer, &lines, plugin_y),
            0,
            "selected 0% row must show 0 filled meter cells"
        );
        let plugin_y = plugin_y as u16;
        let selected_bg = app.theme.color(app.theme.selected_bg);
        let meter_bg = app.theme.color(app.theme.meter_bg);
        for x in meter_x..meter_x + 18 {
            assert_eq!(buffer[(x, plugin_y)].bg, selected_bg);
            assert_eq!(buffer[(x, plugin_y)].fg, meter_bg);
        }

        // `set_no_color(true)` switches meters to '#' / '-' even when graph_symbol is Braille.
        app.set_no_color(true);
        let (nc_lines, _) = render(&mut app);
        assert!(!nc_lines.iter().any(|l| l.contains('■')));
        assert!(nc_lines
            .iter()
            .any(|l| l.contains("used 2.0 GiB / 4.0 GiB #") && l.contains("- 50%")));
        assert!(row_of(&nc_lines, "launchd")
            .1
            .contains("##################"));
        assert!(row_of(&nc_lines, "shell").1.contains("#########---------"));
        assert!(row_of(&nc_lines, "plugin").1.contains("------------------"));
    }

    #[test]
    fn eleven_char_approx_values_keep_tilde_and_active_sort_header_shows_arrow_and_hi_fg() {
        use crate::model::SortBy;

        // 1_048_000 bytes formats as "1023.4 KiB" (10 chars); unknown child / own makes it
        // "1023.4 KiB~" (11 chars) in OWN and <MEM> Σ.
        let snapshot = Snapshot {
            procs: vec![
                ProcRec {
                    pid: 1,
                    ppid: 0,
                    comm: "parent".into(),
                    mem: MemSet {
                        rss: Some(1_048_000),
                        vsz: Some(1_048_000),
                        footprint: Some(1_048_000),
                        ..MemSet::default()
                    },
                },
                ProcRec {
                    pid: 2,
                    ppid: 1,
                    comm: "child_unknown".into(),
                    mem: MemSet {
                        rss: Some(0),
                        vsz: Some(0),
                        footprint: None,
                        ..MemSet::default()
                    },
                },
            ],
            total_mem: 4096 * MIB,
            used_mem: 512 * MIB,
        };
        let mut app = App::with_snapshot(
            snapshot,
            UiSettings {
                mem_kind: MemKind::Footprint,
                truecolor: crate::ui::theme::TruecolorMode::On,
                ..UiSettings::default()
            },
        );
        // Also verify own_approximate (11-char OWN "1023.4 KiB~") via Flat mode on parent or direct DisplayRow.
        let (lines, _) = render_sized(&mut app, 100, 16);
        let parent_row = row_of(&lines, "parent").1;
        assert!(
            parent_row.contains("1023.4 KiB~"),
            "11-char Σ clipped '~': {parent_row}"
        );

        // Verify both OWN (width 11) and shortest header `RSS Σ` (min agg_len 11) render a full
        // 11-char `"1023.4 KiB~"` value in TestBackend without clipping the trailing `~`.
        let mut rss_app = App::with_snapshot(
            Snapshot {
                procs: vec![ProcRec {
                    pid: 1,
                    ppid: 0,
                    comm: "worker".into(),
                    mem: MemSet {
                        rss: Some(1_048_000),
                        vsz: Some(1_048_000),
                        ..MemSet::default()
                    },
                }],
                total_mem: 4096 * MIB,
                used_mem: 512 * MIB,
            },
            UiSettings {
                mem_kind: MemKind::Rss,
                sort_by: SortBy::Comm,
                ..UiSettings::default()
            },
        );
        let mut term = Terminal::new(TestBackend::new(80, 5)).expect("terminal");
        term.draw(|frame| {
            let row_area = ratatui::layout::Rect::new(1, 2, 78, 1);
            let cols = super::build_column_layout(row_area, "RSS Σ", 5);
            let rects = super::data_column_rects(&cols);
            assert_eq!(rects[2].width, 11);
            assert_eq!(rects[3].width, 11);
            let drow = crate::ui::app::DisplayRow {
                pid: 1,
                comm: "worker".into(),
                own: 1_048_000,
                own_approximate: true,
                aggregated: 1_048_000,
                aggregated_approximate: true,
                child_count: 0,
                tree_prefix: String::new(),
                toggle_marker: None,
            };
            super::render_row_cells(frame, &drow, &rects, &rss_app, 1_048_000, false);
        })
        .expect("draw");
        let rendered_row: String = (0..80)
            .map(|x| term.backend().buffer()[(x, 2)].symbol().to_owned())
            .collect();
        assert_eq!(
            rendered_row.matches("1023.4 KiB~").count(),
            2,
            "expected unclipped '1023.4 KiB~' in both OWN and RSS Σ: {rendered_row:?}"
        );
        let _ = &mut rss_app;

        // Active sort header shows ↓/↑ and hi_fg across all SortBy variants.
        let hi_fg = app.theme.color(app.theme.hi_fg);
        let title_fg = app.theme.color(app.theme.title);
        for (sort_by, sort_desc, expected_token, inactive_token) in [
            (SortBy::Aggregated, true, "FOOTPRINT Σ~↓", "COMM"),
            (SortBy::Aggregated, false, "FOOTPRINT Σ~↑", "OWN"),
            (SortBy::Own, true, "OWN↓", "PID"),
            (SortBy::Own, false, "OWN↑", "COMM"),
            (SortBy::Pid, false, "PID↑", "OWN"),
            (SortBy::Comm, false, "COMM↑", "PID"),
            (SortBy::Comm, true, "COMM↓", "OWN"),
        ] {
            app.sort_by = sort_by;
            app.sort_desc = sort_desc;
            let (hdr_lines, hdr_buf) = render_sized(&mut app, 100, 16);
            let (hdr_y, hdr_line) = hdr_lines
                .iter()
                .enumerate()
                .find(|(_, l)| l.contains("METER"))
                .expect("header row");
            assert!(
                hdr_line.contains(expected_token),
                "{sort_by:?} desc={sort_desc} missing {expected_token:?} in {hdr_line:?}"
            );
            let active_x = unicode_width::UnicodeWidthStr::width(
                &hdr_line[..hdr_line.find(expected_token).unwrap()],
            ) as u16;
            assert_eq!(hdr_buf[(active_x, hdr_y as u16)].fg, hi_fg);

            let inactive_x = unicode_width::UnicodeWidthStr::width(
                &hdr_line[..hdr_line.find(inactive_token).unwrap()],
            ) as u16;
            assert_eq!(hdr_buf[(inactive_x, hdr_y as u16)].fg, title_fg);
        }
    }

    #[test]
    fn responsive_status_bar_mem_box_zero_meter_empty_notice_and_cjk_comm() {
        use crate::ui::app::AppMode;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use unicode_width::UnicodeWidthStr;

        // 1) 80x24 status bar keeps `filter: chrome█` visible and never clips mid-word.
        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        for ch in "chrome".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(app.mode, AppMode::FilterInput);
        let (lines_80, buf_80) = render_sized(&mut app, 80, 24);
        assert!(
            lines_80[1].contains("filter: chrome█"),
            "filter prompt clipped on 80x24: {:?}",
            lines_80[1]
        );
        let fx =
            UnicodeWidthStr::width(&lines_80[1][..lines_80[1].find("filter:").unwrap()]) as u16;
        assert_eq!(buf_80[(fx, 1)].fg, app.theme.color(app.theme.hi_fg));

        // Left-crop when filter input alone exceeds status_area.width (width 20 -> status width 18).
        for ch in "_very_long_query_tail".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let (narrow_lines, _) = render_sized(&mut app, 20, 10);
        assert!(
            narrow_lines[1].contains("filter: …") && narrow_lines[1].contains("tail█"),
            "expected left-cropped filter keeping cursor visible, got {:?}",
            narrow_lines[1]
        );

        // Across widths 12..=100 in Normal mode, every kept segment is an intact whole segment.
        let valid_segments = [
            "7 procs",
            "mem FOOTPRINT",
            "agg rollup",
            "sort agg↓",
            "update 2000ms",
            "used 2.0 GiB / 4.0 GiB",
        ];
        let mut normal_app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        for w in 12..=100 {
            let (w_lines, _) = render_sized(&mut normal_app, w, 10);
            let inner_status = w_lines[1]
                .strip_prefix('│')
                .and_then(|s| s.strip_suffix('│'))
                .unwrap_or(&w_lines[1])
                .trim();
            if !inner_status.is_empty() {
                for part in inner_status.split("  ") {
                    assert!(
                        valid_segments.contains(&part),
                        "width {w} produced mid-word clipped segment {part:?} in {inner_status:?}"
                    );
                }
            }
        }

        // 2) `render_mem_box` has no double space before `50%` when `meter_width == 0`.
        let (mem_narrow_lines, _) = render_sized(&mut normal_app, 52, 24);
        let summary_row = mem_narrow_lines
            .iter()
            .find(|l| l.contains("Σ FOOTPRINT"))
            .expect("mem summary row");
        assert!(
            summary_row.contains("used 2.0 GiB / 4.0 GiB 50%")
                && !summary_row.contains("4.0 GiB  50%"),
            "double space in zero-meter mem summary: {summary_row:?}"
        );

        // 3) Zero-match filter shows mode-accurate centered inactive_fg notice; empty snapshot shows "no processes".
        let mut unmatched = synthetic_app(
            AggMode::Rollup,
            GraphSymbolSetting::Braille,
            Some("zzzz_none"),
        );
        assert_eq!(unmatched.mode, AppMode::Normal);
        let (empty_lines, empty_buf) = render_sized(&mut unmatched, 80, 24);
        let (notice_y, notice_line) = empty_lines
            .iter()
            .enumerate()
            .find(|(_, l)| l.contains("no matching processes (press c to clear)"))
            .expect("missing Normal zero-match notice");
        let nx =
            UnicodeWidthStr::width(&notice_line[..notice_line.find("no matching").unwrap()]) as u16;
        assert_eq!(
            empty_buf[(nx, notice_y as u16)].fg,
            unmatched.theme.color(unmatched.theme.inactive_fg)
        );

        let mut live_unmatched = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        live_unmatched.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        for ch in "zzzz_none".chars() {
            live_unmatched.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(live_unmatched.mode, AppMode::FilterInput);
        let (live_lines, live_buf) = render_sized(&mut live_unmatched, 80, 24);
        assert!(
            !live_lines.iter().any(|l| l.contains("press c to clear")),
            "FilterInput must not advertise 'c' to clear while typing"
        );
        let (live_y, live_line) = live_lines
            .iter()
            .enumerate()
            .find(|(_, l)| l.contains("no matching processes (Esc to cancel, Del/Ctrl-U to clear)"))
            .expect("missing FilterInput zero-match notice");
        let live_nx =
            UnicodeWidthStr::width(&live_line[..live_line.find("no matching").unwrap()]) as u16;
        assert_eq!(
            live_buf[(live_nx, live_y as u16)].fg,
            live_unmatched.theme.color(live_unmatched.theme.inactive_fg)
        );
        live_unmatched.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let (committed_lines, _) = render_sized(&mut live_unmatched, 80, 24);
        assert!(committed_lines
            .iter()
            .any(|l| l.contains("no matching processes (press c to clear)")));

        let mut blank_snap = App::with_snapshot(
            Snapshot {
                procs: Vec::new(),
                total_mem: 1024,
                used_mem: 0,
            },
            UiSettings::default(),
        );
        let (blank_lines, _) = render_sized(&mut blank_snap, 80, 16);
        assert!(blank_lines.iter().any(|l| l.contains("no processes")));

        // 4) CJK & emoji COMM names align numeric columns and truncate cleanly with `…`.
        let mut cjk_app = App::with_snapshot(
            Snapshot {
                procs: vec![
                    ProcRec {
                        pid: 10,
                        ppid: 0,
                        comm: "日本語プロセス".into(),
                        mem: MemSet {
                            rss: Some(300 * MIB),
                            vsz: Some(300 * MIB),
                            ..MemSet::default()
                        },
                    },
                    ProcRec {
                        pid: 20,
                        ppid: 0,
                        comm: "非常に長い日本語のプロセス名テスト表示確認".into(),
                        mem: MemSet {
                            rss: Some(200 * MIB),
                            vsz: Some(200 * MIB),
                            ..MemSet::default()
                        },
                    },
                    ProcRec {
                        pid: 30,
                        ppid: 0,
                        comm: "ascii_worker".into(),
                        mem: MemSet {
                            rss: Some(100 * MIB),
                            vsz: Some(100 * MIB),
                            ..MemSet::default()
                        },
                    },
                ],
                total_mem: 1024 * MIB,
                used_mem: 600 * MIB,
            },
            UiSettings::default(),
        );
        let (cjk_lines, cjk_buf) = render_sized(&mut cjk_app, 80, 14);
        let hdr_y = cjk_lines
            .iter()
            .position(|l| l.contains("CHILDREN"))
            .expect("header") as u16;
        // Reconstruct terminal-rendered rows from Buffer (advancing by glyph display width so
        // ratatui's trailing continuation cells after 2-col glyphs are skipped just like Crossterm).
        let rendered_line = |y: u16| -> String {
            let mut x = 0_u16;
            let mut out = String::new();
            while x < 80 {
                let sym = cjk_buf[(x, y)].symbol();
                out.push_str(sym);
                let step = u16::try_from(UnicodeWidthStr::width(sym))
                    .unwrap_or(1)
                    .max(1);
                x = x.saturating_add(step);
            }
            out
        };
        let row1 = rendered_line(hdr_y + 1);
        let row2 = rendered_line(hdr_y + 2);
        let row3 = rendered_line(hdr_y + 3);
        assert_eq!(UnicodeWidthStr::width(row1.as_str()), 80);
        assert_eq!(UnicodeWidthStr::width(row2.as_str()), 80);
        assert_eq!(UnicodeWidthStr::width(row3.as_str()), 80);
        assert!(
            row2.contains('…'),
            "expected overflowing CJK COMM to truncate with '…': {row2:?}"
        );
        // PID column right edge aligns at identical display column across CJK, truncated CJK, and ASCII rows.
        let pid_end_col = |line: &str, pid_str: &str| -> usize {
            let idx = line.find(pid_str).expect("pid") + pid_str.len();
            UnicodeWidthStr::width(&line[..idx])
        };
        assert_eq!(pid_end_col(&row1, " 10 "), pid_end_col(&row3, " 30 "));
        assert_eq!(pid_end_col(&row2, " 20 "), pid_end_col(&row3, " 30 "));
    }

    #[test]
    fn force_tty_renders_square_corners_tty_graphs_ascii_meters_and_ansi16_only() {
        use crate::ui::app::{AppMode, OptionKey};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        assert_eq!(app.graph_symbol, GraphSymbolSetting::Braille);
        assert!(app.rounded_corners);

        // Enable `force_tty` via Options while keeping `graph_symbol = Braille`,
        // `rounded_corners = true`, and `truecolor = On`.
        app.mode = AppMode::Options;
        while app.selected_option_key() != OptionKey::ForceTty {
            app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.mode = AppMode::Normal;
        assert!(app.force_tty);
        assert_eq!(app.graph_symbol, GraphSymbolSetting::Braille);
        assert!(app.rounded_corners);

        let (lines, buffer) = render(&mut app);

        // Outer frame and `mem` sub-box render square corners (`┌`, `┐`, `└`, `┘`), zero rounded.
        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(WIDTH - 1, 0)].symbol(), "┐");
        assert_eq!(buffer[(0, HEIGHT - 1)].symbol(), "└");
        assert_eq!(buffer[(WIDTH - 1, HEIGHT - 1)].symbol(), "┘");
        assert_eq!(buffer[(1, 2)].symbol(), "┌");
        assert_eq!(buffer[(WIDTH - 2, 2)].symbol(), "┐");
        assert_eq!(buffer[(1, 7)].symbol(), "└");
        assert_eq!(buffer[(WIDTH - 2, 7)].symbol(), "┘");
        assert!(
            !lines.iter().any(|line| line.contains(['╭', '╮', '╰', '╯'])),
            "force_tty left rounded corners:\n{}",
            lines.join("\n")
        );

        // Memory history graph uses TTY glyphs (`░`, `▒`, `█`), never braille or quadrant blocks.
        let braille = |c: char| ('\u{2801}'..='\u{28ff}').contains(&c);
        let block_only = |c: char| "▗▐▖▄▟▌▙".contains(c);
        let tty_only = |c: char| "░▒".contains(c);
        let graph: String = lines[3..=5].join("");
        assert!(
            graph.chars().any(tty_only),
            "expected TTY graph glyphs: {graph:?}"
        );
        assert!(
            !graph.chars().any(braille),
            "unexpected braille in force_tty: {graph:?}"
        );
        assert!(
            !graph.chars().any(block_only),
            "unexpected block in force_tty: {graph:?}"
        );

        // Meters use `#` / `-` with zero `■` glyphs.
        assert!(!lines.iter().any(|l| l.contains('■')));
        assert!(lines
            .iter()
            .any(|l| l.contains("used 2.0 GiB / 4.0 GiB #") && l.contains("- 50%")));
        assert!(row_of(&lines, "launchd").1.contains("##################"));
        assert!(row_of(&lines, "shell").1.contains("#########---------"));
        assert!(row_of(&lines, "plugin").1.contains("------------------"));

        // Selecting `shell` (50% meter fill) keeps every `#` and `-` cell distinct from `selected_bg`.
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let (sel_lines, sel_buf) = render(&mut app);
        let header = sel_lines
            .iter()
            .find(|line| line.contains("METER"))
            .expect("header line");
        let meter_x =
            unicode_width::UnicodeWidthStr::width(&header[..header.find("METER").expect("METER")])
                as u16;
        let (shell_y, shell_line) = row_of(&sel_lines, "shell");
        assert!(shell_line.contains("#########---------"));
        assert_eq!(filled_meter_cells(&app, &sel_buf, &sel_lines, shell_y), 9);
        for x in meter_x..meter_x + 18 {
            let cell = &sel_buf[(x, shell_y as u16)];
            assert_ne!(
                cell.fg, cell.bg,
                "selected shell meter cell at x={x} had invisible fg == bg ({:?})",
                cell.fg
            );
        }

        // Zero `Color::Rgb` or `Color::Indexed` cells across Normal, Menu, Options, and Help.
        for mode in [
            AppMode::Normal,
            AppMode::Menu,
            AppMode::Options,
            AppMode::Help,
        ] {
            app.mode = mode;
            let (mode_lines, mode_buf) = render(&mut app);
            assert!(
                !mode_lines
                    .iter()
                    .any(|line| line.contains(['╭', '╮', '╰', '╯'])),
                "{mode:?} rendered rounded corners under force_tty"
            );
            let mut saw_ansi16 = false;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let cell = &mode_buf[(x, y)];
                    assert!(
                        !matches!(cell.fg, Color::Rgb(..) | Color::Indexed(..)),
                        "{mode:?} cell ({x},{y}) fg {:?} used non-ANSI16 colour",
                        cell.fg
                    );
                    assert!(
                        !matches!(cell.bg, Color::Rgb(..) | Color::Indexed(..)),
                        "{mode:?} cell ({x},{y}) bg {:?} used non-ANSI16 colour",
                        cell.bg
                    );
                    if cell.fg != Color::Reset {
                        saw_ansi16 = true;
                    }
                }
            }
            assert!(saw_ansi16, "{mode:?} expected ANSI-16 colours");
        }
    }

    #[test]
    fn no_color_renders_reset_across_all_modes_with_reversed_selection_and_ascii_meters() {
        use crate::ui::app::AppMode;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::layout::Rect;
        use ratatui::style::Modifier;
        use ratatui::widgets::Block;

        let mut app = synthetic_app(AggMode::Rollup, GraphSymbolSetting::Braille, None);
        // Select `shell` (50% meter fill: `#########---------`) so the selected row tests both
        // filled `#` and empty `-` meter cells under `Modifier::REVERSED`.
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.set_no_color(true);

        let assert_all_reset = |buf: &Buffer, width: u16, height: u16, ctx: &str| {
            for y in 0..height {
                for x in 0..width {
                    let cell = &buf[(x, y)];
                    assert_eq!(
                        cell.fg,
                        Color::Reset,
                        "{ctx} cell ({x},{y}) fg was {:?}",
                        cell.fg
                    );
                    assert_eq!(
                        cell.bg,
                        Color::Reset,
                        "{ctx} cell ({x},{y}) bg was {:?}",
                        cell.bg
                    );
                }
            }
        };

        // 1) Normal mode: square corners, TTY graph, `#`/`-` meters, Color::Reset everywhere,
        //    and `Modifier::REVERSED` across the entire selected row (`shell`, including meters).
        app.mode = AppMode::Normal;
        let (lines, normal_buf) = render(&mut app);
        assert_all_reset(&normal_buf, WIDTH, HEIGHT, "Normal");
        assert!(!lines.iter().any(|l| l.contains(['╭', '╮', '╰', '╯', '■'])));
        assert!(row_of(&lines, "launchd").1.contains("##################"));
        assert!(row_of(&lines, "shell").1.contains("#########---------"));
        assert!(row_of(&lines, "plugin").1.contains("------------------"));

        let (shell_y, _) = row_of(&lines, "shell");
        let shell_y = shell_y as u16;
        for x in 1..WIDTH - 1 {
            assert!(
                normal_buf[(x, shell_y)]
                    .modifier
                    .contains(Modifier::REVERSED),
                "selected row cell ({x},{shell_y}) sym={:?} missing REVERSED",
                normal_buf[(x, shell_y)].symbol()
            );
        }
        assert!(
            !normal_buf[(0, shell_y)]
                .modifier
                .contains(Modifier::REVERSED),
            "left frame border should not be REVERSED"
        );
        for other in ["launchd", "editor", "plugin", "build", "daemon", "stray"] {
            let (other_y, _) = row_of(&lines, other);
            let other_y = other_y as u16;
            for x in 0..WIDTH {
                assert!(
                    !normal_buf[(x, other_y)]
                        .modifier
                        .contains(Modifier::REVERSED),
                    "unselected row {other} at ({x},{other_y}) should not be REVERSED"
                );
            }
        }

        // 2) Menu mode: Color::Reset everywhere, selected button interior (`Options`) REVERSED,
        //    button borders and unselected buttons (`Help`, `Quit`) unreversed; short 20x8 too.
        app.mode = AppMode::Menu;
        let (_, menu_buf) = render(&mut app);
        assert_all_reset(&menu_buf, WIDTH, HEIGHT, "Menu");
        let menu_rect = super::menu_overlay_rect(Rect::new(0, 0, WIDTH, HEIGHT));
        let menu_inner = Block::bordered().inner(menu_rect);
        let first_btn_y = menu_inner.y + (menu_inner.height - (3 * 3 + 2)) / 2;
        for (idx, selected) in [(0_u16, true), (1, false), (2, false)] {
            let btn = Rect {
                x: menu_inner.x + 1,
                y: first_btn_y + idx * 4,
                width: menu_inner.width - 2,
                height: 3,
            };
            let btn_inner = Block::bordered().inner(btn);
            for x in btn_inner.x..btn_inner.x + btn_inner.width {
                assert_eq!(
                    menu_buf[(x, btn_inner.y)]
                        .modifier
                        .contains(Modifier::REVERSED),
                    selected,
                    "Menu button {idx} interior at ({x},{}) REVERSED mismatch",
                    btn_inner.y
                );
            }
            assert!(
                !menu_buf[(btn.x, btn.y)]
                    .modifier
                    .contains(Modifier::REVERSED),
                "Menu button {idx} border should not be REVERSED"
            );
        }
        let (short_menu_lines, short_menu_buf) = render_sized(&mut app, 20, 8);
        assert_all_reset(&short_menu_buf, 20, 8, "Menu 20x8");
        let opt_y = short_menu_lines
            .iter()
            .position(|l| l.contains("Options"))
            .expect("Options row") as u16;
        let opt_x = short_menu_lines[usize::from(opt_y)]
            .find("Options")
            .expect("Options col") as u16;
        assert!(short_menu_buf[(opt_x, opt_y)]
            .modifier
            .contains(Modifier::REVERSED));

        // 3) Options mode (full and 80x16 overflowing scrollbar): Color::Reset everywhere,
        //    only the selected option row inside the modal is REVERSED.
        app.mode = AppMode::Options;
        let (_, opt_buf) = render_sized(&mut app, 80, 16);
        assert_all_reset(&opt_buf, 80, 16, "Options 80x16");
        let opt_rect = super::options_overlay_rect(Rect::new(0, 0, 80, 16));
        let opt_inner = Block::bordered().inner(opt_rect);
        for y in opt_rect.y..opt_rect.y + opt_rect.height {
            for x in opt_rect.x..opt_rect.x + opt_rect.width {
                let expected_rev =
                    y == opt_inner.y && x >= opt_inner.x && x < opt_inner.x + opt_inner.width - 1;
                assert_eq!(
                    opt_buf[(x, y)].modifier.contains(Modifier::REVERSED),
                    expected_rev,
                    "Options cell ({x},{y}) REVERSED mismatch"
                );
            }
        }

        // 4) Help mode (80x20 with active scrollbar): Color::Reset across every cell.
        app.mode = AppMode::Help;
        let (_, help_buf) = render_sized(&mut app, 80, 20);
        assert_all_reset(&help_buf, 80, 20, "Help 80x20");
    }
}
