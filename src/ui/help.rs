use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::model::MemKind;
use crate::ui::theme::Theme;

const NAVIGATION_KEYS: [(&str, &str); 11] = [
    ("↑/↓, j/k", "Move selection one row (j/k need vim_keys)."),
    ("PgUp/PgDn", "Move by one page."),
    (
        "Home/End, g/G",
        "Jump to first/last row (g/G need vim_keys).",
    ),
    (
        "h, ?, F1",
        "Open Help (h always opens Help, even with vim_keys off).",
    ),
    ("o, F2", "Open Options."),
    (
        "+ / -",
        "Increase / decrease the refresh interval by 100 ms.",
    ),
    (
        "m vs btop",
        "m cycles the memory type here; btop uses m for its menu (pspp's menu is Esc).",
    ),
    (
        "q/Q, Ctrl-C, Ctrl-Q",
        "Quit pspp (Ctrl-C/Ctrl-Q from any mode).",
    ),
    ("f/F or /", "Start filter input."),
    ("c/C", "Clear the committed filter."),
    ("Esc", "Open main menu."),
];

const TREE_KEYS: [(&str, &str); 5] = [
    (
        "Space or Enter",
        "Toggle expand/collapse on selected node (also while filtering).",
    ),
    ("←", "Collapse selected node, or select parent."),
    ("→, l", "Expand selected node (l needs vim_keys)."),
    (
        "z / [",
        "Collapse all non-root subtrees (again: roots too).",
    ),
    ("Z / ]", "Expand all nodes."),
];

const DISPLAY_KEYS: [(&str, &str); 10] = [
    ("e/E", "Toggle aggregation mode: rollup/flat."),
    ("m/M", "Cycle memory type."),
    ("s", "Cycle sort key."),
    ("S or r/R", "Reverse sort direction."),
    (
        "Filter input",
        "Type any characters (including spaces) to filter live and expand match ancestors.",
    ),
    ("Backspace", "Delete one character in filter input."),
    ("Delete / Ctrl-U", "Clear the filter input."),
    ("Ctrl-W", "Delete previous word in filter input."),
    ("Enter", "Keep the filter and return to the table."),
    (
        "Esc",
        "Cancel filter edit and restore the prior filter and tree.",
    ),
];

const MENU_KEYS: [(&str, &str); 10] = [
    ("Menu: ↑/↓, j/k, Tab", "Select Options / Help / Quit."),
    ("Menu: Enter/Space", "Activate selected menu button."),
    ("Menu: Esc/q/Q", "Close menu."),
    ("Options: ↑/↓, j/k", "Select option row."),
    (
        "Options: ← / →, h / l",
        "Change/toggle selected option (h/l need vim_keys).",
    ),
    ("Options: Enter", "Edit update_ms or advance value."),
    ("Options: Esc/q/Q", "Close options and return to menu."),
    (
        "update_ms / clock_format editor",
        "Type to edit; Backspace deletes; Enter commits; Esc cancels.",
    ),
    (
        "Help: ↑/↓, j/k, PgUp/PgDn",
        "Scroll help text (j/k need vim_keys).",
    ),
    (
        "Help: Home / End, g / G",
        "Jump to top/bottom of help (g/G need vim_keys). Esc/q/Q closes.",
    ),
];

const MOUSE_ACTIONS: [(&str, &str); 8] = [
    ("Wheel up/down", "Scroll process rows."),
    ("Left click row", "Select row."),
    ("Left click [+]/[-]", "Expand/collapse node."),
    ("Double click row", "Expand/collapse node."),
    (
        "Click column header",
        "Sort by that column; click the active column again to reverse.",
    ),
    (
        "Menu: click / wheel",
        "Click button to activate; wheel cycles menu buttons; click outside to close menu.",
    ),
    (
        "Options: click / wheel",
        "Click row to select, click the selected row to edit/advance; wheel moves; click outside closes.",
    ),
    (
        "Help: wheel / click",
        "Wheel scrolls; click outside closes.",
    ),
];

/// Titled key/mouse tables, in display order. Shared by the Help overlay, README and man page.
pub fn key_sections() -> [(&'static str, &'static [(&'static str, &'static str)]); 5] {
    [
        ("Navigation", &NAVIGATION_KEYS),
        ("Tree", &TREE_KEYS),
        ("Display & Filter", &DISPLAY_KEYS),
        ("Menu / Overlays", &MENU_KEYS),
        ("Mouse", &MOUSE_ACTIONS),
    ]
}

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

    let [navigation, tree, display, menu, mouse] = key_sections();
    push_section_header(&mut lines, theme, "key bindings");
    for (title, keys) in [navigation, tree, display, menu] {
        push_subheader(&mut lines, theme, &title.to_lowercase());
        push_key_rows(&mut lines, theme, keys, key_width, width);
    }

    push_section_header(&mut lines, theme, "mouse actions");
    push_key_rows(&mut lines, theme, mouse.1, key_width, width);

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
                let pad = key_width.saturating_sub(UnicodeWidthStr::width(*key));
                format!("{key}{}", " ".repeat(pad))
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

pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0_usize;

    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
            continue;
        }

        let next_len = current_width.saturating_add(1).saturating_add(word_width);
        if next_len <= width {
            current.push(' ');
            current.push_str(word);
            current_width = next_len;
        } else {
            lines.push(current);
            current = word.to_owned();
            current_width = word_width;
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
    use super::{clamp_scroll_offset, key_sections, memory_help_text};
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

    // ---- generated documentation blocks ------------------------------------------------

    fn markdown_escape(text: &str) -> String {
        text.replace('|', "\\|")
    }

    /// README key tables: one `####` heading and table per section (mouse gets its own `###`).
    fn render_markdown_key_tables() -> String {
        let mut out = String::new();
        for (title, rows) in key_sections() {
            if title == "Mouse" {
                out.push_str("### Mouse Support\n\n| Mouse Action | Effect |\n|---|---|\n");
            } else {
                out.push_str(&format!("#### {title}\n\n| Key | Action |\n|---|---|\n"));
            }
            for (keys, action) in rows {
                out.push_str(&format!(
                    "| `{}` | {} |\n",
                    markdown_escape(keys),
                    markdown_escape(action)
                ));
            }
            out.push('\n');
        }
        out.trim_end().to_owned()
    }

    fn roff_escape(text: &str) -> String {
        let mut out = String::new();
        for ch in text.chars() {
            match ch {
                '\\' => out.push_str("\\e"),
                '-' => out.push_str("\\-"),
                '\u{2191}' => out.push_str("Up"),
                '\u{2193}' => out.push_str("Down"),
                '\u{2190}' => out.push_str("Left"),
                '\u{2192}' => out.push_str("Right"),
                '\u{3a3}' => out.push_str("\\(*S"),
                other => out.push(other),
            }
        }
        // A leading `.` or `'` would be read as a roff request.
        if out.starts_with(['.', '\'']) {
            out.insert_str(0, "\\&");
        }
        out
    }

    /// Greedy word wrap so generated roff stays within man-page style limits.
    fn wrap_words(text: &str, width: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for word in text.split_whitespace() {
            match lines.last_mut() {
                Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                    line.push(' ');
                    line.push_str(word);
                }
                _ => lines.push(word.to_owned()),
            }
        }
        lines
    }

    /// Man-page `.SS`/`.TP` entries for every table.
    fn render_roff_key_bindings() -> String {
        let mut out = String::new();
        for (title, rows) in key_sections() {
            let heading = match title {
                "Mouse" => "Mouse Actions",
                other => other,
            };
            out.push_str(&format!(".SS {}\n", roff_escape(heading)));
            for (keys, action) in rows {
                out.push_str(&format!(
                    ".TP\n.B {}\n{}\n",
                    roff_escape(keys),
                    wrap_words(&roff_escape(action), 72).join("\n")
                ));
            }
        }
        out.trim_end().to_owned()
    }

    #[test]
    fn readme_key_tables_match_the_help_tables() {
        use crate::docsync::{markdown_markers, sync_block};
        sync_block(
            "README.md",
            markdown_markers("key-tables"),
            &render_markdown_key_tables(),
        );
    }

    #[test]
    fn man_page_key_bindings_match_the_help_tables() {
        use crate::docsync::{roff_markers, sync_block};
        sync_block(
            "doc/pspp.1",
            roff_markers("key-bindings"),
            &render_roff_key_bindings(),
        );
    }

    #[test]
    fn generators_escape_markup() {
        assert_eq!(markdown_escape("a|b"), "a\\|b");
        assert_eq!(
            roff_escape("Left/Right - \u{2191}/\u{2193} \u{3a3}"),
            "Left/Right \\- Up/Down \\(*S"
        );
        assert_eq!(roff_escape(".hidden"), "\\&.hidden");
        assert_eq!(roff_escape("a\\b"), "a\\eb");
    }

    #[test]
    fn every_key_table_row_is_documented_text() {
        for (title, rows) in key_sections() {
            assert!(!rows.is_empty(), "{title}");
            for (keys, action) in rows {
                assert!(
                    !keys.trim().is_empty() && !action.trim().is_empty(),
                    "{title}"
                );
            }
        }
    }

    #[test]
    fn wrap_text_and_key_rows_measure_unicode_display_width() {
        use unicode_width::UnicodeWidthStr;

        // Each CJK char is 2 display columns: "日本 語文" is 4 + 1 + 4 = 9 columns (not 5).
        assert_eq!(super::wrap_text("日本 語文", 8), vec!["日本", "語文"]);
        assert_eq!(super::wrap_text("日本 語文", 9), vec!["日本 語文"]);

        let theme = crate::ui::theme::Theme::new(true);
        let mut lines = Vec::new();
        super::push_key_rows(&mut lines, &theme, &[("日本語", "desc")], 10, 40);
        assert_eq!(
            UnicodeWidthStr::width(lines[0].spans[0].content.as_ref()),
            10
        );
    }
}
