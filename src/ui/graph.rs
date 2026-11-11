//! btop-style history graphs (braille / block / tty) and `■` meters.

// Block/tty selection and parsing are consumed by the options menu (milestone 10).
#![allow(dead_code)]

use std::collections::VecDeque;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

pub const HISTORY_CAP: usize = 600;

/// Braille symbols indexed `[left][right]` (= btop `left * 5 + right`), levels 0..=4 dot-rows
/// filled from the bottom. The left dot column fills first.
pub const BRAILLE_UP: [[char; 5]; 5] = [
    [' ', '⢀', '⢠', '⢰', '⢸'],
    ['⡀', '⣀', '⣠', '⣰', '⣸'],
    ['⡄', '⣄', '⣤', '⣴', '⣼'],
    ['⡆', '⣆', '⣦', '⣶', '⣾'],
    ['⡇', '⣇', '⣧', '⣷', '⣿'],
];

/// Quadrant-block symbols indexed `[left][right]`.
pub const BLOCK_UP: [[char; 5]; 5] = [
    [' ', '▗', '▗', '▐', '▐'],
    ['▖', '▄', '▄', '▟', '▟'],
    ['▖', '▄', '▄', '▟', '▟'],
    ['▌', '▙', '▙', '█', '█'],
    ['▌', '▙', '▙', '█', '█'],
];

/// TTY symbols indexed by a single level.
pub const TTY_UP: [char; 5] = [' ', '░', '░', '▒', '█'];

/// Ring buffer of samples; `pushed` counts every sample ever pushed so cell pairing stays
/// anchored to absolute sample parity and the graph does not shimmer while scrolling.
#[derive(Debug, Clone)]
pub struct History {
    samples: VecDeque<u64>,
    cap: usize,
    pushed: u64,
}

impl History {
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            samples: VecDeque::with_capacity(cap),
            cap,
            pushed: 0,
        }
    }

    pub fn push(&mut self, value: u64) {
        if self.samples.len() == self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
        self.pushed += 1;
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.pushed = 0;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = u64> + '_ {
        self.samples.iter().copied()
    }

    /// Sample by absolute index (`None` when evicted or not yet pushed).
    fn get_abs(&self, abs: i64) -> Option<u64> {
        let pushed = self.pushed as i64;
        let first = pushed - self.samples.len() as i64;
        if abs < first || abs >= pushed {
            return None;
        }
        self.samples.get((abs - first) as usize).copied()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(HISTORY_CAP)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphSymbols {
    #[default]
    Braille,
    Block,
    Tty,
}

impl GraphSymbols {
    /// Braille by default; the Linux console cannot render it, so `TERM=linux` selects tty.
    pub fn from_term(term: Option<&str>) -> Self {
        match term {
            Some("linux") => Self::Tty,
            _ => Self::Braille,
        }
    }

    pub fn detect() -> Self {
        Self::from_term(std::env::var("TERM").ok().as_deref())
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "braille" => Some(Self::Braille),
            "block" => Some(Self::Block),
            "tty" => Some(Self::Tty),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Braille => "braille",
            Self::Block => "block",
            Self::Tty => "tty",
        }
    }

    /// Samples shown per terminal cell.
    fn samples_per_cell(self) -> usize {
        match self {
            Self::Braille => 2,
            Self::Block | Self::Tty => 1,
        }
    }

    fn glyph(self, left: usize, right: usize) -> char {
        match self {
            Self::Braille => BRAILLE_UP[left][right],
            Self::Block => BLOCK_UP[left][right],
            Self::Tty => TTY_UP[left.max(right)],
        }
    }
}

fn pct_of(value: u64, max: u64) -> f64 {
    if max == 0 {
        return 0.0;
    }
    (value as f64 / max as f64 * 100.0).clamp(0.0, 100.0)
}

/// Vertical level 0..=4 of `value` inside terminal row `row` (0 = bottom) of a `height`-row graph.
/// Follows btop: clamp to the row band, scale to 0..4 and round with a small upward bias.
pub fn row_level(value: u64, max: u64, row: usize, height: usize) -> usize {
    let height = height.max(1);
    let low = row as f64 * 100.0 / height as f64;
    let high = (row + 1) as f64 * 100.0 / height as f64;
    let modifier = if height == 1 { 0.3 } else { 0.1 };
    let cur = pct_of(value, max).clamp(low, high);
    ((cur - low) * 4.0 / (high - low) + modifier)
        .round()
        .clamp(0.0, 4.0) as usize
}

/// Single-row quantization of `value` against `max` into 0..=4.
pub fn quantize(value: u64, max: u64) -> usize {
    row_level(value, max, 0, 1)
}

/// `(left, right)` samples for each of the `cells` columns, oldest first, newest at the right edge.
fn cell_samples(
    history: &History,
    cells: usize,
    symbols: GraphSymbols,
) -> Vec<(Option<u64>, Option<u64>)> {
    let newest = history.pushed as i64 - 1;
    match symbols.samples_per_cell() {
        1 => (0..cells as i64)
            .map(|i| {
                let v = history.get_abs(newest - (cells as i64 - 1 - i));
                (v, v)
            })
            .collect(),
        _ => {
            let last_cell = newest.div_euclid(2);
            (0..cells as i64)
                .map(|i| {
                    let cell = last_cell - (cells as i64 - 1 - i);
                    let left = history.get_abs(cell * 2);
                    // A cell whose second sample is still in the future repeats its first.
                    let right = if cell * 2 + 1 > newest {
                        left
                    } else {
                        history.get_abs(cell * 2 + 1)
                    };
                    (left, right)
                })
                .collect()
        }
    }
}

/// Renders a `width` × `height` graph, newest sample at the right edge, empty history padded
/// with blanks on the left. Rows are returned top first, colored by band position.
pub fn render(
    history: &History,
    width: usize,
    height: usize,
    max: u64,
    symbols: GraphSymbols,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let cells = cell_samples(history, width, symbols);
    let mut lines = Vec::with_capacity(height);

    for row_from_top in 0..height {
        let row = height - 1 - row_from_top;
        let glyphs: Vec<(char, u64)> = cells
            .iter()
            .map(|&(left, right)| {
                let level = |v: Option<u64>| v.map_or(0, |v| row_level(v, max, row, height));
                let ch = symbols.glyph(level(left), level(right));
                (ch, left.unwrap_or(0).max(right.unwrap_or(0)))
            })
            .collect();

        if height == 1 {
            let spans: Vec<Span<'static>> = glyphs
                .iter()
                .map(|&(ch, value)| {
                    let pct = pct_of(value, max).round() as u8;
                    Span::styled(ch.to_string(), Style::default().fg(theme.usage_color(pct)))
                })
                .collect();
            lines.push(Line::from(spans));
        } else {
            let pct = (100 - row_from_top * 100 / height) as u8;
            let text: String = glyphs.iter().map(|&(ch, _)| ch).collect();
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(theme.usage_color(pct)),
            )));
        }
    }

    lines
}

/// btop-style `■` meter: filled cells colored at their gradient position, remainder `meter_bg`.
pub fn meter_spans(theme: &Theme, width: usize, value: u64, scale: u64) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let filled = crate::ui::draw::meter_fill_blocks(value, scale, width);
    let mut spans: Vec<Span<'static>> = (0..filled)
        .map(|i| {
            let pct = ((i + 1) * 100 / width) as u8;
            Span::styled("■", Style::default().fg(theme.usage_color(pct)))
        })
        .collect();
    if filled < width {
        spans.push(Span::styled(
            "■".repeat(width - filled),
            Style::default().fg(theme.color(theme.meter_bg)),
        ));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn history_of(values: &[u64]) -> History {
        let mut h = History::new(HISTORY_CAP);
        for &v in values {
            h.push(v);
        }
        h
    }

    #[test]
    fn braille_table_matches_spec() {
        assert_eq!(BRAILLE_UP[0][0], ' ');
        assert_eq!(BRAILLE_UP[4][4], '⣿');
        assert_eq!(BRAILLE_UP[4][0], '⡇');
        assert_eq!(BRAILLE_UP[0][4], '⢸');
        assert_eq!(BRAILLE_UP[1][1], '⣀');
        assert_eq!(BRAILLE_UP[3][4], '⣾');
        // Dot-row bits: left column 0x47 at level 4, right column 0xB8.
        assert_eq!(BRAILLE_UP[4][4] as u32, 0x2800 + 0xFF);
        assert_eq!(BRAILLE_UP[4][0] as u32, 0x2800 + 0x47);
        assert_eq!(BRAILLE_UP[0][4] as u32, 0x2800 + 0xB8);
    }

    #[test]
    fn quantization_edges() {
        assert_eq!(quantize(0, 100), 0);
        assert_eq!(quantize(100, 100), 4);
        assert_eq!(quantize(1_000, 100), 4);
        assert_eq!(quantize(50, 100), 2);
        assert_eq!(quantize(10, 0), 0);
    }

    #[test]
    fn multi_row_levels_fill_bottom_up() {
        // 50% of a 2-row graph fills the bottom row, leaves the top empty.
        assert_eq!(row_level(50, 100, 0, 2), 4);
        assert_eq!(row_level(50, 100, 1, 2), 0);
        assert_eq!(row_level(75, 100, 1, 2), 2);
        assert_eq!(row_level(100, 100, 1, 2), 4);
    }

    #[test]
    fn braille_cells_pair_samples_with_asymmetry() {
        let theme = Theme::new(true);
        // Samples 0,1 form one cell (left 100%, right 0%), then 2,3 (0%, 100%).
        let h = history_of(&[100, 0, 0, 100]);
        let lines = render(&h, 2, 1, 100, GraphSymbols::Braille, &theme);
        assert_eq!(text(&lines[0]), "⡇⢸");

        let full = history_of(&[100, 100]);
        assert_eq!(
            text(&render(&full, 1, 1, 100, GraphSymbols::Braille, &theme)[0]),
            "⣿"
        );

        let empty = history_of(&[0, 0]);
        assert_eq!(
            text(&render(&empty, 1, 1, 100, GraphSymbols::Braille, &theme)[0]),
            " "
        );
    }

    #[test]
    fn short_history_pads_left_and_long_history_truncates() {
        let theme = Theme::new(true);
        let h = history_of(&[100, 100]);
        let lines = render(&h, 4, 1, 100, GraphSymbols::Braille, &theme);
        assert_eq!(text(&lines[0]), "   ⣿");

        let long: Vec<u64> = (0..50).map(|i| if i % 2 == 0 { 0 } else { 100 }).collect();
        let h = history_of(&long);
        let lines = render(&h, 3, 1, 100, GraphSymbols::Braille, &theme);
        assert_eq!(text(&lines[0]).chars().count(), 3);
        assert_eq!(text(&lines[0]), "⢸⢸⢸");
    }

    #[test]
    fn odd_sample_count_repeats_trailing_sample() {
        let theme = Theme::new(true);
        let h = history_of(&[0, 0, 100]);
        let lines = render(&h, 2, 1, 100, GraphSymbols::Braille, &theme);
        assert_eq!(text(&lines[0]), " ⣿");
    }

    #[test]
    fn multi_row_graph_rows_and_colors() {
        let theme = Theme::new(true);
        let h = history_of(&[50, 50]);
        let lines = render(&h, 1, 2, 100, GraphSymbols::Braille, &theme);
        assert_eq!(lines.len(), 2);
        assert_eq!(text(&lines[0]), " ");
        assert_eq!(text(&lines[1]), "⣿");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.usage_color(100)));
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.usage_color(50)));
    }

    #[test]
    fn block_and_tty_use_one_sample_per_cell() {
        let theme = Theme::new(true);
        let h = history_of(&[0, 100, 25, 75]);
        let block = render(&h, 4, 1, 100, GraphSymbols::Block, &theme);
        assert_eq!(text(&block[0]), " █▄█");
        let tty = render(&h, 4, 1, 100, GraphSymbols::Tty, &theme);
        assert_eq!(text(&tty[0]), " █░▒");
    }

    #[test]
    fn ring_buffer_caps_and_clears() {
        let mut h = History::new(3);
        for v in 1..=5 {
            h.push(v);
        }
        assert_eq!(h.iter().collect::<Vec<_>>(), vec![3, 4, 5]);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn symbol_selection() {
        assert_eq!(GraphSymbols::from_term(Some("linux")), GraphSymbols::Tty);
        assert_eq!(
            GraphSymbols::from_term(Some("xterm-256color")),
            GraphSymbols::Braille
        );
        assert_eq!(GraphSymbols::from_term(None), GraphSymbols::Braille);
        assert_eq!(GraphSymbols::parse("Block"), Some(GraphSymbols::Block));
        assert_eq!(GraphSymbols::parse("nope"), None);
    }

    #[test]
    fn meter_fill_and_width() {
        let theme = Theme::new(true);
        let spans = meter_spans(&theme, 10, 50, 100);
        let total: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        assert_eq!(total, 10);
        assert_eq!(spans.len(), 6);
        assert!(meter_spans(&theme, 0, 1, 2).is_empty());
    }
}
