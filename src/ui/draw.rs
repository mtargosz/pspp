use std::ops::Range;

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::model::SortBy;
use crate::query::{format_pct, format_size};
use crate::ui::app::{App, AppMode, DisplayRow};
use crate::ui::graph;
use crate::ui::help;

const PCT_COL_WIDTH: u16 = 6;
const COMM_MIN_WIDTH: u16 = 12;
const OWN_COL_WIDTH: u16 = 10;
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

    let used_pct = percentage(app.mem_used(), app.tree.total_mem());
    let status = Line::from(
        vec![
            Span::styled(
                format!("{} procs  ", app.tree.proc_count()),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled("mem ", fg(theme.inactive_fg)),
            Span::styled(
                app.mem_kind.label().to_ascii_uppercase(),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!("  agg {}", app.agg_mode),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!("  sort {}{}", app.sort_by, sort_dir_label(app.sort_desc)),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!("  update {}ms", app.update_ms),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!(
                    "  used {} / {}",
                    format_size(app.mem_used(), app.base10),
                    format_size(app.tree.total_mem(), app.base10)
                ),
                Style::default().fg(theme.usage_color(used_pct)),
            ),
        ]
        .into_iter()
        .chain(app.filter_indicator().into_iter().map(|text| {
            Span::styled(
                format!("  {text}"),
                Style::default().fg(theme.color(theme.hi_fg)),
            )
        }))
        .collect::<Vec<_>>(),
    );
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
        if approx {
            format!("{} Σ~", app.mem_kind.label().to_ascii_uppercase())
        } else {
            format!("{} Σ", app.mem_kind.label().to_ascii_uppercase())
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
    let max_text = format!(" / {} ", format_size(max, app.base10));
    let pct_text = format!(" {pct}%");
    let sum_text = format!(
        "  Σ {} {}",
        app.mem_kind.label().to_ascii_uppercase(),
        format_size(app.mem_kind_sum(), app.base10)
    );
    let fixed = label.chars().count()
        + used_text.chars().count()
        + max_text.chars().count()
        + pct_text.chars().count()
        + sum_text.chars().count();
    let meter_width = width.saturating_sub(fixed);

    let mut spans = vec![
        Span::styled(label, Style::default().fg(theme.color(theme.inactive_fg))),
        Span::styled(used_text, Style::default().fg(theme.usage_color(pct))),
        Span::styled(max_text, Style::default().fg(theme.color(theme.main_fg))),
    ];
    spans.extend(graph::meter_spans(theme, meter_width, used, max));
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
    let style = Style::default().fg(theme.color(theme.title));

    frame.render_widget(Paragraph::new("COMM").style(style), columns[0]);
    frame.render_widget(
        Paragraph::new("PID")
            .style(style)
            .alignment(Alignment::Right),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new("OWN")
            .style(style)
            .alignment(Alignment::Right),
        columns[2],
    );
    frame.render_widget(
        Paragraph::new(agg_header)
            .style(style)
            .alignment(Alignment::Right),
        columns[3],
    );
    frame.render_widget(
        Paragraph::new("%").style(style).alignment(Alignment::Right),
        columns[4],
    );
    frame.render_widget(
        Paragraph::new("CHILDREN")
            .style(style)
            .alignment(Alignment::Right),
        columns[5],
    );
    frame.render_widget(Paragraph::new("METER").style(style), columns[6]);
}

fn render_body(
    frame: &mut Frame<'_>,
    body_area: Rect,
    columns: &[Rect; 7],
    app: &App,
) -> (usize, Vec<Option<Range<u16>>>) {
    let rows = app.rows();
    let start = app.scroll();
    let end = (start + usize::from(body_area.height)).min(rows.len());
    let max_visible_agg = rows.iter().map(|row| row.aggregated).max().unwrap_or(0);

    let mut toggle_zones = Vec::with_capacity(usize::from(body_area.height));

    for (line_idx, row_idx) in (start..end).enumerate() {
        let row_rect = Rect {
            x: body_area.x,
            y: body_area.y + u16::try_from(line_idx).unwrap_or(u16::MAX),
            width: body_area.width,
            height: 1,
        };

        let row = &rows[row_idx];
        let selected = row_idx == app.cursor();
        if selected {
            frame.render_widget(
                Block::default().style(
                    Style::default()
                        .bg(app.theme.color(app.theme.selected_bg))
                        .fg(app.theme.color(app.theme.selected_fg)),
                ),
                row_rect,
            );
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
    let selected_style = Style::default()
        .fg(theme.color(theme.selected_fg))
        .bg(theme.color(theme.selected_bg));

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
    let meter_pct = percentage(row.aggregated, max_visible_agg);

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
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(row.tree_prefix.clone(), prefix_style),
            Span::styled(row.comm.clone(), comm_style),
        ]))
        .alignment(Alignment::Left),
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

    let meter_line = meter_line(
        meter_width,
        row.aggregated,
        max_visible_agg,
        app,
        selected,
        meter_pct,
    );
    frame.render_widget(Paragraph::new(meter_line), columns[6]);
}

fn meter_line(
    width: usize,
    value: u64,
    scale: u64,
    app: &App,
    selected: bool,
    meter_pct: u8,
) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let theme = &app.theme;
    let selected_style = Style::default()
        .fg(theme.color(theme.selected_fg))
        .bg(theme.color(theme.selected_bg));

    if selected {
        return Line::from(Span::styled("■".repeat(width), selected_style));
    }

    let filled = meter_fill_blocks(value, scale, width);
    let empty = width.saturating_sub(filled);
    let filled_style = Style::default().fg(theme.usage_color(meter_pct));
    let empty_style = Style::default().fg(theme.color(theme.meter_bg));

    Line::from(vec![
        Span::styled("■".repeat(filled), filled_style),
        Span::styled("■".repeat(empty), empty_style),
    ])
}

/// Key hints for the current mode as `(keys, meaning)` segments, most important first.
fn hint_segments(mode: AppMode) -> &'static [(&'static str, &'static str)] {
    match mode {
        AppMode::Normal => &[
            ("↑/↓ j/k PgUp/PgDn", ""),
            ("←/→", ""),
            ("Space/Enter", ""),
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
        let seg_len = keys.chars().count()
            + if meaning.is_empty() {
                0
            } else {
                1 + meaning.chars().count()
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

fn render_menu_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).clamp(24, 44);
    let height = area.height.saturating_sub(4).clamp(13, 17);
    let overlay = centered_rect(area, width, height);

    let theme = &app.theme;
    let block = Block::bordered()
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme.color(theme.div_line)))
        .title(Span::styled(
            " menu ",
            Style::default().fg(theme.color(theme.title)),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

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
        frame.render_widget(button_block, rect);

        let label = Paragraph::new(button.label())
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.color(border_color)));
        frame.render_widget(label, rect);
    }

    app.set_menu_hit_test(overlay, button_rects);
}

fn render_options_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).clamp(40, 78);
    let height = area.height.saturating_sub(4).clamp(12, 22);
    let overlay = centered_rect(area, width, height);

    let theme = app.theme.clone();
    let block = Block::bordered()
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme.color(theme.div_line)))
        .title(Span::styled(
            " options ",
            Style::default().fg(theme.color(theme.title)),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let sections = Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).split(inner);
    let list_area = sections[0];
    let bottom = sections[1];

    let label_width = 20;
    let mut row_rects = Vec::new();
    let visible_rows = usize::from(list_area.height);
    let first = app.option_scroll_for(visible_rows);
    for (line_idx, idx) in (first..App::option_keys().len())
        .take(visible_rows)
        .enumerate()
    {
        let key = App::option_keys()[idx];
        let row_rect = Rect {
            x: list_area.x,
            y: list_area.y + u16::try_from(line_idx).unwrap_or(u16::MAX),
            width: list_area.width,
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
            Style::default()
                .fg(theme.color(theme.selected_fg))
                .bg(theme.color(theme.selected_bg))
        } else {
            Style::default().fg(theme.color(theme.main_fg))
        };

        frame.render_widget(Paragraph::new(line).style(style), row_rect);
        row_rects.push((row_rect, idx));
    }
    if App::option_keys().len() > visible_rows && list_area.width > 0 {
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
                .style(Style::default().fg(theme.color(theme.div_line)))
                .thumb_style(Style::default().fg(theme.color(theme.hi_fg))),
            list_area,
            &mut bar_state,
        );
    }
    app.set_options_hit_test(overlay, row_rects);

    let desc_area = Rect {
        x: bottom.x,
        y: bottom.y,
        width: bottom.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(app.selected_option_description()).style(
            Style::default()
                .fg(theme.color(theme.main_fg))
                .bg(theme.color(theme.selected_bg)),
        ),
        desc_area,
    );

    let hint_area = Rect {
        x: bottom.x,
        y: bottom.y.saturating_add(1),
        width: bottom.width,
        height: bottom.height.saturating_sub(1),
    };
    frame.render_widget(
        Paragraph::new("↑/↓ select  ←/→ change  Enter edit  Esc/q back")
            .style(Style::default().fg(theme.color(theme.inactive_fg))),
        hint_area,
    );
}

fn render_help_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let max_width = area.width.saturating_sub(2).max(24);
    let max_height = area.height.saturating_sub(2).max(8);
    let width = area.width.saturating_sub(6).clamp(24, max_width);
    let height = area.height.saturating_sub(4).clamp(8, max_height);
    let overlay = centered_rect(area, width, height);

    let theme = app.theme.clone();
    let block = Block::bordered()
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme.color(theme.div_line)))
        .title(Span::styled(
            " help ",
            Style::default().fg(theme.color(theme.title)),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.width == 0 || inner.height == 0 {
        app.set_help_scroll_metrics(0, 1);
        return;
    }

    app.set_help_hit_test(overlay);

    let sections = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let footer = sections[1];
    // The last column is reserved for the scrollbar so text never runs underneath it.
    let columns =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(1)]).split(sections[0]);
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
                .style(Style::default().fg(theme.color(theme.div_line)))
                .thumb_style(Style::default().fg(theme.color(theme.hi_fg))),
            bar_area,
            &mut bar_state,
        );
    }

    let footer_text = Paragraph::new("↑/↓ j/k PgUp/PgDn scroll  Esc/q back")
        .style(Style::default().fg(theme.color(theme.inactive_fg)));
    frame.render_widget(footer_text, footer);
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
    let agg_len = u16::try_from(agg_header.chars().count())
        .unwrap_or(10)
        .max(10);
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
                    mem: MemSet::default(),
                },
                ProcRec {
                    pid: 4_194_303,
                    ppid: 1,
                    comm: "worker".into(),
                    mem: MemSet::default(),
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
                    rss: 1000 * u64::try_from(pid).unwrap_or(1),
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
                rss: mib * MIB,
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
        // Column (in cells) of the METER title; every glyph in these lines is single-width.
        let meter_x = header
            .find("METER")
            .map(|byte| header[..byte].chars().count())
            .unwrap_or(0);
        let empty = app.theme.color(app.theme.meter_bg);
        (meter_x..meter_x + 18)
            .filter(|x| {
                let cell = &buffer[(
                    u16::try_from(*x).unwrap_or(0),
                    u16::try_from(y).unwrap_or(0),
                )];
                cell.symbol() == "■" && cell.fg != empty
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
                lines.iter().any(|l| l.contains("FOOTPRINT Σ~")),
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
        assert!(body("plugin").starts_with("│  │  └─ plugin"));
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
}
