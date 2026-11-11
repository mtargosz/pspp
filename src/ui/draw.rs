use std::ops::Range;

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::query::format_size;
use crate::ui::app::{App, AppMode, DisplayRow};
use crate::ui::graph;
use crate::ui::help;

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
        .border_type(BorderType::Rounded)
        .border_style(fg(theme.div_line))
        .title(Span::styled(" pspp ", fg(theme.title)));
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

    let total_used = app.mem_total_used();
    let total_pct = percentage(total_used, app.snapshot.total_mem);
    let status = Line::from(
        vec![
            Span::styled(
                format!("{} procs  ", app.snapshot.procs.len()),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled("mem ", fg(theme.inactive_fg)),
            Span::styled(
                app.mem_kind.label().to_ascii_uppercase(),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!("  agg {}", agg_mode_label(app.agg_mode)),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!(
                    "  sort {}{}",
                    sort_label(app.sort_by),
                    sort_dir_label(app.sort_desc)
                ),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!("  update {}ms", app.update_ms),
                Style::default().fg(theme.color(theme.main_fg)),
            ),
            Span::styled(
                format!(
                    "  total {}",
                    format_size(app.snapshot.total_mem, app.base10)
                ),
                Style::default().fg(theme.usage_color(total_pct)),
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

    let col_layout = build_column_layout(header_area, &agg_header);
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
    render_hint(frame, hint_area, app);
}

fn draw_overlay(frame: &mut Frame<'_>, app: &mut App) {
    match app.mode {
        AppMode::Normal | AppMode::FilterInput => app.clear_menu_hit_test(),
        AppMode::Menu => render_menu_overlay(frame, app),
        AppMode::Options => {
            app.clear_menu_hit_test();
            render_options_overlay(frame, app);
        }
        AppMode::Help => {
            app.clear_menu_hit_test();
            render_help_overlay(frame, app);
        }
    }
}

fn render_mem_box(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
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
    let used = app.mem_total_used();
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

    let pct = percentage(used, max);
    let label = format!("{} total ", app.mem_kind.label().to_ascii_uppercase());
    let used_text = format_size(used, app.base10);
    let max_text = format!(" / {} ", format_size(max, app.base10));
    let pct_text = format!(" {pct}%");
    let fixed = label.chars().count()
        + used_text.chars().count()
        + max_text.chars().count()
        + pct_text.chars().count();
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

    let summary_area = Rect {
        y: inner.y + inner.height - 1,
        height: 1,
        ..inner
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), summary_area);
}

fn render_header(frame: &mut Frame<'_>, columns: &[Rect; 6], agg_header: &str, app: &App) {
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
        Paragraph::new("CHILDREN")
            .style(style)
            .alignment(Alignment::Right),
        columns[4],
    );
    frame.render_widget(Paragraph::new("METER").style(style), columns[5]);
}

fn render_body(
    frame: &mut Frame<'_>,
    body_area: Rect,
    columns: &[Rect; 6],
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
    columns: &[Rect; 6],
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
    let children_text = row.child_count.to_string();
    let meter_width = usize::from(columns[5].width);

    let own_pct = percentage(row.own, app.snapshot.total_mem);
    let agg_pct = percentage(row.aggregated, app.snapshot.total_mem);
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
        Paragraph::new(children_text)
            .style(if selected {
                selected_style
            } else {
                default_style
            })
            .alignment(Alignment::Right),
        columns[4],
    );

    let meter_line = meter_line(
        meter_width,
        row.aggregated,
        max_visible_agg,
        app,
        selected,
        meter_pct,
    );
    frame.render_widget(Paragraph::new(meter_line), columns[5]);
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

fn render_hint(frame: &mut Frame<'_>, hint_area: Rect, app: &App) {
    let theme = &app.theme;
    let hint = Line::from(vec![
        Span::styled(
            "↑/↓ j/k PgUp/PgDn Home/End g/G",
            Style::default().fg(theme.color(theme.main_fg)),
        ),
        Span::styled("  ←/→", Style::default().fg(theme.color(theme.main_fg))),
        Span::styled(
            "  Space/Enter",
            Style::default().fg(theme.color(theme.main_fg)),
        ),
        Span::styled(
            "  e agg  m mem  s sort  S/r rev",
            Style::default().fg(theme.color(theme.main_fg)),
        ),
        Span::styled(
            "  f// filter",
            Style::default().fg(theme.color(theme.main_fg)),
        ),
        Span::styled(
            "  Esc menu",
            Style::default().fg(theme.color(theme.main_fg)),
        ),
        Span::styled("  q quit", Style::default().fg(theme.color(theme.hi_fg))),
    ]);
    frame.render_widget(Paragraph::new(hint), hint_area);
}

fn render_menu_overlay(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).clamp(24, 44);
    let height = area.height.saturating_sub(4).clamp(13, 17);
    let overlay = centered_rect(area, width, height);

    let theme = &app.theme;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
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
    let height = area.height.saturating_sub(4).clamp(12, 18);
    let overlay = centered_rect(area, width, height);

    let theme = &app.theme;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
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
    for (idx, key) in App::option_keys().iter().enumerate() {
        if idx >= usize::from(list_area.height) {
            break;
        }

        let row_rect = Rect {
            x: list_area.x,
            y: list_area.y + u16::try_from(idx).unwrap_or(u16::MAX),
            width: list_area.width,
            height: 1,
        };

        let selected = idx == app.option_selected_index();
        let line = format!(
            "{:<label_width$} {}",
            key.label(),
            app.option_value(*key),
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
    }

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
        .border_type(BorderType::Rounded)
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

    let sections = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let body = sections[0];
    let footer = sections[1];
    let lines = help::build_help_lines(&theme, usize::from(body.width));
    app.set_help_scroll_metrics(lines.len(), usize::from(body.height));

    let scroll = u16::try_from(app.help_scroll()).unwrap_or(u16::MAX);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);

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

fn build_column_layout(area: Rect, agg_header: &str) -> Vec<Rect> {
    let agg_len = agg_header.chars().count().max(10);
    Layout::horizontal([
        Constraint::Min(12),
        Constraint::Length(1),
        Constraint::Length(6),
        Constraint::Length(1),
        Constraint::Length(10),
        Constraint::Length(1),
        Constraint::Length(u16::try_from(agg_len).unwrap_or(10)),
        Constraint::Length(1),
        Constraint::Length(8),
        Constraint::Length(1),
        Constraint::Length(12),
    ])
    .split(area)
    .to_vec()
}

fn data_column_rects(columns: &[Rect]) -> [Rect; 6] {
    [
        columns[0],
        columns[2],
        columns[4],
        columns[6],
        columns[8],
        columns[10],
    ]
}

fn agg_mode_label(mode: crate::model::AggMode) -> &'static str {
    match mode {
        crate::model::AggMode::Rollup => "rollup",
        crate::model::AggMode::Flat => "flat",
    }
}

fn sort_label(sort_by: crate::model::SortBy) -> &'static str {
    match sort_by {
        crate::model::SortBy::Aggregated => "agg",
        crate::model::SortBy::Own => "own",
        crate::model::SortBy::Pid => "pid",
        crate::model::SortBy::Comm => "comm",
    }
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
    use super::{draw, meter_fill_blocks};
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
        assert!(tall.contains(" total "));
        assert!(tall.contains('%'));
        assert!(tall.contains("COMM"));
        assert!(tall.lines().filter(|line| line.contains("MiB")).count() > 3);

        let short = render_to_string(100, 10);
        assert!(!short.contains("╭ mem "));
        assert!(short.contains("COMM"));
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
}
