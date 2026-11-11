use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::model::MemKind;
use crate::ui::theme::Theme;

const NAVIGATION_KEYS: [(&str, &str); 7] = [
    ("↑/↓, j/k", "Move selection one row."),
    ("PgUp/PgDn", "Move by one page."),
    ("Home/End, g/G", "Jump to first/last row."),
    (
        "q/Q, Ctrl-C, Ctrl-Q",
        "Quit pspp (Ctrl-C/Ctrl-Q from any mode).",
    ),
    ("f/F or /", "Start filter input."),
    ("c/C", "Clear committed filter."),
    ("Esc", "Open main menu."),
];

const TREE_KEYS: [(&str, &str); 3] = [
    ("Space or Enter", "Toggle expand/collapse on selected node."),
    ("←", "Collapse selected node, or select parent."),
    ("→", "Expand selected node."),
];

const DISPLAY_KEYS: [(&str, &str); 7] = [
    ("e/E", "Toggle aggregation mode: rollup/flat."),
    ("m/M", "Cycle memory type."),
    ("s", "Cycle sort key."),
    ("S or r/R", "Reverse sort direction."),
    ("Filter input", "Type any characters (including spaces)."),
    ("Backspace", "Delete one character in filter input."),
    ("Enter/Esc", "Commit filter / clear and exit filter input."),
];

const MENU_KEYS: [(&str, &str); 9] = [
    ("Menu: ↑/↓, j/k, Tab", "Select Options / Help / Quit."),
    ("Menu: Enter/Space", "Activate selected menu button."),
    ("Menu: Esc/q/Q", "Close menu."),
    ("Options: ↑/↓, j/k", "Select option row."),
    ("Options: ←/→", "Change/toggle selected option."),
    ("Options: Enter", "Edit update_ms or advance value."),
    ("Options: Esc/q/Q", "Close options and return to menu."),
    (
        "update_ms editor",
        "Digits append; Backspace deletes; Enter commits; Esc cancels.",
    ),
    (
        "Help: ↑/↓/j/k/PgUp/PgDn",
        "Scroll help text. Esc/q/Q closes.",
    ),
];

const MOUSE_ACTIONS: [(&str, &str); 5] = [
    ("Wheel up/down", "Scroll process rows."),
    ("Left click row", "Select row."),
    ("Left click [+]/[-]", "Expand/collapse node."),
    ("Double click row", "Expand/collapse node."),
    (
        "Menu click",
        "Click button to activate; click outside to close menu.",
    ),
];

pub fn memory_help_text(kind: MemKind) -> &'static str {
    match kind {
        MemKind::Rss => {
            "RSS is RAM the process is using right now, including shared memory also used by \
             others. Available on macOS and Linux. Good quick signal, but shared pages can make \
             per-process usage look larger than what is uniquely owned."
        }
        MemKind::Vsz => {
            "VSZ is all address space the process has reserved. Most of it is not real RAM, so \
             it often looks scary but is mostly harmless. Available on macOS and Linux. Treat it \
             as a rough reservation indicator, not true RAM pressure."
        }
        MemKind::Footprint => {
            "Footprint is Apple’s own \"how heavy is this process\" memory number (like Activity \
             Monitor’s Memory column). Available on macOS, unavailable on Linux. If unavailable, \
             pspp marks values approximate (~)."
        }
        MemKind::Pss => {
            "PSS splits shared pages fairly between sharing processes, making it the best answer \
             to \"who is really using RAM?\" Available on Linux via smaps_rollup, unavailable on \
             macOS. Missing permissions/data are shown as approximate (~)."
        }
        MemKind::Uss => {
            "USS is memory only this process uses — what you usually get back if you kill it. \
             Available on Linux via smaps_rollup, unavailable on macOS. Missing permissions/data \
             are shown as approximate (~)."
        }
        MemKind::Swap => {
            "Swap is memory moved out of RAM onto disk because RAM was full. Available on Linux \
             via smaps_rollup, unavailable on macOS. Missing permissions/data are shown as \
             approximate (~)."
        }
    }
}

pub fn build_help_lines(theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let key_width = width.saturating_div(3).clamp(14, 30);

    push_section_header(&mut lines, theme, "key bindings");
    push_subheader(&mut lines, theme, "navigation");
    push_key_rows(&mut lines, theme, &NAVIGATION_KEYS, key_width, width);
    push_subheader(&mut lines, theme, "tree");
    push_key_rows(&mut lines, theme, &TREE_KEYS, key_width, width);
    push_subheader(&mut lines, theme, "display");
    push_key_rows(&mut lines, theme, &DISPLAY_KEYS, key_width, width);
    push_subheader(&mut lines, theme, "menu / overlays");
    push_key_rows(&mut lines, theme, &MENU_KEYS, key_width, width);

    push_section_header(&mut lines, theme, "mouse actions");
    push_key_rows(&mut lines, theme, &MOUSE_ACTIONS, key_width, width);

    push_section_header(&mut lines, theme, "memory types explained");
    for kind in [
        MemKind::Rss,
        MemKind::Vsz,
        MemKind::Footprint,
        MemKind::Pss,
        MemKind::Uss,
        MemKind::Swap,
    ] {
        push_subheader(&mut lines, theme, &kind.label().to_ascii_lowercase());
        push_wrapped_paragraph(
            &mut lines,
            theme,
            memory_help_text(kind),
            width,
            theme.main_fg,
        );
    }

    push_section_header(&mut lines, theme, "why totals can exceed RAM");
    push_wrapped_paragraph(
        &mut lines,
        theme,
        "The graph and the `used X / Y` figure show real system memory in use. The `Σ` figure \
         adds up one number per process, and the same shared page (libraries, frameworks) is \
         counted once for every process that maps it, so Σ RSS can be larger than the RAM you \
         have. On macOS, footprint also counts compressed memory, which no longer occupies its \
         full size in RAM. The % column is a row's Σ divided by total RAM and can exceed 100%.",
        width,
        theme.main_fg,
    );

    push_section_header(&mut lines, theme, "aggregation modes");
    push_wrapped_paragraph(
        &mut lines,
        theme,
        "Rollup adds a process and all descendants, so parents show full subtree weight. Flat \
         adds only direct children, so parent and child layers stay separate.",
        width,
        theme.main_fg,
    );
    push_wrapped_paragraph(
        &mut lines,
        theme,
        "A trailing ~ means pspp had to approximate one or more values for the selected memory \
         type.",
        width,
        theme.inactive_fg,
    );

    lines
}

pub fn clamp_scroll_offset(offset: usize, content_lines: usize, viewport_lines: usize) -> usize {
    if content_lines == 0 {
        return 0;
    }
    let viewport = viewport_lines.max(1);
    let max_scroll = content_lines.saturating_sub(viewport);
    offset.min(max_scroll)
}

fn push_section_header(lines: &mut Vec<Line<'static>>, theme: &Theme, title: &str) {
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled(
        title.to_ascii_uppercase(),
        Style::default().fg(theme.color(theme.title)),
    )));
}

fn push_subheader(lines: &mut Vec<Line<'static>>, theme: &Theme, title: &str) {
    lines.push(Line::from(Span::styled(
        format!("{title}:"),
        Style::default().fg(theme.color(theme.hi_fg)),
    )));
}

fn push_key_rows(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    rows: &[(&str, &str)],
    key_width: usize,
    width: usize,
) {
    let desc_width = width.saturating_sub(key_width + 1).max(20);
    for (key, description) in rows {
        let wrapped = wrap_text(description, desc_width);
        for (idx, part) in wrapped.iter().enumerate() {
            let left = if idx == 0 {
                format!("{key:<key_width$}")
            } else {
                " ".repeat(key_width)
            };
            lines.push(Line::from(vec![
                Span::styled(left, Style::default().fg(theme.color(theme.hi_fg))),
                Span::styled(
                    format!(" {part}"),
                    Style::default().fg(theme.color(theme.main_fg)),
                ),
            ]));
        }
    }
}

fn push_wrapped_paragraph(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    text: &str,
    width: usize,
    color: crate::ui::theme::Rgb,
) {
    for line in wrap_text(text, width.max(20)) {
        lines.push(Line::from(Span::styled(
            line,
            Style::default().fg(theme.color(color)),
        )));
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
            continue;
        }

        let next_len = current
            .chars()
            .count()
            .saturating_add(1 + word.chars().count());
        if next_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_owned();
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{clamp_scroll_offset, memory_help_text};
    use crate::model::MemKind;

    #[test]
    fn every_mem_kind_has_help_text() {
        for kind in [
            MemKind::Rss,
            MemKind::Vsz,
            MemKind::Footprint,
            MemKind::Pss,
            MemKind::Uss,
            MemKind::Swap,
        ] {
            assert!(!memory_help_text(kind).trim().is_empty());
        }
    }

    #[test]
    fn scroll_clamp_keeps_offset_in_bounds() {
        assert_eq!(clamp_scroll_offset(0, 0, 5), 0);
        assert_eq!(clamp_scroll_offset(10, 3, 5), 0);
        assert_eq!(clamp_scroll_offset(0, 30, 10), 0);
        assert_eq!(clamp_scroll_offset(100, 30, 10), 20);
        assert_eq!(clamp_scroll_offset(7, 30, 10), 7);
    }
}
