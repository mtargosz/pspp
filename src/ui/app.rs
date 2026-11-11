use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::collect::{self, Snapshot};
use crate::model::{AggMode, AggregateRow, MemKind, ProcTree, SortBy, VisibleRow};
use crate::ui::graph::{GraphSymbols, History};
use crate::ui::theme::Theme;

pub const DEFAULT_UPDATE_MS: u64 = 2_000;
pub const MIN_UPDATE_MS: u64 = 100;

const MOUSE_SCROLL_STEP: isize = 3;
const DOUBLE_CLICK_MS: u64 = 350;
const SORT_ORDER: [SortBy; 4] = [SortBy::Aggregated, SortBy::Own, SortBy::Pid, SortBy::Comm];

pub fn clamp_update_ms(update_ms: u64) -> u64 {
    update_ms.max(MIN_UPDATE_MS)
}

pub fn next_tick_deadline(last_tick_at: Instant, update_ms: u64) -> Instant {
    last_tick_at + Duration::from_millis(clamp_update_ms(update_ms))
}

pub fn remaining_until_tick(now: Instant, next_tick_at: Instant) -> Duration {
    next_tick_at.saturating_duration_since(now)
}

#[derive(Clone, Debug)]
pub struct DisplayRow {
    pub pid: i32,
    pub comm: String,
    pub own: u64,
    pub own_approximate: bool,
    pub aggregated: u64,
    pub aggregated_approximate: bool,
    pub child_count: usize,
    pub tree_prefix: String,
    pub toggle_marker: Option<Range<usize>>,
}

pub struct App {
    pub snapshot: Snapshot,
    pub tree: ProcTree,
    pub agg_mode: AggMode,
    pub mem_kind: MemKind,
    pub sort_by: SortBy,
    pub sort_desc: bool,
    pub update_ms: u64,
    pub theme: Theme,
    pub mem_history: History,
    pub graph_symbols: GraphSymbols,
    pub should_quit: bool,
    pub last_tick_at: Instant,
    pub next_tick_at: Instant,
    pub last_mouse_position: Option<(u16, u16)>,

    expanded: HashSet<i32>,
    rows: Vec<DisplayRow>,
    cursor: usize,
    scroll: usize,
    selected_pid: Option<i32>,
    viewport_height: usize,

    table_body_rect: Option<Rect>,
    comm_col_x: u16,
    rendered_start: usize,
    rendered_toggle_zones: Vec<Option<Range<u16>>>,
    last_click: Option<(usize, Instant)>,
}

impl App {
    pub fn new(update_ms: u64) -> Self {
        let update_ms = clamp_update_ms(update_ms);
        let snapshot = collect::take_snapshot();
        let tree = ProcTree::build(&snapshot);
        let now = Instant::now();
        let expanded: HashSet<i32> = snapshot.procs.iter().map(|proc_rec| proc_rec.pid).collect();

        let mut app = Self {
            snapshot,
            tree,
            agg_mode: AggMode::Rollup,
            mem_kind: MemKind::Rss,
            sort_by: SortBy::Aggregated,
            sort_desc: true,
            update_ms,
            theme: Theme::detect(),
            mem_history: History::default(),
            graph_symbols: GraphSymbols::detect(),
            should_quit: false,
            last_tick_at: now,
            next_tick_at: next_tick_deadline(now, update_ms),
            last_mouse_position: None,
            expanded,
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            selected_pid: None,
            viewport_height: 1,
            table_body_rect: None,
            comm_col_x: 0,
            rendered_start: 0,
            rendered_toggle_zones: Vec::new(),
            last_click: None,
        };
        app.record_mem_sample();
        app.rebuild_rows();
        app
    }

    /// System-wide total of the selected memory kind across all processes.
    pub fn mem_total_used(&self) -> u64 {
        self.snapshot
            .procs
            .iter()
            .filter_map(|proc_rec| self.mem_kind.get(&proc_rec.mem))
            .fold(0_u64, u64::saturating_add)
    }

    /// Graph scale: total physical memory.
    pub fn mem_graph_max(&self) -> u64 {
        self.snapshot.total_mem
    }

    fn record_mem_sample(&mut self) {
        let used = self.mem_total_used();
        self.mem_history.push(used);
    }

    pub fn poll_timeout(&self, now: Instant) -> Duration {
        remaining_until_tick(now, self.next_tick_at)
    }

    pub fn should_tick(&self, now: Instant) -> bool {
        now >= self.next_tick_at
    }

    pub fn on_tick(&mut self) {
        let old_pids: HashSet<i32> = self
            .snapshot
            .procs
            .iter()
            .map(|proc_rec| proc_rec.pid)
            .collect();

        self.snapshot = collect::take_snapshot();
        self.tree = ProcTree::build(&self.snapshot);

        let new_pids: HashSet<i32> = self
            .snapshot
            .procs
            .iter()
            .map(|proc_rec| proc_rec.pid)
            .collect();
        self.expanded = remap_expanded_set(&self.expanded, &old_pids, &new_pids);

        self.record_mem_sample();
        self.rebuild_rows();
        self.last_tick_at = Instant::now();
        self.next_tick_at = next_tick_deadline(self.last_tick_at, self.update_ms);
    }

    pub fn on_key(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press {
            return;
        }

        match key_event.code {
            KeyCode::Char(ch) if is_quit_key(ch, key_event.modifiers) => {
                self.should_quit = true;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor_by(1),
            KeyCode::PageUp => self.move_cursor_by(-(self.page_step() as isize)),
            KeyCode::PageDown => self.move_cursor_by(self.page_step() as isize),
            KeyCode::Home | KeyCode::Char('g') => self.go_home(),
            KeyCode::End | KeyCode::Char('G') => self.go_end(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_selected_expand(),
            KeyCode::Left => self.collapse_or_select_parent(),
            KeyCode::Right => self.expand_selected(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.toggle_agg_mode(),
            KeyCode::Char('m') | KeyCode::Char('M') => self.cycle_mem_kind(),
            KeyCode::Char('s') => self.cycle_sort_key(),
            KeyCode::Char('S') | KeyCode::Char('r') | KeyCode::Char('R') => self.reverse_sort(),
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, mouse_event: MouseEvent) {
        self.last_mouse_position = Some((mouse_event.column, mouse_event.row));

        match mouse_event.kind {
            MouseEventKind::ScrollUp => self.move_cursor_by(-MOUSE_SCROLL_STEP),
            MouseEventKind::ScrollDown => self.move_cursor_by(MOUSE_SCROLL_STEP),
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_left_click(mouse_event.column, mouse_event.row)
            }
            _ => {}
        }
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height.max(1);
        self.apply_cursor_scroll_clamp();
    }

    pub fn set_table_hit_test(
        &mut self,
        body_rect: Rect,
        comm_col_x: u16,
        rendered_start: usize,
        toggle_zones: Vec<Option<Range<u16>>>,
    ) {
        self.table_body_rect = Some(body_rect);
        self.comm_col_x = comm_col_x;
        self.rendered_start = rendered_start;
        self.rendered_toggle_zones = toggle_zones;
    }

    pub fn clear_table_hit_test(&mut self) {
        self.table_body_rect = None;
        self.comm_col_x = 0;
        self.rendered_start = 0;
        self.rendered_toggle_zones.clear();
    }

    fn page_step(&self) -> usize {
        self.viewport_height.saturating_sub(1).max(1)
    }

    fn rebuild_rows(&mut self) {
        let aggregates = self.tree.aggregate(self.mem_kind, self.agg_mode);
        let visible = self.tree.flatten_visible_sorted(
            &self.expanded,
            &aggregates,
            self.sort_by,
            self.sort_desc,
        );

        self.rows = build_display_rows(&self.tree, &aggregates, &visible, &self.expanded);
        self.cursor = relocate_cursor_to_selected(self.selected_pid, &self.rows, self.cursor);
        self.apply_cursor_scroll_clamp();
        self.selected_pid = self.rows.get(self.cursor).map(|row| row.pid);
        self.rendered_toggle_zones.clear();
    }

    fn apply_cursor_scroll_clamp(&mut self) {
        let (cursor, scroll) = clamp_cursor_scroll(
            self.cursor,
            self.scroll,
            self.rows.len(),
            self.viewport_height,
        );
        self.cursor = cursor;
        self.scroll = scroll;
    }

    fn move_cursor_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.cursor = 0;
            self.scroll = 0;
            self.selected_pid = None;
            return;
        }

        let max = self.rows.len().saturating_sub(1) as isize;
        let next = (self.cursor as isize + delta).clamp(0, max) as usize;
        self.cursor = next;
        self.selected_pid = self.rows.get(self.cursor).map(|row| row.pid);
        self.apply_cursor_scroll_clamp();
    }

    fn go_home(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = 0;
        self.selected_pid = self.rows.first().map(|row| row.pid);
        self.apply_cursor_scroll_clamp();
    }

    fn go_end(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = self.rows.len().saturating_sub(1);
        self.selected_pid = self.rows.get(self.cursor).map(|row| row.pid);
        self.apply_cursor_scroll_clamp();
    }

    fn toggle_selected_expand(&mut self) {
        if let Some(pid) = self.selected_pid {
            self.toggle_expand_for_pid(pid);
        }
    }

    fn collapse_or_select_parent(&mut self) {
        let Some(pid) = self.selected_pid else {
            return;
        };

        let has_children = !self.tree.child_pids(pid).is_empty();
        if has_children && self.expanded.contains(&pid) {
            self.expanded.remove(&pid);
            self.rebuild_rows();
            return;
        }

        if let Some(parent_pid) = self.tree.parent_pid(pid) {
            self.select_pid(parent_pid);
        }
    }

    fn expand_selected(&mut self) {
        let Some(pid) = self.selected_pid else {
            return;
        };

        if self.tree.child_pids(pid).is_empty() || self.expanded.contains(&pid) {
            return;
        }

        self.expanded.insert(pid);
        self.rebuild_rows();
    }

    fn toggle_agg_mode(&mut self) {
        self.agg_mode = match self.agg_mode {
            AggMode::Rollup => AggMode::Flat,
            AggMode::Flat => AggMode::Rollup,
        };
        self.rebuild_rows();
    }

    fn cycle_mem_kind(&mut self) {
        let available = MemKind::available();
        if available.is_empty() {
            return;
        }

        let next = available
            .iter()
            .position(|kind| *kind == self.mem_kind)
            .map(|idx| (idx + 1) % available.len())
            .unwrap_or(0);

        self.mem_kind = available[next];
        self.mem_history.clear();
        self.record_mem_sample();
        self.rebuild_rows();
    }

    fn cycle_sort_key(&mut self) {
        let next = SORT_ORDER
            .iter()
            .position(|sort_by| *sort_by == self.sort_by)
            .map(|idx| (idx + 1) % SORT_ORDER.len())
            .unwrap_or(0);
        self.sort_by = SORT_ORDER[next];
        self.rebuild_rows();
    }

    fn reverse_sort(&mut self) {
        self.sort_desc = !self.sort_desc;
        self.rebuild_rows();
    }

    fn select_pid(&mut self, pid: i32) {
        if let Some(index) = self.rows.iter().position(|row| row.pid == pid) {
            self.cursor = index;
            self.selected_pid = Some(pid);
            self.apply_cursor_scroll_clamp();
        }
    }

    fn toggle_expand_for_pid(&mut self, pid: i32) {
        if self.tree.child_pids(pid).is_empty() {
            return;
        }

        if self.expanded.contains(&pid) {
            self.expanded.remove(&pid);
        } else {
            self.expanded.insert(pid);
        }
        self.rebuild_rows();
    }

    fn handle_left_click(&mut self, column: u16, row: u16) {
        let Some((row_index, local_row)) = self.hit_test_row(row) else {
            self.last_click = None;
            return;
        };

        self.cursor = row_index;
        self.selected_pid = self.rows.get(row_index).map(|entry| entry.pid);
        self.apply_cursor_scroll_clamp();

        let marker_hit = self.hit_toggle_zone(local_row, column);
        let now = Instant::now();
        let double_click = self
            .last_click
            .map(|(last_row, last_at)| {
                last_row == row_index
                    && now.saturating_duration_since(last_at)
                        <= Duration::from_millis(DOUBLE_CLICK_MS)
            })
            .unwrap_or(false);

        if marker_hit || double_click {
            if let Some(pid) = self.selected_pid {
                self.toggle_expand_for_pid(pid);
            }
            self.last_click = None;
            return;
        }

        self.last_click = Some((row_index, now));
    }

    fn hit_test_row(&self, row: u16) -> Option<(usize, usize)> {
        let rect = self.table_body_rect?;
        if row < rect.y || row >= rect.y.saturating_add(rect.height) {
            return None;
        }

        let local = usize::from(row.saturating_sub(rect.y));
        let global = self.rendered_start.saturating_add(local);
        if global >= self.rows.len() {
            return None;
        }

        Some((global, local))
    }

    fn hit_toggle_zone(&self, local_row: usize, column: u16) -> bool {
        if column < self.comm_col_x {
            return false;
        }

        let Some(range) = self
            .rendered_toggle_zones
            .get(local_row)
            .and_then(|zone| zone.as_ref())
        else {
            return false;
        };

        range.contains(&column)
    }
}

fn remap_expanded_set(
    previous_expanded: &HashSet<i32>,
    old_pids: &HashSet<i32>,
    new_pids: &HashSet<i32>,
) -> HashSet<i32> {
    let mut mapped = new_pids.clone();
    for pid in old_pids.intersection(new_pids) {
        if !previous_expanded.contains(pid) {
            mapped.remove(pid);
        }
    }
    mapped
}

fn build_display_rows(
    tree: &ProcTree,
    aggregates: &HashMap<i32, AggregateRow>,
    visible: &[VisibleRow],
    expanded: &HashSet<i32>,
) -> Vec<DisplayRow> {
    let mut rows = Vec::with_capacity(visible.len());
    let mut ancestor_last = Vec::new();

    for visible_row in visible {
        ancestor_last.truncate(visible_row.depth);

        let continuation_depth = visible_row.depth.saturating_sub(1);
        let (prefix, marker) = build_tree_prefix(
            visible_row.depth,
            visible_row.is_last_child,
            visible_row.has_children,
            expanded.contains(&visible_row.pid),
            &ancestor_last[..continuation_depth],
        );

        let aggregate = aggregates.get(&visible_row.pid);
        let proc_rec = tree.proc(visible_row.pid);
        rows.push(DisplayRow {
            pid: visible_row.pid,
            comm: proc_rec
                .map(|proc_rec| proc_rec.comm.clone())
                .unwrap_or_else(|| "?".to_owned()),
            own: aggregate.map(|row| row.own).unwrap_or_default(),
            own_approximate: aggregate.map(|row| row.own_approximate).unwrap_or(true),
            aggregated: aggregate.map(|row| row.aggregated).unwrap_or_default(),
            aggregated_approximate: aggregate
                .map(|row| row.aggregated_approximate)
                .unwrap_or(true),
            child_count: aggregate.map(|row| row.child_count).unwrap_or_default(),
            tree_prefix: prefix,
            toggle_marker: marker,
        });

        ancestor_last.push(visible_row.is_last_child);
    }

    rows
}

pub fn build_tree_prefix(
    depth: usize,
    is_last_child: bool,
    has_children: bool,
    expanded: bool,
    ancestor_is_last: &[bool],
) -> (String, Option<Range<usize>>) {
    let marker_text = if expanded { "[-]" } else { "[+]" };
    let mut prefix = String::new();

    if depth > 0 {
        for is_last in ancestor_is_last {
            prefix.push_str(if *is_last { "   " } else { "│  " });
        }
        prefix.push_str(if is_last_child { "└─ " } else { "├─ " });
    }

    let marker = if has_children {
        let start = prefix.chars().count();
        prefix.push_str(marker_text);
        let end = prefix.chars().count();
        prefix.push(' ');
        Some(start..end)
    } else {
        None
    };

    (prefix, marker)
}

pub fn clamp_cursor_scroll(
    cursor: usize,
    scroll: usize,
    row_count: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if row_count == 0 {
        return (0, 0);
    }

    let height = viewport_height.max(1);
    let max_cursor = row_count.saturating_sub(1);
    let mut clamped_cursor = cursor.min(max_cursor);
    let max_scroll = row_count.saturating_sub(height);
    let mut clamped_scroll = scroll.min(max_scroll);

    if clamped_cursor < clamped_scroll {
        clamped_scroll = clamped_cursor;
    }

    let bottom = clamped_scroll.saturating_add(height);
    if clamped_cursor >= bottom {
        clamped_scroll = clamped_cursor.saturating_add(1).saturating_sub(height);
    }

    clamped_scroll = clamped_scroll.min(max_scroll);
    clamped_cursor = clamped_cursor.min(max_cursor);
    (clamped_cursor, clamped_scroll)
}

fn relocate_cursor_to_selected(
    selected_pid: Option<i32>,
    rows: &[DisplayRow],
    fallback_cursor: usize,
) -> usize {
    if rows.is_empty() {
        return 0;
    }

    if let Some(pid) = selected_pid {
        if let Some(index) = rows.iter().position(|row| row.pid == pid) {
            return index;
        }
    }

    fallback_cursor.min(rows.len().saturating_sub(1))
}

fn is_quit_key(ch: char, modifiers: KeyModifiers) -> bool {
    let lowered = ch.to_ascii_lowercase();
    if modifiers.contains(KeyModifiers::CONTROL) {
        lowered == 'c' || lowered == 'q'
    } else {
        lowered == 'q'
    }
}

#[cfg(test)]
mod tests {
    use super::{build_tree_prefix, clamp_cursor_scroll, relocate_cursor_to_selected, DisplayRow};
    use std::ops::Range;

    fn row(pid: i32) -> DisplayRow {
        DisplayRow {
            pid,
            comm: format!("p{pid}"),
            own: 0,
            own_approximate: false,
            aggregated: 0,
            aggregated_approximate: false,
            child_count: 0,
            tree_prefix: String::new(),
            toggle_marker: None,
        }
    }

    #[test]
    fn tree_prefix_for_roots_and_children_matches_expected_glyphs() {
        let (root_prefix, root_marker) = build_tree_prefix(0, true, true, true, &[]);
        assert_eq!(root_prefix, "[-] ");
        assert_eq!(root_marker, Some(0..3));

        let (middle_child, middle_marker) = build_tree_prefix(1, false, true, false, &[]);
        assert_eq!(middle_child, "├─ [+] ");
        assert_eq!(middle_marker, Some(3..6));

        let (deep_leaf, deep_marker) = build_tree_prefix(3, true, false, false, &[false, true]);
        assert_eq!(deep_leaf, "│     └─ ");
        assert_eq!(deep_marker, None::<Range<usize>>);
    }

    #[test]
    fn clamp_cursor_scroll_keeps_cursor_visible() {
        assert_eq!(clamp_cursor_scroll(0, 0, 0, 10), (0, 0));
        assert_eq!(clamp_cursor_scroll(10, 0, 5, 3), (4, 2));
        assert_eq!(clamp_cursor_scroll(1, 5, 20, 4), (1, 1));
        assert_eq!(clamp_cursor_scroll(15, 2, 20, 5), (15, 11));
    }

    #[test]
    fn selection_relocation_prefers_pid_then_fallback_cursor() {
        let rows = vec![row(10), row(20), row(30), row(40)];
        assert_eq!(relocate_cursor_to_selected(Some(30), &rows, 0), 2);
        assert_eq!(relocate_cursor_to_selected(Some(999), &rows, 3), 3);
        assert_eq!(relocate_cursor_to_selected(None, &rows, 99), 3);
        assert_eq!(relocate_cursor_to_selected(Some(30), &[], 4), 0);
    }
}
